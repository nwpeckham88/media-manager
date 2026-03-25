use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::middleware;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::audit_store::AuditStore;
use crate::auth;
use crate::config::BrandingConfig;
use crate::domain::sidecar::SidecarState;
use crate::jobs_store::{JobRecord, JobStatus, JobsStore};
use crate::operations::{OperationEvent, OperationKind, OperationLog};
use crate::path_policy;
use crate::preflight::{PreflightReport, run_preflight};
use crate::scanner::ScanSummary;
use crate::sidecar_store;
use crate::sidecar_workflow;
use crate::sidecar_workflow::{SidecarApplyResult, SidecarPlan, SidecarRollbackResult};
use crate::toolchain::ToolchainSnapshot;

const DEFAULT_RECENT_LIMIT: usize = 20;
const MAX_RECENT_LIMIT: usize = 200;

#[derive(Clone)]
pub struct AppState {
    pub branding: BrandingConfig,
    pub toolchain: ToolchainSnapshot,
    pub library_roots: Vec<PathBuf>,
    pub state_dir: PathBuf,
    pub api_token: Option<String>,
    pub operation_log: OperationLog,
    pub audit_store: AuditStore,
    pub jobs_store: JobsStore,
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/config/app", get(app_config))
        .route("/api/config/branding", get(branding))
        .route("/api/diagnostics/toolchain", get(toolchain))
        .route("/api/diagnostics/preflight", get(preflight))
        .route("/api/scan/summary", get(scan_summary))
        .route("/api/operations/recent", get(recent_operations))
        .route("/api/jobs/recent", get(recent_jobs))
        .route("/api/sidecar", get(read_sidecar))
        .route("/api/sidecar/upsert", post(upsert_sidecar))
        .route("/api/sidecar/dry-run", post(sidecar_dry_run))
        .route("/api/sidecar/apply", post(sidecar_apply))
        .route("/api/sidecar/rollback", post(sidecar_rollback))
        .route("/api/sidecar/example", get(sidecar_example))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::api_token_middleware,
        ))
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

async fn app_config(State(state): State<Arc<AppState>>) -> Json<AppConfigResponse> {
    Json(AppConfigResponse {
        library_roots: state
            .library_roots
            .iter()
            .map(|p| p.display().to_string())
            .collect(),
        state_dir: state.state_dir.display().to_string(),
        auth_enabled: state.api_token.is_some(),
    })
}

async fn toolchain(State(state): State<Arc<AppState>>) -> Json<ToolchainSnapshot> {
    Json(state.toolchain.clone())
}

async fn preflight(State(state): State<Arc<AppState>>) -> Json<PreflightReport> {
    Json(run_preflight(&state.library_roots, &state.state_dir, &state.toolchain))
}

async fn scan_summary(State(state): State<Arc<AppState>>) -> Result<Json<ScanSummary>, (StatusCode, String)> {
    let job_id = create_job(&state, "scan_summary", "{}");
    let library_roots = state.library_roots.clone();
    let result = tokio::task::spawn_blocking(move || crate::scanner::scan_library_roots(&library_roots))
        .await
        .map_err(|e| {
            let response = internal_error(e);
            complete_job(&state, job_id, JobStatus::Failed, None, Some(response.1.clone()));
            response
        })?;

    record_event(
        &state,
        OperationKind::ScanSummary,
        format!("roots={} total_media_files={}", result.roots.len(), result.total_media_files),
        true,
    );
    complete_job(
        &state,
        job_id,
        JobStatus::Succeeded,
        serde_json::to_string(&result).ok(),
        None,
    );
    Ok(Json(result))
}

async fn recent_operations(
    State(state): State<Arc<AppState>>,
    Query(query): Query<RecentOpsQuery>,
) -> Json<Vec<OperationEvent>> {
    let limit = normalize_recent_limit(query.limit);
    match state.audit_store.recent_events(limit) {
        Ok(events) => Json(events),
        Err(_) => Json(state.operation_log.recent(limit)),
    }
}

async fn recent_jobs(
    State(state): State<Arc<AppState>>,
    Query(query): Query<RecentOpsQuery>,
) -> Json<Vec<JobRecord>> {
    let limit = normalize_recent_limit(query.limit);
    match state.jobs_store.recent_jobs(limit) {
        Ok(jobs) => Json(jobs),
        Err(_) => Json(Vec::new()),
    }
}

async fn read_sidecar(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SidecarLookupQuery>,
) -> Result<Json<SidecarReadResponse>, (StatusCode, String)> {
    let job_id = create_job(
        &state,
        "sidecar_read",
        &format!("{{\"media_path\":\"{}\"}}", query.media_path),
    );

    let media_path = PathBuf::from(query.media_path);
    if let Err(err) = ensure_media_file_path_allowed(&media_path, &state.library_roots) {
        complete_job(&state, job_id, JobStatus::Failed, None, Some(err.1.clone()));
        return Err(err);
    }

    let sidecar_path = match sidecar_store::sidecar_path_for_media(&media_path) {
        Ok(v) => v,
        Err(err) => {
            let response = internal_error(err);
            complete_job(&state, job_id, JobStatus::Failed, None, Some(response.1.clone()));
            return Err(response);
        }
    };

    let sidecar_state = match sidecar_store::read_sidecar(&media_path) {
        Ok(v) => v,
        Err(err) => {
            let response = internal_error(err);
            complete_job(&state, job_id, JobStatus::Failed, None, Some(response.1.clone()));
            return Err(response);
        }
    };

    record_event(
        &state,
        OperationKind::SidecarRead,
        format!("media_path={}", media_path.display()),
        true,
    );

    complete_job(
        &state,
        job_id,
        JobStatus::Succeeded,
        serde_json::to_string(&sidecar_state).ok(),
        None,
    );

    Ok(Json(SidecarReadResponse {
        sidecar_path: sidecar_path.display().to_string(),
        state: sidecar_state,
    }))
}

async fn upsert_sidecar(
    State(state): State<Arc<AppState>>,
    Json(request): Json<SidecarUpsertRequest>,
) -> Result<Json<SidecarUpsertResponse>, (StatusCode, String)> {
    let job_id = create_job(
        &state,
        "sidecar_upsert",
        &format!(
            "{{\"media_path\":\"{}\",\"item_uid\":\"{}\"}}",
            request.media_path, request.item_uid
        ),
    );

    let media_path = PathBuf::from(request.media_path);
    if let Err(err) = ensure_media_file_path_allowed(&media_path, &state.library_roots) {
        complete_job(&state, job_id, JobStatus::Failed, None, Some(err.1.clone()));
        return Err(err);
    }

    if let Err(err) = ensure_preflight_ready(&state) {
        complete_job(&state, job_id, JobStatus::Failed, None, Some(err.1.clone()));
        return Err(err);
    }

    let existing = match sidecar_store::read_sidecar(&media_path) {
        Ok(v) => v,
        Err(err) => {
            let response = internal_error(err);
            complete_job(&state, job_id, JobStatus::Failed, None, Some(response.1.clone()));
            return Err(response);
        }
    };

    let mut sidecar_state = existing.unwrap_or_else(|| SidecarState::new(request.item_uid.clone()));
    sidecar_state.item_uid = request.item_uid;

    let sidecar_path = match sidecar_store::write_sidecar(&media_path, &sidecar_state) {
        Ok(v) => v,
        Err(err) => {
            let response = internal_error(err);
            complete_job(&state, job_id, JobStatus::Failed, None, Some(response.1.clone()));
            return Err(response);
        }
    };

    record_event(
        &state,
        OperationKind::SidecarUpsert,
        format!("media_path={} sidecar_path={}", media_path.display(), sidecar_path.display()),
        true,
    );

    complete_job(
        &state,
        job_id,
        JobStatus::Succeeded,
        serde_json::to_string(&sidecar_state).ok(),
        None,
    );

    Ok(Json(SidecarUpsertResponse {
        sidecar_path: sidecar_path.display().to_string(),
        state: sidecar_state,
    }))
}

async fn sidecar_dry_run(
    State(state): State<Arc<AppState>>,
    Json(request): Json<SidecarUpsertRequest>,
) -> Result<Json<SidecarDryRunResponse>, (StatusCode, String)> {
    let job_id = create_job(
        &state,
        "sidecar_dry_run",
        &format!(
            "{{\"media_path\":\"{}\",\"item_uid\":\"{}\"}}",
            request.media_path, request.item_uid
        ),
    );

    let media_path = PathBuf::from(request.media_path);
    if let Err(err) = ensure_media_file_path_allowed(&media_path, &state.library_roots) {
        complete_job(&state, job_id, JobStatus::Failed, None, Some(err.1.clone()));
        return Err(err);
    }

    let plan = match sidecar_workflow::build_plan(&media_path, &request.item_uid) {
        Ok(v) => v,
        Err(err) => {
            let response = internal_error(err);
            complete_job(&state, job_id, JobStatus::Failed, None, Some(response.1.clone()));
            return Err(response);
        }
    };
    record_event(
        &state,
        OperationKind::SidecarRead,
        format!("dry_run media_path={} plan_hash={}", media_path.display(), plan.plan_hash),
        true,
    );

    complete_job(
        &state,
        job_id,
        JobStatus::Succeeded,
        serde_json::to_string(&plan).ok(),
        None,
    );

    Ok(Json(SidecarDryRunResponse { plan }))
}

async fn sidecar_apply(
    State(state): State<Arc<AppState>>,
    Json(request): Json<SidecarApplyRequest>,
) -> Result<Json<SidecarApplyResult>, (StatusCode, String)> {
    let job_id = create_job(
        &state,
        "sidecar_apply",
        &format!(
            "{{\"media_path\":\"{}\",\"item_uid\":\"{}\",\"plan_hash\":\"{}\"}}",
            request.media_path, request.item_uid, request.plan_hash
        ),
    );

    let media_path = PathBuf::from(request.media_path);
    if let Err(err) = ensure_media_file_path_allowed(&media_path, &state.library_roots) {
        complete_job(&state, job_id, JobStatus::Failed, None, Some(err.1.clone()));
        return Err(err);
    }

    if let Err(err) = ensure_preflight_ready(&state) {
        complete_job(&state, job_id, JobStatus::Failed, None, Some(err.1.clone()));
        return Err(err);
    }

    let result = sidecar_workflow::apply_plan(
        &media_path,
        &request.item_uid,
        &request.plan_hash,
        &state.state_dir,
    )
    .map_err(|e| match e {
        sidecar_workflow::SidecarWorkflowError::PlanMismatch => {
            (StatusCode::CONFLICT, e.to_string())
        }
        _ => internal_error(e),
    });

    let result = match result {
        Ok(v) => v,
        Err(err) => {
            complete_job(&state, job_id, JobStatus::Failed, None, Some(err.1.clone()));
            return Err(err);
        }
    };

    record_event(
        &state,
        OperationKind::SidecarUpsert,
        format!(
            "apply media_path={} operation_id={}",
            media_path.display(),
            result.operation_id
        ),
        true,
    );

    complete_job(
        &state,
        job_id,
        JobStatus::Succeeded,
        serde_json::to_string(&result).ok(),
        None,
    );

    Ok(Json(result))
}

async fn sidecar_rollback(
    State(state): State<Arc<AppState>>,
    Json(request): Json<SidecarRollbackRequest>,
) -> Result<Json<SidecarRollbackResult>, (StatusCode, String)> {
    let job_id = create_job(
        &state,
        "sidecar_rollback",
        &format!("{{\"operation_id\":\"{}\"}}", request.operation_id),
    );

    if let Err(err) = ensure_preflight_ready(&state) {
        complete_job(&state, job_id, JobStatus::Failed, None, Some(err.1.clone()));
        return Err(err);
    }

    let result = sidecar_workflow::rollback_operation(&request.operation_id, &state.state_dir)
        .map_err(|e| match e {
            sidecar_workflow::SidecarWorkflowError::RollbackSnapshotMissing(_) => {
                (StatusCode::NOT_FOUND, e.to_string())
            }
            _ => internal_error(e),
        });

    let result = match result {
        Ok(v) => v,
        Err(err) => {
            complete_job(&state, job_id, JobStatus::Failed, None, Some(err.1.clone()));
            return Err(err);
        }
    };

    record_event(
        &state,
        OperationKind::SidecarUpsert,
        format!("rollback operation_id={}", result.operation_id),
        true,
    );

    complete_job(
        &state,
        job_id,
        JobStatus::Succeeded,
        serde_json::to_string(&result).ok(),
        None,
    );

    Ok(Json(result))
}

async fn sidecar_example() -> Json<crate::domain::sidecar::SidecarState> {
    Json(crate::domain::sidecar::SidecarState::new("example-item-uid"))
}

fn internal_error(error: impl ToString) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

fn create_job(state: &AppState, kind: &str, payload_json: &str) -> Option<i64> {
    state
        .jobs_store
        .create_job(kind, payload_json, current_timestamp_ms())
        .ok()
}

fn complete_job(
    state: &AppState,
    job_id: Option<i64>,
    status: JobStatus,
    result_json: Option<String>,
    error: Option<String>,
) {
    let Some(id) = job_id else {
        return;
    };

    let _ = state.jobs_store.complete_job(
        id,
        status,
        result_json.as_deref(),
        error.as_deref(),
        current_timestamp_ms(),
    );
}

fn record_event(state: &AppState, kind: OperationKind, detail: String, success: bool) {
    let event = OperationEvent {
        timestamp_ms: current_timestamp_ms(),
        kind,
        detail,
        success,
    };
    state.operation_log.push(event.kind.clone(), event.detail.clone(), event.success);
    let _ = state.audit_store.insert_event(&event);
}

fn current_timestamp_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn normalize_recent_limit(limit: Option<usize>) -> usize {
    limit
        .unwrap_or(DEFAULT_RECENT_LIMIT)
        .min(MAX_RECENT_LIMIT)
}

fn ensure_media_file_path_allowed(
    media_path: &PathBuf,
    library_roots: &[PathBuf],
) -> Result<(), (StatusCode, String)> {
    if library_roots.is_empty() {
        return Err((
            StatusCode::FAILED_DEPENDENCY,
            "MM_LIBRARY_ROOTS is not configured".to_string(),
        ));
    }

    if !media_path.exists() {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("media path does not exist: {}", media_path.display()),
        ));
    }

    if !media_path.is_file() {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("media path is not a file: {}", media_path.display()),
        ));
    }

    if !path_policy::is_path_within_roots(media_path, library_roots) {
        return Err((
            StatusCode::FORBIDDEN,
            format!(
                "media path is outside configured library roots: {}",
                media_path.display()
            ),
        ));
    }

    Ok(())
}

fn ensure_preflight_ready(state: &AppState) -> Result<(), (StatusCode, String)> {
    let report = run_preflight(&state.library_roots, &state.state_dir, &state.toolchain);
    if report.ready {
        return Ok(());
    }

    Err((
        StatusCode::FAILED_DEPENDENCY,
        "preflight is not ready; fix diagnostics before mutating operations".to_string(),
    ))
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: String,
    service: String,
}

#[derive(Debug, Serialize)]
struct AppConfigResponse {
    library_roots: Vec<String>,
    state_dir: String,
    auth_enabled: bool,
}

#[derive(Debug, Deserialize)]
struct SidecarLookupQuery {
    media_path: String,
}

#[derive(Debug, Deserialize)]
struct RecentOpsQuery {
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct SidecarUpsertRequest {
    media_path: String,
    item_uid: String,
}

#[derive(Debug, Deserialize)]
struct SidecarApplyRequest {
    media_path: String,
    item_uid: String,
    plan_hash: String,
}

#[derive(Debug, Deserialize)]
struct SidecarRollbackRequest {
    operation_id: String,
}

#[derive(Debug, Serialize)]
struct SidecarReadResponse {
    sidecar_path: String,
    state: Option<SidecarState>,
}

#[derive(Debug, Serialize)]
struct SidecarUpsertResponse {
    sidecar_path: String,
    state: SidecarState,
}

#[derive(Debug, Serialize)]
struct SidecarDryRunResponse {
    plan: SidecarPlan,
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use axum::http::StatusCode;

    use super::{
        DEFAULT_RECENT_LIMIT, MAX_RECENT_LIMIT, ensure_media_file_path_allowed, normalize_recent_limit,
    };

    fn unique_temp_dir(name: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        dir.push(format!("mm-routes-{name}-{nanos}"));
        dir
    }

    #[test]
    fn recent_limit_defaults_and_caps() {
        assert_eq!(normalize_recent_limit(None), DEFAULT_RECENT_LIMIT);
        assert_eq!(normalize_recent_limit(Some(7)), 7);
        assert_eq!(normalize_recent_limit(Some(MAX_RECENT_LIMIT + 10)), MAX_RECENT_LIMIT);
    }

    #[test]
    fn media_path_must_be_file_within_root() {
        let root = unique_temp_dir("media-file-policy");
        let library = root.join("library");
        let outside = root.join("outside");
        fs::create_dir_all(&library).expect("create library");
        fs::create_dir_all(&outside).expect("create outside");

        let file_in_root = library.join("movie.mkv");
        let dir_in_root = library.join("series");
        fs::write(&file_in_root, b"x").expect("create media file");
        fs::create_dir_all(&dir_in_root).expect("create nested dir");

        let file_outside = outside.join("movie.mkv");
        fs::write(&file_outside, b"x").expect("create outside file");

        let roots = vec![library.clone()];
        assert!(ensure_media_file_path_allowed(&file_in_root, &roots).is_ok());

        let dir_err = ensure_media_file_path_allowed(&dir_in_root, &roots).expect_err("dir should fail");
        assert_eq!(dir_err.0, StatusCode::BAD_REQUEST);

        let outside_err = ensure_media_file_path_allowed(&file_outside, &roots)
            .expect_err("outside root should fail");
        assert_eq!(outside_err.0, StatusCode::FORBIDDEN);

        fs::remove_dir_all(root).expect("cleanup root");
    }
}
