use crate::config::{GatewayConfig, Protocol};
use crate::middleware::{AccessLogger, RequestID};
use crate::service::HandlerService;

use http_body_util::{BodyExt, Empty, Full, combinators::BoxBody};
use hyper::client::conn::http1 as http1_client;
use hyper::server::conn::http1 as http1_server;
use hyper::{
    Request, Response, StatusCode, Uri, body::Bytes, body::Incoming, header::HeaderValue,
    service::service_fn,
};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use std::sync::Arc;
use std::{env, fs, io};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;
use tower::ServiceBuilder;

mod config;

mod logger;

mod middleware;

mod service;

#[tokio::main]
async fn main() {
    let args = env::args().collect::<Vec<_>>();
    assert!(
        args.len() > 1,
        "Config file is required\nUsage: cargo run <config-file-path>"
    );

    let gateway_config = Arc::new(config::load_config(&args[1]));

    logger::init_logger(&gateway_config.log, &gateway_config.access_log);

    let ip_addr = IpAddr::from_str(&gateway_config.server.host).expect("Host must be valid");
    let server_addr = SocketAddr::from((ip_addr, gateway_config.server.port));
    let listener = TcpListener::bind(server_addr).await.unwrap();

    match gateway_config.server.protocol {
        Protocol::Http => {
            tracing::info!("Starting server at http://{:?}", server_addr);
            start_http_server(listener, gateway_config).await;
        }
        Protocol::Https => {
            tracing::info!("Starting server at https://{:?}", server_addr);
            let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

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

            start_https_server(listener, gateway_config, certs, key).await;
        }
    }
}

async fn start_http_server(listener: TcpListener, gateway_config: Arc<GatewayConfig>) {
    loop {
        let (stream, addr) = listener.accept().await.unwrap();
        tracing::info!("Connected with client: {}", addr);

        let aio = TokioIo::new(stream);

        let gateway_config = gateway_config.clone();
        tokio::spawn(async move {
            let client_handler_service =
                service_fn(move |req| handle_client(req, gateway_config.clone(), addr.ip()));
            let base_service = ServiceBuilder::new()
                .layer_fn(RequestID::new)
                .layer_fn(AccessLogger::new)
                .service(client_handler_service);
            let handler_service = HandlerService::new(base_service, addr.ip());

            if let Err(err) = http1_server::Builder::new()
                .serve_connection(aio, handler_service)
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
    certs: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
) {
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
        let client_handler_service =
            service_fn(move |req| handle_client(req, gateway_config.clone(), addr.ip()));
        let base_service = ServiceBuilder::new()
            .layer_fn(RequestID::new)
            .layer_fn(AccessLogger::new)
            .service(client_handler_service);
        let handler_service = HandlerService::new(base_service, addr.ip());

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
                .serve_connection(TokioIo::new(tls_stream), handler_service)
                .await
            {
                tracing::error!("Error serving connection: {err:#}");
            }
        });
    }
}

async fn handle_client(
    request: Request<Incoming>,
    gateway_config: Arc<GatewayConfig>,
    client_ip: IpAddr,
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, Infallible> {
    let original_request = request;
    let original_path = original_request.uri().path();
    let original_method = original_request.method();

    match gateway_config.match_upstream_path(original_path, original_method.as_str()) {
        Ok(upstream_url) => {
            let proxy_uri_str = format!("{}{}", upstream_url, original_request.uri().path());
            let proxy_uri: Uri = match proxy_uri_str.parse() {
                Ok(uri) => uri,
                Err(_) => return Ok(bad_gateway_response()),
            };

            let proxy_host = match proxy_uri.host() {
                Some(host) => host,
                None => return Ok(bad_gateway_response()),
            };

            let proxy_port = match proxy_uri.port_u16() {
                Some(port) => port,
                None => return Ok(bad_gateway_response()),
            };

            let proxy_addr = format!("{proxy_host}:{proxy_port}");
            let stream = match TcpStream::connect(proxy_addr).await {
                Ok(s) => s,
                Err(_) => return Ok(bad_gateway_response()),
            };

            let aio = TokioIo::new(stream);
            let (mut sender, conn) = match http1_client::handshake(aio).await {
                Ok(result) => result,
                Err(_) => return Ok(bad_gateway_response()),
            };

            tokio::task::spawn(async move {
                if let Err(err) = conn.await {
                    tracing::warn!("Connection failed: {:?}", err);
                }
            });

            let mut request_builder = Request::builder()
                .version(original_request.version())
                .uri(proxy_uri.path())
                .method(original_request.method());

            for (key, value) in original_request.headers() {
                request_builder = request_builder.header(key, value);
            }

            // Set X-Forwarded-For header
            if let Ok(ip) = HeaderValue::from_str(&client_ip.to_string()) {
                request_builder = request_builder.header("X-Forwarded-For", ip);
            }

            let proxy_req = match request_builder.body(original_request.into_body()) {
                Ok(req) => req,
                Err(_) => return Ok(bad_gateway_response()),
            };

            match sender.send_request(proxy_req).await {
                Ok(proxy_res) => {
                    let mut response_builder = Response::builder().status(proxy_res.status());
                    for (key, value) in proxy_res.headers() {
                        println!("{key}: {value:#?}");
                        if key != "content-length" || key != "server" {
                            response_builder = response_builder.header(key, value);
                        }
                    }
                    // Add server header
                    response_builder = response_builder.header("server", "portiq");

                    let response_body = proxy_res.map(|b| b.boxed());
                    let res = response_builder.body(response_body.into_body()).unwrap();
                    Ok(res)
                }
                Err(_) => Ok(bad_gateway_response()),
            }
        }
        Err(status_code) => Ok(response_with_status(status_code)),
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
