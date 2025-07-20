use hyper::StatusCode;
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
pub(crate) struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub protocol: Protocol,
    pub cert_file: Option<String>,
    pub key_file: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GatewayLog {
    pub level: String,
    pub format: LogFormat,
    pub file_path: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AccessLog {
    pub enabled: bool,
    pub format: LogFormat,
    pub file_path: String,
}

#[derive(Debug, PartialEq, Deserialize)]
pub(crate) struct RouteConfig {
    pub path: String,
    pub methods: Vec<String>,
    pub upstream: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct GatewayConfig {
    pub server: ServerConfig,
    pub log: GatewayLog,
    pub access_log: AccessLog,
    pub routes: Vec<RouteConfig>,
}

impl GatewayConfig {
    pub fn match_upstream_path(&self, path: &str, method: &str) -> Result<String, StatusCode> {
        match self.routes.iter().find(|route| route.path == path) {
            None => {
                tracing::warn!("No matching route found for path {path}");
                Err(StatusCode::NOT_FOUND)
            }
            Some(route) => {
                if route.methods.is_empty()
                    || route.methods.iter().any(|m| m.eq_ignore_ascii_case(method))
                {
                    Ok(route.upstream.clone())
                } else {
                    Err(StatusCode::METHOD_NOT_ALLOWED)
                }
            }
        }
    }
}

pub(crate) fn load_config(file_path: &str) -> GatewayConfig {
    config::Config::builder()
        .add_source(config::File::with_name(file_path))
        .build()
        .unwrap()
        .try_deserialize()
        .unwrap()
}
