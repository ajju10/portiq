use std::convert::Infallible;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::env;

use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Empty};
use hyper::StatusCode;
use hyper::header::HeaderValue;
use hyper::server::conn::http1 as http1_server;
use hyper::{Request, Response, body::Bytes, service::service_fn};
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use tokio::net::{TcpListener, TcpStream};

use crate::config::GatewayConfig;

mod config;

#[tokio::main]
async fn main() {
    let args = env::args().collect::<Vec<_>>();
    assert!(args.len() > 1, "Config file is required\nUsage: cargo run -- <config-file-path>");

    let gateway_config = Arc::new(config::load_config(&args[1]));

    let server_addr =
        SocketAddr::from((Ipv4Addr::new(0, 0, 0, 0), gateway_config.server.port as u16));
    let listener = TcpListener::bind(server_addr).await.unwrap();

    println!("Started server at {:?}", server_addr);

    loop {
        let (stream, addr) = listener.accept().await.unwrap();
        println!("Connected with client: {}", addr);

        let aio = TokioIo::new(stream);

        let gateway_config = gateway_config.clone();
        tokio::spawn(async move {
            if let Err(err) = http1_server::Builder::new()
                .serve_connection(
                    aio,
                    service_fn(move |req| handle_client(req, gateway_config.clone(), addr.ip())),
                )
                .await
            {
                eprintln!("Error serving connection: {:?}", err);
            }
        });
    }
}

async fn handle_client(
    request: Request<hyper::body::Incoming>,
    gateway_config: Arc<GatewayConfig>,
    client_ip: IpAddr,
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, Infallible> {
    if let Some(upstream_url) = gateway_config.get_upstream_url(request.uri().path()) {
        let original_request = request;

        let proxy_url = format!("{}{}", upstream_url, original_request.uri().path())
            .parse::<hyper::Uri>()
            .unwrap();
        let proxy_host = proxy_url.host().expect("uri has no host");
        let proxy_port = proxy_url.port_u16().unwrap();
        let proxy_addr = format!("{}:{}", proxy_host, proxy_port);
        let stream = TcpStream::connect(proxy_addr).await.unwrap();
        let aio = TokioIo::new(stream);

        let (mut sender, conn) = hyper::client::conn::http1::handshake(aio).await.unwrap();
        tokio::task::spawn(async move {
            if let Err(err) = conn.await {
                println!("Connection failed: {:?}", err);
            }
        });

        let mut request_builder = Request::builder()
            .version(original_request.version())
            .uri(proxy_url.path())
            .method(original_request.method());

        for (key, value) in original_request.headers() {
            request_builder = request_builder.header(key, value);
        }

        // Set proxy headers
        request_builder = request_builder.header(
            "X-Forwarded-For",
            HeaderValue::from_str(&client_ip.to_string()).unwrap(),
        );

        let proxy_req = request_builder.body(original_request.into_body()).unwrap();

        match sender.send_request(proxy_req).await {
            Ok(proxy_res) => {
                println!(
                    "Response code from backend service: {:?}",
                    proxy_res.status()
                );
                println!(
                    "Response headers from backend service: {:#?}",
                    proxy_res.headers()
                );

                let mut response_builder = Response::builder().status(proxy_res.status());

                for (key, value) in proxy_res.headers() {
                    if key == "content-length" || key == "server" {
                        continue;
                    }
                    response_builder = response_builder.header(key, value);
                }

                // Add server header
                response_builder = response_builder.header("Server", "gateway-rs");

                let response_bytes = proxy_res.map(|b| b.boxed());
                let res = response_builder.body(response_bytes.into_body()).unwrap();
                Ok(res)
            }
            Err(_) => Ok(Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(
                    Empty::<Bytes>::new()
                        .map_err(|never| match never {})
                        .boxed(),
                )
                .unwrap()),
        }
    } else {
        println!("No mapping found for path {}", request.uri());
        let response = Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header("X-Proxy-Name", "gateway-rs")
            .body(
                Empty::<Bytes>::new()
                    .map_err(|never| match never {})
                    .boxed(),
            )
            .unwrap();
        Ok(response)
    }
}
