use hyper::{Request, body::Incoming, service::Service};
use std::net::IpAddr;

#[derive(Clone)]
pub struct HandlerService<S> {
    inner: S,
    remote_addr: IpAddr,
}

impl<S> HandlerService<S> {
    pub fn new(inner: S, remote_addr: IpAddr) -> Self {
        HandlerService { inner, remote_addr }
    }
}

impl<S> Service<Request<Incoming>> for HandlerService<S>
where
    S: Service<Request<Incoming>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn call(&self, req: Request<Incoming>) -> Self::Future {
        let mut req = req;
        req.extensions_mut().insert(self.remote_addr);
        self.inner.call(req)
    }
}
