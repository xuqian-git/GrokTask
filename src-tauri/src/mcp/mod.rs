//! MCP stdio server: tools run/start/status/wait/cancel.
//!
//! Stdout is reserved for MCP JSON-RPC framing. Logs go to stderr only.
//! Never initializes Tauri/WebView.

use crate::cli::eprint_line;
use crate::dto::{
    run_result_text_summary, validate_submission_id, validate_task_input, validate_uuid_like,
    RunResult, StartResult, TaskStatus, TurnCancelResult, WaitTimeout, DEFAULT_WAIT_TIMEOUT_MS,
    MAX_TASK_BYTES, MAX_WAIT_TIMEOUT_MS,
};
use crate::ipc::client::{self, unwrap_result};
use crate::ipc::protocol::ClientRole;
use crate::version::{APP_VERSION, PRODUCT_NAME};
use serde_json::{json, Value};
use std::io::{BufRead, Write};

/// Run MCP server on stdio until EOF.
pub fn run_stdio() -> ! {
    eprint_line(&format!(
        "{PRODUCT_NAME} mcp {APP_VERSION}: listening on stdio"
    ));
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                eprint_line(&format!("mcp stdin error: {e}"));
                break;
            }
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                eprint_line(&format!("mcp parse error: {e}"));
                continue;
            }
        };
        // Notifications have no id
        if msg.get("id").is_none() && msg.get("method").is_some() {
            handle_notification(&msg);
            continue;
        }
        if let Some(resp) = handle_message(&msg) {
            if let Ok(s) = serde_json::to_string(&resp) {
                let _ = writeln!(stdout, "{s}");
                let _ = stdout.flush();
            }
        }
    }
    std::process::exit(0);
}

fn handle_notification(msg: &Value) {
    let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
    if method == "notifications/cancelled" {
        // Best-effort: cancel bound run if params include request id — full binding is Phase 3+ daemon map.
        // Here we only log; daemon owns turn lifecycle for start; run disconnect cancels via connection.
        eprint_line("mcp: notifications/cancelled received");
    }
}

fn handle_message(msg: &Value) -> Option<Value> {
    let id = msg.get("id").cloned().unwrap_or(Value::Null);
    let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = msg.get("params").cloned().unwrap_or(Value::Null);

    match method {
        "initialize" => Some(rpc_result(
            id,
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "groktask", "version": APP_VERSION }
            }),
        )),
        "notifications/initialized" | "initialized" => None,
        "ping" => Some(rpc_result(id, json!({}))),
        "tools/list" => Some(rpc_result(id, json!({ "tools": tool_defs() }))),
        "tools/call" => {
            let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            match call_tool(name, &arguments) {
                Ok(result) => Some(rpc_result(id, result)),
                Err(e) => Some(rpc_error(id, -32602, e)),
            }
        }
        "" => Some(rpc_error(id, -32600, "missing method")),
        other => Some(rpc_error(id, -32601, format!("method not found: {other}"))),
    }
}

fn rpc_result(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn rpc_error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message.into() }
    })
}

fn tool_result_text(text: String, structured: Value) -> Value {
    json!({
        "content": [{ "type": "text", "text": text }],
        "structuredContent": structured,
        "isError": false
    })
}

/// Public tool schemas for tests and tools/list.
pub fn tool_defs() -> Vec<Value> {
    vec![
        json!({
            "name": "run",
            "description": "delegate a planned coding implementation, file modification, test-writing, or fix-implementation task to external xAI Grok Build and block until the turn finishes. Use after the host agent has completed planning/analysis and has concrete acceptance criteria. mode must be explicit read|write (write may modify cwd). Progress is persisted locally. Prefer start for long background code work.",
            "inputSchema": task_input_schema(false)
        }),
        json!({
            "name": "start",
            "description": "delegate a long-running planned coding implementation, file modification, test-writing, or fix-implementation task to external xAI Grok Build and return immediately with taskId+turnId. Use after the host agent has completed planning/analysis and should monitor the resulting code changes. Requires caller-generated submissionId (UUID) for exactly-once retry. mode must be explicit read|write. Disconnect does not cancel the task — use cancel.",
            "inputSchema": task_input_schema(true)
        }),
        json!({
            "name": "status",
            "description": "Snapshot status for a delegated Grok taskId returned by run/start. Non-blocking; does not return full transcript.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "taskId": { "type": "string", "description": "Task id from run/start" }
                },
                "required": ["taskId"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "wait",
            "description": "Wait for a specific delegated Grok turn (taskId, turnId) to finish. Returns immutable RunResult or timedOut snapshot. Always pass the original turnId from start/run.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "taskId": { "type": "string" },
                    "turnId": { "type": "string" },
                    "timeoutMs": { "type": "integer", "minimum": 0, "maximum": 300000 }
                },
                "required": ["taskId", "turnId"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "cancel",
            "description": "Cancel a delegated Grok turn (taskId+turnId) or recovery (taskId+recoveryId). Idempotent for terminal turns. Does not affect later turns on the same task.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "taskId": { "type": "string" },
                    "turnId": { "type": "string" },
                    "recoveryId": { "type": "string" }
                },
                "required": ["taskId"],
                "additionalProperties": false
            }
        }),
    ]
}

fn task_input_schema(require_submission: bool) -> Value {
    let mut required = vec!["task", "cwd", "mode"];
    if require_submission {
        required.push("submissionId");
    }
    json!({
        "type": "object",
        "properties": {
            "task": { "type": "string", "maxLength": MAX_TASK_BYTES },
            "cwd": { "type": "string", "description": "Absolute existing directory" },
            "mode": { "type": "string", "enum": ["read", "write"] },
            "model": { "type": "string", "maxLength": 128 },
            "effort": { "type": "string", "maxLength": 64 },
            "title": { "type": "string", "maxLength": 160 },
            "submissionId": { "type": "string", "description": "Caller UUID for start dedupe" }
        },
        "required": required,
        "additionalProperties": false
    })
}

fn call_tool(name: &str, args: &Value) -> Result<Value, String> {
    match name {
        "run" => tool_run(args),
        "start" => tool_start(args),
        "status" => tool_status(args),
        "wait" => tool_wait(args),
        "cancel" => tool_cancel(args),
        other => Err(format!("unknown tool `{other}`")),
    }
}

fn parse_task_args(args: &Value) -> Result<crate::dto::TaskInput, String> {
    let task = args
        .get("task")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "task is required".to_string())?;
    let cwd = args
        .get("cwd")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "cwd is required".to_string())?;
    let mode = args
        .get("mode")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "mode must be explicit `read` or `write`".to_string())?;
    let model = args.get("model").and_then(|v| v.as_str());
    let effort = args.get("effort").and_then(|v| v.as_str());
    let title = args.get("title").and_then(|v| v.as_str());
    validate_task_input(task, cwd, mode, model, effort, title).map_err(|e| e.message)
}

fn tool_run(args: &Value) -> Result<Value, String> {
    let input = parse_task_args(args)?;
    let params = json!({
        "task": input.task,
        "cwd": input.cwd,
        "mode": input.mode.as_str(),
        "model": input.model,
        "effort": input.effort,
        "title": input.title,
    });
    let resp = client::request_blocking(ClientRole::Mcp, "task.run", params)
        .map_err(|e| format!("daemon IPC: {e:#}"))?;
    let v = unwrap_result(resp).map_err(|e| e.to_string())?;
    let result: RunResult = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
    Ok(tool_result_text(run_result_text_summary(&result), v))
}

fn tool_start(args: &Value) -> Result<Value, String> {
    let input = parse_task_args(args)?;
    let submission_id = args
        .get("submissionId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "submissionId is required for start".to_string())?;
    let submission_id = validate_submission_id(submission_id).map_err(|e| e.message)?;
    let params = json!({
        "task": input.task,
        "cwd": input.cwd,
        "mode": input.mode.as_str(),
        "model": input.model,
        "effort": input.effort,
        "title": input.title,
        "submissionId": submission_id,
    });
    let resp = client::request_blocking(ClientRole::Mcp, "task.start", params)
        .map_err(|e| format!("daemon IPC: {e:#}"))?;
    let v = unwrap_result(resp).map_err(|e| e.to_string())?;
    let start: StartResult = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
    let text = format!(
        "started taskId={} turnId={} status={}",
        start.task_id, start.turn_id, start.status
    );
    Ok(tool_result_text(text, v))
}

fn tool_status(args: &Value) -> Result<Value, String> {
    let task_id = args
        .get("taskId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "taskId is required".to_string())?;
    let task_id = validate_uuid_like(task_id, "taskId").map_err(|e| e.message)?;
    let resp =
        client::request_blocking(ClientRole::Mcp, "task.status", json!({ "taskId": task_id }))
            .map_err(|e| format!("daemon IPC: {e:#}"))?;
    let v = unwrap_result(resp).map_err(|e| e.to_string())?;
    let s: TaskStatus = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
    let text = format!(
        "taskId={} status={} mode={} action={}",
        s.task_id,
        s.status.as_str(),
        s.mode.as_str(),
        s.latest_action.as_deref().unwrap_or("-")
    );
    Ok(tool_result_text(text, v))
}

fn tool_wait(args: &Value) -> Result<Value, String> {
    let task_id = args
        .get("taskId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "taskId is required".to_string())?;
    let turn_id = args
        .get("turnId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "turnId is required".to_string())?;
    let task_id = validate_uuid_like(task_id, "taskId").map_err(|e| e.message)?;
    let turn_id = validate_uuid_like(turn_id, "turnId").map_err(|e| e.message)?;
    let timeout_ms = args
        .get("timeoutMs")
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_WAIT_TIMEOUT_MS)
        .min(MAX_WAIT_TIMEOUT_MS);
    let resp = client::request_blocking(
        ClientRole::Mcp,
        "task.wait",
        json!({
            "taskId": task_id,
            "turnId": turn_id,
            "timeoutMs": timeout_ms,
        }),
    )
    .map_err(|e| format!("daemon IPC: {e:#}"))?;
    let v = unwrap_result(resp).map_err(|e| e.to_string())?;
    if v.get("timedOut").and_then(|t| t.as_bool()) == Some(true) {
        let w: WaitTimeout = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
        let text = format!(
            "timedOut taskId={} turnId={} status={}",
            w.task_id,
            w.turn_id,
            w.status.as_str()
        );
        return Ok(tool_result_text(text, v));
    }
    let result: RunResult = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
    Ok(tool_result_text(run_result_text_summary(&result), v))
}

fn tool_cancel(args: &Value) -> Result<Value, String> {
    let task_id = args
        .get("taskId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "taskId is required".to_string())?;
    let task_id = validate_uuid_like(task_id, "taskId").map_err(|e| e.message)?;
    let params = if let Some(turn_id) = args.get("turnId").and_then(|v| v.as_str()) {
        let turn_id = validate_uuid_like(turn_id, "turnId").map_err(|e| e.message)?;
        json!({ "taskId": task_id, "turnId": turn_id })
    } else if let Some(recovery_id) = args.get("recoveryId").and_then(|v| v.as_str()) {
        json!({ "taskId": task_id, "recoveryId": recovery_id })
    } else {
        return Err("turnId or recoveryId is required".into());
    };
    let resp = client::request_blocking(ClientRole::Mcp, "task.cancel", params)
        .map_err(|e| format!("daemon IPC: {e:#}"))?;
    let v = unwrap_result(resp).map_err(|e| e.to_string())?;
    if let Ok(c) = serde_json::from_value::<TurnCancelResult>(v.clone()) {
        let text = format!(
            "cancel turnId={} alreadyTerminal={} taskStatus={}",
            c.turn_id,
            c.already_terminal,
            c.task_status.as_str()
        );
        return Ok(tool_result_text(text, v));
    }
    Ok(tool_result_text("cancel accepted".into(), v))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_list_has_exactly_five() {
        let tools = tool_defs();
        assert_eq!(tools.len(), 5);
        let names: Vec<_> = tools
            .iter()
            .filter_map(|t| t.get("name").and_then(|n| n.as_str()))
            .collect();
        assert_eq!(names, vec!["run", "start", "status", "wait", "cancel"]);
    }

    #[test]
    fn start_schema_requires_submission_id() {
        let tools = tool_defs();
        let start = tools.iter().find(|t| t["name"] == "start").unwrap();
        let req = start["inputSchema"]["required"].as_array().unwrap();
        assert!(req.iter().any(|v| v == "submissionId"));
        assert!(req.iter().any(|v| v == "mode"));
        assert!(req.iter().any(|v| v == "cwd"));
        assert!(req.iter().any(|v| v == "task"));
    }

    #[test]
    fn run_description_mentions_external_grok_and_mode() {
        let tools = tool_defs();
        let run = tools.iter().find(|t| t["name"] == "run").unwrap();
        let d = run["description"].as_str().unwrap();
        assert!(d.contains("xAI Grok") || d.contains("Grok"));
        assert!(d.contains("read") && d.contains("write"));
        assert!(d.contains("delegate") || d.contains("委派"));
        assert!(d.contains("implementation") || d.contains("实现"));
        assert!(!d.contains("debugging"));
        assert!(!d.contains("review"));
        let start = tools.iter().find(|t| t["name"] == "start").unwrap();
        let start_d = start["description"].as_str().unwrap();
        assert!(start_d.contains("long") || start_d.contains("background"));
        assert!(start_d.contains("delegate") || start_d.contains("委派"));
        assert!(!start_d.contains("debugging"));
        assert!(!start_d.contains("review"));
    }

    #[test]
    fn parse_task_args_rejects_missing_mode() {
        let err = parse_task_args(&json!({"task":"hi","cwd":"/tmp"})).unwrap_err();
        assert!(err.contains("mode"));
    }
}
