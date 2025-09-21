use crate::SharedGatewayState;
use crate::config::TcpTlsMode;
use std::io;
use std::net::SocketAddr;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio_rustls::TlsAcceptor;

pub(crate) async fn handle_tcp_client(
    stream: TcpStream,
    listener: String,
    client_addr: SocketAddr,
    tls_acceptor: Option<TlsAcceptor>,
    gateway_state: SharedGatewayState,
) -> io::Result<()> {
    tracing::info!("Connected with client {client_addr}");

    let router = gateway_state.load().get_router();
    match router.get_tcp_route(&listener) {
        Ok(route) => {
            let service = route.get_service();
            if let Ok(upstream) = router.get_tcp_upstream(service) {
                match route.get_tls_mode() {
                    Some(TcpTlsMode::Terminate) => {
                        if let Some(tls_acceptor) = tls_acceptor {
                            let tls_stream = tls_acceptor.accept(stream).await?;
                            return send_upstream(&upstream.target, tls_stream).await;
                        } else {
                            tracing::warn!("TLS not configured for termination");
                        }
                    }
                    _ => return send_upstream(&upstream.target, stream).await,
                }
            } else {
                tracing::warn!("Router: No upstream found for {client_addr}");
            }
        }
        Err(err) => {
            tracing::warn!("No route configured for {client_addr}: {err}");
        }
    }

    Ok(())
}

async fn send_upstream<T>(target: &str, mut stream: T) -> io::Result<()>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    let mut upstream = TcpStream::connect(target).await?;
    let _ = tokio::io::copy_bidirectional(&mut stream, &mut upstream).await?;
    Ok(())
}
