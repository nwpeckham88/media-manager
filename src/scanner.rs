use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Serialize;
use walkdir::WalkDir;

const DEFAULT_LIBRARY_PAGE_LIMIT: usize = 120;
const MAX_LIBRARY_PAGE_LIMIT: usize = 500;

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

#[derive(Debug, Clone)]
pub struct LibraryBrowseOptions {
    pub root_index: Option<usize>,
    pub query: Option<String>,
    pub offset: usize,
    pub limit: usize,
}

impl Default for LibraryBrowseOptions {
    fn default() -> Self {
        Self {
            root_index: None,
            query: None,
            offset: 0,
            limit: DEFAULT_LIBRARY_PAGE_LIMIT,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LibraryBrowseResult {
    pub total_matches: usize,
    pub offset: usize,
    pub limit: usize,
    pub items: Vec<LibraryMediaItem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LibraryMediaItem {
    pub media_path: String,
    pub root: String,
    pub relative_path: String,
    pub file_name: String,
    pub extension: String,
    pub sidecar_path: String,
    pub sidecar_exists: bool,
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

pub fn list_library_media(
    roots: &[PathBuf],
    options: LibraryBrowseOptions,
) -> Result<LibraryBrowseResult, String> {
    if roots.is_empty() {
        return Ok(LibraryBrowseResult {
            total_matches: 0,
            offset: options.offset,
            limit: normalize_library_limit(options.limit),
            items: Vec::new(),
        });
    }

    let normalized_limit = normalize_library_limit(options.limit);

    let roots_to_scan: Vec<(usize, PathBuf)> = if let Some(idx) = options.root_index {
        let root = roots
            .get(idx)
            .ok_or_else(|| format!("invalid root_index: {idx}"))?
            .clone();
        vec![(idx, root)]
    } else {
        roots
            .iter()
            .enumerate()
            .map(|(idx, root)| (idx, root.clone()))
            .collect()
    };

    let allowed = media_extensions();
    let query = options
        .query
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let has_query = !query.is_empty();

    let mut all_matches: Vec<LibraryMediaItem> = Vec::new();

    for (_idx, root) in roots_to_scan {
        for entry in WalkDir::new(&root).into_iter().filter_map(Result::ok) {
            if !entry.file_type().is_file() {
                continue;
            }

            let extension = entry
                .path()
                .extension()
                .and_then(|v| v.to_str())
                .map(|v| v.to_ascii_lowercase());

            let Some(ext) = extension else {
                continue;
            };

            if !allowed.contains(ext.as_str()) {
                continue;
            }

            let media_path = entry.path().to_path_buf();
            let file_name = media_path
                .file_name()
                .and_then(|v| v.to_str())
                .unwrap_or_default()
                .to_string();
            let relative_path = media_path
                .strip_prefix(&root)
                .map(|v| v.display().to_string())
                .unwrap_or_else(|_| file_name.clone());

            if has_query {
                let haystack = format!(
                    "{} {}",
                    file_name.to_ascii_lowercase(),
                    relative_path.to_ascii_lowercase()
                );
                if !haystack.contains(&query) {
                    continue;
                }
            }

            let sidecar_path = match media_path.parent() {
                Some(parent) => parent.join(".mm.json"),
                None => continue,
            };

            all_matches.push(LibraryMediaItem {
                media_path: media_path.display().to_string(),
                root: root.display().to_string(),
                relative_path,
                file_name,
                extension: ext,
                sidecar_path: sidecar_path.display().to_string(),
                sidecar_exists: sidecar_path.exists(),
            });
        }
    }

    // Keep paging deterministic regardless of filesystem traversal order.
    all_matches.sort_by(|a, b| a.media_path.cmp(&b.media_path));

    let total_matches = all_matches.len();
    let items = all_matches
        .into_iter()
        .skip(options.offset)
        .take(normalized_limit)
        .collect();

    Ok(LibraryBrowseResult {
        total_matches,
        offset: options.offset,
        limit: normalized_limit,
        items,
    })
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
        "mkv", "mp4", "avi", "mov", "m4v", "wmv", "flv", "webm", "ts", "m2ts", "mpg", "mpeg",
    ]
    .into_iter()
    .collect()
}

fn normalize_library_limit(limit: usize) -> usize {
    if limit == 0 {
        return DEFAULT_LIBRARY_PAGE_LIMIT;
    }

    limit.min(MAX_LIBRARY_PAGE_LIMIT)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::{LibraryBrowseOptions, list_library_media, scan_library_roots};

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

    #[test]
    fn lists_media_with_search_and_paging() {
        let root = unique_temp_dir("list-media");
        fs::create_dir_all(root.join("movies")).expect("create movies dir");
        fs::write(root.join("movies/alpha.mkv"), b"x").expect("write alpha");
        fs::write(root.join("movies/beta.mp4"), b"x").expect("write beta");
        fs::write(root.join("movies/note.txt"), b"x").expect("write note");

        let all = list_library_media(
            std::slice::from_ref(&root),
            LibraryBrowseOptions {
                offset: 0,
                limit: 10,
                ..Default::default()
            },
        )
        .expect("list all media");
        assert_eq!(all.total_matches, 2);
        assert_eq!(all.items.len(), 2);

        let filtered = list_library_media(
            std::slice::from_ref(&root),
            LibraryBrowseOptions {
                query: Some("alpha".to_string()),
                offset: 0,
                limit: 10,
                ..Default::default()
            },
        )
        .expect("list filtered media");
        assert_eq!(filtered.total_matches, 1);
        assert_eq!(filtered.items[0].file_name, "alpha.mkv");

        let paged = list_library_media(
            std::slice::from_ref(&root),
            LibraryBrowseOptions {
                offset: 1,
                limit: 1,
                ..Default::default()
            },
        )
        .expect("list paged media");
        assert_eq!(paged.total_matches, 2);
        assert_eq!(paged.items.len(), 1);

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
