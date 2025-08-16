use crate::config::MiddlewareConfig;
use crate::middleware::Middleware;
use crate::middleware::registry::MiddlewareFactory;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

mod token_bucket;

pub trait RateLimiter {
    fn allow(&self, key: &str) -> bool;

    fn retry_after(&self, key: &str) -> Option<Duration>;
}

pub struct RateLimiterFactory {
    store: Arc<Mutex<HashMap<String, token_bucket::TokenBucket>>>,
}

impl RateLimiterFactory {
    pub fn new() -> Self {
        RateLimiterFactory {
            store: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl MiddlewareFactory for RateLimiterFactory {
    fn create(&self, config: Option<MiddlewareConfig>) -> Arc<dyn Middleware> {
        match config {
            Some(MiddlewareConfig::RateLimit(cfg)) => {
                Arc::new(token_bucket::TokenBucketRateLimiter::new(
                    cfg.source,
                    cfg.limit,
                    cfg.period,
                    Arc::clone(&self.store),
                ))
            }
            _ => panic!("Invalid config for rate limiter"),
        }
    }
}
