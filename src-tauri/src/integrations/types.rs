//! Shared types for Agent MCP integrations.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// MCP server key written into Agent configs.
pub const SERVER_NAME: &str = "groktask";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentId {
    Codex,
    Claude,
}

impl AgentId {
    pub fn as_str(self) -> &'static str {
        match self {
            AgentId::Codex => "codex",
            AgentId::Claude => "claude",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "codex" => Some(AgentId::Codex),
            "claude" => Some(AgentId::Claude),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationStatus {
    NotInstalled,
    Installed,
    Outdated,
    InvalidConfig,
    Unavailable,
}

impl IntegrationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            IntegrationStatus::NotInstalled => "not_installed",
            IntegrationStatus::Installed => "installed",
            IntegrationStatus::Outdated => "outdated",
            IntegrationStatus::InvalidConfig => "invalid_config",
            IntegrationStatus::Unavailable => "unavailable",
        }
    }
}

/// Global user-level workflow instruction injection status
/// (`~/.codex/AGENTS.md` / `~/.claude/CLAUDE.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStatus {
    NotEnabled,
    Enabled,
    Outdated,
    InvalidFile,
    Unavailable,
}

impl WorkflowStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            WorkflowStatus::NotEnabled => "not_enabled",
            WorkflowStatus::Enabled => "enabled",
            WorkflowStatus::Outdated => "outdated",
            WorkflowStatus::InvalidFile => "invalid_file",
            WorkflowStatus::Unavailable => "unavailable",
        }
    }
}

/// Desired MCP entry values for install/update comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpEntryTemplate {
    pub command: String,
    pub args: Vec<String>,
    pub codex_startup_timeout_sec: i64,
    pub codex_tool_timeout_sec: i64,
    pub claude_timeout_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentIntegrationStatus {
    pub agent: AgentId,
    /// MCP server install status (legacy field name kept for compatibility).
    pub status: IntegrationStatus,
    pub config_path: String,
    pub binary_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// True when Install/Update may safely write MCP config.
    pub can_write: bool,
    /// True when Remove may safely write (entry may or may not exist).
    pub can_remove: bool,
    /// Workflow instruction injection status (global AGENTS.md / CLAUDE.md).
    #[serde(default = "default_workflow_status")]
    pub workflow_status: WorkflowStatus,
    /// Absolute path of the global user instruction file for workflow injection.
    #[serde(default)]
    pub workflow_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_detail: Option<String>,
    /// True when enable/update workflow instruction may safely write.
    #[serde(default = "default_true")]
    pub can_write_workflow: bool,
}

fn default_workflow_status() -> WorkflowStatus {
    WorkflowStatus::NotEnabled
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatusReport {
    pub agents: Vec<AgentIntegrationStatus>,
}

#[derive(Debug, Error)]
pub enum IntegrationError {
    #[error("integration I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid agent config: {0}")]
    InvalidConfig(String),
    #[error("integration unavailable: {0}")]
    Unavailable(String),
    #[error("validation error: {0}")]
    Validation(String),
}

impl IntegrationError {
    pub fn code(&self) -> &'static str {
        match self {
            IntegrationError::Io(_) => "io",
            IntegrationError::InvalidConfig(_) => "invalid_config",
            IntegrationError::Unavailable(_) => "unavailable",
            IntegrationError::Validation(_) => "validation",
        }
    }
}
