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
    let mut walk_errors = 0_u64;
    let mut first_walk_error: Option<String> = None;

    for item in WalkDir::new(root) {
        let entry = match item {
            Ok(entry) => entry,
            Err(err) => {
                walk_errors += 1;
                if first_walk_error.is_none() {
                    first_walk_error = Some(err.to_string());
                }
                continue;
            }
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
        error: if walk_errors == 0 {
            None
        } else {
            Some(format!(
                "encountered {walk_errors} traversal error(s); first: {}",
                first_walk_error.unwrap_or_else(|| "unknown error".to_string())
            ))
        },
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::scan_library_roots;

    fn unique_temp_dir(name: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        dir.push(format!("mm-scanner-{name}-{nanos}"));
        dir
    }

    #[test]
    fn counts_known_media_extensions() {
        let root = unique_temp_dir("count-media");
        fs::create_dir_all(&root).expect("create root");
        fs::write(root.join("movie.mkv"), b"x").expect("write mkv");
        fs::write(root.join("note.txt"), b"x").expect("write txt");

        let summary = scan_library_roots(std::slice::from_ref(&root));
        assert_eq!(summary.total_media_files, 1);
        assert!(summary.roots[0].error.is_none());

        fs::remove_dir_all(root).expect("cleanup root");
    }

    #[cfg(unix)]
    #[test]
    fn reports_walk_errors_for_unreadable_subdirs() {
        use std::os::unix::fs::PermissionsExt;

        let root = unique_temp_dir("walk-errors");
        let blocked = root.join("blocked");
        fs::create_dir_all(&blocked).expect("create blocked dir");
        fs::write(blocked.join("secret.mkv"), b"x").expect("write file in blocked dir");

        let original_mode = fs::metadata(&blocked)
            .expect("stat blocked dir")
            .permissions()
            .mode();

        let mut no_access = fs::metadata(&blocked)
            .expect("stat blocked dir for chmod")
            .permissions();
        no_access.set_mode(0o000);
        fs::set_permissions(&blocked, no_access).expect("chmod blocked dir");

        let summary = scan_library_roots(std::slice::from_ref(&root));

        let mut restore = fs::metadata(&blocked)
            .expect("stat blocked dir for restore")
            .permissions();
        restore.set_mode(original_mode);
        fs::set_permissions(&blocked, restore).expect("restore blocked dir permissions");
        fs::remove_dir_all(root).expect("cleanup root");

        assert_eq!(summary.roots.len(), 1);
        assert!(summary.roots[0].error.is_some());
    }
}
