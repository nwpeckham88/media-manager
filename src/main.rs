mod api;
mod audit_store;
mod auth;
mod config;
mod db_migrations;
mod domain;
mod jobs_store;
mod operations;
mod path_policy;
mod preflight;
mod scanner;
mod sidecar_store;
mod sidecar_workflow;
#[cfg(test)]
mod sidecar_workflow_tests;
mod toolchain;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use api::routes::{AppState, router};
use audit_store::AuditStore;
use axum::Router;
use axum::response::Html;
use axum::routing::get;
use jobs_store::JobsStore;
use operations::OperationLog;
use tokio::net::TcpListener;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};
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

    if let Err(err) = std::fs::create_dir_all(&config.state_dir) {
        error!(
            "failed to create state dir {}: {err}",
            config.state_dir.display()
        );
        std::process::exit(1);
    }

    if let Err(err) = db_migrations::run(&config.audit_db_path) {
        error!(
            "failed to apply database migrations for {}: {err}",
            config.audit_db_path.display()
        );
        std::process::exit(1);
    }

    let audit_store = match AuditStore::open(&config.audit_db_path) {
        Ok(store) => store,
        Err(err) => {
            error!("failed to initialize audit database: {err}");
            std::process::exit(1);
        }
    };

    let jobs_store = match JobsStore::open(&config.audit_db_path) {
        Ok(store) => store,
        Err(err) => {
            error!("failed to initialize jobs database: {err}");
            std::process::exit(1);
        }
    };

    let state = Arc::new(AppState {
        branding: config.branding.clone(),
        toolchain,
        library_roots: config.library_roots.clone(),
        state_dir: config.state_dir.clone(),
        audit_db_path: config.audit_db_path.clone(),
        api_token: config.api_token.clone(),
        operation_log: OperationLog::new(),
        audit_store,
        jobs_store,
    });

    let cors = CorsLayer::new()
        .allow_methods(Any)
        .allow_headers(Any)
        .allow_origin(Any);

    let app = router(state)
        .merge(frontend_router())
        .layer(TraceLayer::new_for_http())
        .layer(cors);
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

fn frontend_router() -> Router {
    let build_dir = PathBuf::from("frontend/build");
    if build_dir.exists() {
        let index_file = build_dir.join("index.html");
        return Router::new().fallback_service(
            ServeDir::new(build_dir).not_found_service(ServeFile::new(index_file)),
        );
    }

    Router::new().route("/", get(frontend_not_built))
}

async fn frontend_not_built() -> Html<&'static str> {
    Html(
        "<h1>Frontend not built yet</h1><p>Run <code>cd frontend && pnpm build</code> to generate static assets.</p>",
    )
}
