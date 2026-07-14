//! Shared CLI/MCP/daemon DTOs for task lifecycle results.
//! Single source of truth for structured JSON shapes (cli-mcp.md).

use serde::{Deserialize, Serialize};

/// Explicit task mode — never inferred, never defaulted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskMode {
    Read,
    Write,
}

impl TaskMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "read" => Some(Self::Read),
            "write" => Some(Self::Write),
            _ => None,
        }
    }
}

impl std::fmt::Display for TaskMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Canonical task container status (tasks.status).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskContainerStatus {
    Queued,
    Starting,
    Running,
    Cancelling,
    Recovering,
    Idle,
    Cancelled,
    Failed,
    Interrupted,
}

impl TaskContainerStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Cancelling => "cancelling",
            Self::Recovering => "recovering",
            Self::Idle => "idle",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "queued" => Some(Self::Queued),
            "starting" => Some(Self::Starting),
            "running" => Some(Self::Running),
            "cancelling" => Some(Self::Cancelling),
            "recovering" => Some(Self::Recovering),
            "idle" => Some(Self::Idle),
            "cancelled" => Some(Self::Cancelled),
            "failed" => Some(Self::Failed),
            "interrupted" => Some(Self::Interrupted),
            _ => None,
        }
    }

    pub fn is_active(self) -> bool {
        matches!(
            self,
            Self::Queued | Self::Starting | Self::Running | Self::Cancelling | Self::Recovering
        )
    }
}

/// Turn terminal / run outcome status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnOutcome {
    Completed,
    Partial,
    Refused,
    Cancelled,
    Failed,
}

impl TurnOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Partial => "partial",
            Self::Refused => "refused",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }
}

/// Top-level run/wait status field (MCP RunResult.status).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Completed,
    Cancelled,
    Failed,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ErrorInfo {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

/// Immutable turn result returned by run/wait/cancel.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RunResult {
    pub task_id: String,
    pub turn_id: String,
    pub turn_ordinal: u32,
    pub status: RunStatus,
    pub mode: TaskMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    pub turn_outcome: TurnOutcome,
    pub partial: bool,
    pub answer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorInfo>,
    pub started_at: String,
    pub finished_at: String,
    pub duration_ms: u64,
}

/// Task status snapshot (status tool / CLI status).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskStatus {
    pub task_id: String,
    pub status: TaskContainerStatus,
    pub mode: TaskMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_recovery_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_turn_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_step: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer_preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorInfo>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
}

/// Accepted async start response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StartResult {
    pub submission_id: String,
    pub task_id: String,
    pub turn_id: String,
    pub turn_ordinal: u32,
    pub status: String,
    pub mode: TaskMode,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_deleted: Option<bool>,
}

/// Wait timeout payload (not an MCP protocol error).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WaitTimeout {
    pub task_id: String,
    pub turn_id: String,
    pub timed_out: bool,
    pub status: TaskContainerStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_step: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_action: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TurnCancelResult {
    pub target: String,
    pub task_id: String,
    pub turn_id: String,
    pub task_status: TaskContainerStatus,
    pub already_terminal: bool,
    pub result: RunResult,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryCancelResult {
    pub target: String,
    pub task_id: String,
    pub recovery_id: String,
    pub task_status: TaskContainerStatus,
    pub already_terminal: bool,
    pub recovery_status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorInfo>,
}

/// Task creation input (run/start).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskInput {
    pub task: String,
    pub cwd: String,
    pub mode: TaskMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StartInput {
    #[serde(flatten)]
    pub task: TaskInput,
    pub submission_id: String,
}

/// Validation limits from cli-mcp.md.
pub const MAX_TASK_BYTES: usize = 200_000;
pub const MAX_MODEL_BYTES: usize = 128;
pub const MAX_EFFORT_BYTES: usize = 64;
pub const MAX_TITLE_CHARS: usize = 160;
pub const DEFAULT_WAIT_TIMEOUT_MS: u64 = 30_000;
pub const MAX_WAIT_TIMEOUT_MS: u64 = 300_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    pub code: String,
    pub message: String,
}

impl ValidationError {
    pub fn invalid(msg: impl Into<String>) -> Self {
        Self {
            code: "invalid_params".into(),
            message: msg.into(),
        }
    }
}

/// Validate and normalize task input. cwd must be absolute and exist as a directory.
pub fn validate_task_input(
    task: &str,
    cwd: &str,
    mode: &str,
    model: Option<&str>,
    effort: Option<&str>,
    title: Option<&str>,
) -> Result<TaskInput, ValidationError> {
    let mode = TaskMode::parse(mode).ok_or_else(|| {
        ValidationError::invalid("mode must be explicit `read` or `write` (no default)")
    })?;

    let task = task.trim();
    if task.is_empty() {
        return Err(ValidationError::invalid(
            "task must be non-empty after trim",
        ));
    }
    if task.len() > MAX_TASK_BYTES {
        return Err(ValidationError::invalid(format!(
            "task exceeds {MAX_TASK_BYTES} UTF-8 bytes"
        )));
    }

    let cwd_path = std::path::Path::new(cwd);
    if !cwd_path.is_absolute() {
        return Err(ValidationError::invalid(
            "cwd must be an absolute path that exists as a directory",
        ));
    }
    if !cwd_path.is_dir() {
        return Err(ValidationError::invalid(format!(
            "cwd does not exist or is not a directory: {cwd}"
        )));
    }
    let cwd = cwd_path
        .canonicalize()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| cwd.to_string());

    let model = match model {
        None => None,
        Some(m) => {
            let m = m.trim();
            if m.is_empty() {
                return Err(ValidationError::invalid("model must be non-empty when set"));
            }
            if m.len() > MAX_MODEL_BYTES {
                return Err(ValidationError::invalid(format!(
                    "model exceeds {MAX_MODEL_BYTES} bytes"
                )));
            }
            Some(m.to_string())
        }
    };

    let effort = match effort {
        None => None,
        Some(e) => {
            let e = e.trim();
            if e.is_empty() {
                return Err(ValidationError::invalid(
                    "effort must be non-empty when set",
                ));
            }
            if e.len() > MAX_EFFORT_BYTES {
                return Err(ValidationError::invalid(format!(
                    "effort exceeds {MAX_EFFORT_BYTES} bytes"
                )));
            }
            Some(e.to_string())
        }
    };

    let title = match title {
        None => None,
        Some(t) => {
            let t = t.trim();
            if t.is_empty() {
                None
            } else if t.chars().count() > MAX_TITLE_CHARS {
                return Err(ValidationError::invalid(format!(
                    "title exceeds {MAX_TITLE_CHARS} characters"
                )));
            } else {
                Some(t.to_string())
            }
        }
    };

    Ok(TaskInput {
        task: task.to_string(),
        cwd,
        mode,
        model,
        effort,
        title,
    })
}

pub fn validate_submission_id(id: &str) -> Result<String, ValidationError> {
    let id = id.trim();
    if id.is_empty() {
        return Err(ValidationError::invalid(
            "submissionId is required for start (caller-generated UUID)",
        ));
    }
    // Accept any non-empty opaque id that looks like a UUID or stable key.
    if id.len() > 128 {
        return Err(ValidationError::invalid("submissionId is too long"));
    }
    Ok(id.to_string())
}

pub fn validate_uuid_like(id: &str, field: &str) -> Result<String, ValidationError> {
    let id = id.trim();
    if id.is_empty() {
        return Err(ValidationError::invalid(format!("{field} is required")));
    }
    if id.len() > 128 {
        return Err(ValidationError::invalid(format!("{field} is too long")));
    }
    Ok(id.to_string())
}

/// Canonical input hash for start dedupe (excludes submissionId).
pub fn task_input_hash(input: &TaskInput) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(input.task.as_bytes());
    hasher.update(b"\0");
    hasher.update(input.cwd.as_bytes());
    hasher.update(b"\0");
    hasher.update(input.mode.as_str().as_bytes());
    hasher.update(b"\0");
    if let Some(m) = &input.model {
        hasher.update(m.as_bytes());
    }
    hasher.update(b"\0");
    if let Some(e) = &input.effort {
        hasher.update(e.as_bytes());
    }
    hasher.update(b"\0");
    if let Some(t) = &input.title {
        hasher.update(t.as_bytes());
    }
    hex::encode(hasher.finalize())
}

pub fn ms_to_rfc3339(ms: i64) -> String {
    use chrono::{TimeZone, Utc};
    Utc.timestamp_millis_opt(ms)
        .single()
        .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_else(|| "1970-01-01T00:00:00.000Z".into())
}

pub fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Map stopReason + optional terminationCause to turn outcome and run status.
pub fn map_stop_to_outcome(
    stop_reason: Option<&str>,
    termination_cause: Option<&str>,
) -> (TurnOutcome, RunStatus, bool, Option<ErrorInfo>) {
    // terminationCause takes precedence over ACP cancelled (acp-runtime §3.2).
    if let Some(cause) = termination_cause {
        match cause {
            "user_cancel" | "mcp_cancel" | "client_disconnect" | "restart_force" => {
                return (TurnOutcome::Cancelled, RunStatus::Cancelled, false, None);
            }
            "read_mode_violation" => {
                return (
                    TurnOutcome::Failed,
                    RunStatus::Failed,
                    false,
                    Some(ErrorInfo {
                        code: "read_mode_violation".into(),
                        message: "Grok requested a write permission in read mode; task stopped. Re-run with --mode write if writes are intended.".into(),
                        retryable: false,
                    }),
                );
            }
            "permission_unavailable" => {
                return (
                    TurnOutcome::Failed,
                    RunStatus::Failed,
                    false,
                    Some(ErrorInfo {
                        code: "permission_unavailable".into(),
                        message: "No allow_once permission option available for write mode.".into(),
                        retryable: false,
                    }),
                );
            }
            "hard_timeout" => {
                return (
                    TurnOutcome::Failed,
                    RunStatus::Failed,
                    false,
                    Some(ErrorInfo {
                        code: "task_timeout".into(),
                        message: "Task hit the hard runtime limit and was terminated.".into(),
                        retryable: true,
                    }),
                );
            }
            "cancel_timeout" => {
                return (
                    TurnOutcome::Failed,
                    RunStatus::Failed,
                    false,
                    Some(ErrorInfo {
                        code: "cancel_timeout".into(),
                        message: "Cancel could not confirm process tree exit in time.".into(),
                        retryable: false,
                    }),
                );
            }
            _ => {}
        }
    }

    match stop_reason.unwrap_or("") {
        "end_turn" => (TurnOutcome::Completed, RunStatus::Completed, false, None),
        "max_tokens" | "max_turn_requests" => {
            (TurnOutcome::Partial, RunStatus::Completed, true, None)
        }
        "refusal" => (
            TurnOutcome::Refused,
            RunStatus::Failed,
            false,
            Some(ErrorInfo {
                code: "agent_refusal".into(),
                message: "Grok refused the request. Rephrase and retry if appropriate.".into(),
                retryable: false,
            }),
        ),
        "cancelled" => (TurnOutcome::Cancelled, RunStatus::Cancelled, false, None),
        other if !other.is_empty() => (
            TurnOutcome::Failed,
            RunStatus::Failed,
            false,
            Some(ErrorInfo {
                code: "unexpected_stop_reason".into(),
                message: format!("Unexpected ACP stopReason `{other}`."),
                retryable: false,
            }),
        ),
        _ => (
            TurnOutcome::Failed,
            RunStatus::Failed,
            false,
            Some(ErrorInfo {
                code: "internal_error".into(),
                message: "Turn ended without stopReason.".into(),
                retryable: false,
            }),
        ),
    }
}

/// Build short human title from prompt.
pub fn default_title(prompt: &str) -> String {
    let line = prompt
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or(prompt);
    let mut t = String::new();
    for (i, ch) in line.chars().enumerate() {
        if i >= 80 {
            t.push('…');
            break;
        }
        t.push(ch);
    }
    let t = t.trim();
    if t.is_empty() {
        "Untitled task".into()
    } else {
        t.to_string()
    }
}

// ---------------------------------------------------------------------------
// Conversation / task list DTOs (shared by CLI, MCP, GUI)
// ---------------------------------------------------------------------------

/// Compact task row for history lists.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskListItem {
    pub task_id: String,
    pub title: String,
    pub cwd: String,
    pub mode: TaskMode,
    pub status: TaskContainerStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_action: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
}

/// Full task detail including timeline snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskDetail {
    pub task: TaskStatus,
    pub title: String,
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_model: Option<String>,
    #[serde(default)]
    pub timeline: Vec<TimelineEventDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_plan: Option<PlanDto>,
    pub last_sequence: i64,
    pub timeline_generation: i64,
}

/// One semantic timeline event for UI/CLI (never raw ACP JSON as primary).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TimelineEventDto {
    pub item_id: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    /// Human-meaningful primary line.
    pub message: String,
    #[serde(default)]
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default)]
    pub streaming: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer_mark: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub locations: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_entries: Option<Vec<PlanEntryDto>>,
    pub first_sequence: i64,
    pub last_sequence: i64,
    /// Redacted diagnostic only when diagnostics are enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlanEntryDto {
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlanDto {
    pub item_id: String,
    pub entries: Vec<PlanEntryDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_step: Option<String>,
}

/// Streaming reply fragment (for live UI updates).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReplyFragmentDto {
    pub item_id: String,
    pub kind: String, // reasoning_segment | assistant_segment
    pub text: String,
    pub streaming: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage_title: Option<String>,
}

/// Convert a reducer timeline item JSON payload into a DTO.
pub fn timeline_item_from_payload(
    item_id: &str,
    kind: &str,
    turn_id: Option<&str>,
    payload: &serde_json::Value,
    first_sequence: i64,
    last_sequence: i64,
) -> TimelineEventDto {
    let message = payload
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let text = payload
        .get("text")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let plan_entries = payload
        .get("planEntries")
        .and_then(|v| serde_json::from_value::<Vec<PlanEntryDto>>(v.clone()).ok());
    let locations = payload
        .get("locations")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    TimelineEventDto {
        item_id: item_id.into(),
        kind: kind.into(),
        turn_id: turn_id.map(str::to_string),
        message,
        text,
        title: payload
            .get("title")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        status: payload
            .get("status")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        streaming: payload
            .get("streaming")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        answer_mark: payload
            .get("answerMark")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        stage_title: payload
            .get("stageTitle")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        tool_kind: payload
            .get("toolKind")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        locations,
        plan_entries,
        first_sequence,
        last_sequence,
        diagnostic: None, // never default primary path
    }
}

/// MCP text content summary for run results.
pub fn run_result_text_summary(result: &RunResult) -> String {
    match result.status {
        RunStatus::Completed if result.partial => {
            format!("部分结果（达到 token/turn 上限）\n\n{}", result.answer)
        }
        RunStatus::Completed => {
            if result.answer.is_empty() {
                format!(
                    "completed taskId={} turnId={}",
                    result.task_id, result.turn_id
                )
            } else {
                result.answer.clone()
            }
        }
        RunStatus::Cancelled => format!(
            "cancelled taskId={} turnId={} stopReason={}",
            result.task_id,
            result.turn_id,
            result.stop_reason.as_deref().unwrap_or("cancelled")
        ),
        RunStatus::Failed => {
            let err = result
                .error
                .as_ref()
                .map(|e| format!("{}: {}", e.code, e.message))
                .unwrap_or_else(|| "failed".into());
            format!(
                "failed taskId={} turnId={} — {err}",
                result.task_id, result.turn_id
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn mode_required_no_default() {
        let err = validate_task_input("hi", "/tmp", "auto", None, None, None).unwrap_err();
        assert!(err.message.contains("read") || err.message.contains("write"));
    }

    #[test]
    fn cwd_must_be_absolute_dir() {
        let tmp = TempDir::new().unwrap();
        let ok = validate_task_input("hi", tmp.path().to_str().unwrap(), "read", None, None, None)
            .unwrap();
        assert_eq!(ok.mode, TaskMode::Read);

        let err = validate_task_input("hi", "relative/path", "read", None, None, None).unwrap_err();
        assert!(err.message.contains("absolute"));
    }

    #[test]
    fn termination_cause_beats_cancelled() {
        let (o, s, partial, err) = map_stop_to_outcome(Some("cancelled"), Some("hard_timeout"));
        assert_eq!(o, TurnOutcome::Failed);
        assert_eq!(s, RunStatus::Failed);
        assert!(!partial);
        assert_eq!(err.unwrap().code, "task_timeout");
    }

    #[test]
    fn end_turn_completed() {
        let (o, s, partial, err) = map_stop_to_outcome(Some("end_turn"), None);
        assert_eq!(o, TurnOutcome::Completed);
        assert_eq!(s, RunStatus::Completed);
        assert!(!partial);
        assert!(err.is_none());
    }

    #[test]
    fn partial_answer_summary() {
        let r = RunResult {
            task_id: "t".into(),
            turn_id: "u".into(),
            turn_ordinal: 1,
            status: RunStatus::Completed,
            mode: TaskMode::Read,
            session_id: None,
            requested_model: None,
            actual_model: None,
            stop_reason: Some("max_tokens".into()),
            turn_outcome: TurnOutcome::Partial,
            partial: true,
            answer: "half".into(),
            error: None,
            started_at: "a".into(),
            finished_at: "b".into(),
            duration_ms: 1,
        };
        let text = run_result_text_summary(&r);
        assert!(text.starts_with("部分结果"));
        assert!(text.contains("half"));
    }

    #[test]
    fn input_hash_stable() {
        let a = TaskInput {
            task: "x".into(),
            cwd: "/tmp".into(),
            mode: TaskMode::Read,
            model: None,
            effort: None,
            title: None,
        };
        let b = a.clone();
        assert_eq!(task_input_hash(&a), task_input_hash(&b));
    }
}
