use crate::gateway_runtime::GatewayRuntime;
use crate::{CONFIG_FILE_PATH, SharedGatewayState};
use config::{Config, File};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayConfig {
    #[serde(default = "default_config_version")]
    pub version: u8,
    #[serde(default)]
    pub admin_api: AdminAPIConfig,
    #[serde(default)]
    pub log: GatewayLog,
    #[serde(default)]
    pub access_log: AccessLog,
    pub tls: Option<Vec<TLSConfig>>,
    pub listeners: Vec<Listener>,
    pub http: HttpConfig,
}

impl GatewayConfig {
    fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err(String::from("version value must be 1"));
        }

        // Check if a default tls config is provided (if at all)
        if let Some(tls_config) = &self.tls {
            let count = tls_config.iter().filter(|cfg| cfg.default).count();
            if count != 1 {
                return Err(format!(
                    "Exactly one TLS config must be marked as default, found {count}",
                ));
            }
        }

        let mut seen_listeners = HashSet::with_capacity(self.listeners.len());
        for listener in &self.listeners {
            if !seen_listeners.insert(&listener.name) {
                return Err(format!("Duplicate listener name {}", listener.name));
            }

            if let Protocol::Https = listener.protocol
                && self.tls.is_none()
            {
                return Err(format!(
                    "TLS config is required to spawn listener {}",
                    listener.name
                ));
            }
        }

        let mut seen_services = HashSet::with_capacity(self.http.services.len());
        for key in self.http.services.keys() {
            if seen_services.contains(key) {
                return Err(format!("Duplicate service name {}", key));
            }
            seen_services.insert(key);
        }

        for route in &self.http.routes {
            if route.hosts.is_none() && route.path.is_none() {
                return Err(format!(
                    "At least one of hosts or path is required for matching route against service {}",
                    route.service
                ));
            }

            for listener in &route.listeners {
                if !seen_listeners.contains(listener) {
                    return Err(format!("Undefined listener {}", listener));
                }
            }

            if !seen_services.contains(&route.service) {
                return Err(format!("Undefined service {}", route.service));
            }

            if let Some(route_middlewares) = &route.middlewares {
                for middleware in route_middlewares {
                    if !self.http.middlewares.contains_key(middleware) {
                        return Err(format!("Middleware {} is not defined", middleware));
                    }
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AdminAPIConfig {
    pub addr: SocketAddr,
}

impl Default for AdminAPIConfig {
    fn default() -> Self {
        AdminAPIConfig {
            addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 5678),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TLSConfig {
    pub cert_file: PathBuf,
    pub key_file: PathBuf,
    #[serde(default)]
    pub default: bool,
    pub hostnames: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Listener {
    pub name: String,
    pub addr: SocketAddr,
    #[serde(default)]
    pub protocol: Protocol,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpConfig {
    #[serde(default)]
    pub middlewares: HashMap<String, MiddlewareConfig>,
    pub services: HashMap<String, HttpServiceConfig>,
    pub routes: Vec<RouteConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpServiceConfig {
    pub upstreams: Vec<Upstream>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteConfig {
    pub hosts: Option<Vec<String>>,
    pub path: Option<String>,
    pub listeners: Vec<String>,
    pub service: String,
    pub middlewares: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    #[default]
    Compact,
    Json,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    #[default]
    Http,
    Https,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddPrefixConfig {
    pub prefix: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RateLimitKeySource {
    #[serde(rename = "ip")]
    IP(Option<String>),
    RequestHeader(String),
}

impl Default for RateLimitKeySource {
    fn default() -> Self {
        RateLimitKeySource::IP(None)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    #[serde(default)]
    pub source: RateLimitKeySource,
    pub limit: u32,
    #[serde(with = "humantime_serde")]
    pub period: Duration,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MiddlewareConfig {
    AddPrefix(AddPrefixConfig),
    RateLimit(RateLimitConfig),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GatewayLog {
    #[serde(default = "default_log_level")]
    pub level: String,
    #[serde(default)]
    pub format: LogFormat,
    #[serde(default = "default_log_file_path")]
    pub file_path: String,
}

impl Default for GatewayLog {
    fn default() -> Self {
        GatewayLog {
            level: default_log_level(),
            format: LogFormat::default(),
            file_path: default_log_file_path(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AccessLog {
    #[serde(default = "default_access_log_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub format: LogFormat,
    #[serde(default = "default_log_file_path")]
    pub file_path: String,
}

impl Default for AccessLog {
    fn default() -> Self {
        AccessLog {
            enabled: default_access_log_enabled(),
            format: LogFormat::default(),
            file_path: default_log_file_path(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Upstream {
    pub target: String,
    #[serde(default = "default_upstream_weight")]
    pub weight: u32,
}

fn default_log_level() -> String {
    "INFO".to_string()
}

fn default_access_log_enabled() -> bool {
    true
}

fn default_log_file_path() -> String {
    "stdout".to_string()
}

fn default_upstream_weight() -> u32 {
    1
}

fn default_config_version() -> u8 {
    1
}

pub fn load_config() -> Result<GatewayConfig, String> {
    let file_path = CONFIG_FILE_PATH.get().ok_or("Config file path not found")?;

    let cfg = Config::builder()
        .add_source(File::with_name(file_path))
        .build()
        .map_err(|err| err.to_string())?
        .try_deserialize::<GatewayConfig>()
        .map_err(|err| err.to_string())?;

    cfg.validate().map_or_else(Err, |_| Ok(cfg))
}

pub fn reload_config(current_state: SharedGatewayState) -> Result<(), String> {
    let cfg = load_config()?;
    {
        let current_state = current_state.load();
        // perform validations for non-reloadable values, currently reject if anything changes
        if !static_config_same(current_state.get_last_applied_config(), &cfg) {
            tracing::warn!("Start part of config changed, using previous config");
            return Err(String::from(
                "Static fields of config has changed, config not applied",
            ));
        }
    }

    // Build new gateway runtime and swap
    let new_config = Arc::new(cfg);
    let new_runtime = GatewayRuntime::new(new_config);
    current_state.store(Arc::new(new_runtime));

    Ok(())
}

fn static_config_same(previous: &GatewayConfig, new: &GatewayConfig) -> bool {
    previous.version == new.version
        && previous.admin_api == new.admin_api
        && previous.log == new.log
        && previous.access_log == new.access_log
        && previous.tls == new.tls
        && previous.listeners == new.listeners
}
