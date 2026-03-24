use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::domain::sidecar::SidecarState;

pub const SIDECAR_FILENAME: &str = ".mm.json";

#[derive(Debug, Error)]
pub enum SidecarStoreError {
    #[error("invalid media path: {0}")]
    InvalidPath(String),
    #[error("failed to read sidecar: {0}")]
    ReadFailed(String),
    #[error("failed to deserialize sidecar: {0}")]
    DecodeFailed(String),
    #[error("failed to serialize sidecar: {0}")]
    EncodeFailed(String),
    #[error("failed to write sidecar: {0}")]
    WriteFailed(String),
}

pub fn sidecar_path_for_media(media_path: &Path) -> Result<PathBuf, SidecarStoreError> {
    if media_path.as_os_str().is_empty() {
        return Err(SidecarStoreError::InvalidPath("empty path".to_string()));
    }

    if media_path.is_dir() {
        return Ok(media_path.join(SIDECAR_FILENAME));
    }

    let parent = media_path.parent().ok_or_else(|| {
        SidecarStoreError::InvalidPath(format!("cannot resolve parent for {}", media_path.display()))
    })?;

    Ok(parent.join(SIDECAR_FILENAME))
}

pub fn read_sidecar(media_path: &Path) -> Result<Option<SidecarState>, SidecarStoreError> {
    let path = sidecar_path_for_media(media_path)?;
    read_sidecar_at_path(&path)
}

pub fn read_sidecar_at_path(path: &Path) -> Result<Option<SidecarState>, SidecarStoreError> {
    if !path.exists() {
        return Ok(None);
    }

    let contents = fs::read_to_string(&path)
        .map_err(|e| SidecarStoreError::ReadFailed(format!("{} ({})", path.display(), e)))?;
    let state = serde_json::from_str::<SidecarState>(&contents)
        .map_err(|e| SidecarStoreError::DecodeFailed(format!("{} ({})", path.display(), e)))?;

    Ok(Some(state))
}

pub fn write_sidecar(media_path: &Path, state: &SidecarState) -> Result<PathBuf, SidecarStoreError> {
    let path = sidecar_path_for_media(media_path)?;
    write_sidecar_at_path(&path, state)?;
    Ok(path)
}

pub fn write_sidecar_at_path(path: &Path, state: &SidecarState) -> Result<(), SidecarStoreError> {
    let parent = path.parent().ok_or_else(|| {
        SidecarStoreError::InvalidPath(format!("cannot resolve parent for {}", path.display()))
    })?;

    fs::create_dir_all(parent)
        .map_err(|e| SidecarStoreError::WriteFailed(format!("create_dir_all {} ({})", parent.display(), e)))?;

    let serialized = serde_json::to_string_pretty(state)
        .map_err(|e| SidecarStoreError::EncodeFailed(e.to_string()))?;

    let temp_path = path.with_extension(format!("json.tmp.{}", std::process::id()));
    fs::write(&temp_path, serialized)
        .map_err(|e| SidecarStoreError::WriteFailed(format!("write {} ({})", temp_path.display(), e)))?;

    fs::rename(&temp_path, &path)
        .map_err(|e| SidecarStoreError::WriteFailed(format!("rename {} -> {} ({})", temp_path.display(), path.display(), e)))?;

    Ok(())
}

pub fn delete_sidecar_at_path(path: &Path) -> Result<(), SidecarStoreError> {
    if !path.exists() {
        return Ok(());
    }

    fs::remove_file(path)
        .map_err(|e| SidecarStoreError::WriteFailed(format!("remove {} ({})", path.display(), e)))
}
