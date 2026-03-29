use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::domain::sidecar::SidecarState;
use crate::sidecar_store;

static OPERATION_NONCE: AtomicU64 = AtomicU64::new(0);
static SNAPSHOT_TEMP_NONCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SidecarPlanAction {
    Create,
    Update,
    Noop,
}

#[derive(Debug, Clone, Serialize)]
pub struct SidecarPlan {
    pub plan_hash: String,
    pub media_path: String,
    pub sidecar_path: String,
    pub action: SidecarPlanAction,
    pub existing_state: Option<SidecarState>,
    pub next_state: SidecarState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackSnapshot {
    pub operation_id: String,
    pub created_at_ms: u128,
    pub sidecar_path: String,
    pub previous_state: Option<SidecarState>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SidecarApplyResult {
    pub operation_id: String,
    pub sidecar_path: String,
    pub applied_state: SidecarState,
}

#[derive(Debug, Clone, Serialize)]
pub struct SidecarRollbackResult {
    pub operation_id: String,
    pub sidecar_path: String,
    pub restored: bool,
}

#[derive(Debug, Error)]
pub enum SidecarWorkflowError {
    #[error("sidecar store error: {0}")]
    Store(String),
    #[error("plan mismatch: approved hash does not match current plan")]
    PlanMismatch,
    #[error("rollback snapshot not found for operation: {0}")]
    RollbackSnapshotMissing(String),
    #[error("rollback snapshot decode failed: {0}")]
    RollbackSnapshotDecode(String),
    #[error("rollback snapshot encode failed: {0}")]
    RollbackSnapshotEncode(String),
    #[error("rollback snapshot write failed: {0}")]
    RollbackSnapshotWrite(String),
    #[error("rollback snapshot delete failed: {0}")]
    RollbackSnapshotDelete(String),
}

pub fn build_plan(media_path: &Path, item_uid: &str) -> Result<SidecarPlan, SidecarWorkflowError> {
    let sidecar_path = sidecar_store::sidecar_path_for_media(media_path)
        .map_err(|e| SidecarWorkflowError::Store(e.to_string()))?;

    let existing_state = sidecar_store::read_sidecar_at_path(&sidecar_path)
        .map_err(|e| SidecarWorkflowError::Store(e.to_string()))?;

    let mut next_state = existing_state
        .clone()
        .unwrap_or_else(|| SidecarState::new(item_uid.to_string()));
    next_state.item_uid = item_uid.to_string();

    let action = match &existing_state {
        None => SidecarPlanAction::Create,
        Some(current) if current.item_uid != item_uid => SidecarPlanAction::Update,
        Some(_) => SidecarPlanAction::Noop,
    };

    let plan_hash = hash_plan(&sidecar_path, &existing_state, &next_state);

    Ok(SidecarPlan {
        plan_hash,
        media_path: media_path.display().to_string(),
        sidecar_path: sidecar_path.display().to_string(),
        action,
        existing_state,
        next_state,
    })
}

pub fn apply_plan(
    media_path: &Path,
    item_uid: &str,
    approved_plan_hash: &str,
    state_dir: &Path,
) -> Result<SidecarApplyResult, SidecarWorkflowError> {
    let plan = build_plan(media_path, item_uid)?;
    if plan.plan_hash != approved_plan_hash {
        return Err(SidecarWorkflowError::PlanMismatch);
    }

    let operation_id = generate_operation_id();
    let snapshot = RollbackSnapshot {
        operation_id: operation_id.clone(),
        created_at_ms: now_ms(),
        sidecar_path: plan.sidecar_path.clone(),
        previous_state: plan.existing_state.clone(),
    };

    write_rollback_snapshot(state_dir, &snapshot)?;

    let sidecar_path = PathBuf::from(&plan.sidecar_path);
    sidecar_store::write_sidecar_at_path(&sidecar_path, &plan.next_state)
        .map_err(|e| SidecarWorkflowError::Store(e.to_string()))?;

    Ok(SidecarApplyResult {
        operation_id,
        sidecar_path: plan.sidecar_path,
        applied_state: plan.next_state,
    })
}

pub fn rollback_operation(
    operation_id: &str,
    state_dir: &Path,
) -> Result<SidecarRollbackResult, SidecarWorkflowError> {
    let snapshot = read_rollback_snapshot(state_dir, operation_id)?;
    let sidecar_path = PathBuf::from(&snapshot.sidecar_path);

    if let Some(previous_state) = snapshot.previous_state {
        sidecar_store::write_sidecar_at_path(&sidecar_path, &previous_state)
            .map_err(|e| SidecarWorkflowError::Store(e.to_string()))?;
    } else {
        sidecar_store::delete_sidecar_at_path(&sidecar_path)
            .map_err(|e| SidecarWorkflowError::Store(e.to_string()))?;
    }

    delete_rollback_snapshot(state_dir, operation_id)?;

    Ok(SidecarRollbackResult {
        operation_id: operation_id.to_string(),
        sidecar_path: sidecar_path.display().to_string(),
        restored: true,
    })
}

fn rollback_snapshot_path(state_dir: &Path, operation_id: &str) -> PathBuf {
    state_dir
        .join("rollback")
        .join(format!("{operation_id}.json"))
}

fn write_rollback_snapshot(
    state_dir: &Path,
    snapshot: &RollbackSnapshot,
) -> Result<(), SidecarWorkflowError> {
    let path = rollback_snapshot_path(state_dir, &snapshot.operation_id);
    let parent = path.parent().ok_or_else(|| {
        SidecarWorkflowError::RollbackSnapshotWrite("invalid rollback path".to_string())
    })?;

    fs::create_dir_all(parent).map_err(|e| {
        SidecarWorkflowError::RollbackSnapshotWrite(format!("create {} ({})", parent.display(), e))
    })?;

    let content = serde_json::to_string_pretty(snapshot)
        .map_err(|e| SidecarWorkflowError::RollbackSnapshotEncode(e.to_string()))?;

    let temp_path = unique_snapshot_temp_path(&path);
    fs::write(&temp_path, content).map_err(|e| {
        SidecarWorkflowError::RollbackSnapshotWrite(format!(
            "write {} ({})",
            temp_path.display(),
            e
        ))
    })?;

    fs::rename(&temp_path, &path).map_err(|e| {
        SidecarWorkflowError::RollbackSnapshotWrite(format!(
            "rename {} -> {} ({})",
            temp_path.display(),
            path.display(),
            e
        ))
    })?;

    Ok(())
}

fn read_rollback_snapshot(
    state_dir: &Path,
    operation_id: &str,
) -> Result<RollbackSnapshot, SidecarWorkflowError> {
    let path = rollback_snapshot_path(state_dir, operation_id);
    if !path.exists() {
        return Err(SidecarWorkflowError::RollbackSnapshotMissing(
            operation_id.to_string(),
        ));
    }

    let content = fs::read_to_string(&path).map_err(|e| {
        SidecarWorkflowError::RollbackSnapshotDecode(format!("read {} ({})", path.display(), e))
    })?;

    serde_json::from_str::<RollbackSnapshot>(&content).map_err(|e| {
        SidecarWorkflowError::RollbackSnapshotDecode(format!("decode {} ({})", path.display(), e))
    })
}

fn delete_rollback_snapshot(
    state_dir: &Path,
    operation_id: &str,
) -> Result<(), SidecarWorkflowError> {
    let path = rollback_snapshot_path(state_dir, operation_id);
    if !path.exists() {
        return Ok(());
    }

    fs::remove_file(&path).map_err(|e| {
        SidecarWorkflowError::RollbackSnapshotDelete(format!("remove {} ({})", path.display(), e))
    })
}

fn hash_plan(
    sidecar_path: &Path,
    existing_state: &Option<SidecarState>,
    next_state: &SidecarState,
) -> String {
    let existing_json =
        serde_json::to_string(existing_state).unwrap_or_else(|_| "null".to_string());
    let next_json = serde_json::to_string(next_state).unwrap_or_else(|_| "null".to_string());

    let mut hasher = DefaultHasher::new();
    sidecar_path.display().to_string().hash(&mut hasher);
    existing_json.hash(&mut hasher);
    next_json.hash(&mut hasher);

    format!("{:016x}", hasher.finish())
}

fn generate_operation_id() -> String {
    let ts = now_ms();
    let nonce = OPERATION_NONCE.fetch_add(1, Ordering::Relaxed);
    format!("op-{ts}-{nonce}-{}", std::process::id())
}

fn unique_snapshot_temp_path(path: &Path) -> PathBuf {
    let nonce = SNAPSHOT_TEMP_NONCE.fetch_add(1, Ordering::Relaxed);
    path.with_extension(format!("tmp.{}.{}.{}", std::process::id(), now_ms(), nonce))
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::generate_operation_id;

    #[test]
    fn operation_ids_are_unique_for_many_generations() {
        let mut ids = HashSet::new();
        for _ in 0..2_000 {
            let id = generate_operation_id();
            assert!(ids.insert(id));
        }
    }
}
