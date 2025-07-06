use crate::config::{AccessLog, GatewayLog, LogFormat};
use std::fs::File;
use tracing_subscriber::Layer;
use tracing_subscriber::filter::filter_fn;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Registry};

pub(crate) fn init_logger(gateway_log_config: &GatewayLog, access_log_config: &AccessLog) {
    let mut layers = vec![];

    let gateway_log_stream = if gateway_log_config.file_path.as_str() == "stdout" {
        None
    } else {
        Some(gateway_log_config.file_path.as_str())
    };
    let gateway_log_layer = build_layer(gateway_log_stream, &gateway_log_config.format)
        .with_filter(EnvFilter::new(&gateway_log_config.level))
        .with_filter(filter_fn(|metadata| metadata.target() != "access"))
        .boxed();
    layers.push(gateway_log_layer);

    if access_log_config.enabled {
        let access_log_stream = if access_log_config.file_path.as_str() == "stdout" {
            None
        } else {
            Some(access_log_config.file_path.as_str())
        };
        let access_log_layer = build_layer(access_log_stream, &access_log_config.format)
            .with_filter(filter_fn(|metadata| {
                metadata.target() == "access" && metadata.level() == &tracing::Level::INFO
            }))
            .boxed();
        layers.push(access_log_layer);
    }

    tracing_subscriber::registry().with(layers).init();
}

fn build_layer(
    file_path: Option<&str>,
    log_format: &LogFormat,
) -> Box<dyn Layer<Registry> + Send + Sync> {
    if let Some(path) = file_path {
        let file = File::create(path).expect("Failed to create log file");
        let layer = tracing_subscriber::fmt::layer().with_writer(file);
        match log_format {
            LogFormat::Common => layer.compact().boxed(),
            LogFormat::Json => layer.json().boxed(),
        }
    } else {
        let layer = tracing_subscriber::fmt::layer();
        match log_format {
            LogFormat::Common => layer.compact().boxed(),
            LogFormat::Json => layer.json().boxed(),
        }
    }
}
