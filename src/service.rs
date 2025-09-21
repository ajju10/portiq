use crate::config::{GatewayConfig, Upstream};
use crate::load_balancer::{LoadBalancer, WeightedRoundRobin};
use std::collections::HashMap;
use std::sync::Arc;

pub struct Service {
    lb: LoadBalancer,
}

impl Service {
    fn new(upstreams: &[Upstream]) -> Self {
        let strategy = Box::new(WeightedRoundRobin::new(upstreams));
        Service {
            lb: LoadBalancer::new(strategy),
        }
    }
}

pub struct ServiceRegistry {
    http: HashMap<String, Service>,
    tcp: HashMap<String, Service>,
}

impl ServiceRegistry {
    pub fn init(gateway_config: Arc<GatewayConfig>) -> Self {
        let http = gateway_config
            .http
            .services
            .iter()
            .map(|(name, service_config)| (name.clone(), Service::new(&service_config.upstreams)))
            .collect();

        let tcp = gateway_config
            .tcp
            .services
            .iter()
            .map(|(name, service_config)| (name.clone(), Service::new(&service_config.upstreams)))
            .collect();

        ServiceRegistry { http, tcp }
    }

    pub fn get_http_service_endpoint(&self, name: &str) -> Option<&Upstream> {
        self.http.get(name).and_then(|svc| svc.lb.get_next())
    }

    pub fn get_tcp_service_endpoint(&self, name: &str) -> Option<&Upstream> {
        self.tcp.get(name).and_then(|svc| svc.lb.get_next())
    }
}
