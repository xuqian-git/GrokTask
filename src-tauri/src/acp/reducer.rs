//! Strict-order ACP → timeline reducer.
//!
//! Converts [`NormalizedUpdate`] streams into timeline item mutations with
//! stable IDs. Thought/tool/reply order is preserved exactly as updates arrive.

use super::types::{first_line_truncated, tool_human_title, NormalizedUpdate, PlanEntry};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Timeline item kinds matching conversation-stream.md.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    UserMessage,
    ReasoningSegment,
    AssistantSegment,
    ToolCall,
    PlanSnapshot,
    PermissionRequest,
    ContextNotice,
}

impl ItemKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UserMessage => "user_message",
            Self::ReasoningSegment => "reasoning_segment",
            Self::AssistantSegment => "assistant_segment",
            Self::ToolCall => "tool_call",
            Self::PlanSnapshot => "plan_snapshot",
            Self::PermissionRequest => "permission_request",
            Self::ContextNotice => "context_notice",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineItem {
    pub item_id: String,
    pub kind: String,
    pub turn_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Human-meaningful primary line (never raw ACP JSON).
    pub message: String,
    #[serde(default)]
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub locations: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_entries: Option<Vec<PlanEntry>>,
    /// Streaming vs completed.
    #[serde(default)]
    pub streaming: bool,
    /// Mark final/partial assistant answer on drain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer_mark: Option<String>,
    /// Stage title for reasoning (from xAI or derived summary).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage_title: Option<String>,
    /// Redacted diagnostic only — never primary UI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineMutation {
    pub operation: String, // add | update | plan | plan_finalize | remove
    pub item_id: String,
    pub sequence: i64,
    pub item: TimelineItem,
}

#[derive(Debug, Clone, PartialEq)]
enum OpenSegment {
    Reasoning { item_id: String },
    Assistant { item_id: String },
}

/// In-memory reducer state for one turn (or staging load).
#[derive(Debug, Clone)]
pub struct TurnReducer {
    pub task_id: String,
    pub turn_id: String,
    pub session_id: String,
    segment_ordinal: u32,
    open: Option<OpenSegment>,
    items: Vec<TimelineItem>,
    /// toolCallId → item index
    tool_index: std::collections::HashMap<String, usize>,
    plan_item_id: Option<String>,
    next_sequence: i64,
    pending_mutations: Vec<TimelineMutation>,
    /// Latest human action for task status.
    pub latest_action: Option<String>,
    pub current_step: Option<String>,
    /// Stage title injected into the next/current reasoning segment.
    pending_stage_title: Option<String>,
}

impl TurnReducer {
    pub fn new(
        task_id: impl Into<String>,
        turn_id: impl Into<String>,
        session_id: impl Into<String>,
    ) -> Self {
        Self {
            task_id: task_id.into(),
            turn_id: turn_id.into(),
            session_id: session_id.into(),
            segment_ordinal: 0,
            open: None,
            items: Vec::new(),
            tool_index: std::collections::HashMap::new(),
            plan_item_id: None,
            next_sequence: 0,
            pending_mutations: Vec::new(),
            latest_action: None,
            current_step: None,
            pending_stage_title: None,
        }
    }

    pub fn items(&self) -> &[TimelineItem] {
        &self.items
    }

    pub fn take_mutations(&mut self) -> Vec<TimelineMutation> {
        std::mem::take(&mut self.pending_mutations)
    }

    pub fn answer_markdown(&self) -> String {
        self.items
            .iter()
            .filter(|i| i.kind == ItemKind::AssistantSegment.as_str())
            .map(|i| i.text.as_str())
            .collect::<Vec<_>>()
            .join("")
    }

    pub fn apply(&mut self, update: NormalizedUpdate) {
        if let Some(msg) = update.human_message() {
            self.latest_action = Some(msg.clone());
            // current_step prefers plan running entry / tool activity
            match &update {
                NormalizedUpdate::Plan { .. }
                | NormalizedUpdate::ToolCall { .. }
                | NormalizedUpdate::ToolCallUpdate { .. }
                | NormalizedUpdate::PermissionRequest { .. } => {
                    self.current_step = Some(msg);
                }
                _ => {}
            }
        }

        match update {
            NormalizedUpdate::AgentThought {
                text, message_id, ..
            } => {
                self.append_text(ItemKind::ReasoningSegment, &text, message_id.as_deref());
            }
            NormalizedUpdate::AgentMessage {
                text, message_id, ..
            } => {
                self.append_text(ItemKind::AssistantSegment, &text, message_id.as_deref());
            }
            NormalizedUpdate::UserMessage {
                text, message_id, ..
            } => {
                self.flush_open();
                let item_id = self.stable_msg_id(message_id.as_deref(), "user");
                // Dedupe identical echo
                if let Some(last) = self.items.iter().rev().find(|i| i.kind == "user_message") {
                    if last.text == text
                        || text.starts_with(&last.text) && last.text.len() >= text.len()
                    {
                        return;
                    }
                    if last.text.starts_with(&text) {
                        return;
                    }
                }
                let item = TimelineItem {
                    item_id: item_id.clone(),
                    kind: ItemKind::UserMessage.as_str().into(),
                    turn_id: self.turn_id.clone(),
                    title: None,
                    message: first_line_truncated(&text, 120),
                    text,
                    status: None,
                    tool_call_id: None,
                    tool_kind: None,
                    locations: vec![],
                    plan_entries: None,
                    streaming: false,
                    answer_mark: None,
                    stage_title: None,
                    diagnostic: None,
                };
                self.push_add(item);
            }
            NormalizedUpdate::ToolCall {
                tool_call_id,
                title,
                kind,
                status,
                content_text,
                locations,
                raw,
            } => {
                self.flush_open();
                self.upsert_tool(
                    &tool_call_id,
                    title,
                    kind,
                    status,
                    content_text,
                    locations,
                    Some(raw),
                    false,
                );
            }
            NormalizedUpdate::ToolCallUpdate {
                tool_call_id,
                title,
                kind,
                status,
                content_text,
                locations,
                raw,
            } => {
                // Tool update merges in place; still a segment boundary for open text.
                // Spec: any different visible type flushes. Tool is different from text.
                if !self.tool_index.contains_key(&tool_call_id) {
                    self.flush_open();
                } else {
                    // Existing tool card: flush text so thought after tool stays ordered.
                    // But update itself doesn't create a new position.
                    self.flush_open();
                }
                self.upsert_tool(
                    &tool_call_id,
                    title,
                    kind,
                    status,
                    content_text,
                    locations,
                    Some(raw),
                    true,
                );
            }
            NormalizedUpdate::Plan { entries, raw } => {
                self.flush_open();
                let item_id = self
                    .plan_item_id
                    .clone()
                    .unwrap_or_else(|| format!("plan:{}", self.turn_id));
                self.plan_item_id = Some(item_id.clone());
                let running = entries
                    .iter()
                    .find(|e| {
                        matches!(
                            e.status.as_deref(),
                            Some("in_progress" | "running" | "pending")
                        )
                    })
                    .map(|e| e.content.clone());
                if let Some(r) = running {
                    self.current_step = Some(r);
                }
                let message = entries
                    .first()
                    .map(|_e| format!("Plan · {} steps", entries.len()))
                    .unwrap_or_else(|| "Plan".into());
                let item = TimelineItem {
                    item_id: item_id.clone(),
                    kind: "plan".into(),
                    turn_id: self.turn_id.clone(),
                    title: Some("Plan".into()),
                    message,
                    text: String::new(),
                    status: Some("active_hidden".into()),
                    tool_call_id: None,
                    tool_kind: None,
                    locations: vec![],
                    plan_entries: Some(entries),
                    streaming: false,
                    answer_mark: None,
                    stage_title: None,
                    diagnostic: Some(raw),
                };
                if let Some(idx) = self.items.iter().position(|i| i.item_id == item_id) {
                    self.items[idx] = item.clone();
                    self.push_mutation("plan", item);
                } else {
                    self.push_add(item);
                    // rewrite last op to plan
                    if let Some(m) = self.pending_mutations.last_mut() {
                        m.operation = "plan".into();
                    }
                }
            }
            NormalizedUpdate::PermissionRequest {
                request_id,
                tool_call_id,
                summary,
                raw,
                ..
            } => {
                self.flush_open();
                let item_id = format!("permission:{}:{}", self.turn_id, request_id);
                let item = TimelineItem {
                    item_id: item_id.clone(),
                    kind: ItemKind::PermissionRequest.as_str().into(),
                    turn_id: self.turn_id.clone(),
                    title: Some("Permission".into()),
                    message: summary,
                    text: String::new(),
                    status: Some("requesting".into()),
                    tool_call_id,
                    tool_kind: None,
                    locations: vec![],
                    plan_entries: None,
                    streaming: false,
                    answer_mark: None,
                    stage_title: None,
                    diagnostic: Some(raw),
                };
                self.push_add(item);
            }
            NormalizedUpdate::XaiExtension {
                stage_title,
                summary_text,
                ..
            } => {
                // Thought summaries belong inside their relevant stage/step.
                if let Some(title) = stage_title.or(summary_text) {
                    self.pending_stage_title = Some(title.clone());
                    if let Some(OpenSegment::Reasoning { item_id }) = &self.open {
                        let id = item_id.clone();
                        if let Some(item) = self.items.iter_mut().find(|i| i.item_id == id) {
                            item.stage_title = Some(title.clone());
                            item.title = Some(title.clone());
                            item.message = title;
                        }
                        let snapshot = self.items.iter().find(|i| i.item_id == id).cloned();
                        if let Some(item) = snapshot {
                            self.push_mutation("update", item);
                        }
                    }
                }
            }
            NormalizedUpdate::SessionInfo { model, .. } => {
                if let Some(m) = model {
                    self.latest_action = Some(format!("Model: {m}"));
                }
            }
            NormalizedUpdate::Usage { .. }
            | NormalizedUpdate::CurrentMode { .. }
            | NormalizedUpdate::ConfigOption { .. }
            | NormalizedUpdate::AvailableCommands { .. }
            | NormalizedUpdate::DiagnosticOnly { .. } => {
                // Not part of main conversation timeline.
            }
        }
    }

    /// Close open segments and mark the last assistant segment.
    pub fn finalize_turn(&mut self, answer_mark: Option<&str>) {
        self.flush_open();
        if let Some(mark) = answer_mark {
            if let Some(item) = self
                .items
                .iter_mut()
                .rev()
                .find(|i| i.kind == ItemKind::AssistantSegment.as_str())
            {
                item.answer_mark = Some(mark.into());
                item.streaming = false;
                item.message = first_line_truncated(&item.text, 120);
            }
            if let Some(item) = self
                .items
                .iter()
                .rev()
                .find(|i| i.kind == ItemKind::AssistantSegment.as_str())
            {
                self.push_mutation("update", item.clone());
            }
        }
        // Finalize plan if present
        if let Some(plan_id) = self.plan_item_id.clone() {
            if let Some(item) = self.items.iter_mut().find(|i| i.item_id == plan_id) {
                item.status = Some("completed_visible".into());
                item.kind = ItemKind::PlanSnapshot.as_str().into();
            }
            if let Some(finalized) = self.items.iter().find(|i| i.item_id == plan_id).cloned() {
                self.push_mutation("plan_finalize", finalized);
            }
        }
        // Ensure all tools not terminal are marked
        let mut tool_updates = Vec::new();
        for item in &mut self.items {
            if item.kind == ItemKind::ToolCall.as_str() {
                let st = item.status.as_deref().unwrap_or("running");
                if matches!(st, "pending" | "running" | "in_progress") {
                    item.status = Some("unknown".into());
                    item.streaming = false;
                    tool_updates.push(item.clone());
                }
            }
        }
        for item in tool_updates {
            self.push_mutation("update", item);
        }
    }

    fn append_text(&mut self, kind: ItemKind, text: &str, message_id: Option<&str>) {
        let want_reasoning = matches!(kind, ItemKind::ReasoningSegment);
        let open_matches = match &self.open {
            Some(OpenSegment::Reasoning { .. }) if want_reasoning => true,
            Some(OpenSegment::Assistant { .. }) if !want_reasoning => true,
            _ => false,
        };

        if open_matches {
            let item_id = match self.open.as_ref().unwrap() {
                OpenSegment::Reasoning { item_id } | OpenSegment::Assistant { item_id } => {
                    item_id.clone()
                }
            };
            if let Some(item) = self.items.iter_mut().find(|i| i.item_id == item_id) {
                item.text.push_str(text);
                item.streaming = true;
                if want_reasoning {
                    item.message = if let Some(t) = &item.stage_title {
                        t.clone()
                    } else {
                        "Thinking…".into()
                    };
                } else {
                    item.message = first_line_truncated(&item.text, 120);
                }
                if let Some(st) = self.pending_stage_title.take() {
                    if want_reasoning {
                        item.stage_title = Some(st.clone());
                        item.title = Some(st.clone());
                        item.message = st;
                    }
                }
            }
            if let Some(item) = self.items.iter().find(|i| i.item_id == item_id) {
                self.push_mutation("update", item.clone());
            }
            return;
        }

        self.flush_open();
        self.segment_ordinal += 1;
        let item_id = if let Some(mid) = message_id {
            let k = if want_reasoning {
                "thought"
            } else {
                "assistant"
            };
            format!("msg:{}:{mid}:{k}", self.session_id)
        } else {
            let k = if want_reasoning {
                "thought"
            } else {
                "assistant"
            };
            format!("seg:{}:{}:{k}", self.turn_id, self.segment_ordinal)
        };

        let mut stage_title = None;
        let mut title = None;
        let mut message = if want_reasoning {
            "Thinking…".into()
        } else {
            first_line_truncated(text, 120)
        };
        if want_reasoning {
            if let Some(st) = self.pending_stage_title.take() {
                stage_title = Some(st.clone());
                title = Some(st.clone());
                message = st;
            }
        }

        let item = TimelineItem {
            item_id: item_id.clone(),
            kind: kind.as_str().into(),
            turn_id: self.turn_id.clone(),
            title,
            message,
            text: text.to_string(),
            status: None,
            tool_call_id: None,
            tool_kind: None,
            locations: vec![],
            plan_entries: None,
            streaming: true,
            answer_mark: None,
            stage_title,
            diagnostic: None,
        };
        self.open = Some(if want_reasoning {
            OpenSegment::Reasoning {
                item_id: item_id.clone(),
            }
        } else {
            OpenSegment::Assistant {
                item_id: item_id.clone(),
            }
        });
        self.push_add(item);
    }

    #[allow(clippy::too_many_arguments)]
    fn upsert_tool(
        &mut self,
        tool_call_id: &str,
        title: Option<String>,
        kind: Option<String>,
        status: Option<String>,
        content_text: Option<String>,
        locations: Vec<String>,
        raw: Option<Value>,
        _is_update: bool,
    ) {
        let item_id = format!("tool:{}:{}", self.session_id, tool_call_id);
        let message = tool_human_title(
            title.as_deref(),
            kind.as_deref(),
            content_text.as_deref(),
            &locations,
            status.as_deref(),
        );

        if let Some(&idx) = self.tool_index.get(tool_call_id) {
            {
                let item = &mut self.items[idx];
                if let Some(t) = title {
                    item.title = Some(t);
                }
                if let Some(k) = kind {
                    item.tool_kind = Some(k);
                }
                if let Some(s) = status {
                    item.status = Some(s);
                }
                if let Some(c) = content_text {
                    if !c.is_empty() {
                        item.text = c;
                    }
                }
                if !locations.is_empty() {
                    item.locations = locations;
                }
                item.message = tool_human_title(
                    item.title.as_deref(),
                    item.tool_kind.as_deref(),
                    if item.text.is_empty() {
                        None
                    } else {
                        Some(item.text.as_str())
                    },
                    &item.locations,
                    item.status.as_deref(),
                );
                if let Some(r) = raw {
                    item.diagnostic = Some(r);
                }
                let done = matches!(
                    item.status.as_deref(),
                    Some("completed" | "success" | "done" | "failed" | "error" | "cancelled")
                );
                item.streaming = !done;
            }
            let cloned = self.items[idx].clone();
            self.push_mutation("update", cloned);
            return;
        }

        let item = TimelineItem {
            item_id: item_id.clone(),
            kind: ItemKind::ToolCall.as_str().into(),
            turn_id: self.turn_id.clone(),
            title,
            message,
            text: content_text.unwrap_or_default(),
            status: status.or_else(|| Some("pending".into())),
            tool_call_id: Some(tool_call_id.into()),
            tool_kind: kind,
            locations,
            plan_entries: None,
            streaming: true,
            answer_mark: None,
            stage_title: None,
            diagnostic: raw,
        };
        let idx = self.items.len();
        self.tool_index.insert(tool_call_id.to_string(), idx);
        self.push_add(item);
    }

    fn flush_open(&mut self) {
        if let Some(open) = self.open.take() {
            let item_id = match open {
                OpenSegment::Reasoning { item_id } | OpenSegment::Assistant { item_id } => item_id,
            };
            if let Some(item) = self.items.iter_mut().find(|i| i.item_id == item_id) {
                item.streaming = false;
                if item.kind == ItemKind::ReasoningSegment.as_str() {
                    // Derive stage summary if missing
                    if item.stage_title.is_none() {
                        let summary = derive_thought_summary(&item.text);
                        item.stage_title = Some(summary.clone());
                        item.title = Some(summary.clone());
                        item.message = summary;
                    } else {
                        item.message = item
                            .stage_title
                            .clone()
                            .unwrap_or_else(|| "Thinking".into());
                    }
                } else {
                    item.message = first_line_truncated(&item.text, 120);
                }
            }
            if let Some(item) = self.items.iter().find(|i| i.item_id == item_id) {
                self.push_mutation("update", item.clone());
            }
        }
    }

    fn stable_msg_id(&mut self, message_id: Option<&str>, kind: &str) -> String {
        if let Some(mid) = message_id {
            format!("msg:{}:{mid}:{kind}", self.session_id)
        } else {
            self.segment_ordinal += 1;
            format!("seg:{}:{}:{kind}", self.turn_id, self.segment_ordinal)
        }
    }

    fn push_add(&mut self, item: TimelineItem) {
        self.items.push(item.clone());
        self.push_mutation("add", item);
    }

    fn push_mutation(&mut self, operation: &str, item: TimelineItem) {
        self.next_sequence += 1;
        self.pending_mutations.push(TimelineMutation {
            operation: operation.into(),
            item_id: item.item_id.clone(),
            sequence: self.next_sequence,
            item,
        });
    }
}

fn derive_thought_summary(text: &str) -> String {
    // Prefer first markdown heading
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix('#') {
            let rest = rest.trim_start_matches('#').trim();
            if !rest.is_empty() {
                return first_line_truncated(rest, 80);
            }
        }
    }
    // First non-empty complete sentence
    let flat = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if let Some(idx) = flat.find(['。', '.', '!', '?', '！', '？']) {
        let s = flat[..=idx].trim();
        if !s.is_empty() {
            return first_line_truncated(s, 80);
        }
    }
    let stripped = strip_md_light(&flat);
    if stripped.is_empty() {
        "思考过程".into()
    } else {
        first_line_truncated(&stripped, 80)
    }
}

fn strip_md_light(s: &str) -> String {
    s.replace("**", "")
        .replace(['*', '`'], "")
        .trim()
        .to_string()
}

/// Convenience: reduce a list of raw ACP lines into items + mutations.
pub fn reduce_lines(
    task_id: &str,
    turn_id: &str,
    session_id: &str,
    lines: &[&str],
) -> (TurnReducer, Vec<TimelineMutation>) {
    use super::normalize::normalize_line;
    let mut r = TurnReducer::new(task_id, turn_id, session_id);
    for line in lines {
        for up in normalize_line(line) {
            r.apply(up);
        }
    }
    let muts = r.take_mutations();
    (r, muts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::normalize::normalize_line;

    fn thought(t: &str) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","method":"session/update","params":{{"update":{{"sessionUpdate":"agent_thought_chunk","content":{{"type":"text","text":"{t}"}}}}}}}}"#
        )
    }
    fn msg(t: &str) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","method":"session/update","params":{{"update":{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"{t}"}}}}}}}}"#
        )
    }
    fn tool(id: &str, path: &str) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","method":"session/update","params":{{"update":{{"sessionUpdate":"tool_call","toolCallId":"{id}","kind":"read","status":"pending","locations":[{{"path":"{path}"}}]}}}}}}"#
        )
    }
    fn tool_update(id: &str, status: &str) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","method":"session/update","params":{{"update":{{"sessionUpdate":"tool_call_update","toolCallId":"{id}","status":"{status}"}}}}}}"#
        )
    }

    #[test]
    fn thought_tool_thought_reply_order() {
        let lines = [
            thought("AAA"),
            thought(" more"),
            tool("t1", "src/a.ts"),
            thought("BBB"),
            msg("Hello"),
            msg(" world"),
        ];
        let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        let (r, _) = reduce_lines("task", "turn", "sess", &refs);
        let kinds: Vec<_> = r.items().iter().map(|i| i.kind.as_str()).collect();
        assert_eq!(
            kinds,
            vec![
                "reasoning_segment",
                "tool_call",
                "reasoning_segment",
                "assistant_segment"
            ]
        );
        assert_eq!(r.items()[0].text, "AAA more");
        assert_eq!(r.items()[3].text, "Hello world");
        assert_eq!(r.answer_markdown(), "Hello world");
        // No raw protocol names in messages
        for item in r.items() {
            assert!(!item.message.contains("session/update"));
            assert!(!item.message.contains("tool_call_update"));
            assert!(!item.message.contains("agent_thought_chunk"));
        }
    }

    #[test]
    fn tool_update_merges_same_card() {
        let lines = [tool("t1", "a.ts"), tool_update("t1", "completed")];
        let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        let (r, _) = reduce_lines("task", "turn", "sess", &refs);
        let tools: Vec<_> = r.items().iter().filter(|i| i.kind == "tool_call").collect();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].status.as_deref(), Some("completed"));
    }

    #[test]
    fn update_before_create_still_one_card() {
        let lines = [tool_update("t9", "running"), tool("t9", "b.ts")];
        let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        let (r, _) = reduce_lines("task", "turn", "sess", &refs);
        let tools: Vec<_> = r.items().iter().filter(|i| i.kind == "tool_call").collect();
        assert_eq!(tools.len(), 1);
        assert!(tools[0].locations.iter().any(|p| p == "b.ts"));
    }

    #[test]
    fn token_chunks_coalesce() {
        let chunks: Vec<String> = ["你", "好", "🎉", "a"].iter().map(|t| thought(t)).collect();
        let refs: Vec<&str> = chunks.iter().map(|s| s.as_str()).collect();
        let (r, _) = reduce_lines("task", "turn", "sess", &refs);
        assert_eq!(r.items().len(), 1);
        assert_eq!(r.items()[0].text, "你好🎉a");
    }

    #[test]
    fn xai_stage_title_attaches_to_thought() {
        let mut r = TurnReducer::new("task", "turn", "sess");
        for up in normalize_line(
            r#"{"jsonrpc":"2.0","method":"_x.ai/summary","params":{"title":"检查事件顺序"}}"#,
        ) {
            r.apply(up);
        }
        for up in normalize_line(&thought("detail about order")) {
            r.apply(up);
        }
        r.flush_open();
        assert_eq!(r.items()[0].stage_title.as_deref(), Some("检查事件顺序"));
        assert!(!r.items()[0].message.contains("{"));
    }

    #[test]
    fn plan_full_replacement() {
        let mut r = TurnReducer::new("task", "turn", "sess");
        let p1 = r#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"plan","entries":[{"content":"A","status":"pending"}]}}}"#;
        let p2 = r#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"plan","entries":[{"content":"A","status":"completed"},{"content":"B","status":"in_progress"}]}}}"#;
        for up in normalize_line(p1) {
            r.apply(up);
        }
        for up in normalize_line(p2) {
            r.apply(up);
        }
        let plans: Vec<_> = r
            .items()
            .iter()
            .filter(|i| i.kind == "plan" || i.kind == "plan_snapshot")
            .collect();
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].plan_entries.as_ref().unwrap().len(), 2);
    }
}
