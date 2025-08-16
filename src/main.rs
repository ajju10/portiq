use crate::config::{GatewayConfig, Protocol};
use crate::error::RouterError;
use crate::middleware::registry::MiddlewareRegistry;
use crate::middleware::{HandlerFunc, Next, RequestBody};
use crate::router::Router;
use crate::utils::{bad_gateway_response, load_certs, load_private_key, response_with_status};
use http_body_util::{BodyExt, Full, combinators::BoxBody};
use hyper::body::{Bytes, Incoming};
use hyper::service::service_fn;
use hyper::{HeaderMap, Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto;
use reqwest::RequestBuilder;
use std::convert::Infallible;
use std::env;
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpListener;
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::watch::{Receiver, Sender};
use tokio_rustls::TlsAcceptor;

mod config;

mod router;

mod error;

mod utils;

mod logger;

mod middleware;

mod load_balancer;

pub struct RouterContext {
    middleware_registry: Arc<MiddlewareRegistry>,
    router: Arc<Router>,
    ip_addr: IpAddr,
    http_client: Arc<reqwest::Client>,
}

impl RouterContext {
    fn new(
        middleware_registry: Arc<MiddlewareRegistry>,
        router: Arc<Router>,
        ip_addr: IpAddr,
        http_client: Arc<reqwest::Client>,
    ) -> Self {
        RouterContext {
            middleware_registry,
            router,
            ip_addr,
            http_client,
        }
    }
}

#[tokio::main]
async fn main() {
    let args = env::args().collect::<Vec<_>>();
    assert!(
        args.len() > 1,
        "Config file is required\nUsage: cargo run <config-file-path>"
    );

    let gateway_config = Arc::new(config::load_config(&args[1]));

    let router = Arc::new(Router::new(gateway_config.routes.clone()));

    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    logger::init_logger(&gateway_config.log, &gateway_config.access_log);

    let http_client = reqwest::Client::builder()
        .use_rustls_tls()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("Invalid tls config");

    let middleware_registry = Arc::new(MiddlewareRegistry::init());

    let ip_addr = IpAddr::from_str(&gateway_config.server.host).expect("Host must be valid");
    let server_addr = SocketAddr::from((ip_addr, gateway_config.server.port));
    let listener = TcpListener::bind(server_addr).await.unwrap();

    match gateway_config.server.protocol {
        Protocol::Http => {
            tracing::info!("Starting server at http://{server_addr}");
            start_http_server(listener, middleware_registry, router, Arc::new(http_client)).await;
        }
        Protocol::Https => {
            tracing::info!("Starting server at https://{server_addr}");
            start_https_server(
                listener,
                gateway_config,
                middleware_registry,
                router,
                Arc::new(http_client),
            )
            .await;
        }
    }
}

async fn serve_incoming_connection<S>(
    stream: S,
    middleware_registry: Arc<MiddlewareRegistry>,
    router: Arc<Router>,
    addr: SocketAddr,
    http_client: Arc<reqwest::Client>,
    mut shutdown_rx: Receiver<bool>,
) where
    S: AsyncRead + AsyncWrite + Unpin + 'static,
{
    let service = service_fn(move |req| {
        let context = RouterContext::new(
            middleware_registry.clone(),
            router.clone(),
            addr.ip(),
            http_client.clone(),
        );
        handle_client(req, context)
    });

    let builder = auto::Builder::new(TokioExecutor::new());
    let conn = builder.serve_connection(TokioIo::new(stream), service);
    tokio::pin!(conn);

    tokio::select! {
        res = conn.as_mut() => {
            if let Err(err) = res {
                tracing::error!("Error serving connection: {err}");
            }
        }
        _ = shutdown_rx.changed() => {
            conn.as_mut().graceful_shutdown();
        }
    }
}

async fn start_http_server(
    listener: TcpListener,
    middleware_registry: Arc<MiddlewareRegistry>,
    router: Arc<Router>,
    http_client: Arc<reqwest::Client>,
) {
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    loop {
        tokio::select! {
            Ok((stream, addr)) = listener.accept(), if !*shutdown_rx.borrow() => {
                tracing::info!("Connected with client {addr} over http");

                let shutdown_rx = shutdown_rx.clone();
                let middleware_registry = middleware_registry.clone();
                let router = router.clone();
                let http_client = http_client.clone();
                tokio::spawn(async move {
                    serve_incoming_connection(
                        stream,
                        middleware_registry,
                        router,
                        addr,
                        http_client,
                        shutdown_rx,
                    )
                    .await;
                });
            },
            _ = shutdown_signal() => {
                initiate_shutdown(shutdown_tx).await;
                break;
            }
        }
    }
}

async fn start_https_server(
    listener: TcpListener,
    gateway_config: Arc<GatewayConfig>,
    middleware_registry: Arc<MiddlewareRegistry>,
    router: Arc<Router>,
    http_client: Arc<reqwest::Client>,
) {
    let cert_file = gateway_config.server.cert_file.as_ref().unwrap_or_else(|| {
        tracing::error!("Certificate file is required for https");
        panic!("Certificate file is not provided for https protocol");
    });

    let key_file = gateway_config.server.key_file.as_ref().unwrap_or_else(|| {
        tracing::error!("Key file is required for https");
        panic!("Key file is not provided for https protocol");
    });

    let certs = load_certs(cert_file).expect("Failed to load certificate");
    let key = load_private_key(key_file).expect("Failed to load private key");

    let mut server_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .expect("Certificate and key must be valid");
    server_config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    let tls_acceptor = TlsAcceptor::from(Arc::new(server_config));

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    loop {
        tokio::select! {
            Ok((stream, addr)) = listener.accept(), if !*shutdown_rx.borrow() => {
                let tls_acceptor = tls_acceptor.clone();
                let middleware_registry = middleware_registry.clone();
                let router = router.clone();
                let http_client = http_client.clone();
                let shutdown_rx = shutdown_rx.clone();

                tokio::spawn(async move {
                    let tls_stream = match tls_acceptor.accept(stream).await {
                        Ok(tls_stream) => tls_stream,
                        Err(err) => {
                            tracing::error!("Failed to perform tls handshake: {err}");
                            return;
                        }
                    };
                    tracing::info!("Connected with client {addr} over https");

                    serve_incoming_connection(
                        tls_stream,
                        middleware_registry,
                        router,
                        addr,
                        http_client,
                        shutdown_rx,
                    )
                    .await;
                });
            }
            _ = shutdown_signal() => {
                initiate_shutdown(shutdown_tx).await;
                break;
            }
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

fn set_proxy_headers(
    client_ip: IpAddr,
    host: &str,
    proto: &str,
    mut builder: RequestBuilder,
    original_headers: &HeaderMap,
) -> RequestBuilder {
    if let Some(val) = original_headers.get("x-forwarded-for") {
        builder = builder.header(
            "x-forwarded-for",
            format!("{},{}", val.to_str().unwrap(), client_ip),
        );
    } else {
        builder = builder.header("x-forwarded-for", client_ip.to_string())
    }

    if !original_headers.contains_key("x-forwarded-host") {
        builder = builder.header("x-forwarded-host", host)
    }

    if !original_headers.contains_key("x-forwarded-proto") {
        builder = builder.header("x-forwarded-proto", proto)
    }

    builder
}

async fn handle_client(
    request: Request<Incoming>,
    context: RouterContext,
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, Infallible> {
    let original_request = request;
    let original_path = original_request.uri().path();
    let original_method = original_request.method();

    let router = context.router;
    match router.get_route(original_path, original_method.as_str()) {
        Ok(route) => {
            if let Ok(upstream) = route.get_upstream() {
                let route_middlewares = route.get_middlewares();
                let middlewares = context.middleware_registry.create_chain(route_middlewares);
                let handler =
                    send_upstream(upstream.url.clone(), context.ip_addr, context.http_client)
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
                RouterError::MethodNotAllowed => tracing::warn!(
                    "Router error: Method {original_method} not allowed for path {original_path}"
                ),
                RouterError::NoUpstream => {
                    unreachable!("get_route should never return NoUpstream error")
                }
            }
            Ok(response_with_status(err.status_code()))
        }
    }
}

async fn shutdown_signal() {
    let mut sigint = signal(SignalKind::interrupt()).expect("Failed to install SIGINT");
    let mut sigterm = signal(SignalKind::terminate()).expect("Failed to install SIGTERM");
    tokio::select! {
        _ = sigint.recv() => {
            tracing::info!("Received SIGINT");
        }
        _ = sigterm.recv() => {
            tracing::info!("Received SIGTERM");
        }
    }
}

async fn initiate_shutdown(shutdown_tx: Sender<bool>) {
    tracing::info!("Shutdown initiated. No longer accepting connections");
    tracing::info!("Server will stop after 5 seconds");
    shutdown_tx.send(true).unwrap();
    tokio::time::sleep(Duration::from_secs(5)).await;
}
