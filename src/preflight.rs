use std::path::PathBuf;

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
    state_dir: &PathBuf,
    toolchain: &ToolchainSnapshot,
) -> PreflightReport {
    let mut checks = Vec::new();

    checks.push(check_library_roots(library_roots));
    checks.push(check_state_dir(state_dir));
    checks.push(check_tool("ffmpeg", &toolchain.ffmpeg.status));
    checks.push(check_tool("ffprobe", &toolchain.ffprobe.status));

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

    let missing: Vec<String> = library_roots
        .iter()
        .filter(|p| !p.exists())
        .map(|p| p.display().to_string())
        .collect();

    if missing.is_empty() {
        return PreflightCheck {
            name: "library_roots".to_string(),
            ok: true,
            detail: format!("{} root(s) configured", library_roots.len()),
        };
    }

    PreflightCheck {
        name: "library_roots".to_string(),
        ok: false,
        detail: format!("missing root(s): {}", missing.join(", ")),
    }
}

fn check_state_dir(state_dir: &PathBuf) -> PreflightCheck {
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
