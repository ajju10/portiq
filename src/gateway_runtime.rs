use crate::config::GatewayConfig;
use crate::router::Router;
use crate::service::ServiceRegistry;
use std::sync::Arc;

pub struct GatewayRuntime {
    router: Arc<Router>,
    applied_config: GatewayConfig,
}

impl GatewayRuntime {
    pub fn new(gateway_config: Arc<GatewayConfig>) -> Self {
        let service_registry = Arc::new(ServiceRegistry::init(gateway_config.clone()));
        let router = Arc::new(Router::new(gateway_config.clone(), service_registry));
        GatewayRuntime {
            router,
            applied_config: (*gateway_config).clone(),
        }
    }

    pub fn get_last_applied_config(&self) -> &GatewayConfig {
        &self.applied_config
    }

    pub fn get_router(&self) -> Arc<Router> {
        self.router.clone()
    }
}
