use crate::middleware::REQUEST_ID_HEADER;
use hyper::{Request, body::Incoming, http::HeaderValue, service::Service};
use uuid::Uuid;

#[derive(Clone)]
pub struct RequestID<S> {
    inner: S,
}

impl<S> RequestID<S> {
    pub fn new(inner: S) -> Self {
        RequestID { inner }
    }
}

impl<S> Service<Request<Incoming>> for RequestID<S>
where
    S: Service<Request<Incoming>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn call(&self, req: Request<Incoming>) -> Self::Future {
        let request_id = Uuid::new_v4();
        let mut req = req;
        req.headers_mut().insert(
            REQUEST_ID_HEADER,
            HeaderValue::from_str(&request_id.to_string()).unwrap(),
        );
        self.inner.call(req)
    }
}
