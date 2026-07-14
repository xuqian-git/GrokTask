//! Codex user-level MCP integration via `~/.codex/config.toml`.
//!
//! Only edits `[mcp_servers.groktask]`. Uses `toml_edit` to preserve comments
//! and unrelated tables.

use super::types::{
    AgentId, AgentIntegrationStatus, IntegrationError, IntegrationStatus, McpEntryTemplate,
    SERVER_NAME,
};
use crate::config::atomic_write;
use std::fs;
use std::path::{Path, PathBuf};
use toml_edit::{value, Array, DocumentMut, Item, Table};

#[derive(Debug, Clone)]
pub struct CodexPaths {
    pub config_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct CodexIntegration {
    paths: CodexPaths,
}

impl CodexIntegration {
    pub fn new(paths: CodexPaths) -> Self {
        Self { paths }
    }

    pub fn config_path(&self) -> &Path {
        &self.paths.config_path
    }

    pub fn status(&self, template: &McpEntryTemplate) -> AgentIntegrationStatus {
        let path = self.config_path();
        let display = path.display().to_string();
        let binary = template.command.clone();

        // Parent dir missing is not an error — treat as not installed.
        if !path.exists() {
            return AgentIntegrationStatus {
                agent: AgentId::Codex,
                status: IntegrationStatus::NotInstalled,
                config_path: display,
                binary_path: binary,
                detail: None,
                can_write: true,
                can_remove: true,
            };
        }

        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => {
                return AgentIntegrationStatus {
                    agent: AgentId::Codex,
                    status: IntegrationStatus::Unavailable,
                    config_path: display,
                    binary_path: binary,
                    detail: Some(format!("cannot read config: {e}")),
                    can_write: false,
                    can_remove: false,
                };
            }
        };

        if text.trim().is_empty() {
            return AgentIntegrationStatus {
                agent: AgentId::Codex,
                status: IntegrationStatus::NotInstalled,
                config_path: display,
                binary_path: binary,
                detail: None,
                can_write: true,
                can_remove: true,
            };
        }

        let doc = match text.parse::<DocumentMut>() {
            Ok(d) => d,
            Err(e) => {
                return AgentIntegrationStatus {
                    agent: AgentId::Codex,
                    status: IntegrationStatus::InvalidConfig,
                    config_path: display,
                    binary_path: binary,
                    detail: Some(format!("invalid TOML: {e}")),
                    can_write: false,
                    can_remove: false,
                };
            }
        };

        // Parent type wrong → invalid
        if let Some(servers) = doc.get("mcp_servers") {
            if !servers.is_table_like() {
                return AgentIntegrationStatus {
                    agent: AgentId::Codex,
                    status: IntegrationStatus::InvalidConfig,
                    config_path: display,
                    binary_path: binary,
                    detail: Some("`mcp_servers` is not a table".into()),
                    can_write: false,
                    can_remove: false,
                };
            }
        }

        match entry_status(&doc, template) {
            EntryState::Absent => AgentIntegrationStatus {
                agent: AgentId::Codex,
                status: IntegrationStatus::NotInstalled,
                config_path: display,
                binary_path: binary,
                detail: None,
                can_write: true,
                can_remove: true,
            },
            EntryState::Matches => AgentIntegrationStatus {
                agent: AgentId::Codex,
                status: IntegrationStatus::Installed,
                config_path: display,
                binary_path: binary,
                detail: None,
                can_write: true,
                can_remove: true,
            },
            EntryState::Mismatch => AgentIntegrationStatus {
                agent: AgentId::Codex,
                status: IntegrationStatus::Outdated,
                config_path: display,
                binary_path: binary,
                detail: Some("command, args, or timeouts differ from template".into()),
                can_write: true,
                can_remove: true,
            },
            EntryState::InvalidParent => AgentIntegrationStatus {
                agent: AgentId::Codex,
                status: IntegrationStatus::InvalidConfig,
                config_path: display,
                binary_path: binary,
                detail: Some("`mcp_servers.groktask` is not a table".into()),
                can_write: false,
                can_remove: false,
            },
        }
    }

    pub fn install(&self, template: &McpEntryTemplate) -> Result<(), IntegrationError> {
        let path = self.config_path();
        let existing = if path.exists() {
            fs::read_to_string(path)?
        } else {
            String::new()
        };
        let updated = apply_install_toml(&existing, template)?;
        // Idempotent: skip write if bytes unchanged.
        if updated == existing {
            return Ok(());
        }
        write_config(path, &updated)
    }

    pub fn remove(&self) -> Result<(), IntegrationError> {
        let path = self.config_path();
        if !path.exists() {
            return Ok(());
        }
        let existing = fs::read_to_string(path)?;
        if existing.trim().is_empty() {
            return Ok(());
        }
        let updated = apply_uninstall_toml(&existing)?;
        if updated == existing {
            return Ok(());
        }
        write_config(path, &updated)
    }
}

#[derive(Debug, PartialEq, Eq)]
enum EntryState {
    Absent,
    Matches,
    Mismatch,
    InvalidParent,
}

fn entry_status(doc: &DocumentMut, template: &McpEntryTemplate) -> EntryState {
    let Some(servers) = doc.get("mcp_servers").and_then(|i| i.as_table_like()) else {
        return EntryState::Absent;
    };
    let Some(entry_item) = servers.get(SERVER_NAME) else {
        return EntryState::Absent;
    };
    let Some(entry) = entry_item.as_table_like() else {
        return EntryState::InvalidParent;
    };

    let cmd_ok = entry.get("command").and_then(|i| i.as_str()) == Some(template.command.as_str());
    let args_ok = entry
        .get("args")
        .and_then(|i| i.as_array())
        .map(|a| {
            a.len() == template.args.len()
                && template.args.iter().enumerate().all(|(i, expected)| {
                    a.get(i).and_then(|x| x.as_str()) == Some(expected.as_str())
                })
        })
        .unwrap_or(false);
    let startup_ok = entry.get("startup_timeout_sec").and_then(toml_int)
        == Some(template.codex_startup_timeout_sec);
    let tool_ok =
        entry.get("tool_timeout_sec").and_then(toml_int) == Some(template.codex_tool_timeout_sec);

    if cmd_ok && args_ok && startup_ok && tool_ok {
        EntryState::Matches
    } else {
        EntryState::Mismatch
    }
}

/// Tolerate integer and whole-number float timeouts (Codex may normalize `30` → `30.0`).
fn toml_int(item: &Item) -> Option<i64> {
    if let Some(i) = item.as_integer() {
        return Some(i);
    }
    let f = item.as_float()?;
    if f.fract() == 0.0 {
        Some(f as i64)
    } else {
        None
    }
}

fn apply_install_toml(text: &str, template: &McpEntryTemplate) -> Result<String, IntegrationError> {
    let mut doc = if text.trim().is_empty() {
        DocumentMut::new()
    } else {
        text.parse::<DocumentMut>()
            .map_err(|e| IntegrationError::InvalidConfig(format!("invalid TOML: {e}")))?
    };

    if !doc.as_table().contains_key("mcp_servers") {
        let mut t = Table::new();
        t.set_implicit(true);
        doc.as_table_mut().insert("mcp_servers", Item::Table(t));
    }
    let servers = doc
        .as_table_mut()
        .get_mut("mcp_servers")
        .and_then(Item::as_table_mut)
        .ok_or_else(|| IntegrationError::InvalidConfig("`mcp_servers` is not a table".into()))?;

    if !servers.contains_key(SERVER_NAME) {
        servers.insert(SERVER_NAME, Item::Table(Table::new()));
    }
    let entry = servers
        .get_mut(SERVER_NAME)
        .and_then(Item::as_table_mut)
        .ok_or_else(|| {
            IntegrationError::InvalidConfig("`mcp_servers.groktask` is not a table".into())
        })?;

    entry.insert("command", value(template.command.as_str()));
    let mut args = Array::new();
    for a in &template.args {
        args.push(a.as_str());
    }
    entry.insert("args", value(args));
    entry.insert(
        "startup_timeout_sec",
        value(template.codex_startup_timeout_sec),
    );
    entry.insert("tool_timeout_sec", value(template.codex_tool_timeout_sec));
    Ok(doc.to_string())
}

fn apply_uninstall_toml(text: &str) -> Result<String, IntegrationError> {
    if text.trim().is_empty() {
        return Ok(text.to_string());
    }
    let mut doc = text
        .parse::<DocumentMut>()
        .map_err(|e| IntegrationError::InvalidConfig(format!("invalid TOML: {e}")))?;
    if let Some(servers) = doc.get_mut("mcp_servers").and_then(Item::as_table_mut) {
        servers.remove(SERVER_NAME);
        if servers.is_empty() {
            doc.as_table_mut().remove("mcp_servers");
        }
    }
    Ok(doc.to_string())
}

fn write_config(path: &Path, text: &str) -> Result<(), IntegrationError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    // Preserve permissions when overwriting.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::mcp_template;
    use tempfile::TempDir;

    fn tmpl(cmd: &str) -> McpEntryTemplate {
        mcp_template(cmd)
    }

    #[test]
    fn not_installed_to_installed() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        let integ = CodexIntegration::new(CodexPaths {
            config_path: path.clone(),
        });
        let t = tmpl("/opt/GrokTask");
        assert_eq!(integ.status(&t).status, IntegrationStatus::NotInstalled);
        integ.install(&t).unwrap();
        assert_eq!(integ.status(&t).status, IntegrationStatus::Installed);
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("[mcp_servers.groktask]"));
        assert!(text.contains("command = \"/opt/GrokTask\""));
        assert!(text.contains("startup_timeout_sec = 30"));
        assert!(text.contains("tool_timeout_sec = 86400"));
    }

    #[test]
    fn outdated_path_updates_to_installed() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        let integ = CodexIntegration::new(CodexPaths {
            config_path: path.clone(),
        });
        integ.install(&tmpl("/old/GrokTask")).unwrap();
        let t = tmpl("/new/GrokTask");
        assert_eq!(integ.status(&t).status, IntegrationStatus::Outdated);
        integ.install(&t).unwrap();
        assert_eq!(integ.status(&t).status, IntegrationStatus::Installed);
    }

    #[test]
    fn outdated_timeout_updates() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        fs::write(
            &path,
            r#"
[mcp_servers.groktask]
command = "/opt/GrokTask"
args = ["mcp"]
startup_timeout_sec = 5
tool_timeout_sec = 60
"#,
        )
        .unwrap();
        let integ = CodexIntegration::new(CodexPaths { config_path: path });
        let t = tmpl("/opt/GrokTask");
        assert_eq!(integ.status(&t).status, IntegrationStatus::Outdated);
        integ.install(&t).unwrap();
        assert_eq!(integ.status(&t).status, IntegrationStatus::Installed);
    }

    #[test]
    fn remove_only_groktask_preserves_others_and_comments() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        let input = r#"# keep me
[mcp_servers.other]
command = "x"
args = []

[mcp_servers.groktask]
command = "/opt/GrokTask"
args = ["mcp"]
startup_timeout_sec = 30
tool_timeout_sec = 86400

[features]
foo = true
"#;
        fs::write(&path, input).unwrap();
        let integ = CodexIntegration::new(CodexPaths {
            config_path: path.clone(),
        });
        integ.remove().unwrap();
        let out = fs::read_to_string(&path).unwrap();
        assert!(out.contains("# keep me"));
        assert!(out.contains("[mcp_servers.other]"));
        assert!(out.contains("command = \"x\""));
        assert!(!out.contains("groktask"));
        assert!(out.contains("[features]"));
        assert_eq!(
            integ.status(&tmpl("/opt/GrokTask")).status,
            IntegrationStatus::NotInstalled
        );
    }

    #[test]
    fn invalid_toml_does_not_write() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        let bad = "[[[not valid";
        fs::write(&path, bad).unwrap();
        let before = fs::read(&path).unwrap();
        let integ = CodexIntegration::new(CodexPaths {
            config_path: path.clone(),
        });
        let t = tmpl("/opt/GrokTask");
        assert_eq!(integ.status(&t).status, IntegrationStatus::InvalidConfig);
        assert!(integ.install(&t).is_err());
        assert_eq!(fs::read(&path).unwrap(), before, "bytes must be unchanged");
    }

    #[test]
    fn install_idempotent() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        let integ = CodexIntegration::new(CodexPaths {
            config_path: path.clone(),
        });
        let t = tmpl("/opt/GrokTask");
        integ.install(&t).unwrap();
        let a = fs::read_to_string(&path).unwrap();
        integ.install(&t).unwrap();
        let b = fs::read_to_string(&path).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn remove_absent_is_noop() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        let integ = CodexIntegration::new(CodexPaths { config_path: path });
        integ.remove().unwrap();
    }

    #[test]
    fn float_timeout_still_matches() {
        let text = r#"
[mcp_servers.groktask]
command = "/opt/GrokTask"
args = ["mcp"]
startup_timeout_sec = 30.0
tool_timeout_sec = 86400.0
"#;
        let doc = text.parse::<DocumentMut>().unwrap();
        assert_eq!(
            entry_status(&doc, &tmpl("/opt/GrokTask")),
            EntryState::Matches
        );
    }
}
