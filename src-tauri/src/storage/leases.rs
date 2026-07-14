//! Window / request leases and deletion guards.

use rusqlite::{params, Connection, OptionalExtension};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LeaseError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("deletion guard active for task {0}")]
    DeletionGuard(String),
    #[error("lease not found")]
    NotFound,
}

pub const DEFAULT_TTL_MS: i64 = 60_000;
pub const RENEW_INTERVAL_MS: i64 = 30_000;
pub const SNAPSHOT_DEADLINE_MS: i64 = 45_000;

/// In-memory deletion tombstones (per storage actor). Persistence table optional;
/// Phase 1 foundations use a side set checked before lease acquire.
#[derive(Debug, Default)]
pub struct DeletionGuards {
    guards: std::sync::Mutex<std::collections::HashSet<String>>,
}

impl DeletionGuards {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn acquire(&self, task_id: &str) -> bool {
        let mut g = self.guards.lock().unwrap();
        g.insert(task_id.to_string())
    }

    pub fn release(&self, task_id: &str) {
        self.guards.lock().unwrap().remove(task_id);
    }

    pub fn is_guarded(&self, task_id: &str) -> bool {
        self.guards.lock().unwrap().contains(task_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lease {
    pub connection_id: String,
    pub lease_id: String,
    pub scope: String,
    pub expires_at: i64,
    pub created_at: i64,
}

pub fn scope_daemon() -> String {
    "daemon".into()
}

pub fn scope_task(task_id: &str) -> String {
    format!("task:{task_id}")
}

/// Idempotent acquire: same (connection_id, lease_id) renews TTL; scope cannot change.
pub fn acquire(
    conn: &Connection,
    guards: &DeletionGuards,
    connection_id: &str,
    lease_id: &str,
    scope: &str,
    now_ms: i64,
    ttl_ms: i64,
) -> Result<Lease, LeaseError> {
    if let Some(task_id) = scope.strip_prefix("task:") {
        if guards.is_guarded(task_id) {
            return Err(LeaseError::DeletionGuard(task_id.into()));
        }
    }
    let expires = now_ms + ttl_ms;
    if let Some(existing) = get(conn, connection_id, lease_id)? {
        if existing.scope != scope {
            return Err(LeaseError::Sqlite(rusqlite::Error::InvalidParameterName(
                "lease scope immutable".into(),
            )));
        }
        conn.execute(
            "UPDATE leases SET expires_at = ?1 WHERE connection_id = ?2 AND lease_id = ?3",
            params![expires, connection_id, lease_id],
        )?;
        return Ok(Lease {
            expires_at: expires,
            ..existing
        });
    }
    conn.execute(
        "INSERT INTO leases (connection_id, lease_id, scope, expires_at, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![connection_id, lease_id, scope, expires, now_ms],
    )?;
    Ok(Lease {
        connection_id: connection_id.into(),
        lease_id: lease_id.into(),
        scope: scope.into(),
        expires_at: expires,
        created_at: now_ms,
    })
}

pub fn renew(
    conn: &Connection,
    connection_id: &str,
    lease_id: &str,
    now_ms: i64,
    ttl_ms: i64,
) -> Result<Lease, LeaseError> {
    let existing = get(conn, connection_id, lease_id)?.ok_or(LeaseError::NotFound)?;
    let expires = now_ms + ttl_ms;
    conn.execute(
        "UPDATE leases SET expires_at = ?1 WHERE connection_id = ?2 AND lease_id = ?3",
        params![expires, connection_id, lease_id],
    )?;
    Ok(Lease {
        expires_at: expires,
        ..existing
    })
}

pub fn release(conn: &Connection, connection_id: &str, lease_id: &str) -> Result<(), LeaseError> {
    conn.execute(
        "DELETE FROM leases WHERE connection_id = ?1 AND lease_id = ?2",
        params![connection_id, lease_id],
    )?;
    Ok(())
}

pub fn release_connection(conn: &Connection, connection_id: &str) -> Result<usize, LeaseError> {
    let n = conn.execute(
        "DELETE FROM leases WHERE connection_id = ?1",
        params![connection_id],
    )?;
    Ok(n)
}

pub fn expire_stale(conn: &Connection, now_ms: i64) -> Result<usize, LeaseError> {
    let n = conn.execute("DELETE FROM leases WHERE expires_at <= ?1", params![now_ms])?;
    Ok(n)
}

pub fn get(
    conn: &Connection,
    connection_id: &str,
    lease_id: &str,
) -> Result<Option<Lease>, LeaseError> {
    let row = conn
        .query_row(
            "SELECT connection_id, lease_id, scope, expires_at, created_at
             FROM leases WHERE connection_id = ?1 AND lease_id = ?2",
            params![connection_id, lease_id],
            |r| {
                Ok(Lease {
                    connection_id: r.get(0)?,
                    lease_id: r.get(1)?,
                    scope: r.get(2)?,
                    expires_at: r.get(3)?,
                    created_at: r.get(4)?,
                })
            },
        )
        .optional()?;
    Ok(row)
}

pub fn task_has_active_lease(
    conn: &Connection,
    task_id: &str,
    now_ms: i64,
) -> Result<bool, LeaseError> {
    let scope = scope_task(task_id);
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM leases WHERE scope = ?1 AND expires_at > ?2",
        params![scope, now_ms],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::db::open_memory;

    #[test]
    fn acquire_renew_release() {
        let conn = open_memory().unwrap();
        let guards = DeletionGuards::new();
        let l = acquire(
            &conn,
            &guards,
            "c1",
            "lease1",
            &scope_task("t1"),
            1000,
            DEFAULT_TTL_MS,
        )
        .unwrap();
        assert_eq!(l.expires_at, 1000 + DEFAULT_TTL_MS);
        let l2 = renew(&conn, "c1", "lease1", 2000, DEFAULT_TTL_MS).unwrap();
        assert_eq!(l2.expires_at, 2000 + DEFAULT_TTL_MS);
        release(&conn, "c1", "lease1").unwrap();
        assert!(get(&conn, "c1", "lease1").unwrap().is_none());
    }

    #[test]
    fn deletion_guard_blocks_acquire() {
        let conn = open_memory().unwrap();
        let guards = DeletionGuards::new();
        assert!(guards.acquire("t1"));
        let err = acquire(
            &conn,
            &guards,
            "c1",
            "l1",
            &scope_task("t1"),
            0,
            DEFAULT_TTL_MS,
        );
        assert!(matches!(err, Err(LeaseError::DeletionGuard(_))));
    }

    #[test]
    fn connection_disconnect_releases_all() {
        let conn = open_memory().unwrap();
        let guards = DeletionGuards::new();
        acquire(&conn, &guards, "c1", "a", &scope_daemon(), 0, 60_000).unwrap();
        acquire(&conn, &guards, "c1", "b", &scope_task("t1"), 0, 60_000).unwrap();
        let n = release_connection(&conn, "c1").unwrap();
        assert_eq!(n, 2);
    }
}
