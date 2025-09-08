use crate::config::{GatewayConfig, Upstream};
use crate::error::RouterError;
use crate::service::ServiceRegistry;
use std::net::IpAddr;
use std::sync::Arc;

pub struct Route {
    hosts: Option<Box<[String]>>,
    path: Option<String>,
    listeners: Box<[String]>,
    service: String,
    middlewares: Box<[String]>,
}

impl Route {
    pub fn get_service(&self) -> String {
        self.service.clone()
    }

    pub fn get_middlewares(&self) -> &[String] {
        &self.middlewares
    }
}

pub struct Router {
    http: Box<[Route]>,
    svc_registry: Arc<ServiceRegistry>,
}

/// Router for new setup, the routing logic should be added now
impl Router {
    pub fn new(gateway_config: Arc<GatewayConfig>, svc_registry: Arc<ServiceRegistry>) -> Self {
        let mut http_routes = Vec::with_capacity(gateway_config.http.routes.len());
        for route in &gateway_config.http.routes {
            let route_v1 = Route {
                hosts: route.hosts.clone().map(|hosts| hosts.into_boxed_slice()),
                path: route.path.clone(),
                listeners: route.listeners.clone().into_boxed_slice(),
                service: route.service.clone(),
                middlewares: route
                    .middlewares
                    .as_ref()
                    .cloned()
                    .unwrap_or_else(Vec::new)
                    .into_boxed_slice(),
            };
            http_routes.push(route_v1);
        }
        Router {
            http: http_routes.into_boxed_slice(),
            svc_registry,
        }
    }

    pub fn get_route(&self, host: &str, path: &str, listener: &str) -> Result<&Route, RouterError> {
        let route = &self
            .http
            .iter()
            .filter(|&route| {
                let matches_listener = self.match_listener(listener, &route.listeners);

                let matches_host = if let Some(router_hosts) = &route.hosts {
                    self.match_host(host, router_hosts)
                } else {
                    true
                };

                let matches_path = if let Some(router_path) = &route.path {
                    self.match_path(path, router_path)
                } else {
                    true
                };

                matches_listener && matches_host && matches_path
            })
            .max_by_key(|&route| {
                let mut score = 0;
                if route.hosts.is_some() {
                    score += 1;
                }
                if route.path.is_some() {
                    score += 1
                }
                score
            });

        route.ok_or(RouterError::NotFound)
    }

    pub fn get_service(&self, name: &str) -> Result<&Upstream, RouterError> {
        self.svc_registry
            .get_service_endpoint(name)
            .ok_or(RouterError::NoUpstream)
    }

    fn match_host(&self, host: &str, router_hosts: &[String]) -> bool {
        for rh in router_hosts {
            if let Some(suffix) = rh.strip_prefix("*.") {
                if host.ends_with(suffix) && host != suffix {
                    return true;
                }
            } else if rh == host {
                return true;
            }
        }

        false
    }

    fn match_path(&self, path: &str, router_path: &str) -> bool {
        if router_path.ends_with("/*") {
            let prefix = &router_path[..router_path.len() - 1];
            if path.ends_with('/') {
                path.starts_with(prefix)
            } else {
                path.starts_with(&prefix[..prefix.len() - 1])
            }
        } else {
            // check for both trailing slashes and exact match
            path == router_path || path == format!("{router_path}/")
        }
    }

    fn match_listener(&self, listener: &str, router_listeners: &[String]) -> bool {
        router_listeners.iter().any(|rl| rl == listener)
    }
}

pub struct RouterContext {
    pub(crate) router: Arc<Router>,
    pub(crate) ip_addr: IpAddr,
    pub(crate) listener: String,
    pub(crate) http_client: Arc<reqwest::Client>,
    pub(crate) gateway_config: Arc<GatewayConfig>,
}

impl RouterContext {
    pub(crate) fn new(
        router: Arc<Router>,
        ip_addr: IpAddr,
        listener: String,
        http_client: Arc<reqwest::Client>,
        gateway_config: Arc<GatewayConfig>,
    ) -> Self {
        RouterContext {
            router,
            ip_addr,
            listener,
            http_client,
            gateway_config,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::ServiceRegistry;
    use config::{Config, File, FileFormat};

    const TEST_ROUTING_CONFIG: &str = r#"
        listeners:
          - name: http-main
            addr: 0.0.0.0:3000

          - name: internal-http
            addr: 127.0.0.1:8080

        http:
          services:
            user-service:
              upstreams:
                - target: http://user.service1:3000

            auth-service:
              upstreams:
                - target: https://auth.service:3000

          routes:
            - hosts: [ api.example.com, "*.api.example.com" ]
              path: /v1/*
              listeners: [ http-main ]
              service: user-service

            - path: /new
              listeners: [ internal-main ]
              service: auth-service
    "#;

    fn build_gateway_config() -> GatewayConfig {
        Config::builder()
            .add_source(File::from_str(TEST_ROUTING_CONFIG, FileFormat::Yaml))
            .build()
            .unwrap()
            .try_deserialize()
            .unwrap()
    }

    fn build_svc_registry() -> ServiceRegistry {
        let config = build_gateway_config();
        ServiceRegistry::init(Arc::new(config))
    }

    fn build_router() -> Router {
        let config = build_gateway_config();
        let svc_registry = build_svc_registry();
        Router::new(Arc::new(config), Arc::new(svc_registry))
    }

    #[test]
    fn test_route_matches_with_host_and_path() {
        let router = build_router();
        let route_result = router.get_route("api.example.com", "/v1/api", "http-main");
        assert!(
            matches!(route_result, Ok(_)),
            "This route should match to user-service"
        );
        let route = route_result.unwrap();
        assert_eq!(route.get_service(), "user-service");
    }

    #[test]
    fn test_wildcard_host_matches_user_service() {
        let router = build_router();
        let route_result = router.get_route("some.api.example.com", "/v1", "http-main");
        assert!(
            matches!(route_result, Ok(_)),
            "This route should match to user-service"
        );
        let route = route_result.unwrap();
        assert_eq!(route.get_service(), "user-service");
    }

    //     #[test]
    //     fn test_route_matches_correct_path_and_method() {
    //         let router = build_router();
    //         let route_result = router
    //             .get_route("/api/test", "POST")
    //             .expect("Router should match path: /api/test and method: POST");
    //         let upstream = route_result
    //             .get_upstream()
    //             .expect("Route should return upstream");
    //         assert_eq!(upstream.target, "http://localhost:5000");
    //     }
    //
    //     #[test]
    //     fn test_route_rejects_wrong_method() {
    //         let router = build_router();
    //         let result = router.get_route("/api/test", "PUT");
    //         assert!(matches!(result, Err(RouterError::MethodNotAllowed)));
    //     }
    //
    //     #[test]
    //     fn test_route_accepts_any_method_if_none_specified() {
    //         let router = build_router();
    //         for method in &["GET", "POST", "PUT", "DELETE"] {
    //             let route = router
    //                 .get_route("/api/health", method)
    //                 .expect(&format!("Route should accept method {}", method));
    //             let upstream = route.get_upstream().expect("Route should return upstream");
    //             assert_eq!(upstream.target, "http://localhost:5001");
    //         }
    //     }
    //
    //     #[test]
    //     fn test_route_not_found() {
    //         let router = build_router();
    //         let result = router.get_route("/nonexistent", "GET");
    //         assert!(matches!(result, Err(RouterError::NotFound)));
    //     }
    //
    //     #[test]
    //     fn test_multiple_routes_distinct_paths() {
    //         let router = build_router();
    //
    //         let test_result = router
    //             .get_route("/api/test", "GET")
    //             .expect("Router should match path: /api/test and method: GET");
    //         let upstream = test_result
    //             .get_upstream()
    //             .expect("Route should return upstream");
    //         assert_eq!(upstream.target, "http://localhost:5000");
    //
    //         let health_result = router
    //             .get_route("/api/health", "POST")
    //             .expect("Router should match path: /api/health and method: POST");
    //         let upstream = health_result
    //             .get_upstream()
    //             .expect("Route should return upstream");
    //         assert_eq!(upstream.target, "http://localhost:5001");
    //     }
    //
    //     #[test]
    //     fn test_prefix_path_matches() {
    //         let router = build_router();
    //         let exact_match = router.get_route("/api/test", "GET");
    //         let trailing_slash_match = router.get_route("/api/test/", "GET");
    //         let wildcard_match = router.get_route("/api/test/new", "GET");
    //
    //         assert!(exact_match.is_ok(), "Expected exact match to succeed");
    //         assert!(
    //             trailing_slash_match.is_ok(),
    //             "Expected trailing slash match to succeed"
    //         );
    //         assert!(wildcard_match.is_ok(), "Expected wildcard match to succeed");
    //     }
}
