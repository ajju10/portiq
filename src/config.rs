use std::{fs::File, io::Read};

use serde::{Deserialize, Serialize};

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
    pub fn get_upstream_url(&self, path: &str) -> Option<String> {
        if let Some(route) = self.routes.iter().find(|&route| route.path == path) {
            Some(String::from(&route.upstream_url))
        } else {
            None
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
