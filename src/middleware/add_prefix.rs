use crate::config::MiddlewareConfig;
use crate::middleware::registry::MiddlewareFactory;
use crate::middleware::{Middleware, Next, RequestBody, ResponseBody};
use async_trait::async_trait;
use hyper::{Request, Response, Uri};
use std::sync::Arc;

pub struct AddPrefix {
    prefix: String,
}

#[async_trait]
impl Middleware for AddPrefix {
    async fn call(
        &self,
        req: Request<RequestBody>,
        next: Next<'_>,
    ) -> crate::middleware::Result<Response<ResponseBody>> {
        let mut req = req;
        let query_params = req
            .uri()
            .query()
            .map(|q| format!("?{q}"))
            .unwrap_or_default();
        let prefixed_path = format!("{}{}", self.prefix, req.uri().path());

        let mut parts = req.uri().clone().into_parts();
        parts.path_and_query = Some(
            format!("{prefixed_path}{query_params}")
                .parse()
                .expect("Invalid path and query parameters"),
        );
        let new_uri = Uri::from_parts(parts).expect("Invalid new URI");
        *req.uri_mut() = new_uri;

        next.run(req).await
    }
}

pub struct AddPrefixFactory;

impl MiddlewareFactory for AddPrefixFactory {
    fn create(&self, config: Option<MiddlewareConfig>) -> Arc<dyn Middleware> {
        match config {
            Some(MiddlewareConfig::AddPrefix(cfg)) => Arc::new(AddPrefix { prefix: cfg.prefix }),
            _ => panic!("Invalid config for add prefix middleware"),
        }
    }
}
