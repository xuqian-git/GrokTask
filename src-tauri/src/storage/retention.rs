//! History retention: never delete active / leased / protect-until tasks.

use rusqlite::{params, Connection};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RetentionError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

/// Statuses that block retention deletion.
const PROTECTED_STATUSES: &[&str] = &[
    "queued",
    "starting",
    "running",
    "cancelling",
    "recovering",
    "interrupted",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetentionResult {
    pub deleted_ids: Vec<String>,
    pub skipped: usize,
}

/// Delete oldest eligible tasks beyond `history_limit`.
/// `history_limit == 0` still respects retention_protect_until and leases.
pub fn run_retention(
    conn: &Connection,
    history_limit: u32,
    now_ms: i64,
) -> Result<RetentionResult, RetentionError> {
    // Candidates: not protected status, not under protect-until, not leased, not warm.
    let mut stmt = conn.prepare(
        "SELECT id, created_at FROM tasks t
         WHERE t.status NOT IN ('queued','starting','running','cancelling','recovering','interrupted')
           AND (t.retention_protect_until IS NULL OR t.retention_protect_until <= ?1)
           AND (t.session_state IS NULL OR t.session_state != 'warm')
           AND NOT EXISTS (
             SELECT 1 FROM leases l
             WHERE l.scope = 'task:' || t.id AND l.expires_at > ?1
           )
         ORDER BY t.created_at ASC",
    )?;
    let candidates: Vec<(String, i64)> = stmt
        .query_map(params![now_ms], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;

    let total: i64 = conn.query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))?;
    let mut deleted_ids = Vec::new();
    let mut skipped = 0usize;

    if history_limit == 0 {
        // Delete all eligible (protect-until already filtered).
        for (id, _) in candidates {
            conn.execute("DELETE FROM tasks WHERE id = ?1", params![id])?;
            deleted_ids.push(id);
        }
        return Ok(RetentionResult {
            deleted_ids,
            skipped,
        });
    }

    let limit = history_limit as i64;
    let mut remaining = total;
    for (id, _) in candidates {
        if remaining <= limit {
            break;
        }
        let n = conn.execute("DELETE FROM tasks WHERE id = ?1", params![id])?;
        if n > 0 {
            deleted_ids.push(id);
            remaining -= 1;
        } else {
            skipped += 1;
        }
    }

    // Active tasks must never appear in deleted list — sanity via status check above.
    let _ = PROTECTED_STATUSES;
    Ok(RetentionResult {
        deleted_ids,
        skipped,
    })
}

/// Count tasks that would be protected from deletion right now.
pub fn count_protected(conn: &Connection, now_ms: i64) -> Result<i64, RetentionError> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks t
         WHERE t.status IN ('queued','starting','running','cancelling','recovering','interrupted')
            OR (t.retention_protect_until IS NOT NULL AND t.retention_protect_until > ?1)
            OR t.session_state = 'warm'
            OR EXISTS (
              SELECT 1 FROM leases l
              WHERE l.scope = 'task:' || t.id AND l.expires_at > ?1
            )",
        params![now_ms],
        |r| r.get(0),
    )?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::db::open_memory;
    use crate::storage::repository::{insert_task, TaskRow};

    fn task(id: &str, status: &str, created: i64, protect: Option<i64>) -> TaskRow {
        TaskRow {
            id: id.into(),
            title: id.into(),
            cwd: "/tmp".into(),
            mode: "read".into(),
            status: status.into(),
            session_state: Some("cold".into()),
            recovery_state: Some("none".into()),
            active_recovery_id: None,
            last_turn_id: None,
            acp_session_id: None,
            daemon_instance_id: None,
            supervisor_pid: None,
            supervisor_started_at: None,
            retention_protect_until: protect,
            last_sequence: 0,
            timeline_generation: 1,
            state_revision: 1,
            created_at: created,
            updated_at: created,
            finished_at: Some(created),
        }
    }

    #[test]
    fn does_not_delete_active_or_protected() {
        let conn = open_memory().unwrap();
        insert_task(&conn, &task("old", "idle", 1, None)).unwrap();
        insert_task(&conn, &task("active", "running", 2, None)).unwrap();
        insert_task(
            &conn,
            &task("protected", "idle", 3, Some(9_999_999_999_999)),
        )
        .unwrap();
        let res = run_retention(&conn, 0, 1_000).unwrap();
        assert_eq!(res.deleted_ids, vec!["old".to_string()]);
        assert!(get_exists(&conn, "active"));
        assert!(get_exists(&conn, "protected"));
        assert!(!get_exists(&conn, "old"));
    }

    #[test]
    fn history_limit_keeps_newest() {
        let conn = open_memory().unwrap();
        for i in 0..5 {
            insert_task(&conn, &task(&format!("t{i}"), "idle", i, None)).unwrap();
        }
        let res = run_retention(&conn, 2, 10_000).unwrap();
        assert_eq!(res.deleted_ids.len(), 3);
        assert!(get_exists(&conn, "t3"));
        assert!(get_exists(&conn, "t4"));
    }

    fn get_exists(conn: &Connection, id: &str) -> bool {
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tasks WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap();
        n > 0
    }
}
