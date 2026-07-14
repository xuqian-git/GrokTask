//! Repositories for Task, Turn, recovery, submissions, ui_state, timeline.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum RepoError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Task
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskRow {
    pub id: String,
    pub title: String,
    pub cwd: String,
    pub mode: String,
    pub status: String,
    pub session_state: Option<String>,
    pub recovery_state: Option<String>,
    pub active_recovery_id: Option<String>,
    pub last_turn_id: Option<String>,
    pub acp_session_id: Option<String>,
    pub daemon_instance_id: Option<String>,
    pub supervisor_pid: Option<i64>,
    pub supervisor_started_at: Option<i64>,
    pub retention_protect_until: Option<i64>,
    pub last_sequence: i64,
    pub timeline_generation: i64,
    pub state_revision: i64,
    pub created_at: i64,
    pub updated_at: i64,
    pub finished_at: Option<i64>,
}

pub fn insert_task(conn: &Connection, task: &TaskRow) -> Result<(), RepoError> {
    conn.execute(
        "INSERT INTO tasks (
            id, title, cwd, mode, status, session_state, recovery_state,
            active_recovery_id, last_turn_id, acp_session_id, daemon_instance_id,
            supervisor_pid, supervisor_started_at, retention_protect_until,
            last_sequence, timeline_generation, state_revision,
            created_at, updated_at, finished_at
        ) VALUES (
            ?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20
        )",
        params![
            task.id,
            task.title,
            task.cwd,
            task.mode,
            task.status,
            task.session_state,
            task.recovery_state,
            task.active_recovery_id,
            task.last_turn_id,
            task.acp_session_id,
            task.daemon_instance_id,
            task.supervisor_pid,
            task.supervisor_started_at,
            task.retention_protect_until,
            task.last_sequence,
            task.timeline_generation,
            task.state_revision,
            task.created_at,
            task.updated_at,
            task.finished_at,
        ],
    )?;
    Ok(())
}

pub fn get_task(conn: &Connection, id: &str) -> Result<Option<TaskRow>, RepoError> {
    let row = conn
        .query_row(
            "SELECT id, title, cwd, mode, status, session_state, recovery_state,
                    active_recovery_id, last_turn_id, acp_session_id, daemon_instance_id,
                    supervisor_pid, supervisor_started_at, retention_protect_until,
                    last_sequence, timeline_generation, state_revision,
                    created_at, updated_at, finished_at
             FROM tasks WHERE id = ?1",
            params![id],
            |r| {
                Ok(TaskRow {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    cwd: r.get(2)?,
                    mode: r.get(3)?,
                    status: r.get(4)?,
                    session_state: r.get(5)?,
                    recovery_state: r.get(6)?,
                    active_recovery_id: r.get(7)?,
                    last_turn_id: r.get(8)?,
                    acp_session_id: r.get(9)?,
                    daemon_instance_id: r.get(10)?,
                    supervisor_pid: r.get(11)?,
                    supervisor_started_at: r.get(12)?,
                    retention_protect_until: r.get(13)?,
                    last_sequence: r.get(14)?,
                    timeline_generation: r.get(15)?,
                    state_revision: r.get(16)?,
                    created_at: r.get(17)?,
                    updated_at: r.get(18)?,
                    finished_at: r.get(19)?,
                })
            },
        )
        .optional()?;
    Ok(row)
}

pub fn update_task_status(
    conn: &Connection,
    id: &str,
    status: &str,
    updated_at: i64,
) -> Result<(), RepoError> {
    let n = conn.execute(
        "UPDATE tasks SET status = ?1, updated_at = ?2 WHERE id = ?3",
        params![status, updated_at, id],
    )?;
    if n == 0 {
        return Err(RepoError::NotFound(id.into()));
    }
    Ok(())
}

/// List tasks newest-first.
pub fn list_tasks(conn: &Connection, limit: i64) -> Result<Vec<TaskRow>, RepoError> {
    let mut stmt = conn.prepare(
        "SELECT id, title, cwd, mode, status, session_state, recovery_state,
                active_recovery_id, last_turn_id, acp_session_id, daemon_instance_id,
                supervisor_pid, supervisor_started_at, retention_protect_until,
                last_sequence, timeline_generation, state_revision,
                created_at, updated_at, finished_at
         FROM tasks ORDER BY created_at DESC LIMIT ?1",
    )?;
    let rows = stmt
        .query_map(params![limit], |r| {
            Ok(TaskRow {
                id: r.get(0)?,
                title: r.get(1)?,
                cwd: r.get(2)?,
                mode: r.get(3)?,
                status: r.get(4)?,
                session_state: r.get(5)?,
                recovery_state: r.get(6)?,
                active_recovery_id: r.get(7)?,
                last_turn_id: r.get(8)?,
                acp_session_id: r.get(9)?,
                daemon_instance_id: r.get(10)?,
                supervisor_pid: r.get(11)?,
                supervisor_started_at: r.get(12)?,
                retention_protect_until: r.get(13)?,
                last_sequence: r.get(14)?,
                timeline_generation: r.get(15)?,
                state_revision: r.get(16)?,
                created_at: r.get(17)?,
                updated_at: r.get(18)?,
                finished_at: r.get(19)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Patch common task mirror fields after a turn progresses.
#[allow(clippy::too_many_arguments)]
pub fn update_task_progress(
    conn: &Connection,
    id: &str,
    status: &str,
    last_turn_id: Option<&str>,
    actual_model: Option<&str>,
    stop_reason: Option<&str>,
    error_code: Option<&str>,
    error_message: Option<&str>,
    finished_at: Option<i64>,
    updated_at: i64,
) -> Result<(), RepoError> {
    let n = conn.execute(
        "UPDATE tasks SET
            status = ?1,
            last_turn_id = COALESCE(?2, last_turn_id),
            actual_model = COALESCE(?3, actual_model),
            stop_reason = COALESCE(?4, stop_reason),
            error_code = COALESCE(?5, error_code),
            error_message = COALESCE(?6, error_message),
            finished_at = COALESCE(?7, finished_at),
            updated_at = ?8
         WHERE id = ?9",
        params![
            status,
            last_turn_id,
            actual_model,
            stop_reason,
            error_code,
            error_message,
            finished_at,
            updated_at,
            id
        ],
    )?;
    if n == 0 {
        return Err(RepoError::NotFound(id.into()));
    }
    Ok(())
}

type TaskModelFields = (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

pub fn get_task_models(conn: &Connection, id: &str) -> Result<TaskModelFields, RepoError> {
    let row = conn
        .query_row(
            "SELECT requested_model, actual_model, stop_reason, error_code FROM tasks WHERE id = ?1",
            params![id],
            |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()?;
    Ok(row.unwrap_or((None, None, None, None)))
}

pub fn list_turns_for_task(conn: &Connection, task_id: &str) -> Result<Vec<TurnRow>, RepoError> {
    let mut stmt = conn.prepare(
        "SELECT id, task_id, ordinal, prompt_markdown, status, owner_kind,
                owner_connection_id, owner_request_id, mode, termination_cause,
                answer_markdown, partial, result_json, created_at, started_at, finished_at
         FROM turns WHERE task_id = ?1 ORDER BY ordinal ASC",
    )?;
    let rows = stmt
        .query_map(params![task_id], |r| {
            Ok(TurnRow {
                id: r.get(0)?,
                task_id: r.get(1)?,
                ordinal: r.get(2)?,
                prompt_markdown: r.get(3)?,
                status: r.get(4)?,
                owner_kind: r.get(5)?,
                owner_connection_id: r.get(6)?,
                owner_request_id: r.get(7)?,
                mode: r.get(8)?,
                termination_cause: r.get(9)?,
                answer_markdown: r.get(10)?,
                partial: r.get::<_, i64>(11)? != 0,
                result_json: r.get(12)?,
                created_at: r.get(13)?,
                started_at: r.get(14)?,
                finished_at: r.get(15)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn update_turn_status(
    conn: &Connection,
    turn_id: &str,
    status: &str,
    started_at: Option<i64>,
) -> Result<(), RepoError> {
    let n = conn.execute(
        "UPDATE turns SET status = ?1, started_at = COALESCE(?2, started_at) WHERE id = ?3",
        params![status, started_at, turn_id],
    )?;
    if n == 0 {
        return Err(RepoError::NotFound(turn_id.into()));
    }
    Ok(())
}

pub fn set_turn_termination_cause(
    conn: &Connection,
    turn_id: &str,
    cause: &str,
) -> Result<(), RepoError> {
    conn.execute(
        "UPDATE turns SET termination_cause = ?1 WHERE id = ?2 AND termination_cause IS NULL",
        params![cause, turn_id],
    )?;
    Ok(())
}

/// Insert a redacted raw ACP event for diagnostics.
pub fn insert_raw_event(
    conn: &Connection,
    task_id: &str,
    raw_sequence: i64,
    direction: &str,
    method: Option<&str>,
    payload_json: &str,
) -> Result<(), RepoError> {
    conn.execute(
        "INSERT INTO raw_acp_events (task_id, raw_sequence, direction, method, payload_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            task_id,
            raw_sequence,
            direction,
            method,
            payload_json,
            now_ms()
        ],
    )?;
    Ok(())
}

pub fn set_retention_protect_until(
    conn: &Connection,
    id: &str,
    until: i64,
) -> Result<(), RepoError> {
    conn.execute(
        "UPDATE tasks SET retention_protect_until = ?1, updated_at = ?2 WHERE id = ?3",
        params![until, now_ms(), id],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Turn (immutable after terminal result_json)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TurnRow {
    pub id: String,
    pub task_id: String,
    pub ordinal: i64,
    pub prompt_markdown: String,
    pub status: String,
    pub owner_kind: String,
    pub owner_connection_id: Option<String>,
    pub owner_request_id: Option<String>,
    pub mode: String,
    pub termination_cause: Option<String>,
    pub answer_markdown: String,
    pub partial: bool,
    pub result_json: Option<String>,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
}

pub fn insert_turn(conn: &Connection, turn: &TurnRow) -> Result<(), RepoError> {
    conn.execute(
        "INSERT INTO turns (
            id, task_id, ordinal, prompt_markdown, status, owner_kind,
            owner_connection_id, owner_request_id, mode, termination_cause,
            answer_markdown, partial, result_json, created_at, started_at, finished_at
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
        params![
            turn.id,
            turn.task_id,
            turn.ordinal,
            turn.prompt_markdown,
            turn.status,
            turn.owner_kind,
            turn.owner_connection_id,
            turn.owner_request_id,
            turn.mode,
            turn.termination_cause,
            turn.answer_markdown,
            turn.partial as i64,
            turn.result_json,
            turn.created_at,
            turn.started_at,
            turn.finished_at,
        ],
    )?;
    Ok(())
}

/// Finalize a turn in one transaction: result fields become immutable thereafter.
#[allow(clippy::too_many_arguments)]
pub fn finalize_turn(
    conn: &Connection,
    turn_id: &str,
    status: &str,
    answer: &str,
    termination_cause: Option<&str>,
    partial: bool,
    result: &Value,
    finished_at: i64,
) -> Result<(), RepoError> {
    let result_json = serde_json::to_string(result)?;
    // Refuse to overwrite an already-final result_json.
    let existing: Option<String> = conn
        .query_row(
            "SELECT result_json FROM turns WHERE id = ?1",
            params![turn_id],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    if existing.is_some() {
        return Err(RepoError::Conflict(format!(
            "turn {turn_id} already finalized"
        )));
    }
    let n = conn.execute(
        "UPDATE turns SET status = ?1, answer_markdown = ?2, termination_cause = ?3,
         partial = ?4, result_json = ?5, finished_at = ?6 WHERE id = ?7",
        params![
            status,
            answer,
            termination_cause,
            partial as i64,
            result_json,
            finished_at,
            turn_id
        ],
    )?;
    if n == 0 {
        return Err(RepoError::NotFound(turn_id.into()));
    }
    Ok(())
}

pub fn get_turn(conn: &Connection, id: &str) -> Result<Option<TurnRow>, RepoError> {
    let row = conn
        .query_row(
            "SELECT id, task_id, ordinal, prompt_markdown, status, owner_kind,
                    owner_connection_id, owner_request_id, mode, termination_cause,
                    answer_markdown, partial, result_json, created_at, started_at, finished_at
             FROM turns WHERE id = ?1",
            params![id],
            |r| {
                Ok(TurnRow {
                    id: r.get(0)?,
                    task_id: r.get(1)?,
                    ordinal: r.get(2)?,
                    prompt_markdown: r.get(3)?,
                    status: r.get(4)?,
                    owner_kind: r.get(5)?,
                    owner_connection_id: r.get(6)?,
                    owner_request_id: r.get(7)?,
                    mode: r.get(8)?,
                    termination_cause: r.get(9)?,
                    answer_markdown: r.get(10)?,
                    partial: r.get::<_, i64>(11)? != 0,
                    result_json: r.get(12)?,
                    created_at: r.get(13)?,
                    started_at: r.get(14)?,
                    finished_at: r.get(15)?,
                })
            },
        )
        .optional()?;
    Ok(row)
}

// ---------------------------------------------------------------------------
// Recovery operations
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryRow {
    pub id: String,
    pub task_id: String,
    pub action: String,
    pub input_hash: String,
    pub status: String,
    pub expected_last_turn_id: String,
    pub created_turn_id: Option<String>,
    pub created_at: i64,
}

pub fn insert_recovery(conn: &Connection, row: &RecoveryRow) -> Result<(), RepoError> {
    conn.execute(
        "INSERT INTO recovery_operations (
            id, task_id, action, input_hash, status, expected_last_turn_id,
            created_turn_id, created_at
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        params![
            row.id,
            row.task_id,
            row.action,
            row.input_hash,
            row.status,
            row.expected_last_turn_id,
            row.created_turn_id,
            row.created_at,
        ],
    )?;
    Ok(())
}

pub fn get_recovery(conn: &Connection, id: &str) -> Result<Option<RecoveryRow>, RepoError> {
    let row = conn
        .query_row(
            "SELECT id, task_id, action, input_hash, status, expected_last_turn_id,
                    created_turn_id, created_at
             FROM recovery_operations WHERE id = ?1",
            params![id],
            |r| {
                Ok(RecoveryRow {
                    id: r.get(0)?,
                    task_id: r.get(1)?,
                    action: r.get(2)?,
                    input_hash: r.get(3)?,
                    status: r.get(4)?,
                    expected_last_turn_id: r.get(5)?,
                    created_turn_id: r.get(6)?,
                    created_at: r.get(7)?,
                })
            },
        )
        .optional()?;
    Ok(row)
}

// ---------------------------------------------------------------------------
// Submissions (24h dedupe)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SubmissionRow {
    pub submission_id: String,
    pub input_hash: String,
    pub task_id: String,
    pub turn_id: String,
    pub accepted_result_json: String,
    pub created_at: i64,
    pub expires_at: i64,
}

pub const SUBMISSION_TTL_MS: i64 = 24 * 60 * 60 * 1000;

pub fn insert_submission(conn: &Connection, row: &SubmissionRow) -> Result<(), RepoError> {
    conn.execute(
        "INSERT INTO submissions (
            submission_id, input_hash, task_id, turn_id,
            accepted_result_json, created_at, expires_at
        ) VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![
            row.submission_id,
            row.input_hash,
            row.task_id,
            row.turn_id,
            row.accepted_result_json,
            row.created_at,
            row.expires_at,
        ],
    )?;
    Ok(())
}

pub fn get_submission(
    conn: &Connection,
    submission_id: &str,
) -> Result<Option<SubmissionRow>, RepoError> {
    let row = conn
        .query_row(
            "SELECT submission_id, input_hash, task_id, turn_id,
                    accepted_result_json, created_at, expires_at
             FROM submissions WHERE submission_id = ?1",
            params![submission_id],
            |r| {
                Ok(SubmissionRow {
                    submission_id: r.get(0)?,
                    input_hash: r.get(1)?,
                    task_id: r.get(2)?,
                    turn_id: r.get(3)?,
                    accepted_result_json: r.get(4)?,
                    created_at: r.get(5)?,
                    expires_at: r.get(6)?,
                })
            },
        )
        .optional()?;
    Ok(row)
}

/// Accept start: create task+turn+submission in one transaction.
pub fn accept_start_submission(
    conn: &Connection,
    task: &TaskRow,
    turn: &TurnRow,
    submission: &SubmissionRow,
) -> Result<(), RepoError> {
    let tx = conn.unchecked_transaction()?;
    insert_task(&tx, task)?;
    insert_turn(&tx, turn)?;
    insert_submission(&tx, submission)?;
    tx.commit()?;
    Ok(())
}

pub fn purge_expired_submissions(conn: &Connection, now: i64) -> Result<usize, RepoError> {
    let n = conn.execute(
        "DELETE FROM submissions WHERE expires_at < ?1",
        params![now],
    )?;
    Ok(n)
}

// ---------------------------------------------------------------------------
// Timeline + mutations (transaction-before-broadcast contract)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TimelineItemRow {
    pub task_id: String,
    pub item_id: String,
    pub turn_id: Option<String>,
    pub kind: String,
    pub first_sequence: i64,
    pub last_sequence: i64,
    pub payload_json: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MutationRow {
    pub task_id: String,
    pub sequence: i64,
    pub generation: i64,
    pub operation: String,
    pub item_id: Option<String>,
    pub payload_json: String,
    pub created_at: i64,
}

/// Apply mutations + item upserts + advance last_sequence inside one transaction.
/// Caller broadcasts only after this returns Ok.
pub fn commit_timeline_mutations(
    conn: &Connection,
    task_id: &str,
    items: &[TimelineItemRow],
    mutations: &[MutationRow],
    new_last_sequence: i64,
) -> Result<(), RepoError> {
    let tx = conn.unchecked_transaction()?;
    for item in items {
        tx.execute(
            "INSERT INTO timeline_items (
                task_id, item_id, turn_id, kind, first_sequence, last_sequence,
                payload_json, created_at, updated_at
            ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
            ON CONFLICT(task_id, item_id) DO UPDATE SET
                turn_id = excluded.turn_id,
                kind = excluded.kind,
                last_sequence = excluded.last_sequence,
                payload_json = excluded.payload_json,
                updated_at = excluded.updated_at",
            params![
                item.task_id,
                item.item_id,
                item.turn_id,
                item.kind,
                item.first_sequence,
                item.last_sequence,
                item.payload_json,
                item.created_at,
                item.updated_at,
            ],
        )?;
    }
    for m in mutations {
        tx.execute(
            "INSERT INTO timeline_mutations (
                task_id, sequence, generation, operation, item_id, payload_json, created_at
            ) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                m.task_id,
                m.sequence,
                m.generation,
                m.operation,
                m.item_id,
                m.payload_json,
                m.created_at,
            ],
        )?;
    }
    tx.execute(
        "UPDATE tasks SET last_sequence = ?1, updated_at = ?2 WHERE id = ?3",
        params![new_last_sequence, now_ms(), task_id],
    )?;
    tx.commit()?;
    Ok(())
}

pub fn list_timeline_items(
    conn: &Connection,
    task_id: &str,
) -> Result<Vec<TimelineItemRow>, RepoError> {
    let mut stmt = conn.prepare(
        "SELECT task_id, item_id, turn_id, kind, first_sequence, last_sequence,
                payload_json, created_at, updated_at
         FROM timeline_items WHERE task_id = ?1 ORDER BY first_sequence ASC",
    )?;
    let rows = stmt
        .query_map(params![task_id], |r| {
            Ok(TimelineItemRow {
                task_id: r.get(0)?,
                item_id: r.get(1)?,
                turn_id: r.get(2)?,
                kind: r.get(3)?,
                first_sequence: r.get(4)?,
                last_sequence: r.get(5)?,
                payload_json: r.get(6)?,
                created_at: r.get(7)?,
                updated_at: r.get(8)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn list_mutations_after(
    conn: &Connection,
    task_id: &str,
    generation: i64,
    after_sequence: i64,
) -> Result<Vec<MutationRow>, RepoError> {
    let mut stmt = conn.prepare(
        "SELECT task_id, sequence, generation, operation, item_id, payload_json, created_at
         FROM timeline_mutations
         WHERE task_id = ?1 AND generation = ?2 AND sequence > ?3
         ORDER BY sequence ASC",
    )?;
    let rows = stmt
        .query_map(params![task_id, generation, after_sequence], |r| {
            Ok(MutationRow {
                task_id: r.get(0)?,
                sequence: r.get(1)?,
                generation: r.get(2)?,
                operation: r.get(3)?,
                item_id: r.get(4)?,
                payload_json: r.get(5)?,
                created_at: r.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

// ---------------------------------------------------------------------------
// UI state generation / revision
// ---------------------------------------------------------------------------

pub fn ui_state_generation(conn: &Connection) -> Result<String, RepoError> {
    let v: String = conn.query_row(
        "SELECT value FROM meta_kv WHERE key = 'ui_state_generation'",
        [],
        |r| r.get(0),
    )?;
    Ok(v)
}

pub fn ui_state_revision(conn: &Connection) -> Result<i64, RepoError> {
    let v: String = conn.query_row(
        "SELECT value FROM meta_kv WHERE key = 'ui_state_revision'",
        [],
        |r| r.get(0),
    )?;
    Ok(v.parse().unwrap_or(0))
}

/// Upsert disclosure expansion; returns new global revision.
pub fn ui_state_set(
    conn: &Connection,
    task_id: &str,
    disclosure_key: &str,
    expansion: &str,
) -> Result<i64, RepoError> {
    let tx = conn.unchecked_transaction()?;
    let rev: i64 = {
        let cur: String = tx.query_row(
            "SELECT value FROM meta_kv WHERE key = 'ui_state_revision'",
            [],
            |r| r.get(0),
        )?;
        cur.parse().unwrap_or(0) + 1
    };
    tx.execute(
        "UPDATE meta_kv SET value = ?1 WHERE key = 'ui_state_revision'",
        params![rev.to_string()],
    )?;
    if expansion == "auto" {
        tx.execute(
            "DELETE FROM ui_state WHERE task_id = ?1 AND disclosure_key = ?2",
            params![task_id, disclosure_key],
        )?;
    } else {
        tx.execute(
            "INSERT INTO ui_state (task_id, disclosure_key, expansion, revision, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(task_id, disclosure_key) DO UPDATE SET
                expansion = excluded.expansion,
                revision = excluded.revision,
                updated_at = excluded.updated_at",
            params![task_id, disclosure_key, expansion, rev, now_ms()],
        )?;
    }
    tx.commit()?;
    Ok(rev)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UiStateRow {
    pub task_id: String,
    pub disclosure_key: String,
    pub expansion: String,
    pub revision: i64,
    pub updated_at: i64,
}

pub fn list_ui_state(conn: &Connection, task_id: &str) -> Result<Vec<UiStateRow>, RepoError> {
    let mut stmt = conn.prepare(
        "SELECT task_id, disclosure_key, expansion, revision, updated_at
         FROM ui_state WHERE task_id = ?1",
    )?;
    let rows = stmt
        .query_map(params![task_id], |r| {
            Ok(UiStateRow {
                task_id: r.get(0)?,
                disclosure_key: r.get(1)?,
                expansion: r.get(2)?,
                revision: r.get(3)?,
                updated_at: r.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Atomically bump timeline generation and append a reset mutation.
pub fn timeline_generation_reset(
    conn: &Connection,
    task_id: &str,
) -> Result<(i64, i64), RepoError> {
    let tx = conn.unchecked_transaction()?;
    let (gen, seq): (i64, i64) = tx.query_row(
        "SELECT timeline_generation, last_sequence FROM tasks WHERE id = ?1",
        params![task_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let new_gen = gen + 1;
    let new_seq = seq + 1;
    tx.execute(
        "UPDATE tasks SET timeline_generation = ?1, last_sequence = ?2, updated_at = ?3 WHERE id = ?4",
        params![new_gen, new_seq, now_ms(), task_id],
    )?;
    tx.execute(
        "INSERT INTO timeline_mutations (
            task_id, sequence, generation, operation, item_id, payload_json, created_at
        ) VALUES (?1, ?2, ?3, 'reset', NULL, '{}', ?4)",
        params![task_id, new_seq, new_gen, now_ms()],
    )?;
    tx.commit()?;
    Ok((new_gen, new_seq))
}

pub fn new_id() -> String {
    Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::db::open_memory;

    fn sample_task(id: &str) -> TaskRow {
        let t = now_ms();
        TaskRow {
            id: id.into(),
            title: "t".into(),
            cwd: "/tmp".into(),
            mode: "read".into(),
            status: "idle".into(),
            session_state: Some("cold".into()),
            recovery_state: Some("none".into()),
            active_recovery_id: None,
            last_turn_id: None,
            acp_session_id: None,
            daemon_instance_id: None,
            supervisor_pid: None,
            supervisor_started_at: None,
            retention_protect_until: None,
            last_sequence: 0,
            timeline_generation: 1,
            state_revision: 1,
            created_at: t,
            updated_at: t,
            finished_at: None,
        }
    }

    #[test]
    fn turn_finalize_is_immutable() {
        let conn = open_memory().unwrap();
        insert_task(&conn, &sample_task("task1")).unwrap();
        let turn = TurnRow {
            id: "turn1".into(),
            task_id: "task1".into(),
            ordinal: 1,
            prompt_markdown: "hi".into(),
            status: "running".into(),
            owner_kind: "daemon".into(),
            owner_connection_id: None,
            owner_request_id: None,
            mode: "read".into(),
            termination_cause: None,
            answer_markdown: String::new(),
            partial: false,
            result_json: None,
            created_at: now_ms(),
            started_at: Some(now_ms()),
            finished_at: None,
        };
        insert_turn(&conn, &turn).unwrap();
        let result = serde_json::json!({"status": "completed"});
        finalize_turn(
            &conn,
            "turn1",
            "completed",
            "ok",
            None,
            false,
            &result,
            now_ms(),
        )
        .unwrap();
        let err = finalize_turn(
            &conn,
            "turn1",
            "failed",
            "nope",
            Some("user_cancel"),
            false,
            &result,
            now_ms(),
        );
        assert!(matches!(err, Err(RepoError::Conflict(_))));
        let got = get_turn(&conn, "turn1").unwrap().unwrap();
        assert_eq!(got.status, "completed");
        assert_eq!(got.answer_markdown, "ok");
    }

    #[test]
    fn list_turns_for_task_maps_columns_correctly() {
        let conn = open_memory().unwrap();
        insert_task(&conn, &sample_task("task-list")).unwrap();
        let created = 1_700_000_000_100_i64;
        let started = 1_700_000_000_200_i64;
        let finished = 1_700_000_000_300_i64;
        let turn = TurnRow {
            id: "turn-map".into(),
            task_id: "task-list".into(),
            ordinal: 1,
            prompt_markdown: "prompt body".into(),
            status: "running".into(),
            owner_kind: "daemon".into(),
            owner_connection_id: Some("conn-1".into()),
            owner_request_id: Some("req-9".into()),
            mode: "write".into(),
            termination_cause: None,
            answer_markdown: String::new(),
            partial: false,
            result_json: None,
            created_at: created,
            started_at: Some(started),
            finished_at: None,
        };
        insert_turn(&conn, &turn).unwrap();
        let result = serde_json::json!({"status": "completed", "tokens": 42});
        finalize_turn(
            &conn,
            "turn-map",
            "completed",
            "## final answer",
            Some("natural"),
            true,
            &result,
            finished,
        )
        .unwrap();

        let listed = list_turns_for_task(&conn, "task-list").unwrap();
        assert_eq!(listed.len(), 1);
        let got = &listed[0];
        assert_eq!(got.id, "turn-map");
        assert_eq!(got.task_id, "task-list");
        assert_eq!(got.ordinal, 1);
        assert_eq!(got.prompt_markdown, "prompt body");
        assert_eq!(got.status, "completed");
        assert_eq!(got.owner_kind, "daemon");
        assert_eq!(got.owner_connection_id.as_deref(), Some("conn-1"));
        assert_eq!(got.owner_request_id.as_deref(), Some("req-9"));
        assert_eq!(got.mode, "write");
        assert_eq!(got.termination_cause.as_deref(), Some("natural"));
        assert_eq!(got.answer_markdown, "## final answer");
        assert!(got.partial);
        assert_eq!(
            got.result_json.as_deref(),
            Some(r#"{"status":"completed","tokens":42}"#)
        );
        assert_eq!(got.created_at, created);
        assert_eq!(got.started_at, Some(started));
        assert_eq!(got.finished_at, Some(finished));
    }

    #[test]
    fn submission_dedupe_same_transaction() {
        let conn = open_memory().unwrap();
        let now = now_ms();
        let task = sample_task("t1");
        let turn = TurnRow {
            id: "tu1".into(),
            task_id: "t1".into(),
            ordinal: 1,
            prompt_markdown: "p".into(),
            status: "queued".into(),
            owner_kind: "daemon".into(),
            owner_connection_id: None,
            owner_request_id: None,
            mode: "read".into(),
            termination_cause: None,
            answer_markdown: String::new(),
            partial: false,
            result_json: None,
            created_at: now,
            started_at: None,
            finished_at: None,
        };
        let sub = SubmissionRow {
            submission_id: "sub1".into(),
            input_hash: "abc".into(),
            task_id: "t1".into(),
            turn_id: "tu1".into(),
            accepted_result_json: r#"{"taskId":"t1"}"#.into(),
            created_at: now,
            expires_at: now + SUBMISSION_TTL_MS,
        };
        accept_start_submission(&conn, &task, &turn, &sub).unwrap();
        let got = get_submission(&conn, "sub1").unwrap().unwrap();
        assert_eq!(got.input_hash, "abc");
        // Expired purge
        let n = purge_expired_submissions(&conn, now + SUBMISSION_TTL_MS + 1).unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn ui_state_revision_monotonic() {
        let conn = open_memory().unwrap();
        insert_task(&conn, &sample_task("t1")).unwrap();
        let r1 = ui_state_set(&conn, "t1", "item:1:details", "user-expanded").unwrap();
        let r2 = ui_state_set(&conn, "t1", "item:2:details", "user-collapsed").unwrap();
        assert!(r2 > r1);
        assert_eq!(ui_state_revision(&conn).unwrap(), r2);
        assert!(!ui_state_generation(&conn).unwrap().is_empty());
    }

    #[test]
    fn commit_then_sequence_visible() {
        let conn = open_memory().unwrap();
        insert_task(&conn, &sample_task("t1")).unwrap();
        let item = TimelineItemRow {
            task_id: "t1".into(),
            item_id: "i1".into(),
            turn_id: None,
            kind: "assistant_segment".into(),
            first_sequence: 1,
            last_sequence: 1,
            payload_json: r#"{"text":"hi"}"#.into(),
            created_at: now_ms(),
            updated_at: now_ms(),
        };
        let m = MutationRow {
            task_id: "t1".into(),
            sequence: 1,
            generation: 1,
            operation: "add".into(),
            item_id: Some("i1".into()),
            payload_json: r#"{"text":"hi"}"#.into(),
            created_at: now_ms(),
        };
        commit_timeline_mutations(&conn, "t1", &[item], &[m], 1).unwrap();
        let task = get_task(&conn, "t1").unwrap().unwrap();
        assert_eq!(task.last_sequence, 1);
        assert_eq!(list_timeline_items(&conn, "t1").unwrap().len(), 1);
    }
}
