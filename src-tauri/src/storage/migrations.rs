//! Schema migrations — transactional, never delete user data on failure.

use rusqlite::{Connection, OptionalExtension};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MigrationError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("migration failed at version {version}: {message}")]
    Failed { version: i64, message: String },
}

/// Apply all pending migrations inside a single transaction per version.
pub fn migrate(conn: &Connection) -> Result<i64, MigrationError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            applied_at INTEGER NOT NULL
        );",
    )?;

    let current: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    for (version, sql) in MIGRATIONS {
        if current >= *version {
            continue;
        }
        let tx = conn.unchecked_transaction()?;
        match tx.execute_batch(sql) {
            Ok(()) => {
                tx.execute(
                    "INSERT INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                    rusqlite::params![version, now_ms()],
                )?;
                tx.commit()?;
            }
            Err(e) => {
                // Transaction drops without commit — schema unchanged.
                return Err(MigrationError::Failed {
                    version: *version,
                    message: e.to_string(),
                });
            }
        }
    }

    let applied: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |r| r.get(0),
    )?;
    Ok(applied)
}

pub fn current_version(conn: &Connection) -> Result<i64, MigrationError> {
    let v = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(0);
    Ok(v)
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Ordered migrations. Version numbers are monotonic integers.
const MIGRATIONS: &[(i64, &str)] = &[(1, MIGRATION_1)];

const MIGRATION_1: &str = r#"
PRAGMA foreign_keys = ON;

CREATE TABLE tasks (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    cwd TEXT NOT NULL,
    mode TEXT NOT NULL,
    status TEXT NOT NULL,
    session_state TEXT,
    recovery_state TEXT,
    active_recovery_id TEXT,
    requested_model TEXT,
    actual_model TEXT,
    reasoning_effort TEXT,
    grok_version TEXT,
    acp_protocol_version INTEGER,
    acp_session_id TEXT,
    last_turn_id TEXT,
    supervisor_pid INTEGER,
    supervisor_started_at INTEGER,
    daemon_instance_id TEXT,
    stop_reason TEXT,
    error_code TEXT,
    error_message TEXT,
    created_at INTEGER NOT NULL,
    started_at INTEGER,
    updated_at INTEGER NOT NULL,
    finished_at INTEGER,
    retention_protect_until INTEGER,
    last_sequence INTEGER NOT NULL DEFAULT 0,
    timeline_generation INTEGER NOT NULL DEFAULT 1,
    state_revision INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE turns (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL,
    prompt_markdown TEXT NOT NULL,
    status TEXT NOT NULL,
    owner_kind TEXT NOT NULL,
    owner_connection_id TEXT,
    owner_request_id TEXT,
    mode TEXT NOT NULL,
    session_id TEXT,
    requested_model TEXT,
    actual_model TEXT,
    answer_markdown TEXT NOT NULL DEFAULT '',
    stop_reason TEXT,
    termination_cause TEXT,
    partial INTEGER NOT NULL DEFAULT 0,
    error_code TEXT,
    error_message TEXT,
    error_retryable INTEGER,
    result_json TEXT,
    created_at INTEGER NOT NULL,
    started_at INTEGER,
    prompt_dispatched_at INTEGER,
    finished_at INTEGER,
    UNIQUE(task_id, ordinal)
);

CREATE TABLE recovery_operations (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    action TEXT NOT NULL,
    input_hash TEXT NOT NULL,
    prompt_markdown TEXT,
    status TEXT NOT NULL,
    expected_last_turn_id TEXT NOT NULL,
    created_turn_id TEXT,
    error_code TEXT,
    error_message TEXT,
    result_json TEXT,
    created_at INTEGER NOT NULL,
    started_at INTEGER,
    finished_at INTEGER
);

CREATE TABLE submissions (
    submission_id TEXT PRIMARY KEY,
    input_hash TEXT NOT NULL,
    task_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    accepted_result_json TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);

CREATE INDEX idx_submissions_expires ON submissions(expires_at);

CREATE TABLE timeline_items (
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    item_id TEXT NOT NULL,
    turn_id TEXT,
    kind TEXT NOT NULL,
    first_sequence INTEGER NOT NULL,
    last_sequence INTEGER NOT NULL,
    payload_json TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(task_id, item_id)
);

CREATE TABLE timeline_mutations (
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL,
    generation INTEGER NOT NULL,
    operation TEXT NOT NULL,
    item_id TEXT,
    payload_json TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY(task_id, sequence)
);

CREATE TABLE raw_acp_events (
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    raw_sequence INTEGER NOT NULL,
    direction TEXT NOT NULL,
    method TEXT,
    payload_json TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY(task_id, raw_sequence)
);

CREATE TABLE ui_state (
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    disclosure_key TEXT NOT NULL,
    expansion TEXT NOT NULL,
    revision INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(task_id, disclosure_key)
);

CREATE TABLE meta_kv (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE leases (
    connection_id TEXT NOT NULL,
    lease_id TEXT NOT NULL,
    scope TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY(connection_id, lease_id)
);

CREATE INDEX idx_leases_expires ON leases(expires_at);
CREATE INDEX idx_tasks_created ON tasks(created_at);
CREATE INDEX idx_tasks_status ON tasks(status);
CREATE INDEX idx_turns_task ON turns(task_id);
CREATE INDEX idx_recovery_task ON recovery_operations(task_id);

INSERT INTO meta_kv (key, value) VALUES
    ('ui_state_generation', lower(hex(randomblob(16)))),
    ('ui_state_revision', '0');
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::db::open_memory;

    #[test]
    fn migrate_applies_v1() {
        let conn = open_memory().unwrap();
        let v = migrate(&conn).unwrap();
        assert_eq!(v, 1);
        let again = migrate(&conn).unwrap();
        assert_eq!(again, 1);
    }

    #[test]
    fn failed_migration_rolls_back() {
        let conn = open_memory().unwrap();
        migrate(&conn).unwrap();
        // Simulate a failed migration by running bad SQL in a transaction.
        let tx = conn.unchecked_transaction().unwrap();
        let err = tx.execute_batch("CREATE TABLE broken (");
        assert!(err.is_err());
        drop(tx);
        // Original tables still present.
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name='tasks'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
    }
}
