use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use std::{fs::File, io::Read};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct ServerConfig {
    pub host: String,
    pub port: i32,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct RouteConfig {
    pub path: String,
    pub methods: Vec<String>,
    pub upstream_url: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct GatewayConfig {
    pub server: ServerConfig,
    pub routes: Vec<RouteConfig>,
}

impl GatewayConfig {
    pub fn match_upstream_path(&self, path: &str, method: &str) -> Result<String, StatusCode> {
        println!("Matching path: {path} and method: {method}");
        match self.routes.iter().find(|route| route.path == path) {
            None => Err(StatusCode::NOT_FOUND),
            Some(route) => {
                if route.methods.is_empty() {
                    return Ok(route.upstream_url.clone());
                }

                let method_allowed = route.methods.iter().any(|m| m.eq_ignore_ascii_case(method));
                if method_allowed {
                    Ok(route.upstream_url.clone())
                } else {
                    Err(StatusCode::METHOD_NOT_ALLOWED)
                }
            }
        }
    }
}

pub(crate) fn load_config(file_path: &str) -> GatewayConfig {
    let mut file = File::open(file_path).expect("Config file should exist at path");
    let mut file_content = String::new();
    file.read_to_string(&mut file_content)
        .expect("File content should be valid");
    serde_yaml_ng::from_str(&file_content).unwrap()
}
