use crate::middleware::Result;
use crate::middleware::registry::MiddlewareFactory;
use crate::middleware::{Middleware, Next, REQUEST_ID_HEADER, RequestBody, ResponseBody};
use async_trait::async_trait;
use config::Value;
use hyper::header::USER_AGENT;
use hyper::{Request, Response};
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::time::Instant;

pub struct AccessLogger;

#[async_trait]
impl Middleware for AccessLogger {
    async fn call(
        &self,
        req: Request<RequestBody>,
        next: Next<'_>,
    ) -> Result<Response<ResponseBody>> {
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
            .unwrap_or(&IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)))
            .to_owned();
        let request_id = req
            .headers()
            .get(REQUEST_ID_HEADER)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("-")
            .to_string();

        let response = next.run(req).await.unwrap();
        let duration = start.elapsed().as_millis();
        let status_code = response.status().as_u16();
        if response.status().is_success() {
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
        Ok(response)
    }
}

impl MiddlewareFactory for AccessLogger {
    fn create(&self, _config: Option<Value>) -> Arc<dyn Middleware> {
        Arc::new(AccessLogger)
    }
}
