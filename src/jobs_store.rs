use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::{Connection, params};
use serde::Serialize;
use thiserror::Error;

#[derive(Clone)]
pub struct JobsStore {
    conn: Arc<Mutex<Connection>>,
}

#[derive(Debug, Error)]
pub enum JobsStoreError {
    #[error("sqlite error: {0}")]
    Sql(String),
    #[error("store lock poisoned")]
    LockPoisoned,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Running,
    Succeeded,
    Failed,
}

impl JobStatus {
    fn as_str(&self) -> &'static str {
        match self {
            JobStatus::Running => "running",
            JobStatus::Succeeded => "succeeded",
            JobStatus::Failed => "failed",
        }
    }

    fn from_str(v: &str) -> Self {
        match v {
            "succeeded" => JobStatus::Succeeded,
            "failed" => JobStatus::Failed,
            _ => JobStatus::Running,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct JobRecord {
    pub id: i64,
    pub kind: String,
    pub status: JobStatus,
    pub created_at_ms: u128,
    pub updated_at_ms: u128,
    pub payload_json: String,
    pub result_json: Option<String>,
    pub error: Option<String>,
}

impl JobsStore {
    pub fn open(path: &Path) -> Result<Self, JobsStoreError> {
        let conn = Connection::open(path).map_err(|e| JobsStoreError::Sql(e.to_string()))?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn create_job(&self, kind: &str, payload_json: &str, now_ms: u128) -> Result<i64, JobsStoreError> {
        let lock = self.conn.lock().map_err(|_| JobsStoreError::LockPoisoned)?;
        lock.execute(
            "INSERT INTO jobs(kind, status, created_at_ms, updated_at_ms, payload_json) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![kind, JobStatus::Running.as_str(), now_ms as i64, now_ms as i64, payload_json],
        )
        .map_err(|e| JobsStoreError::Sql(e.to_string()))?;
        Ok(lock.last_insert_rowid())
    }

    pub fn complete_job(
        &self,
        id: i64,
        status: JobStatus,
        result_json: Option<&str>,
        error: Option<&str>,
        now_ms: u128,
    ) -> Result<(), JobsStoreError> {
        let lock = self.conn.lock().map_err(|_| JobsStoreError::LockPoisoned)?;
        lock.execute(
            "UPDATE jobs SET status = ?1, updated_at_ms = ?2, result_json = ?3, error = ?4 WHERE id = ?5",
            params![status.as_str(), now_ms as i64, result_json, error, id],
        )
        .map_err(|e| JobsStoreError::Sql(e.to_string()))?;
        Ok(())
    }

    pub fn recent_jobs(&self, limit: usize) -> Result<Vec<JobRecord>, JobsStoreError> {
        let lock = self.conn.lock().map_err(|_| JobsStoreError::LockPoisoned)?;
        let mut stmt = lock
            .prepare(
                "SELECT id, kind, status, created_at_ms, updated_at_ms, payload_json, result_json, error
                 FROM jobs
                 ORDER BY id DESC
                 LIMIT ?1",
            )
            .map_err(|e| JobsStoreError::Sql(e.to_string()))?;

        let rows = stmt
            .query_map([limit as i64], |row| {
                let status: String = row.get(2)?;
                Ok(JobRecord {
                    id: row.get(0)?,
                    kind: row.get(1)?,
                    status: JobStatus::from_str(&status),
                    created_at_ms: row.get::<_, i64>(3)? as u128,
                    updated_at_ms: row.get::<_, i64>(4)? as u128,
                    payload_json: row.get(5)?,
                    result_json: row.get(6)?,
                    error: row.get(7)?,
                })
            })
            .map_err(|e| JobsStoreError::Sql(e.to_string()))?;

        let mut jobs = Vec::new();
        for row in rows {
            jobs.push(row.map_err(|e| JobsStoreError::Sql(e.to_string()))?);
        }
        Ok(jobs)
    }
}
