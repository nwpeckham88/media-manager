use std::io::Read;
use std::path::PathBuf;
use std::process::Command;
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
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::audit_store::AuditStore;
use crate::auth;
use crate::config::BrandingConfig;
use crate::domain::sidecar::{DesiredMediaState, SidecarState};
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
const MAX_PATH_COMPONENT_BYTES: usize = 255;
const MAX_PATH_BYTES: usize = 4096;
static FS_OPERATION_NONCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone)]
pub struct AppState {
    pub branding: BrandingConfig,
    pub toolchain: ToolchainSnapshot,
    pub library_roots: Vec<PathBuf>,
    pub state_dir: PathBuf,
    pub audit_db_path: PathBuf,
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
        .route("/api/index/start", post(start_library_index))
        .route("/api/index/stats", get(index_stats))
        .route("/api/index/items", get(index_items))
        .route("/api/formatting/candidates", get(formatting_candidates))
        .route(
            "/api/consolidation/exact-duplicates",
            get(consolidation_exact_duplicates),
        )
        .route(
            "/api/consolidation/semantic-duplicates",
            get(consolidation_semantic_duplicates),
        )
        .route(
            "/api/consolidation/quarantine",
            post(consolidation_quarantine),
        )
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

async fn start_library_index(
    State(state): State<Arc<AppState>>,
    Json(request): Json<IndexStartRequest>,
) -> Result<Json<IndexStartResponse>, (StatusCode, String)> {
    if state.library_roots.is_empty() {
        return Err((
            StatusCode::FAILED_DEPENDENCY,
            "MM_LIBRARY_ROOTS is not configured".to_string(),
        ));
    }

    let running_index_jobs = state
        .jobs_store
        .count_jobs_filtered(Some("running"), Some("library_index"), false)
        .map_err(internal_error)?;
    if running_index_jobs > 0 {
        return Err((
            StatusCode::CONFLICT,
            "an index job is already running".to_string(),
        ));
    }

    let include_hashes = request.include_hashes.unwrap_or(true);
    let include_probe = request.include_probe.unwrap_or(true);
    let payload_json = serde_json::to_string(&json!({
        "include_hashes": include_hashes,
        "include_probe": include_probe,
    }))
    .unwrap_or_else(|_| "{}".to_string());

    let Some(job_id) = create_job(&state, "library_index", &payload_json) else {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to create index job".to_string(),
        ));
    };

    let db_path = state.audit_db_path.clone();
    let library_roots = state.library_roots.clone();
    let ffprobe_path = state.toolchain.ffprobe.path.clone();

    tokio::task::spawn_blocking(move || {
        let result = run_library_index_job(
            &db_path,
            &library_roots,
            &ffprobe_path,
            job_id,
            include_hashes,
            include_probe,
        );

        if let Err(err) = result {
            let _ = complete_job_direct(
                &db_path,
                job_id,
                JobStatus::Failed,
                None,
                Some(err.to_string()),
            );
        }
    });

    Ok(Json(IndexStartResponse {
        job_id,
        started: true,
        message: "library indexing started".to_string(),
    }))
}

async fn index_stats(
    State(state): State<Arc<AppState>>,
) -> Result<Json<IndexStatsResponse>, (StatusCode, String)> {
    let conn = Connection::open(&state.audit_db_path).map_err(internal_error)?;
    let total_indexed: i64 = conn
        .query_row("SELECT COUNT(*) FROM media_index", [], |row| row.get(0))
        .map_err(internal_error)?;
    let hashed: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM media_index WHERE content_hash_sha256 IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .map_err(internal_error)?;
    let probed: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM media_index WHERE duration_seconds IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .map_err(internal_error)?;
    let last_indexed_at_ms: Option<i64> = conn
        .query_row("SELECT MAX(indexed_at_ms) FROM media_index", [], |row| {
            row.get(0)
        })
        .map_err(internal_error)?;

    Ok(Json(IndexStatsResponse {
        total_indexed: total_indexed.max(0) as usize,
        hashed: hashed.max(0) as usize,
        probed: probed.max(0) as usize,
        last_indexed_at_ms,
    }))
}

async fn index_items(
    State(state): State<Arc<AppState>>,
    Query(query): Query<IndexItemsQuery>,
) -> Result<Json<IndexItemsResponse>, (StatusCode, String)> {
    let conn = Connection::open(&state.audit_db_path).map_err(internal_error)?;
    let limit = query.limit.unwrap_or(120).clamp(1, 500) as i64;
    let offset = query.offset.unwrap_or(0) as i64;

    let mut items: Vec<IndexedMediaItem> = Vec::new();
    let mut stmt = conn
        .prepare(
            "SELECT media_path, root, parsed_title, parsed_year, parsed_provider_id, metadata_confidence,
                    content_hash_sha256, duration_seconds, video_codec, audio_codec, width, height, indexed_at_ms
             FROM media_index
             ORDER BY media_path ASC
             LIMIT ?1 OFFSET ?2",
        )
        .map_err(internal_error)?;
    let rows = stmt
        .query_map(params![limit, offset], |row| {
            Ok(IndexedMediaItem {
                media_path: row.get(0)?,
                root: row.get(1)?,
                parsed_title: row.get(2)?,
                parsed_year: row.get(3)?,
                parsed_provider_id: row.get(4)?,
                metadata_confidence: row.get(5)?,
                content_hash_sha256: row.get(6)?,
                duration_seconds: row.get(7)?,
                video_codec: row.get(8)?,
                audio_codec: row.get(9)?,
                width: row.get(10)?,
                height: row.get(11)?,
                indexed_at_ms: row.get(12)?,
            })
        })
        .map_err(internal_error)?;

    for row in rows {
        items.push(row.map_err(internal_error)?);
    }

    if let Some(only_missing_provider) = query.only_missing_provider {
        if only_missing_provider {
            items.retain(|item| item.parsed_provider_id.is_none());
        }
    }

    if let Some(min_confidence) = query.min_confidence {
        items.retain(|item| {
            item.metadata_confidence
                .map(|value| value >= min_confidence)
                .unwrap_or(false)
        });
    }

    if let Some(max_confidence) = query.max_confidence {
        items.retain(|item| {
            item.metadata_confidence
                .map(|value| value <= max_confidence)
                .unwrap_or(true)
        });
    }

    if let Some(search) = query.q.as_ref().map(|v| v.trim()).filter(|v| !v.is_empty()) {
        let search = search.to_ascii_lowercase();
        items.retain(|item| {
            item.media_path.to_ascii_lowercase().contains(&search)
                || item
                    .parsed_title
                    .as_ref()
                    .map(|v| v.to_ascii_lowercase().contains(&search))
                    .unwrap_or(false)
                || item
                    .parsed_provider_id
                    .as_ref()
                    .map(|v| v.to_ascii_lowercase().contains(&search))
                    .unwrap_or(false)
        });
    }

    Ok(Json(IndexItemsResponse {
        total_items: items.len(),
        offset: offset.max(0) as usize,
        limit: limit.max(1) as usize,
        items,
    }))
}

async fn formatting_candidates(
    State(state): State<Arc<AppState>>,
    Query(query): Query<FormattingCandidatesQuery>,
) -> Result<Json<FormattingCandidatesResponse>, (StatusCode, String)> {
    let conn = Connection::open(&state.audit_db_path).map_err(internal_error)?;
    let limit = query.limit.unwrap_or(120).clamp(1, 500) as i64;
    let offset = query.offset.unwrap_or(0) as i64;

    let mut candidates = Vec::new();
    let mut stmt = conn
        .prepare(
            "SELECT media_path
             FROM media_index
             ORDER BY media_path ASC
             LIMIT ?1 OFFSET ?2",
        )
        .map_err(internal_error)?;
    let rows = stmt
        .query_map(params![limit, offset], |row| row.get::<_, String>(0))
        .map_err(internal_error)?;

    for row in rows {
        let media_path = PathBuf::from(row.map_err(internal_error)?);
        if !media_path.exists() {
            continue;
        }
        if let Ok((target, note)) = compute_rename_target(&media_path, None, false) {
            if target != media_path {
                candidates.push(FormattingCandidateItem {
                    media_path: media_path.display().to_string(),
                    proposed_media_path: target.display().to_string(),
                    note,
                });
            }
        }
    }

    Ok(Json(FormattingCandidatesResponse {
        total_items: candidates.len(),
        offset: offset.max(0) as usize,
        limit: limit.max(1) as usize,
        items: candidates,
    }))
}

async fn consolidation_exact_duplicates(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ExactDuplicatesQuery>,
) -> Result<Json<ExactDuplicatesResponse>, (StatusCode, String)> {
    let conn = Connection::open(&state.audit_db_path).map_err(internal_error)?;
    let group_limit = query.limit.unwrap_or(40).clamp(1, 200) as i64;
    let min_group_size = query.min_group_size.unwrap_or(2).clamp(2, 1000) as i64;

    let mut group_stmt = conn
        .prepare(
            "SELECT content_hash_sha256, COUNT(*) AS count
             FROM media_index
             WHERE content_hash_sha256 IS NOT NULL
             GROUP BY content_hash_sha256
             HAVING COUNT(*) >= ?1
             ORDER BY count DESC
             LIMIT ?2",
        )
        .map_err(internal_error)?;

    let mut groups = Vec::new();
    let rows = group_stmt
        .query_map(params![min_group_size, group_limit], |row| {
            let hash: String = row.get(0)?;
            let count: i64 = row.get(1)?;
            Ok((hash, count))
        })
        .map_err(internal_error)?;

    for row in rows {
        let (content_hash, count) = row.map_err(internal_error)?;
        let mut item_stmt = conn
            .prepare(
                "SELECT media_path, file_size, parsed_title, parsed_year, parsed_provider_id,
                        video_codec, audio_codec, width, height, duration_seconds
                 FROM media_index
                 WHERE content_hash_sha256 = ?1
                 ORDER BY media_path ASC",
            )
            .map_err(internal_error)?;
        let item_rows = item_stmt
            .query_map([&content_hash], |item_row| {
                Ok(ExactDuplicateItem {
                    media_path: item_row.get(0)?,
                    file_size: item_row.get::<_, i64>(1)? as u64,
                    parsed_title: item_row.get(2)?,
                    parsed_year: item_row.get(3)?,
                    parsed_provider_id: item_row.get(4)?,
                    video_codec: item_row.get(5)?,
                    audio_codec: item_row.get(6)?,
                    width: item_row.get(7)?,
                    height: item_row.get(8)?,
                    duration_seconds: item_row.get(9)?,
                })
            })
            .map_err(internal_error)?;

        let mut items = Vec::new();
        for item in item_rows {
            items.push(item.map_err(internal_error)?);
        }

        groups.push(ExactDuplicateGroup {
            content_hash,
            count: count.max(0) as usize,
            items,
        });
    }

    Ok(Json(ExactDuplicatesResponse {
        total_groups: groups.len(),
        groups,
    }))
}

async fn consolidation_semantic_duplicates(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SemanticDuplicatesQuery>,
) -> Result<Json<SemanticDuplicatesResponse>, (StatusCode, String)> {
    let conn = Connection::open(&state.audit_db_path).map_err(internal_error)?;
    let group_limit = query.limit.unwrap_or(40).clamp(1, 200) as i64;
    let min_group_size = query.min_group_size.unwrap_or(2).clamp(2, 1000) as i64;

    let mut group_stmt = conn
        .prepare(
            "SELECT
                parsed_title,
                parsed_year,
                parsed_provider_id,
                COUNT(*) AS item_count,
                COUNT(DISTINCT COALESCE(content_hash_sha256, media_path)) AS variant_count
             FROM media_index
             WHERE parsed_title IS NOT NULL AND TRIM(parsed_title) != ''
             GROUP BY parsed_title, parsed_year, parsed_provider_id
             HAVING COUNT(*) >= ?1 AND COUNT(DISTINCT COALESCE(content_hash_sha256, media_path)) > 1
             ORDER BY item_count DESC, variant_count DESC
             LIMIT ?2",
        )
        .map_err(internal_error)?;

    let mut groups = Vec::new();
    let rows = group_stmt
        .query_map(params![min_group_size, group_limit], |row| {
            let parsed_title: String = row.get(0)?;
            let parsed_year: Option<i64> = row.get(1)?;
            let parsed_provider_id: Option<String> = row.get(2)?;
            let item_count: i64 = row.get(3)?;
            let variant_count: i64 = row.get(4)?;
            Ok((
                parsed_title,
                parsed_year,
                parsed_provider_id,
                item_count,
                variant_count,
            ))
        })
        .map_err(internal_error)?;

    for row in rows {
        let (parsed_title, parsed_year, parsed_provider_id, _item_count, _variant_count) =
            row.map_err(internal_error)?;

        let mut item_stmt = conn
            .prepare(
                "SELECT media_path, file_size, content_hash_sha256, video_codec, audio_codec, width, height
                 FROM media_index
                 WHERE parsed_title = ?1
                   AND ((parsed_year = ?2) OR (parsed_year IS NULL AND ?2 IS NULL))
                   AND ((parsed_provider_id = ?3) OR (parsed_provider_id IS NULL AND ?3 IS NULL))
                 ORDER BY media_path ASC",
            )
            .map_err(internal_error)?;

        let item_rows = item_stmt
            .query_map(
                params![parsed_title, parsed_year, parsed_provider_id],
                |item_row| {
                    Ok(SemanticDuplicateItem {
                        media_path: item_row.get(0)?,
                        file_size: item_row.get::<_, i64>(1)? as u64,
                        content_hash: item_row.get(2)?,
                        video_codec: item_row.get(3)?,
                        audio_codec: item_row.get(4)?,
                        width: item_row.get(5)?,
                        height: item_row.get(6)?,
                    })
                },
            )
            .map_err(internal_error)?;

        let mut items = Vec::new();
        for item in item_rows {
            items.push(item.map_err(internal_error)?);
        }

        for subgroup in partition_semantic_group_items(items, min_group_size.max(2) as usize) {
            let subgroup_variant_count = subgroup
                .iter()
                .map(|item| {
                    item.content_hash
                        .as_ref()
                        .cloned()
                        .unwrap_or_else(|| item.media_path.clone())
                })
                .collect::<std::collections::HashSet<String>>()
                .len();

            if subgroup_variant_count <= 1 {
                continue;
            }

            groups.push(SemanticDuplicateGroup {
                parsed_title: parsed_title.clone(),
                parsed_year,
                parsed_provider_id: parsed_provider_id.clone(),
                item_count: subgroup.len(),
                variant_count: subgroup_variant_count,
                items: subgroup,
            });
        }
    }

    Ok(Json(SemanticDuplicatesResponse {
        total_groups: groups.len(),
        groups,
    }))
}

async fn consolidation_quarantine(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ConsolidationQuarantineRequest>,
) -> Result<Json<ConsolidationQuarantineResponse>, (StatusCode, String)> {
    ensure_preflight_ready(&state)?;

    if request.media_paths.len() < 2 {
        return Err((
            StatusCode::BAD_REQUEST,
            "at least two media paths are required to quarantine duplicates".to_string(),
        ));
    }

    let keep_path = PathBuf::from(&request.keep_media_path);
    if let Err(err) = ensure_media_file_path_allowed(&keep_path, &state.library_roots) {
        return Err(err);
    }

    let set: std::collections::HashSet<&str> =
        request.media_paths.iter().map(String::as_str).collect();
    if !set.contains(request.keep_media_path.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            "keep_media_path must be included in media_paths".to_string(),
        ));
    }

    let payload_json = serde_json::to_string(&request).unwrap_or_else(|_| "{}".to_string());
    let job_id = create_job(&state, "consolidation_quarantine", &payload_json);

    let mut items = Vec::new();
    let mut succeeded = 0_usize;
    let mut failed = 0_usize;
    let quarantine_root = state.state_dir.join("quarantine");
    if let Err(err) = fs::create_dir_all(&quarantine_root) {
        complete_job(
            &state,
            job_id,
            JobStatus::Failed,
            None,
            Some(format!("failed to create quarantine root: {err}")),
        );
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to create quarantine root: {err}"),
        ));
    }

    for media_path in &request.media_paths {
        if media_path == &request.keep_media_path {
            items.push(ConsolidationQuarantineItemResult {
                media_path: media_path.clone(),
                quarantined_path: None,
                operation_id: None,
                success: true,
                error: None,
                note: Some("kept".to_string()),
            });
            continue;
        }

        let source = PathBuf::from(media_path);
        if let Err(err) = ensure_media_file_path_allowed(&source, &state.library_roots) {
            failed += 1;
            items.push(ConsolidationQuarantineItemResult {
                media_path: media_path.clone(),
                quarantined_path: None,
                operation_id: None,
                success: false,
                error: Some(err.1),
                note: Some("skipped".to_string()),
            });
            continue;
        }

        let file_name = source
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("unknown.bin");
        let unique = format!(
            "{}-{}-{}",
            current_timestamp_ms(),
            std::process::id(),
            FS_OPERATION_NONCE.fetch_add(1, Ordering::Relaxed)
        );
        let target = quarantine_root.join(format!("{unique}-{file_name}"));

        match apply_rename_with_rollback(&state.state_dir, &source, &target) {
            Ok(operation_id) => {
                succeeded += 1;
                items.push(ConsolidationQuarantineItemResult {
                    media_path: media_path.clone(),
                    quarantined_path: Some(target.display().to_string()),
                    operation_id: Some(operation_id),
                    success: true,
                    error: None,
                    note: Some("quarantined".to_string()),
                });
            }
            Err(err) => {
                failed += 1;
                items.push(ConsolidationQuarantineItemResult {
                    media_path: media_path.clone(),
                    quarantined_path: None,
                    operation_id: None,
                    success: false,
                    error: Some(err),
                    note: Some("failed".to_string()),
                });
            }
        }
    }

    let response = ConsolidationQuarantineResponse {
        keep_media_path: request.keep_media_path,
        total_items: request.media_paths.len(),
        succeeded,
        failed,
        items,
    };

    complete_job(
        &state,
        job_id,
        if failed == 0 {
            JobStatus::Succeeded
        } else {
            JobStatus::Failed
        },
        serde_json::to_string(&response).ok(),
        if failed == 0 {
            None
        } else {
            Some("one or more quarantine operations failed".to_string())
        },
    );

    Ok(Json(response))
}

fn run_library_index_job(
    db_path: &PathBuf,
    roots: &[PathBuf],
    ffprobe_path: &str,
    job_id: i64,
    include_hashes: bool,
    include_probe: bool,
) -> Result<(), String> {
    let media_files = discover_media_files(roots)?;
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;

    let mut hashed_count = 0_usize;
    let mut probed_count = 0_usize;
    for media_path in &media_files {
        let metadata = fs::metadata(media_path).map_err(|e| e.to_string())?;
        let modified_at_ms = metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as i64)
            .unwrap_or(0);

        let item_uid = media_path
            .file_stem()
            .and_then(|v| v.to_str())
            .unwrap_or("unknown-item")
            .to_string();
        let metadata_candidate = infer_metadata_candidate(media_path, &item_uid, None);
        let content_hash = if include_hashes {
            let value = hash_file_sha256(media_path)?;
            hashed_count += 1;
            Some(value)
        } else {
            None
        };
        let probe_summary = if include_probe {
            let summary = run_ffprobe_summary(ffprobe_path, media_path)?;
            if summary.duration_seconds.is_some()
                || summary.video_codec.is_some()
                || summary.audio_codec.is_some()
            {
                probed_count += 1;
            }
            summary
        } else {
            ProbeSummary::default()
        };

        let root = roots
            .iter()
            .find(|candidate| media_path.starts_with(candidate))
            .map(|v| v.display().to_string())
            .unwrap_or_default();

        conn.execute(
            "INSERT INTO media_index(
                media_path,
                root,
                file_size,
                modified_at_ms,
                content_hash_sha256,
                parsed_title,
                parsed_year,
                parsed_provider_id,
                metadata_confidence,
                duration_seconds,
                video_codec,
                audio_codec,
                width,
                height,
                indexed_at_ms
            ) VALUES(
                ?1, ?2, ?3, ?4, ?5,
                ?6, ?7, ?8, ?9, ?10,
                ?11, ?12, ?13, ?14, ?15
            )
            ON CONFLICT(media_path) DO UPDATE SET
                root = excluded.root,
                file_size = excluded.file_size,
                modified_at_ms = excluded.modified_at_ms,
                content_hash_sha256 = excluded.content_hash_sha256,
                parsed_title = excluded.parsed_title,
                parsed_year = excluded.parsed_year,
                parsed_provider_id = excluded.parsed_provider_id,
                metadata_confidence = excluded.metadata_confidence,
                duration_seconds = excluded.duration_seconds,
                video_codec = excluded.video_codec,
                audio_codec = excluded.audio_codec,
                width = excluded.width,
                height = excluded.height,
                indexed_at_ms = excluded.indexed_at_ms",
            params![
                media_path.display().to_string(),
                root,
                metadata.len() as i64,
                modified_at_ms,
                content_hash,
                metadata_candidate.title,
                metadata_candidate.year.map(i64::from),
                metadata_candidate.provider_id,
                metadata_candidate.confidence,
                probe_summary.duration_seconds,
                probe_summary.video_codec,
                probe_summary.audio_codec,
                probe_summary.width,
                probe_summary.height,
                current_timestamp_ms() as i64,
            ],
        )
        .map_err(|e| e.to_string())?;
    }

    let result_json = serde_json::to_string(&json!({
        "indexed": media_files.len(),
        "hashed": hashed_count,
        "probed": probed_count,
    }))
    .map_err(|e| e.to_string())?;

    complete_job_direct(
        db_path,
        job_id,
        JobStatus::Succeeded,
        Some(result_json),
        None,
    )?;

    Ok(())
}

fn discover_media_files(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let allowed = [
        "mkv", "mp4", "avi", "mov", "m4v", "wmv", "flv", "webm", "ts", "m2ts", "mpg", "mpeg",
    ];

    let mut files = Vec::new();
    for root in roots {
        for entry in walkdir::WalkDir::new(root)
            .into_iter()
            .filter_map(Result::ok)
        {
            if !entry.file_type().is_file() {
                continue;
            }

            let extension = entry
                .path()
                .extension()
                .and_then(|v| v.to_str())
                .map(|v| v.to_ascii_lowercase());
            let Some(extension) = extension else {
                continue;
            };
            if !allowed.contains(&extension.as_str()) {
                continue;
            }

            files.push(entry.path().to_path_buf());
        }
    }

    files.sort();
    Ok(files)
}

#[derive(Debug, Default)]
struct ProbeSummary {
    duration_seconds: Option<f64>,
    video_codec: Option<String>,
    audio_codec: Option<String>,
    width: Option<i64>,
    height: Option<i64>,
}

fn run_ffprobe_summary(ffprobe_path: &str, media_path: &PathBuf) -> Result<ProbeSummary, String> {
    let output = Command::new(ffprobe_path)
        .arg("-v")
        .arg("error")
        .arg("-print_format")
        .arg("json")
        .arg("-show_streams")
        .arg("-show_format")
        .arg(media_path)
        .output()
        .map_err(|e| format!("ffprobe failed to execute: {e}"))?;

    if !output.status.success() {
        return Ok(ProbeSummary::default());
    }

    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|e| format!("ffprobe json error: {e}"))?;

    let duration_seconds = parsed
        .get("format")
        .and_then(|v| v.get("duration"))
        .and_then(|v| v.as_str())
        .and_then(|v| v.parse::<f64>().ok());

    let streams = parsed
        .get("streams")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut video_codec = None;
    let mut audio_codec = None;
    let mut width = None;
    let mut height = None;

    for stream in streams {
        let codec_type = stream
            .get("codec_type")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if codec_type == "video" && video_codec.is_none() {
            video_codec = stream
                .get("codec_name")
                .and_then(|v| v.as_str())
                .map(ToString::to_string);
            width = stream.get("width").and_then(|v| v.as_i64());
            height = stream.get("height").and_then(|v| v.as_i64());
        } else if codec_type == "audio" && audio_codec.is_none() {
            audio_codec = stream
                .get("codec_name")
                .and_then(|v| v.as_str())
                .map(ToString::to_string);
        }
    }

    Ok(ProbeSummary {
        duration_seconds,
        video_codec,
        audio_codec,
        width,
        height,
    })
}

fn hash_file_sha256(path: &PathBuf) -> Result<String, String> {
    let file = fs::File::open(path).map_err(|e| format!("open hash input failed: {e}"))?;
    let mut reader = std::io::BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];

    loop {
        let bytes_read = reader
            .read(&mut buffer)
            .map_err(|e| format!("read hash input failed: {e}"))?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    let digest = hasher.finalize();
    Ok(format!("{digest:x}"))
}

fn complete_job_direct(
    db_path: &PathBuf,
    job_id: i64,
    status: JobStatus,
    result_json: Option<String>,
    error: Option<String>,
) -> Result<(), String> {
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE jobs SET status = ?1, updated_at_ms = ?2, result_json = ?3, error = ?4 WHERE id = ?5",
        params![
            match status {
                JobStatus::Running => "running",
                JobStatus::Succeeded => "succeeded",
                JobStatus::Failed => "failed",
                JobStatus::Canceled => "canceled",
            },
            current_timestamp_ms() as i64,
            result_json,
            error,
            job_id,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
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
    if let Some(desired_state) = request.desired_state {
        sidecar_state.preferred_policy_state =
            serde_json::to_value(desired_state).map_err(internal_error)?;
    }

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

    let plan = match sidecar_workflow::build_plan_with_desired_state(
        &media_path,
        &request.item_uid,
        request.desired_state.as_ref(),
    ) {
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

    let result = sidecar_workflow::apply_plan_with_desired_state(
        &media_path,
        &request.item_uid,
        &request.plan_hash,
        &state.state_dir,
        request.desired_state.as_ref(),
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

            if let Err((_, err)) = ensure_media_file_path_allowed(&source_path, &state.library_roots)
            {
                applied_items.push(BulkApplyItemResult {
                    media_path: item.media_path,
                    final_media_path: Some(target_path_value),
                    item_uid: item.item_uid,
                    applied_provider_id: None,
                    success: false,
                    operation_id: None,
                    sidecar_path: None,
                    error: Some(format!(
                        "rename source changed after preview and is no longer valid: {err}"
                    )),
                });
                continue;
            }

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
                Ok(operation_id) => {
                    if let Err(err) = rename_linked_sidecar_family(&source_path, &target_path) {
                        let rollback_result = rollback_fs_operation(state, &operation_id);
                        let rollback_note = match rollback_result {
                            Ok(detail) => format!("; media rollback succeeded: {detail}"),
                            Err(rollback_err) => {
                                format!("; media rollback failed: {rollback_err}")
                            }
                        };
                        applied_items.push(BulkApplyItemResult {
                            media_path: item.media_path,
                            final_media_path: Some(target_path_value),
                            item_uid: item.item_uid,
                            applied_provider_id: None,
                            success: false,
                            operation_id: None,
                            sidecar_path: None,
                            error: Some(format!(
                                "rename sidecar update failed: {err}{rollback_note}"
                            )),
                        });
                        continue;
                    }

                    applied_items.push(BulkApplyItemResult {
                        media_path: item.media_path,
                        final_media_path: Some(target_path_value),
                        item_uid: item.item_uid,
                        applied_provider_id: None,
                        success: true,
                        operation_id: Some(operation_id),
                        sidecar_path: None,
                        error: None,
                    })
                }
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
            match compute_rename_target(
                &media_path,
                item.metadata_override.as_ref(),
                item.rename_parent_folder.unwrap_or(false),
            ) {
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

fn compute_rename_target(
    media_path: &std::path::Path,
    metadata_override: Option<&MetadataOverrideInput>,
    allow_multi_file_parent_rename: bool,
) -> Result<(PathBuf, String), String> {
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

    let inferred = infer_metadata_candidate(media_path, file_stem, metadata_override);
    let normalized_title = normalize_title_for_display_name(&inferred.title);
    let sanitized_title = sanitize_display_filename_component(&normalized_title);
    let fallback_title =
        sanitize_display_filename_component(&normalize_title_for_display_name(file_stem));
    let final_title = if sanitized_title.is_empty() {
        fallback_title
    } else {
        sanitized_title
    };

    if final_title.is_empty() {
        return Err("rename target stem is empty after normalization".to_string());
    }

    let (mut normalized, mut was_truncated) = build_rename_stem_with_limits(
        parent,
        false,
        &final_title,
        inferred.year,
        &extension,
    )?;

    let should_move_parent = should_move_into_canonical_folder(
        parent,
        &normalized,
        allow_multi_file_parent_rename,
    );
    if should_move_parent {
        let (moved_normalized, moved_was_truncated) = build_rename_stem_with_limits(
            parent,
            true,
            &final_title,
            inferred.year,
            &extension,
        )?;
        normalized = moved_normalized;
        was_truncated = was_truncated || moved_was_truncated;
    }

    let target_parent = if should_move_parent {
        parent.with_file_name(&normalized)
    } else {
        parent.to_path_buf()
    };

    let target = target_parent.join(format!("{normalized}{extension}"));
    if target == media_path {
        return Ok((
            target,
            "already matches Movie Name - Subtitle (Year) format".to_string(),
        ));
    }

    if target.exists() {
        if was_truncated {
            return Err(format!(
                "rename collision after truncation: target already exists ({})",
                target.display()
            ));
        }
        return Err(format!(
            "rename collision: target already exists ({})",
            target.display()
        ));
    }

    let note = if was_truncated {
        "will rename to Movie Name - Subtitle (Year) format (truncated to fit filesystem limits)"
            .to_string()
    } else {
        "will rename to Movie Name - Subtitle (Year) format".to_string()
    };

    Ok((target, note))
}

fn build_rename_stem_with_limits(
    source_parent: &std::path::Path,
    move_into_canonical_parent: bool,
    title: &str,
    year: Option<u16>,
    extension: &str,
) -> Result<(String, bool), String> {
    let suffix = year
        .map(|value| format!(" ({value})"))
        .unwrap_or_default();
    let mut title_candidate = title.trim().to_string();
    if title_candidate.is_empty() {
        return Err("rename target stem is empty after normalization".to_string());
    }

    let mut truncated = false;
    loop {
        let stem = format!("{title_candidate}{suffix}");
        if rename_stem_fits_limits(source_parent, move_into_canonical_parent, &stem, extension) {
            return Ok((stem, truncated));
        }

        truncated = true;
        let Some(_) = title_candidate.pop() else {
            break;
        };
        title_candidate = title_candidate
            .trim_end_matches(|ch: char| ch.is_whitespace() || ch == '-' || ch == '_')
            .to_string();
        if title_candidate.is_empty() {
            title_candidate = "untitled".to_string();
        }
    }

    Err("rename target exceeds filesystem limits and cannot be truncated safely".to_string())
}


fn rename_stem_fits_limits(
    source_parent: &std::path::Path,
    move_into_canonical_parent: bool,
    stem: &str,
    extension: &str,
) -> bool {
    let stem_bytes = stem.as_bytes().len();
    if stem_bytes > MAX_PATH_COMPONENT_BYTES {
        return false;
    }

    let file_name = format!("{stem}{extension}");
    if file_name.as_bytes().len() > MAX_PATH_COMPONENT_BYTES {
        return false;
    }

    let target_parent = if move_into_canonical_parent {
        source_parent.with_file_name(stem)
    } else {
        source_parent.to_path_buf()
    };

    let target_path = target_parent.join(&file_name);
    target_path.to_string_lossy().as_bytes().len() <= MAX_PATH_BYTES
}

fn sanitize_display_filename_component(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut previous_was_space = false;
    let mut previous_was_dash = false;

    for ch in value.chars() {
        let mapped = if matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
            '-'
        } else {
            ch
        };

        if mapped.is_whitespace() {
            if !previous_was_space {
                output.push(' ');
            }
            previous_was_space = true;
            previous_was_dash = false;
        } else if mapped == '-' {
            if !previous_was_dash {
                output.push('-');
            }
            previous_was_space = false;
            previous_was_dash = true;
        } else {
            output.push(mapped);
            previous_was_space = false;
            previous_was_dash = false;
        }
    }

    output.trim_matches(|ch: char| ch.is_whitespace() || ch == '-').to_string()
}

fn normalize_title_for_display_name(value: &str) -> String {
    let mut normalized = value
        .replace(['.', '_'], " ")
        .replace(':', " - ");

    // Normalize existing separators to a single spaced dash for subtitle-like titles.
    normalized = normalized
        .replace(" - ", " __MM_DASH_SEP__ ")
        .replace('-', " ")
        .replace("__MM_DASH_SEP__", "-");

    let mut output = String::with_capacity(normalized.len());
    let mut previous_was_space = false;
    for ch in normalized.chars() {
        if ch.is_whitespace() {
            if !previous_was_space {
                output.push(' ');
            }
            previous_was_space = true;
        } else {
            output.push(ch);
            previous_was_space = false;
        }
    }

    let compact = output.trim();
    compact
        .split('-')
        .map(|part| part.trim())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" - ")
}

fn should_move_into_canonical_folder(
    parent: &std::path::Path,
    canonical_stem: &str,
    allow_multi_file_parent_rename: bool,
) -> bool {
    let Some(folder_name) = parent.file_name().and_then(|v| v.to_str()) else {
        return false;
    };

    let entries = match fs::read_dir(parent) {
        Ok(v) => v,
        Err(_) => return false,
    };

    let mut media_count = 0_usize;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            continue;
        }
        if !path.is_file() {
            continue;
        }
        let Some(ext) = path.extension().and_then(|v| v.to_str()) else {
            continue;
        };
        let ext = ext.to_ascii_lowercase();
        if matches!(
            ext.as_str(),
            "mkv" | "mp4" | "avi" | "mov" | "wmv" | "m4v" | "mpg" | "mpeg" | "ts" | "m2ts" | "webm"
        ) {
            media_count += 1;
        }
    }

    if media_count == 0 {
        return false;
    }

    if media_count > 1 && !allow_multi_file_parent_rename {
        return false;
    }

    // Skip moving when the parent already matches the canonical folder name.
    let normalized_folder = sanitize_display_filename_component(&normalize_title_for_display_name(folder_name));
    let normalized_canonical =
        sanitize_display_filename_component(&normalize_title_for_display_name(canonical_stem));

    !normalized_folder.eq_ignore_ascii_case(&normalized_canonical)
}

fn rename_linked_sidecar_family(
    source_media_path: &std::path::Path,
    target_media_path: &std::path::Path,
) -> Result<(), String> {
    let source_parent = source_media_path
        .parent()
        .ok_or_else(|| "cannot determine source parent directory".to_string())?;
    let target_parent = target_media_path
        .parent()
        .ok_or_else(|| "cannot determine target parent directory".to_string())?;

    let source_stem = source_media_path
        .file_stem()
        .and_then(|v| v.to_str())
        .ok_or_else(|| "cannot determine source media stem".to_string())?;
    let target_stem = target_media_path
        .file_stem()
        .and_then(|v| v.to_str())
        .ok_or_else(|| "cannot determine target media stem".to_string())?;

    if source_stem == target_stem && source_parent == target_parent {
        return Ok(());
    }

    let mut rename_pairs: Vec<(PathBuf, PathBuf)> = Vec::new();
    let entries = fs::read_dir(source_parent)
        .map_err(|err| format!("read_dir {} ({err})", source_parent.display()))?;

    for entry in entries {
        let entry = entry.map_err(|err| format!("read_dir entry failure ({err})"))?;
        let source_path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|err| format!("read_dir file type failure ({err})"))?;

        if source_path == source_media_path {
            continue;
        }

        if file_type.is_symlink() {
            return Err(format!(
                "linked sidecar entry is symlink and cannot be renamed safely ({})",
                source_path.display()
            ));
        }

        if file_type.is_dir() {
            let Some(dir_name) = source_path.file_name().and_then(|v| v.to_str()) else {
                continue;
            };
            if !is_trickplay_directory_name(dir_name, source_stem) {
                continue;
            }
            let target_name = rename_trickplay_directory_name(dir_name, source_stem, target_stem);
            let target_path = target_parent.join(target_name);
            if source_path == target_path {
                continue;
            }
            if target_path.exists() {
                return Err(format!(
                    "target linked sidecar path already exists ({})",
                    target_path.display()
                ));
            }
            rename_pairs.push((source_path, target_path));
            continue;
        }

        if !file_type.is_file() {
            continue;
        }

        let Some(file_name) = source_path.file_name().and_then(|v| v.to_str()) else {
            continue;
        };

        let Some(target_name) = linked_family_target_name(file_name, source_stem, target_stem)
        else {
            continue;
        };
        let target_path = target_parent.join(target_name);
        if source_path == target_path {
            continue;
        }
        if target_path.exists() {
            return Err(format!(
                "target linked sidecar path already exists ({})",
                target_path.display()
            ));
        }
        rename_pairs.push((source_path, target_path));
    }

    if source_parent != target_parent {
        fs::create_dir_all(target_parent).map_err(|err| {
            format!(
                "failed to create target parent {} ({err})",
                target_parent.display()
            )
        })?;
    }

    rename_pairs.sort_by(|(left_source, _), (right_source, _)| left_source.cmp(right_source));

    let mut completed_pairs: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (source_path, target_path) in rename_pairs {
        if let Err(err) = fs::rename(&source_path, &target_path) {
            for (completed_source, completed_target) in completed_pairs.iter().rev() {
                let _ = fs::rename(completed_target, completed_source);
            }
            return Err(format!(
                "failed to rename linked sidecar {} -> {} ({err})",
                source_path.display(),
                target_path.display()
            ));
        }
        completed_pairs.push((source_path, target_path));
    }

    if source_parent != target_parent {
        let is_empty = fs::read_dir(source_parent)
            .ok()
            .and_then(|mut entries| entries.next())
            .is_none();
        if is_empty {
            fs::remove_dir(source_parent).map_err(|err| {
                format!(
                    "failed to remove empty source parent {} ({err})",
                    source_parent.display()
                )
            })?;
        }
    }

    Ok(())
}

fn linked_family_target_name(
    file_name: &str,
    source_stem: &str,
    target_stem: &str,
) -> Option<String> {
    let path = std::path::Path::new(file_name);
    let stem = path.file_stem().and_then(|v| v.to_str())?;
    let extension = path
        .extension()
        .and_then(|v| v.to_str())
        .map(|v| v.to_ascii_lowercase());
    let stem_lower = stem.to_ascii_lowercase();

    let mapped_stem = if stem == source_stem {
        Some(target_stem.to_string())
    } else if let Some(rest) = stem.strip_prefix(&format!("{source_stem}.")) {
        Some(format!("{target_stem}.{rest}"))
    } else if let Some(rest) = stem.strip_prefix(&format!("{source_stem}-")) {
        Some(format!("{target_stem}-{rest}"))
    } else if let Some(rest) = stem.strip_prefix(&format!("{source_stem}_")) {
        Some(format!("{target_stem}_{rest}"))
    } else if stem_lower == "movie" && extension.as_deref() == Some("nfo") {
        Some(format!("{target_stem}-movie"))
    } else if is_folder_level_image_alias(&stem_lower, extension.as_deref()) {
        Some(format!("{target_stem}-{stem_lower}"))
    } else {
        None
    }?;

    match extension {
        Some(ext) => Some(format!("{mapped_stem}.{ext}")),
        None => Some(mapped_stem),
    }
}

fn is_folder_level_image_alias(stem_lower: &str, extension: Option<&str>) -> bool {
    let Some(ext) = extension else {
        return false;
    };
    if !matches!(ext, "jpg" | "jpeg" | "png" | "webp" | "tbn") {
        return false;
    }

    matches!(
        stem_lower,
        "poster"
            | "fanart"
            | "banner"
            | "clearlogo"
            | "clearart"
            | "landscape"
            | "thumb"
            | "logo"
            | "folder"
            | "discart"
            | "disc"
            | "keyart"
    )
}

fn is_trickplay_directory_name(dir_name: &str, source_stem: &str) -> bool {
    let lower = dir_name.to_ascii_lowercase();
    let source_lower = source_stem.to_ascii_lowercase();
    lower.starts_with(&source_lower) && lower.contains("trickplay")
}

fn rename_trickplay_directory_name(dir_name: &str, source_stem: &str, target_stem: &str) -> String {
    if let Some(rest) = dir_name.strip_prefix(source_stem) {
        return format!("{target_stem}{rest}");
    }

    let lower = dir_name.to_ascii_lowercase();
    let source_lower = source_stem.to_ascii_lowercase();
    if let Some(index) = lower.find(&source_lower) {
        let end = index + source_lower.len();
        return format!("{}{}{}", &dir_name[..index], target_stem, &dir_name[end..]);
    }

    dir_name.to_string()
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
    if let Some(parsed) = parse_nfo_metadata_for_media_path(media_path) {
        return build_semantic_duplicate_key(&parsed);
    }

    if let Some(parsed) = parse_folder_metadata_from_media_path(media_path) {
        return build_semantic_duplicate_key(&parsed);
    }

    let stem = media_path
        .file_stem()
        .and_then(|v| v.to_str())
        .unwrap_or("unknown-item");
    normalize_duplicate_key(stem)
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

fn partition_semantic_group_items(
    items: Vec<SemanticDuplicateItem>,
    min_group_size: usize,
) -> Vec<Vec<SemanticDuplicateItem>> {
    let mut episode_buckets: HashMap<String, Vec<SemanticDuplicateItem>> = HashMap::new();
    let mut non_episode_items: Vec<SemanticDuplicateItem> = Vec::new();

    for item in items {
        let episode_key = extract_episode_signature_from_media_path(&item.media_path);
        if let Some(key) = episode_key {
            episode_buckets.entry(key).or_default().push(item);
        } else {
            non_episode_items.push(item);
        }
    }

    let mut output: Vec<Vec<SemanticDuplicateItem>> = Vec::new();
    for bucket in episode_buckets.into_values() {
        if bucket.len() >= min_group_size {
            output.push(bucket);
        }
    }

    if non_episode_items.len() >= min_group_size {
        output.push(non_episode_items);
    }

    output
}

fn extract_episode_signature_from_media_path(media_path: &str) -> Option<String> {
    let file_name = PathBuf::from(media_path)
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or(media_path)
        .to_ascii_uppercase();

    let chars: Vec<char> = file_name.chars().collect();
    let mut i = 0_usize;
    while i < chars.len() {
        if chars[i] != 'S' {
            i += 1;
            continue;
        }

        let mut j = i + 1;
        let season_start = j;
        while j < chars.len() && chars[j].is_ascii_digit() && j - season_start < 4 {
            j += 1;
        }
        let season_len = j.saturating_sub(season_start);
        if season_len == 0 || j >= chars.len() || chars[j] != 'E' {
            i += 1;
            continue;
        }

        j += 1;
        let episode_start = j;
        while j < chars.len() && chars[j].is_ascii_digit() && j - episode_start < 3 {
            j += 1;
        }
        let episode_len = j.saturating_sub(episode_start);
        if episode_len == 0 {
            i += 1;
            continue;
        }

        let season_raw: String = chars[season_start..season_start + season_len]
            .iter()
            .collect();
        let episode_raw: String = chars[episode_start..episode_start + episode_len]
            .iter()
            .collect();

        let season = season_raw.parse::<u16>().ok()?;
        let episode = episode_raw.parse::<u16>().ok()?;
        return Some(format!("S{season:04}E{episode:03}"));
    }

    None
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
    if let Some(parsed) = parse_nfo_metadata_for_media_path(media_path) {
        let override_title = metadata_override.and_then(|v| normalize_optional_string(&v.title));
        let override_provider_id =
            metadata_override.and_then(|v| normalize_optional_string(&v.provider_id));
        let override_year = metadata_override.and_then(|v| v.year);
        let override_confidence = metadata_override.and_then(|v| v.confidence);

        let provider_id = parsed
            .provider_id
            .unwrap_or_else(|| build_local_provider_id(&parsed.title, parsed.year));

        return MetadataCandidate {
            title: override_title.unwrap_or(parsed.title),
            year: override_year.or(parsed.year),
            provider_id: override_provider_id.unwrap_or(provider_id),
            confidence: override_confidence
                .unwrap_or(parsed.confidence)
                .clamp(0.0, 1.0),
        };
    }

    if let Some(parsed) = parse_folder_metadata_from_media_path(media_path) {
        let override_title = metadata_override.and_then(|v| normalize_optional_string(&v.title));
        let override_provider_id =
            metadata_override.and_then(|v| normalize_optional_string(&v.provider_id));
        let override_year = metadata_override.and_then(|v| v.year);
        let override_confidence = metadata_override.and_then(|v| v.confidence);

        let provider_id = parsed
            .provider_id
            .unwrap_or_else(|| build_local_provider_id(&parsed.title, parsed.year));

        return MetadataCandidate {
            title: override_title.unwrap_or(parsed.title),
            year: override_year.or(parsed.year),
            provider_id: override_provider_id.unwrap_or(provider_id),
            confidence: override_confidence
                .unwrap_or(parsed.confidence)
                .clamp(0.0, 1.0),
        };
    }

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

    let provider_id = build_local_provider_id(&title, year);

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

#[derive(Debug, Clone)]
struct ParsedFolderMetadata {
    title: String,
    year: Option<u16>,
    provider_id: Option<String>,
    confidence: f32,
}

fn build_semantic_duplicate_key(parsed: &ParsedFolderMetadata) -> String {
    let title_key = normalize_filename_stem(&parsed.title).to_ascii_lowercase();
    format!(
        "{}|{}|{}",
        title_key,
        parsed
            .year
            .map(|v| v.to_string())
            .unwrap_or_else(|| "none".to_string()),
        parsed
            .provider_id
            .as_ref()
            .map(|v| v.to_ascii_lowercase())
            .unwrap_or_else(|| "provider:none".to_string())
    )
}

fn build_local_provider_id(title: &str, year: Option<u16>) -> String {
    let mut hasher = DefaultHasher::new();
    title.to_ascii_lowercase().hash(&mut hasher);
    year.unwrap_or(0).hash(&mut hasher);
    format!("tmdb-local-{:08x}", (hasher.finish() & 0xffff_ffff))
}

fn parse_folder_metadata_from_media_path(
    media_path: &std::path::Path,
) -> Option<ParsedFolderMetadata> {
    let folder_name = media_path.parent()?.file_name()?.to_str()?;
    parse_movie_folder_pattern(folder_name)
}

fn parse_nfo_metadata_for_media_path(media_path: &std::path::Path) -> Option<ParsedFolderMetadata> {
    let parent = media_path.parent()?;
    let stem = media_path.file_stem().and_then(|v| v.to_str())?;
    let candidates = [parent.join(format!("{stem}.nfo")), parent.join("movie.nfo")];

    for candidate in &candidates {
        if !candidate.exists() || !candidate.is_file() {
            continue;
        }

        let content = match fs::read_to_string(candidate) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if content.trim().is_empty() {
            continue;
        }

        if let Some(parsed) = parse_nfo_metadata_content(&content) {
            return Some(parsed);
        }
    }

    None
}

fn parse_nfo_metadata_content(content: &str) -> Option<ParsedFolderMetadata> {
    let title = extract_xml_tag_value(content, "title")
        .or_else(|| extract_xml_tag_value(content, "originaltitle"))
        .map(|v| normalize_filename_stem(&v))
        .filter(|v| !v.is_empty())?;

    let year = extract_xml_tag_value(content, "year")
        .and_then(|v| parse_year_token(&v))
        .or_else(|| {
            extract_xml_tag_value(content, "premiered")
                .and_then(|v| v.get(0..4).map(ToString::to_string))
                .and_then(|v| parse_year_token(&v))
        });

    let provider_id = extract_nfo_provider_id(content);

    Some(ParsedFolderMetadata {
        title,
        year,
        provider_id,
        confidence: 0.99,
    })
}

fn extract_xml_tag_value(content: &str, tag: &str) -> Option<String> {
    let lower = content.to_ascii_lowercase();
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = lower.find(&open)? + open.len();
    let end = lower[start..].find(&close)? + start;
    let value = content.get(start..end)?.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn extract_nfo_provider_id(content: &str) -> Option<String> {
    if let Some(imdbid) = extract_xml_tag_value(content, "imdbid") {
        let compact: String = imdbid
            .chars()
            .filter(|ch| !ch.is_ascii_whitespace())
            .collect();
        if compact.starts_with("tt") && compact[2..].chars().all(|ch| ch.is_ascii_digit()) {
            return Some(format!("imdb-{compact}"));
        }
    }

    if let Some(tmdbid) = extract_xml_tag_value(content, "tmdbid") {
        if tmdbid.chars().all(|ch| ch.is_ascii_digit()) {
            return Some(format!("tmdb-{tmdbid}"));
        }
    }

    let lower = content.to_ascii_lowercase();
    let mut cursor = 0;
    while let Some(start_rel) = lower[cursor..].find("<uniqueid") {
        let start = cursor + start_rel;
        let Some(head_end_rel) = lower[start..].find('>') else {
            break;
        };
        let head_end = start + head_end_rel;
        let head = &lower[start..=head_end];
        let Some(close_rel) = lower[head_end + 1..].find("</uniqueid>") else {
            break;
        };
        let close = head_end + 1 + close_rel;
        let value = content
            .get(head_end + 1..close)
            .map(|v| v.trim().to_string())
            .unwrap_or_default();

        if head.contains("type=\"imdb\"") || head.contains("type='imdb'") {
            let compact: String = value
                .chars()
                .filter(|ch| !ch.is_ascii_whitespace())
                .collect();
            if compact.starts_with("tt") && compact[2..].chars().all(|ch| ch.is_ascii_digit()) {
                return Some(format!("imdb-{compact}"));
            }
        }

        if head.contains("type=\"tmdb\"") || head.contains("type='tmdb'") {
            let compact: String = value
                .chars()
                .filter(|ch| !ch.is_ascii_whitespace())
                .collect();
            if compact.chars().all(|ch| ch.is_ascii_digit()) {
                return Some(format!("tmdb-{compact}"));
            }
        }

        cursor = close + "</uniqueid>".len();
    }

    None
}

fn parse_year_token(value: &str) -> Option<u16> {
    let trimmed = value.trim();
    if trimmed.len() != 4 {
        return None;
    }
    let year = trimmed.parse::<u16>().ok()?;
    if (1900..=2100).contains(&year) {
        Some(year)
    } else {
        None
    }
}

fn parse_movie_folder_pattern(folder_name: &str) -> Option<ParsedFolderMetadata> {
    if folder_name.trim().is_empty() {
        return None;
    }

    let (provider_id, cleaned_provider) = extract_provider_id(folder_name);
    let (year, cleaned_year) = extract_year(&cleaned_provider);
    let title = cleaned_year.trim().to_string();
    if title.is_empty() || (year.is_none() && provider_id.is_none()) {
        return None;
    }

    let mut confidence: f32 = 0.68;
    if year.is_some() {
        confidence += 0.17;
    }
    if provider_id.is_some() {
        confidence += 0.12;
    }

    Some(ParsedFolderMetadata {
        title,
        year,
        provider_id,
        confidence: confidence.min(0.98),
    })
}

fn extract_provider_id(input: &str) -> (Option<String>, String) {
    let mut working = input.to_string();
    let mut provider_id: Option<String> = None;

    let mut cursor = 0;
    while let Some(start_rel) = working[cursor..].find('[') {
        let start = cursor + start_rel;
        let Some(end_rel) = working[start + 1..].find(']') else {
            break;
        };
        let end = start + 1 + end_rel;
        let raw = working[start + 1..end].trim().to_ascii_lowercase();
        let compact: String = raw
            .chars()
            .filter(|ch| !ch.is_ascii_whitespace() && *ch != '_')
            .collect();

        if let Some(rest) = compact.strip_prefix("imdb-") {
            if rest.starts_with("tt") && rest[2..].chars().all(|ch| ch.is_ascii_digit()) {
                provider_id = Some(format!("imdb-{rest}"));
            }
        } else if let Some(rest) = compact.strip_prefix("imdbtt") {
            if !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit()) {
                provider_id = Some(format!("imdb-tt{rest}"));
            }
        } else if let Some(rest) = compact.strip_prefix("tmdbid-") {
            if !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit()) {
                provider_id = Some(format!("tmdb-{rest}"));
            }
        } else if let Some(rest) = compact.strip_prefix("tmdbid") {
            if !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit()) {
                provider_id = Some(format!("tmdb-{rest}"));
            }
        } else if let Some(rest) = compact.strip_prefix("tmdb-") {
            if !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit()) {
                provider_id = Some(format!("tmdb-{rest}"));
            }
        }

        if provider_id.is_some() {
            working.replace_range(start..=end, "");
            break;
        }

        cursor = end + 1;
    }

    (provider_id, normalize_filename_stem(&working))
}

fn extract_year(input: &str) -> (Option<u16>, String) {
    let mut year = None;
    let mut working = input.to_string();

    if let Some(start) = working.rfind('(') {
        if let Some(end_rel) = working[start + 1..].find(')') {
            let end = start + 1 + end_rel;
            let candidate = working[start + 1..end].trim();
            if candidate.len() == 4 {
                if let Ok(parsed) = candidate.parse::<u16>() {
                    if (1900..=2100).contains(&parsed) {
                        year = Some(parsed);
                        working.replace_range(start..=end, "");
                        return (year, normalize_filename_stem(&working));
                    }
                }
            }
        }
    }

    for token in input.split_whitespace() {
        if token.len() == 4 {
            if let Ok(parsed) = token.parse::<u16>() {
                if (1900..=2100).contains(&parsed) {
                    year = Some(parsed);
                    let marker = token.to_string();
                    working = working.replace(&marker, " ");
                    break;
                }
            }
        }
    }

    (year, normalize_filename_stem(&working))
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
struct IndexStartRequest {
    include_hashes: Option<bool>,
    include_probe: Option<bool>,
}

#[derive(Debug, Serialize)]
struct IndexStartResponse {
    job_id: i64,
    started: bool,
    message: String,
}

#[derive(Debug, Serialize)]
struct IndexStatsResponse {
    total_indexed: usize,
    hashed: usize,
    probed: usize,
    last_indexed_at_ms: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct IndexItemsQuery {
    q: Option<String>,
    offset: Option<usize>,
    limit: Option<usize>,
    only_missing_provider: Option<bool>,
    min_confidence: Option<f32>,
    max_confidence: Option<f32>,
}

#[derive(Debug, Serialize)]
struct IndexItemsResponse {
    total_items: usize,
    offset: usize,
    limit: usize,
    items: Vec<IndexedMediaItem>,
}

#[derive(Debug, Serialize)]
struct IndexedMediaItem {
    media_path: String,
    root: String,
    parsed_title: Option<String>,
    parsed_year: Option<i64>,
    parsed_provider_id: Option<String>,
    metadata_confidence: Option<f32>,
    content_hash_sha256: Option<String>,
    duration_seconds: Option<f64>,
    video_codec: Option<String>,
    audio_codec: Option<String>,
    width: Option<i64>,
    height: Option<i64>,
    indexed_at_ms: i64,
}

#[derive(Debug, Deserialize)]
struct FormattingCandidatesQuery {
    offset: Option<usize>,
    limit: Option<usize>,
}

#[derive(Debug, Serialize)]
struct FormattingCandidatesResponse {
    total_items: usize,
    offset: usize,
    limit: usize,
    items: Vec<FormattingCandidateItem>,
}

#[derive(Debug, Serialize)]
struct FormattingCandidateItem {
    media_path: String,
    proposed_media_path: String,
    note: String,
}

#[derive(Debug, Deserialize)]
struct ExactDuplicatesQuery {
    limit: Option<usize>,
    min_group_size: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct SemanticDuplicatesQuery {
    limit: Option<usize>,
    min_group_size: Option<usize>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ConsolidationQuarantineRequest {
    keep_media_path: String,
    media_paths: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ExactDuplicatesResponse {
    total_groups: usize,
    groups: Vec<ExactDuplicateGroup>,
}

#[derive(Debug, Serialize)]
struct ExactDuplicateGroup {
    content_hash: String,
    count: usize,
    items: Vec<ExactDuplicateItem>,
}

#[derive(Debug, Serialize)]
struct ExactDuplicateItem {
    media_path: String,
    file_size: u64,
    parsed_title: Option<String>,
    parsed_year: Option<i64>,
    parsed_provider_id: Option<String>,
    video_codec: Option<String>,
    audio_codec: Option<String>,
    width: Option<i64>,
    height: Option<i64>,
    duration_seconds: Option<f64>,
}

#[derive(Debug, Serialize)]
struct SemanticDuplicatesResponse {
    total_groups: usize,
    groups: Vec<SemanticDuplicateGroup>,
}

#[derive(Debug, Serialize)]
struct SemanticDuplicateGroup {
    parsed_title: String,
    parsed_year: Option<i64>,
    parsed_provider_id: Option<String>,
    item_count: usize,
    variant_count: usize,
    items: Vec<SemanticDuplicateItem>,
}

#[derive(Debug, Serialize)]
struct SemanticDuplicateItem {
    media_path: String,
    file_size: u64,
    content_hash: Option<String>,
    video_codec: Option<String>,
    audio_codec: Option<String>,
    width: Option<i64>,
    height: Option<i64>,
}

#[derive(Debug, Serialize)]
struct ConsolidationQuarantineResponse {
    keep_media_path: String,
    total_items: usize,
    succeeded: usize,
    failed: usize,
    items: Vec<ConsolidationQuarantineItemResult>,
}

#[derive(Debug, Serialize)]
struct ConsolidationQuarantineItemResult {
    media_path: String,
    quarantined_path: Option<String>,
    operation_id: Option<String>,
    success: bool,
    error: Option<String>,
    note: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SidecarUpsertRequest {
    media_path: String,
    item_uid: String,
    desired_state: Option<DesiredMediaState>,
}

#[derive(Debug, Deserialize)]
struct SidecarApplyRequest {
    media_path: String,
    item_uid: String,
    plan_hash: String,
    desired_state: Option<DesiredMediaState>,
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
    rename_parent_folder: Option<bool>,
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
    use std::sync::Arc;

    use std::fs;
    use std::path::PathBuf;

    use axum::http::StatusCode;
    use rusqlite::Connection;

    use crate::audit_store::AuditStore;
    use crate::config::{BrandingConfig, BrandingThemeTokens};
    use crate::jobs_store::JobsStore;
    use crate::operations::OperationLog;
    use crate::toolchain::{ProbeStatus, ResolvedBinary, ToolchainSnapshot};

    use super::{
        AppState, BulkApplyRequest, BulkDryRunRequest, BulkItemInput, DEFAULT_LIBRARY_LIMIT,
        DEFAULT_RECENT_LIMIT, MAX_LIBRARY_LIMIT, MAX_RECENT_LIMIT, MetadataOverrideInput,
        SemanticDuplicateItem, build_duplicate_groups, compute_nfo_target, compute_rename_target,
        duplicate_key_for_media_path, ensure_media_file_path_allowed, execute_bulk_apply,
        execute_bulk_dry_run, extract_episode_signature_from_media_path, infer_metadata_candidate,
        normalize_bulk_action, normalize_duplicate_key, normalize_filename_stem,
        normalize_job_status_filter, normalize_library_limit, normalize_recent_limit,
        partition_semantic_group_items, rename_linked_sidecar_family,
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

    fn test_toolchain_snapshot() -> ToolchainSnapshot {
        ToolchainSnapshot {
            ffmpeg: ResolvedBinary {
                command_name: "ffmpeg".to_string(),
                path: "/bin/ffmpeg".to_string(),
                version_output: Some("ok".to_string()),
                status: ProbeStatus::Ok,
            },
            ffprobe: ResolvedBinary {
                command_name: "ffprobe".to_string(),
                path: "/bin/ffprobe".to_string(),
                version_output: Some("ok".to_string()),
                status: ProbeStatus::Ok,
            },
            mediainfo: None,
        }
    }

    fn create_minimal_tables(db_path: &PathBuf) {
        let conn = Connection::open(db_path).expect("open sqlite db");
        conn.execute(
            "CREATE TABLE IF NOT EXISTS operation_events(\
                id INTEGER PRIMARY KEY AUTOINCREMENT,\
                timestamp_ms INTEGER NOT NULL,\
                kind TEXT NOT NULL,\
                detail TEXT NOT NULL,\
                success INTEGER NOT NULL\
            )",
            [],
        )
        .expect("create operation_events table");
        conn.execute(
            "CREATE TABLE IF NOT EXISTS jobs(\
                id INTEGER PRIMARY KEY AUTOINCREMENT,\
                kind TEXT NOT NULL,\
                status TEXT NOT NULL,\
                created_at_ms INTEGER NOT NULL,\
                updated_at_ms INTEGER NOT NULL,\
                payload_json TEXT NOT NULL,\
                result_json TEXT,\
                error TEXT\
            )",
            [],
        )
        .expect("create jobs table");
    }

    fn test_app_state(library_root: &PathBuf, state_dir: &PathBuf) -> Arc<AppState> {
        fs::create_dir_all(state_dir).expect("create state dir");
        let audit_db_path = state_dir.join("audit.sqlite3");
        let jobs_db_path = state_dir.join("jobs.sqlite3");
        create_minimal_tables(&audit_db_path);
        create_minimal_tables(&jobs_db_path);

        let audit_store = AuditStore::open(&audit_db_path).expect("open audit store");
        let jobs_store = JobsStore::open(&jobs_db_path).expect("open jobs store");

        Arc::new(AppState {
            branding: BrandingConfig {
                app_name: "Media Manager".to_string(),
                short_name: "MM".to_string(),
                logo_url: "/assets/logo.svg".to_string(),
                browser_title_template: "{app_name}".to_string(),
                theme_tokens: BrandingThemeTokens {
                    accent: "#0f766e".to_string(),
                    accent_contrast: "#f8fafc".to_string(),
                },
            },
            toolchain: test_toolchain_snapshot(),
            library_roots: vec![library_root.clone()],
            state_dir: state_dir.clone(),
            audit_db_path,
            api_token: None,
            operation_log: OperationLog::new(),
            audit_store,
            jobs_store,
        })
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
    fn rename_target_preserves_extension_and_uses_canonical_parent() {
        let root = unique_temp_dir("rename-target");
        fs::create_dir_all(&root).expect("create root");
        let movie_dir = root.join("My Movie (2024)");
        fs::create_dir_all(&movie_dir).expect("create movie dir");
        let media_path = movie_dir.join("My.Movie_2024.mkv");
        fs::write(&media_path, b"x").expect("write media file");

        let (target, note) =
            compute_rename_target(&media_path, None, false).expect("rename target computed");
        assert_eq!(
            target
                .parent()
                .and_then(|v| v.file_name())
                .and_then(|v| v.to_str()),
            Some("My Movie (2024)")
        );
        assert_eq!(target.extension().and_then(|v| v.to_str()), Some("mkv"));
        assert_eq!(
            target.file_name().and_then(|v| v.to_str()),
            Some("My Movie (2024).mkv")
        );
        assert_eq!(
            note,
            "will rename to Movie Name - Subtitle (Year) format"
        );

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn rename_target_moves_noisy_parent_folder_to_canonical_name() {
        let root = unique_temp_dir("rename-target-folder-canonical");
        fs::create_dir_all(&root).expect("create root");
        let noisy_dir = root.join("My.Movie.2024.1080p");
        fs::create_dir_all(&noisy_dir).expect("create noisy dir");
        let media_path = noisy_dir.join("My.Movie.2024.1080p.mkv");
        fs::write(&media_path, b"x").expect("write media file");

        let (target, _note) =
            compute_rename_target(&media_path, None, true).expect("rename target computed");

        let target_parent_name = target
            .parent()
            .and_then(|v| v.file_name())
            .and_then(|v| v.to_str())
            .expect("target parent name")
            .to_string();
        let target_file_stem = target
            .file_stem()
            .and_then(|v| v.to_str())
            .expect("target file stem")
            .to_string();

        assert_ne!(target_parent_name, "My.Movie.2024.1080p");
        assert_eq!(
            target_parent_name,
            target_file_stem,
            "folder should be renamed to the same canonical stem as the media file"
        );
        assert_eq!(target.extension().and_then(|v| v.to_str()), Some("mkv"));

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn rename_target_does_not_move_multi_file_parent_without_flag() {
        let root = unique_temp_dir("rename-target-multi-file-no-flag");
        fs::create_dir_all(&root).expect("create root");
        let noisy_dir = root.join("My.Movie.2024.Collection");
        fs::create_dir_all(&noisy_dir).expect("create noisy dir");
        let media_one = noisy_dir.join("My.Movie.2024.Part1.mkv");
        let media_two = noisy_dir.join("My.Movie.2024.Part2.mp4");
        fs::write(&media_one, b"x").expect("write media one");
        fs::write(&media_two, b"x").expect("write media two");

        let (target, _note) =
            compute_rename_target(&media_one, None, false).expect("rename target computed");

        assert_eq!(target.parent(), Some(noisy_dir.as_path()));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn rename_target_moves_multi_file_parent_with_flag() {
        let root = unique_temp_dir("rename-target-multi-file-with-flag");
        fs::create_dir_all(&root).expect("create root");
        let noisy_dir = root.join("My.Movie.2024.Collection");
        fs::create_dir_all(&noisy_dir).expect("create noisy dir");
        let media_one = noisy_dir.join("My.Movie.2024.Part1.mkv");
        let media_two = noisy_dir.join("My.Movie.2024.Part2.mp4");
        fs::write(&media_one, b"x").expect("write media one");
        fs::write(&media_two, b"x").expect("write media two");

        let (target, _note) =
            compute_rename_target(&media_one, None, true).expect("rename target computed");

        assert_ne!(target.parent(), Some(noisy_dir.as_path()));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn rename_target_prefers_nfo_title_and_year() {
        let root = unique_temp_dir("rename-target-nfo");
        fs::create_dir_all(&root).expect("create root");
        let media_path = root.join("some.random.1080p.mkv");
        fs::write(&media_path, b"x").expect("write media file");
        fs::write(
            root.join("some.random.1080p.nfo"),
            "<movie><title>Actual Movie Name</title><year>2022</year></movie>",
        )
        .expect("write nfo");

        let (target, _note) =
            compute_rename_target(&media_path, None, false).expect("rename target computed");
        assert_eq!(
            target.file_name().and_then(|v| v.to_str()),
            Some("Actual Movie Name (2022).mkv")
        );

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn linked_sidecar_family_renames_with_media_file() {
        let root = unique_temp_dir("rename-linked-nfo");
        fs::create_dir_all(&root).expect("create root");

        let source_media = root.join("Old Name.mkv");
        let target_media = root.join("New Name (2024).mkv");
        let source_nfo = root.join("Old Name.nfo");
        let target_nfo = root.join("New Name (2024).nfo");
        let movie_nfo = root.join("movie.nfo");
        let renamed_movie_nfo = root.join("New Name (2024)-movie.nfo");

        fs::write(&source_media, b"x").expect("write source media");
        fs::write(&source_nfo, "<movie><title>Old Name</title></movie>").expect("write source nfo");
        fs::write(&movie_nfo, "<movie><title>Folder Movie</title></movie>")
            .expect("write movie.nfo");

        rename_linked_sidecar_family(&source_media, &target_media)
            .expect("rename linked sidecar family");
        assert!(!source_nfo.exists());
        assert!(target_nfo.exists());
        assert!(!movie_nfo.exists());
        assert!(renamed_movie_nfo.exists());

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn linked_sidecar_family_renames_images_and_subtitles() {
        let root = unique_temp_dir("rename-linked-family");
        fs::create_dir_all(&root).expect("create root");

        let source_media = root.join("Old Name.mkv");
        let target_media = root.join("New Name (2024).mkv");
        let source_poster = root.join("Old Name.jpg");
        let source_subtitle = root.join("Old Name.srt");
        let folder_fanart = root.join("fanart.jpg");

        fs::write(&source_media, b"x").expect("write source media");
        fs::write(&source_poster, b"poster").expect("write poster");
        fs::write(&source_subtitle, b"subtitle").expect("write subtitle");
        fs::write(&folder_fanart, b"fanart").expect("write fanart");

        rename_linked_sidecar_family(&source_media, &target_media)
            .expect("rename linked sidecar family");

        assert!(!source_poster.exists());
        assert!(!source_subtitle.exists());
        assert!(root.join("New Name (2024).jpg").exists());
        assert!(root.join("New Name (2024).srt").exists());
        assert!(!folder_fanart.exists());
        assert!(root.join("New Name (2024)-fanart.jpg").exists());

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn linked_sidecar_family_moves_trickplay_folder_when_parent_changes() {
        let root = unique_temp_dir("rename-linked-trickplay");
        let old_parent = root.join("Old Folder");
        let new_parent = root.join("New Folder");
        fs::create_dir_all(&old_parent).expect("create old parent");

        let source_media = old_parent.join("Old Name.mkv");
        let target_media = new_parent.join("New Name (2024).mkv");
        let trickplay_dir = old_parent.join("Old Name-trickplay");
        fs::create_dir_all(&trickplay_dir).expect("create trickplay dir");

        fs::write(&source_media, b"x").expect("write source media");
        fs::write(trickplay_dir.join("0001.jpg"), b"frame").expect("write trickplay frame");

        rename_linked_sidecar_family(&source_media, &target_media)
            .expect("rename linked sidecar family");

        assert!(!trickplay_dir.exists());
        assert!(new_parent.join("New Name (2024)-trickplay").exists());

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn rename_target_formats_subtitle_with_spaced_dash() {
        let root = unique_temp_dir("rename-invalid-char");
        fs::create_dir_all(&root).expect("create root");
        let movie_dir = root.join("Movie Title (2024)");
        fs::create_dir_all(&movie_dir).expect("create movie dir");
        let media_path = movie_dir.join("Movie.Title.2024.mkv");
        fs::write(&media_path, b"x").expect("write media");

        let override_data = MetadataOverrideInput {
            title: Some("Movie: Title".to_string()),
            year: Some(2024),
            provider_id: None,
            confidence: None,
        };

        let (target, _note) = compute_rename_target(&media_path, Some(&override_data), false)
            .expect("rename target");
        assert_eq!(
            target.file_name().and_then(|v| v.to_str()),
            Some("Movie - Title (2024).mkv")
        );

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn rename_target_preserves_spaces_and_only_uses_dash_for_invalid_chars() {
        let root = unique_temp_dir("rename-space-preserve");
        fs::create_dir_all(&root).expect("create root");
        let media_path = root.join("Some.Movie.File.mkv");
        fs::write(&media_path, b"x").expect("write media");

        let override_data = MetadataOverrideInput {
            title: Some("Movie Name: Part/2".to_string()),
            year: Some(2024),
            provider_id: None,
            confidence: None,
        };

        let (target, _note) = compute_rename_target(&media_path, Some(&override_data), false)
            .expect("rename target");
        assert_eq!(
            target.file_name().and_then(|v| v.to_str()),
            Some("Movie Name - Part-2 (2024).mkv")
        );

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn rename_target_truncates_long_title_to_fit_component_limit() {
        let root = unique_temp_dir("rename-long-title");
        fs::create_dir_all(&root).expect("create root");
        let media_path = root.join("Some.Movie.File.mkv");
        fs::write(&media_path, b"x").expect("write media");

        let unique_token = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos().to_string())
            .unwrap_or_else(|_| "0".to_string());
        let long_title = format!("Unique{unique_token}{}", "A".repeat(320));
        let override_data = MetadataOverrideInput {
            title: Some(long_title),
            year: Some(2024),
            provider_id: None,
            confidence: None,
        };

        let (target, note) = compute_rename_target(&media_path, Some(&override_data), false)
            .expect("rename target");
        let file_name = target
            .file_name()
            .and_then(|v| v.to_str())
            .expect("target file name");

        assert!(file_name.ends_with(".mkv"));
        assert!(file_name.as_bytes().len() <= super::MAX_PATH_COMPONENT_BYTES);
        assert!(note.contains("truncated to fit filesystem limits"));

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[cfg(unix)]
    #[test]
    fn linked_sidecar_family_rejects_symlink_entries() {
        use std::os::unix::fs::symlink;

        let root = unique_temp_dir("rename-linked-symlink");
        fs::create_dir_all(&root).expect("create root");

        let source_media = root.join("Old Name.mkv");
        let target_media = root.join("New Name (2024).mkv");
        let external = root.join("external.nfo");
        let symlink_path = root.join("Old Name.nfo");

        fs::write(&source_media, b"x").expect("write source media");
        fs::write(&external, b"external").expect("write external file");
        symlink(&external, &symlink_path).expect("create symlink");

        let err = rename_linked_sidecar_family(&source_media, &target_media)
            .expect_err("symlink entries should fail");
        assert!(err.contains("symlink"));
        assert!(symlink_path.exists());

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn rename_target_reports_explicit_collision_when_truncated_target_exists() {
        let root = unique_temp_dir("rename-truncation-collision");
        fs::create_dir_all(&root).expect("create root");
        let media_path = root.join("Some.Movie.File.mkv");
        fs::write(&media_path, b"x").expect("write media");

        let unique_token = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos().to_string())
            .unwrap_or_else(|_| "0".to_string());
        let long_title = format!("Unique{unique_token}{}", "A".repeat(320));
        let override_data = MetadataOverrideInput {
            title: Some(long_title),
            year: Some(2024),
            provider_id: None,
            confidence: None,
        };

        let (first_target, _) = compute_rename_target(&media_path, Some(&override_data), false)
            .expect("first rename target");
        if let Some(parent) = first_target.parent() {
            fs::create_dir_all(parent).expect("create first target parent");
        }
        fs::write(&first_target, b"occupied").expect("occupy first target");

        let err = compute_rename_target(&media_path, Some(&override_data), false)
            .expect_err("truncated collision should return explicit conflict");
        assert!(err.contains("rename collision after truncation"));

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn bulk_apply_rename_fails_when_source_disappears_after_preview() {
        let root = unique_temp_dir("bulk-apply-race-source");
        fs::create_dir_all(&root).expect("create root");
        let state_dir = root.join("state");
        let state = test_app_state(&root, &state_dir);

        let media_path = root.join("Old Name.mkv");
        fs::write(&media_path, b"x").expect("write media");

        let item = BulkItemInput {
            media_path: media_path.display().to_string(),
            item_uid: None,
            metadata_override: Some(MetadataOverrideInput {
                title: Some("New Name".to_string()),
                year: Some(2024),
                provider_id: None,
                confidence: None,
            }),
            rename_parent_folder: None,
        };
        let apply_item = BulkItemInput {
            media_path: media_path.display().to_string(),
            item_uid: None,
            metadata_override: Some(MetadataOverrideInput {
                title: Some("New Name".to_string()),
                year: Some(2024),
                provider_id: None,
                confidence: None,
            }),
            rename_parent_folder: None,
        };

        let dry_run_request = BulkDryRunRequest {
            action: "rename".to_string(),
            items: vec![item],
        };
        let dry_run = execute_bulk_dry_run(&state, &dry_run_request).expect("dry run rename");

        fs::remove_file(&media_path).expect("delete source media");

        let apply_request = BulkApplyRequest {
            action: "rename".to_string(),
            approved_batch_hash: dry_run.batch_hash,
            items: vec![apply_item],
        };

        let err = execute_bulk_apply(&state, &apply_request)
            .expect_err("apply should reject stale dry-run hash");
        assert_eq!(err.0, StatusCode::CONFLICT);
        assert!(err.1.contains("approved_batch_hash"));

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn bulk_apply_rename_fails_when_target_appears_after_preview() {
        let root = unique_temp_dir("bulk-apply-race-target");
        fs::create_dir_all(&root).expect("create root");
        let state_dir = root.join("state");
        let state = test_app_state(&root, &state_dir);

        let media_path = root.join("Old Name.mkv");
        fs::write(&media_path, b"x").expect("write media");

        let item = BulkItemInput {
            media_path: media_path.display().to_string(),
            item_uid: None,
            metadata_override: Some(MetadataOverrideInput {
                title: Some("New Name".to_string()),
                year: Some(2024),
                provider_id: None,
                confidence: None,
            }),
            rename_parent_folder: None,
        };
        let apply_item = BulkItemInput {
            media_path: media_path.display().to_string(),
            item_uid: None,
            metadata_override: Some(MetadataOverrideInput {
                title: Some("New Name".to_string()),
                year: Some(2024),
                provider_id: None,
                confidence: None,
            }),
            rename_parent_folder: None,
        };

        let dry_run_request = BulkDryRunRequest {
            action: "rename".to_string(),
            items: vec![item],
        };
        let dry_run = execute_bulk_dry_run(&state, &dry_run_request).expect("dry run rename");
        let target_path = PathBuf::from(
            dry_run
                .items
                .first()
                .and_then(|v| v.proposed_media_path.as_ref())
                .expect("dry run target path"),
        );
        fs::write(&target_path, b"conflict").expect("create target conflict");

        let apply_request = BulkApplyRequest {
            action: "rename".to_string(),
            approved_batch_hash: dry_run.batch_hash,
            items: vec![apply_item],
        };

        let err = execute_bulk_apply(&state, &apply_request)
            .expect_err("apply should reject stale dry-run hash");
        assert_eq!(err.0, StatusCode::CONFLICT);
        assert!(err.1.contains("approved_batch_hash"));

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[cfg(unix)]
    #[test]
    fn bulk_apply_rolls_back_media_when_sidecar_update_fails() {
        use std::os::unix::fs::symlink;

        let root = unique_temp_dir("bulk-apply-sidecar-rollback");
        fs::create_dir_all(&root).expect("create root");
        let state_dir = root.join("state");
        let state = test_app_state(&root, &state_dir);

        let source_media = root.join("Old Name.mkv");
        let source_sidecar = root.join("Old Name.nfo");
        let external = root.join("external.nfo");
        fs::write(&source_media, b"x").expect("write media");
        fs::write(&external, b"external").expect("write external");
        symlink(&external, &source_sidecar).expect("create symlinked sidecar");

        let item = BulkItemInput {
            media_path: source_media.display().to_string(),
            item_uid: None,
            metadata_override: Some(MetadataOverrideInput {
                title: Some("New Name".to_string()),
                year: Some(2024),
                provider_id: None,
                confidence: None,
            }),
            rename_parent_folder: None,
        };
        let apply_item = BulkItemInput {
            media_path: source_media.display().to_string(),
            item_uid: None,
            metadata_override: Some(MetadataOverrideInput {
                title: Some("New Name".to_string()),
                year: Some(2024),
                provider_id: None,
                confidence: None,
            }),
            rename_parent_folder: None,
        };

        let dry_run_request = BulkDryRunRequest {
            action: "rename".to_string(),
            items: vec![item],
        };
        let dry_run = execute_bulk_dry_run(&state, &dry_run_request).expect("dry run rename");

        let apply_request = BulkApplyRequest {
            action: "rename".to_string(),
            approved_batch_hash: dry_run.batch_hash,
            items: vec![apply_item],
        };
        let apply = execute_bulk_apply(&state, &apply_request).expect("apply response");
        let target_media = root.join("New Name (2024).mkv");

        assert_eq!(apply.failed, 1);
        let result = apply.items.first().expect("single apply result");
        assert!(!result.success);
        assert!(result
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("rename sidecar update failed"));
        assert!(source_media.exists());
        assert!(!target_media.exists());

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn duplicate_grouping_uses_normalized_key() {
        let items = vec![
            BulkItemInput {
                media_path: "/tmp/Movie.Name.2024.mkv".to_string(),
                item_uid: None,
                metadata_override: None,
                rename_parent_folder: None,
            },
            BulkItemInput {
                media_path: "/tmp/movie name 2024.mp4".to_string(),
                item_uid: None,
                metadata_override: None,
                rename_parent_folder: None,
            },
            BulkItemInput {
                media_path: "/tmp/Other.Movie.mkv".to_string(),
                item_uid: None,
                metadata_override: None,
                rename_parent_folder: None,
            },
        ];

        let groups = build_duplicate_groups(&items);
        assert_eq!(
            normalize_duplicate_key("Movie.Name.2024"),
            "movie name|2024"
        );
        assert_eq!(groups.get("movie name|2024").map(Vec::len), Some(2));
        assert_eq!(groups.get("other movie|none").map(Vec::len), Some(1));
    }

    #[test]
    fn duplicate_key_removes_noise_tokens() {
        assert_eq!(
            normalize_duplicate_key("The.Movie.2021.1080p.BluRay.x264"),
            "the movie|2021"
        );
    }

    #[test]
    fn duplicate_key_uses_folder_movie_signature_when_present() {
        let root = unique_temp_dir("duplicate-folder-signature");
        fs::create_dir_all(&root).expect("create root");
        let folder = root.join("Sample Movie (2021) - [imdb-tt7654321]");
        fs::create_dir_all(&folder).expect("create folder");
        let media_path = folder.join("sample-file-v1.mkv");
        fs::write(&media_path, vec![0_u8; 1_500_000]).expect("write media");

        let key = duplicate_key_for_media_path(&media_path);
        assert_eq!(key, "sample movie|2021|imdb-tt7654321");

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn metadata_inference_prefers_folder_pattern_when_available() {
        let media = PathBuf::from(
            "/media/movies/Ghostbusters - Frozen Empire (2024) - [tmdbid-967847]/Ghostbusters.mkv",
        );
        let candidate = infer_metadata_candidate(&media, "ghostbusters-uid", None);
        assert_eq!(candidate.title, "Ghostbusters Frozen Empire");
        assert_eq!(candidate.year, Some(2024));
        assert_eq!(candidate.provider_id, "tmdb-967847");
        assert!(candidate.confidence >= 0.9);
    }

    #[test]
    fn metadata_inference_parses_imdb_folder_pattern() {
        let media = PathBuf::from(
            "/media/movies/Zootopia 2 (2025) - [imdb-tt26443597]/Zootopia 2 (2025).mkv",
        );
        let candidate = infer_metadata_candidate(&media, "zootopia-uid", None);
        assert_eq!(candidate.title, "Zootopia 2");
        assert_eq!(candidate.year, Some(2025));
        assert_eq!(candidate.provider_id, "imdb-tt26443597");
    }

    #[test]
    fn metadata_inference_prefers_nfo_when_present() {
        let root = unique_temp_dir("nfo-metadata-priority");
        fs::create_dir_all(&root).expect("create root");
        let folder = root.join("Wrong Movie (2010) - [imdb-tt0000001]");
        fs::create_dir_all(&folder).expect("create folder");
        let media = folder.join("Wrong.Movie.2010.mkv");
        fs::write(&media, b"x").expect("write media");
        fs::write(
            folder.join("Wrong.Movie.2010.nfo"),
            "<movie><title>Correct Movie</title><year>2024</year><uniqueid type=\"imdb\">tt1234567</uniqueid></movie>",
        )
        .expect("write nfo");

        let candidate = infer_metadata_candidate(&media, "wrong-movie-uid", None);
        assert_eq!(candidate.title, "Correct Movie");
        assert_eq!(candidate.year, Some(2024));
        assert_eq!(candidate.provider_id, "imdb-tt1234567");
        assert!(candidate.confidence >= 0.99);

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn duplicate_key_uses_nfo_metadata_when_present() {
        let root = unique_temp_dir("nfo-duplicate-key");
        fs::create_dir_all(&root).expect("create root");
        let folder = root.join("Ambiguous Folder (2000)");
        fs::create_dir_all(&folder).expect("create folder");
        let media = folder.join("random-name.mkv");
        fs::write(&media, b"x").expect("write media");
        fs::write(
            folder.join("random-name.nfo"),
            "<movie><title>Exact Key Movie</title><year>2023</year><tmdbid>778899</tmdbid></movie>",
        )
        .expect("write nfo");

        let key = duplicate_key_for_media_path(&media);
        assert_eq!(key, "exact key movie|2023|tmdb-778899");

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn episode_signature_is_extracted_from_media_path() {
        let path = "/media/tv/MythBusters/Season 2006/MythBusters - S2006E01 - Paper Crossbow.mkv";
        assert_eq!(
            extract_episode_signature_from_media_path(path),
            Some("S2006E001".to_string())
        );
    }

    #[test]
    fn semantic_partition_splits_tv_season_into_episode_buckets() {
        let items = vec![
            SemanticDuplicateItem {
                media_path: "/media/tv/Show/Season 1/Show - S01E01 - A.mkv".to_string(),
                file_size: 10,
                content_hash: Some("hash-a".to_string()),
                video_codec: None,
                audio_codec: None,
                width: None,
                height: None,
            },
            SemanticDuplicateItem {
                media_path: "/media/tv/Show/Season 1/Show - S01E02 - B.mkv".to_string(),
                file_size: 11,
                content_hash: Some("hash-b".to_string()),
                video_codec: None,
                audio_codec: None,
                width: None,
                height: None,
            },
        ];

        let buckets = partition_semantic_group_items(items, 2);
        assert_eq!(buckets.len(), 0);
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
