use crate::config::MiddlewareConfig;
use crate::middleware::constants::{ACCESS_LOGGER_MIDDLEWARE, REQUEST_ID_MIDDLEWARE};
use crate::middleware::{AccessLogger, Middleware, RequestID};
use config::Value;
use std::collections::HashMap;
use std::sync::Arc;

pub trait MiddlewareFactory: Send + Sync {
    fn create(&self, config: Option<Value>) -> Arc<dyn Middleware>;
}

pub struct MiddlewareRegistry {
    factories: HashMap<&'static str, Box<dyn MiddlewareFactory>>,
}

impl MiddlewareRegistry {
    pub fn init() -> Self {
        let mut factories: HashMap<&str, Box<dyn MiddlewareFactory>> = HashMap::new();
        factories.insert(REQUEST_ID_MIDDLEWARE, Box::new(RequestID));
        factories.insert(ACCESS_LOGGER_MIDDLEWARE, Box::new(AccessLogger));

        MiddlewareRegistry { factories }
    }

    pub fn create_chain(&self, _middlewares: &[MiddlewareConfig]) -> Vec<Arc<dyn Middleware>> {
        let mut route_middlewares = vec![];

        if let Some(request_id_middleware) = self
            .factories
            .get(REQUEST_ID_MIDDLEWARE)
            .map(|factory| factory.create(None))
        {
            route_middlewares.push(request_id_middleware);
        }

        if let Some(access_logger_middleware) = self
            .factories
            .get(ACCESS_LOGGER_MIDDLEWARE)
            .map(|factory| factory.create(None))
        {
            route_middlewares.push(access_logger_middleware);
        }

        // let chain = middlewares
        //     .iter()
        //     .map(|middleware_config| match middleware_config {
        //         MiddlewareConfig::RequestId => self
        //             .factories
        //             .get(REQUEST_ID_MIDDLEWARE)
        //             .map(|factory| factory.create(None)),
        //     })
        //     .collect();
        //
        // route_middlewares.extend(chain);
        route_middlewares
    }
}
