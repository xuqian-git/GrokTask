//! ACP JSON-RPC and normalized session-update types.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Raw JSON-RPC message kinds we care about on the ACP wire.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum JsonRpcMessage {
    Request(JsonRpcRequest),
    Response(JsonRpcResponse),
    Notification(JsonRpcNotification),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Value,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// Normalized internal update after parsing a `session/update` notification.
#[derive(Debug, Clone, PartialEq)]
pub enum NormalizedUpdate {
    AgentThought {
        text: String,
        message_id: Option<String>,
        meta: Option<Value>,
    },
    AgentMessage {
        text: String,
        message_id: Option<String>,
        meta: Option<Value>,
    },
    UserMessage {
        text: String,
        message_id: Option<String>,
        meta: Option<Value>,
    },
    ToolCall {
        tool_call_id: String,
        title: Option<String>,
        kind: Option<String>,
        status: Option<String>,
        content_text: Option<String>,
        locations: Vec<String>,
        raw: Value,
    },
    ToolCallUpdate {
        tool_call_id: String,
        title: Option<String>,
        kind: Option<String>,
        status: Option<String>,
        content_text: Option<String>,
        locations: Vec<String>,
        raw: Value,
    },
    Plan {
        entries: Vec<PlanEntry>,
        raw: Value,
    },
    Usage {
        raw: Value,
    },
    PermissionRequest {
        request_id: String,
        tool_call_id: Option<String>,
        summary: String,
        options: Vec<PermissionOption>,
        raw: Value,
    },
    CurrentMode {
        mode: Option<String>,
        raw: Value,
    },
    ConfigOption {
        raw: Value,
    },
    SessionInfo {
        session_id: Option<String>,
        model: Option<String>,
        raw: Value,
    },
    AvailableCommands {
        raw: Value,
    },
    /// Recognized xAI extension fields that contribute human-facing stage titles, etc.
    XaiExtension {
        method: String,
        stage_title: Option<String>,
        summary_text: Option<String>,
        event_id: Option<String>,
        is_replay: bool,
        raw: Value,
    },
    /// Unknown / unhandled — diagnostics only.
    DiagnosticOnly {
        method: String,
        reason: String,
        raw: Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlanEntry {
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PermissionOption {
    pub option_id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// Human-facing semantic event ready for timeline / status surfaces.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SemanticAction {
    /// Short verb phrase, e.g. "Reading src/server.ts"
    pub message: String,
    /// Optional category for UI icons.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Never the primary display — redacted raw for diagnostics only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<Value>,
}

impl NormalizedUpdate {
    /// Extract a concise human-meaningful message for status/latestAction.
    pub fn human_message(&self) -> Option<String> {
        match self {
            Self::AgentThought { text, .. } => {
                let t = text.trim();
                if t.is_empty() {
                    Some("Thinking…".into())
                } else {
                    Some(format!("Thinking: {}", first_line_truncated(t, 80)))
                }
            }
            Self::AgentMessage { text, .. } => {
                let t = text.trim();
                if t.is_empty() {
                    Some("Writing reply…".into())
                } else {
                    Some(format!("Replying: {}", first_line_truncated(t, 80)))
                }
            }
            Self::UserMessage { text, .. } => {
                Some(format!("User: {}", first_line_truncated(text.trim(), 80)))
            }
            Self::ToolCall {
                title,
                kind,
                content_text,
                locations,
                status,
                ..
            }
            | Self::ToolCallUpdate {
                title,
                kind,
                content_text,
                locations,
                status,
                ..
            } => Some(tool_human_title(
                title.as_deref(),
                kind.as_deref(),
                content_text.as_deref(),
                locations,
                status.as_deref(),
            )),
            Self::Plan { entries, .. } => {
                let running = entries.iter().find(|e| {
                    e.status
                        .as_deref()
                        .map(|s| s == "in_progress" || s == "running")
                        .unwrap_or(false)
                });
                if let Some(e) = running {
                    Some(format!("Plan: {}", first_line_truncated(&e.content, 80)))
                } else if let Some(e) = entries.first() {
                    Some(format!(
                        "Plan ({} steps): {}",
                        entries.len(),
                        first_line_truncated(&e.content, 60)
                    ))
                } else {
                    Some("Plan updated".into())
                }
            }
            Self::PermissionRequest { summary, .. } => Some(summary.clone()),
            Self::XaiExtension {
                stage_title,
                summary_text,
                ..
            } => stage_title
                .clone()
                .or_else(|| summary_text.clone())
                .map(|s| first_line_truncated(&s, 100)),
            Self::SessionInfo { model, .. } => model.as_ref().map(|m| format!("Model: {m}")),
            Self::Usage { .. }
            | Self::CurrentMode { .. }
            | Self::ConfigOption { .. }
            | Self::AvailableCommands { .. }
            | Self::DiagnosticOnly { .. } => None,
        }
    }
}

pub fn first_line_truncated(s: &str, max_chars: usize) -> String {
    let line = s.lines().next().unwrap_or(s).trim();
    let mut out = String::new();
    for (i, ch) in line.chars().enumerate() {
        if i >= max_chars {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
}

pub fn tool_human_title(
    title: Option<&str>,
    kind: Option<&str>,
    content_text: Option<&str>,
    locations: &[String],
    status: Option<&str>,
) -> String {
    let done = matches!(
        status.map(|s| s.to_ascii_lowercase()).as_deref(),
        Some("completed" | "success" | "done" | "ok")
    );
    let failed = matches!(
        status.map(|s| s.to_ascii_lowercase()).as_deref(),
        Some("failed" | "error")
    );

    if let Some(t) = title.map(str::trim).filter(|t| !t.is_empty()) {
        return t.to_string();
    }
    if let Some(c) = content_text.map(str::trim).filter(|t| !t.is_empty()) {
        return first_line_truncated(c, 100);
    }
    if locations.len() == 1 {
        let path = &locations[0];
        let verb = match kind.map(|k| k.to_ascii_lowercase()).as_deref() {
            Some("read") | Some("read_file") => {
                if done {
                    "Read"
                } else {
                    "Reading"
                }
            }
            Some("edit") | Some("write") | Some("edit_file") => {
                if done {
                    "Modified"
                } else {
                    "Modifying"
                }
            }
            Some("search") | Some("grep") => {
                if done {
                    "Searched"
                } else {
                    "Searching"
                }
            }
            Some("execute") | Some("terminal") | Some("bash") => {
                if done {
                    "Ran"
                } else if failed {
                    "Failed running"
                } else {
                    "Running"
                }
            }
            _ => {
                if done {
                    "Used"
                } else {
                    "Using"
                }
            }
        };
        return format!("{verb} {path}");
    }
    let kind_label = kind.unwrap_or("tool");
    if failed {
        format!("Tool failed ({kind_label})")
    } else if done {
        format!("Used {kind_label}")
    } else {
        format!("Using {kind_label}")
    }
}
