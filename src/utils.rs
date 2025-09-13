use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Empty, Full};
use hyper::body::Bytes;
use hyper::http::HeaderMap;
use hyper::{Response, StatusCode};
use reqwest::RequestBuilder;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use std::net::IpAddr;
use std::time::Duration;
use std::{fs, io};
use tokio::signal::unix::{SignalKind, signal};
use tokio_util::sync::CancellationToken;

// Load public certificate from file.
pub fn load_certs(filename: &str) -> io::Result<Vec<CertificateDer<'static>>> {
    let certfile = fs::File::open(filename)
        .map_err(|e| io::Error::other(format!("Failed to open {filename}: {e}")))?;
    let mut reader = io::BufReader::new(certfile);
    rustls_pemfile::certs(&mut reader).collect()
}

// Load private key from file.
pub fn load_private_key(filename: &str) -> io::Result<PrivateKeyDer<'static>> {
    let keyfile = fs::File::open(filename)
        .map_err(|e| io::Error::other(format!("Failed to open {filename}: {e}")))?;
    let mut reader = io::BufReader::new(keyfile);
    rustls_pemfile::private_key(&mut reader).map(|key| key.unwrap())
}

pub fn response_with_status(status_code: StatusCode) -> Response<BoxBody<Bytes, hyper::Error>> {
    Response::builder()
        .status(status_code)
        .header("Server", "portiq")
        .body(
            Empty::<Bytes>::new()
                .map_err(|never| match never {})
                .boxed(),
        )
        .unwrap()
}

pub fn bad_gateway_response() -> Response<BoxBody<Bytes, hyper::Error>> {
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

pub async fn graceful_shutdown(cancel_token: CancellationToken) {
    cancel_token.cancel();
    tracing::info!("Initiating shutdown, application will exit after 5 seconds");
    tokio::time::sleep(Duration::from_secs(5)).await;
}

pub async fn shutdown_signal() {
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

pub fn set_proxy_headers(
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
