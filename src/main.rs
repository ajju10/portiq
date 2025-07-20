use crate::config::{GatewayConfig, Protocol};
use crate::middleware::registry::MiddlewareRegistry;
use crate::middleware::{AccessLogger, HandlerFunc, Next, RequestBody, RequestID};
use http_body_util::{BodyExt, Empty, Full, combinators::BoxBody};
use hyper::{
    Method, Request, Response, StatusCode, body::Bytes, body::Incoming, service::service_fn,
};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::{env, fs, io};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

mod config;

mod logger;

mod middleware;

pub(crate) struct RouterContext {
    pub(crate) gateway_config: Arc<GatewayConfig>,
    pub(crate) middleware_registry: Arc<Mutex<MiddlewareRegistry>>,
    pub(crate) ip_addr: IpAddr,
}

impl RouterContext {
    pub(crate) fn new(
        gateway_config: Arc<GatewayConfig>,
        middleware_registry: Arc<Mutex<MiddlewareRegistry>>,
        ip_addr: IpAddr,
    ) -> Self {
        RouterContext {
            gateway_config,
            middleware_registry,
            ip_addr,
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

    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    logger::init_logger(&gateway_config.log, &gateway_config.access_log);

    let middleware_registry = Arc::new(Mutex::new(MiddlewareRegistry::new()));
    {
        let mut locked_registry = middleware_registry.lock().unwrap();
        locked_registry.register("request_id", RequestID);
        locked_registry.register("access_logger", AccessLogger);
    }

    let ip_addr = IpAddr::from_str(&gateway_config.server.host).expect("Host must be valid");
    let server_addr = SocketAddr::from((ip_addr, gateway_config.server.port));
    let listener = TcpListener::bind(server_addr).await.unwrap();

    match gateway_config.server.protocol {
        Protocol::Http => {
            tracing::info!("Starting server at http://{:?}", server_addr);
            start_http_server(listener, gateway_config, middleware_registry).await;
        }
        Protocol::Https => {
            tracing::info!("Starting server at https://{:?}", server_addr);
            start_https_server(listener, gateway_config, middleware_registry).await;
        }
    }
}

async fn start_http_server(
    listener: TcpListener,
    gateway_config: Arc<GatewayConfig>,
    middleware_registry: Arc<Mutex<MiddlewareRegistry>>,
) {
    loop {
        let (stream, addr) = listener.accept().await.unwrap();
        tracing::info!("Connected with client: {}", addr);

        let gateway_config = gateway_config.clone();
        let middleware_registry = middleware_registry.clone();
        let client_handler_service = service_fn(move |req| {
            let context = RouterContext::new(
                gateway_config.clone(),
                middleware_registry.clone(),
                addr.ip(),
            );
            handle_client(req, context)
        });

        tokio::spawn(async move {
            if let Err(err) = auto::Builder::new(TokioExecutor::new())
                .serve_connection(TokioIo::new(stream), client_handler_service)
                .await
            {
                tracing::error!("Error serving connection: {:?}", err);
            }
        });
    }
}

async fn start_https_server(
    listener: TcpListener,
    gateway_config: Arc<GatewayConfig>,
    middleware_registry: Arc<Mutex<MiddlewareRegistry>>,
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

    loop {
        let (stream, addr) = listener.accept().await.unwrap();
        let tls_acceptor = tls_acceptor.clone();

        let gateway_config = gateway_config.clone();
        let middleware_registry = middleware_registry.clone();
        let client_handler_service = service_fn(move |req| {
            let context = RouterContext::new(
                gateway_config.clone(),
                middleware_registry.clone(),
                addr.ip(),
            );
            handle_client(req, context)
        });

        tokio::spawn(async move {
            let tls_stream = match tls_acceptor.accept(stream).await {
                Ok(tls_stream) => tls_stream,
                Err(err) => {
                    tracing::error!("failed to perform tls handshake: {err:#}");
                    return;
                }
            };
            tracing::info!("Connected with client: {} over TLS", addr);

            if let Err(err) = auto::Builder::new(TokioExecutor::new())
                .serve_connection(TokioIo::new(tls_stream), client_handler_service)
                .await
            {
                tracing::error!("Error serving connection: {err:#}");
            }
        });
    }
}

fn bad_gateway_response() -> Response<BoxBody<Bytes, hyper::Error>> {
    let html_res = r#"<!DOCTYPE html>
        <html>
        <head>
        <title>502 Bad Gateway</title>
        </head>
        <body>
        <center><h1>502 Bad Gateway</h1></center>
        <hr><center>portiq</center>
        </body>
        </html>"#;

    let body = Full::new(Bytes::from_owner(html_res));
    let boxed_body = BoxBody::new(body).map_err(|never| match never {}).boxed();
    Response::builder()
        .status(StatusCode::BAD_GATEWAY)
        .header("Server", "portiq")
        .header("Content-Type", "text/html; charset=utf-8")
        .body(boxed_body)
        .expect("Failed to construct response")
}

fn response_with_status(status_code: StatusCode) -> Response<BoxBody<Bytes, hyper::Error>> {
    Response::builder()
        .status(status_code)
        .header("X-Proxy-Name", "portiq")
        .body(
            Empty::<Bytes>::new()
                .map_err(|never| match never {})
                .boxed(),
        )
        .unwrap()
}

// Load public certificate from file.
fn load_certs(filename: &str) -> io::Result<Vec<CertificateDer<'static>>> {
    // Open certificate file.
    let certfile =
        fs::File::open(filename).map_err(|e| error(format!("failed to open {filename}: {e}")))?;
    let mut reader = io::BufReader::new(certfile);

    // Load and return certificate.
    rustls_pemfile::certs(&mut reader).collect()
}

// Load private key from file.
fn load_private_key(filename: &str) -> io::Result<PrivateKeyDer<'static>> {
    // Open keyfile.
    let keyfile =
        fs::File::open(filename).map_err(|e| error(format!("failed to open {filename}: {e}")))?;
    let mut reader = io::BufReader::new(keyfile);

    // Load and return a single private key.
    rustls_pemfile::private_key(&mut reader).map(|key| key.unwrap())
}

fn error(err: String) -> io::Error {
    io::Error::other(err)
}

fn send_upstream(upstream_url: String) -> HandlerFunc {
    Arc::new(move |req: Request<RequestBody>| {
        let url = upstream_url.clone();
        let req_client = reqwest::Client::new();
        Box::pin(async move {
            match req.method() {
                &Method::GET => {
                    let mut request = req_client.get(url);
                    for (key, value) in req.headers() {
                        request = request.header(key, value);
                    }
                    match request.send().await {
                        Ok(resp) => {
                            let resp = resp.bytes().await.unwrap();
                            let body = Full::from(resp);
                            let response = Response::new(
                                BoxBody::new(body).map_err(|never| match never {}).boxed(),
                            );
                            Ok(response)
                        }
                        Err(_) => Ok(bad_gateway_response()),
                    }
                }
                _ => {
                    println!("Unsupported method: {}", req.method().as_str());
                    Ok(bad_gateway_response())
                }
            }
        })
    })
}

async fn handle_client(
    request: Request<Incoming>,
    context: RouterContext,
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, Infallible> {
    let original_request = request;
    let original_path = original_request.uri().path();
    let original_method = original_request.method();

    let route_match_result = context
        .gateway_config
        .match_upstream_path(original_path, original_method.as_str());
    if let Err(status_code) = route_match_result {
        return Ok(response_with_status(status_code));
    }

    let upstream_url = route_match_result.unwrap();
    let proxy_uri_str = format!("{}{}", upstream_url, original_request.uri().path());

    let middlewares = {
        let locked_registry = context.middleware_registry.lock().unwrap();
        let global_middlewares = ["request_id".to_string(), "access_logger".to_string()];
        locked_registry.create_chain(&global_middlewares)
    };

    let handler = send_upstream(proxy_uri_str).clone();
    let next = Next::new(handler, &middlewares);
    let (parts, body) = original_request.into_parts();
    let request = Request::from_parts(parts, RequestBody::new(body));
    next.run(request).await
}
