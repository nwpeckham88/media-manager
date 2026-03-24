mod api;
mod config;
mod domain;
mod toolchain;

use std::net::SocketAddr;
use std::sync::Arc;

use api::routes::{AppState, router};
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let config = config::AppConfig::from_env();

    let toolchain = match toolchain::probe_toolchain(&config.toolchain) {
        Ok(snapshot) => snapshot,
        Err(err) => {
            error!("toolchain probe failed: {err}");
            std::process::exit(1);
        }
    };

    let state = Arc::new(AppState {
        branding: config.branding.clone(),
        toolchain,
    });

    let app = router(state).layer(TraceLayer::new_for_http());
    let addr: SocketAddr = format!("{}:{}", config.server.host, config.server.port)
        .parse()
        .expect("server host/port must form a valid socket address");

    info!("media-manager listening on {addr}");
    let listener = TcpListener::bind(addr)
        .await
        .expect("failed to bind server listener");

    axum::serve(listener, app)
        .await
        .expect("server terminated unexpectedly");
}
