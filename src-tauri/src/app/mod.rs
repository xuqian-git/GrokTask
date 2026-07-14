//! GUI host application role (Tauri).

pub mod gui_host;
pub mod login_item;
pub mod tray;
pub mod windows;

/// Tauri commands exposed to the frontend Settings / Task UI.
pub mod commands {
    use crate::config::{ConfigDocument, LanguagePref, ThemePref, TrayMode};
    use crate::doctor::{self, DoctorReport, GrokCliStatus};
    use crate::dto::{TaskDetail, TaskListItem};
    use crate::integrations::{self, AgentId, AgentIntegrationStatus, AgentStatusReport};
    use crate::ipc::client::{self, unwrap_result};
    use crate::ipc::protocol::ClientRole;
    use serde::{Deserialize, Serialize};
    use serde_json::{json, Value};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct SettingsSnapshot {
        pub tray_mode: String,
        pub language: String,
        pub theme: String,
        pub history_limit: u32,
        pub popover_width: u32,
        pub popover_height: u32,
        pub max_concurrent_tasks: u32,
        pub version: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ActionResult {
        pub ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub message: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub status: Option<AgentIntegrationStatus>,
    }

    fn tray_mode_str(m: TrayMode) -> &'static str {
        match m {
            TrayMode::Off => "off",
            TrayMode::Active => "active",
            TrayMode::Always => "always",
        }
    }

    fn parse_tray_mode(s: &str) -> Option<TrayMode> {
        match s {
            "off" => Some(TrayMode::Off),
            "active" => Some(TrayMode::Active),
            "always" => Some(TrayMode::Always),
            _ => None,
        }
    }

    fn language_str(l: LanguagePref) -> &'static str {
        match l {
            LanguagePref::System => "system",
            LanguagePref::ZhCn => "zh-CN",
            LanguagePref::En => "en",
        }
    }

    fn theme_str(t: ThemePref) -> &'static str {
        match t {
            ThemePref::System => "system",
            ThemePref::Light => "light",
            ThemePref::Dark => "dark",
        }
    }

    #[tauri::command]
    pub fn settings_get() -> Result<SettingsSnapshot, String> {
        let doc = ConfigDocument::load().map_err(|e| e.to_string())?;
        let c = &doc.config;
        Ok(SettingsSnapshot {
            tray_mode: tray_mode_str(c.general.tray_mode).into(),
            language: language_str(c.general.language).into(),
            theme: theme_str(c.general.theme).into(),
            history_limit: c.general.history_limit,
            popover_width: c.ui.popover_width,
            popover_height: c.ui.popover_height,
            max_concurrent_tasks: c.general.max_concurrent_tasks,
            version: crate::version::APP_VERSION.into(),
        })
    }

    #[tauri::command]
    pub fn settings_set_tray_mode(
        app: tauri::AppHandle,
        mode: String,
    ) -> Result<SettingsSnapshot, String> {
        let tray = parse_tray_mode(&mode).ok_or_else(|| format!("invalid tray mode `{mode}`"))?;
        let mut doc = ConfigDocument::load().map_err(|e| e.to_string())?;
        doc.config.general.tray_mode = tray;
        doc.save().map_err(|e| e.to_string())?;
        // Best-effort login item sync (tests use GROKTASK_LOGIN_ITEM_ROOT).
        let _ = crate::app::login_item::sync_login_item_for_mode(tray);
        // Reflect tray presence immediately (create/remove) without restart.
        crate::app::gui_host::apply_tray_mode_runtime(&app, tray);
        settings_get()
    }

    #[tauri::command]
    pub fn agents_status(agent: Option<String>) -> Result<AgentStatusReport, String> {
        let filter = match agent.as_deref() {
            None | Some("") => None,
            Some(s) => Some(AgentId::parse(s).ok_or_else(|| format!("unknown agent `{s}`"))?),
        };
        let command = integrations::current_exe_path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "GrokTask".into());
        let roots = integrations::IntegrationRoots::user_default();
        Ok(integrations::status_report(&roots, filter, &command))
    }

    #[tauri::command]
    pub fn agents_install(agent: String) -> Result<ActionResult, String> {
        let id = AgentId::parse(&agent).ok_or_else(|| format!("unknown agent `{agent}`"))?;
        let command = integrations::current_exe_path()
            .map(|p| p.display().to_string())
            .map_err(|e| e.to_string())?;
        let roots = integrations::IntegrationRoots::user_default();
        match integrations::install(&roots, id, &command) {
            Ok(status) => Ok(ActionResult {
                ok: true,
                message: Some(format!(
                    "Installed/updated {} MCP entry. Restart or reload MCP in the agent to apply.",
                    id.as_str()
                )),
                status: Some(status),
            }),
            Err(e) => Ok(ActionResult {
                ok: false,
                message: Some(e.to_string()),
                status: None,
            }),
        }
    }

    #[tauri::command]
    pub fn agents_remove(agent: String) -> Result<ActionResult, String> {
        let id = AgentId::parse(&agent).ok_or_else(|| format!("unknown agent `{agent}`"))?;
        let command = integrations::current_exe_path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "GrokTask".into());
        let roots = integrations::IntegrationRoots::user_default();
        match integrations::remove(&roots, id, &command) {
            Ok(status) => Ok(ActionResult {
                ok: true,
                message: Some(format!(
                    "Removed {} MCP entry (no-op if absent). Reload MCP in the agent to apply.",
                    id.as_str()
                )),
                status: Some(status),
            }),
            Err(e) => Ok(ActionResult {
                ok: false,
                message: Some(e.to_string()),
                status: None,
            }),
        }
    }

    #[tauri::command]
    pub fn doctor_report() -> Result<DoctorReport, String> {
        let cfg = ConfigDocument::load().ok().map(|d| d.config);
        Ok(doctor::run_doctor(cfg.as_ref()))
    }

    #[tauri::command]
    pub fn grok_cli_status() -> Result<GrokCliStatus, String> {
        let cfg = ConfigDocument::load().ok().map(|d| d.config);
        Ok(doctor::detect_grok_cli(cfg.as_ref()))
    }

    #[tauri::command]
    pub fn daemon_status_text() -> Result<String, String> {
        crate::daemon::status_text().map_err(|e| format!("{e:#}"))
    }

    #[tauri::command]
    pub fn daemon_restart(force: bool) -> Result<String, String> {
        crate::daemon::restart(force).map_err(|e| format!("{e:#}"))?;
        Ok("daemon restart requested".into())
    }

    /// List recent tasks from the local daemon (starts daemon if needed).
    #[tauri::command]
    pub fn tasks_list(limit: Option<i64>) -> Result<Vec<TaskListItem>, String> {
        let limit = limit.unwrap_or(50);
        let resp =
            client::request_blocking(ClientRole::GuiHost, "tasks.list", json!({ "limit": limit }))
                .map_err(|e| format!("{e:#}"))?;
        let v = unwrap_result(resp).map_err(|e| format!("{e:#}"))?;
        decode_tasks_list(v)
    }

    /// Full task detail + timeline snapshot from the local daemon.
    #[tauri::command]
    pub fn tasks_show(task_id: String) -> Result<TaskDetail, String> {
        let resp = client::request_blocking(
            ClientRole::GuiHost,
            "tasks.show",
            json!({ "taskId": task_id }),
        )
        .map_err(|e| format!("{e:#}"))?;
        let v = unwrap_result(resp).map_err(|e| format!("{e:#}"))?;
        decode_task_detail(v)
    }

    /// Pure decode helpers (unit-tested without a live daemon).
    pub(crate) fn decode_tasks_list(v: Value) -> Result<Vec<TaskListItem>, String> {
        serde_json::from_value(v).map_err(|e| format!("tasks.list decode: {e}"))
    }

    pub(crate) fn decode_task_detail(v: Value) -> Result<TaskDetail, String> {
        serde_json::from_value(v).map_err(|e| format!("tasks.show decode: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::commands::{decode_task_detail, decode_tasks_list};
    use serde_json::json;

    #[test]
    fn decode_tasks_list_camel_case_fixture() {
        let v = json!([
            {
                "taskId": "2e79aa9c-09e7-409b-9048-f24890a763f9",
                "title": "Reply with exactly: hello",
                "cwd": "/tmp/demo",
                "mode": "read",
                "status": "idle",
                "actualModel": "grok-4",
                "latestAction": "Replying: hello",
                "createdAt": "2026-07-15T00:00:00.000Z",
                "updatedAt": "2026-07-15T00:01:00.000Z",
                "finishedAt": "2026-07-15T00:01:00.000Z"
            }
        ]);
        let list = decode_tasks_list(v).expect("list decodes");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].task_id, "2e79aa9c-09e7-409b-9048-f24890a763f9");
        assert_eq!(list[0].title, "Reply with exactly: hello");
        assert_eq!(list[0].status.as_str(), "idle");
        assert_eq!(list[0].mode.as_str(), "read");
    }

    #[test]
    fn decode_task_detail_camel_case_fixture() {
        let v = json!({
            "task": {
                "taskId": "2e79aa9c-09e7-409b-9048-f24890a763f9",
                "status": "idle",
                "mode": "read",
                "actualModel": "grok-4",
                "latestAction": "Replying: hello",
                "answerPreview": "hello",
                "createdAt": "2026-07-15T00:00:00.000Z",
                "updatedAt": "2026-07-15T00:01:00.000Z",
                "finishedAt": "2026-07-15T00:01:00.000Z"
            },
            "title": "Reply with exactly: hello",
            "cwd": "/tmp/demo",
            "timeline": [
                {
                    "itemId": "seg:t1:0:user",
                    "kind": "user_message",
                    "message": "Reply with exactly: hello",
                    "text": "Reply with exactly: hello",
                    "streaming": false,
                    "locations": [],
                    "firstSequence": 1,
                    "lastSequence": 1
                },
                {
                    "itemId": "seg:t1:1:agent",
                    "kind": "agent_message_chunk",
                    "message": "hello",
                    "text": "hello",
                    "streaming": false,
                    "locations": [],
                    "firstSequence": 2,
                    "lastSequence": 2
                }
            ],
            "lastSequence": 2,
            "timelineGeneration": 1
        });
        let d = decode_task_detail(v).expect("detail decodes");
        assert_eq!(d.task.task_id, "2e79aa9c-09e7-409b-9048-f24890a763f9");
        assert_eq!(d.title, "Reply with exactly: hello");
        assert_eq!(d.timeline.len(), 2);
        assert_eq!(d.timeline[1].text, "hello");
        assert_eq!(d.last_sequence, 2);
    }

    #[test]
    fn decode_tasks_list_rejects_invalid_shape() {
        let err = decode_tasks_list(json!({ "not": "an array" })).unwrap_err();
        assert!(err.contains("tasks.list decode"), "{err}");
    }
}
