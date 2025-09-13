use crate::SharedGatewayState;
use crate::config::{GatewayConfig, reload_config};
use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Serialize;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

const BASE_URL: &str = "/api/v1";

#[derive(Serialize)]
struct APIResponse<T: Serialize> {
    success: bool,
    message: String,
    data: Option<T>,
}

#[derive(Serialize)]
struct AppMetadata {
    version: &'static str,
    api_version: &'static str,
    current_config: GatewayConfig,
}

async fn graceful_shutdown_api_server(cancel_token: CancellationToken) {
    cancel_token.cancelled().await;
    tracing::info!(target: "api", "Gracefully shutting down API Server");
}

pub async fn start_api_server(gateway_state: SharedGatewayState, cancel_token: CancellationToken) {
    let socket_addr = gateway_state
        .load()
        .get_last_applied_config()
        .admin_api
        .addr;
    let api_router = Router::new()
        .route("/", get(get_app_context))
        .route("/reload", post(reload_config_from_file))
        .with_state(gateway_state);

    let app = Router::new().nest(BASE_URL, api_router);

    let listener = TcpListener::bind(socket_addr).await.unwrap();
    tracing::info!(target: "api", "API Server is running on http://{}", listener.local_addr().expect("The address should be valid"));
    axum::serve(listener, app)
        .with_graceful_shutdown(graceful_shutdown_api_server(cancel_token))
        .await
        .unwrap();
}

async fn get_app_context(
    State(gateway_state): State<SharedGatewayState>,
) -> Json<APIResponse<AppMetadata>> {
    let current_state = gateway_state.load();
    let current_config = current_state.get_last_applied_config();
    let data = AppMetadata {
        version: env!("CARGO_PKG_VERSION"),
        api_version: "v1",
        current_config: current_config.clone(),
    };
    Json(APIResponse {
        success: true,
        data: Some(data),
        message: String::from("Context fetched successfully"),
    })
}

async fn reload_config_from_file(
    State(gateway_state): State<SharedGatewayState>,
) -> Json<APIResponse<()>> {
    match reload_config(gateway_state) {
        Ok(()) => Json(APIResponse {
            success: true,
            message: "Config reloaded successfully".to_string(),
            data: None,
        }),
        Err(err) => Json(APIResponse {
            success: false,
            message: err,
            data: None,
        }),
    }
}
