#![deny(warnings)]

use crate::middleware::registry::MiddlewareRegistry;
use crate::middleware::{HandlerFunc, RequestBody};
use crate::router::Router;
use crate::service::ServiceRegistry;
use crate::utils::bad_gateway_response;
use futures::future::TryJoinAll;
use http_body_util::{BodyExt, Full, combinators::BoxBody};
use hyper::{HeaderMap, Method, Request, Response};
use reqwest::RequestBuilder;
use std::env;
use std::net::IpAddr;
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use tokio::signal::unix::{SignalKind, signal};
use tokio_rustls::TlsAcceptor;
use tokio_util::sync::CancellationToken;

mod config;

mod server;

mod service;

mod router;

mod error;

mod utils;

mod logger;

mod middleware;

mod load_balancer;

static MIDDLEWARE_REGISTRY: LazyLock<MiddlewareRegistry> = LazyLock::new(MiddlewareRegistry::init);

async fn graceful_shutdown(cancel_token: CancellationToken) {
    cancel_token.cancel();
    tracing::info!("Initiating shutdown, application will exit after 5 seconds");
    tokio::time::sleep(Duration::from_secs(5)).await;
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

#[tokio::main]
async fn main() {
    let args = env::args().collect::<Vec<_>>();
    assert!(
        args.len() > 1,
        "Config file is required\nUsage: cargo run <config-file-path>"
    );

    let gateway_config = Arc::new(config::load_config(&args[1]).unwrap());

    let svc_registry = Arc::new(ServiceRegistry::init(gateway_config.clone()));

    let router = Arc::new(Router::new(gateway_config.clone(), svc_registry));

    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let _ = logger::init_layers(&gateway_config.log, &gateway_config.access_log);

    let tls_acceptor = gateway_config.tls.as_ref().map(|tls_config| {
        let rustls_server_config = server::init_rustls_server_config(tls_config);
        TlsAcceptor::from(rustls_server_config)
    });

    let http_client = Arc::new(
        reqwest::Client::builder()
            .use_rustls_tls()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Invalid tls config"),
    );

    let cancel_token = CancellationToken::new();

    let mut handles = Vec::with_capacity(gateway_config.listeners.len());
    for listener_cfg in &gateway_config.listeners {
        let cancel_token = cancel_token.clone();
        let listener_cfg = listener_cfg.clone();
        let tls_acceptor = tls_acceptor.clone();
        let router = router.clone();
        let http_client = http_client.clone();
        let cfg = gateway_config.clone();
        let handle = tokio::spawn(async move {
            server::run_tcp_listener(
                listener_cfg,
                tls_acceptor.clone(),
                router,
                http_client,
                cfg,
                cancel_token,
            )
            .await
            .unwrap()
        });
        handles.push(handle);
    }

    let joined_handles = handles.into_iter().collect::<TryJoinAll<_>>();
    tokio::select! {
        _ = joined_handles => {
            graceful_shutdown(cancel_token).await;
        }
        _ = shutdown_signal() => {
            graceful_shutdown(cancel_token).await;
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
