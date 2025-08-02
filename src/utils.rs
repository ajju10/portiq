use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Empty, Full};
use hyper::body::Bytes;
use hyper::{Response, StatusCode};
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use std::{fs, io};

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
