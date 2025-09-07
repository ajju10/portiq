use crate::config::{GatewayConfig, Listener, Protocol, TLSConfig};
use crate::error::RouterError;
use crate::middleware::{Next, RequestBody};
use crate::router::{Router, RouterContext};
use crate::utils::{load_certs, load_private_key, response_with_status};
use crate::{MIDDLEWARE_REGISTRY, send_upstream};
use http_body_util::combinators::BoxBody;
use hyper::body::{Bytes, Incoming};
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto;
use rustls::crypto::aws_lc_rs::sign::any_supported_type;
use rustls::server::{ClientHello, ResolvesServerCert, ResolvesServerCertUsingSni};
use rustls::sign::CertifiedKey;
use std::convert::Infallible;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;
use tokio_util::sync::CancellationToken;

#[derive(Debug)]
pub struct SNICertificateResolver {
    default: Arc<CertifiedKey>,
    sni: ResolvesServerCertUsingSni,
}

impl SNICertificateResolver {
    pub fn new(cert_file: &str, key_file: &str) -> Self {
        let certs = load_certs(cert_file).unwrap();
        let private_key = load_private_key(key_file).unwrap();
        let signing_key = any_supported_type(&private_key).unwrap();
        SNICertificateResolver {
            default: Arc::new(CertifiedKey::new(certs, signing_key)),
            sni: ResolvesServerCertUsingSni::new(),
        }
    }

    pub fn add_sni_cert(
        &mut self,
        hostname: &str,
        cert_file: &str,
        key_file: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let certs = load_certs(cert_file)?;
        let private_key = load_private_key(key_file)?;
        let signing_key = any_supported_type(&private_key)?;
        self.sni
            .add(hostname, CertifiedKey::new(certs, signing_key))?;
        Ok(())
    }
}

impl ResolvesServerCert for SNICertificateResolver {
    fn resolve(&self, client_hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        self.sni
            .resolve(client_hello)
            .or_else(|| Some(self.default.clone()))
    }
}

pub fn init_rustls_server_config(tls_configs: &[TLSConfig]) -> Arc<rustls::ServerConfig> {
    let default_cfg = tls_configs
        .iter()
        .find(|&cfg| cfg.default)
        .expect("A default config is required for TLS");

    let mut resolver = SNICertificateResolver::new(
        default_cfg.cert_file.to_str().unwrap(),
        default_cfg.key_file.to_str().unwrap(),
    );

    for tls_config in tls_configs {
        if let Some(hosts) = &tls_config.hostnames {
            let cert_file = tls_config.cert_file.to_str().unwrap();
            let key_file = tls_config.key_file.to_str().unwrap();
            for host in hosts {
                resolver
                    .add_sni_cert(host, cert_file, key_file)
                    .unwrap_or_else(|_| {
                        panic!("The certificate should be valid for hostname `{host}`")
                    });
            }
        }
    }

    let mut server_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(resolver));

    server_config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Arc::new(server_config)
}

pub async fn run_tcp_listener(
    listener_cfg: Listener,
    tls_acceptor: Option<TlsAcceptor>,
    router: Arc<Router>,
    http_client: Arc<reqwest::Client>,
    gateway_config: Arc<GatewayConfig>,
    cancel_token: CancellationToken,
) -> io::Result<()> {
    let listener = TcpListener::bind(listener_cfg.addr).await?;
    tracing::info!("Listening on {}", listener_cfg.addr);

    loop {
        tokio::select! {
            Ok((stream, client_addr)) = listener.accept() => {
                let protocol = listener_cfg.protocol.clone();
                let listener_name = listener_cfg.name.clone();
                let tls_acceptor = tls_acceptor.clone();
                let router = router.clone();
                let http_client = http_client.clone();
                let gateway_config = gateway_config.clone();
                tokio::spawn(async move {
                    match protocol {
                        Protocol::Http => serve_http_connection(stream, router, client_addr, listener_name, http_client, gateway_config).await,
                        Protocol::Https => {
                            match tls_acceptor {
                                Some(tls_acceptor) => handle_https(stream, router, client_addr, tls_acceptor, listener_name, http_client, gateway_config).await,
                                None => panic!("Https requires a valid TLS configuration"),
                            }
                        }
                    }
                });
            }

            _ = cancel_token.cancelled() => {
                tracing::info!("Shutdown received on {}", listener_cfg.addr);
                break;
            }
        }
    }

    Ok(())
}

async fn handle_https(
    stream: TcpStream,
    router: Arc<Router>,
    client_addr: SocketAddr,
    tls_acceptor: TlsAcceptor,
    listener_name: String,
    http_client: Arc<reqwest::Client>,
    gateway_config: Arc<GatewayConfig>,
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
        router,
        client_addr,
        listener_name,
        http_client,
        gateway_config,
    )
    .await;
}

async fn serve_http_connection<S>(
    stream: S,
    router: Arc<Router>,
    addr: SocketAddr,
    listener: String,
    http_client: Arc<reqwest::Client>,
    gateway_config: Arc<GatewayConfig>,
) where
    S: AsyncRead + AsyncWrite + Unpin + 'static,
{
    let service = service_fn(move |req| {
        let context = RouterContext::new(
            router.clone(),
            addr.ip(),
            listener.clone(),
            http_client.clone(),
            gateway_config.clone(),
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

    let router = context.router;
    let middleware_configs = &context.gateway_config.http.middlewares;
    match router.get_route(original_host, original_path, &context.listener) {
        Ok(route) => {
            let service_name = route.get_service();
            if let Ok(upstream) = router.get_service(&service_name) {
                let named_middlewares = route.get_middlewares();
                let mut route_middlewares = Vec::new();
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
