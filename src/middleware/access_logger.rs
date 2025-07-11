use crate::middleware::REQUEST_ID_HEADER;
use http_body_util::combinators::BoxBody;
use hyper::body::Bytes;
use hyper::{Error, Request, Response, body::Incoming, header::USER_AGENT, service::Service};
use std::convert::Infallible;
use std::{net::IpAddr, pin::Pin, str::FromStr, time::Instant};

#[derive(Clone)]
pub struct AccessLogger<S> {
    inner: S,
}

impl<S> AccessLogger<S> {
    pub fn new(inner: S) -> Self {
        AccessLogger { inner }
    }
}

impl<S> Service<Request<Incoming>> for AccessLogger<S>
where
    S: Service<Request<Incoming>, Response = Response<BoxBody<Bytes, Error>>, Error = Infallible>
        + Clone
        + Send
        + 'static,
    <S as Service<Request<Incoming>>>::Future: Send,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn call(&self, req: Request<Incoming>) -> Self::Future {
        let start = Instant::now();
        let method = req.method().clone();
        let path = req.uri().path().to_string();
        let user_agent = req
            .headers()
            .get(USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("-")
            .to_string();
        let client_ip = req
            .extensions()
            .get::<IpAddr>()
            .unwrap_or(&IpAddr::from_str("127.0.0.1").unwrap())
            .to_owned();
        let request_id = req
            .headers()
            .get(REQUEST_ID_HEADER)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("-")
            .to_string();

        let inner = self.inner.clone();
        Box::pin(async move {
            let result = inner.call(req).await.unwrap();

            let duration = start.elapsed().as_millis();
            let status_code = result.status().as_u16();
            if result.status().is_success() {
                tracing::info!(
                    target: "access",
                    status = %status_code,
                    method = %method,
                    path = %path,
                    duration_ms = %duration,
                    client_ip = %client_ip,
                    user_agent = %user_agent,
                    request_id = %request_id,
                );
            } else {
                tracing::error!(
                    target: "access",
                    status = %status_code,
                    method = %method,
                    path = %path,
                    duration_ms = %duration,
                    client_ip = %client_ip,
                    user_agent = %user_agent,
                    request_id = %request_id,
                );
            }

            Ok(result)
        })
    }
}
