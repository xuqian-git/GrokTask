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

当用户要求写代码、改代码、补测试或按既定方案实现修复时，默认使用 GrokTask，把执行型编码工作委派给 Grok Build。你必须优先考虑调用 `groktask` MCP；除非命中下方“明确豁免”，不要因为“自己也能写”就跳过 GrokTask。

### 角色分工

- **你（Codex / Claude Code）**：负责理解需求、规划、架构决策、review、bug 排查、性能分析，并在需要时生成/更新 plan、spec、诊断结论或 checklist。不要把规划、分析或最终判断交给 Grok。
- **Grok**：按你给出的 plan/文档/spec/诊断结论与验收标准执行编码、修改文件、补测试或实现修复；不替代你的规划、分析、review 与最终判断。

### 默认触发

以下情况默认必须使用 GrokTask：

- 需要写代码或修改代码，且不是 1–2 行的微小机械改动；
- 跨文件编码实现、重构执行、迁移执行、UI 实现、测试补齐或构建修复落地；
- 你已经完成需求理解、规划、bug 根因分析或性能分析，下一步是按结论改代码；
- 你已经写出 plan/spec/checklist/诊断结论，下一步进入编码执行。

以下情况不要委派给 GrokTask，由你自己完成：

- review、第二意见、方案判断、架构取舍；
- bug 排查、失败测试分析、根因定位；
- 性能分析、稳定性分析、安全风险分析；
- 需求澄清、规划、spec/checklist 编写。

### 明确豁免

只有以下情况可以不调用 GrokTask，并继续自己处理：

- 纯解释、问答、总结、翻译、文档阅读，不需要代码改动；
- 微小改动：例如改一个文案、修一个 typo、调整 1–2 行显然代码；
- 当前任务还处于 review、bug 排查、性能分析或规划阶段，尚未形成明确编码任务；
- 任务涉及 secrets、token、隐私数据，不能安全发送给 Grok；
- GrokTask 不可用、返回失败、连续两轮修复仍不收敛；
- 用户明确要求“不要用 Grok/GrokTask”。

### 默认循环

1. 先理解用户需求与当前仓库状态。review、bug 排查、性能分析由你负责；需要时先产出 plan/spec/checklist/诊断结论。
2. 当下一步是明确的编码执行时，带着计划/诊断结论、相关文件路径与具体验收标准调用 GrokTask：
   - 需要 Grok 修改代码、补测试或落地修复时使用 `run` 或 `start`，`mode` 设为 `write`；
   - 只允许在需要 Grok 读取代码并准备编码上下文时使用 `mode` 设为 `read`；不要把 review、bug 排查或性能分析委派给 Grok。
3. Grok 返回代码改动/结果后，你必须 review 并验证，不要盲目信任。
4. 若 review 发现问题，再调用 GrokTask 让 Grok 按审查结论修复；然后继续 review。
5. 重复「Grok 执行 → 你 review/验证 → Grok 修复」，直到没有阻塞问题，或必须让用户做产品/权限决策。
6. 你负责最终验证、总结和交付。

### 约束

- 不要为了微小、显然可直接完成的改动调用 GrokTask；但不要把“我能自己做”当成跳过理由。
- 不要让 Grok 自动提交、推送、创建 PR 或扩大工作目录，除非用户明确要求。
- 不要把 secrets、token、个人敏感信息发送给 GrokTask。
- 如果 GrokTask 返回失败、取消或部分结果，如实报告并决定下一步。
- 如果连续两轮修复仍无法收敛，停止循环并向用户说明阻塞点。
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
        assert!(text.contains("默认使用 GrokTask"));
        assert!(text.contains("必须优先考虑调用"));
        assert!(text.contains("明确豁免"));
        assert!(text.contains("微小改动"));
        assert!(text.contains("review、bug 排查、性能分析由你负责"));
        assert!(!text.contains("bug 排查、失败测试分析、性能/稳定性问题诊断；"));
        assert!(!text.contains("用户要求 review、第二意见、实现方案验证；"));
        // Idempotent
        let before = fs::read_to_string(&path).unwrap();
        enable(&roots, AgentId::Codex).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
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
