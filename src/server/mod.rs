use crate::SharedGatewayState;
use crate::config::{Listener, Protocol};
use crate::server::http::{handle_https, serve_http_connection};
use crate::server::tcp::handle_tcp_client;
use std::io;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tokio_util::sync::CancellationToken;

pub use tls::init_rustls_server_config;

mod tls;

mod http;

mod tcp;

pub async fn run_tcp_listener(
    listener_cfg: Listener,
    tls_acceptor: Option<TlsAcceptor>,
    http_client: Arc<reqwest::Client>,
    gateway_state: SharedGatewayState,
    cancel_token: CancellationToken,
) -> io::Result<()> {
    let listener = TcpListener::bind(listener_cfg.addr).await?;
    match listener_cfg.protocol {
        Protocol::Http => tracing::info!(
            "Listener `{}` is running on http://{}",
            listener_cfg.name,
            listener_cfg.addr
        ),
        Protocol::Https => tracing::info!(
            "Listener `{}` is running on https://{}",
            listener_cfg.name,
            listener_cfg.addr
        ),
        _ => tracing::info!(
            "Listener `{}` is running on {}/tcp",
            listener_cfg.name,
            listener_cfg.addr
        ),
    }

    loop {
        tokio::select! {
            maybe_conn = listener.accept() => {
                match maybe_conn {
                    Ok((stream, client_addr)) => {
                        let protocol = listener_cfg.protocol.clone();
                        let listener_name = listener_cfg.name.clone();
                        let tls_acceptor = tls_acceptor.clone();
                        let http_client = http_client.clone();
                        let gateway_state = gateway_state.clone();
                        tokio::spawn(async move {
                            match protocol {
                                Protocol::Http => {
                                    serve_http_connection(
                                        stream,
                                        client_addr,
                                        listener_name,
                                        http_client,
                                        gateway_state
                                    ).await;
                                },
                                Protocol::Https => {
                                    match tls_acceptor {
                                        Some(tls_acceptor) => {
                                            handle_https(
                                                stream,
                                                client_addr,
                                                tls_acceptor,
                                                listener_name,
                                                http_client,
                                                gateway_state
                                            ).await
                                        }
                                        None => panic!("Https requires a valid TLS configuration"),
                                    }
                                },
                                _ => { // Raw TCP
                                    if let Err(err) = handle_tcp_client(
                                        stream,
                                        listener_name,
                                        client_addr,
                                        tls_acceptor,
                                        gateway_state,
                                    ).await {
                                        tracing::error!("Error while handling the client {client_addr}: {err}")
                                    }
                                },
                            }
                        });
                    },
                    Err(err) => tracing::error!("Connection attempt failed {err:?}"),
                }
            }

            _ = cancel_token.cancelled() => {
                tracing::info!("Shutdown received on listener `{}`", listener_cfg.name);
                break;
            }
        }
    }

    Ok(())
}
