use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    #[default]
    Common,
    Json,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    #[default]
    Http,
    Https,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AddPrefixConfig {
    pub prefix: String,
}

#[derive(Debug, Clone, Deserialize)]
pub enum RateLimitKeySource {
    #[serde(rename = "ip")]
    IP(Option<String>),
    #[serde(rename = "request_header")]
    RequestHeader(String),
}

#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitConfig {
    pub source: RateLimitKeySource,
    pub limit: u32,
    #[serde(with = "humantime_serde")]
    pub period: Duration,
}

#[derive(Debug, Clone, Deserialize)]
pub enum MiddlewareConfig {
    #[serde(rename = "add_prefix")]
    AddPrefix(AddPrefixConfig),
    #[serde(rename = "rate_limit")]
    RateLimit(RateLimitConfig),
}

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub protocol: Protocol,
    pub cert_file: Option<String>,
    pub key_file: Option<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            host: default_host(),
            port: default_port(),
            protocol: Protocol::default(),
            cert_file: None,
            key_file: None,
        }
    }
}

#[derive(Debug, Deserialize)]
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

#[derive(Debug, Deserialize)]
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

#[derive(Debug, Clone, Deserialize)]
pub struct Upstream {
    pub url: String,
    #[serde(default = "default_upstream_weight")]
    pub weight: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RouteConfig {
    pub path: String,
    #[serde(default)]
    pub methods: Vec<String>,
    pub upstream: Vec<Upstream>,
    pub middlewares: Option<Vec<MiddlewareConfig>>,
}

#[derive(Debug, Deserialize)]
pub struct GatewayConfig {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub log: GatewayLog,
    #[serde(default)]
    pub access_log: AccessLog,
    pub routes: Vec<RouteConfig>,
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> u16 {
    8000
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

pub fn load_config(file_path: &str) -> GatewayConfig {
    config::Config::builder()
        .add_source(config::File::with_name(file_path))
        .build()
        .unwrap()
        .try_deserialize()
        .unwrap()
}
