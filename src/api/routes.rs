use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::{
    collections::HashMap,
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
};

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::middleware;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::audit_store::AuditStore;
use crate::auth;
use crate::config::BrandingConfig;
use crate::domain::sidecar::SidecarState;
use crate::jobs_store::{JobRecord, JobStatus, JobsStore};
use crate::operations::{OperationEvent, OperationKind, OperationLog};
use crate::path_policy;
use crate::preflight::{PreflightReport, run_preflight};
use crate::scanner::{LibraryBrowseOptions, LibraryBrowseResult, ScanSummary};
use crate::sidecar_store;
use crate::sidecar_workflow;
use crate::sidecar_workflow::{SidecarApplyResult, SidecarPlan, SidecarRollbackResult};
use crate::toolchain::ToolchainSnapshot;

const DEFAULT_RECENT_LIMIT: usize = 20;
const MAX_RECENT_LIMIT: usize = 200;
const DEFAULT_LIBRARY_LIMIT: usize = 120;
const MAX_LIBRARY_LIMIT: usize = 500;
static FS_OPERATION_NONCE: AtomicU64 = AtomicU64::new(0);

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
        .route("/api/library/items", get(library_items))
        .route("/api/operations/recent", get(recent_operations))
        .route("/api/jobs/recent", get(recent_jobs))
        .route("/api/jobs/cancel", post(cancel_job))
        .route("/api/jobs/retry", post(retry_job))
        .route("/api/sidecar", get(read_sidecar))
        .route("/api/sidecar/upsert", post(upsert_sidecar))
        .route("/api/sidecar/dry-run", post(sidecar_dry_run))
        .route("/api/sidecar/apply", post(sidecar_apply))
        .route("/api/sidecar/rollback", post(sidecar_rollback))
        .route("/api/sidecar/example", get(sidecar_example))
        .route("/api/bulk/dry-run", post(bulk_dry_run))
        .route("/api/bulk/apply", post(bulk_apply))
        .route("/api/bulk/rollback", post(bulk_rollback))
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
    Json(run_preflight(
        &state.library_roots,
        &state.state_dir,
        &state.toolchain,
    ))
}

async fn scan_summary(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ScanSummary>, (StatusCode, String)> {
    let job_id = create_job(&state, "scan_summary", "{}");
    let library_roots = state.library_roots.clone();
    let result =
        tokio::task::spawn_blocking(move || crate::scanner::scan_library_roots(&library_roots))
            .await
            .map_err(|e| {
                let response = internal_error(e);
                complete_job(
                    &state,
                    job_id,
                    JobStatus::Failed,
                    None,
                    Some(response.1.clone()),
                );
                response
            })?;

    record_event(
        &state,
        OperationKind::ScanSummary,
        format!(
            "roots={} total_media_files={}",
            result.roots.len(),
            result.total_media_files
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

async fn library_items(
    State(state): State<Arc<AppState>>,
    Query(query): Query<LibraryItemsQuery>,
) -> Result<Json<LibraryBrowseResult>, (StatusCode, String)> {
    let root_index = query.root_index;
    let search_query = query.q.clone();
    if let Some(idx) = root_index {
        if idx >= state.library_roots.len() {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("root_index {idx} is out of bounds"),
            ));
        }
    }

    let job_id = create_job(
        &state,
        "library_items",
        &format!(
            "{{\"root_index\":{},\"q\":\"{}\",\"offset\":{},\"limit\":{}}}",
            root_index
                .map(|v| v.to_string())
                .unwrap_or_else(|| "null".to_string()),
            search_query.clone().unwrap_or_default(),
            query.offset.unwrap_or(0),
            normalize_library_limit(query.limit)
        ),
    );

    let library_roots = state.library_roots.clone();
    let options = LibraryBrowseOptions {
        root_index,
        query: search_query.clone(),
        offset: query.offset.unwrap_or(0),
        limit: normalize_library_limit(query.limit),
    };
    let result = tokio::task::spawn_blocking(move || {
        crate::scanner::list_library_media(&library_roots, options)
    })
    .await
    .map_err(|e| {
        let response = internal_error(e);
        complete_job(
            &state,
            job_id,
            JobStatus::Failed,
            None,
            Some(response.1.clone()),
        );
        response
    })?;

    let result = match result {
        Ok(v) => v,
        Err(err) => {
            let response = (StatusCode::BAD_REQUEST, err);
            complete_job(
                &state,
                job_id,
                JobStatus::Failed,
                None,
                Some(response.1.clone()),
            );
            return Err(response);
        }
    };

    record_event(
        &state,
        OperationKind::LibraryBrowse,
        format!(
            "root_index={:?} q={} offset={} limit={} returned={} total_matches={}",
            root_index,
            search_query.unwrap_or_default(),
            result.offset,
            result.limit,
            result.items.len(),
            result.total_matches
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
    Query(query): Query<RecentJobsQuery>,
) -> Json<RecentJobsResponse> {
    let limit = normalize_recent_limit(query.limit);
    let offset = query.offset.unwrap_or(0);
    let status = normalize_job_status_filter(query.status.as_deref());
    let kind = query
        .kind
        .as_deref()
        .map(|v| v.trim())
        .filter(|v| !v.is_empty());
    let bulk_only = query.bulk_only.unwrap_or(false);
    let items = match state.jobs_store.recent_jobs_filtered(
        status.as_deref(),
        kind,
        bulk_only,
        offset,
        limit,
    ) {
        Ok(jobs) => jobs,
        Err(_) => Vec::new(),
    };

    let total_count = state
        .jobs_store
        .count_jobs_filtered(status.as_deref(), kind, bulk_only)
        .unwrap_or(0);

    Json(RecentJobsResponse {
        total_count,
        offset,
        limit,
        items,
    })
}

async fn cancel_job(
    State(state): State<Arc<AppState>>,
    Json(request): Json<JobActionRequest>,
) -> Result<Json<JobActionResponse>, (StatusCode, String)> {
    let Some(job) = state
        .jobs_store
        .get_job(request.job_id)
        .map_err(internal_error)?
    else {
        return Err((
            StatusCode::NOT_FOUND,
            format!("job {} was not found", request.job_id),
        ));
    };

    if !job.kind.starts_with("bulk_") {
        return Err((
            StatusCode::BAD_REQUEST,
            "only bulk jobs can be cancelled from queue".to_string(),
        ));
    }

    if !matches!(job.status, JobStatus::Running) {
        return Err((
            StatusCode::CONFLICT,
            "job is not running and cannot be cancelled".to_string(),
        ));
    }

    state
        .jobs_store
        .set_job_status(
            request.job_id,
            JobStatus::Canceled,
            Some("cancelled by operator"),
            current_timestamp_ms(),
        )
        .map_err(internal_error)?;

    record_event(
        &state,
        OperationKind::JobControl,
        format!("cancel_job id={}", request.job_id),
        true,
    );

    Ok(Json(JobActionResponse {
        job_id: request.job_id,
        ok: true,
        message: "job cancelled".to_string(),
        retried_job_id: None,
    }))
}

async fn retry_job(
    State(state): State<Arc<AppState>>,
    Json(request): Json<JobActionRequest>,
) -> Result<Json<JobActionResponse>, (StatusCode, String)> {
    let Some(job) = state
        .jobs_store
        .get_job(request.job_id)
        .map_err(internal_error)?
    else {
        return Err((
            StatusCode::NOT_FOUND,
            format!("job {} was not found", request.job_id),
        ));
    };

    if !job.kind.starts_with("bulk_") {
        return Err((
            StatusCode::BAD_REQUEST,
            "only bulk jobs can be retried from queue".to_string(),
        ));
    }

    if matches!(job.status, JobStatus::Running) {
        return Err((
            StatusCode::CONFLICT,
            "running jobs cannot be retried".to_string(),
        ));
    }

    let retried_job_id = create_job(&state, &job.kind, &job.payload_json);
    let Some(new_id) = retried_job_id else {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to create retry job record".to_string(),
        ));
    };

    if job.kind == "bulk_dry_run" {
        let retry_request: BulkDryRunRequest =
            serde_json::from_str(&job.payload_json).map_err(|_| {
                (
                    StatusCode::BAD_REQUEST,
                    "job payload is not a valid bulk_dry_run request".to_string(),
                )
            })?;

        match execute_bulk_dry_run(&state, &retry_request) {
            Ok(response) => {
                complete_job(
                    &state,
                    Some(new_id),
                    JobStatus::Succeeded,
                    serde_json::to_string(&response).ok(),
                    None,
                );
            }
            Err((status, err)) => {
                complete_job(
                    &state,
                    Some(new_id),
                    JobStatus::Failed,
                    None,
                    Some(err.clone()),
                );
                return Err((status, err));
            }
        }
    } else if job.kind == "bulk_apply" {
        let retry_request: BulkApplyRequest =
            serde_json::from_str(&job.payload_json).map_err(|_| {
                (
                    StatusCode::BAD_REQUEST,
                    "job payload is not a valid bulk_apply request".to_string(),
                )
            })?;

        match execute_bulk_apply(&state, &retry_request) {
            Ok(response) => {
                complete_job(
                    &state,
                    Some(new_id),
                    if response.failed == 0 {
                        JobStatus::Succeeded
                    } else {
                        JobStatus::Failed
                    },
                    serde_json::to_string(&response).ok(),
                    None,
                );
            }
            Err((status, err)) => {
                complete_job(
                    &state,
                    Some(new_id),
                    JobStatus::Failed,
                    None,
                    Some(err.clone()),
                );
                return Err((status, err));
            }
        }
    } else {
        complete_job(
            &state,
            Some(new_id),
            JobStatus::Failed,
            None,
            Some("retry not supported for this job kind".to_string()),
        );
        return Err((
            StatusCode::BAD_REQUEST,
            "retry is only supported for bulk_dry_run and bulk_apply".to_string(),
        ));
    }

    record_event(
        &state,
        OperationKind::JobControl,
        format!("retry_job source_id={} new_id={}", request.job_id, new_id),
        true,
    );

    Ok(Json(JobActionResponse {
        job_id: request.job_id,
        ok: true,
        message: "job retried".to_string(),
        retried_job_id: Some(new_id),
    }))
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
            complete_job(
                &state,
                job_id,
                JobStatus::Failed,
                None,
                Some(response.1.clone()),
            );
            return Err(response);
        }
    };

    let sidecar_state = match sidecar_store::read_sidecar(&media_path) {
        Ok(v) => v,
        Err(err) => {
            let response = internal_error(err);
            complete_job(
                &state,
                job_id,
                JobStatus::Failed,
                None,
                Some(response.1.clone()),
            );
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
            complete_job(
                &state,
                job_id,
                JobStatus::Failed,
                None,
                Some(response.1.clone()),
            );
            return Err(response);
        }
    };

    let mut sidecar_state = existing.unwrap_or_else(|| SidecarState::new(request.item_uid.clone()));
    sidecar_state.item_uid = request.item_uid;

    let sidecar_path = match sidecar_store::write_sidecar(&media_path, &sidecar_state) {
        Ok(v) => v,
        Err(err) => {
            let response = internal_error(err);
            complete_job(
                &state,
                job_id,
                JobStatus::Failed,
                None,
                Some(response.1.clone()),
            );
            return Err(response);
        }
    };

    record_event(
        &state,
        OperationKind::SidecarUpsert,
        format!(
            "media_path={} sidecar_path={}",
            media_path.display(),
            sidecar_path.display()
        ),
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
            complete_job(
                &state,
                job_id,
                JobStatus::Failed,
                None,
                Some(response.1.clone()),
            );
            return Err(response);
        }
    };
    record_event(
        &state,
        OperationKind::SidecarRead,
        format!(
            "dry_run media_path={} plan_hash={}",
            media_path.display(),
            plan.plan_hash
        ),
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
    Json(crate::domain::sidecar::SidecarState::new(
        "example-item-uid",
    ))
}

async fn bulk_dry_run(
    State(state): State<Arc<AppState>>,
    Json(request): Json<BulkDryRunRequest>,
) -> Result<Json<BulkDryRunResponse>, (StatusCode, String)> {
    let job_id = create_job(
        &state,
        "bulk_dry_run",
        &serde_json::to_string(&request)
            .unwrap_or_else(|_| "{\"error\":\"failed_to_encode_request\"}".to_string()),
    );

    let response = execute_bulk_dry_run(&state, &request);
    let response = match response {
        Ok(v) => v,
        Err(err) => {
            complete_job(&state, job_id, JobStatus::Failed, None, Some(err.1.clone()));
            return Err(err);
        }
    };

    record_event(
        &state,
        OperationKind::SidecarRead,
        format!(
            "bulk_dry_run action={} items={} ready={}",
            response.action, response.total_items, response.plan_ready
        ),
        true,
    );

    complete_job(
        &state,
        job_id,
        JobStatus::Succeeded,
        serde_json::to_string(&response).ok(),
        None,
    );

    Ok(Json(response))
}

async fn bulk_apply(
    State(state): State<Arc<AppState>>,
    Json(request): Json<BulkApplyRequest>,
) -> Result<Json<BulkApplyResponse>, (StatusCode, String)> {
    let job_id = create_job(
        &state,
        "bulk_apply",
        &serde_json::to_string(&request)
            .unwrap_or_else(|_| "{\"error\":\"failed_to_encode_request\"}".to_string()),
    );

    let response = execute_bulk_apply(&state, &request);
    let response = match response {
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
            "bulk_apply action={} total={} succeeded={} failed={}",
            response.action, response.total_items, response.succeeded, response.failed
        ),
        response.failed == 0,
    );

    complete_job(
        &state,
        job_id,
        if response.failed == 0 {
            JobStatus::Succeeded
        } else {
            JobStatus::Failed
        },
        serde_json::to_string(&response).ok(),
        None,
    );

    Ok(Json(response))
}

async fn bulk_rollback(
    State(state): State<Arc<AppState>>,
    Json(request): Json<BulkRollbackRequest>,
) -> Result<Json<BulkRollbackResponse>, (StatusCode, String)> {
    if request.operation_ids.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "operation_ids must not be empty".to_string(),
        ));
    }

    let job_id = create_job(
        &state,
        "bulk_rollback",
        &serde_json::to_string(&request)
            .unwrap_or_else(|_| "{\"error\":\"failed_to_encode_request\"}".to_string()),
    );

    if let Err(err) = ensure_preflight_ready(&state) {
        complete_job(&state, job_id, JobStatus::Failed, None, Some(err.1.clone()));
        return Err(err);
    }

    let mut results = Vec::with_capacity(request.operation_ids.len());
    for operation_id in request.operation_ids {
        let restored = rollback_bulk_operation(&state, &operation_id);
        match restored {
            Ok(detail) => results.push(BulkRollbackItemResult {
                operation_id,
                success: true,
                detail: Some(detail),
                error: None,
            }),
            Err(err) => results.push(BulkRollbackItemResult {
                operation_id,
                success: false,
                detail: None,
                error: Some(err),
            }),
        }
    }

    let succeeded = results.iter().filter(|v| v.success).count();
    let failed = results.len().saturating_sub(succeeded);
    let response = BulkRollbackResponse {
        total_items: results.len(),
        succeeded,
        failed,
        items: results,
    };

    record_event(
        &state,
        OperationKind::JobControl,
        format!(
            "bulk_rollback total={} succeeded={} failed={}",
            response.total_items, response.succeeded, response.failed
        ),
        response.failed == 0,
    );

    complete_job(
        &state,
        job_id,
        if response.failed == 0 {
            JobStatus::Succeeded
        } else {
            JobStatus::Failed
        },
        serde_json::to_string(&response).ok(),
        None,
    );

    Ok(Json(response))
}

fn execute_bulk_dry_run(
    state: &AppState,
    request: &BulkDryRunRequest,
) -> Result<BulkDryRunResponse, (StatusCode, String)> {
    if request.items.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "items must not be empty".to_string(),
        ));
    }

    build_bulk_preview(state, &request.action, &request.items)
}

fn execute_bulk_apply(
    state: &AppState,
    request: &BulkApplyRequest,
) -> Result<BulkApplyResponse, (StatusCode, String)> {
    if request.items.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "items must not be empty".to_string(),
        ));
    }

    ensure_preflight_ready(state)?;

    let preview = build_bulk_preview(state, &request.action, &request.items)?;
    if preview.batch_hash != request.approved_batch_hash {
        return Err((
            StatusCode::CONFLICT,
            "approved_batch_hash does not match current dry-run batch hash".to_string(),
        ));
    }

    let mut applied_items: Vec<BulkApplyItemResult> = Vec::with_capacity(preview.items.len());
    for item in preview.items {
        if !item.can_apply {
            applied_items.push(BulkApplyItemResult {
                media_path: item.media_path,
                final_media_path: None,
                item_uid: item.item_uid,
                applied_provider_id: None,
                success: false,
                operation_id: None,
                sidecar_path: None,
                error: Some(
                    item.note
                        .unwrap_or_else(|| "item is not applicable".to_string()),
                ),
            });
            continue;
        }

        if request.action == "rename" {
            let Some(target_path_value) = item.proposed_media_path.clone() else {
                applied_items.push(BulkApplyItemResult {
                    media_path: item.media_path,
                    final_media_path: None,
                    item_uid: item.item_uid,
                    applied_provider_id: None,
                    success: false,
                    operation_id: None,
                    sidecar_path: None,
                    error: Some("rename preview did not include target path".to_string()),
                });
                continue;
            };

            let source_path = PathBuf::from(&item.media_path);
            let target_path = PathBuf::from(&target_path_value);

            if source_path == target_path {
                applied_items.push(BulkApplyItemResult {
                    media_path: item.media_path,
                    final_media_path: Some(target_path_value),
                    item_uid: item.item_uid,
                    applied_provider_id: None,
                    success: true,
                    operation_id: None,
                    sidecar_path: None,
                    error: None,
                });
                continue;
            }

            if target_path.exists() {
                applied_items.push(BulkApplyItemResult {
                    media_path: item.media_path,
                    final_media_path: Some(target_path_value),
                    item_uid: item.item_uid,
                    applied_provider_id: None,
                    success: false,
                    operation_id: None,
                    sidecar_path: None,
                    error: Some("target rename path already exists".to_string()),
                });
                continue;
            }

            match apply_rename_with_rollback(&state.state_dir, &source_path, &target_path) {
                Ok(operation_id) => applied_items.push(BulkApplyItemResult {
                    media_path: item.media_path,
                    final_media_path: Some(target_path_value),
                    item_uid: item.item_uid,
                    applied_provider_id: None,
                    success: true,
                    operation_id: Some(operation_id),
                    sidecar_path: None,
                    error: None,
                }),
                Err(err) => applied_items.push(BulkApplyItemResult {
                    media_path: item.media_path,
                    final_media_path: Some(target_path_value),
                    item_uid: item.item_uid,
                    applied_provider_id: None,
                    success: false,
                    operation_id: None,
                    sidecar_path: None,
                    error: Some(err),
                }),
            }

            continue;
        }

        if request.action == "validate_nfo" {
            let Some(target_nfo_path) = item.proposed_media_path.clone() else {
                applied_items.push(BulkApplyItemResult {
                    media_path: item.media_path,
                    final_media_path: None,
                    item_uid: item.item_uid,
                    applied_provider_id: None,
                    success: false,
                    operation_id: None,
                    sidecar_path: None,
                    error: Some("nfo preview did not include target path".to_string()),
                });
                continue;
            };

            let target_path = PathBuf::from(&target_nfo_path);
            if target_path.exists() {
                let existing = fs::read_to_string(&target_path).unwrap_or_default();
                if !existing.trim().is_empty() {
                    applied_items.push(BulkApplyItemResult {
                        media_path: item.media_path,
                        final_media_path: Some(target_nfo_path),
                        item_uid: item.item_uid,
                        applied_provider_id: None,
                        success: true,
                        operation_id: None,
                        sidecar_path: None,
                        error: None,
                    });
                    continue;
                }
            }

            let content = generate_nfo_content(&item.media_path, &item.item_uid);
            match apply_write_text_with_rollback(&state.state_dir, &target_path, &content) {
                Ok(operation_id) => applied_items.push(BulkApplyItemResult {
                    media_path: item.media_path,
                    final_media_path: Some(target_nfo_path),
                    item_uid: item.item_uid,
                    applied_provider_id: None,
                    success: true,
                    operation_id: Some(operation_id),
                    sidecar_path: None,
                    error: None,
                }),
                Err(err) => applied_items.push(BulkApplyItemResult {
                    media_path: item.media_path,
                    final_media_path: Some(target_nfo_path),
                    item_uid: item.item_uid,
                    applied_provider_id: None,
                    success: false,
                    operation_id: None,
                    sidecar_path: None,
                    error: Some(err),
                }),
            }

            continue;
        }

        if request.action == "metadata_lookup" {
            let media_path = PathBuf::from(&item.media_path);
            let metadata_title = item
                .metadata_title
                .clone()
                .unwrap_or_else(|| item.item_uid.clone());
            let metadata_year = item.metadata_year;
            let provider_id = item.proposed_provider_id.clone().unwrap_or_else(|| {
                infer_metadata_candidate(&media_path, &item.item_uid, None).provider_id
            });
            let metadata_confidence = item.metadata_confidence.unwrap_or(0.5_f32);
            let existing = sidecar_store::read_sidecar(&media_path)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()));

            let mut sidecar_state = match existing {
                Ok(Some(state_value)) => state_value,
                Ok(None) => SidecarState::new(item.item_uid.clone()),
                Err((_, err)) => {
                    applied_items.push(BulkApplyItemResult {
                        media_path: item.media_path,
                        final_media_path: None,
                        item_uid: item.item_uid,
                        applied_provider_id: None,
                        success: false,
                        operation_id: None,
                        sidecar_path: None,
                        error: Some(err),
                    });
                    continue;
                }
            };

            sidecar_state.item_uid = item.item_uid.clone();
            sidecar_state.provider_ids.tmdb = Some(provider_id.clone());
            sidecar_state.applied_state = json!({
                "metadata_lookup": {
                    "title": metadata_title,
                    "year": metadata_year,
                    "confidence": metadata_confidence,
                    "provider_id": provider_id,
                }
            });

            let write_result =
                apply_sidecar_with_rollback(&state.state_dir, &media_path, &sidecar_state)
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e));

            match write_result {
                Ok((operation_id, sidecar_path)) => applied_items.push(BulkApplyItemResult {
                    media_path: item.media_path,
                    final_media_path: None,
                    item_uid: item.item_uid,
                    applied_provider_id: sidecar_state.provider_ids.tmdb.clone(),
                    success: true,
                    operation_id: Some(operation_id),
                    sidecar_path: Some(sidecar_path.display().to_string()),
                    error: None,
                }),
                Err((_, err)) => applied_items.push(BulkApplyItemResult {
                    media_path: item.media_path,
                    final_media_path: None,
                    item_uid: item.item_uid,
                    applied_provider_id: None,
                    success: false,
                    operation_id: None,
                    sidecar_path: None,
                    error: Some(err),
                }),
            }

            continue;
        }

        if request.action == "combine_duplicates" {
            let target_uid = item
                .proposed_item_uid
                .clone()
                .unwrap_or_else(|| item.item_uid.clone());
            let media_path = PathBuf::from(&item.media_path);
            let plan = sidecar_workflow::build_plan(&media_path, &target_uid)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()));

            let plan = match plan {
                Ok(v) => v,
                Err((_, err)) => {
                    applied_items.push(BulkApplyItemResult {
                        media_path: item.media_path,
                        final_media_path: None,
                        item_uid: item.item_uid,
                        applied_provider_id: None,
                        success: false,
                        operation_id: None,
                        sidecar_path: None,
                        error: Some(err),
                    });
                    continue;
                }
            };

            let applied = sidecar_workflow::apply_plan(
                &media_path,
                &target_uid,
                &plan.plan_hash,
                &state.state_dir,
            );

            match applied {
                Ok(result) => applied_items.push(BulkApplyItemResult {
                    media_path: item.media_path,
                    final_media_path: None,
                    item_uid: target_uid,
                    applied_provider_id: None,
                    success: true,
                    operation_id: Some(result.operation_id),
                    sidecar_path: Some(result.sidecar_path),
                    error: None,
                }),
                Err(err) => applied_items.push(BulkApplyItemResult {
                    media_path: item.media_path,
                    final_media_path: None,
                    item_uid: target_uid,
                    applied_provider_id: None,
                    success: false,
                    operation_id: None,
                    sidecar_path: None,
                    error: Some(err.to_string()),
                }),
            }

            continue;
        }

        let Some(plan) = item.plan else {
            applied_items.push(BulkApplyItemResult {
                media_path: item.media_path,
                final_media_path: None,
                item_uid: item.item_uid,
                applied_provider_id: None,
                success: false,
                operation_id: None,
                sidecar_path: None,
                error: Some(item.error.unwrap_or_else(|| "plan unavailable".to_string())),
            });
            continue;
        };

        let media_path = PathBuf::from(&item.media_path);
        let applied = sidecar_workflow::apply_plan(
            &media_path,
            &item.item_uid,
            &plan.plan_hash,
            &state.state_dir,
        );
        match applied {
            Ok(result) => applied_items.push(BulkApplyItemResult {
                media_path: item.media_path,
                final_media_path: None,
                item_uid: item.item_uid,
                applied_provider_id: None,
                success: true,
                operation_id: Some(result.operation_id),
                sidecar_path: Some(result.sidecar_path),
                error: None,
            }),
            Err(err) => applied_items.push(BulkApplyItemResult {
                media_path: item.media_path,
                final_media_path: None,
                item_uid: item.item_uid,
                applied_provider_id: None,
                success: false,
                operation_id: None,
                sidecar_path: None,
                error: Some(err.to_string()),
            }),
        }
    }

    let succeeded = applied_items.iter().filter(|v| v.success).count();
    let failed = applied_items.len().saturating_sub(succeeded);
    Ok(BulkApplyResponse {
        action: request.action.clone(),
        batch_hash: request.approved_batch_hash.clone(),
        total_items: applied_items.len(),
        succeeded,
        failed,
        items: applied_items,
    })
}

fn build_bulk_preview(
    state: &AppState,
    action: &str,
    items: &[BulkItemInput],
) -> Result<BulkDryRunResponse, (StatusCode, String)> {
    let action = normalize_bulk_action(action)?;
    let duplicate_groups = if action == "combine_duplicates" {
        Some(build_duplicate_groups(items))
    } else {
        None
    };
    let mut response_items: Vec<BulkDryRunItem> = Vec::with_capacity(items.len());
    let mut creates = 0;
    let mut updates = 0;
    let mut noops = 0;

    for item in items {
        let media_path = PathBuf::from(&item.media_path);
        if let Err((status, message)) =
            ensure_media_file_path_allowed(&media_path, &state.library_roots)
        {
            if status == StatusCode::FORBIDDEN || status == StatusCode::FAILED_DEPENDENCY {
                return Err((status, message));
            }

            response_items.push(BulkDryRunItem {
                media_path: item.media_path.clone(),
                item_uid: derive_item_uid(item, &media_path),
                plan: None,
                proposed_media_path: None,
                proposed_item_uid: None,
                proposed_provider_id: None,
                metadata_title: None,
                metadata_year: None,
                metadata_confidence: None,
                can_apply: false,
                note: Some("path validation failed".to_string()),
                error: Some(message),
            });
            continue;
        }

        let item_uid = derive_item_uid(item, &media_path);

        let (
            proposed_media_path,
            proposed_item_uid,
            proposed_provider_id,
            metadata_title,
            metadata_year,
            metadata_confidence,
            can_apply,
            note,
            error,
        ) = if action == "rename" {
            match compute_rename_target(&media_path) {
                Ok((target, rename_note)) => (
                    Some(target.display().to_string()),
                    None,
                    None,
                    None,
                    None,
                    None,
                    true,
                    Some(rename_note),
                    None,
                ),
                Err(message) => (
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    false,
                    None,
                    Some(message),
                ),
            }
        } else if action == "validate_nfo" {
            match compute_nfo_target(&media_path) {
                Ok((nfo_path, nfo_note)) => (
                    Some(nfo_path.display().to_string()),
                    None,
                    None,
                    None,
                    None,
                    None,
                    true,
                    Some(nfo_note),
                    None,
                ),
                Err(message) => (
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    false,
                    None,
                    Some(message),
                ),
            }
        } else if action == "combine_duplicates" {
            match &duplicate_groups {
                Some(groups) => {
                    let (proposed_uid, group_size) =
                        duplicate_uid_for_item(&item.media_path, groups);
                    let note = if group_size > 1 {
                        format!("duplicate group size={group_size}")
                    } else {
                        "unique item (no duplicates in current selection)".to_string()
                    };
                    (
                        None,
                        Some(proposed_uid),
                        None,
                        None,
                        None,
                        None,
                        true,
                        Some(note),
                        None,
                    )
                }
                None => (
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    false,
                    None,
                    Some("duplicate groups unavailable".to_string()),
                ),
            }
        } else if action == "metadata_lookup" {
            let candidate =
                infer_metadata_candidate(&media_path, &item_uid, item.metadata_override.as_ref());
            (
                None,
                None,
                Some(candidate.provider_id.clone()),
                Some(candidate.title.clone()),
                candidate.year,
                Some(candidate.confidence),
                true,
                Some(format!(
                    "metadata candidate title={} year={} confidence={:.2}",
                    candidate.title,
                    candidate
                        .year
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    candidate.confidence
                )),
                None,
            )
        } else {
            (None, None, None, None, None, None, true, None, None)
        };

        if error.is_some() {
            response_items.push(BulkDryRunItem {
                media_path: item.media_path.clone(),
                item_uid,
                plan: None,
                proposed_media_path,
                proposed_item_uid,
                proposed_provider_id,
                metadata_title,
                metadata_year,
                metadata_confidence,
                can_apply,
                note,
                error,
            });
            continue;
        }

        let plan = sidecar_workflow::build_plan(&media_path, &item_uid)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        match plan.action {
            sidecar_workflow::SidecarPlanAction::Create => creates += 1,
            sidecar_workflow::SidecarPlanAction::Update => updates += 1,
            sidecar_workflow::SidecarPlanAction::Noop => noops += 1,
        }

        response_items.push(BulkDryRunItem {
            media_path: item.media_path.clone(),
            item_uid,
            plan: Some(plan),
            proposed_media_path,
            proposed_item_uid,
            proposed_provider_id,
            metadata_title,
            metadata_year,
            metadata_confidence,
            can_apply,
            note,
            error,
        });
    }

    let batch_hash = hash_bulk_preview(&action, &response_items);
    let invalid = response_items.iter().filter(|v| v.plan.is_none()).count();

    Ok(BulkDryRunResponse {
        action,
        batch_hash,
        total_items: response_items.len(),
        plan_ready: invalid == 0,
        summary: BulkDryRunSummary {
            creates,
            updates,
            noops,
            invalid,
        },
        items: response_items,
    })
}

fn hash_bulk_preview(action: &str, items: &[BulkDryRunItem]) -> String {
    let mut hasher = DefaultHasher::new();
    action.hash(&mut hasher);
    for item in items {
        item.media_path.hash(&mut hasher);
        item.item_uid.hash(&mut hasher);
        item.can_apply.hash(&mut hasher);
        if let Some(path) = &item.proposed_media_path {
            path.hash(&mut hasher);
        }
        if let Some(uid) = &item.proposed_item_uid {
            uid.hash(&mut hasher);
        }
        if let Some(provider_id) = &item.proposed_provider_id {
            provider_id.hash(&mut hasher);
        }
        if let Some(title) = &item.metadata_title {
            title.hash(&mut hasher);
        }
        if let Some(year) = item.metadata_year {
            year.hash(&mut hasher);
        }
        if let Some(confidence) = item.metadata_confidence {
            confidence.to_bits().hash(&mut hasher);
        }
        if let Some(note) = &item.note {
            note.hash(&mut hasher);
        }
        if let Some(plan) = &item.plan {
            plan.plan_hash.hash(&mut hasher);
        }
        if let Some(error) = &item.error {
            error.hash(&mut hasher);
        }
    }

    format!("{:016x}", hasher.finish())
}

fn normalize_bulk_action(action: &str) -> Result<String, (StatusCode, String)> {
    let trimmed = action.trim().to_ascii_lowercase();
    let normalized = match trimmed.as_str() {
        "metadata_lookup" => "metadata_lookup",
        "combine_duplicates" => "combine_duplicates",
        "rename" => "rename",
        "validate_nfo" => "validate_nfo",
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                "unsupported action; expected one of metadata_lookup, combine_duplicates, rename, validate_nfo".to_string(),
            ));
        }
    };

    Ok(normalized.to_string())
}

fn derive_item_uid(item: &BulkItemInput, media_path: &std::path::Path) -> String {
    if let Some(uid) = &item.item_uid {
        let trimmed = uid.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    media_path
        .file_stem()
        .and_then(|v| v.to_str())
        .unwrap_or("unknown-item")
        .to_string()
}

fn compute_rename_target(media_path: &std::path::Path) -> Result<(PathBuf, String), String> {
    let parent = media_path
        .parent()
        .ok_or_else(|| "cannot determine rename parent directory".to_string())?;

    let extension = media_path
        .extension()
        .and_then(|v| v.to_str())
        .map(|v| format!(".{v}"))
        .unwrap_or_default();
    let file_stem = media_path
        .file_stem()
        .and_then(|v| v.to_str())
        .ok_or_else(|| "cannot determine media file stem".to_string())?;

    let normalized = normalize_filename_stem(file_stem);
    if normalized.is_empty() {
        return Err("rename target stem is empty after normalization".to_string());
    }

    let target = parent.join(format!("{normalized}{extension}"));
    if target == media_path {
        return Ok((target, "already normalized".to_string()));
    }

    if target.exists() {
        return Err(format!(
            "rename collision: target already exists ({})",
            target.display()
        ));
    }

    Ok((target, "will normalize filename".to_string()))
}

fn normalize_filename_stem(stem: &str) -> String {
    let mut output = String::with_capacity(stem.len());
    let mut previous_was_space = false;

    for ch in stem.chars() {
        let mapped = if ch == '.' || ch == '_' || ch == '-' {
            ' '
        } else {
            ch
        };

        if mapped.is_whitespace() {
            if !previous_was_space {
                output.push(' ');
            }
            previous_was_space = true;
        } else {
            output.push(mapped);
            previous_was_space = false;
        }
    }

    output.trim().to_string()
}

fn build_duplicate_groups(items: &[BulkItemInput]) -> HashMap<String, Vec<String>> {
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();

    for item in items {
        let media_path = PathBuf::from(&item.media_path);
        let key = duplicate_key_for_media_path(&media_path);
        groups.entry(key).or_default().push(item.media_path.clone());
    }

    groups
}

fn duplicate_uid_for_item(
    media_path: &str,
    groups: &HashMap<String, Vec<String>>,
) -> (String, usize) {
    let path = PathBuf::from(media_path);
    let stem = path
        .file_stem()
        .and_then(|v| v.to_str())
        .unwrap_or("unknown-item");
    let key = duplicate_key_for_media_path(&path);
    let Some(group) = groups.get(&key) else {
        return (normalize_filename_stem(stem), 1);
    };

    // Deterministic canonical item UID for all duplicates in a group.
    let mut sorted = group.clone();
    sorted.sort();
    let canonical = PathBuf::from(&sorted[0]);
    let canonical_stem = canonical
        .file_stem()
        .and_then(|v| v.to_str())
        .unwrap_or("unknown-item");

    (normalize_filename_stem(canonical_stem), group.len())
}

fn duplicate_key_for_media_path(media_path: &std::path::Path) -> String {
    let stem = media_path
        .file_stem()
        .and_then(|v| v.to_str())
        .unwrap_or("unknown-item");
    let base = normalize_duplicate_key(stem);
    let size_bucket = duplicate_size_bucket_for_media_path(media_path)
        .map(|v| v.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    format!("{base}|size:{size_bucket}")
}

fn duplicate_size_bucket_for_media_path(media_path: &std::path::Path) -> Option<u64> {
    // Coarse 512MB buckets reduce false positives while allowing near-identical encodes to group.
    const BUCKET_BYTES: u64 = 512 * 1024 * 1024;
    let metadata = fs::metadata(media_path).ok()?;
    Some(metadata.len() / BUCKET_BYTES)
}

fn normalize_duplicate_key(stem: &str) -> String {
    let normalized = normalize_filename_stem(stem).to_ascii_lowercase();
    let noise = [
        "2160p",
        "1080p",
        "720p",
        "480p",
        "bluray",
        "bdrip",
        "webrip",
        "web",
        "x264",
        "x265",
        "hevc",
        "h264",
        "dvdrip",
        "remux",
        "hdr",
        "proper",
        "repack",
        "extended",
        "directors",
        "cut",
        "yify",
    ];

    let tokens: Vec<&str> = normalized.split_whitespace().collect();
    let year = tokens.iter().find_map(|token| {
        if token.len() == 4 {
            token
                .parse::<u16>()
                .ok()
                .filter(|value| (1900..=2100).contains(value))
        } else {
            None
        }
    });

    let mut filtered = Vec::with_capacity(tokens.len());
    for token in tokens {
        if noise.contains(&token) {
            continue;
        }
        if token.len() == 4 {
            if let Some(y) = year {
                if token == y.to_string() {
                    continue;
                }
            }
        }
        filtered.push(token);
    }

    let title_key = if filtered.is_empty() {
        normalized.split_whitespace().collect::<Vec<_>>().join(" ")
    } else {
        filtered.join(" ")
    };

    format!(
        "{}|{}",
        title_key,
        year.map(|v| v.to_string())
            .unwrap_or_else(|| "none".to_string())
    )
}

fn compute_nfo_target(media_path: &std::path::Path) -> Result<(PathBuf, String), String> {
    let parent = media_path
        .parent()
        .ok_or_else(|| "cannot determine media parent directory".to_string())?;
    let stem = media_path
        .file_stem()
        .and_then(|v| v.to_str())
        .ok_or_else(|| "cannot determine media file stem".to_string())?;

    let nfo_path = parent.join(format!("{stem}.nfo"));
    if !nfo_path.exists() {
        return Ok((nfo_path, "nfo missing; will create".to_string()));
    }

    let existing = fs::read_to_string(&nfo_path).unwrap_or_default();
    if existing.trim().is_empty() {
        return Ok((nfo_path, "nfo empty; will rewrite".to_string()));
    }

    Ok((nfo_path, "nfo exists and appears valid".to_string()))
}

fn generate_nfo_content(media_path: &str, item_uid: &str) -> String {
    let file_name = PathBuf::from(media_path)
        .file_name()
        .and_then(|v| v.to_str())
        .map(|v| v.to_string())
        .unwrap_or_else(|| media_path.to_string());
    format!(
        "<movie>\n  <title>{item_uid}</title>\n  <originalfilename>{file_name}</originalfilename>\n  <id>{item_uid}</id>\n</movie>\n"
    )
}

#[derive(Debug, Clone)]
struct MetadataCandidate {
    title: String,
    year: Option<u16>,
    provider_id: String,
    confidence: f32,
}

fn infer_metadata_candidate(
    media_path: &std::path::Path,
    item_uid: &str,
    metadata_override: Option<&MetadataOverrideInput>,
) -> MetadataCandidate {
    let stem = media_path
        .file_stem()
        .and_then(|v| v.to_str())
        .unwrap_or(item_uid);
    let normalized = normalize_filename_stem(stem);
    let tokens: Vec<String> = normalized
        .split_whitespace()
        .map(|v| v.to_string())
        .collect();

    let year = tokens.iter().find_map(|token| {
        if token.len() == 4 {
            token
                .parse::<u16>()
                .ok()
                .filter(|value| (1900..=2100).contains(value))
        } else {
            None
        }
    });

    let noise = [
        "2160p", "1080p", "720p", "480p", "bluray", "webrip", "web", "x264", "x265", "hevc",
        "h264", "dvdrip", "remux", "hdr", "proper", "repack",
    ];

    let mut filtered_tokens: Vec<String> = Vec::new();
    for token in tokens {
        let lowered = token.to_ascii_lowercase();
        if let Some(y) = year {
            if lowered == y.to_string() {
                continue;
            }
        }
        if noise.contains(&lowered.as_str()) {
            continue;
        }
        filtered_tokens.push(token);
    }

    let title = if filtered_tokens.is_empty() {
        normalize_filename_stem(item_uid)
    } else {
        filtered_tokens.join(" ")
    };

    let mut confidence = 0.45_f32;
    if year.is_some() {
        confidence += 0.3;
    }
    if title.split_whitespace().count() >= 2 {
        confidence += 0.15;
    }
    let confidence = confidence.min(0.95);

    let mut hasher = DefaultHasher::new();
    title.to_ascii_lowercase().hash(&mut hasher);
    year.unwrap_or(0).hash(&mut hasher);
    let provider_id = format!("tmdb-local-{:08x}", (hasher.finish() & 0xffff_ffff));

    let override_title = metadata_override.and_then(|v| normalize_optional_string(&v.title));
    let override_provider_id =
        metadata_override.and_then(|v| normalize_optional_string(&v.provider_id));
    let override_year = metadata_override.and_then(|v| v.year);
    let override_confidence = metadata_override.and_then(|v| v.confidence);

    MetadataCandidate {
        title: override_title.unwrap_or(title),
        year: override_year.or(year),
        provider_id: override_provider_id.unwrap_or(provider_id),
        confidence: override_confidence.unwrap_or(confidence).clamp(0.0, 1.0),
    }
}

fn normalize_optional_string(value: &Option<String>) -> Option<String> {
    value.as_ref().and_then(|v| {
        let trimmed = v.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum FsRollbackSnapshotData {
    Rename {
        source_path: String,
        target_path: String,
    },
    WriteText {
        file_path: String,
        previous_content: Option<String>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct FsRollbackSnapshot {
    operation_id: String,
    created_at_ms: u128,
    data: FsRollbackSnapshotData,
}

fn rollback_bulk_operation(state: &AppState, operation_id: &str) -> Result<String, String> {
    if operation_id.starts_with("fsop-") {
        return rollback_fs_operation(state, operation_id);
    }

    let restored = sidecar_workflow::rollback_operation(operation_id, &state.state_dir)
        .map_err(|e| e.to_string())?;
    Ok(format!("sidecar restored at {}", restored.sidecar_path))
}

fn apply_rename_with_rollback(
    state_dir: &std::path::Path,
    source_path: &std::path::Path,
    target_path: &std::path::Path,
) -> Result<String, String> {
    let operation_id = generate_fs_operation_id();
    let snapshot = FsRollbackSnapshot {
        operation_id: operation_id.clone(),
        created_at_ms: current_timestamp_ms(),
        data: FsRollbackSnapshotData::Rename {
            source_path: source_path.display().to_string(),
            target_path: target_path.display().to_string(),
        },
    };

    write_fs_rollback_snapshot(state_dir, &snapshot)?;
    if let Err(err) = fs::rename(source_path, target_path) {
        let _ = delete_fs_rollback_snapshot(state_dir, &operation_id);
        return Err(format!("rename failed: {err}"));
    }

    Ok(operation_id)
}

fn apply_write_text_with_rollback(
    state_dir: &std::path::Path,
    file_path: &std::path::Path,
    content: &str,
) -> Result<String, String> {
    let operation_id = generate_fs_operation_id();
    let previous_content = if file_path.exists() {
        Some(fs::read_to_string(file_path).unwrap_or_default())
    } else {
        None
    };

    let snapshot = FsRollbackSnapshot {
        operation_id: operation_id.clone(),
        created_at_ms: current_timestamp_ms(),
        data: FsRollbackSnapshotData::WriteText {
            file_path: file_path.display().to_string(),
            previous_content,
        },
    };

    write_fs_rollback_snapshot(state_dir, &snapshot)?;
    if let Err(err) = fs::write(file_path, content) {
        let _ = delete_fs_rollback_snapshot(state_dir, &operation_id);
        return Err(format!("write failed: {err}"));
    }

    Ok(operation_id)
}

fn apply_sidecar_with_rollback(
    state_dir: &std::path::Path,
    media_path: &std::path::Path,
    sidecar_state: &SidecarState,
) -> Result<(String, PathBuf), String> {
    let sidecar_path = sidecar_store::sidecar_path_for_media(media_path)
        .map_err(|e| format!("resolve sidecar path failed: {e}"))?;
    let serialized = serde_json::to_string_pretty(sidecar_state)
        .map_err(|e| format!("encode sidecar state failed: {e}"))?;
    let operation_id = apply_write_text_with_rollback(state_dir, &sidecar_path, &serialized)?;
    Ok((operation_id, sidecar_path))
}

fn rollback_fs_operation(state: &AppState, operation_id: &str) -> Result<String, String> {
    let snapshot = read_fs_rollback_snapshot(&state.state_dir, operation_id)?;
    match snapshot.data {
        FsRollbackSnapshotData::Rename {
            source_path,
            target_path,
        } => {
            let source = PathBuf::from(source_path);
            let target = PathBuf::from(target_path);
            if !target.exists() {
                return Err(format!(
                    "rollback failed: target path missing ({})",
                    target.display()
                ));
            }
            if source.exists() {
                return Err(format!(
                    "rollback failed: source path already exists ({})",
                    source.display()
                ));
            }

            fs::rename(&target, &source).map_err(|e| {
                format!(
                    "rollback rename failed {} -> {} ({})",
                    target.display(),
                    source.display(),
                    e
                )
            })?;
            delete_fs_rollback_snapshot(&state.state_dir, operation_id)?;
            Ok(format!(
                "renamed {} back to {}",
                target.display(),
                source.display()
            ))
        }
        FsRollbackSnapshotData::WriteText {
            file_path,
            previous_content,
        } => {
            let path = PathBuf::from(file_path);
            match previous_content {
                Some(content) => {
                    fs::write(&path, content)
                        .map_err(|e| format!("rollback write failed {} ({})", path.display(), e))?;
                }
                None => {
                    if path.exists() {
                        fs::remove_file(&path).map_err(|e| {
                            format!("rollback remove failed {} ({})", path.display(), e)
                        })?;
                    }
                }
            }
            delete_fs_rollback_snapshot(&state.state_dir, operation_id)?;
            Ok(format!("restored file {}", path.display()))
        }
    }
}

fn generate_fs_operation_id() -> String {
    let ts = current_timestamp_ms();
    let nonce = FS_OPERATION_NONCE.fetch_add(1, Ordering::Relaxed);
    format!("fsop-{ts}-{nonce}-{}", std::process::id())
}

fn fs_rollback_snapshot_path(state_dir: &std::path::Path, operation_id: &str) -> PathBuf {
    state_dir
        .join("rollback")
        .join(format!("{operation_id}.json"))
}

fn write_fs_rollback_snapshot(
    state_dir: &std::path::Path,
    snapshot: &FsRollbackSnapshot,
) -> Result<(), String> {
    let path = fs_rollback_snapshot_path(state_dir, &snapshot.operation_id);
    let parent = path
        .parent()
        .ok_or_else(|| "invalid rollback snapshot path".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|e| format!("create rollback dir {} ({})", parent.display(), e))?;

    let encoded = serde_json::to_string_pretty(snapshot)
        .map_err(|e| format!("encode rollback snapshot ({})", e))?;
    fs::write(&path, encoded)
        .map_err(|e| format!("write rollback snapshot {} ({})", path.display(), e))?;
    Ok(())
}

fn read_fs_rollback_snapshot(
    state_dir: &std::path::Path,
    operation_id: &str,
) -> Result<FsRollbackSnapshot, String> {
    let path = fs_rollback_snapshot_path(state_dir, operation_id);
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("read rollback snapshot {} ({})", path.display(), e))?;
    serde_json::from_str::<FsRollbackSnapshot>(&content)
        .map_err(|e| format!("decode rollback snapshot {} ({})", path.display(), e))
}

fn delete_fs_rollback_snapshot(
    state_dir: &std::path::Path,
    operation_id: &str,
) -> Result<(), String> {
    let path = fs_rollback_snapshot_path(state_dir, operation_id);
    if !path.exists() {
        return Ok(());
    }
    fs::remove_file(&path)
        .map_err(|e| format!("delete rollback snapshot {} ({})", path.display(), e))?;
    Ok(())
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
    state
        .operation_log
        .push(event.kind.clone(), event.detail.clone(), event.success);
    let _ = state.audit_store.insert_event(&event);
}

fn current_timestamp_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn normalize_recent_limit(limit: Option<usize>) -> usize {
    limit.unwrap_or(DEFAULT_RECENT_LIMIT).min(MAX_RECENT_LIMIT)
}

fn normalize_library_limit(limit: Option<usize>) -> usize {
    let normalized = limit.unwrap_or(DEFAULT_LIBRARY_LIMIT);
    if normalized == 0 {
        return DEFAULT_LIBRARY_LIMIT;
    }

    normalized.min(MAX_LIBRARY_LIMIT)
}

fn normalize_job_status_filter(value: Option<&str>) -> Option<String> {
    let trimmed = value?.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("all") {
        return None;
    }

    let normalized = trimmed.to_ascii_lowercase();
    match normalized.as_str() {
        "running" => Some("running".to_string()),
        "succeeded" => Some("succeeded".to_string()),
        "failed" => Some("failed".to_string()),
        "canceled" => Some("canceled".to_string()),
        _ => None,
    }
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
struct RecentJobsQuery {
    limit: Option<usize>,
    offset: Option<usize>,
    status: Option<String>,
    kind: Option<String>,
    bulk_only: Option<bool>,
}

#[derive(Debug, Serialize)]
struct RecentJobsResponse {
    total_count: usize,
    offset: usize,
    limit: usize,
    items: Vec<JobRecord>,
}

#[derive(Debug, Deserialize)]
struct JobActionRequest {
    job_id: i64,
}

#[derive(Debug, Serialize)]
struct JobActionResponse {
    job_id: i64,
    ok: bool,
    message: String,
    retried_job_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct LibraryItemsQuery {
    root_index: Option<usize>,
    q: Option<String>,
    offset: Option<usize>,
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

#[derive(Debug, Deserialize, Serialize, Clone)]
struct MetadataOverrideInput {
    title: Option<String>,
    year: Option<u16>,
    provider_id: Option<String>,
    confidence: Option<f32>,
}

#[derive(Debug, Deserialize, Serialize)]
struct BulkItemInput {
    media_path: String,
    item_uid: Option<String>,
    metadata_override: Option<MetadataOverrideInput>,
}

#[derive(Debug, Deserialize, Serialize)]
struct BulkDryRunRequest {
    action: String,
    items: Vec<BulkItemInput>,
}

#[derive(Debug, Deserialize, Serialize)]
struct BulkApplyRequest {
    action: String,
    approved_batch_hash: String,
    items: Vec<BulkItemInput>,
}

#[derive(Debug, Serialize)]
struct BulkDryRunSummary {
    creates: usize,
    updates: usize,
    noops: usize,
    invalid: usize,
}

#[derive(Debug, Serialize)]
struct BulkDryRunItem {
    media_path: String,
    item_uid: String,
    plan: Option<SidecarPlan>,
    proposed_media_path: Option<String>,
    proposed_item_uid: Option<String>,
    proposed_provider_id: Option<String>,
    metadata_title: Option<String>,
    metadata_year: Option<u16>,
    metadata_confidence: Option<f32>,
    can_apply: bool,
    note: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct BulkDryRunResponse {
    action: String,
    batch_hash: String,
    total_items: usize,
    plan_ready: bool,
    summary: BulkDryRunSummary,
    items: Vec<BulkDryRunItem>,
}

#[derive(Debug, Serialize)]
struct BulkApplyItemResult {
    media_path: String,
    final_media_path: Option<String>,
    item_uid: String,
    applied_provider_id: Option<String>,
    success: bool,
    operation_id: Option<String>,
    sidecar_path: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct BulkApplyResponse {
    action: String,
    batch_hash: String,
    total_items: usize,
    succeeded: usize,
    failed: usize,
    items: Vec<BulkApplyItemResult>,
}

#[derive(Debug, Deserialize, Serialize)]
struct BulkRollbackRequest {
    operation_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
struct BulkRollbackItemResult {
    operation_id: String,
    success: bool,
    detail: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct BulkRollbackResponse {
    total_items: usize,
    succeeded: usize,
    failed: usize,
    items: Vec<BulkRollbackItemResult>,
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use axum::http::StatusCode;

    use super::{
        BulkItemInput, DEFAULT_LIBRARY_LIMIT, DEFAULT_RECENT_LIMIT, MAX_LIBRARY_LIMIT,
        MAX_RECENT_LIMIT, MetadataOverrideInput, build_duplicate_groups, compute_nfo_target,
        compute_rename_target, duplicate_key_for_media_path, ensure_media_file_path_allowed,
        infer_metadata_candidate, normalize_bulk_action, normalize_duplicate_key,
        normalize_filename_stem, normalize_job_status_filter, normalize_library_limit,
        normalize_recent_limit,
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
        assert_eq!(
            normalize_recent_limit(Some(MAX_RECENT_LIMIT + 10)),
            MAX_RECENT_LIMIT
        );
    }

    #[test]
    fn library_limit_defaults_and_caps() {
        assert_eq!(normalize_library_limit(None), DEFAULT_LIBRARY_LIMIT);
        assert_eq!(normalize_library_limit(Some(0)), DEFAULT_LIBRARY_LIMIT);
        assert_eq!(normalize_library_limit(Some(250)), 250);
        assert_eq!(
            normalize_library_limit(Some(MAX_LIBRARY_LIMIT + 25)),
            MAX_LIBRARY_LIMIT
        );
    }

    #[test]
    fn job_status_filter_normalization() {
        assert_eq!(normalize_job_status_filter(None), None);
        assert_eq!(normalize_job_status_filter(Some("all")), None);
        assert_eq!(
            normalize_job_status_filter(Some(" running ")),
            Some("running".to_string())
        );
        assert_eq!(normalize_job_status_filter(Some("unknown")), None);
    }

    #[test]
    fn bulk_action_accepts_expected_values() {
        assert_eq!(
            normalize_bulk_action("rename").expect("valid action"),
            "rename"
        );
        assert_eq!(
            normalize_bulk_action(" metadata_lookup ").expect("valid action"),
            "metadata_lookup"
        );
        assert!(normalize_bulk_action("unknown").is_err());
    }

    #[test]
    fn filename_normalization_compacts_separators() {
        assert_eq!(
            normalize_filename_stem("Movie.Name_2024-1080p"),
            "Movie Name 2024 1080p"
        );
        assert_eq!(normalize_filename_stem("  A..B__C  "), "A B C");
    }

    #[test]
    fn rename_target_stays_in_same_parent_and_preserves_extension() {
        let root = unique_temp_dir("rename-target");
        fs::create_dir_all(&root).expect("create root");
        let media_path = root.join("My.Movie_2024.mkv");
        fs::write(&media_path, b"x").expect("write media file");

        let (target, note) = compute_rename_target(&media_path).expect("rename target computed");
        assert_eq!(target.parent(), media_path.parent());
        assert_eq!(target.extension().and_then(|v| v.to_str()), Some("mkv"));
        assert_eq!(note, "will normalize filename");

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn duplicate_grouping_uses_normalized_key() {
        let items = vec![
            BulkItemInput {
                media_path: "/tmp/Movie.Name.2024.mkv".to_string(),
                item_uid: None,
                metadata_override: None,
            },
            BulkItemInput {
                media_path: "/tmp/movie name 2024.mp4".to_string(),
                item_uid: None,
                metadata_override: None,
            },
            BulkItemInput {
                media_path: "/tmp/Other.Movie.mkv".to_string(),
                item_uid: None,
                metadata_override: None,
            },
        ];

        let groups = build_duplicate_groups(&items);
        assert_eq!(
            normalize_duplicate_key("Movie.Name.2024"),
            "movie name|2024"
        );
        assert_eq!(
            groups.get("movie name|2024|size:unknown").map(Vec::len),
            Some(2)
        );
        assert_eq!(
            groups.get("other movie|none|size:unknown").map(Vec::len),
            Some(1)
        );
    }

    #[test]
    fn duplicate_key_removes_noise_tokens() {
        assert_eq!(
            normalize_duplicate_key("The.Movie.2021.1080p.BluRay.x264"),
            "the movie|2021"
        );
    }

    #[test]
    fn duplicate_key_includes_size_bucket_for_existing_files() {
        let root = unique_temp_dir("duplicate-size");
        fs::create_dir_all(&root).expect("create root");
        let media_path = root.join("Sample.Movie.2021.mkv");
        fs::write(&media_path, vec![0_u8; 1_500_000]).expect("write media");

        let key = duplicate_key_for_media_path(&media_path);
        assert!(key.starts_with("sample movie|2021|size:"));
        assert!(!key.ends_with("unknown"));

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn nfo_target_reports_missing_and_valid_states() {
        let root = unique_temp_dir("nfo-target");
        fs::create_dir_all(&root).expect("create root");
        let media = root.join("Title.2024.mkv");
        fs::write(&media, b"x").expect("write media");

        let (nfo_missing, msg_missing) = compute_nfo_target(&media).expect("nfo target");
        assert!(nfo_missing.ends_with("Title.2024.nfo"));
        assert!(msg_missing.contains("missing"));

        fs::write(&nfo_missing, "<movie><title>x</title></movie>").expect("write nfo");
        let (_nfo_valid, msg_valid) = compute_nfo_target(&media).expect("nfo target valid");
        assert!(msg_valid.contains("valid"));

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn metadata_inference_extracts_title_year_and_provider() {
        let media = PathBuf::from("/tmp/The.Movie.2021.1080p.WEBRip.x264.mkv");
        let candidate = infer_metadata_candidate(&media, "the-movie-uid", None);
        assert_eq!(candidate.title, "The Movie");
        assert_eq!(candidate.year, Some(2021));
        assert!(candidate.provider_id.starts_with("tmdb-local-"));
        assert!(candidate.confidence >= 0.75);
    }

    #[test]
    fn metadata_inference_honors_override_values() {
        let media = PathBuf::from("/tmp/The.Movie.2021.1080p.WEBRip.x264.mkv");
        let metadata_override = MetadataOverrideInput {
            title: Some("Custom Title".to_string()),
            year: Some(1999),
            provider_id: Some("tmdb-override-123".to_string()),
            confidence: Some(0.98),
        };
        let candidate = infer_metadata_candidate(&media, "the-movie-uid", Some(&metadata_override));
        assert_eq!(candidate.title, "Custom Title");
        assert_eq!(candidate.year, Some(1999));
        assert_eq!(candidate.provider_id, "tmdb-override-123");
        assert_eq!(candidate.confidence, 0.98);
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

        let dir_err =
            ensure_media_file_path_allowed(&dir_in_root, &roots).expect_err("dir should fail");
        assert_eq!(dir_err.0, StatusCode::BAD_REQUEST);

        let outside_err = ensure_media_file_path_allowed(&file_outside, &roots)
            .expect_err("outside root should fail");
        assert_eq!(outside_err.0, StatusCode::FORBIDDEN);

        fs::remove_dir_all(root).expect("cleanup root");
    }
}
