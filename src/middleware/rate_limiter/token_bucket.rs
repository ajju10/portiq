use crate::config::RateLimitKeySource;
use crate::middleware::rate_limiter::RateLimiter;
use crate::middleware::{Middleware, Next, RequestBody, ResponseBody};
use async_trait::async_trait;
use http_body_util::{BodyExt, Empty};
use hyper::body::Bytes;
use hyper::{Request, Response, StatusCode};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct TokenBucket {
    capacity: u32,
    refill_rate: f64, // per-second
    available_tokens: f64,
    last_refill: Instant,
}

impl TokenBucket {
    fn new(capacity: u32, refill_rate: f64) -> Self {
        TokenBucket {
            capacity,
            refill_rate,
            available_tokens: capacity as f64,
            last_refill: Instant::now(),
        }
    }

    fn allow(&mut self) -> bool {
        self.refill();

        if self.available_tokens >= 1.0 {
            self.available_tokens -= 1.0;
            true
        } else {
            false
        }
    }

    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        let tokens_to_add = self.refill_rate * elapsed;
        if tokens_to_add > 0.0 {
            let total_tokens = self.available_tokens + tokens_to_add;
            self.available_tokens = if (self.capacity as f64) < total_tokens {
                self.capacity as f64
            } else {
                total_tokens
            };
            self.last_refill = now;
        }
    }
}

pub struct TokenBucketRateLimiter {
    source: RateLimitKeySource,
    limit: u32,
    duration: Duration,
    store: Arc<Mutex<HashMap<String, TokenBucket>>>,
}

impl TokenBucketRateLimiter {
    pub fn new(
        source: RateLimitKeySource,
        limit: u32,
        duration: Duration,
        store: Arc<Mutex<HashMap<String, TokenBucket>>>,
    ) -> Self {
        assert!(limit > 0, "Limit should be greater than 0");
        assert!(duration.as_nanos() > 0, "Duration should be greater than 0");

        TokenBucketRateLimiter {
            source,
            limit,
            duration,
            store,
        }
    }
}

impl RateLimiter for TokenBucketRateLimiter {
    fn allow(&self, key: &str) -> bool {
        let mut store = self.store.lock().unwrap();
        let bucket = store.entry(key.to_string()).or_insert_with(|| {
            let capacity = self.limit;
            let refill_rate = self.limit as f64 / self.duration.as_secs_f64();
            TokenBucket::new(capacity, refill_rate)
        });

        bucket.allow()
    }

    fn retry_after(&self, key: &str) -> Option<Duration> {
        let mut store = self.store.lock().unwrap();
        if let Some(bucket) = store.get_mut(key) {
            if bucket.available_tokens >= 1.0 {
                Some(Duration::from_secs(0))
            } else {
                let tokens_needed = 1.0 - bucket.available_tokens;
                let seconds_to_wait = (tokens_needed / bucket.refill_rate).ceil() as u64;
                Some(Duration::from_secs(seconds_to_wait))
            }
        } else {
            None
        }
    }
}

#[async_trait]
impl Middleware for TokenBucketRateLimiter {
    async fn call(
        &self,
        req: Request<RequestBody>,
        next: Next<'_>,
    ) -> crate::middleware::Result<Response<ResponseBody>> {
        let key = match &self.source {
            RateLimitKeySource::IP(Some(header)) => {
                if let Some(v) = req.headers().get(header) {
                    v.to_str().unwrap_or("").to_string()
                } else {
                    req.extensions()
                        .get::<IpAddr>()
                        .unwrap_or(&IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)))
                        .to_string()
                }
            }
            RateLimitKeySource::IP(None) => req
                .extensions()
                .get::<IpAddr>()
                .unwrap_or(&IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)))
                .to_string(),
            RateLimitKeySource::RequestHeader(header) => req
                .headers()
                .get(header)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("-")
                .to_string(),
        };

        if self.allow(&key) {
            next.run(req).await
        } else {
            let retry_duration = self.retry_after(&key).unwrap_or(Duration::from_secs(0));
            Ok(Response::builder()
                .status(StatusCode::TOO_MANY_REQUESTS)
                .header("Server", "portiq")
                .header("Retry-After", retry_duration.as_secs())
                .body(
                    Empty::<Bytes>::new()
                        .map_err(|never| match never {})
                        .boxed(),
                )
                .expect("Response builder should not fail"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;
    // use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn test_blocks_after_exceeding_limit() {
        let key = "ajay:yadav";
        let store = Mutex::new(HashMap::new());
        let limiter = TokenBucketRateLimiter::new(
            RateLimitKeySource::IP(None),
            10,
            Duration::from_secs(60),
            Arc::new(store),
        );
        for _i in 1..=10 {
            assert!(limiter.allow(key));
        }
        assert!(!limiter.allow(key));
    }

    #[test]
    fn test_returns_retry_duration_on_limit_exceeded() {
        let key = "ajay:yadav";
        let store = Mutex::new(HashMap::new());
        let limiter = TokenBucketRateLimiter::new(
            RateLimitKeySource::IP(None),
            1,
            Duration::from_secs(5),
            Arc::new(store),
        );

        // first request should pass
        assert!(limiter.allow(key));

        let retry = limiter.retry_after(key);
        assert!(
            retry.unwrap() >= Duration::from_secs(4) && retry.unwrap() <= Duration::from_secs(5)
        );
    }

    #[test]
    fn test_refills_tokens_over_time() {
        let key = "ajay:yadav";
        let store = Mutex::new(HashMap::new());
        let limiter = TokenBucketRateLimiter::new(
            RateLimitKeySource::IP(None),
            3,
            Duration::from_secs(2),
            Arc::new(store),
        );

        // first 3 requests should pass
        assert!(limiter.allow(key));
        assert!(limiter.allow(key));
        assert!(limiter.allow(key));

        // this should fail
        assert!(!limiter.allow(key));

        sleep(Duration::from_secs(2));

        // bucket refilled this should pass
        assert!(limiter.allow(key));
    }
}
