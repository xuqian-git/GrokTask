//! GUI host application role (Tauri).

pub mod gui_host;
pub mod login_item;
pub mod tray;
pub mod windows;

/// Tauri commands exposed to the frontend Settings / Task UI.
pub mod commands {
    use crate::config::{ConfigDocument, LanguagePref, ThemePref, TrayMode};
    use crate::doctor::{self, DoctorReport, GrokCliStatus};
    use crate::dto::{StartResult, TaskDetail, TaskListItem};
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

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct HistoryClearResult {
        pub deleted: usize,
        pub skipped: usize,
        pub protected: i64,
        pub settings: SettingsSnapshot,
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

    fn parse_language(s: &str) -> Option<LanguagePref> {
        match s {
            "zh-CN" => Some(LanguagePref::ZhCn),
            "en" => Some(LanguagePref::En),
            _ => None,
        }
    }

    fn parse_theme(s: &str) -> Option<ThemePref> {
        match s {
            "dark" => Some(ThemePref::Dark),
            "light" => Some(ThemePref::Light),
            "system" => Some(ThemePref::System),
            _ => None,
        }
    }

    fn language_str(l: LanguagePref) -> &'static str {
        match l {
            // Legacy config value: UI no longer exposes "system" for language.
            LanguagePref::System => "zh-CN",
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
    pub fn settings_set_history_limit(limit: u32) -> Result<SettingsSnapshot, String> {
        if limit > 5000 {
            return Err("historyLimit must be 0–5000".into());
        }
        let mut doc = ConfigDocument::load().map_err(|e| e.to_string())?;
        doc.config.general.history_limit = limit;
        doc.save().map_err(|e| e.to_string())?;
        // Apply the new limit immediately. Failures should surface because the
        // user explicitly asked to edit retention.
        let conn = crate::storage::open_path(&crate::paths::history_db())
            .map_err(|e| format!("open history db: {e}"))?;
        crate::storage::retention::run_retention(&conn, limit, now_ms())
            .map_err(|e| e.to_string())?;
        settings_get()
    }

    #[tauri::command]
    pub fn settings_set_language(language: String) -> Result<SettingsSnapshot, String> {
        let language = parse_language(&language)
            .ok_or_else(|| format!("invalid language `{language}`; expected zh-CN|en"))?;
        let mut doc = ConfigDocument::load().map_err(|e| e.to_string())?;
        doc.config.general.language = language;
        doc.save().map_err(|e| e.to_string())?;
        settings_get()
    }

    #[tauri::command]
    pub fn settings_set_theme(theme: String) -> Result<SettingsSnapshot, String> {
        let theme = parse_theme(&theme)
            .ok_or_else(|| format!("invalid theme `{theme}`; expected dark|light|system"))?;
        let mut doc = ConfigDocument::load().map_err(|e| e.to_string())?;
        doc.config.general.theme = theme;
        doc.save().map_err(|e| e.to_string())?;
        settings_get()
    }

    #[tauri::command]
    pub fn agents_status(
        agent: Option<String>,
        #[allow(unused_variables)] cwd: Option<String>,
    ) -> Result<AgentStatusReport, String> {
        let filter = match agent.as_deref() {
            None | Some("") => None,
            Some(s) => Some(AgentId::parse(s).ok_or_else(|| format!("unknown agent `{s}`"))?),
        };
        let command = integrations::current_exe_path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "GrokTask".into());
        // Workflow + MCP both use user home roots; cwd is accepted for IPC
        // compatibility but not required for status resolution.
        let roots = integrations::IntegrationRoots::user_default();
        Ok(integrations::status_report(&roots, filter, &command))
    }

    #[tauri::command]
    pub fn agents_install(
        agent: String,
        #[allow(unused_variables)] cwd: Option<String>,
    ) -> Result<ActionResult, String> {
        let id = AgentId::parse(&agent).ok_or_else(|| format!("unknown agent `{agent}`"))?;
        let command = integrations::current_exe_path()
            .map(|p| p.display().to_string())
            .map_err(|e| e.to_string())?;
        let roots = integrations::IntegrationRoots::user_default();
        match integrations::install(&roots, id, &command) {
            Ok(status) => Ok(ActionResult {
                ok: true,
                message: Some(format!(
                    "已安装/更新 {} 的 MCP 条目。请在 Agent 中重启或重新加载 MCP。",
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
    pub fn agents_remove(
        agent: String,
        #[allow(unused_variables)] cwd: Option<String>,
    ) -> Result<ActionResult, String> {
        let id = AgentId::parse(&agent).ok_or_else(|| format!("unknown agent `{agent}`"))?;
        let command = integrations::current_exe_path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "GrokTask".into());
        let roots = integrations::IntegrationRoots::user_default();
        match integrations::remove(&roots, id, &command) {
            Ok(status) => Ok(ActionResult {
                ok: true,
                message: Some(format!(
                    "已移除 {} 的 MCP 条目（不存在时为 no-op）。请在 Agent 中重新加载 MCP。",
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

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct WorkflowActionResult {
        pub ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub message: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub status: Option<AgentIntegrationStatus>,
    }

    #[tauri::command]
    pub fn agents_workflow_enable(
        agent: String,
        #[allow(unused_variables)] cwd: Option<String>,
    ) -> Result<WorkflowActionResult, String> {
        let id = AgentId::parse(&agent).ok_or_else(|| format!("unknown agent `{agent}`"))?;
        // Global user instruction files — do not require workspace_cwd.
        let roots = integrations::IntegrationRoots::user_default();
        let command = integrations::current_exe_path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "GrokTask".into());
        match integrations::workflow_enable(&roots, id) {
            Ok(_) => {
                let report = integrations::status_report(&roots, Some(id), &command);
                Ok(WorkflowActionResult {
                    ok: true,
                    message: Some(format!(
                        "已写入 {} 全局自动触发指令到 {}。Agent 下次会话将读取该文件。",
                        id.as_str(),
                        report
                            .agents
                            .first()
                            .map(|a| a.workflow_path.as_str())
                            .unwrap_or("?")
                    )),
                    status: report.agents.into_iter().next(),
                })
            }
            Err(e) => Ok(WorkflowActionResult {
                ok: false,
                message: Some(e.to_string()),
                status: None,
            }),
        }
    }

    #[tauri::command]
    pub fn agents_workflow_disable(
        agent: String,
        #[allow(unused_variables)] cwd: Option<String>,
    ) -> Result<WorkflowActionResult, String> {
        let id = AgentId::parse(&agent).ok_or_else(|| format!("unknown agent `{agent}`"))?;
        // Global user instruction files — do not require workspace_cwd.
        let roots = integrations::IntegrationRoots::user_default();
        let command = integrations::current_exe_path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "GrokTask".into());
        match integrations::workflow_disable(&roots, id) {
            Ok(_) => {
                let report = integrations::status_report(&roots, Some(id), &command);
                Ok(WorkflowActionResult {
                    ok: true,
                    message: Some(format!(
                        "已从全局指令文件移除 {} 的 GrokTask 托管区块。",
                        id.as_str()
                    )),
                    status: report.agents.into_iter().next(),
                })
            }
            Err(e) => Ok(WorkflowActionResult {
                ok: false,
                message: Some(e.to_string()),
                status: None,
            }),
        }
    }

    /// Trusted project workspace path for Settings task/MCP context display.
    ///
    /// Returns the path last provided by `GrokTask setup` (via `gui.open_settings`
    /// with `cwd`). Does **not** fall back to the GUI process working directory,
    /// which is unsafe/wrong when the host was started from Finder or the menu bar.
    /// Workflow instruction writes use global user files and do not need this path.
    #[tauri::command]
    pub fn workspace_cwd(app: tauri::AppHandle) -> Result<String, String> {
        let selected = crate::app::gui_host::selected_workspace_cwd(&app);
        crate::app::gui_host::resolve_trusted_workspace_cwd(selected.as_deref())
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

    /// Continue an existing task with a new user prompt.
    #[tauri::command]
    pub fn tasks_send(task_id: String, prompt: String) -> Result<StartResult, String> {
        let resp = client::request_blocking(
            ClientRole::GuiHost,
            "task.continue",
            json!({ "taskId": task_id, "prompt": prompt }),
        )
        .map_err(|e| format!("{e:#}"))?;
        let v = unwrap_result(resp).map_err(|e| format!("{e:#}"))?;
        serde_json::from_value(v).map_err(|e| format!("task.continue decode: {e}"))
    }

    /// Clear eligible task history. Active / protected tasks are kept.
    #[tauri::command]
    pub fn history_clear() -> Result<HistoryClearResult, String> {
        let conn = crate::storage::open_path(&crate::paths::history_db())
            .map_err(|e| format!("open history db: {e}"))?;
        let now = now_ms();
        let protected =
            crate::storage::retention::count_protected(&conn, now).map_err(|e| e.to_string())?;
        let result =
            crate::storage::retention::run_retention(&conn, 0, now).map_err(|e| e.to_string())?;
        Ok(HistoryClearResult {
            deleted: result.deleted_ids.len(),
            skipped: result.skipped,
            protected,
            settings: settings_get()?,
        })
    }

    /// Pure decode helpers (unit-tested without a live daemon).
    pub(crate) fn decode_tasks_list(v: Value) -> Result<Vec<TaskListItem>, String> {
        serde_json::from_value(v).map_err(|e| format!("tasks.list decode: {e}"))
    }

    pub(crate) fn decode_task_detail(v: Value) -> Result<TaskDetail, String> {
        serde_json::from_value(v).map_err(|e| format!("tasks.show decode: {e}"))
    }

    fn now_ms() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
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
