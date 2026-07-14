//! Parse raw ACP JSON-RPC notifications into [`NormalizedUpdate`] values.
//!
//! Raw payloads are never the primary UI display — callers should store them
//! only as redacted diagnostics via [`crate::acp::redact`].

use super::redact::{bound_payload, DEFAULT_DIAGNOSTIC_MAX_BYTES};
use super::types::{
    first_line_truncated, JsonRpcNotification, NormalizedUpdate, PermissionOption, PlanEntry,
};
use serde_json::Value;

/// Parse one NDJSON line of ACP agent output into zero or more normalized updates.
pub fn normalize_line(line: &str) -> Vec<NormalizedUpdate> {
    let line = line.trim();
    if line.is_empty() {
        return Vec::new();
    }
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return vec![NormalizedUpdate::DiagnosticOnly {
            method: "parse_error".into(),
            reason: "invalid JSON line".into(),
            raw: bound_payload(
                &Value::String(line.chars().take(500).collect()),
                DEFAULT_DIAGNOSTIC_MAX_BYTES,
            ),
        }];
    };
    normalize_value(&v)
}

pub fn normalize_value(v: &Value) -> Vec<NormalizedUpdate> {
    // Notification shape
    if let Ok(n) = serde_json::from_value::<JsonRpcNotification>(v.clone()) {
        return normalize_notification(&n.method, &n.params);
    }
    // Sometimes agents omit jsonrpc field
    if let Some(method) = v.get("method").and_then(|m| m.as_str()) {
        let params = v.get("params").cloned().unwrap_or(Value::Null);
        return normalize_notification(method, &params);
    }
    vec![NormalizedUpdate::DiagnosticOnly {
        method: "unknown".into(),
        reason: "unrecognized JSON-RPC shape".into(),
        raw: bound_payload(v, DEFAULT_DIAGNOSTIC_MAX_BYTES),
    }]
}

pub fn normalize_notification(method: &str, params: &Value) -> Vec<NormalizedUpdate> {
    match method {
        "session/update" => normalize_session_update(params),
        // Agent → client permission request is a JSON-RPC *request*, but some
        // bridges forward it as a notification with method name.
        "session/request_permission" | "session/requestPermission" => {
            vec![normalize_permission(params)]
        }
        m if m.starts_with("_x.ai/") || m.starts_with("x.ai/") => {
            vec![normalize_xai(m, params)]
        }
        // Lifecycle / other — diagnostics only
        other => vec![NormalizedUpdate::DiagnosticOnly {
            method: other.into(),
            reason: "method not shown in main timeline".into(),
            raw: bound_payload(params, DEFAULT_DIAGNOSTIC_MAX_BYTES),
        }],
    }
}

fn normalize_session_update(params: &Value) -> Vec<NormalizedUpdate> {
    let update = params
        .get("update")
        .cloned()
        .or_else(|| params.get("sessionUpdate").cloned())
        .unwrap_or_else(|| params.clone());

    let session_update = update
        .get("sessionUpdate")
        .or_else(|| update.get("type"))
        .or_else(|| update.get("kind"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Some agents nest under update.update
    let body = if update.get("content").is_some()
        || update.get("toolCallId").is_some()
        || update.get("entries").is_some()
    {
        &update
    } else if let Some(inner) = update.get("update") {
        inner
    } else {
        &update
    };

    match session_update {
        "agent_thought_chunk" | "agent_thought" | "thought" => {
            vec![NormalizedUpdate::AgentThought {
                text: extract_text(body),
                message_id: extract_message_id(body),
                meta: body.get("_meta").cloned(),
            }]
        }
        "agent_message_chunk" | "agent_message" | "message" | "assistant_message" => {
            vec![NormalizedUpdate::AgentMessage {
                text: extract_text(body),
                message_id: extract_message_id(body),
                meta: body.get("_meta").cloned(),
            }]
        }
        "user_message_chunk" | "user_message" => {
            vec![NormalizedUpdate::UserMessage {
                text: extract_text(body),
                message_id: extract_message_id(body),
                meta: body.get("_meta").cloned(),
            }]
        }
        "tool_call" => vec![normalize_tool(body, false)],
        "tool_call_update" => vec![normalize_tool(body, true)],
        "plan" => vec![normalize_plan(body)],
        "usage_update" | "usage" => vec![NormalizedUpdate::Usage {
            raw: bound_payload(body, DEFAULT_DIAGNOSTIC_MAX_BYTES),
        }],
        "current_mode_update" => vec![NormalizedUpdate::CurrentMode {
            mode: body
                .get("mode")
                .or_else(|| body.get("currentMode"))
                .and_then(|v| v.as_str())
                .map(str::to_string),
            raw: bound_payload(body, DEFAULT_DIAGNOSTIC_MAX_BYTES),
        }],
        "config_option_update" => vec![NormalizedUpdate::ConfigOption {
            raw: bound_payload(body, DEFAULT_DIAGNOSTIC_MAX_BYTES),
        }],
        "session_info_update" => vec![NormalizedUpdate::SessionInfo {
            session_id: body
                .get("sessionId")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            model: extract_model(body),
            raw: bound_payload(body, DEFAULT_DIAGNOSTIC_MAX_BYTES),
        }],
        "available_commands_update" => vec![NormalizedUpdate::AvailableCommands {
            raw: bound_payload(body, DEFAULT_DIAGNOSTIC_MAX_BYTES),
        }],
        other if other.starts_with("_x.ai") || other.is_empty() => {
            // Try content-based inference when type missing
            if body.get("toolCallId").is_some() {
                let is_update = session_update.contains("update")
                    || body.get("status").is_some() && body.get("title").is_none();
                return vec![normalize_tool(body, is_update)];
            }
            if body.get("entries").is_some() {
                return vec![normalize_plan(body)];
            }
            if let Some(text) = try_chunk_text(body) {
                let kind = body
                    .get("content")
                    .and_then(|c| c.get("type"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("");
                if kind.contains("thought") {
                    return vec![NormalizedUpdate::AgentThought {
                        text,
                        message_id: extract_message_id(body),
                        meta: body.get("_meta").cloned(),
                    }];
                }
                return vec![NormalizedUpdate::AgentMessage {
                    text,
                    message_id: extract_message_id(body),
                    meta: body.get("_meta").cloned(),
                }];
            }
            if !other.is_empty() {
                return vec![NormalizedUpdate::DiagnosticOnly {
                    method: format!("session/update:{other}"),
                    reason: "unrecognized sessionUpdate type".into(),
                    raw: bound_payload(body, DEFAULT_DIAGNOSTIC_MAX_BYTES),
                }];
            }
            vec![NormalizedUpdate::DiagnosticOnly {
                method: "session/update".into(),
                reason: "empty or unknown session update".into(),
                raw: bound_payload(body, DEFAULT_DIAGNOSTIC_MAX_BYTES),
            }]
        }
        other => vec![NormalizedUpdate::DiagnosticOnly {
            method: format!("session/update:{other}"),
            reason: "unrecognized sessionUpdate type".into(),
            raw: bound_payload(body, DEFAULT_DIAGNOSTIC_MAX_BYTES),
        }],
    }
}

fn normalize_tool(body: &Value, is_update: bool) -> NormalizedUpdate {
    let tool_call_id = body
        .get("toolCallId")
        .or_else(|| body.get("tool_call_id"))
        .or_else(|| body.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let title = body
        .get("title")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let kind = body
        .get("kind")
        .or_else(|| body.get("toolName"))
        .or_else(|| body.get("name"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let status = body
        .get("status")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    let content_text = extract_tool_content_text(body);
    let locations = extract_locations(body);
    let raw = bound_payload(body, DEFAULT_DIAGNOSTIC_MAX_BYTES);

    if is_update {
        NormalizedUpdate::ToolCallUpdate {
            tool_call_id,
            title,
            kind,
            status,
            content_text,
            locations,
            raw,
        }
    } else {
        NormalizedUpdate::ToolCall {
            tool_call_id,
            title,
            kind,
            status,
            content_text,
            locations,
            raw,
        }
    }
}

fn normalize_plan(body: &Value) -> NormalizedUpdate {
    let entries = body
        .get("entries")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|e| {
                    let content = e
                        .get("content")
                        .or_else(|| e.get("title"))
                        .or_else(|| e.get("text"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    if content.is_empty() {
                        return None;
                    }
                    Some(PlanEntry {
                        content,
                        status: e.get("status").and_then(|v| v.as_str()).map(str::to_string),
                        priority: e
                            .get("priority")
                            .and_then(|v| v.as_str())
                            .map(str::to_string),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    NormalizedUpdate::Plan {
        entries,
        raw: bound_payload(body, DEFAULT_DIAGNOSTIC_MAX_BYTES),
    }
}

fn normalize_permission(params: &Value) -> NormalizedUpdate {
    let request_id = params
        .get("requestId")
        .or_else(|| params.get("id"))
        .map(|v| match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        })
        .unwrap_or_else(|| "unknown".into());

    let tool_call_id = params
        .get("toolCallId")
        .or_else(|| params.get("toolCall").and_then(|t| t.get("toolCallId")))
        .and_then(|v| v.as_str())
        .map(str::to_string);

    let options = params
        .get("options")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|o| {
                    let option_id = o
                        .get("optionId")
                        .or_else(|| o.get("id"))
                        .and_then(|v| v.as_str())?
                        .to_string();
                    let name = o
                        .get("name")
                        .or_else(|| o.get("label"))
                        .and_then(|v| v.as_str())
                        .unwrap_or(&option_id)
                        .to_string();
                    Some(PermissionOption {
                        option_id,
                        name,
                        kind: o.get("kind").and_then(|v| v.as_str()).map(str::to_string),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let summary = params
        .get("title")
        .or_else(|| params.get("description"))
        .and_then(|v| v.as_str())
        .map(|s| first_line_truncated(s, 120))
        .unwrap_or_else(|| {
            if let Some(ref tid) = tool_call_id {
                format!("Permission requested for tool {tid}")
            } else {
                "Permission requested".into()
            }
        });

    NormalizedUpdate::PermissionRequest {
        request_id,
        tool_call_id,
        summary,
        options,
        raw: bound_payload(params, DEFAULT_DIAGNOSTIC_MAX_BYTES),
    }
}

fn normalize_xai(method: &str, params: &Value) -> NormalizedUpdate {
    let stage_title = params
        .get("title")
        .or_else(|| params.get("stageTitle"))
        .or_else(|| params.get("summary").and_then(|s| s.get("title")))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let summary_text = params
        .get("text")
        .or_else(|| params.get("summary").and_then(|s| s.get("text")))
        .or_else(|| params.get("message"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let event_id = params
        .get("eventId")
        .or_else(|| params.get("_meta").and_then(|m| m.get("eventId")))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let is_replay = params
        .get("_meta")
        .and_then(|m| m.get("isReplay"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    NormalizedUpdate::XaiExtension {
        method: method.into(),
        stage_title,
        summary_text,
        event_id,
        is_replay,
        raw: bound_payload(params, DEFAULT_DIAGNOSTIC_MAX_BYTES),
    }
}

fn extract_text(body: &Value) -> String {
    if let Some(t) = try_chunk_text(body) {
        return t;
    }
    body.get("text")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn try_chunk_text(body: &Value) -> Option<String> {
    // content: { type: "text", text: "..." }
    if let Some(c) = body.get("content") {
        if let Some(t) = c.get("text").and_then(|v| v.as_str()) {
            return Some(t.to_string());
        }
        if let Some(arr) = c.as_array() {
            let mut s = String::new();
            for part in arr {
                if let Some(t) = part.get("text").and_then(|v| v.as_str()) {
                    s.push_str(t);
                } else if let Some(t) = part.as_str() {
                    s.push_str(t);
                }
            }
            if !s.is_empty() {
                return Some(s);
            }
        }
        if let Some(t) = c.as_str() {
            return Some(t.to_string());
        }
    }
    body.get("delta")
        .and_then(|d| d.get("text").or(Some(d)))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn extract_message_id(body: &Value) -> Option<String> {
    body.get("messageId")
        .or_else(|| body.get("id"))
        .or_else(|| body.get("_meta").and_then(|m| m.get("messageId")))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn extract_model(body: &Value) -> Option<String> {
    body.get("model")
        .or_else(|| body.get("currentModel"))
        .or_else(|| body.get("modelId"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn extract_tool_content_text(body: &Value) -> Option<String> {
    if let Some(t) = body.get("content").and_then(|c| {
        if let Some(s) = c.as_str() {
            return Some(s.to_string());
        }
        if let Some(arr) = c.as_array() {
            let mut s = String::new();
            for part in arr {
                if let Some(tx) = part.get("text").and_then(|v| v.as_str()) {
                    if !s.is_empty() {
                        s.push('\n');
                    }
                    s.push_str(tx);
                } else if let Some(tx) = part.as_str() {
                    if !s.is_empty() {
                        s.push('\n');
                    }
                    s.push_str(tx);
                }
            }
            if !s.is_empty() {
                return Some(s);
            }
        }
        c.get("text").and_then(|v| v.as_str()).map(str::to_string)
    }) {
        return Some(t);
    }
    body.get("rawInput")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn extract_locations(body: &Value) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(arr) = body.get("locations").and_then(|v| v.as_array()) {
        for loc in arr {
            if let Some(p) = loc
                .get("path")
                .or_else(|| loc.get("uri"))
                .and_then(|v| v.as_str())
            {
                out.push(p.to_string());
            } else if let Some(p) = loc.as_str() {
                out.push(p.to_string());
            }
        }
    }
    if let Some(p) = body.get("path").and_then(|v| v.as_str()) {
        out.push(p.to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thought_chunk_extracts_text() {
        let line = r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s1","update":{"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"Checking order"}}}}"#;
        let ups = normalize_line(line);
        assert_eq!(ups.len(), 1);
        match &ups[0] {
            NormalizedUpdate::AgentThought { text, .. } => assert_eq!(text, "Checking order"),
            other => panic!("expected thought, got {other:?}"),
        }
        assert!(ups[0].human_message().unwrap().contains("Checking"));
    }

    #[test]
    fn tool_call_human_title_from_path() {
        let line = r#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"tool_call","toolCallId":"t1","kind":"read","status":"pending","locations":[{"path":"src/server.ts"}]}}}"#;
        let ups = normalize_line(line);
        match &ups[0] {
            NormalizedUpdate::ToolCall {
                tool_call_id,
                locations,
                ..
            } => {
                assert_eq!(tool_call_id, "t1");
                assert_eq!(locations, &vec!["src/server.ts".to_string()]);
            }
            other => panic!("{other:?}"),
        }
        let msg = ups[0].human_message().unwrap();
        assert!(msg.contains("src/server.ts"), "{msg}");
        assert!(!msg.contains("tool_call"), "{msg}");
        assert!(!msg.contains("session/update"), "{msg}");
    }

    #[test]
    fn agent_message_not_raw_json() {
        let line = r#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Hello **world**"}}}}"#;
        let ups = normalize_line(line);
        match &ups[0] {
            NormalizedUpdate::AgentMessage { text, .. } => {
                assert_eq!(text, "Hello **world**");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn plan_entries_extracted() {
        let line = r#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"plan","entries":[{"content":"Step A","status":"completed"},{"content":"Step B","status":"in_progress"}]}}}"#;
        let ups = normalize_line(line);
        match &ups[0] {
            NormalizedUpdate::Plan { entries, .. } => {
                assert_eq!(entries.len(), 2);
                assert_eq!(entries[1].status.as_deref(), Some("in_progress"));
            }
            other => panic!("{other:?}"),
        }
        let msg = ups[0].human_message().unwrap();
        assert!(msg.contains("Step B"), "{msg}");
    }

    #[test]
    fn xai_extension_stays_diagnostic_unless_useful() {
        let line = r#"{"jsonrpc":"2.0","method":"_x.ai/hook","params":{"foo":"bar"}}"#;
        let ups = normalize_line(line);
        match &ups[0] {
            NormalizedUpdate::XaiExtension { method, .. } => {
                assert!(method.contains("_x.ai"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn raw_secret_redacted_in_diagnostic() {
        let line = r#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"tool_call","toolCallId":"t","title":"x","rawOutput":"Authorization: Bearer super-secret-token-value"}}}"#;
        let ups = normalize_line(line);
        match &ups[0] {
            NormalizedUpdate::ToolCall { raw, .. } => {
                let s = raw.to_string();
                assert!(!s.contains("super-secret-token-value"), "{s}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn unknown_does_not_panic() {
        let ups = normalize_line(
            r#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"totally_new_thing","x":1}}}"#,
        );
        assert!(matches!(ups[0], NormalizedUpdate::DiagnosticOnly { .. }));
    }
}
