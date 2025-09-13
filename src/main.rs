#![deny(warnings)]

use crate::config::load_config;
use crate::middleware::registry::MiddlewareRegistry;
use crate::utils::{graceful_shutdown, shutdown_signal};
use arc_swap::ArcSwap;
use gateway_runtime::GatewayRuntime;
use std::env;
use std::sync::{Arc, LazyLock, OnceLock};
use std::time::Duration;
use tokio::task::JoinSet;
use tokio_rustls::TlsAcceptor;
use tokio_util::sync::CancellationToken;

mod config;

mod server;

mod service;

mod router;

mod api;

mod error;

mod utils;

mod logger;

mod middleware;

mod load_balancer;

mod gateway_runtime;

pub type SharedGatewayState = Arc<ArcSwap<GatewayRuntime>>;

static MIDDLEWARE_REGISTRY: LazyLock<MiddlewareRegistry> = LazyLock::new(MiddlewareRegistry::init);

static CONFIG_FILE_PATH: OnceLock<String> = OnceLock::new();

#[tokio::main]
async fn main() {
    let args = env::args().collect::<Vec<_>>();
    assert!(
        args.len() > 2,
        "Config file is required\nUsage: cargo run --config <config-file-path>"
    );

    if args[1] != "--config" {
        panic!("expected --config found {:?}", args[1]);
    }

    let _ = CONFIG_FILE_PATH.set(args[2].clone());

    let gateway_config = Arc::new(load_config().unwrap());

    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let _guard = logger::init_layers(&gateway_config.log, &gateway_config.access_log);

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

    let gateway_runtime = GatewayRuntime::new(gateway_config.clone());
    let gateway_state = SharedGatewayState::new(ArcSwap::from_pointee(gateway_runtime));

    let mut listener_joinset = JoinSet::new();
    for listener_cfg in &gateway_config.listeners {
        let cancel_token = cancel_token.clone();
        let listener_cfg = listener_cfg.clone();
        let tls_acceptor = tls_acceptor.clone();
        let http_client = http_client.clone();
        let gateway_state = gateway_state.clone();
        listener_joinset.spawn(async move {
            server::run_tcp_listener(
                listener_cfg,
                tls_acceptor.clone(),
                http_client,
                gateway_state,
                cancel_token,
            )
            .await
            .unwrap()
        });
    }

    tokio::select! {
        _ = listener_joinset.join_next() => {}
        _ = api::start_api_server(gateway_state.clone(), cancel_token.clone()) => {}
        _ = shutdown_signal() => {
            graceful_shutdown(cancel_token).await;
        }
    }
}
