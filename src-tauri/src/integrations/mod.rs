//! Agent MCP integration management (Codex + Claude Code).
//!
//! Only mutates each target's `groktask` entry. Tests must use temp config roots
//! and never touch real `~/.codex` or `~/.claude.json`.

mod claude;
mod codex;
mod types;

pub use claude::{ClaudeIntegration, ClaudePaths};
pub use codex::{CodexIntegration, CodexPaths};
pub use types::{
    AgentId, AgentIntegrationStatus, AgentStatusReport, IntegrationError, IntegrationStatus,
    McpEntryTemplate,
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

/// Status for one or both agents.
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
        agents.push(codex.status(&template));
    }
    if want(AgentId::Claude) {
        let claude = ClaudeIntegration::new(ClaudePaths {
            config_path: roots.claude_config(),
        });
        agents.push(claude.status(&template));
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
            Ok(codex.status(&template))
        }
        AgentId::Claude => {
            let claude = ClaudeIntegration::new(ClaudePaths {
                config_path: roots.claude_config(),
            });
            claude.install(&template)?;
            Ok(claude.status(&template))
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
            Ok(codex.status(&template))
        }
        AgentId::Claude => {
            let claude = ClaudeIntegration::new(ClaudePaths {
                config_path: roots.claude_config(),
            });
            claude.remove()?;
            Ok(claude.status(&template))
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
}
