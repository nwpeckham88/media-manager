use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::{Connection, params};
use thiserror::Error;

use crate::operations::{OperationEvent, OperationKind};

#[derive(Clone)]
pub struct AuditStore {
    conn: Arc<Mutex<Connection>>,
}

#[derive(Debug, Error)]
pub enum AuditStoreError {
    #[error("sqlite error: {0}")]
    Sql(String),
    #[error("store lock poisoned")]
    LockPoisoned,
}

impl AuditStore {
    pub fn open(path: &Path) -> Result<Self, AuditStoreError> {
        let conn = Connection::open(path).map_err(|e| AuditStoreError::Sql(e.to_string()))?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn insert_event(&self, event: &OperationEvent) -> Result<(), AuditStoreError> {
        let lock = self.conn.lock().map_err(|_| AuditStoreError::LockPoisoned)?;
        lock.execute(
            "INSERT INTO operation_events(timestamp_ms, kind, detail, success) VALUES (?1, ?2, ?3, ?4)",
            params![
                event.timestamp_ms as i64,
                event.kind.as_str(),
                event.detail,
                if event.success { 1_i64 } else { 0_i64 }
            ],
        )
        .map_err(|e| AuditStoreError::Sql(e.to_string()))?;

        Ok(())
    }

    pub fn recent_events(&self, limit: usize) -> Result<Vec<OperationEvent>, AuditStoreError> {
        let lock = self.conn.lock().map_err(|_| AuditStoreError::LockPoisoned)?;
        let mut stmt = lock
            .prepare(
                "SELECT timestamp_ms, kind, detail, success
                 FROM operation_events
                 ORDER BY id DESC
                 LIMIT ?1",
            )
            .map_err(|e| AuditStoreError::Sql(e.to_string()))?;

        let rows = stmt
            .query_map([limit as i64], |row| {
                let kind: String = row.get(1)?;
                Ok(OperationEvent {
                    timestamp_ms: row.get::<_, i64>(0)? as u128,
                    kind: OperationKind::from_db_str(&kind),
                    detail: row.get(2)?,
                    success: row.get::<_, i64>(3)? == 1,
                })
            })
            .map_err(|e| AuditStoreError::Sql(e.to_string()))?;

        let mut events = Vec::new();
        for row in rows {
            events.push(row.map_err(|e| AuditStoreError::Sql(e.to_string()))?);
        }
        Ok(events)
    }
}
