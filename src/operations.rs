use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

const MAX_EVENTS: usize = 500;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    ScanSummary,
    LibraryBrowse,
    JobControl,
    SidecarRead,
    SidecarUpsert,
}

impl OperationKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            OperationKind::ScanSummary => "scan_summary",
            OperationKind::LibraryBrowse => "library_browse",
            OperationKind::JobControl => "job_control",
            OperationKind::SidecarRead => "sidecar_read",
            OperationKind::SidecarUpsert => "sidecar_upsert",
        }
    }

    pub fn from_db_str(value: &str) -> Self {
        match value {
            "scan_summary" => OperationKind::ScanSummary,
            "library_browse" => OperationKind::LibraryBrowse,
            "job_control" => OperationKind::JobControl,
            "sidecar_read" => OperationKind::SidecarRead,
            "sidecar_upsert" => OperationKind::SidecarUpsert,
            _ => OperationKind::SidecarRead,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct OperationEvent {
    pub timestamp_ms: u128,
    pub kind: OperationKind,
    pub detail: String,
    pub success: bool,
}

#[derive(Clone)]
pub struct OperationLog {
    events: Arc<Mutex<VecDeque<OperationEvent>>>,
}

impl OperationLog {
    pub fn new() -> Self {
        Self {
            events: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    pub fn push(&self, kind: OperationKind, detail: impl Into<String>, success: bool) {
        let Ok(mut lock) = self.events.lock() else {
            return;
        };

        if lock.len() >= MAX_EVENTS {
            lock.pop_front();
        }

        lock.push_back(OperationEvent {
            timestamp_ms: now_ms(),
            kind,
            detail: detail.into(),
            success,
        });
    }

    pub fn recent(&self, limit: usize) -> Vec<OperationEvent> {
        let Ok(lock) = self.events.lock() else {
            return Vec::new();
        };

        let take = limit.min(lock.len());
        lock.iter().rev().take(take).cloned().collect()
    }
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}
