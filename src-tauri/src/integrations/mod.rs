//! Agent MCP integration management (Codex + Claude Code) and global-user
//! workflow instruction injection.
//!
//! Only mutates each target's `groktask` MCP entry and GrokTask managed
//! instruction blocks. Tests must use temp config roots (`IntegrationRoots::from_home`)
//! and never touch real `~/.codex`, `~/.claude.json`, or real user instruction files.

mod claude;
mod codex;
mod types;
pub mod workflow;

pub use claude::{ClaudeIntegration, ClaudePaths};
pub use codex::{CodexIntegration, CodexPaths};
pub use types::{
    AgentId, AgentIntegrationStatus, AgentStatusReport, IntegrationError, IntegrationStatus,
    McpEntryTemplate, WorkflowStatus,
};

// Ensure IntegrationStatus stays in the public surface used by CLI/tests.
#[allow(dead_code)]
fn _status_surface() -> IntegrationStatus {
    IntegrationStatus::NotInstalled
}

use std::path::{Path, PathBuf};

/// Resolve the absolute path of the current GrokTask executable.
pub fn current_exe_path() -> Result<PathBuf, IntegrationError> {
    let p = std::env::current_exe().map_err(|e| {
        IntegrationError::Unavailable(format!("cannot resolve current executable: {e}"))
    })?;
    // Prefer canonical path when available so config stores a stable absolute path.
    Ok(dunce_canonicalize(&p).unwrap_or(p))
}

fn dunce_canonicalize(p: &Path) -> std::io::Result<PathBuf> {
    // std::fs::canonicalize on macOS may produce /private/var…; fine for config data.
    std::fs::canonicalize(p)
}

/// Default paths under the real user home (or overridden roots for tests).
#[derive(Debug, Clone)]
pub struct IntegrationRoots {
    pub home: PathBuf,
}

impl IntegrationRoots {
    pub fn from_home(home: impl Into<PathBuf>) -> Self {
        Self { home: home.into() }
    }

    pub fn user_default() -> Self {
        Self {
            home: crate::paths::home(),
        }
    }

    pub fn codex_config(&self) -> PathBuf {
        self.home.join(".codex").join("config.toml")
    }

    pub fn claude_config(&self) -> PathBuf {
        self.home.join(".claude.json")
    }

    /// Global Codex instruction file: `<home>/.codex/AGENTS.md`.
    pub fn codex_agents_md(&self) -> PathBuf {
        self.home.join(".codex").join("AGENTS.md")
    }

    /// Global Claude Code instruction file: `<home>/.claude/CLAUDE.md`.
    pub fn claude_claude_md(&self) -> PathBuf {
        self.home.join(".claude").join("CLAUDE.md")
    }
}

/// Template written for both agents: command = absolute GrokTask, args = ["mcp"].
pub fn mcp_template(command: impl Into<String>) -> McpEntryTemplate {
    McpEntryTemplate {
        command: command.into(),
        args: vec!["mcp".into()],
        codex_startup_timeout_sec: 30,
        codex_tool_timeout_sec: 86_400,
        claude_timeout_ms: 86_400_000,
    }
}

/// Attach global workflow inspection fields onto an MCP status card.
fn with_workflow(
    mut status: AgentIntegrationStatus,
    roots: &IntegrationRoots,
) -> AgentIntegrationStatus {
    let w = workflow::inspect(roots, status.agent);
    status.workflow_status = w.status;
    status.workflow_path = w.path;
    status.workflow_detail = w.detail;
    status.can_write_workflow = w.can_write;
    status
}

/// Status for one or both agents (MCP + global workflow). No workspace required.
pub fn status_report(
    roots: &IntegrationRoots,
    filter: Option<AgentId>,
    command: &str,
) -> AgentStatusReport {
    let template = mcp_template(command);
    let mut agents = Vec::new();
    let want = |id: AgentId| match filter {
        None => true,
        Some(f) => f == id,
    };

    if want(AgentId::Codex) {
        let codex = CodexIntegration::new(CodexPaths {
            config_path: roots.codex_config(),
        });
        agents.push(with_workflow(codex.status(&template), roots));
    }
    if want(AgentId::Claude) {
        let claude = ClaudeIntegration::new(ClaudePaths {
            config_path: roots.claude_config(),
        });
        agents.push(with_workflow(claude.status(&template), roots));
    }
    AgentStatusReport { agents }
}

/// Install or update MCP entry for `agent`.
pub fn install(
    roots: &IntegrationRoots,
    agent: AgentId,
    command: &str,
) -> Result<AgentIntegrationStatus, IntegrationError> {
    let template = mcp_template(command);
    match agent {
        AgentId::Codex => {
            let codex = CodexIntegration::new(CodexPaths {
                config_path: roots.codex_config(),
            });
            codex.install(&template)?;
            Ok(with_workflow(codex.status(&template), roots))
        }
        AgentId::Claude => {
            let claude = ClaudeIntegration::new(ClaudePaths {
                config_path: roots.claude_config(),
            });
            claude.install(&template)?;
            Ok(with_workflow(claude.status(&template), roots))
        }
    }
}

/// Remove MCP entry for `agent` (no-op if absent).
pub fn remove(
    roots: &IntegrationRoots,
    agent: AgentId,
    command: &str,
) -> Result<AgentIntegrationStatus, IntegrationError> {
    let template = mcp_template(command);
    match agent {
        AgentId::Codex => {
            let codex = CodexIntegration::new(CodexPaths {
                config_path: roots.codex_config(),
            });
            codex.remove()?;
            Ok(with_workflow(codex.status(&template), roots))
        }
        AgentId::Claude => {
            let claude = ClaudeIntegration::new(ClaudePaths {
                config_path: roots.claude_config(),
            });
            claude.remove()?;
            Ok(with_workflow(claude.status(&template), roots))
        }
    }
}

/// Set mode `mcp` (install) or `none` (remove).
pub fn set_mode(
    roots: &IntegrationRoots,
    agent: AgentId,
    mode: &str,
    command: &str,
) -> Result<AgentIntegrationStatus, IntegrationError> {
    match mode {
        "mcp" => install(roots, agent, command),
        "none" => remove(roots, agent, command),
        other => Err(IntegrationError::Validation(format!(
            "unknown mode `{other}`; expected mcp|none"
        ))),
    }
}

/// Enable global workflow instructions for `agent` under `roots`.
pub fn workflow_enable(
    roots: &IntegrationRoots,
    agent: AgentId,
) -> Result<workflow::WorkflowInspection, IntegrationError> {
    workflow::enable(roots, agent)
}

/// Disable global workflow instructions for `agent` under `roots`.
pub fn workflow_disable(
    roots: &IntegrationRoots,
    agent: AgentId,
) -> Result<workflow::WorkflowInspection, IntegrationError> {
    workflow::disable(roots, agent)
}

/// Inspect workflow only (no MCP).
pub fn workflow_status(roots: &IntegrationRoots, agent: AgentId) -> workflow::WorkflowInspection {
    workflow::inspect(roots, agent)
}

/// Resolve workspace for CLI/UI: explicit path, else current directory.
///
/// Still used for task cwd / MCP context / setup navigation — not for workflow
/// instruction target resolution (workflow is global under [`IntegrationRoots`]).
pub fn resolve_workspace(cwd: Option<&Path>) -> Result<PathBuf, IntegrationError> {
    match cwd {
        Some(p) => {
            let abs = if p.is_absolute() {
                p.to_path_buf()
            } else {
                std::env::current_dir()
                    .map_err(|e| IntegrationError::Unavailable(e.to_string()))?
                    .join(p)
            };
            Ok(dunce_canonicalize(&abs).unwrap_or(abs))
        }
        None => {
            let cwd = std::env::current_dir()
                .map_err(|e| IntegrationError::Unavailable(format!("cannot resolve cwd: {e}")))?;
            Ok(dunce_canonicalize(&cwd).unwrap_or(cwd))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn status_not_installed_on_empty_home() {
        let tmp = TempDir::new().unwrap();
        let roots = IntegrationRoots::from_home(tmp.path());
        let report = status_report(&roots, None, "/tmp/GrokTask");
        assert_eq!(report.agents.len(), 2);
        assert_eq!(report.agents[0].status, IntegrationStatus::NotInstalled);
        assert_eq!(report.agents[1].status, IntegrationStatus::NotInstalled);
        // Workflow is global — no workspace required; empty home → not_enabled.
        assert_eq!(report.agents[0].workflow_status, WorkflowStatus::NotEnabled);
        assert!(report.agents[0].can_write_workflow);
        assert!(
            report.agents[0].workflow_path.ends_with(".codex/AGENTS.md")
                || report.agents[0]
                    .workflow_path
                    .ends_with(".codex\\AGENTS.md")
        );
        assert!(
            report.agents[1]
                .workflow_path
                .ends_with(".claude/CLAUDE.md")
                || report.agents[1]
                    .workflow_path
                    .ends_with(".claude\\CLAUDE.md")
        );
    }

    #[test]
    fn install_remove_codex_and_claude_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let roots = IntegrationRoots::from_home(tmp.path());
        let cmd = "/opt/GrokTask";

        let s = install(&roots, AgentId::Codex, cmd).unwrap();
        assert_eq!(s.status, IntegrationStatus::Installed);
        let s = remove(&roots, AgentId::Codex, cmd).unwrap();
        assert_eq!(s.status, IntegrationStatus::NotInstalled);

        let s = install(&roots, AgentId::Claude, cmd).unwrap();
        assert_eq!(s.status, IntegrationStatus::Installed);
        let s = remove(&roots, AgentId::Claude, cmd).unwrap();
        assert_eq!(s.status, IntegrationStatus::NotInstalled);
    }

    #[test]
    fn set_mode_rejects_unknown() {
        let tmp = TempDir::new().unwrap();
        let roots = IntegrationRoots::from_home(tmp.path());
        let err = set_mode(&roots, AgentId::Codex, "plugin", "/x").unwrap_err();
        assert!(matches!(err, IntegrationError::Validation(_)));
    }

    #[test]
    fn mcp_installed_workflow_disabled() {
        let home = TempDir::new().unwrap();
        let roots = IntegrationRoots::from_home(home.path());
        install(&roots, AgentId::Codex, "/opt/GrokTask").unwrap();
        let report = status_report(&roots, Some(AgentId::Codex), "/opt/GrokTask");
        assert_eq!(report.agents[0].status, IntegrationStatus::Installed);
        assert_eq!(report.agents[0].workflow_status, WorkflowStatus::NotEnabled);
        assert!(
            report.agents[0].workflow_path.ends_with(".codex/AGENTS.md")
                || report.agents[0]
                    .workflow_path
                    .ends_with(".codex\\AGENTS.md")
        );
    }

    #[test]
    fn mcp_installed_and_workflow_enabled() {
        let home = TempDir::new().unwrap();
        let roots = IntegrationRoots::from_home(home.path());
        install(&roots, AgentId::Claude, "/opt/GrokTask").unwrap();
        workflow_enable(&roots, AgentId::Claude).unwrap();
        let report = status_report(&roots, Some(AgentId::Claude), "/opt/GrokTask");
        assert_eq!(report.agents[0].status, IntegrationStatus::Installed);
        assert_eq!(report.agents[0].workflow_status, WorkflowStatus::Enabled);
        assert!(
            report.agents[0]
                .workflow_path
                .ends_with(".claude/CLAUDE.md")
                || report.agents[0]
                    .workflow_path
                    .ends_with(".claude\\CLAUDE.md")
        );
        assert!(home.path().join(".claude").join("CLAUDE.md").exists());
    }

    #[test]
    fn workflow_outdated_and_invalid() {
        let home = TempDir::new().unwrap();
        let roots = IntegrationRoots::from_home(home.path());
        let path = roots.codex_agents_md();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // Outdated body
        std::fs::write(
            &path,
            format!("{}\nold\n{}\n", workflow::BLOCK_BEGIN, workflow::BLOCK_END),
        )
        .unwrap();
        let report = status_report(&roots, Some(AgentId::Codex), "/x");
        assert_eq!(report.agents[0].workflow_status, WorkflowStatus::Outdated);

        // Invalid markers
        std::fs::write(&path, format!("{}\nno end\n", workflow::BLOCK_BEGIN)).unwrap();
        let report = status_report(&roots, Some(AgentId::Codex), "/x");
        assert_eq!(
            report.agents[0].workflow_status,
            WorkflowStatus::InvalidFile
        );
        assert!(!report.agents[0].can_write_workflow);
    }
}
