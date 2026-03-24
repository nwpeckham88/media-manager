use std::sync::Arc;

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

use crate::config::BrandingConfig;
use crate::toolchain::ToolchainSnapshot;

#[derive(Clone)]
pub struct AppState {
    pub branding: BrandingConfig,
    pub toolchain: ToolchainSnapshot,
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/config/branding", get(branding))
        .route("/api/diagnostics/toolchain", get(toolchain))
        .route("/api/sidecar/example", get(sidecar_example))
        .with_state(state)
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        service: "media-manager".to_string(),
    })
}

async fn branding(State(state): State<Arc<AppState>>) -> Json<BrandingConfig> {
    Json(state.branding.clone())
}

async fn toolchain(State(state): State<Arc<AppState>>) -> Json<ToolchainSnapshot> {
    Json(state.toolchain.clone())
}

async fn sidecar_example() -> Json<crate::domain::sidecar::SidecarState> {
    Json(crate::domain::sidecar::SidecarState::new("example-item-uid"))
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: String,
    service: String,
}
