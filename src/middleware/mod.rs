use async_trait::async_trait;
use http_body_util::combinators::BoxBody;
use hyper::body::Bytes;
use hyper::header::HeaderName;
use hyper::{Error, Request, Response};
use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");

mod access_logger;

pub mod registry;

mod constants;

mod request_id;

pub use access_logger::AccessLogger;
pub use request_id::RequestID;

type Result<T> = std::result::Result<T, Infallible>;

pub type RequestBody = BoxBody<Bytes, Error>;

type ResponseBody = BoxBody<Bytes, Error>;

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub type HandlerFunc = Arc<
    dyn Send
        + Sync
        + Fn(
            Request<RequestBody>,
        ) -> Pin<Box<dyn Future<Output = Result<Response<ResponseBody>>> + Send>>,
>;

#[async_trait]
pub trait Middleware: Send + Sync {
    async fn call(
        &self,
        req: Request<RequestBody>,
        next: Next<'_>,
    ) -> Result<Response<ResponseBody>>;
}

pub struct Next<'a> {
    handler: HandlerFunc,
    middlewares: &'a [Arc<dyn Middleware>],
}

impl<'a> Next<'a> {
    pub fn new(handler: HandlerFunc, middlewares: &'a [Arc<dyn Middleware>]) -> Self {
        Next {
            handler,
            middlewares,
        }
    }

    pub fn run(
        mut self,
        req: Request<RequestBody>,
    ) -> BoxFuture<'a, Result<Response<ResponseBody>>> {
        if let Some((current, rest)) = self.middlewares.split_first() {
            self.middlewares = rest;
            current.call(req, self)
        } else {
            Box::pin(async move { (self.handler)(req).await })
        }
    }
}
