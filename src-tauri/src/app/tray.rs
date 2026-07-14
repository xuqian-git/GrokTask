//! Tray icon menu model and builders.
//!
//! The declarative menu model is pure and unit-tested. Live Tauri menu wiring
//! lives in `gui_host` so headless tests never need a display.

use serde::{Deserialize, Serialize};

/// Inputs used to build the right-click tray menu.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrayMenuInput {
    /// Number of running/queued tasks.
    pub running_count: usize,
    /// Short summary of the current task, if any.
    pub current_summary: Option<String>,
    /// Whether a current task can be opened.
    pub has_current_task: bool,
    /// Human-readable daemon status line (e.g. "running" / "stopped").
    pub daemon_status: String,
    /// Whether restart daemon is offered.
    pub can_restart_daemon: bool,
}

/// A single menu entry in the tray context menu.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TrayMenuEntry {
    /// Non-clickable (or soft) status line.
    Status {
        id: String,
        text: String,
        enabled: bool,
    },
    Action {
        id: String,
        text: String,
        enabled: bool,
    },
    Separator {
        id: String,
    },
}

/// Stable action ids (also used as Tauri menu item ids).
pub mod menu_id {
    pub const SUMMARY: &str = "summary";
    pub const CURRENT: &str = "current";
    pub const OPEN_CURRENT: &str = "open_current";
    pub const OPEN_APP: &str = "open_app";
    pub const OPEN_POPOVER: &str = "open_popover";
    pub const HISTORY: &str = "history";
    pub const SETTINGS: &str = "settings";
    pub const DAEMON_STATUS: &str = "daemon_status";
    pub const RESTART_DAEMON: &str = "restart_daemon";
    pub const QUIT: &str = "quit";
    pub const SEP1: &str = "sep1";
    pub const SEP2: &str = "sep2";
    pub const SEP3: &str = "sep3";
}

/// Build the ordered tray menu model from live summary inputs.
pub fn build_tray_menu(input: &TrayMenuInput) -> Vec<TrayMenuEntry> {
    let summary = if input.running_count == 0 {
        "GrokTask · 空闲".to_string()
    } else if input.running_count == 1 {
        "GrokTask · 1 个任务运行中".to_string()
    } else {
        format!("GrokTask · {} 个任务运行中", input.running_count)
    };

    let current = input
        .current_summary
        .clone()
        .unwrap_or_else(|| "当前：（无）".into());
    let current_text = if current.starts_with("当前：") || current.starts_with("Current:") {
        current
    } else {
        format!("当前：{current}")
    };

    let mut items = vec![
        TrayMenuEntry::Status {
            id: menu_id::SUMMARY.into(),
            text: summary,
            enabled: false,
        },
        TrayMenuEntry::Status {
            id: menu_id::CURRENT.into(),
            text: current_text,
            enabled: false,
        },
        TrayMenuEntry::Separator {
            id: menu_id::SEP1.into(),
        },
        TrayMenuEntry::Action {
            id: menu_id::OPEN_CURRENT.into(),
            text: "打开当前任务".into(),
            enabled: input.has_current_task,
        },
        TrayMenuEntry::Action {
            id: menu_id::OPEN_APP.into(),
            text: "打开完整窗口".into(),
            enabled: true,
        },
        TrayMenuEntry::Action {
            id: menu_id::OPEN_POPOVER.into(),
            text: "打开实时面板".into(),
            enabled: true,
        },
        TrayMenuEntry::Action {
            id: menu_id::HISTORY.into(),
            text: "ACP 记录".into(),
            enabled: true,
        },
        TrayMenuEntry::Action {
            id: menu_id::SETTINGS.into(),
            text: "设置".into(),
            enabled: true,
        },
        TrayMenuEntry::Separator {
            id: menu_id::SEP2.into(),
        },
        TrayMenuEntry::Status {
            id: menu_id::DAEMON_STATUS.into(),
            text: format!("Daemon：{}", input.daemon_status),
            enabled: false,
        },
    ];

    if input.can_restart_daemon {
        items.push(TrayMenuEntry::Action {
            id: menu_id::RESTART_DAEMON.into(),
            text: "重启 Daemon".into(),
            enabled: true,
        });
    }

    items.push(TrayMenuEntry::Separator {
        id: menu_id::SEP3.into(),
    });
    items.push(TrayMenuEntry::Action {
        id: menu_id::QUIT.into(),
        text: "退出 GrokTask".into(),
        enabled: true,
    });

    items
}

/// Whether tray should be shown for this mode (ignoring platform capability).
pub fn tray_visible_for_mode(mode: crate::config::TrayMode) -> bool {
    !matches!(mode, crate::config::TrayMode::Off)
}

/// Desired tray presence after applying a tray mode (pure; no I/O).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayPresence {
    /// Create the tray if missing; ensure visible.
    Present,
    /// Remove or hide the tray if present.
    Absent,
}

/// Map tray mode → presence action. Used by GUI host reconcile without Tauri.
pub fn tray_presence_for_mode(mode: crate::config::TrayMode) -> TrayPresence {
    if tray_visible_for_mode(mode) {
        TrayPresence::Present
    } else {
        TrayPresence::Absent
    }
}

/// Desired macOS `NSApplicationActivationPolicy` for a tray presence change.
///
/// - Present → Accessory (menu-bar agent; no Dock icon while tray is visible)
/// - Absent → Regular (restore Dock / normal app activation)
///
/// Startup with Off must leave the default policy alone (do not force Accessory).
/// Runtime Off must restore Regular after a prior Accessory switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacosActivationPolicy {
    Accessory,
    Regular,
}

/// Pure policy decision for macOS tray presence (no Tauri / GUI required).
pub fn macos_activation_policy_for_tray_presence(presence: TrayPresence) -> MacosActivationPolicy {
    match presence {
        TrayPresence::Present => MacosActivationPolicy::Accessory,
        TrayPresence::Absent => MacosActivationPolicy::Regular,
    }
}

/// Stable tray icon id used with Tauri's tray manager.
pub const TRAY_ICON_ID: &str = "groktask-tray";

/// Merge current-task navigation state into a tray menu input.
///
/// When `task_id` is set, "Open current task" is enabled. If no summary text is
/// available yet, a short generic label is used so the status line is not blank.
pub fn with_current_task(mut input: TrayMenuInput, task_id: Option<&str>) -> TrayMenuInput {
    match task_id {
        Some(id) if !id.is_empty() => {
            input.has_current_task = true;
            if input.current_summary.is_none() {
                input.current_summary = Some(id.to_string());
            }
        }
        _ => {
            input.has_current_task = false;
        }
    }
    input
}

/// Whitelist Settings section ids accepted from IPC / CLI navigation.
pub fn sanitize_settings_section(section: Option<&str>) -> Option<&'static str> {
    match section {
        Some("general") => Some("general"),
        Some("integrations") => Some("integrations"),
        Some("diagnostics") => Some("diagnostics"),
        Some("history") => Some("history"),
        _ => None,
    }
}

/// Build a safe `eval` script that dispatches a settings-section CustomEvent.
/// Returns `None` when the section is not whitelisted.
pub fn settings_section_dispatch_js(section: &str) -> Option<String> {
    let sec = sanitize_settings_section(Some(section))?;
    // Serialize so the value cannot break out of the JS string literal.
    let detail = serde_json::to_string(sec).ok()?;
    Some(format!(
        "window.dispatchEvent(new CustomEvent('groktask-settings-section', {{ detail: {detail} }}));"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TrayMode;

    #[test]
    fn menu_includes_required_entries() {
        let menu = build_tray_menu(&TrayMenuInput {
            running_count: 2,
            current_summary: Some("Running tests".into()),
            has_current_task: true,
            daemon_status: "running".into(),
            can_restart_daemon: true,
        });
        let ids: Vec<&str> = menu
            .iter()
            .map(|e| match e {
                TrayMenuEntry::Status { id, .. }
                | TrayMenuEntry::Action { id, .. }
                | TrayMenuEntry::Separator { id } => id.as_str(),
            })
            .collect();
        assert!(ids.contains(&menu_id::SUMMARY));
        assert!(ids.contains(&menu_id::OPEN_CURRENT));
        assert!(ids.contains(&menu_id::OPEN_APP));
        assert!(ids.contains(&menu_id::HISTORY));
        assert!(ids.contains(&menu_id::SETTINGS));
        assert!(ids.contains(&menu_id::DAEMON_STATUS));
        assert!(ids.contains(&menu_id::RESTART_DAEMON));
        assert!(ids.contains(&menu_id::QUIT));

        let summary = menu.iter().find_map(|e| match e {
            TrayMenuEntry::Status { id, text, .. } if id == menu_id::SUMMARY => Some(text.clone()),
            _ => None,
        });
        assert_eq!(summary.as_deref(), Some("GrokTask · 2 个任务运行中"));
    }

    #[test]
    fn idle_menu_disables_open_current() {
        let menu = build_tray_menu(&TrayMenuInput {
            running_count: 0,
            current_summary: None,
            has_current_task: false,
            daemon_status: "stopped".into(),
            can_restart_daemon: false,
        });
        let open = menu.iter().find_map(|e| match e {
            TrayMenuEntry::Action { id, enabled, .. } if id == menu_id::OPEN_CURRENT => {
                Some(*enabled)
            }
            _ => None,
        });
        assert_eq!(open, Some(false));
        assert!(!menu.iter().any(|e| matches!(
            e,
            TrayMenuEntry::Action { id, .. } if id == menu_id::RESTART_DAEMON
        )));
    }

    #[test]
    fn tray_mode_visibility() {
        assert!(!tray_visible_for_mode(TrayMode::Off));
        assert!(tray_visible_for_mode(TrayMode::Active));
        assert!(tray_visible_for_mode(TrayMode::Always));
    }

    #[test]
    fn tray_presence_lifecycle_decisions() {
        assert_eq!(tray_presence_for_mode(TrayMode::Off), TrayPresence::Absent);
        assert_eq!(
            tray_presence_for_mode(TrayMode::Active),
            TrayPresence::Present
        );
        assert_eq!(
            tray_presence_for_mode(TrayMode::Always),
            TrayPresence::Present
        );
    }

    #[test]
    fn macos_activation_policy_tracks_tray_presence() {
        assert_eq!(
            macos_activation_policy_for_tray_presence(TrayPresence::Present),
            MacosActivationPolicy::Accessory
        );
        assert_eq!(
            macos_activation_policy_for_tray_presence(TrayPresence::Absent),
            MacosActivationPolicy::Regular
        );
        // Runtime Off (via mode → presence) restores Regular; Present modes use Accessory.
        assert_eq!(
            macos_activation_policy_for_tray_presence(tray_presence_for_mode(TrayMode::Off)),
            MacosActivationPolicy::Regular
        );
        assert_eq!(
            macos_activation_policy_for_tray_presence(tray_presence_for_mode(TrayMode::Active)),
            MacosActivationPolicy::Accessory
        );
        assert_eq!(
            macos_activation_policy_for_tray_presence(tray_presence_for_mode(TrayMode::Always)),
            MacosActivationPolicy::Accessory
        );
    }

    #[test]
    fn current_task_enables_open_current_even_without_summary() {
        let base = TrayMenuInput {
            running_count: 0,
            current_summary: None,
            has_current_task: false,
            daemon_status: "running".into(),
            can_restart_daemon: true,
        };
        let input = with_current_task(base, Some("task-abc"));
        assert!(input.has_current_task);
        assert_eq!(input.current_summary.as_deref(), Some("task-abc"));
        let menu = build_tray_menu(&input);
        let open = menu.iter().find_map(|e| match e {
            TrayMenuEntry::Action { id, enabled, .. } if id == menu_id::OPEN_CURRENT => {
                Some(*enabled)
            }
            _ => None,
        });
        assert_eq!(open, Some(true));
    }

    #[test]
    fn settings_section_whitelist_and_safe_js() {
        assert_eq!(
            sanitize_settings_section(Some("integrations")),
            Some("integrations")
        );
        assert_eq!(sanitize_settings_section(Some("general")), Some("general"));
        assert_eq!(sanitize_settings_section(Some("not-a-section")), None);
        // Injection-like payloads must not pass the whitelist.
        let inject = concat!("integrations", "'", ";//");
        assert_eq!(sanitize_settings_section(Some(inject)), None);
        assert_eq!(sanitize_settings_section(None), None);

        let js = settings_section_dispatch_js("integrations").unwrap();
        assert!(js.contains("groktask-settings-section"));
        assert!(js.contains("\"integrations\""));
        assert!(!js.contains("not-a-section"));
        let inject_js = concat!("'", "));alert(1)//");
        assert!(settings_section_dispatch_js(inject_js).is_none());
    }
}
