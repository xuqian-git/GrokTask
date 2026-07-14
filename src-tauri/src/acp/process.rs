//! Safe spawn argument builders for Grok ACP (full argv policy in Phase 2).
//! Phase 0–1 only exposes typed mode + discovery placeholders.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskMode {
    Read,
    Write,
}

/// Build base Grok argv for a mode (without model/effort). Phase 2 locks exact values with tests.
pub fn mode_args(mode: TaskMode) -> Vec<String> {
    match mode {
        TaskMode::Read => vec![
            "--sandbox".into(),
            "read-only".into(),
            "--permission-mode".into(),
            "dontAsk".into(),
            "--disable-web-search".into(),
            "--no-subagents".into(),
            "--allow".into(),
            "Read".into(),
            "--allow".into(),
            "Grep".into(),
            "--deny".into(),
            "Edit".into(),
            "--deny".into(),
            "WebFetch".into(),
        ],
        TaskMode::Write => vec![
            "--sandbox".into(),
            "workspace".into(),
            "--always-approve".into(),
            "--deny".into(),
            "Bash(git push*)".into(),
            "--deny".into(),
            "Bash(git commit*)".into(),
            "--deny".into(),
            "Bash(git clean*)".into(),
            "--deny".into(),
            "Bash(git reset --hard*)".into(),
            "--deny".into(),
            "Bash(gh pr*)".into(),
            "--deny".into(),
            "Bash(rm -rf*)".into(),
        ],
    }
}

/// Full argv: `grok --no-auto-update <mode args> agent stdio`
pub fn build_grok_argv(mode: TaskMode, model: Option<&str>, effort: Option<&str>) -> Vec<String> {
    let mut argv = vec!["--no-auto-update".to_string()];
    argv.extend(mode_args(mode));
    if let Some(m) = model {
        argv.push("--model".into());
        argv.push(m.into());
    }
    if let Some(e) = effort {
        argv.push("--reasoning-effort".into());
        argv.push(e.into());
    }
    argv.push("agent".into());
    argv.push("stdio".into());
    argv
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_has_no_broad_git_bash() {
        let args = mode_args(TaskMode::Read);
        let joined = args.join(" ");
        assert!(!joined.contains("Bash(git *)"));
        assert!(args.contains(&"read-only".to_string()));
    }

    #[test]
    fn write_uses_workspace_sandbox() {
        let args = mode_args(TaskMode::Write);
        assert!(args
            .windows(2)
            .any(|w| w[0] == "--sandbox" && w[1] == "workspace"));
    }
}
