//! GUI host application role (Tauri).

pub mod gui_host;
pub mod login_item;
pub mod tray;
pub mod windows;

/// Tauri commands exposed to the frontend Settings UI.
pub mod commands {
    use crate::config::{ConfigDocument, LanguagePref, ThemePref, TrayMode};
    use crate::doctor::{self, DoctorReport, GrokCliStatus};
    use crate::integrations::{self, AgentId, AgentIntegrationStatus, AgentStatusReport};
    use serde::{Deserialize, Serialize};

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
}
