use crate::middleware::Middleware;
use std::collections::HashMap;
use std::sync::Arc;

pub trait MiddlewareFactory {
    fn create(&self) -> Arc<dyn Middleware>;
}

pub struct MiddlewareRegistry {
    middlewares: HashMap<String, Arc<dyn Middleware>>,
}

impl MiddlewareRegistry {
    pub fn new() -> Self {
        MiddlewareRegistry {
            middlewares: HashMap::new(),
        }
    }

    pub fn register_all(&mut self, middlewares: Vec<(&str, Box<dyn MiddlewareFactory>)>) {
        for (name, factory) in middlewares {
            self.middlewares.insert(name.to_string(), factory.create());
        }
    }

    pub fn create_chain(&self, names: &[&str]) -> Vec<Arc<dyn Middleware>> {
        names
            .iter()
            .filter_map(|name| self.middlewares.get(*name).cloned())
            .collect()
    }
}
