use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use thiserror::Error;

pub const LATEST_SCHEMA_VERSION: i64 = 1;

#[derive(Debug, Error)]
pub enum DbMigrationError {
    #[error("sqlite open failed: {0}")]
    Open(String),
    #[error("sqlite migrate failed: {0}")]
    Sql(String),
}

pub fn run(path: &Path) -> Result<(), DbMigrationError> {
    let mut conn = Connection::open(path).map_err(|e| DbMigrationError::Open(e.to_string()))?;

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at_ms INTEGER NOT NULL
        );
        ",
    )
    .map_err(|e| DbMigrationError::Sql(e.to_string()))?;

    let current_version: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
        .map_err(|e| DbMigrationError::Sql(e.to_string()))?;

    for migration in migrations() {
        if migration.version <= current_version {
            continue;
        }

        let tx = conn
            .transaction()
            .map_err(|e| DbMigrationError::Sql(e.to_string()))?;
        tx.execute_batch(migration.sql)
            .map_err(|e| DbMigrationError::Sql(e.to_string()))?;
        tx.execute(
            "INSERT INTO schema_migrations(version, name, applied_at_ms) VALUES (?1, ?2, ?3)",
            (migration.version, migration.name, now_ms() as i64),
        )
        .map_err(|e| DbMigrationError::Sql(e.to_string()))?;
        tx.commit()
            .map_err(|e| DbMigrationError::Sql(e.to_string()))?;
    }

    Ok(())
}

struct Migration {
    version: i64,
    name: &'static str,
    sql: &'static str,
}

fn migrations() -> &'static [Migration] {
    &[Migration {
        version: 1,
        name: "initial_audit_and_jobs",
        sql: r#"
            CREATE TABLE IF NOT EXISTS operation_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp_ms INTEGER NOT NULL,
                kind TEXT NOT NULL,
                detail TEXT NOT NULL,
                success INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_operation_events_timestamp
            ON operation_events(timestamp_ms DESC);

            CREATE TABLE IF NOT EXISTS jobs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                kind TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                payload_json TEXT NOT NULL,
                result_json TEXT,
                error TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_jobs_updated
            ON jobs(updated_at_ms DESC);
        "#,
    }]
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use rusqlite::Connection;

    use super::{LATEST_SCHEMA_VERSION, run};

    fn unique_temp_path(name: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        dir.push(format!("mm-{name}-{nanos}.sqlite3"));
        dir
    }

    #[test]
    fn migrates_fresh_database_to_latest() {
        let db_path = unique_temp_path("migrate-fresh");

        run(&db_path).expect("run migrations");

        let conn = Connection::open(&db_path).expect("open db");
        let version: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
                [],
                |r| r.get(0),
            )
            .expect("read version");
        assert_eq!(version, LATEST_SCHEMA_VERSION);

        let tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('operation_events', 'jobs')",
                [],
                |r| r.get(0),
            )
            .expect("check tables");
        assert_eq!(tables, 2);

        fs::remove_file(db_path).expect("cleanup db");
    }

    #[test]
    fn upgrades_legacy_unversioned_database() {
        let db_path = unique_temp_path("migrate-legacy");
        let conn = Connection::open(&db_path).expect("open db");
        conn.execute_batch(
            "
            CREATE TABLE jobs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                kind TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                payload_json TEXT NOT NULL,
                result_json TEXT,
                error TEXT
            );
            INSERT INTO jobs(kind, status, created_at_ms, updated_at_ms, payload_json)
            VALUES ('legacy', 'running', 1, 1, '{}');
            ",
        )
        .expect("seed legacy schema");
        drop(conn);

        run(&db_path).expect("run migrations");

        let conn = Connection::open(&db_path).expect("open db after migration");
        let version: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
                [],
                |r| r.get(0),
            )
            .expect("read version");
        assert_eq!(version, LATEST_SCHEMA_VERSION);

        let legacy_row_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM jobs WHERE kind='legacy'", [], |r| {
                r.get(0)
            })
            .expect("read legacy jobs");
        assert_eq!(legacy_row_count, 1);

        let operation_table_exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='operation_events'",
                [],
                |r| r.get(0),
            )
            .expect("check operation table");
        assert_eq!(operation_table_exists, 1);

        fs::remove_file(db_path).expect("cleanup db");
    }

    #[test]
    fn migration_runner_is_idempotent() {
        let db_path = unique_temp_path("migrate-idempotent");

        run(&db_path).expect("first migration run");
        run(&db_path).expect("second migration run");

        let conn = Connection::open(&db_path).expect("open db");
        let migration_rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |r| r.get(0))
            .expect("count migration rows");
        assert_eq!(migration_rows, 1);

        fs::remove_file(db_path).expect("cleanup db");
    }
}
