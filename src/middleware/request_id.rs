use crate::middleware::Result;
use crate::middleware::registry::MiddlewareFactory;
use crate::middleware::{Middleware, Next, REQUEST_ID_HEADER, RequestBody, ResponseBody};
use async_trait::async_trait;
use hyper::http::HeaderValue;
use hyper::{Request, Response};
use std::sync::Arc;
use uuid::Uuid;

pub struct RequestID;

#[async_trait]
impl Middleware for RequestID {
    async fn call(
        &self,
        req: Request<RequestBody>,
        next: Next<'_>,
    ) -> Result<Response<ResponseBody>> {
        let request_id = Uuid::new_v4();
        let mut req = req;
        req.headers_mut().insert(
            REQUEST_ID_HEADER,
            HeaderValue::from_str(&request_id.to_string()).unwrap(),
        );
        next.run(req).await
    }
}

impl MiddlewareFactory for RequestID {
    fn create(&self) -> Arc<dyn Middleware> {
        Arc::new(RequestID)
    }
}
