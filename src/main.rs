use std::convert::Infallible;
use std::env;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;

use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Empty};
use hyper::client::conn::http1 as http1_client;
use hyper::header::HeaderValue;
use hyper::server::conn::http1 as http1_server;
use hyper::{Request, Response, body::Bytes, service::service_fn};
use hyper::{StatusCode, Uri};
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use tokio::net::{TcpListener, TcpStream};

use crate::config::GatewayConfig;

mod config;

#[tokio::main]
async fn main() {
    let args = env::args().collect::<Vec<_>>();
    assert!(
        args.len() > 1,
        "Config file is required\nUsage: cargo run -- <config-file-path>"
    );

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
    let original_request = request;
    let original_path = original_request.uri().path();
    let original_method = original_request.method();

    match gateway_config.match_upstream_path(original_path, original_method.as_str()) {
        Ok(upstream_url) => {
            let proxy_uri_str = format!("{}{}", upstream_url, original_request.uri().path());
            let proxy_uri: Uri = match proxy_uri_str.parse() {
                Ok(uri) => uri,
                Err(_) => return Ok(response_with_status(StatusCode::BAD_GATEWAY)),
            };

            let proxy_host = match proxy_uri.host() {
                Some(host) => host,
                None => return Ok(response_with_status(StatusCode::BAD_GATEWAY)),
            };

            let proxy_port = match proxy_uri.port_u16() {
                Some(port) => port,
                None => return Ok(response_with_status(StatusCode::BAD_GATEWAY)),
            };

            let proxy_addr = format!("{}:{}", proxy_host, proxy_port);
            let stream = match TcpStream::connect(proxy_addr).await {
                Ok(s) => s,
                Err(_) => return Ok(response_with_status(StatusCode::BAD_GATEWAY)),
            };

            let aio = TokioIo::new(stream);
            let (mut sender, conn) = match http1_client::handshake(aio).await {
                Ok(result) => result,
                Err(_) => return Ok(response_with_status(StatusCode::BAD_GATEWAY)),
            };

            tokio::task::spawn(async move {
                if let Err(err) = conn.await {
                    println!("Connection failed: {:?}", err);
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
                Err(_) => return Ok(response_with_status(StatusCode::BAD_GATEWAY)),
            };

            match sender.send_request(proxy_req).await {
                Ok(proxy_res) => {
                    let mut response_builder = Response::builder().status(proxy_res.status());
                    for (key, value) in proxy_res.headers() {
                        if key != "content-length" || key != "server" {
                            response_builder = response_builder.header(key, value);
                        }
                    }
                    // Add server header
                    response_builder = response_builder.header("Server", "portiq");

                    let response_body = proxy_res.map(|b| b.boxed());
                    let res = response_builder.body(response_body.into_body()).unwrap();
                    Ok(res)
                }
                Err(_) => Ok(response_with_status(StatusCode::BAD_GATEWAY)),
            }
        }
        Err(status_code) => {
            println!("{}", status_code);
            Ok(response_with_status(status_code))
        }
    }
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
