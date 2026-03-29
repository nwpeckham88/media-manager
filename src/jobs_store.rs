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
    Canceled,
}

impl JobStatus {
    fn as_str(&self) -> &'static str {
        match self {
            JobStatus::Running => "running",
            JobStatus::Succeeded => "succeeded",
            JobStatus::Failed => "failed",
            JobStatus::Canceled => "canceled",
        }
    }

    fn from_str(v: &str) -> Self {
        match v {
            "succeeded" => JobStatus::Succeeded,
            "failed" => JobStatus::Failed,
            "canceled" => JobStatus::Canceled,
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

    pub fn create_job(
        &self,
        kind: &str,
        payload_json: &str,
        now_ms: u128,
    ) -> Result<i64, JobsStoreError> {
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

    pub fn recent_jobs_filtered(
        &self,
        status: Option<&str>,
        kind_like: Option<&str>,
        bulk_only: bool,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<JobRecord>, JobsStoreError> {
        let lock = self.conn.lock().map_err(|_| JobsStoreError::LockPoisoned)?;
        let mut sql = String::from(
            "SELECT id, kind, status, created_at_ms, updated_at_ms, payload_json, result_json, error
                 FROM jobs",
        );
        let mut conditions: Vec<String> = Vec::new();
        let mut params: Vec<rusqlite::types::Value> = Vec::new();

        if let Some(status_value) = status {
            conditions.push("status = ?".to_string());
            params.push(rusqlite::types::Value::Text(status_value.to_string()));
        }

        if let Some(kind_value) = kind_like {
            conditions.push("kind LIKE ?".to_string());
            params.push(rusqlite::types::Value::Text(format!("%{kind_value}%")));
        }

        if bulk_only {
            conditions.push("kind LIKE ?".to_string());
            params.push(rusqlite::types::Value::Text("bulk_%".to_string()));
        }

        if !conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&conditions.join(" AND "));
        }

        sql.push_str(" ORDER BY id DESC LIMIT ? OFFSET ?");
        params.push(rusqlite::types::Value::Integer(limit as i64));
        params.push(rusqlite::types::Value::Integer(offset as i64));

        let mut stmt = lock
            .prepare(&sql)
            .map_err(|e| JobsStoreError::Sql(e.to_string()))?;

        let rows = stmt
            .query_map(rusqlite::params_from_iter(params), |row| {
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

    pub fn count_jobs_filtered(
        &self,
        status: Option<&str>,
        kind_like: Option<&str>,
        bulk_only: bool,
    ) -> Result<usize, JobsStoreError> {
        let lock = self.conn.lock().map_err(|_| JobsStoreError::LockPoisoned)?;
        let mut sql = String::from("SELECT COUNT(*) FROM jobs");
        let mut conditions: Vec<String> = Vec::new();
        let mut params: Vec<rusqlite::types::Value> = Vec::new();

        if let Some(status_value) = status {
            conditions.push("status = ?".to_string());
            params.push(rusqlite::types::Value::Text(status_value.to_string()));
        }

        if let Some(kind_value) = kind_like {
            conditions.push("kind LIKE ?".to_string());
            params.push(rusqlite::types::Value::Text(format!("%{kind_value}%")));
        }

        if bulk_only {
            conditions.push("kind LIKE ?".to_string());
            params.push(rusqlite::types::Value::Text("bulk_%".to_string()));
        }

        if !conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&conditions.join(" AND "));
        }

        let mut stmt = lock
            .prepare(&sql)
            .map_err(|e| JobsStoreError::Sql(e.to_string()))?;
        let count: i64 = stmt
            .query_row(rusqlite::params_from_iter(params), |row| row.get(0))
            .map_err(|e| JobsStoreError::Sql(e.to_string()))?;

        Ok(count.max(0) as usize)
    }

    pub fn get_job(&self, id: i64) -> Result<Option<JobRecord>, JobsStoreError> {
        let lock = self.conn.lock().map_err(|_| JobsStoreError::LockPoisoned)?;
        let mut stmt = lock
            .prepare(
                "SELECT id, kind, status, created_at_ms, updated_at_ms, payload_json, result_json, error
                 FROM jobs
                 WHERE id = ?1",
            )
            .map_err(|e| JobsStoreError::Sql(e.to_string()))?;

        let mut rows = stmt
            .query([id])
            .map_err(|e| JobsStoreError::Sql(e.to_string()))?;

        let Some(row) = rows
            .next()
            .map_err(|e| JobsStoreError::Sql(e.to_string()))?
        else {
            return Ok(None);
        };

        let status: String = row.get(2).map_err(|e| JobsStoreError::Sql(e.to_string()))?;
        Ok(Some(JobRecord {
            id: row.get(0).map_err(|e| JobsStoreError::Sql(e.to_string()))?,
            kind: row.get(1).map_err(|e| JobsStoreError::Sql(e.to_string()))?,
            status: JobStatus::from_str(&status),
            created_at_ms: row
                .get::<_, i64>(3)
                .map_err(|e| JobsStoreError::Sql(e.to_string()))?
                as u128,
            updated_at_ms: row
                .get::<_, i64>(4)
                .map_err(|e| JobsStoreError::Sql(e.to_string()))?
                as u128,
            payload_json: row.get(5).map_err(|e| JobsStoreError::Sql(e.to_string()))?,
            result_json: row.get(6).map_err(|e| JobsStoreError::Sql(e.to_string()))?,
            error: row.get(7).map_err(|e| JobsStoreError::Sql(e.to_string()))?,
        }))
    }

    pub fn set_job_status(
        &self,
        id: i64,
        status: JobStatus,
        error: Option<&str>,
        now_ms: u128,
    ) -> Result<(), JobsStoreError> {
        let lock = self.conn.lock().map_err(|_| JobsStoreError::LockPoisoned)?;
        lock.execute(
            "UPDATE jobs SET status = ?1, updated_at_ms = ?2, error = ?3 WHERE id = ?4",
            params![status.as_str(), now_ms as i64, error, id],
        )
        .map_err(|e| JobsStoreError::Sql(e.to_string()))?;
        Ok(())
    }
}
