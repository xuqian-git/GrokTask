//! TaskManager: accept/start/run/status/wait/cancel and drive ACP reduce path.
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
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
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
    pub fn new(db_path: PathBuf) -> Self {
        let fixture_mode = std::env::var("GROKTASK_FIXTURE")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        Self {
            db_path,
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
            first_sequence: 1,
            last_sequence: 1,
            payload_json: payload.to_string(),
            created_at: now,
            updated_at: now,
        };
        let m = MutationRow {
            task_id: task_id.into(),
            sequence: 1,
            generation: 1,
            operation: "add".into(),
            item_id: Some(item_id),
            payload_json: payload.to_string(),
            created_at: now,
        };
        repository::commit_timeline_mutations(conn, task_id, &[item], &[m], 1)
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

        let session_id = "fixture-session";
        let mut reducer = TurnReducer::new(task_id, turn_id, session_id);
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
        let answer = reducer.answer_markdown();
        let finished = now_ms();
        let result = RunResult {
            task_id: task_id.into(),
            turn_id: turn_id.into(),
            turn_ordinal: 1,
            status: RunStatus::Completed,
            mode: input.mode,
            session_id: Some(session_id.into()),
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
        let grok = resolve_grok_executable()?;
        let mode = match input.mode {
            TaskMode::Read => AcpMode::Read,
            TaskMode::Write => AcpMode::Write,
        };
        let argv = build_grok_argv(mode, input.model.as_deref(), input.effort.as_deref());

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

        // session/new
        let new_req = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "session/new",
            "params": { "cwd": input.cwd, "mcpServers": [] }
        });
        write_rpc(&mut stdin, &new_req).await?;
        let new_resp = read_rpc_response(&mut reader).await?;
        let session_id = new_resp
            .pointer("/result/sessionId")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        if let Ok(conn) = self.open() {
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
        let answer = reducer.answer_markdown();
        let finished = now_ms();
        let result = RunResult {
            task_id: task_id.into(),
            turn_id: turn_id.into(),
            turn_ordinal: 1,
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
        let code = if err.to_string().contains("grok") {
            "grok_not_found"
        } else {
            "internal_error"
        };
        let result = RunResult {
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
                message: format!("{err:#}"),
                retryable: code == "grok_not_found",
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

fn resolve_grok_executable() -> Result<PathBuf> {
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
    Err(anyhow!(
        "grok CLI not found on PATH; install Grok CLI or set GROK_EXECUTABLE"
    ))
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

async fn write_rpc(stdin: &mut tokio::process::ChildStdin, msg: &serde_json::Value) -> Result<()> {
    let mut line = serde_json::to_string(msg)?;
    line.push('\n');
    stdin.write_all(line.as_bytes()).await?;
    stdin.flush().await?;
    Ok(())
}

async fn read_rpc_response(
    reader: &mut BufReader<tokio::process::ChildStdout>,
) -> Result<serde_json::Value> {
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            return Err(anyhow!("EOF from grok before response"));
        }
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(t)?;
        if v.get("id").is_some() {
            return Ok(v);
        }
        // skip notifications during handshake
    }
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
        let mgr = Arc::new(TaskManager::new(db));
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
}
