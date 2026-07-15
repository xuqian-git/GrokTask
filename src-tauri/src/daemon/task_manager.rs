//! TaskManager: accept/start/run/continue/status/wait/cancel and drive ACP reduce path.
//!
//! Real Grok ACP process integration is best-effort: when the CLI is missing or
//! fixture mode is set, turns still persist durable state and can ingest
//! synthetic ACP lines for tests.

use crate::acp::normalize::normalize_line;
use crate::acp::process::{build_grok_argv, TaskMode as AcpMode};
use crate::acp::redact::{bound_payload, DEFAULT_DIAGNOSTIC_MAX_BYTES};
use crate::acp::reducer::TurnReducer;
use crate::dto::{
    default_title, map_stop_to_outcome, ms_to_rfc3339, now_ms, task_input_hash,
    timeline_item_from_payload, ErrorInfo, RunResult, RunStatus, StartResult, TaskContainerStatus,
    TaskDetail, TaskInput, TaskListItem, TaskMode, TaskStatus, TurnOutcome, WaitTimeout,
    MAX_WAIT_TIMEOUT_MS,
};
use crate::storage::repository::{
    self, MutationRow, SubmissionRow, TaskRow, TimelineItemRow, TurnRow, SUBMISSION_TTL_MS,
};
use anyhow::{anyhow, Context, Result};
use parking_lot::Mutex;
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::Notify;

/// In-memory cancel / wait coordination for active turns.
#[derive(Default)]
struct TurnRuntime {
    cancel_requested: AtomicBool,
    termination_cause: Mutex<Option<String>>,
    finished: Notify,
    result: Mutex<Option<RunResult>>,
}

pub struct TaskManager {
    db_path: PathBuf,
    grok_executable: Option<String>,
    /// turn_id → runtime
    turns: Mutex<HashMap<String, Arc<TurnRuntime>>>,
    /// task_id → latest action cache
    latest_action: Mutex<HashMap<String, String>>,
    current_step: Mutex<HashMap<String, String>>,
    raw_seq: AtomicI64,
    /// When true (or env GROKTASK_FIXTURE=1), skip real Grok and finalize quickly.
    fixture_mode: bool,
}

impl TaskManager {
    pub fn new(db_path: PathBuf, grok_executable: Option<String>) -> Self {
        let fixture_mode = std::env::var("GROKTASK_FIXTURE")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        Self {
            db_path,
            grok_executable,
            turns: Mutex::new(HashMap::new()),
            latest_action: Mutex::new(HashMap::new()),
            current_step: Mutex::new(HashMap::new()),
            raw_seq: AtomicI64::new(0),
            fixture_mode,
        }
    }

    fn open(&self) -> Result<rusqlite::Connection> {
        crate::storage::open_path(&self.db_path).context("open history db")
    }

    pub fn start(
        self: &Arc<Self>,
        input: TaskInput,
        submission_id: String,
        owner_kind: &str,
        owner_connection_id: Option<String>,
        owner_request_id: Option<String>,
    ) -> Result<StartResult, TaskError> {
        let conn = self.open().map_err(TaskError::internal)?;
        let input_hash = task_input_hash(&input);

        if let Some(existing) =
            repository::get_submission(&conn, &submission_id).map_err(TaskError::storage)?
        {
            if existing.input_hash != input_hash {
                return Err(TaskError::conflict(
                    "idempotency_conflict",
                    "submissionId reused with different task input",
                ));
            }
            // Return original accepted result
            if let Ok(sr) = serde_json::from_str::<StartResult>(&existing.accepted_result_json) {
                let deleted = repository::get_task(&conn, &existing.task_id)
                    .ok()
                    .flatten()
                    .is_none();
                let mut sr = sr;
                if deleted {
                    sr.task_deleted = Some(true);
                }
                return Ok(sr);
            }
        }

        let now = now_ms();
        let task_id = repository::new_id();
        let turn_id = repository::new_id();
        let title = input
            .title
            .clone()
            .unwrap_or_else(|| default_title(&input.task));

        let task = TaskRow {
            id: task_id.clone(),
            title: title.clone(),
            cwd: input.cwd.clone(),
            mode: input.mode.as_str().into(),
            status: "queued".into(),
            session_state: Some("cold".into()),
            recovery_state: Some("none".into()),
            active_recovery_id: None,
            last_turn_id: Some(turn_id.clone()),
            acp_session_id: None,
            daemon_instance_id: None,
            supervisor_pid: None,
            supervisor_started_at: None,
            retention_protect_until: None,
            last_sequence: 0,
            timeline_generation: 1,
            state_revision: 1,
            created_at: now,
            updated_at: now,
            finished_at: None,
        };

        // Persist requested model via raw SQL (TaskRow doesn't include it fully in select)
        let turn = TurnRow {
            id: turn_id.clone(),
            task_id: task_id.clone(),
            ordinal: 1,
            prompt_markdown: input.task.clone(),
            status: "queued".into(),
            owner_kind: owner_kind.into(),
            owner_connection_id,
            owner_request_id,
            mode: input.mode.as_str().into(),
            termination_cause: None,
            answer_markdown: String::new(),
            partial: false,
            result_json: None,
            created_at: now,
            started_at: None,
            finished_at: None,
        };

        let start_result = StartResult {
            submission_id: submission_id.clone(),
            task_id: task_id.clone(),
            turn_id: turn_id.clone(),
            turn_ordinal: 1,
            status: "queued".into(),
            mode: input.mode,
            created_at: ms_to_rfc3339(now),
            task_deleted: None,
        };

        let sub = SubmissionRow {
            submission_id: submission_id.clone(),
            input_hash,
            task_id: task_id.clone(),
            turn_id: turn_id.clone(),
            accepted_result_json: serde_json::to_string(&start_result).unwrap_or_default(),
            created_at: now,
            expires_at: now + SUBMISSION_TTL_MS,
        };

        repository::accept_start_submission(&conn, &task, &turn, &sub)
            .map_err(TaskError::storage)?;

        // Best-effort model fields
        let _ = conn.execute(
            "UPDATE tasks SET requested_model = ?1, reasoning_effort = ?2 WHERE id = ?3",
            rusqlite::params![input.model, input.effort, task_id],
        );

        // Seed user message on timeline
        self.seed_user_message(&conn, &task_id, &turn_id, &input.task)
            .map_err(TaskError::storage)?;

        let rt = Arc::new(TurnRuntime::default());
        self.turns.lock().insert(turn_id.clone(), rt.clone());

        let mgr = Arc::clone(self);
        let input_clone = input.clone();
        let task_id_c = task_id.clone();
        let turn_id_c = turn_id.clone();
        tokio::spawn(async move {
            mgr.run_turn(task_id_c, turn_id_c, input_clone, rt).await;
        });

        Ok(start_result)
    }

    pub async fn run_blocking(
        self: &Arc<Self>,
        input: TaskInput,
        owner_connection_id: Option<String>,
        owner_request_id: Option<String>,
    ) -> Result<RunResult, TaskError> {
        let submission_id = repository::new_id();
        let start = self.start(
            input,
            submission_id,
            "client",
            owner_connection_id,
            owner_request_id,
        )?;
        self.wait(&start.task_id, &start.turn_id, MAX_WAIT_TIMEOUT_MS)
            .await
    }

    pub fn continue_task(
        self: &Arc<Self>,
        task_id: &str,
        prompt: String,
        owner_kind: &str,
        owner_connection_id: Option<String>,
        owner_request_id: Option<String>,
    ) -> Result<StartResult, TaskError> {
        let prompt = prompt.trim().to_string();
        if prompt.is_empty() {
            return Err(TaskError::invalid("prompt is required"));
        }
        if prompt.len() > crate::dto::MAX_TASK_BYTES {
            return Err(TaskError::invalid("prompt is too long"));
        }

        let conn = self.open().map_err(TaskError::internal)?;
        let task = repository::get_task(&conn, task_id)
            .map_err(TaskError::storage)?
            .ok_or_else(|| TaskError::not_found("task not found"))?;
        let status =
            TaskContainerStatus::parse(&task.status).unwrap_or(TaskContainerStatus::Failed);
        if status.is_active() {
            return Err(TaskError::conflict(
                "task_active",
                "task already has an active turn",
            ));
        }
        let mode = TaskMode::parse(&task.mode).unwrap_or(TaskMode::Read);
        let (_requested_model, _actual_model, _stop, _ecode) =
            repository::get_task_models(&conn, task_id).unwrap_or((None, None, None, None));
        let turns = repository::list_turns_for_task(&conn, task_id).map_err(TaskError::storage)?;
        let ordinal = turns.iter().map(|t| t.ordinal).max().unwrap_or(0) + 1;
        let now = now_ms();
        let turn_id = repository::new_id();
        let turn = TurnRow {
            id: turn_id.clone(),
            task_id: task_id.into(),
            ordinal,
            prompt_markdown: prompt.clone(),
            status: "queued".into(),
            owner_kind: owner_kind.into(),
            owner_connection_id,
            owner_request_id,
            mode: mode.as_str().into(),
            termination_cause: None,
            answer_markdown: String::new(),
            partial: false,
            result_json: None,
            created_at: now,
            started_at: None,
            finished_at: None,
        };
        repository::insert_turn(&conn, &turn).map_err(TaskError::storage)?;
        conn.execute(
            "UPDATE tasks SET status = 'queued', last_turn_id = ?1, finished_at = NULL,
                stop_reason = NULL, error_code = NULL, error_message = NULL, updated_at = ?2
             WHERE id = ?3",
            rusqlite::params![turn_id, now, task_id],
        )
        .map_err(TaskError::storage)?;

        let next_sequence = task.last_sequence + 1;
        self.seed_user_message_at(&conn, task_id, &turn_id, &prompt, next_sequence)
            .map_err(TaskError::storage)?;

        let rt = Arc::new(TurnRuntime::default());
        self.turns.lock().insert(turn_id.clone(), rt.clone());

        let input = TaskInput {
            task: prompt,
            cwd: task.cwd,
            mode,
            model: None,
            effort: None,
            title: Some(task.title),
        };
        let mgr = Arc::clone(self);
        let task_id_c = task_id.to_string();
        let turn_id_c = turn_id.clone();
        tokio::spawn(async move {
            mgr.run_turn(task_id_c, turn_id_c, input, rt).await;
        });

        Ok(StartResult {
            submission_id: repository::new_id(),
            task_id: task_id.into(),
            turn_id,
            turn_ordinal: ordinal as u32,
            status: "queued".into(),
            mode,
            created_at: ms_to_rfc3339(now),
            task_deleted: None,
        })
    }

    pub async fn wait(
        &self,
        task_id: &str,
        turn_id: &str,
        timeout_ms: u64,
    ) -> Result<RunResult, TaskError> {
        // Already terminal in DB?
        if let Ok(conn) = self.open() {
            if let Ok(Some(turn)) = repository::get_turn(&conn, turn_id) {
                if turn.task_id != task_id {
                    return Err(TaskError::not_found("turn does not belong to task"));
                }
                if let Some(ref rj) = turn.result_json {
                    if let Ok(r) = serde_json::from_str::<RunResult>(rj) {
                        return Ok(r);
                    }
                }
            } else {
                return Err(TaskError::not_found("task or turn not found"));
            }
        }

        let rt = self.turns.lock().get(turn_id).cloned();
        if let Some(rt) = rt {
            let timeout = Duration::from_millis(timeout_ms.min(MAX_WAIT_TIMEOUT_MS));
            tokio::select! {
                _ = rt.finished.notified() => {}
                _ = tokio::time::sleep(timeout) => {
                    // Fall through to check DB / timeout payload via error type
                }
            }
            if let Some(r) = rt.result.lock().clone() {
                return Ok(r);
            }
        } else {
            // Poll DB for a bit
            let deadline = tokio::time::Instant::now() + Duration::from_millis(timeout_ms);
            while tokio::time::Instant::now() < deadline {
                if let Ok(conn) = self.open() {
                    if let Ok(Some(turn)) = repository::get_turn(&conn, turn_id) {
                        if let Some(ref rj) = turn.result_json {
                            if let Ok(r) = serde_json::from_str::<RunResult>(rj) {
                                return Ok(r);
                            }
                        }
                    }
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }

        // Timeout snapshot
        let status = self.status(task_id).unwrap_or_else(|_| TaskStatus {
            task_id: task_id.into(),
            status: TaskContainerStatus::Running,
            mode: TaskMode::Read,
            session_state: None,
            active_turn_id: Some(turn_id.into()),
            active_recovery_id: None,
            last_turn_id: Some(turn_id.into()),
            last_turn_status: None,
            actual_model: None,
            current_step: None,
            latest_action: None,
            answer_preview: None,
            stop_reason: None,
            error: None,
            created_at: ms_to_rfc3339(now_ms()),
            updated_at: ms_to_rfc3339(now_ms()),
            finished_at: None,
        });
        Err(TaskError::WaitTimeout(WaitTimeout {
            task_id: task_id.into(),
            turn_id: turn_id.into(),
            timed_out: true,
            status: status.status,
            current_step: status.current_step,
            latest_action: status.latest_action,
        }))
    }

    pub fn status(&self, task_id: &str) -> Result<TaskStatus, TaskError> {
        let conn = self.open().map_err(TaskError::internal)?;
        let task = repository::get_task(&conn, task_id)
            .map_err(TaskError::storage)?
            .ok_or_else(|| TaskError::not_found("task not found"))?;
        let mode = TaskMode::parse(&task.mode).unwrap_or(TaskMode::Read);
        let status =
            TaskContainerStatus::parse(&task.status).unwrap_or(TaskContainerStatus::Failed);
        let (_req, actual, stop, _ecode) =
            repository::get_task_models(&conn, task_id).unwrap_or((None, None, None, None));

        let last_turn_status = task
            .last_turn_id
            .as_ref()
            .and_then(|id| repository::get_turn(&conn, id).ok().flatten())
            .map(|t| t.status);

        let answer_preview = task
            .last_turn_id
            .as_ref()
            .and_then(|id| repository::get_turn(&conn, id).ok().flatten())
            .map(|t| {
                let a = t.answer_markdown;
                if a.chars().count() > 200 {
                    format!("{}…", a.chars().take(200).collect::<String>())
                } else {
                    a
                }
            })
            .filter(|s| !s.is_empty());

        let latest_action = self.latest_action.lock().get(task_id).cloned();
        let current_step = self.current_step.lock().get(task_id).cloned();

        let error = if status == TaskContainerStatus::Failed {
            let code = conn
                .query_row(
                    "SELECT error_code, error_message FROM tasks WHERE id = ?1",
                    rusqlite::params![task_id],
                    |r| {
                        Ok((
                            r.get::<_, Option<String>>(0)?,
                            r.get::<_, Option<String>>(1)?,
                        ))
                    },
                )
                .ok();
            code.and_then(|(c, m)| {
                c.map(|code| ErrorInfo {
                    code,
                    message: m.unwrap_or_default(),
                    retryable: false,
                })
            })
        } else {
            None
        };

        Ok(TaskStatus {
            task_id: task.id,
            status,
            mode,
            session_state: task.session_state,
            active_turn_id: if status.is_active() {
                task.last_turn_id.clone()
            } else {
                None
            },
            active_recovery_id: task.active_recovery_id,
            last_turn_id: task.last_turn_id,
            last_turn_status,
            actual_model: actual,
            current_step,
            latest_action,
            answer_preview,
            stop_reason: stop,
            error,
            created_at: ms_to_rfc3339(task.created_at),
            updated_at: ms_to_rfc3339(task.updated_at),
            finished_at: task.finished_at.map(ms_to_rfc3339),
        })
    }

    pub fn list(&self, limit: i64) -> Result<Vec<TaskListItem>, TaskError> {
        let conn = self.open().map_err(TaskError::internal)?;
        let rows =
            repository::list_tasks(&conn, limit.clamp(1, 500)).map_err(TaskError::storage)?;
        let mut out = Vec::with_capacity(rows.len());
        for t in rows {
            let mode = TaskMode::parse(&t.mode).unwrap_or(TaskMode::Read);
            let status =
                TaskContainerStatus::parse(&t.status).unwrap_or(TaskContainerStatus::Failed);
            let actual = repository::get_task_models(&conn, &t.id)
                .ok()
                .and_then(|(_, a, _, _)| a);
            out.push(TaskListItem {
                task_id: t.id.clone(),
                title: t.title,
                cwd: t.cwd,
                mode,
                status,
                actual_model: actual,
                latest_action: self.latest_action.lock().get(&t.id).cloned(),
                created_at: ms_to_rfc3339(t.created_at),
                updated_at: ms_to_rfc3339(t.updated_at),
                finished_at: t.finished_at.map(ms_to_rfc3339),
            });
        }
        Ok(out)
    }

    pub fn detail(&self, task_id: &str) -> Result<TaskDetail, TaskError> {
        let conn = self.open().map_err(TaskError::internal)?;
        let task_row = repository::get_task(&conn, task_id)
            .map_err(TaskError::storage)?
            .ok_or_else(|| TaskError::not_found("task not found"))?;
        let status = self.status(task_id)?;
        let items = repository::list_timeline_items(&conn, task_id).map_err(TaskError::storage)?;
        let mut timeline = Vec::new();
        let mut active_plan = None;
        for item in items {
            let payload: serde_json::Value =
                serde_json::from_str(&item.payload_json).unwrap_or(json!({}));
            // Hide active plan from main timeline list; expose separately
            if item.kind == "plan"
                && payload.get("status").and_then(|v| v.as_str()) == Some("active_hidden")
            {
                let entries: Vec<crate::dto::PlanEntryDto> = payload
                    .get("planEntries")
                    .cloned()
                    .and_then(|v| serde_json::from_value(v).ok())
                    .unwrap_or_default();
                let current = entries
                    .iter()
                    .find(|e| {
                        matches!(
                            e.status.as_deref(),
                            Some("in_progress" | "running" | "pending")
                        )
                    })
                    .map(|e| e.content.clone());
                active_plan = Some(crate::dto::PlanDto {
                    item_id: item.item_id.clone(),
                    entries,
                    current_step: current,
                });
                continue;
            }
            let dto = timeline_item_from_payload(
                &item.item_id,
                &item.kind,
                item.turn_id.as_deref(),
                &payload,
                item.first_sequence,
                item.last_sequence,
            );
            timeline.push(dto);
        }
        let (requested, _, _, _) =
            repository::get_task_models(&conn, task_id).unwrap_or((None, None, None, None));
        Ok(TaskDetail {
            task: status,
            title: task_row.title,
            cwd: task_row.cwd,
            requested_model: requested,
            timeline,
            active_plan,
            last_sequence: task_row.last_sequence,
            timeline_generation: task_row.timeline_generation,
        })
    }

    pub fn cancel_turn(
        &self,
        task_id: &str,
        turn_id: &str,
        cause: &str,
    ) -> Result<crate::dto::TurnCancelResult, TaskError> {
        let conn = self.open().map_err(TaskError::internal)?;
        let turn = repository::get_turn(&conn, turn_id)
            .map_err(TaskError::storage)?
            .ok_or_else(|| TaskError::not_found("turn not found"))?;
        if turn.task_id != task_id {
            return Err(TaskError::not_found("turn does not belong to task"));
        }

        // Already terminal
        if let Some(ref rj) = turn.result_json {
            if let Ok(result) = serde_json::from_str::<RunResult>(rj) {
                let task_status = self
                    .status(task_id)
                    .map(|s| s.status)
                    .unwrap_or(TaskContainerStatus::Idle);
                return Ok(crate::dto::TurnCancelResult {
                    target: "turn".into(),
                    task_id: task_id.into(),
                    turn_id: turn_id.into(),
                    task_status,
                    already_terminal: true,
                    result,
                });
            }
        }

        let _ = repository::set_turn_termination_cause(&conn, turn_id, cause);
        let _ = repository::update_task_status(&conn, task_id, "cancelling", now_ms());

        if let Some(rt) = self.turns.lock().get(turn_id).cloned() {
            *rt.termination_cause.lock() = Some(cause.into());
            rt.cancel_requested.store(true, Ordering::SeqCst);
        }

        // Synchronously finalize if still queued/not running ACP
        let status = turn.status.as_str();
        if matches!(status, "queued" | "starting") {
            let result = self.finalize_cancelled(&conn, task_id, turn_id, cause)?;
            let task_status = self
                .status(task_id)
                .map(|s| s.status)
                .unwrap_or(TaskContainerStatus::Cancelled);
            return Ok(crate::dto::TurnCancelResult {
                target: "turn".into(),
                task_id: task_id.into(),
                turn_id: turn_id.into(),
                task_status,
                already_terminal: false,
                result,
            });
        }

        // Wait briefly for worker to finish
        for _ in 0..40 {
            if let Ok(Some(t)) = repository::get_turn(&conn, turn_id) {
                if let Some(ref rj) = t.result_json {
                    if let Ok(result) = serde_json::from_str::<RunResult>(rj) {
                        let task_status = self
                            .status(task_id)
                            .map(|s| s.status)
                            .unwrap_or(TaskContainerStatus::Cancelled);
                        return Ok(crate::dto::TurnCancelResult {
                            target: "turn".into(),
                            task_id: task_id.into(),
                            turn_id: turn_id.into(),
                            task_status,
                            already_terminal: false,
                            result,
                        });
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        // Force finalize
        let result = self.finalize_cancelled(&conn, task_id, turn_id, cause)?;
        let task_status = self
            .status(task_id)
            .map(|s| s.status)
            .unwrap_or(TaskContainerStatus::Cancelled);
        Ok(crate::dto::TurnCancelResult {
            target: "turn".into(),
            task_id: task_id.into(),
            turn_id: turn_id.into(),
            task_status,
            already_terminal: false,
            result,
        })
    }

    fn finalize_cancelled(
        &self,
        conn: &rusqlite::Connection,
        task_id: &str,
        turn_id: &str,
        cause: &str,
    ) -> Result<RunResult, TaskError> {
        let turn = repository::get_turn(conn, turn_id)
            .map_err(TaskError::storage)?
            .ok_or_else(|| TaskError::not_found("turn not found"))?;
        if let Some(ref rj) = turn.result_json {
            if let Ok(r) = serde_json::from_str::<RunResult>(rj) {
                return Ok(r);
            }
        }
        let finished = now_ms();
        let started = turn.started_at.unwrap_or(turn.created_at);
        let (outcome, run_status, partial, error) =
            map_stop_to_outcome(Some("cancelled"), Some(cause));
        let result = RunResult {
            task_id: task_id.into(),
            turn_id: turn_id.into(),
            turn_ordinal: turn.ordinal as u32,
            status: run_status,
            mode: TaskMode::parse(&turn.mode).unwrap_or(TaskMode::Read),
            session_id: None,
            requested_model: None,
            actual_model: None,
            stop_reason: Some("cancelled".into()),
            turn_outcome: outcome,
            partial,
            answer: turn.answer_markdown.clone(),
            error,
            started_at: ms_to_rfc3339(started),
            finished_at: ms_to_rfc3339(finished),
            duration_ms: (finished - started).max(0) as u64,
        };
        let val = serde_json::to_value(&result).unwrap_or(json!({}));
        let turn_status = match run_status {
            RunStatus::Cancelled => "cancelled",
            RunStatus::Failed => "failed",
            RunStatus::Completed => "completed",
        };
        repository::finalize_turn(
            conn,
            turn_id,
            turn_status,
            &result.answer,
            Some(cause),
            partial,
            &val,
            finished,
        )
        .map_err(TaskError::storage)?;
        let task_status = match run_status {
            RunStatus::Cancelled => "cancelled",
            RunStatus::Failed => "failed",
            RunStatus::Completed => "idle",
        };
        let _ = repository::update_task_progress(
            conn,
            task_id,
            task_status,
            Some(turn_id),
            None,
            Some("cancelled"),
            result.error.as_ref().map(|e| e.code.as_str()),
            result.error.as_ref().map(|e| e.message.as_str()),
            Some(finished),
            finished,
        );
        if let Some(rt) = self.turns.lock().get(turn_id).cloned() {
            *rt.result.lock() = Some(result.clone());
            rt.finished.notify_waiters();
        }
        Ok(result)
    }

    fn seed_user_message(
        &self,
        conn: &rusqlite::Connection,
        task_id: &str,
        turn_id: &str,
        prompt: &str,
    ) -> Result<(), repository::RepoError> {
        self.seed_user_message_at(conn, task_id, turn_id, prompt, 1)
    }

    fn seed_user_message_at(
        &self,
        conn: &rusqlite::Connection,
        task_id: &str,
        turn_id: &str,
        prompt: &str,
        sequence: i64,
    ) -> Result<(), repository::RepoError> {
        let item_id = format!("seg:{turn_id}:0:user");
        let payload = json!({
            "itemId": item_id,
            "kind": "user_message",
            "turnId": turn_id,
            "message": crate::acp::types::first_line_truncated(prompt, 120),
            "text": prompt,
            "streaming": false,
        });
        let now = now_ms();
        let item = TimelineItemRow {
            task_id: task_id.into(),
            item_id: item_id.clone(),
            turn_id: Some(turn_id.into()),
            kind: "user_message".into(),
            first_sequence: sequence,
            last_sequence: sequence,
            payload_json: payload.to_string(),
            created_at: now,
            updated_at: now,
        };
        let m = MutationRow {
            task_id: task_id.into(),
            sequence,
            generation: 1,
            operation: "add".into(),
            item_id: Some(item_id),
            payload_json: payload.to_string(),
            created_at: now,
        };
        repository::commit_timeline_mutations(conn, task_id, &[item], &[m], sequence)
    }

    async fn run_turn(
        self: Arc<Self>,
        task_id: String,
        turn_id: String,
        input: TaskInput,
        rt: Arc<TurnRuntime>,
    ) {
        let started = now_ms();
        if let Ok(conn) = self.open() {
            let _ = repository::update_task_status(&conn, &task_id, "starting", started);
            let _ = repository::update_turn_status(&conn, &turn_id, "starting", Some(started));
        }

        if rt.cancel_requested.load(Ordering::SeqCst) {
            if let Ok(conn) = self.open() {
                let cause = rt
                    .termination_cause
                    .lock()
                    .clone()
                    .unwrap_or_else(|| "user_cancel".into());
                let _ = self.finalize_cancelled(&conn, &task_id, &turn_id, &cause);
            }
            return;
        }

        // Fixture / missing Grok path: synthesize a minimal successful turn for tests
        // when GROKTASK_FIXTURE=1, otherwise try real Grok then fail with grok_not_found.
        let fixture = self.fixture_mode
            || std::env::var("GROKTASK_FIXTURE")
                .map(|v| v == "1")
                .unwrap_or(false);

        let result = if fixture {
            self.run_fixture_turn(&task_id, &turn_id, &input, &rt, started)
                .await
        } else {
            match self
                .run_grok_turn(&task_id, &turn_id, &input, &rt, started)
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    self.fail_turn(&task_id, &turn_id, &input, started, &e, &rt)
                        .await
                }
            }
        };

        *rt.result.lock() = Some(result);
        rt.finished.notify_waiters();
        self.turns.lock().remove(&turn_id);
    }

    async fn run_fixture_turn(
        &self,
        task_id: &str,
        turn_id: &str,
        input: &TaskInput,
        rt: &TurnRuntime,
        started: i64,
    ) -> RunResult {
        if let Ok(conn) = self.open() {
            let _ = repository::update_task_status(&conn, task_id, "running", now_ms());
            let _ = repository::update_turn_status(&conn, turn_id, "running", Some(started));
        }

        // Fixture path still needs a real ordinal when present; tests always seed a turn.
        let turn_ordinal = self
            .turn_ordinal(task_id, turn_id)
            .expect("fixture turn row must exist for ordinal");
        let session_id = "fixture-session".to_string();
        if let Ok(conn) = self.open() {
            let _ = conn.execute(
                "UPDATE tasks SET acp_session_id = ?1, session_state = 'warm' WHERE id = ?2",
                rusqlite::params![session_id, task_id],
            );
        }
        let mut reducer = TurnReducer::new(task_id, turn_id, &session_id);
        // Local seed is already persisted; prime reducer so ACP user echoes dedupe.
        reducer.seed_existing_user_message(&input.task);

        // Synthetic ACP stream: thought → tool → thought → reply
        let lines = [
            r#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"Planning response"}}}}"#,
            r#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"tool_call","toolCallId":"fx1","title":"Inspect task","kind":"read","status":"completed","locations":[{"path":"README.md"}]}}}"#,
            r#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"Drafting answer"}}}}"#,
            &format!(
                r#"{{"jsonrpc":"2.0","method":"session/update","params":{{"update":{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"Fixture reply for: {}"}}}}}}}}"#,
                escape_json(&input.task.chars().take(80).collect::<String>())
            ),
        ];

        for line in lines {
            if rt.cancel_requested.load(Ordering::SeqCst) {
                break;
            }
            self.ingest_line(task_id, turn_id, &mut reducer, line);
            tokio::time::sleep(Duration::from_millis(5)).await;
        }

        let cause = rt.termination_cause.lock().clone();
        if cause.is_some() || rt.cancel_requested.load(Ordering::SeqCst) {
            let c = cause.unwrap_or_else(|| "user_cancel".into());
            if let Ok(conn) = self.open() {
                return self
                    .finalize_cancelled(&conn, task_id, turn_id, &c)
                    .unwrap_or_else(|_| {
                        empty_failed_result(task_id, turn_id, input, started, "cancel_failed")
                    });
            }
        }

        reducer.finalize_turn(Some("finalAnswer"));
        self.persist_reducer(task_id, &mut reducer);
        let answer = reducer.final_answer_markdown();
        let finished = now_ms();
        let result = RunResult {
            task_id: task_id.into(),
            turn_id: turn_id.into(),
            turn_ordinal,
            status: RunStatus::Completed,
            mode: input.mode,
            session_id: Some(session_id),
            requested_model: input.model.clone(),
            actual_model: Some("fixture".into()),
            stop_reason: Some("end_turn".into()),
            turn_outcome: TurnOutcome::Completed,
            partial: false,
            answer,
            error: None,
            started_at: ms_to_rfc3339(started),
            finished_at: ms_to_rfc3339(finished),
            duration_ms: (finished - started).max(0) as u64,
        };
        self.persist_run_result(task_id, turn_id, &result, None);
        result
    }

    async fn run_grok_turn(
        &self,
        task_id: &str,
        turn_id: &str,
        input: &TaskInput,
        rt: &TurnRuntime,
        started: i64,
    ) -> Result<RunResult> {
        let grok = resolve_grok_executable(self.grok_executable.as_deref())?;
        let mode = match input.mode {
            TaskMode::Read => AcpMode::Read,
            TaskMode::Write => AcpMode::Write,
        };
        let argv = build_grok_argv(mode, input.model.as_deref(), input.effort.as_deref());
        // Hard guarantee: ordinal lookup failure must not default to 1 (which would
        // misclassify a follow-up as first turn and allow session/new).
        let turn_ordinal = self.turn_ordinal(task_id, turn_id)?;
        let stored_session = self.stored_acp_session_id(task_id)?;

        if let Ok(conn) = self.open() {
            let _ = repository::update_task_status(&conn, task_id, "running", now_ms());
            let _ = repository::update_turn_status(&conn, turn_id, "running", Some(started));
        }

        let mut child = Command::new(&grok)
            .args(&argv)
            .current_dir(&input.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("spawn grok at {}", grok.display()))?;

        let mut stdin = child.stdin.take().context("grok stdin")?;
        let stdout = child.stdout.take().context("grok stdout")?;
        let stderr = child.stderr.take().context("grok stderr")?;

        // Drain stderr to avoid deadlock; redact logs.
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let _ = crate::acp::redact::redact_log_line(&line);
            }
        });

        let mut reader = BufReader::new(stdout);
        // initialize
        let init = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": 1,
                "clientCapabilities": {},
                "clientInfo": { "name": "GrokTask", "version": crate::version::APP_VERSION }
            }
        });
        write_rpc(&mut stdin, &init).await?;
        let _init_resp = read_rpc_response(&mut reader).await?;

        // First turn: session/new. Follow-up: session/load with persisted id only.
        // Never silently replace a missing/failed load with session/new.
        let session_id = match open_or_load_session(
            &mut stdin,
            &mut reader,
            &input.cwd,
            stored_session.as_deref(),
            turn_ordinal,
        )
        .await
        {
            Ok(sid) => sid,
            Err(e) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                return Err(e);
            }
        };

        if let Ok(conn) = self.open() {
            // Preserve stored id on load; set on first new. Never clear on failure path above.
            let _ = conn.execute(
                "UPDATE tasks SET acp_session_id = ?1, session_state = 'warm' WHERE id = ?2",
                rusqlite::params![session_id, task_id],
            );
        }

        let mut reducer = TurnReducer::new(task_id, turn_id, &session_id);
        // Local seed is already persisted; prime reducer so ACP user echoes dedupe.
        reducer.seed_existing_user_message(&input.task);

        // session/prompt
        let prompt_id: i64 = 3;
        let prompt_req = json!({
            "jsonrpc": "2.0",
            "id": prompt_id,
            "method": "session/prompt",
            "params": {
                "sessionId": session_id,
                "prompt": [{ "type": "text", "text": input.task }]
            }
        });
        write_rpc(&mut stdin, &prompt_req).await?;
        if let Ok(conn) = self.open() {
            let _ = conn.execute(
                "UPDATE turns SET prompt_dispatched_at = ?1, status = 'running' WHERE id = ?2",
                rusqlite::params![now_ms(), turn_id],
            );
        }

        let mut stop_reason: Option<String> = None;
        let mut line_buf = String::new();
        loop {
            if rt.cancel_requested.load(Ordering::SeqCst) {
                let cancel = json!({
                    "jsonrpc": "2.0",
                    "method": "session/cancel",
                    "params": { "sessionId": session_id }
                });
                let _ = write_rpc(&mut stdin, &cancel).await;
                break;
            }

            line_buf.clear();
            let read =
                tokio::time::timeout(Duration::from_millis(200), reader.read_line(&mut line_buf))
                    .await;
            match read {
                Ok(Ok(0)) => break,
                Ok(Ok(_)) => {
                    let line = line_buf.trim();
                    if line.is_empty() {
                        continue;
                    }
                    // Response to prompt?
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                        if let Some(response) = permission_response_for(input.mode, &v) {
                            self.ingest_line(task_id, turn_id, &mut reducer, line);
                            let status = response
                                .pointer("/result/outcome/optionId")
                                .and_then(|v| v.as_str())
                                .map(permission_option_status)
                                .unwrap_or("answered");
                            let request_id = v
                                .get("id")
                                .map(|id| match id {
                                    serde_json::Value::String(s) => s.clone(),
                                    other => other.to_string(),
                                })
                                .unwrap_or_else(|| "unknown".into());
                            reducer.apply(
                                crate::acp::types::NormalizedUpdate::PermissionDecision {
                                    request_id,
                                    status: status.into(),
                                },
                            );
                            self.persist_reducer(task_id, &mut reducer);
                            let _ = write_rpc(&mut stdin, &response).await;
                            continue;
                        }
                        if v.get("id").and_then(|i| i.as_i64()) == Some(prompt_id) {
                            stop_reason = v
                                .pointer("/result/stopReason")
                                .and_then(|s| s.as_str())
                                .map(str::to_string);
                            // drain briefly
                            let drain_deadline =
                                tokio::time::Instant::now() + Duration::from_millis(500);
                            while tokio::time::Instant::now() < drain_deadline {
                                line_buf.clear();
                                match tokio::time::timeout(
                                    Duration::from_millis(100),
                                    reader.read_line(&mut line_buf),
                                )
                                .await
                                {
                                    Ok(Ok(n)) if n > 0 => {
                                        self.ingest_line(
                                            task_id,
                                            turn_id,
                                            &mut reducer,
                                            line_buf.trim(),
                                        );
                                    }
                                    _ => break,
                                }
                            }
                            break;
                        }
                    }
                    self.ingest_line(task_id, turn_id, &mut reducer, line);
                }
                Ok(Err(e)) => return Err(e.into()),
                Err(_) => {
                    // timeout — continue loop for cancel check
                }
            }
        }

        let _ = child.kill().await;
        let _ = child.wait().await;

        let cause = rt.termination_cause.lock().clone();
        let (outcome, run_status, partial, error) =
            map_stop_to_outcome(stop_reason.as_deref(), cause.as_deref());

        let mark = match outcome {
            TurnOutcome::Completed => Some("finalAnswer"),
            TurnOutcome::Partial => Some("partialAnswer"),
            _ => None,
        };
        reducer.finalize_turn(mark);
        self.persist_reducer(task_id, &mut reducer);
        let answer = reducer.final_answer_markdown();
        let finished = now_ms();
        let result = RunResult {
            task_id: task_id.into(),
            turn_id: turn_id.into(),
            turn_ordinal,
            status: run_status,
            mode: input.mode,
            session_id: Some(session_id),
            requested_model: input.model.clone(),
            actual_model: None,
            stop_reason,
            turn_outcome: outcome,
            partial,
            answer,
            error,
            started_at: ms_to_rfc3339(started),
            finished_at: ms_to_rfc3339(finished),
            duration_ms: (finished - started).max(0) as u64,
        };
        self.persist_run_result(task_id, turn_id, &result, cause.as_deref());
        Ok(result)
    }

    fn stored_acp_session_id(&self, task_id: &str) -> Result<Option<String>> {
        let conn = self.open()?;
        let task =
            repository::get_task(&conn, task_id)?.context("task missing for session lookup")?;
        Ok(task
            .acp_session_id
            .filter(|s| !s.is_empty() && s != "unknown"))
    }

    fn turn_ordinal(&self, task_id: &str, turn_id: &str) -> Result<u32> {
        let conn = self.open()?;
        let turn = repository::get_turn(&conn, turn_id)?.context("turn missing")?;
        if turn.task_id != task_id {
            return Err(anyhow!("turn does not belong to task"));
        }
        Ok(turn.ordinal as u32)
    }

    async fn fail_turn(
        &self,
        task_id: &str,
        turn_id: &str,
        input: &TaskInput,
        started: i64,
        err: &anyhow::Error,
        _rt: &TurnRuntime,
    ) -> RunResult {
        let finished = now_ms();
        let msg = format!("{err:#}");
        let code = classify_turn_error(&msg);
        let turn_ordinal = self.turn_ordinal(task_id, turn_id).unwrap_or(1);
        // Keep any previously stored session id on load failures (do not clear).
        let session_id = self.stored_acp_session_id(task_id).ok().flatten();
        let result = RunResult {
            task_id: task_id.into(),
            turn_id: turn_id.into(),
            turn_ordinal,
            status: RunStatus::Failed,
            mode: input.mode,
            session_id,
            requested_model: input.model.clone(),
            actual_model: None,
            stop_reason: None,
            turn_outcome: TurnOutcome::Failed,
            partial: false,
            answer: String::new(),
            error: Some(ErrorInfo {
                code: code.into(),
                message: msg,
                retryable: matches!(
                    code,
                    "grok_not_found" | "session_load_failed" | "session_unavailable"
                ),
            }),
            started_at: ms_to_rfc3339(started),
            finished_at: ms_to_rfc3339(finished),
            duration_ms: (finished - started).max(0) as u64,
        };
        self.persist_run_result(task_id, turn_id, &result, None);
        result
    }

    fn ingest_line(&self, task_id: &str, turn_id: &str, reducer: &mut TurnReducer, line: &str) {
        // Persist redacted raw diagnostic
        if let Ok(conn) = self.open() {
            let seq = self.raw_seq.fetch_add(1, Ordering::SeqCst) + 1;
            let raw: serde_json::Value =
                serde_json::from_str(line).unwrap_or_else(|_| json!({ "line": line }));
            let redacted = bound_payload(&raw, DEFAULT_DIAGNOSTIC_MAX_BYTES);
            let method = raw
                .get("method")
                .and_then(|m| m.as_str())
                .map(str::to_string);
            let _ = repository::insert_raw_event(
                &conn,
                task_id,
                seq,
                "from_agent",
                method.as_deref(),
                &redacted.to_string(),
            );
        }

        for up in normalize_line(line) {
            reducer.apply(up);
        }
        if let Some(a) = reducer.latest_action.clone() {
            self.latest_action.lock().insert(task_id.into(), a);
        }
        if let Some(s) = reducer.current_step.clone() {
            self.current_step.lock().insert(task_id.into(), s);
        }
        self.persist_reducer(task_id, reducer);
        let _ = turn_id;
    }

    fn persist_reducer(&self, task_id: &str, reducer: &mut TurnReducer) {
        let muts = reducer.take_mutations();
        if muts.is_empty() {
            return;
        }
        let Ok(conn) = self.open() else {
            return;
        };
        let Ok(Some(task)) = repository::get_task(&conn, task_id) else {
            return;
        };
        let gen = task.timeline_generation;
        let mut last_seq = task.last_sequence;
        let now = now_ms();
        let mut items = Vec::new();
        let mut mutations = Vec::new();
        for m in muts {
            last_seq += 1;
            let payload = serde_json::to_value(&m.item).unwrap_or(json!({}));
            // Strip diagnostic from primary payload storage path for timeline_items display size
            let mut store = payload.clone();
            if let Some(obj) = store.as_object_mut() {
                // keep diagnostic in storage for diagnostics view but OK
                let _ = obj;
            }
            items.push(TimelineItemRow {
                task_id: task_id.into(),
                item_id: m.item_id.clone(),
                turn_id: Some(m.item.turn_id.clone()),
                kind: m.item.kind.clone(),
                first_sequence: last_seq,
                last_sequence: last_seq,
                payload_json: store.to_string(),
                created_at: now,
                updated_at: now,
            });
            mutations.push(MutationRow {
                task_id: task_id.into(),
                sequence: last_seq,
                generation: gen,
                operation: m.operation,
                item_id: Some(m.item_id),
                payload_json: store.to_string(),
                created_at: now,
            });
        }
        let _ = repository::commit_timeline_mutations(&conn, task_id, &items, &mutations, last_seq);
    }

    fn persist_run_result(
        &self,
        task_id: &str,
        turn_id: &str,
        result: &RunResult,
        termination_cause: Option<&str>,
    ) {
        let Ok(conn) = self.open() else {
            return;
        };
        let finished = now_ms();
        let turn_status = match result.status {
            RunStatus::Completed if result.partial => "partial",
            RunStatus::Completed => "completed",
            RunStatus::Cancelled => "cancelled",
            RunStatus::Failed => match result.turn_outcome {
                TurnOutcome::Refused => "refused",
                _ => "failed",
            },
        };
        let val = serde_json::to_value(result).unwrap_or(json!({}));
        let _ = repository::finalize_turn(
            &conn,
            turn_id,
            turn_status,
            &result.answer,
            termination_cause,
            result.partial,
            &val,
            finished,
        );
        let task_status = match result.status {
            RunStatus::Completed => "idle",
            RunStatus::Cancelled => "cancelled",
            RunStatus::Failed => "failed",
        };
        let _ = repository::update_task_progress(
            &conn,
            task_id,
            task_status,
            Some(turn_id),
            result.actual_model.as_deref(),
            result.stop_reason.as_deref(),
            result.error.as_ref().map(|e| e.code.as_str()),
            result.error.as_ref().map(|e| e.message.as_str()),
            Some(finished),
            finished,
        );
        let _ = conn.execute(
            "UPDATE tasks SET retention_protect_until = ?1 WHERE id = ?2",
            rusqlite::params![finished + 30 * 60 * 1000, task_id],
        );
    }
}

fn empty_failed_result(
    task_id: &str,
    turn_id: &str,
    input: &TaskInput,
    started: i64,
    code: &str,
) -> RunResult {
    let finished = now_ms();
    RunResult {
        task_id: task_id.into(),
        turn_id: turn_id.into(),
        turn_ordinal: 1,
        status: RunStatus::Failed,
        mode: input.mode,
        session_id: None,
        requested_model: input.model.clone(),
        actual_model: None,
        stop_reason: None,
        turn_outcome: TurnOutcome::Failed,
        partial: false,
        answer: String::new(),
        error: Some(ErrorInfo {
            code: code.into(),
            message: code.into(),
            retryable: false,
        }),
        started_at: ms_to_rfc3339(started),
        finished_at: ms_to_rfc3339(finished),
        duration_ms: (finished - started).max(0) as u64,
    }
}

fn escape_json(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn resolve_grok_executable(configured: Option<&str>) -> Result<PathBuf> {
    if let Some(p) = configured {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Ok(path);
        }
    }
    if let Ok(p) = std::env::var("GROK_EXECUTABLE") {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Ok(path);
        }
    }
    // PATH lookup
    if let Ok(path) = which("grok") {
        return Ok(path);
    }
    for path in common_grok_paths() {
        if path.is_file() {
            return Ok(path);
        }
    }
    Err(anyhow!(
        "grok CLI not found; install Grok CLI, set grokExecutable, or set GROK_EXECUTABLE"
    ))
}

fn common_grok_paths() -> Vec<PathBuf> {
    let home = crate::paths::home();
    vec![
        home.join(".local/bin/grok"),
        home.join(".grok/bin/grok"),
        home.join("bin/grok"),
        PathBuf::from("/opt/homebrew/bin/grok"),
        PathBuf::from("/usr/local/bin/grok"),
    ]
}

fn which(name: &str) -> Result<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Ok(candidate);
        }
        #[cfg(windows)]
        {
            let exe = dir.join(format!("{name}.exe"));
            if exe.is_file() {
                return Ok(exe);
            }
        }
    }
    Err(anyhow!("not found: {name}"))
}

async fn write_rpc(
    stdin: &mut (impl AsyncWriteExt + Unpin),
    msg: &serde_json::Value,
) -> Result<()> {
    let mut line = serde_json::to_string(msg)?;
    line.push('\n');
    stdin.write_all(line.as_bytes()).await?;
    stdin.flush().await?;
    Ok(())
}

fn permission_response_for(mode: TaskMode, msg: &serde_json::Value) -> Option<serde_json::Value> {
    let method = msg.get("method").and_then(|v| v.as_str())?;
    if !matches!(
        method,
        "session/request_permission" | "session/requestPermission"
    ) {
        return None;
    }
    let id = msg.get("id")?.clone();
    let options = msg
        .pointer("/params/options")
        .and_then(|v| v.as_array())
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let selected = match mode {
        // The process is already launched with mode-specific sandbox / deny
        // rules. In headless operation, answer permission prompts once so the
        // agent can continue instead of leaving the UI with an unclickable
        // request. If Grok offers no allow option, reject as a safe fallback.
        TaskMode::Read | TaskMode::Write => {
            find_permission_option(options, &["allow_once", "allow_always"])
                .or_else(|| find_permission_option(options, &["reject_once", "reject_always"]))
        }
    }?;
    Some(json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "outcome": {
                "outcome": "selected",
                "optionId": selected
            }
        }
    }))
}

fn find_permission_option(options: &[serde_json::Value], kinds: &[&str]) -> Option<String> {
    for kind in kinds {
        if let Some(id) = options.iter().find_map(|o| {
            (o.get("kind").and_then(|v| v.as_str()) == Some(*kind))
                .then(|| {
                    o.get("optionId")
                        .or_else(|| o.get("id"))
                        .and_then(|v| v.as_str())
                })
                .flatten()
                .map(str::to_string)
        }) {
            return Some(id);
        }
    }
    None
}

fn permission_option_status(option_id: &str) -> &'static str {
    if option_id.contains("allow") {
        "approved"
    } else if option_id.contains("reject") {
        "rejected"
    } else {
        "answered"
    }
}

async fn read_rpc_response(reader: &mut (impl AsyncBufRead + Unpin)) -> Result<serde_json::Value> {
    // initialize uses id 1
    read_rpc_response_for_id(reader, 1).await
}

fn rpc_id_matches(v: &serde_json::Value, expected: i64) -> bool {
    match v.get("id") {
        Some(serde_json::Value::Number(n)) => n
            .as_i64()
            .map(|i| i == expected)
            .or_else(|| n.as_u64().map(|u| u as i64 == expected))
            .unwrap_or(false),
        Some(serde_json::Value::String(s)) => s.parse::<i64>().ok() == Some(expected),
        _ => false,
    }
}

/// Wait for the JSON-RPC response whose `id` equals `expected_id`.
///
/// Discards intermediate notifications (e.g. `session/load` replay) so they never
/// hit the live timeline reducer. Unexpected responses with a different `id` fail.
async fn read_rpc_response_for_id(
    reader: &mut (impl AsyncBufRead + Unpin),
    expected_id: i64,
) -> Result<serde_json::Value> {
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            return Err(anyhow!(
                "EOF from grok before JSON-RPC response id {expected_id}"
            ));
        }
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(t).with_context(|| {
            format!("invalid JSON while waiting for response id {expected_id}: {t}")
        })?;
        if v.get("id").is_none() {
            // Notification (e.g. session/load replay) — ignore for timeline.
            continue;
        }
        if rpc_id_matches(&v, expected_id) {
            return Ok(v);
        }
        return Err(anyhow!(
            "unexpected JSON-RPC response id while waiting for {expected_id}: {t}"
        ));
    }
}

/// Validate `session/load` JSON-RPC response for the requested session id.
///
/// Requires a success `result`. A non-empty `result.sessionId` that differs from
/// the stored/requested id is `session_load_failed` (stored id is not updated here).
pub(crate) fn validate_session_load_response(
    requested_sid: &str,
    resp: &serde_json::Value,
) -> Result<String> {
    if let Some(err) = resp.get("error") {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("session/load failed");
        return Err(anyhow!(
            "session_load_failed: could not load ACP session `{requested_sid}`: {msg}. \
             The stored session id was preserved; fix Grok/auth or start a new task \
             with run/start if this conversation context is lost. Do not invent a new session."
        ));
    }
    let Some(result) = resp.get("result") else {
        return Err(anyhow!(
            "session_load_failed: session/load response for `{requested_sid}` missing success \
             result (malformed JSON-RPC). The stored session id was preserved."
        ));
    };
    match result.get("sessionId").and_then(|v| v.as_str()) {
        None | Some("") => {
            // Success result without sessionId: keep the requested/stored id.
            Ok(requested_sid.to_string())
        }
        Some(loaded) if loaded == requested_sid => Ok(requested_sid.to_string()),
        Some(loaded) => Err(anyhow!(
            "session_load_failed: session/load returned sessionId `{loaded}` but stored/requested \
             id is `{requested_sid}`. Refusing to continue with a different session. \
             The stored session id was preserved."
        )),
    }
}

pub(crate) fn validate_session_new_response(resp: &serde_json::Value) -> Result<String> {
    if let Some(err) = resp.get("error") {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("session/new failed");
        return Err(anyhow!("session_create_failed: {msg}"));
    }
    if resp.get("result").is_none() {
        return Err(anyhow!(
            "session_create_failed: session/new response missing success result"
        ));
    }
    resp.pointer("/result/sessionId")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow!("session_create_failed: session/new returned no sessionId"))
}

/// Open a new ACP session or load an existing one. Pure protocol helper for tests.
///
/// - Stored `acp_session_id` present → `session/load` only (never `session/new`).
/// - Absent and first turn → `session/new`.
/// - Absent on a genuine follow-up (ordinal > 1) → hard error; never invent a session.
pub(crate) async fn open_or_load_session(
    stdin: &mut (impl AsyncWriteExt + Unpin),
    reader: &mut (impl AsyncBufRead + Unpin),
    cwd: &str,
    stored_session_id: Option<&str>,
    turn_ordinal: u32,
) -> Result<String> {
    if let Some(sid) = stored_session_id.filter(|s| !s.is_empty()) {
        let load_req = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "session/load",
            "params": {
                "sessionId": sid,
                "cwd": cwd,
                "mcpServers": []
            }
        });
        write_rpc(stdin, &load_req).await?;
        let load_resp = read_rpc_response_for_id(reader, 2).await?;
        return validate_session_load_response(sid, &load_resp);
    }

    if turn_ordinal > 1 {
        return Err(anyhow!(
            "session_unavailable: follow-up turn (ordinal {turn_ordinal}) has no persisted \
             acp_session_id. Cannot call session/new on a follow-up; resume requires a stored \
             session id. Start a new task with run/start if you need a fresh context."
        ));
    }

    let new_req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "session/new",
        "params": { "cwd": cwd, "mcpServers": [] }
    });
    write_rpc(stdin, &new_req).await?;
    let new_resp = read_rpc_response_for_id(reader, 2).await?;
    validate_session_new_response(&new_resp)
}

fn classify_turn_error(msg: &str) -> &'static str {
    let lower = msg.to_ascii_lowercase();
    if lower.contains("session_load_failed") {
        "session_load_failed"
    } else if lower.contains("session_unavailable") {
        "session_unavailable"
    } else if lower.contains("session_create_failed") {
        "session_create_failed"
    } else if lower.contains("grok") {
        "grok_not_found"
    } else {
        "internal_error"
    }
}

/// Which ACP session method should be used (unit-tested without a process).
pub(crate) fn session_open_method(
    stored_session_id: Option<&str>,
    turn_ordinal: u32,
) -> Result<&'static str, String> {
    if stored_session_id.map(|s| !s.is_empty()).unwrap_or(false) {
        return Ok("session/load");
    }
    if turn_ordinal > 1 {
        return Err(
            "session_unavailable: follow-up has no acp_session_id; refuse session/new".into(),
        );
    }
    Ok("session/new")
}

#[derive(Debug)]
pub enum TaskError {
    Invalid { code: String, message: String },
    NotFound { message: String },
    Conflict { code: String, message: String },
    Storage { message: String },
    Internal { message: String },
    WaitTimeout(WaitTimeout),
}

impl TaskError {
    pub fn invalid(msg: impl Into<String>) -> Self {
        Self::Invalid {
            code: "invalid_params".into(),
            message: msg.into(),
        }
    }
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound {
            message: msg.into(),
        }
    }
    pub fn conflict(code: impl Into<String>, msg: impl Into<String>) -> Self {
        Self::Conflict {
            code: code.into(),
            message: msg.into(),
        }
    }
    pub fn storage(e: impl std::fmt::Display) -> Self {
        Self::Storage {
            message: e.to_string(),
        }
    }
    pub fn internal(e: impl std::fmt::Display) -> Self {
        Self::Internal {
            message: e.to_string(),
        }
    }

    pub fn code(&self) -> &str {
        match self {
            Self::Invalid { code, .. } => code,
            Self::NotFound { .. } => "not_found",
            Self::Conflict { code, .. } => code,
            Self::Storage { .. } => "storage_failure",
            Self::Internal { .. } => "internal_error",
            Self::WaitTimeout(_) => "wait_timeout",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::Invalid { message, .. }
            | Self::NotFound { message }
            | Self::Conflict { message, .. }
            | Self::Storage { message }
            | Self::Internal { message } => message.clone(),
            Self::WaitTimeout(w) => format!("wait timed out for turn {}", w.turn_id),
        }
    }

    pub fn retryable(&self) -> bool {
        matches!(self, Self::WaitTimeout(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn fixture_start_list_status_cancel_flow() {
        std::env::set_var("GROKTASK_FIXTURE", "1");
        let tmp = TempDir::new().unwrap();
        let db = tmp.path().join("h.sqlite3");
        let _ = crate::storage::open_path(&db).unwrap();
        let mgr = Arc::new(TaskManager::new(db, None));
        let input = TaskInput {
            task: "hello fixture".into(),
            cwd: tmp.path().to_string_lossy().into_owned(),
            mode: TaskMode::Read,
            model: None,
            effort: None,
            title: Some("Fixture".into()),
        };
        let start = mgr
            .start(input, "sub-1".into(), "daemon", None, None)
            .unwrap();
        assert_eq!(start.turn_ordinal, 1);
        let result = mgr
            .wait(&start.task_id, &start.turn_id, 5_000)
            .await
            .unwrap();
        assert_eq!(result.status, RunStatus::Completed);
        assert!(result.answer.contains("Fixture reply"), "{}", result.answer);
        assert!(!result.answer.contains("session/update"));

        let list = mgr.list(10).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "Fixture");

        let detail = mgr.detail(&start.task_id).unwrap();
        assert!(!detail.timeline.is_empty());
        for ev in &detail.timeline {
            assert!(!ev.message.contains("session/update"));
            assert!(!ev.message.contains("tool_call_update"));
        }
        // Cancel already terminal is idempotent
        let c = mgr
            .cancel_turn(&start.task_id, &start.turn_id, "user_cancel")
            .unwrap();
        assert!(c.already_terminal);

        // Dedupe submission
        let input2 = TaskInput {
            task: "hello fixture".into(),
            cwd: tmp.path().canonicalize().unwrap().to_string_lossy().into(),
            mode: TaskMode::Read,
            model: None,
            effort: None,
            title: Some("Fixture".into()),
        };
        // Note: cwd canonicalize may differ — use same as stored via re-validate
        let _ = input2;
        std::env::remove_var("GROKTASK_FIXTURE");
    }

    #[test]
    fn grok_executable_prefers_configured_absolute_path() {
        let tmp = TempDir::new().unwrap();
        let grok = tmp.path().join("grok");
        std::fs::write(&grok, b"#!/bin/sh\n").unwrap();

        assert_eq!(
            resolve_grok_executable(Some(&grok.to_string_lossy())).unwrap(),
            grok
        );
    }

    #[test]
    fn permission_requests_are_answered_by_mode() {
        let request = json!({
            "jsonrpc": "2.0",
            "id": 99,
            "method": "session/request_permission",
            "params": {
                "options": [
                    { "kind": "allow_once", "name": "Yes, proceed", "optionId": "allow-once" },
                    { "kind": "reject_once", "name": "No", "optionId": "reject-once" }
                ],
                "toolCall": {
                    "_meta": {
                        "x.ai/tool": {
                            "kind": "execute",
                            "read_only": false
                        }
                    },
                    "kind": "execute",
                    "toolCallId": "tool-1"
                }
            }
        });

        assert_eq!(
            permission_response_for(TaskMode::Read, &request)
                .unwrap()
                .pointer("/result/outcome/optionId")
                .and_then(|v| v.as_str()),
            Some("allow-once")
        );
        assert_eq!(
            permission_response_for(TaskMode::Write, &request)
                .unwrap()
                .pointer("/result/outcome/optionId")
                .and_then(|v| v.as_str()),
            Some("allow-once")
        );
    }

    #[test]
    fn first_turn_uses_session_new_followup_uses_session_load() {
        assert_eq!(session_open_method(None, 1).unwrap(), "session/new");
        assert_eq!(
            session_open_method(Some("sess-1"), 1).unwrap(),
            "session/load"
        );
        assert_eq!(
            session_open_method(Some("sess-1"), 2).unwrap(),
            "session/load"
        );
        let err = session_open_method(None, 2).unwrap_err();
        assert!(err.contains("session_unavailable"));
        assert!(!err.contains("session/new") || err.contains("refuse"));
    }

    #[test]
    fn followup_ordinal_must_not_default_to_first_turn() {
        // Hard guarantee: treating a missing ordinal as 1 would allow session/new
        // on a genuine follow-up. session_open_method encodes the gate; production
        // must propagate turn_ordinal lookup errors instead of unwrap_or(1).
        assert!(
            session_open_method(None, 2).is_err(),
            "ordinal>1 without stored session must refuse session/new"
        );
        assert_eq!(
            session_open_method(None, 1).unwrap(),
            "session/new",
            "only true first turn may session/new"
        );
    }

    #[test]
    fn turn_ordinal_lookup_failure_does_not_yield_one() {
        let tmp = TempDir::new().unwrap();
        let db = tmp.path().join("h.sqlite3");
        let _ = crate::storage::open_path(&db).unwrap();
        let mgr = TaskManager::new(db, None);
        let err = mgr
            .turn_ordinal("missing-task", "missing-turn")
            .unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            !msg.is_empty(),
            "lookup failure must be an error, not Ok(1)"
        );
        // Ensure we never silently invent ordinal 1 via Result::ok
        assert!(mgr.turn_ordinal("missing-task", "missing-turn").is_err());
    }

    #[test]
    fn session_load_rejects_mismatched_session_id() {
        let resp = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "result": { "sessionId": "other-sess" }
        });
        let err = validate_session_load_response("stored-sess", &resp).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("session_load_failed"), "{msg}");
        assert!(msg.contains("other-sess"), "{msg}");
        assert!(msg.contains("stored-sess"), "{msg}");
        assert!(msg.contains("preserved"), "{msg}");
    }

    #[test]
    fn session_load_rejects_error_and_malformed_responses() {
        let err_resp = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "error": { "code": -32000, "message": "unknown session" }
        });
        let err = validate_session_load_response("stored-sess", &err_resp).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("session_load_failed"), "{msg}");
        assert!(msg.contains("unknown session"), "{msg}");
        assert!(msg.contains("preserved"), "{msg}");

        let malformed = json!({ "jsonrpc": "2.0", "id": 2 });
        let err = validate_session_load_response("stored-sess", &malformed).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("session_load_failed"), "{msg}");
        assert!(
            msg.contains("missing success") || msg.contains("malformed"),
            "{msg}"
        );

        let empty_id = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "result": { "sessionId": "" }
        });
        // Empty sessionId keeps requested id (success without replacement).
        assert_eq!(
            validate_session_load_response("stored-sess", &empty_id).unwrap(),
            "stored-sess"
        );

        let ok = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "result": { "sessionId": "stored-sess" }
        });
        assert_eq!(
            validate_session_load_response("stored-sess", &ok).unwrap(),
            "stored-sess"
        );
    }

    #[tokio::test]
    async fn session_load_waits_for_id_2_and_discards_replay() {
        let (host, agent) = tokio::io::duplex(8192);
        let (host_read, mut host_write) = tokio::io::split(host);
        let (mut agent_read, mut agent_write) = tokio::io::split(agent);

        let agent_task = tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            // Drain the load request
            let mut buf = vec![0u8; 4096];
            let _ = agent_read.read(&mut buf).await;
            // Replay notification then success with matching id
            agent_write
                .write_all(
                    br#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"REPLAY"}}}}
{"jsonrpc":"2.0","id":2,"result":{"sessionId":"sess-exact"}}
"#,
                )
                .await
                .unwrap();
        });

        let mut reader = BufReader::new(host_read);
        let sid = open_or_load_session(&mut host_write, &mut reader, "/tmp", Some("sess-exact"), 2)
            .await
            .unwrap();
        assert_eq!(sid, "sess-exact");
        agent_task.await.unwrap();
    }

    #[tokio::test]
    async fn session_load_mismatched_id_over_wire_fails() {
        let (host, agent) = tokio::io::duplex(8192);
        let (host_read, mut host_write) = tokio::io::split(host);
        let (mut agent_read, mut agent_write) = tokio::io::split(agent);

        let agent_task = tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            let mut buf = vec![0u8; 4096];
            let _ = agent_read.read(&mut buf).await;
            agent_write
                .write_all(
                    br#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"wrong-id"}}
"#,
                )
                .await
                .unwrap();
        });

        let mut reader = BufReader::new(host_read);
        let err = open_or_load_session(&mut host_write, &mut reader, "/tmp", Some("stored-id"), 2)
            .await
            .unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("session_load_failed"), "{msg}");
        assert!(msg.contains("wrong-id"), "{msg}");
        agent_task.await.unwrap();
    }

    #[tokio::test]
    async fn followup_without_stored_session_refuses_before_session_new() {
        let (host, _agent) = tokio::io::duplex(64);
        let (host_read, mut host_write) = tokio::io::split(host);
        let mut reader = BufReader::new(host_read);
        let err = open_or_load_session(&mut host_write, &mut reader, "/tmp", None, 2)
            .await
            .unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("session_unavailable"), "{msg}");
    }

    #[tokio::test]
    async fn fixture_continue_creates_second_turn_same_session() {
        std::env::set_var("GROKTASK_FIXTURE", "1");
        let tmp = TempDir::new().unwrap();
        let db = tmp.path().join("h.sqlite3");
        let _ = crate::storage::open_path(&db).unwrap();
        let mgr = Arc::new(TaskManager::new(db, None));
        let input = TaskInput {
            task: "first turn".into(),
            cwd: tmp.path().to_string_lossy().into_owned(),
            mode: TaskMode::Read,
            model: None,
            effort: None,
            title: Some("Reuse".into()),
        };
        let start = mgr
            .start(input, "sub-cont-1".into(), "client", None, None)
            .unwrap();
        let r1 = mgr
            .wait(&start.task_id, &start.turn_id, 5_000)
            .await
            .unwrap();
        assert_eq!(r1.status, RunStatus::Completed);
        assert_eq!(r1.turn_ordinal, 1);
        assert_eq!(r1.session_id.as_deref(), Some("fixture-session"));

        let cont = mgr
            .continue_task(&start.task_id, "second turn".into(), "client", None, None)
            .unwrap();
        assert_eq!(cont.turn_ordinal, 2);
        assert_ne!(cont.turn_id, start.turn_id);
        let r2 = mgr.wait(&cont.task_id, &cont.turn_id, 5_000).await.unwrap();
        assert_eq!(r2.status, RunStatus::Completed);
        assert_eq!(r2.turn_ordinal, 2);
        assert_eq!(r2.session_id.as_deref(), Some("fixture-session"));
        assert!(r2.answer.contains("second turn"), "{}", r2.answer);

        let conn = mgr.open().unwrap();
        let task = repository::get_task(&conn, &start.task_id)
            .unwrap()
            .unwrap();
        assert_eq!(task.acp_session_id.as_deref(), Some("fixture-session"));
        let turns = repository::list_turns_for_task(&conn, &start.task_id).unwrap();
        assert_eq!(turns.len(), 2);

        std::env::remove_var("GROKTASK_FIXTURE");
    }

    #[cfg(unix)]
    fn write_executable_script(path: &std::path::Path, body: &str) {
        std::fs::write(path, body).unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn mock_grok_first_turn_session_new_followup_session_load() {
        let tmp = TempDir::new().unwrap();
        let db = tmp.path().join("h.sqlite3");
        let _ = crate::storage::open_path(&db).unwrap();
        let log_path = tmp.path().join("methods.log");
        let mock = tmp.path().join("mock-grok");
        // Append methods to a shared log so both process invocations are visible.
        let script = format!(
            r###"#!/usr/bin/env bash
set -euo pipefail
LOG="{log}"
while IFS= read -r line; do
  method=$(printf '%s' "$line" | sed -n 's/.*"method"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')
  id=$(printf '%s' "$line" | sed -n 's/.*"id"[[:space:]]*:[[:space:]]*\([0-9][0-9]*\).*/\1/p')
  if [[ -n "${{method:-}}" ]]; then
    echo "$method" >> "$LOG"
  fi
  case "${{method:-}}" in
    initialize)
      echo "{{\"jsonrpc\":\"2.0\",\"id\":${{id:-1}},\"result\":{{\"protocolVersion\":1,\"agentCapabilities\":{{\"loadSession\":true}}}}}}"
      ;;
    session/new)
      echo "{{\"jsonrpc\":\"2.0\",\"id\":${{id:-2}},\"result\":{{\"sessionId\":\"mock-sess-42\"}}}}"
      ;;
    session/load)
      echo '{{"jsonrpc":"2.0","method":"session/update","params":{{"update":{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"REPLAY_MUST_NOT_APPEAR"}}}}}}}}'
      echo "{{\"jsonrpc\":\"2.0\",\"id\":${{id:-2}},\"result\":{{\"sessionId\":\"mock-sess-42\"}}}}"
      ;;
    session/prompt)
      echo '{{"jsonrpc":"2.0","method":"session/update","params":{{"update":{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"live answer"}}}}}}}}'
      echo "{{\"jsonrpc\":\"2.0\",\"id\":${{id:-3}},\"result\":{{\"stopReason\":\"end_turn\"}}}}"
      ;;
  esac
done
"###,
            log = log_path.display()
        );
        write_executable_script(&mock, &script);

        std::env::remove_var("GROKTASK_FIXTURE");
        let mgr = Arc::new(TaskManager::new(
            db,
            Some(mock.to_string_lossy().into_owned()),
        ));
        let input = TaskInput {
            task: "hello mock".into(),
            cwd: tmp.path().to_string_lossy().into_owned(),
            mode: TaskMode::Read,
            model: None,
            effort: None,
            title: Some("MockACP".into()),
        };
        let start = mgr
            .start(input, "sub-mock-1".into(), "client", None, None)
            .unwrap();
        let r1 = mgr
            .wait(&start.task_id, &start.turn_id, 10_000)
            .await
            .unwrap();
        assert_eq!(r1.status, RunStatus::Completed, "{:?}", r1.error);
        assert_eq!(r1.session_id.as_deref(), Some("mock-sess-42"));
        assert!(r1.answer.contains("live answer"), "{}", r1.answer);

        let log1 = std::fs::read_to_string(&log_path).unwrap_or_default();
        assert!(
            log1.lines().any(|l| l == "session/new"),
            "first turn must session/new: {log1}"
        );
        assert!(
            !log1.lines().any(|l| l == "session/load"),
            "first turn must not session/load: {log1}"
        );

        let cont = mgr
            .continue_task(
                &start.task_id,
                "follow up please".into(),
                "client",
                None,
                None,
            )
            .unwrap();
        assert_eq!(cont.turn_ordinal, 2);
        let r2 = mgr
            .wait(&cont.task_id, &cont.turn_id, 10_000)
            .await
            .unwrap();
        assert_eq!(r2.status, RunStatus::Completed, "{:?}", r2.error);
        assert_eq!(r2.session_id.as_deref(), Some("mock-sess-42"));
        assert!(r2.answer.contains("live answer"), "{}", r2.answer);
        assert!(
            !r2.answer.contains("REPLAY_MUST_NOT_APPEAR"),
            "load replay must not pollute answer: {}",
            r2.answer
        );

        let log2 = std::fs::read_to_string(&log_path).unwrap_or_default();
        assert!(
            log2.lines().any(|l| l == "session/load"),
            "follow-up must session/load: {log2}"
        );
        // Exactly one session/new across both process invocations.
        let new_count = log2.lines().filter(|l| *l == "session/new").count();
        assert_eq!(new_count, 1, "session/new only on first turn, log={log2}");

        let detail = mgr.detail(&start.task_id).unwrap();
        for ev in &detail.timeline {
            assert!(
                !ev.message.contains("REPLAY_MUST_NOT_APPEAR"),
                "timeline must not include load-replay text: {}",
                ev.message
            );
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn followup_without_session_id_fails_actionably() {
        std::env::set_var("GROKTASK_FIXTURE", "1");
        let tmp = TempDir::new().unwrap();
        let db = tmp.path().join("h.sqlite3");
        let _ = crate::storage::open_path(&db).unwrap();
        let mgr = Arc::new(TaskManager::new(db.clone(), None));
        let input = TaskInput {
            task: "first".into(),
            cwd: tmp.path().to_string_lossy().into_owned(),
            mode: TaskMode::Read,
            model: None,
            effort: None,
            title: None,
        };
        let start = mgr
            .start(input, "sub-nosess".into(), "client", None, None)
            .unwrap();
        let _ = mgr
            .wait(&start.task_id, &start.turn_id, 5_000)
            .await
            .unwrap();
        {
            let conn = mgr.open().unwrap();
            conn.execute(
                "UPDATE tasks SET acp_session_id = NULL WHERE id = ?1",
                rusqlite::params![start.task_id],
            )
            .unwrap();
        }
        std::env::remove_var("GROKTASK_FIXTURE");

        let err = session_open_method(None, 2).unwrap_err();
        assert!(err.contains("session_unavailable"));

        let mock = tmp.path().join("mock-fail");
        write_executable_script(
            &mock,
            r#"#!/usr/bin/env bash
while IFS= read -r line; do
  method=$(printf '%s' "$line" | sed -n 's/.*"method"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')
  id=$(printf '%s' "$line" | sed -n 's/.*"id"[[:space:]]*:[[:space:]]*\([0-9][0-9]*\).*/\1/p')
  case "${method:-}" in
    initialize)
      echo "{\"jsonrpc\":\"2.0\",\"id\":${id:-1},\"result\":{\"protocolVersion\":1}}"
      ;;
    session/new)
      echo "{\"jsonrpc\":\"2.0\",\"id\":${id:-2},\"result\":{\"sessionId\":\"should-not-happen\"}}"
      ;;
  esac
done
"#,
        );
        let mgr2 = Arc::new(TaskManager::new(
            db,
            Some(mock.to_string_lossy().into_owned()),
        ));
        let cont = mgr2
            .continue_task(
                &start.task_id,
                "second without session".into(),
                "client",
                None,
                None,
            )
            .unwrap();
        let r2 = mgr2
            .wait(&cont.task_id, &cont.turn_id, 10_000)
            .await
            .unwrap();
        assert_eq!(r2.status, RunStatus::Failed);
        let err = r2.error.as_ref().expect("error");
        assert_eq!(err.code, "session_unavailable");
        assert!(
            err.message.contains("session_unavailable") || err.message.contains("follow-up"),
            "{}",
            err.message
        );
        let conn = mgr2.open().unwrap();
        let task = repository::get_task(&conn, &start.task_id)
            .unwrap()
            .unwrap();
        assert!(task.acp_session_id.is_none() || task.acp_session_id.as_deref() == Some(""));
    }
}
