use crate::config::{RouteConfig, Upstream};
use crate::error::RouterError;
use crate::load_balancer::{LoadBalancer, WeightedRoundRobin};
use std::collections::HashMap;

struct Route {
    methods: Vec<String>,
    lb: LoadBalancer,
}

pub struct Router {
    routes: HashMap<String, Route>,
}

impl Router {
    pub fn new(route_configs: Vec<RouteConfig>) -> Self {
        let mut routes = HashMap::new();
        for rc in route_configs {
            let strategy = Box::new(WeightedRoundRobin::new(&rc.upstream));
            let lb = LoadBalancer::new(strategy);
            let route = Route {
                methods: rc.methods,
                lb,
            };
            routes.insert(rc.path, route);
        }

        Router { routes }
    }

    pub fn match_route(&self, path: &str, method: &str) -> Result<Upstream, RouterError> {
        let route = self.routes.get(path).ok_or(RouterError::NotFound)?;
        if route.methods.is_empty() || route.methods.iter().any(|m| m.eq_ignore_ascii_case(method))
        {
            let upstream = route.lb.get_next().ok_or(RouterError::NoUpstream)?;
            Ok(upstream.clone())
        } else {
            tracing::warn!("Router error: Method not allowed");
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
            },
            RouteConfig {
                path: "/api/health".to_string(),
                methods: vec![], // allow all methods
                upstream: vec![Upstream {
                    url: "http://localhost:5001".to_string(),
                    weight: 1,
                }],
            },
        ])
    }

    #[test]
    fn test_route_matches_correct_path_and_method() {
        let router = build_router();
        let result = router.match_route("/api/test", "POST");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().url, "http://localhost:5000");
    }

    #[test]
    fn test_route_rejects_wrong_method() {
        let router = build_router();
        let result = router.match_route("/api/test", "PUT");
        assert!(matches!(result, Err(RouterError::MethodNotAllowed)));
    }

    #[test]
    fn test_route_accepts_any_method_if_none_specified() {
        let router = build_router();
        for method in &["GET", "POST", "PUT", "DELETE"] {
            let result = router.match_route("/api/health", method);
            assert!(result.is_ok(), "Route should accept method {}", method);
            assert_eq!(result.unwrap().url, "http://localhost:5001");
        }
    }

    #[test]
    fn test_route_not_found() {
        let router = build_router();
        let result = router.match_route("/nonexistent", "GET");
        assert!(matches!(result, Err(RouterError::NotFound)));
    }

    #[test]
    fn test_multiple_routes_distinct_paths() {
        let router = build_router();

        let test_result = router.match_route("/api/test", "GET");
        assert!(test_result.is_ok());
        assert_eq!(test_result.unwrap().url, "http://localhost:5000");

        let health_result = router.match_route("/api/health", "POST");
        assert!(health_result.is_ok());
        assert_eq!(health_result.unwrap().url, "http://localhost:5001");
    }
}
