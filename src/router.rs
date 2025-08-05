use crate::config::{MiddlewareConfig, RouteConfig, Upstream};
use crate::error::RouterError;
use crate::load_balancer::{LoadBalancer, WeightedRoundRobin};
use matchit::Router as MatchItRouter;
use std::collections::HashSet;
use std::sync::Arc;

pub struct Route {
    methods: HashSet<String>,
    middlewares: Box<[MiddlewareConfig]>,
    lb: LoadBalancer,
}

impl Route {
    pub fn get_upstream(&self) -> Result<&Upstream, RouterError> {
        self.lb.get_next().ok_or(RouterError::NoUpstream)
    }

    pub fn get_middlewares(&self) -> &[MiddlewareConfig] {
        &self.middlewares
    }
}

pub struct Router {
    inner: MatchItRouter<Arc<Route>>,
}

impl Router {
    pub fn new(route_configs: Vec<RouteConfig>) -> Self {
        let mut inner = MatchItRouter::new();
        for rc in route_configs {
            let strategy = Box::new(WeightedRoundRobin::new(&rc.upstream));
            let lb = LoadBalancer::new(strategy);
            let route = Arc::new(Route {
                methods: rc.methods.into_iter().collect(),
                middlewares: rc
                    .middlewares
                    .as_ref()
                    .map_or_else(Vec::new, |middleware_config| middleware_config.clone())
                    .into_boxed_slice(),
                lb,
            });

            // Exact match like /api/v1
            inner.insert(rc.path.as_str(), route.clone()).unwrap();

            // Trailing slash match like /api/v1/
            inner
                .insert(format!("{}/", rc.path.as_str()), route.clone())
                .unwrap();

            // Wildcard match like /api/v1/*
            inner.insert(format!("{}/{{*p}}", rc.path), route).unwrap();
        }

        Router { inner }
    }

    pub fn get_route(&self, path: &str, method: &str) -> Result<Arc<Route>, RouterError> {
        let matched = self.inner.at(path).map_err(|_| RouterError::NotFound)?;
        if matched.value.methods.is_empty() || matched.value.methods.contains(method) {
            Ok(matched.value.clone())
        } else {
            Err(RouterError::MethodNotAllowed)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_router() -> Router {
        Router::new(vec![
            RouteConfig {
                path: "/api/test".to_string(),
                methods: vec!["POST".to_string(), "GET".to_string()],
                upstream: vec![Upstream {
                    url: "http://localhost:5000".to_string(),
                    weight: 1,
                }],
                middlewares: None,
            },
            RouteConfig {
                path: "/api/health".to_string(),
                methods: vec![], // allow all methods
                upstream: vec![Upstream {
                    url: "http://localhost:5001".to_string(),
                    weight: 1,
                }],
                middlewares: None,
            },
        ])
    }

    #[test]
    fn test_route_matches_correct_path_and_method() {
        let router = build_router();
        let route_result = router
            .get_route("/api/test", "POST")
            .expect("Router should match path: /api/test and method: POST");
        let upstream = route_result
            .get_upstream()
            .expect("Route should return upstream");
        assert_eq!(upstream.url, "http://localhost:5000");
    }

    #[test]
    fn test_route_rejects_wrong_method() {
        let router = build_router();
        let result = router.get_route("/api/test", "PUT");
        assert!(matches!(result, Err(RouterError::MethodNotAllowed)));
    }

    #[test]
    fn test_route_accepts_any_method_if_none_specified() {
        let router = build_router();
        for method in &["GET", "POST", "PUT", "DELETE"] {
            let route = router
                .get_route("/api/health", method)
                .expect(&format!("Route should accept method {}", method));
            let upstream = route.get_upstream().expect("Route should return upstream");
            assert_eq!(upstream.url, "http://localhost:5001");
        }
    }

    #[test]
    fn test_route_not_found() {
        let router = build_router();
        let result = router.get_route("/nonexistent", "GET");
        assert!(matches!(result, Err(RouterError::NotFound)));
    }

    #[test]
    fn test_multiple_routes_distinct_paths() {
        let router = build_router();

        let test_result = router
            .get_route("/api/test", "GET")
            .expect("Router should match path: /api/test and method: GET");
        let upstream = test_result
            .get_upstream()
            .expect("Route should return upstream");
        assert_eq!(upstream.url, "http://localhost:5000");

        let health_result = router
            .get_route("/api/health", "POST")
            .expect("Router should match path: /api/health and method: POST");
        let upstream = health_result
            .get_upstream()
            .expect("Route should return upstream");
        assert_eq!(upstream.url, "http://localhost:5001");
    }

    #[test]
    fn test_prefix_path_matches() {
        let router = build_router();
        let exact_match = router.get_route("/api/test", "GET");
        let trailing_slash_match = router.get_route("/api/test/", "GET");
        let wildcard_match = router.get_route("/api/test/new", "GET");

        assert!(exact_match.is_ok(), "Expected exact match to succeed");
        assert!(
            trailing_slash_match.is_ok(),
            "Expected trailing slash match to succeed"
        );
        assert!(wildcard_match.is_ok(), "Expected wildcard match to succeed");
    }
}
