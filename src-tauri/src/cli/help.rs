//! Help / version text. Pure stdout — no daemon, no Tauri.

use crate::version::{APP_VERSION, PRODUCT_NAME};

pub fn program_name() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|s| s.to_string_lossy().into_owned()))
        .unwrap_or_else(|| PRODUCT_NAME.to_string())
}

pub fn version_text() -> String {
    format!("{PRODUCT_NAME} {APP_VERSION}")
}

pub fn help_text() -> String {
    let prog = program_name();
    format!(
        r#"{PRODUCT_NAME} {APP_VERSION}

Usage:
  {prog} --help
  {prog} --version
  {prog} doctor
  {prog} setup
  {prog} app [--task TASK_ID]

  {prog} mcp

  {prog} run --mode read|write --cwd PATH [--model ID] [--effort VALUE] TASK...
  {prog} start --mode read|write --cwd PATH [--model ID] [--effort VALUE] [--submission-id UUID] TASK...
  {prog} submit --mode read|write --cwd PATH TASK...   (alias of run)
  {prog} status TASK_ID [--json]
  {prog} wait TASK_ID TURN_ID [--timeout SECONDS] [--json]
  {prog} cancel TASK_ID (--turn TURN_ID | --recovery RECOVERY_ID) [--json]

  {prog} tasks list [--limit N] [--json]
  {prog} tasks show TASK_ID [--json]
  {prog} list | show TASK_ID                           (aliases)
  {prog} tasks clear [--inactive-only]

  {prog} agents status [codex|claude] [--cwd PATH]
  {prog} agents mode codex|claude none|mcp
  {prog} agents workflow status [codex|claude] [--cwd PATH]
  {prog} agents workflow enable codex|claude [--cwd PATH]
  {prog} agents workflow disable codex|claude [--cwd PATH]

  {prog} daemon run
  {prog} daemon start|stop|restart [--force]|status|logs

Notes:
  - --help / --version never start the daemon or GUI.
  - mcp and daemon roles do not initialize Tauri/WebView.
  - Mode is always explicit (read|write); there is no default.
"#
    )
}
