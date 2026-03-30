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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContainerFormat {
    Mkv,
    Mp4,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VideoCodecPreference {
    Av1,
    Hevc,
    H264,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AudioLayout {
    Stereo,
    Surround51,
    Surround71,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DesiredVideoPolicy {
    pub preferred_codec: VideoCodecPreference,
    pub minimum_allowed_codec: VideoCodecPreference,
    pub allow_manual_codec_upgrade: bool,
}

impl Default for DesiredVideoPolicy {
    fn default() -> Self {
        Self {
            preferred_codec: VideoCodecPreference::Av1,
            minimum_allowed_codec: VideoCodecPreference::H264,
            allow_manual_codec_upgrade: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DesiredAudioPolicy {
    pub allowed_layouts: Vec<AudioLayout>,
    pub require_stereo_track: bool,
    pub add_night_mode_stereo_track: bool,
    pub transcode_stereo_to_opus: bool,
    pub transcode_standard_surround_to_opus: bool,
    pub preserve_object_audio_tracks: bool,
    pub night_mode_target_lufs: f32,
}

impl Default for DesiredAudioPolicy {
    fn default() -> Self {
        Self {
            allowed_layouts: vec![
                AudioLayout::Stereo,
                AudioLayout::Surround51,
                AudioLayout::Surround71,
            ],
            require_stereo_track: true,
            add_night_mode_stereo_track: false,
            transcode_stereo_to_opus: true,
            transcode_standard_surround_to_opus: true,
            preserve_object_audio_tracks: true,
            night_mode_target_lufs: -16.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DesiredSubtitlePolicy {
    pub keep_existing_subtitles: bool,
    pub require_text_subtitle: bool,
}

impl Default for DesiredSubtitlePolicy {
    fn default() -> Self {
        Self {
            keep_existing_subtitles: true,
            require_text_subtitle: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DesiredTranscodePolicy {
    pub require_manual_approval: bool,
    pub allow_automatic_transcode: bool,
}

impl Default for DesiredTranscodePolicy {
    fn default() -> Self {
        Self {
            require_manual_approval: true,
            allow_automatic_transcode: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DesiredMediaState {
    pub container: ContainerFormat,
    pub video: DesiredVideoPolicy,
    pub audio: DesiredAudioPolicy,
    pub subtitles: DesiredSubtitlePolicy,
    pub transcode: DesiredTranscodePolicy,
}

impl Default for DesiredMediaState {
    fn default() -> Self {
        Self {
            container: ContainerFormat::Mkv,
            video: DesiredVideoPolicy::default(),
            audio: DesiredAudioPolicy::default(),
            subtitles: DesiredSubtitlePolicy::default(),
            transcode: DesiredTranscodePolicy::default(),
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
