//! Global user-level workflow instruction managed blocks for Codex / Claude Code.
//!
//! Injects a GrokTask-managed instruction block into the host agent's global
//! instruction files so agents proactively call the `groktask` MCP server.
//!
//! Targets (user-global, not project-scoped):
//! - Codex: `<home>/.codex/AGENTS.md` (never `AGENTS.override.md`)
//! - Claude Code: `<home>/.claude/CLAUDE.md`
//!
//! Paths resolve via [`IntegrationRoots`] so tests can route to temp homes and
//! never write the real user home.
//!
//! Never edits AskHuman managed blocks. Operations are idempotent and refuse
//! to write when markers are malformed.

use super::types::{AgentId, IntegrationError, WorkflowStatus};
use super::IntegrationRoots;
use crate::config::atomic_write;
use std::fs;
use std::path::{Path, PathBuf};

/// Managed block start marker (must match end pair).
pub const BLOCK_BEGIN: &str = "<!-- GrokTask:begin DO NOT EDIT (managed by GrokTask) -->";
/// Managed block end marker.
pub const BLOCK_END: &str = "<!-- GrokTask:end -->";

/// AskHuman block markers — never touch these regions.
const ASKHUMAN_BEGIN: &str = "<!-- AskHuman:begin";
const ASKHUMAN_END: &str = "<!-- AskHuman:end -->";

/// Default instruction body injected inside the managed block.
pub const DEFAULT_WORKFLOW_BODY: &str = r#"## GrokTask 协作协议

职责边界（严格）：**你（Codex / Claude Code）负责分析、规划、诊断、审查与最终判断；Grok 是实现执行器。** 通过 `groktask` MCP 把**已规划好的实现工作**交给 Grok Build 落地。

### 你（Codex / Claude Code）负责

1. 需求理解与澄清；产品/权限决策；架构与取舍；规划 / 规格 / 检查清单 / 验收标准。
2. Bug 与 CI 根因分析；性能 / 稳定性 / 安全分析；code review；最终验证与判断。
3. 委派实现前必须先完成分析：给出明确的 plan/spec/诊断结论与验收标准。简单改动也无需正式长 plan，但仍须分析后给出简洁实现指令与验收标准——**禁止“不思考、立即委派”**。
4. 纯解释 / 总结 / 翻译，以及分析 / 审查 / 诊断 / 规划类任务由你完成，不要委派给 Grok。
5. Grok 返回后做终审与验证（是否达标、是否误伤范围），再向用户交付。

### Grok 负责（通过 groktask）

在收到你给出的 plan/spec/诊断与验收标准后：写 / 改代码、补测试、跑相关检查，并修复因其实现或主机审查反馈导致的编译 / 测试问题。可检查足够周围代码以保证安全实现，可自纠实现级失败。**不得**替代你的规划、独立诊断、架构、审查或最终判断。

### 会话策略（由主机决定）

- **你**决定复用现有 Grok task/session，还是 `run`/`start` 新开。Grok 不决定此生命周期。
- 请求是真正的实现 follow-up，且现有上下文相关且健康时：用 `continue`（保留的 `taskId` + 下一轮 prompt）。
- 工作不相关、上下文陈旧 / 污染、先前 ACP 会话不健康 / 空 / 不收敛、mode 或 workspace 边界需要，或干净实现上下文更安全时：你可选用 `run`/`start` 新开。用户明确要求重置足够，但不强制。
- `mode` 必须按意图显式选 `read` 或 `write`，之后不要静默把 read 改成 write。
- 需要改代码、补测试、落地修复：`mode: write`。只需只读检查辅助实现：`mode: read`。
- 长任务可用 `start`，再用 `status`/`wait`/`cancel`；阻塞短路径用 `run` 或 `continue`。

### 明确豁免（可不调用 GrokTask）

- 纯解释、问答、总结、翻译；
- 1–2 行显然机械改动（改文案、typo）；
- 任务涉及 secrets/token/隐私，不能安全发送给 Grok；
- GrokTask 不可用、返回失败、连续两轮仍不收敛；
- 用户明确要求“不要用 Grok/GrokTask”。

### 默认循环

1. 分析意图 / 范围 / 敏感度；写出 plan/spec/诊断与验收标准（简单改动也给简洁指令 + 验收标准）。
2. 你决定 `run`/`start`（新任务 / 干净上下文）或 `continue`（相关健康 follow-up）。
3. 审查 Grok 结果；有问题再 `continue` 让其按审查意见修，或新开 session。
4. 重复直到达标，或需要用户决策时停下来问用户。
5. 你负责最终验证摘要与交付说明。

### 约束

- 不要让 Grok 自动提交、推送、创建 PR 或扩大工作目录，除非用户明确要求且由你（或用户）执行外部副作用。
- 不要把 secrets、token、个人敏感信息发送给 GrokTask。
- 失败 / 取消 / 部分结果如实报告；连续两轮不收敛则向用户说明阻塞点。
"#;

/// Instruction file basename for each agent.
pub fn instruction_filename(agent: AgentId) -> &'static str {
    match agent {
        AgentId::Codex => "AGENTS.md",
        AgentId::Claude => "CLAUDE.md",
    }
}

/// Absolute path to the global user instruction file for `agent` under `roots.home`.
///
/// - Codex: `<home>/.codex/AGENTS.md`
/// - Claude: `<home>/.claude/CLAUDE.md`
pub fn instruction_path(roots: &IntegrationRoots, agent: AgentId) -> PathBuf {
    match agent {
        AgentId::Codex => roots.home.join(".codex").join("AGENTS.md"),
        AgentId::Claude => roots.home.join(".claude").join("CLAUDE.md"),
    }
}

/// Whether text contains a well-formed GrokTask managed block.
pub fn has_block(text: &str) -> bool {
    block_span(text).is_some()
}

/// Extract managed block body (between markers), if well-formed.
pub fn block_body(text: &str) -> Option<String> {
    let (start, end) = block_span(text)?;
    let inner_start = start + BLOCK_BEGIN.len();
    let inner_end = end - BLOCK_END.len();
    if inner_end < inner_start {
        return None;
    }
    Some(text[inner_start..inner_end].trim_matches('\n').to_string())
}

/// Locate well-formed `[begin, end)` byte span including both markers.
///
/// Returns `None` when markers are absent. Callers that need to distinguish
/// malformed pairs should use [`marker_state`].
pub fn block_span(text: &str) -> Option<(usize, usize)> {
    match marker_state(text) {
        MarkerState::WellFormed { start, end } => Some((start, end)),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerState {
    Absent,
    WellFormed {
        start: usize,
        end: usize,
    },
    /// Begin without matching end, multiple begins/ends, or end before begin.
    Malformed,
}

/// Analyze GrokTask marker pairs without mutating text.
pub fn marker_state(text: &str) -> MarkerState {
    let begin_count = text.matches(BLOCK_BEGIN).count();
    let end_count = text.matches(BLOCK_END).count();
    if begin_count == 0 && end_count == 0 {
        return MarkerState::Absent;
    }
    if begin_count != 1 || end_count != 1 {
        return MarkerState::Malformed;
    }
    let Some(start) = text.find(BLOCK_BEGIN) else {
        return MarkerState::Malformed;
    };
    let Some(end_rel) = text[start + BLOCK_BEGIN.len()..].find(BLOCK_END) else {
        return MarkerState::Malformed;
    };
    let end = start + BLOCK_BEGIN.len() + end_rel + BLOCK_END.len();
    // Ensure the only end marker is this one (already enforced by count==1).
    if text[..start].contains(BLOCK_END) {
        return MarkerState::Malformed;
    }
    MarkerState::WellFormed { start, end }
}

/// Insert or replace the managed block. Preserves all content outside the block,
/// including AskHuman managed sections. Idempotent when body already matches.
pub fn upsert_block(text: &str, body: &str) -> Result<String, IntegrationError> {
    let block = format_block(body);
    match marker_state(text) {
        MarkerState::Malformed => Err(IntegrationError::InvalidConfig(
            "malformed GrokTask managed block markers; fix or remove begin/end pair before enabling workflow"
                .into(),
        )),
        MarkerState::WellFormed { start, end } => {
            let mut out = String::with_capacity(text.len() + block.len());
            out.push_str(&text[..start]);
            out.push_str(&block);
            out.push_str(&text[end..]);
            Ok(ensure_trailing_newline(&out))
        }
        MarkerState::Absent => {
            let base = text.trim_end();
            let out = if base.is_empty() {
                format!("{block}\n")
            } else {
                format!("{base}\n\n{block}\n")
            };
            Ok(out)
        }
    }
}

/// Remove only the GrokTask managed block. Preserves user content and AskHuman blocks.
pub fn remove_block(text: &str) -> Result<String, IntegrationError> {
    match marker_state(text) {
        MarkerState::Malformed => Err(IntegrationError::InvalidConfig(
            "malformed GrokTask managed block markers; fix or remove begin/end pair before disabling workflow"
                .into(),
        )),
        MarkerState::Absent => Ok(text.to_string()),
        MarkerState::WellFormed { start, end } => {
            let mut out = String::with_capacity(text.len());
            out.push_str(&text[..start]);
            out.push_str(&text[end..]);
            Ok(tidy(&out))
        }
    }
}

fn format_block(body: &str) -> String {
    let body = body.trim_matches('\n');
    format!("{BLOCK_BEGIN}\n{body}\n{BLOCK_END}")
}

fn ensure_trailing_newline(s: &str) -> String {
    if s.is_empty() || s.ends_with('\n') {
        s.to_string()
    } else {
        format!("{s}\n")
    }
}

/// Collapse consecutive blank lines, trim trailing whitespace, keep one final newline.
fn tidy(s: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let mut prev_empty = false;
    for line in s.split('\n') {
        let is_empty = line.trim().is_empty();
        if is_empty && prev_empty {
            continue;
        }
        out.push(line);
        prev_empty = is_empty;
    }
    let trimmed = out.join("\n").trim_end().to_string();
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("{trimmed}\n")
    }
}

/// Workflow status for one agent under a user home root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowInspection {
    pub status: WorkflowStatus,
    pub path: String,
    pub detail: Option<String>,
    pub can_write: bool,
}

/// Inspect workflow instruction status without writing.
pub fn inspect(roots: &IntegrationRoots, agent: AgentId) -> WorkflowInspection {
    let path = instruction_path(roots, agent);
    let display = path.display().to_string();

    // Parent dir may not exist yet — still not_enabled and writable (enable creates dirs).
    if !path.exists() {
        return WorkflowInspection {
            status: WorkflowStatus::NotEnabled,
            path: display,
            detail: None,
            can_write: true,
        };
    }

    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            return WorkflowInspection {
                status: WorkflowStatus::Unavailable,
                path: display,
                detail: Some(format!("cannot read instruction file: {e}")),
                can_write: false,
            };
        }
    };

    match marker_state(&text) {
        MarkerState::Absent => WorkflowInspection {
            status: WorkflowStatus::NotEnabled,
            path: display,
            detail: None,
            can_write: true,
        },
        MarkerState::Malformed => WorkflowInspection {
            status: WorkflowStatus::InvalidFile,
            path: display,
            detail: Some("malformed GrokTask managed block markers (begin/end mismatch)".into()),
            can_write: false,
        },
        MarkerState::WellFormed { .. } => {
            let body = block_body(&text).unwrap_or_default();
            if body.trim() == DEFAULT_WORKFLOW_BODY.trim() {
                WorkflowInspection {
                    status: WorkflowStatus::Enabled,
                    path: display,
                    detail: None,
                    can_write: true,
                }
            } else {
                WorkflowInspection {
                    status: WorkflowStatus::Outdated,
                    path: display,
                    detail: Some("managed block body differs from current template".into()),
                    can_write: true,
                }
            }
        }
    }
}

/// Enable (upsert) the default managed instruction block for `agent`.
pub fn enable(
    roots: &IntegrationRoots,
    agent: AgentId,
) -> Result<WorkflowInspection, IntegrationError> {
    let path = instruction_path(roots, agent);

    let existing = if path.exists() {
        fs::read_to_string(&path)?
    } else {
        String::new()
    };

    // Refuse if malformed before any write.
    let updated = upsert_block(&existing, DEFAULT_WORKFLOW_BODY)?;
    if updated == existing && path.exists() {
        return Ok(inspect(roots, agent));
    }

    write_instruction(&path, &updated)?;
    Ok(inspect(roots, agent))
}

/// Disable: remove only the GrokTask managed block.
pub fn disable(
    roots: &IntegrationRoots,
    agent: AgentId,
) -> Result<WorkflowInspection, IntegrationError> {
    let path = instruction_path(roots, agent);
    if !path.exists() {
        return Ok(inspect(roots, agent));
    }
    let existing = fs::read_to_string(&path)?;
    let updated = remove_block(&existing)?;
    if updated == existing {
        return Ok(inspect(roots, agent));
    }
    write_instruction(&path, &updated)?;
    Ok(inspect(roots, agent))
}

fn write_instruction(path: &Path, text: &str) -> Result<(), IntegrationError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let prev_perm = if path.exists() {
        fs::metadata(path).ok().map(|m| m.permissions())
    } else {
        None
    };
    atomic_write(path, text.as_bytes())?;
    if let Some(perm) = prev_perm {
        let _ = fs::set_permissions(path, perm);
    }
    Ok(())
}

/// True if text still contains AskHuman managed markers (used by tests).
#[cfg(test)]
pub fn contains_askhuman_block(text: &str) -> bool {
    text.contains(ASKHUMAN_BEGIN) && text.contains(ASKHUMAN_END)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn temp_roots() -> (TempDir, IntegrationRoots) {
        let tmp = TempDir::new().unwrap();
        let roots = IntegrationRoots::from_home(tmp.path());
        (tmp, roots)
    }

    #[test]
    fn instruction_paths_are_global_under_home() {
        let (_tmp, roots) = temp_roots();
        assert_eq!(
            instruction_path(&roots, AgentId::Codex),
            roots.home.join(".codex").join("AGENTS.md")
        );
        assert_eq!(
            instruction_path(&roots, AgentId::Claude),
            roots.home.join(".claude").join("CLAUDE.md")
        );
    }

    #[test]
    fn create_missing_agents_md() {
        let (_tmp, roots) = temp_roots();
        let st = enable(&roots, AgentId::Codex).unwrap();
        assert_eq!(st.status, WorkflowStatus::Enabled);
        let path = roots.home.join(".codex").join("AGENTS.md");
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains(BLOCK_BEGIN));
        assert!(text.contains(BLOCK_END));
        assert!(text.contains("GrokTask 协作协议"));
        assert!(text.contains("职责边界"));
        assert!(text.contains("实现执行器"));
        assert!(text.contains("验收标准"));
        assert!(text.contains("continue"));
        assert!(text.contains("taskId"));
        assert!(text.contains("明确豁免"));
        assert!(text.contains("会话策略（由主机决定）"));
        assert!(text.contains("不要静默把 read 改成 write"));
        // Host owns analysis / planning / review / final judgment
        assert!(text.contains("根因分析") || text.contains("code review"));
        assert!(text.contains("最终验证"));
        assert!(text.contains("禁止“不思考、立即委派”") || text.contains("禁止"));
        // Grok is implementation executor only
        assert!(text.contains("写 / 改代码") || text.contains("写/改代码"));
        assert!(text.contains("不得") && text.contains("规划"));
        // Session lifecycle is host-decided (reuse or fresh)
        assert!(text.contains("你可选用") || text.contains("你决定"));
        assert!(text.contains("干净实现上下文") || text.contains("不收敛"));
        // Reject prior maximum-delegation / do-not-think / rigid-only-user-reset copy
        assert!(!text.contains("默认最大委派"));
        assert!(!text.contains("最大委派"));
        assert!(!text.contains("不要自己做诊断、根因分析、review 推理或实现推演——立即委派"));
        assert!(!text.contains("不要因为“自己也能做”就跳过"));
        assert!(!text.contains("不要把“我能自己做”当成跳过理由"));
        assert!(!text.contains("同一主机对话 + 同一 workspace 内复用同一 Grok ACP 会话"));
        assert!(!text.contains("用户明确要求新上下文"));
        assert!(!text.contains("不要再 `run`/`start` 新开 session"));
        // Idempotent
        let before = fs::read_to_string(&path).unwrap();
        enable(&roots, AgentId::Codex).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
    }

    #[test]
    fn default_workflow_body_encodes_host_executor_boundary() {
        let body = DEFAULT_WORKFLOW_BODY;
        // Positive: host analyzes/plans/reviews; Grok executes
        assert!(body.contains("职责边界"));
        assert!(body.contains("实现执行器"));
        assert!(body.contains("验收标准"));
        assert!(body.contains("code review") || body.contains("最终验证"));
        assert!(body.contains("会话策略（由主机决定）"));
        assert!(body.contains("continue"));
        assert!(body.contains("run") && body.contains("start"));
        // Negative: prior maximum-delegation / do-not-think / rigid session policy
        assert!(!body.contains("默认最大委派"));
        assert!(!body.contains("最大委派"));
        assert!(!body.contains("立即委派 Grok"));
        assert!(!body.contains("不要自己做诊断"));
        assert!(!body.contains("默认委派"));
        assert!(!body.contains("调试与根因定位、CI 诊断与修复、code review"));
        assert!(!body.contains("性能/稳定性/安全分析，以及按你给出的 plan"));
        assert!(!body.contains("用户明确要求新上下文"));
        assert!(!body.contains("不要再 `run`/`start` 新开 session"));
    }

    #[test]
    fn create_missing_claude_md() {
        let (_tmp, roots) = temp_roots();
        let st = enable(&roots, AgentId::Claude).unwrap();
        assert_eq!(st.status, WorkflowStatus::Enabled);
        assert!(roots.home.join(".claude").join("CLAUDE.md").exists());
    }

    #[test]
    fn append_to_existing_file() {
        let (_tmp, roots) = temp_roots();
        let path = roots.home.join(".codex").join("AGENTS.md");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "# My user rules\n\nAlways run tests.\n").unwrap();
        enable(&roots, AgentId::Codex).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# My user rules"));
        assert!(text.contains("Always run tests."));
        assert!(text.contains(BLOCK_BEGIN));
    }

    #[test]
    fn update_old_block_body() {
        let (_tmp, roots) = temp_roots();
        let path = roots.home.join(".codex").join("AGENTS.md");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let old = format!("{BLOCK_BEGIN}\nold body\n{BLOCK_END}\n");
        fs::write(&path, &old).unwrap();
        let st = inspect(&roots, AgentId::Codex);
        assert_eq!(st.status, WorkflowStatus::Outdated);
        enable(&roots, AgentId::Codex).unwrap();
        let st = inspect(&roots, AgentId::Codex);
        assert_eq!(st.status, WorkflowStatus::Enabled);
        let body = block_body(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(body.trim(), DEFAULT_WORKFLOW_BODY.trim());
    }

    #[test]
    fn disable_removes_only_groktask_block() {
        let (_tmp, roots) = temp_roots();
        let path = roots.home.join(".codex").join("AGENTS.md");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let askhuman = "<!-- AskHuman:begin DO NOT EDIT (managed by AskHuman) -->\nask content\n<!-- AskHuman:end -->";
        let content = format!(
            "# header\n\n{askhuman}\n\n{BLOCK_BEGIN}\n{DEFAULT_WORKFLOW_BODY}{BLOCK_END}\n\nfooter\n"
        );
        fs::write(&path, &content).unwrap();
        disable(&roots, AgentId::Codex).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains(BLOCK_BEGIN));
        assert!(!text.contains(BLOCK_END));
        assert!(contains_askhuman_block(&text));
        assert!(text.contains("# header"));
        assert!(text.contains("footer"));
        assert!(text.contains("ask content"));
    }

    #[test]
    fn malformed_marker_refuses_write() {
        let (_tmp, roots) = temp_roots();
        let path = roots.home.join(".codex").join("AGENTS.md");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        // Begin without end
        fs::write(&path, format!("{BLOCK_BEGIN}\nno end\n")).unwrap();
        let before = fs::read(&path).unwrap();
        let st = inspect(&roots, AgentId::Codex);
        assert_eq!(st.status, WorkflowStatus::InvalidFile);
        assert!(enable(&roots, AgentId::Codex).is_err());
        assert!(disable(&roots, AgentId::Codex).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);

        // Multiple begins
        fs::write(
            &path,
            format!("{BLOCK_BEGIN}\nx\n{BLOCK_END}\n{BLOCK_BEGIN}\ny\n{BLOCK_END}\n"),
        )
        .unwrap();
        assert_eq!(
            inspect(&roots, AgentId::Codex).status,
            WorkflowStatus::InvalidFile
        );
        assert!(enable(&roots, AgentId::Codex).is_err());
    }

    #[test]
    fn askhuman_block_preserved_on_enable() {
        let (_tmp, roots) = temp_roots();
        let path = roots.home.join(".codex").join("AGENTS.md");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let askhuman = "<!-- AskHuman:begin DO NOT EDIT (managed by AskHuman) -->\nkeep me\n<!-- AskHuman:end -->\n";
        fs::write(&path, askhuman).unwrap();
        enable(&roots, AgentId::Codex).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(contains_askhuman_block(&text));
        assert!(text.contains("keep me"));
        assert!(text.contains(BLOCK_BEGIN));
    }

    #[test]
    fn not_enabled_when_empty_home() {
        let (_tmp, roots) = temp_roots();
        let st = inspect(&roots, AgentId::Codex);
        assert_eq!(st.status, WorkflowStatus::NotEnabled);
        assert!(st.can_write);
        assert!(st.path.ends_with(".codex/AGENTS.md") || st.path.ends_with(".codex\\AGENTS.md"));
    }

    #[test]
    fn never_writes_agents_override() {
        let (_tmp, roots) = temp_roots();
        enable(&roots, AgentId::Codex).unwrap();
        assert!(!roots
            .home
            .join(".codex")
            .join("AGENTS.override.md")
            .exists());
        assert!(roots.home.join(".codex").join("AGENTS.md").exists());
    }

    #[test]
    fn upsert_idempotent_pure() {
        let once = upsert_block("", DEFAULT_WORKFLOW_BODY).unwrap();
        let twice = upsert_block(&once, DEFAULT_WORKFLOW_BODY).unwrap();
        assert_eq!(once, twice);
    }

    #[test]
    fn remove_absent_is_identity() {
        let text = "# only user content\n";
        assert_eq!(remove_block(text).unwrap(), text);
    }
}
