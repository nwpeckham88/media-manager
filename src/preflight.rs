use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::toolchain::{ProbeStatus, ToolchainSnapshot};

#[derive(Debug, Clone, Serialize)]
pub struct PreflightReport {
    pub ready: bool,
    pub checks: Vec<PreflightCheck>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PreflightCheck {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

pub fn run_preflight(
    library_roots: &[PathBuf],
    state_dir: &Path,
    toolchain: &ToolchainSnapshot,
) -> PreflightReport {
    let mut checks = vec![
        check_library_roots(library_roots),
        check_state_dir(state_dir),
        check_tool("ffmpeg", &toolchain.ffmpeg.status),
        check_tool("ffprobe", &toolchain.ffprobe.status),
    ];

    if let Some(mediainfo) = &toolchain.mediainfo {
        checks.push(check_tool("mediainfo", &mediainfo.status));
    }

    PreflightReport {
        ready: checks.iter().all(|c| c.ok),
        checks,
    }
}

fn check_library_roots(library_roots: &[PathBuf]) -> PreflightCheck {
    if library_roots.is_empty() {
        return PreflightCheck {
            name: "library_roots".to_string(),
            ok: false,
            detail: "MM_LIBRARY_ROOTS is empty".to_string(),
        };
    }

    let invalid: Vec<String> = library_roots
        .iter()
        .filter_map(|p| {
            validate_library_root(p)
                .err()
                .map(|reason| format!("{} ({reason})", p.display()))
        })
        .collect();

    if invalid.is_empty() {
        return PreflightCheck {
            name: "library_roots".to_string(),
            ok: true,
            detail: format!("{} root(s) configured", library_roots.len()),
        };
    }

    PreflightCheck {
        name: "library_roots".to_string(),
        ok: false,
        detail: format!("invalid root(s): {}", invalid.join(", ")),
    }
}

fn validate_library_root(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err("missing".to_string());
    }

    if !path.is_dir() {
        return Err("not a directory".to_string());
    }

    std::fs::read_dir(path)
        .map(|_| ())
        .map_err(|err| format!("unreadable: {err}"))
}

fn check_state_dir(state_dir: &Path) -> PreflightCheck {
    match std::fs::create_dir_all(state_dir) {
        Ok(()) => PreflightCheck {
            name: "state_dir".to_string(),
            ok: true,
            detail: format!("writable: {}", state_dir.display()),
        },
        Err(err) => PreflightCheck {
            name: "state_dir".to_string(),
            ok: false,
            detail: format!("cannot create state dir {} ({})", state_dir.display(), err),
        },
    }
}

fn check_tool(name: &str, status: &ProbeStatus) -> PreflightCheck {
    let ok = matches!(status, ProbeStatus::Ok);
    let detail = if ok {
        "available".to_string()
    } else {
        "unavailable".to_string()
    };

    PreflightCheck {
        name: name.to_string(),
        ok,
        detail,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use crate::toolchain::{ProbeStatus, ResolvedBinary, ToolchainSnapshot};

    use super::run_preflight;

    fn unique_temp_dir(name: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        dir.push(format!("mm-preflight-{name}-{nanos}"));
        dir
    }

    fn toolchain_ok() -> ToolchainSnapshot {
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

    #[test]
    fn preflight_rejects_root_that_is_file() {
        let root = unique_temp_dir("root-is-file");
        fs::create_dir_all(&root).expect("create root dir");
        let not_dir = root.join("library.txt");
        fs::write(&not_dir, b"x").expect("write file root");

        let state_dir = root.join("state");
        let report = run_preflight(std::slice::from_ref(&not_dir), &state_dir, &toolchain_ok());

        assert!(!report.ready);
        let check = report
            .checks
            .iter()
            .find(|c| c.name == "library_roots")
            .expect("library_roots check");
        assert!(!check.ok);
        assert!(check.detail.contains("not a directory"));

        fs::remove_dir_all(root).expect("cleanup root");
    }

    #[cfg(unix)]
    #[test]
    fn preflight_rejects_unreadable_root() {
        use std::os::unix::fs::PermissionsExt;

        let root = unique_temp_dir("root-unreadable");
        let blocked = root.join("blocked");
        fs::create_dir_all(&blocked).expect("create blocked dir");

        let original_mode = fs::metadata(&blocked)
            .expect("stat blocked dir")
            .permissions()
            .mode();
        let mut no_access = fs::metadata(&blocked)
            .expect("stat blocked dir for chmod")
            .permissions();
        no_access.set_mode(0o000);
        fs::set_permissions(&blocked, no_access).expect("chmod blocked dir");

        let state_dir = root.join("state");
        let report = run_preflight(std::slice::from_ref(&blocked), &state_dir, &toolchain_ok());

        let mut restore = fs::metadata(&blocked)
            .expect("stat blocked dir for restore")
            .permissions();
        restore.set_mode(original_mode);
        fs::set_permissions(&blocked, restore).expect("restore blocked dir permissions");
        fs::remove_dir_all(root).expect("cleanup root");

        assert!(!report.ready);
        let check = report
            .checks
            .iter()
            .find(|c| c.name == "library_roots")
            .expect("library_roots check");
        assert!(!check.ok);
        assert!(check.detail.contains("unreadable"));
    }
}
