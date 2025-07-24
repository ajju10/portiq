use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub enum LogFormat {
    #[serde(rename = "common")]
    Common,
    #[serde(rename = "json")]
    Json,
}

#[derive(Debug, Deserialize)]
pub enum Protocol {
    #[serde(rename = "http")]
    Http,
    #[serde(rename = "https")]
    Https,
}

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub protocol: Protocol,
    pub cert_file: Option<String>,
    pub key_file: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GatewayLog {
    pub level: String,
    pub format: LogFormat,
    pub file_path: String,
}

#[derive(Debug, Deserialize)]
pub struct AccessLog {
    pub enabled: bool,
    pub format: LogFormat,
    pub file_path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Upstream {
    pub url: String,
    pub weight: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RouteConfig {
    pub path: String,
    pub methods: Vec<String>,
    pub upstream: Vec<Upstream>,
}

#[derive(Debug, Deserialize)]
pub struct GatewayConfig {
    pub server: ServerConfig,
    pub log: GatewayLog,
    pub access_log: AccessLog,
    pub routes: Vec<RouteConfig>,
}

pub fn load_config(file_path: &str) -> GatewayConfig {
    config::Config::builder()
        .add_source(config::File::with_name(file_path))
        .build()
        .unwrap()
        .try_deserialize()
        .unwrap()
}
