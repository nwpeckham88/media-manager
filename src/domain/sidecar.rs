use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const CURRENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SidecarState {
    pub schema_version: u32,
    pub item_uid: String,
    pub source_fingerprints: Vec<Fingerprint>,
    pub provider_ids: ProviderIds,
    pub nfo_state: NfoState,
    pub preferred_policy_state: Value,
    pub applied_state: Value,
    pub derived_artifacts: Vec<DerivedArtifact>,
    pub last_reconcile_result: ReconcileResult,
    pub last_operation_id: Option<String>,
    pub protected_flags: ProtectedFlags,
}

impl SidecarState {
    pub fn new(item_uid: impl Into<String>) -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            item_uid: item_uid.into(),
            source_fingerprints: Vec::new(),
            provider_ids: ProviderIds::default(),
            nfo_state: NfoState::Unknown,
            preferred_policy_state: Value::Null,
            applied_state: Value::Null,
            derived_artifacts: Vec::new(),
            last_reconcile_result: ReconcileResult::Unknown,
            last_operation_id: None,
            protected_flags: ProtectedFlags::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fingerprint {
    pub kind: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderIds {
    pub tmdb: Option<String>,
    pub imdb: Option<String>,
    pub tvdb: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NfoState {
    Unknown,
    Missing,
    Valid,
    Invalid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DerivedArtifact {
    pub id: String,
    pub artifact_type: String,
    pub path: String,
    pub profile_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReconcileResult {
    Unknown,
    Clean,
    DriftDetected,
    NeedsManualReview,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProtectedFlags {
    pub path_protected: bool,
    pub item_protected: bool,
    pub tag_protected: bool,
}
