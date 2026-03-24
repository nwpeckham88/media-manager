use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;
use thiserror::Error;

use crate::config::ToolchainConfig;

#[derive(Debug, Clone, Serialize)]
pub struct ToolchainSnapshot {
    pub ffmpeg: ResolvedBinary,
    pub ffprobe: ResolvedBinary,
    pub mediainfo: Option<ResolvedBinary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolvedBinary {
    pub command_name: String,
    pub path: String,
    pub version_output: Option<String>,
    pub status: ProbeStatus,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeStatus {
    Ok,
    Unavailable,
}

#[derive(Debug, Error)]
pub enum ToolchainError {
    #[error("required binary not found: {0}")]
    RequiredBinaryMissing(&'static str),
}

pub fn probe_toolchain(config: &ToolchainConfig) -> Result<ToolchainSnapshot, ToolchainError> {
    let ffmpeg = resolve_required_binary("ffmpeg", &config.ffmpeg_path, &["jellyfin-ffmpeg", "ffmpeg"])?;
    let ffprobe = resolve_required_binary("ffprobe", &config.ffprobe_path, &["jellyfin-ffprobe", "ffprobe"])?;
    let mediainfo = resolve_optional_binary("mediainfo", &config.mediainfo_path, &["mediainfo"]);

    Ok(ToolchainSnapshot {
        ffmpeg,
        ffprobe,
        mediainfo,
    })
}

fn resolve_required_binary(
    command_name: &'static str,
    explicit_path: &Option<PathBuf>,
    candidates: &[&str],
) -> Result<ResolvedBinary, ToolchainError> {
    resolve_binary(command_name, explicit_path, candidates)
        .ok_or(ToolchainError::RequiredBinaryMissing(command_name))
}

fn resolve_optional_binary(
    command_name: &'static str,
    explicit_path: &Option<PathBuf>,
    candidates: &[&str],
) -> Option<ResolvedBinary> {
    resolve_binary(command_name, explicit_path, candidates)
}

fn resolve_binary(
    command_name: &'static str,
    explicit_path: &Option<PathBuf>,
    candidates: &[&str],
) -> Option<ResolvedBinary> {
    if let Some(path) = explicit_path {
        return Some(probe_path(command_name, path));
    }

    for candidate in candidates {
        if let Some(path) = which(candidate) {
            return Some(probe_path(command_name, &path));
        }
    }

    None
}

fn probe_path(command_name: &'static str, path: &Path) -> ResolvedBinary {
    match Command::new(path).arg("-version").output() {
        Ok(output) => {
            let first_line = String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .map(ToOwned::to_owned);
            ResolvedBinary {
                command_name: command_name.to_string(),
                path: path.display().to_string(),
                version_output: first_line,
                status: if output.status.success() {
                    ProbeStatus::Ok
                } else {
                    ProbeStatus::Unavailable
                },
            }
        }
        Err(_) => ResolvedBinary {
            command_name: command_name.to_string(),
            path: path.display().to_string(),
            version_output: None,
            status: ProbeStatus::Unavailable,
        },
    }
}

fn which(binary: &str) -> Option<PathBuf> {
    let output = Command::new("which").arg(binary).output().ok()?;
    if !output.status.success() {
        return None;
    }

    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        return None;
    }

    Some(PathBuf::from(path))
}
