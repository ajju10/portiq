use crate::config::{GatewayConfig, Upstream};
use crate::load_balancer::{LoadBalancer, WeightedRoundRobin};
use std::collections::HashMap;
use std::sync::Arc;

pub struct Service {
    lb: LoadBalancer,
}

pub struct ServiceRegistry {
    http: HashMap<String, Service>,
}

impl ServiceRegistry {
    pub fn init(gateway_config: Arc<GatewayConfig>) -> Self {
        let mut http_map = HashMap::with_capacity(gateway_config.http.services.len());
        for (name, svc_config) in &gateway_config.http.services {
            let strategy = Box::new(WeightedRoundRobin::new(&svc_config.upstreams));
            let lb = LoadBalancer::new(strategy);
            http_map.insert(name.clone(), Service { lb });
        }
        ServiceRegistry { http: http_map }
    }

    pub fn get_service_endpoint(&self, name: &str) -> Option<&Upstream> {
        self.http.get(name).and_then(|svc| svc.lb.get_next())
    }
}
