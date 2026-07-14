//! Claude Code user-level MCP integration via `~/.claude.json`.
//!
//! Only edits top-level `mcpServers.groktask`. Uses serde_json for semantic
//! minimal edits (unrelated keys preserved; exact whitespace/comments are not
//! guaranteed — invalid JSON never writes).

use super::types::{
    AgentId, AgentIntegrationStatus, IntegrationError, IntegrationStatus, McpEntryTemplate,
    SERVER_NAME,
};
use crate::config::atomic_write;
use serde_json::{json, Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ClaudePaths {
    pub config_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ClaudeIntegration {
    paths: ClaudePaths,
}

impl ClaudeIntegration {
    pub fn new(paths: ClaudePaths) -> Self {
        Self { paths }
    }

    pub fn config_path(&self) -> &Path {
        &self.paths.config_path
    }

    pub fn status(&self, template: &McpEntryTemplate) -> AgentIntegrationStatus {
        let path = self.config_path();
        let display = path.display().to_string();
        let binary = template.command.clone();

        if !path.exists() {
            return AgentIntegrationStatus {
                agent: AgentId::Claude,
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
                    agent: AgentId::Claude,
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
                agent: AgentId::Claude,
                status: IntegrationStatus::NotInstalled,
                config_path: display,
                binary_path: binary,
                detail: None,
                can_write: true,
                can_remove: true,
            };
        }

        let value = match serde_json::from_str::<Value>(&text) {
            Ok(v) => v,
            Err(e) => {
                return AgentIntegrationStatus {
                    agent: AgentId::Claude,
                    status: IntegrationStatus::InvalidConfig,
                    config_path: display,
                    binary_path: binary,
                    detail: Some(format!("invalid JSON: {e}")),
                    can_write: false,
                    can_remove: false,
                };
            }
        };

        let Some(root) = value.as_object() else {
            return AgentIntegrationStatus {
                agent: AgentId::Claude,
                status: IntegrationStatus::InvalidConfig,
                config_path: display,
                binary_path: binary,
                detail: Some("root is not a JSON object".into()),
                can_write: false,
                can_remove: false,
            };
        };

        if let Some(servers) = root.get("mcpServers") {
            if !servers.is_object() {
                return AgentIntegrationStatus {
                    agent: AgentId::Claude,
                    status: IntegrationStatus::InvalidConfig,
                    config_path: display,
                    binary_path: binary,
                    detail: Some("`mcpServers` is not an object".into()),
                    can_write: false,
                    can_remove: false,
                };
            }
        }

        match entry_status(&value, template) {
            EntryState::Absent => AgentIntegrationStatus {
                agent: AgentId::Claude,
                status: IntegrationStatus::NotInstalled,
                config_path: display,
                binary_path: binary,
                detail: None,
                can_write: true,
                can_remove: true,
            },
            EntryState::Matches => AgentIntegrationStatus {
                agent: AgentId::Claude,
                status: IntegrationStatus::Installed,
                config_path: display,
                binary_path: binary,
                detail: None,
                can_write: true,
                can_remove: true,
            },
            EntryState::Mismatch => AgentIntegrationStatus {
                agent: AgentId::Claude,
                status: IntegrationStatus::Outdated,
                config_path: display,
                binary_path: binary,
                detail: Some("command, args, or timeout differ from template".into()),
                can_write: true,
                can_remove: true,
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
        let updated = apply_install_json(&existing, template)?;
        if updated == existing {
            return Ok(());
        }
        // Semantic no-op check (pretty-print may differ from source).
        if !existing.trim().is_empty() {
            if let (Ok(a), Ok(b)) = (
                serde_json::from_str::<Value>(&existing),
                serde_json::from_str::<Value>(&updated),
            ) {
                if a == b {
                    return Ok(());
                }
            }
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
        let updated = apply_uninstall_json(&existing)?;
        if let (Ok(a), Ok(b)) = (
            serde_json::from_str::<Value>(&existing),
            serde_json::from_str::<Value>(&updated),
        ) {
            if a == b {
                return Ok(());
            }
        }
        write_config(path, &updated)
    }
}

#[derive(Debug, PartialEq, Eq)]
enum EntryState {
    Absent,
    Matches,
    Mismatch,
}

fn entry_status(value: &Value, template: &McpEntryTemplate) -> EntryState {
    let Some(entry) = value
        .get("mcpServers")
        .and_then(|s| s.as_object())
        .and_then(|m| m.get(SERVER_NAME))
    else {
        return EntryState::Absent;
    };
    if entry_matches(entry, template) {
        EntryState::Matches
    } else {
        EntryState::Mismatch
    }
}

fn entry_matches(entry: &Value, template: &McpEntryTemplate) -> bool {
    let cmd_ok = entry.get("command").and_then(|v| v.as_str()) == Some(template.command.as_str());
    let args_ok = entry
        .get("args")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.len() == template.args.len()
                && template.args.iter().enumerate().all(|(i, expected)| {
                    a.get(i).and_then(|x| x.as_str()) == Some(expected.as_str())
                })
        })
        .unwrap_or(false);
    let timeout_ok =
        entry.get("timeout").and_then(|v| v.as_i64()) == Some(template.claude_timeout_ms);
    cmd_ok && args_ok && timeout_ok
}

fn desired_entry(template: &McpEntryTemplate) -> Value {
    json!({
        "command": template.command,
        "args": template.args,
        "timeout": template.claude_timeout_ms,
    })
}

fn apply_install_json(text: &str, template: &McpEntryTemplate) -> Result<String, IntegrationError> {
    let mut root: Value = if text.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(text)
            .map_err(|e| IntegrationError::InvalidConfig(format!("invalid JSON: {e}")))?
    };

    let obj = root
        .as_object_mut()
        .ok_or_else(|| IntegrationError::InvalidConfig("root is not a JSON object".into()))?;

    match obj.get("mcpServers") {
        None => {
            let mut servers = Map::new();
            servers.insert(SERVER_NAME.into(), desired_entry(template));
            obj.insert("mcpServers".into(), Value::Object(servers));
        }
        Some(v) if v.is_object() => {
            let servers = obj
                .get_mut("mcpServers")
                .and_then(|v| v.as_object_mut())
                .expect("checked is_object");
            servers.insert(SERVER_NAME.into(), desired_entry(template));
        }
        Some(_) => {
            return Err(IntegrationError::InvalidConfig(
                "`mcpServers` is not an object".into(),
            ));
        }
    }

    Ok(serde_json::to_string_pretty(&root)
        .map_err(|e| IntegrationError::InvalidConfig(e.to_string()))?
        + "\n")
}

fn apply_uninstall_json(text: &str) -> Result<String, IntegrationError> {
    if text.trim().is_empty() {
        return Ok(text.to_string());
    }
    let mut root: Value = serde_json::from_str(text)
        .map_err(|e| IntegrationError::InvalidConfig(format!("invalid JSON: {e}")))?;
    let obj = root
        .as_object_mut()
        .ok_or_else(|| IntegrationError::InvalidConfig("root is not a JSON object".into()))?;

    if let Some(servers_val) = obj.get_mut("mcpServers") {
        if let Some(servers) = servers_val.as_object_mut() {
            servers.remove(SERVER_NAME);
            if servers.is_empty() {
                obj.remove("mcpServers");
            }
        } else {
            return Err(IntegrationError::InvalidConfig(
                "`mcpServers` is not an object".into(),
            ));
        }
    }

    Ok(serde_json::to_string_pretty(&root)
        .map_err(|e| IntegrationError::InvalidConfig(e.to_string()))?
        + "\n")
}

fn write_config(path: &Path, text: &str) -> Result<(), IntegrationError> {
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
        let path = tmp.path().join(".claude.json");
        let integ = ClaudeIntegration::new(ClaudePaths {
            config_path: path.clone(),
        });
        let t = tmpl("/opt/GrokTask");
        assert_eq!(integ.status(&t).status, IntegrationStatus::NotInstalled);
        integ.install(&t).unwrap();
        assert_eq!(integ.status(&t).status, IntegrationStatus::Installed);
        let v: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["mcpServers"]["groktask"]["command"], "/opt/GrokTask");
        assert_eq!(v["mcpServers"]["groktask"]["args"][0], "mcp");
        assert_eq!(v["mcpServers"]["groktask"]["timeout"], 86_400_000);
    }

    #[test]
    fn outdated_path_and_timeout_update() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(".claude.json");
        fs::write(
            &path,
            r#"{
  "mcpServers": {
    "groktask": {
      "command": "/old/GrokTask",
      "args": ["mcp"],
      "timeout": 60000
    }
  }
}
"#,
        )
        .unwrap();
        let integ = ClaudeIntegration::new(ClaudePaths {
            config_path: path.clone(),
        });
        let t = tmpl("/new/GrokTask");
        assert_eq!(integ.status(&t).status, IntegrationStatus::Outdated);
        integ.install(&t).unwrap();
        assert_eq!(integ.status(&t).status, IntegrationStatus::Installed);
    }

    #[test]
    fn remove_only_groktask_preserves_others() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(".claude.json");
        fs::write(
            &path,
            r#"{
  "theme": "dark",
  "mcpServers": {
    "other": { "command": "x", "args": [] },
    "groktask": { "command": "/opt/GrokTask", "args": ["mcp"], "timeout": 86400000 }
  },
  "projects": { "a": 1 }
}
"#,
        )
        .unwrap();
        let integ = ClaudeIntegration::new(ClaudePaths {
            config_path: path.clone(),
        });
        integ.remove().unwrap();
        let v: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(v["mcpServers"].get("groktask").is_none());
        assert_eq!(v["mcpServers"]["other"]["command"], "x");
        assert_eq!(v["theme"], "dark");
        assert_eq!(v["projects"]["a"], 1);
    }

    #[test]
    fn invalid_json_does_not_write() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(".claude.json");
        let bad = "{ not json";
        fs::write(&path, bad).unwrap();
        let before = fs::read(&path).unwrap();
        let integ = ClaudeIntegration::new(ClaudePaths {
            config_path: path.clone(),
        });
        let t = tmpl("/opt/GrokTask");
        assert_eq!(integ.status(&t).status, IntegrationStatus::InvalidConfig);
        assert!(integ.install(&t).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[test]
    fn wrong_parent_type_does_not_write() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(".claude.json");
        fs::write(&path, r#"{ "mcpServers": [] }"#).unwrap();
        let before = fs::read(&path).unwrap();
        let integ = ClaudeIntegration::new(ClaudePaths {
            config_path: path.clone(),
        });
        let t = tmpl("/opt/GrokTask");
        assert_eq!(integ.status(&t).status, IntegrationStatus::InvalidConfig);
        assert!(integ.install(&t).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[test]
    fn install_idempotent_semantically() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(".claude.json");
        let integ = ClaudeIntegration::new(ClaudePaths {
            config_path: path.clone(),
        });
        let t = tmpl("/opt/GrokTask");
        integ.install(&t).unwrap();
        let a: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        integ.install(&t).unwrap();
        let b: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn remove_absent_is_noop() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(".claude.json");
        fs::write(&path, r#"{ "theme": "light" }"#).unwrap();
        let integ = ClaudeIntegration::new(ClaudePaths {
            config_path: path.clone(),
        });
        integ.remove().unwrap();
        let v: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["theme"], "light");
        assert!(v.get("mcpServers").is_none());
    }

    #[test]
    fn remove_drops_empty_mcp_servers() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(".claude.json");
        let integ = ClaudeIntegration::new(ClaudePaths {
            config_path: path.clone(),
        });
        integ.install(&tmpl("/opt/GrokTask")).unwrap();
        integ.remove().unwrap();
        let v: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(v.get("mcpServers").is_none());
    }
}
