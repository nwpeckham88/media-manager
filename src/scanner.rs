use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Serialize;
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize)]
pub struct ScanSummary {
    pub roots: Vec<RootScanSummary>,
    pub total_media_files: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RootScanSummary {
    pub root: String,
    pub exists: bool,
    pub media_files: u64,
    pub error: Option<String>,
}

pub fn scan_library_roots(roots: &[PathBuf]) -> ScanSummary {
    let mut entries = Vec::with_capacity(roots.len());
    let mut total = 0_u64;

    for root in roots {
        let summary = scan_root(root);
        total += summary.media_files;
        entries.push(summary);
    }

    ScanSummary {
        roots: entries,
        total_media_files: total,
    }
}

fn scan_root(root: &Path) -> RootScanSummary {
    let root_display = root.display().to_string();
    if !root.exists() {
        return RootScanSummary {
            root: root_display,
            exists: false,
            media_files: 0,
            error: Some("path does not exist".to_string()),
        };
    }

    let allowed = media_extensions();
    let mut media_files = 0_u64;

    for item in WalkDir::new(root) {
        let Ok(entry) = item else {
            continue;
        };

        if !entry.file_type().is_file() {
            continue;
        }

        let extension = entry
            .path()
            .extension()
            .and_then(|v| v.to_str())
            .map(|v| v.to_ascii_lowercase());

        if let Some(ext) = extension {
            if allowed.contains(ext.as_str()) {
                media_files += 1;
            }
        }
    }

    RootScanSummary {
        root: root_display,
        exists: true,
        media_files,
        error: None,
    }
}

fn media_extensions() -> HashSet<&'static str> {
    [
        "mkv", "mp4", "avi", "mov", "m4v", "wmv", "flv", "webm", "ts", "m2ts", "mpg",
        "mpeg",
    ]
    .into_iter()
    .collect()
}
