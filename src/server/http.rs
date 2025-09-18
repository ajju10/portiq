use crate::error::RouterError;
use crate::middleware::{HandlerFunc, Next, RequestBody};
use crate::router::RouterContext;
use crate::utils::{bad_gateway_response, response_with_status, set_proxy_headers};
use crate::{MIDDLEWARE_REGISTRY, SharedGatewayState};
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto;
use reqwest::Method;
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio_rustls::TlsAcceptor;

pub(crate) async fn handle_https(
    stream: TcpStream,
    client_addr: SocketAddr,
    tls_acceptor: TlsAcceptor,
    listener_name: String,
    http_client: Arc<reqwest::Client>,
    gateway_state: SharedGatewayState,
) {
    let tls_stream = match tls_acceptor.accept(stream).await {
        Ok(tls_stream) => tls_stream,
        Err(err) => {
            tracing::error!("Failed to perform tls handshake: {err}");
            return;
        }
    };

    tracing::info!("Connected with client {client_addr} over https");
    serve_http_connection(
        tls_stream,
        client_addr,
        listener_name,
        http_client,
        gateway_state,
    )
    .await;
}

pub(crate) async fn serve_http_connection<S>(
    stream: S,
    addr: SocketAddr,
    listener: String,
    http_client: Arc<reqwest::Client>,
    gateway_state: SharedGatewayState,
) where
    S: AsyncRead + AsyncWrite + Unpin + 'static,
{
    let service = service_fn(move |req| {
        let context = RouterContext::new(
            addr.ip(),
            listener.clone(),
            http_client.clone(),
            gateway_state.clone(),
        );
        handle_client(req, context)
    });

    if let Err(err) = auto::Builder::new(TokioExecutor::new())
        .serve_connection(TokioIo::new(stream), service)
        .await
    {
        tracing::error!("Error serving http request: {err}");
    }
}

async fn handle_client(
    request: Request<Incoming>,
    context: RouterContext,
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, Infallible> {
    let original_request = request;
    // Extract host from header for http/1.1 requests
    let original_host = if let Some(host) = original_request
        .headers()
        .get(hyper::header::HOST)
        .and_then(|h| h.to_str().ok())
    {
        host
    } else {
        // Get from uri for http2
        original_request.uri().host().unwrap()
    };
    let original_path = original_request.uri().path();

    let gateway_state = context.gateway_state.load();
    let current_config = gateway_state.get_last_applied_config();
    let router = gateway_state.get_router();
    match router.get_http_route(original_host, original_path, &context.listener) {
        Ok(route) => {
            let service_name = route.get_service();
            if let Ok(upstream) = router.get_http_upstream(&service_name) {
                let named_middlewares = route.get_middlewares();
                let mut route_middlewares = Vec::new();
                let middleware_configs = &current_config.http.middlewares;
                for name in named_middlewares {
                    let cfg = middleware_configs.get(name).unwrap();
                    route_middlewares.push(cfg);
                }
                let middlewares = MIDDLEWARE_REGISTRY.create_chain(&route_middlewares);

                let handler = send_upstream(
                    upstream.target.clone(),
                    context.ip_addr,
                    context.http_client,
                )
                .clone();

                let next = Next::new(handler, &middlewares);
                let (parts, body) = original_request.into_parts();
                let request = Request::from_parts(parts, RequestBody::new(body));
                next.run(request).await
            } else {
                tracing::warn!(
                    "Router error: No upstream available to handle request for path {original_path}"
                );
                Ok(response_with_status(StatusCode::SERVICE_UNAVAILABLE))
            }
        }
        Err(err) => {
            match err {
                RouterError::NotFound => {
                    tracing::warn!("Router error: Route not found for path {original_path}")
                }
                _ => {
                    tracing::error!("This match arm should never run for `router.get_route(...)`");
                    unreachable!("This match arm should never run for `router.get_route(...)`")
                }
            }
            Ok(response_with_status(err.status_code()))
        }
    }
}

fn send_upstream(
    upstream_url: String,
    client_ip: IpAddr,
    http_client: Arc<reqwest::Client>,
) -> HandlerFunc {
    Arc::new(move |req: Request<RequestBody>| {
        let url = format!(
            "{upstream_url}{}",
            req.uri().path_and_query().unwrap().as_str()
        );

        let host = if let Some(val) = req.headers().get("host") {
            String::from(val.to_str().unwrap())
        } else {
            req.uri().authority().map(|a| a.to_string()).unwrap()
        };
        let proto = if req.uri().scheme_str() == Some("https") {
            "https"
        } else {
            "http"
        };

        let mut request_builder = http_client.request(req.method().clone(), url);
        request_builder =
            set_proxy_headers(client_ip, &host, proto, request_builder, req.headers());

        Box::pin(async move {
            if matches!(req.method(), &Method::POST | &Method::PUT | &Method::PATCH) {
                let body = req.into_body();
                let collected = body.collect().await.unwrap();
                request_builder = request_builder.body(collected.to_bytes());
            }

            match request_builder.send().await {
                Ok(resp) => {
                    let mut response_builder = Response::builder().status(resp.status());
                    for (key, value) in resp.headers() {
                        if key != "server" {
                            response_builder = response_builder.header(key, value);
                        } else {
                            response_builder = response_builder.header("Server", "portiq");
                        }
                    }
                    let resp_bytes = resp.bytes().await.unwrap();
                    let body = Full::from(resp_bytes);
                    let response = response_builder
                        .body(BoxBody::new(body).map_err(|never| match never {}).boxed())
                        .unwrap();
                    Ok(response)
                }
                Err(err) => {
                    tracing::error!("Error sending request to upstream: {err:?}");
                    Ok(bad_gateway_response())
                }
            }
        })
    })
}
