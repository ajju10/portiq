use crate::config::{AccessLog, GatewayLog, LogFormat};
use std::fs::File;
use tracing::metadata::LevelFilter;
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_subscriber::filter::filter_fn;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer, fmt};

pub fn init_layers(
    gateway_log_config: &GatewayLog,
    access_log_config: &AccessLog,
) -> (WorkerGuard, Option<WorkerGuard>) {
    let mut layers = vec![];

    let (gateway_writer, gateway_guard) = get_log_writer(gateway_log_config.file_path.as_str());
    let writer_layer = fmt::layer().with_writer(gateway_writer);
    let formatted_layer = match gateway_log_config.format {
        LogFormat::Compact => writer_layer.compact().boxed(),
        LogFormat::Json => writer_layer.json().boxed(),
    };
    let gateway_layer = formatted_layer
        .with_filter(EnvFilter::new(&gateway_log_config.level))
        .with_filter(filter_fn(|metadata| metadata.target() != "access"))
        .boxed();

    layers.push(gateway_layer);

    let access_guard = if access_log_config.enabled {
        let (access_writer, access_guard) = get_log_writer(access_log_config.file_path.as_str());
        let writer_layer = fmt::layer().with_writer(access_writer);
        let formatted_layer = match access_log_config.format {
            LogFormat::Compact => writer_layer.compact().boxed(),
            LogFormat::Json => writer_layer.json().boxed(),
        };
        let access_layer = formatted_layer
            .with_filter(LevelFilter::INFO)
            .with_filter(filter_fn(|metadata| metadata.target() == "access"))
            .boxed();

        layers.push(access_layer);
        Some(access_guard)
    } else {
        None
    };

    tracing_subscriber::registry().with(layers).init();

    (gateway_guard, access_guard)
}

fn get_log_writer(file_path: &str) -> (NonBlocking, WorkerGuard) {
    let (writer, guard) = if file_path == "stdout" {
        tracing_appender::non_blocking(std::io::stdout())
    } else {
        let file = File::options()
            .create(true)
            .append(true)
            .open(file_path)
            .expect("Failed to create log file");
        tracing_appender::non_blocking(file)
    };

    (writer, guard)
}
