//! Hidden `--gui-host` role: single-instance Tauri event loop, tray, windows, nav IPC.

use crate::app::commands;
use crate::app::login_item;
use crate::app::tray::{self, menu_id, TrayMenuEntry, TrayMenuInput, TrayPresence, TRAY_ICON_ID};
use crate::app::windows::{
    clamp_to_work_area, compute_popover_position, default_anchor_fallback, detect_tray_capability,
    effective_popover_size, surface_url, tray_click_coords_reliable, PopoverPositionInput, Rect,
    TrayIconRect,
};
use crate::config::{ConfigDocument, TrayMode};
use crate::daemon::lifecycle::{acquire_lock_at, LockResult};
use crate::ipc::codec::{read_msg, write_msg};
use crate::ipc::protocol::GuiNavCommand;
use crate::ipc::transport::{self, IpcStream};
use crate::paths;
use crate::version::window_label;
use std::sync::Mutex;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{
    AppHandle, Manager, PhysicalPosition, Position, Size, WebviewUrl, WebviewWindowBuilder,
    WindowEvent,
};
use tokio::io::BufReader;
use tokio::sync::mpsc;

/// Main window visibility for the hidden internal `--gui-host` role
/// (starts hidden until a navigation command shows it).
pub fn initial_main_window_visible() -> bool {
    false
}

struct HostState {
    tray_mode: Mutex<TrayMode>,
    popover_visible: Mutex<bool>,
    current_task_id: Mutex<Option<String>>,
    /// Project cwd last trusted via `GrokTask setup` / OpenSettings with cwd.
    /// Never derived from the GUI process working directory (unsafe for Finder launches).
    selected_workspace_cwd: Mutex<Option<String>>,
}

/// Run the GUI host for the hidden `--gui-host` role.
/// Main window starts hidden; navigation IPC shows surfaces.
/// Acquires `gui-host.lock`; if already held, exits immediately.
pub fn run() -> ! {
    run_impl(GuiHostLaunch {
        show_main_on_start: false,
        focus_existing_on_lock_busy: false,
    })
}

/// macOS `.app` launch with no CLI args (double-click / `open GrokTask.app`).
///
/// Focuses an existing GUI host if one is already running; otherwise becomes
/// the host in-process and shows the main window so LaunchServices keeps the
/// app process associated with a visible UI.
pub fn run_as_app_bundle_launch() -> ! {
    // Fast path: another host already owns the lock / socket.
    if try_navigate(GuiNavCommand::Focus) {
        std::process::exit(0);
    }
    run_impl(GuiHostLaunch {
        show_main_on_start: true,
        focus_existing_on_lock_busy: true,
    })
}

struct GuiHostLaunch {
    /// When true, create the main window visible (app-bundle double-click).
    show_main_on_start: bool,
    /// When lock is already held, try Focus navigate before exiting (app re-open).
    focus_existing_on_lock_busy: bool,
}

fn run_impl(launch: GuiHostLaunch) -> ! {
    match acquire_lock_at(&paths::gui_host_lock()) {
        Ok(LockResult::Acquired(_guard)) => {
            std::mem::forget(_guard);
        }
        Ok(LockResult::AlreadyRunning) => {
            if launch.focus_existing_on_lock_busy {
                // Race: host appeared between try_navigate and lock acquisition.
                let _ = try_navigate(GuiNavCommand::Focus);
            } else {
                eprintln!("gui-host already running");
            }
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("gui-host lock error: {e}");
            std::process::exit(1);
        }
    }

    if let Err(e) = paths::ensure_config_dir() {
        eprintln!("gui-host config dir: {e}");
        std::process::exit(1);
    }

    let config = ConfigDocument::load().unwrap_or_default();
    let tray_mode = config.config.general.tray_mode;
    let popover_w = config.config.ui.popover_width;
    let popover_h = config.config.ui.popover_height;
    let show_main_on_start = launch.show_main_on_start;

    // Best-effort login item sync for always mode.
    let _ = login_item::sync_login_item_for_mode(tray_mode);

    let (nav_tx, mut nav_rx) = mpsc::unbounded_channel::<GuiNavCommand>();

    let nav_tx_ipc = nav_tx.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("gui ipc runtime");
        rt.block_on(async move {
            transport::remove_stale_gui_endpoint();
            let listener = match transport::bind_gui_host() {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("gui-host bind failed: {e}");
                    return;
                }
            };
            loop {
                match listener.accept().await {
                    Ok(stream) => {
                        let tx = nav_tx_ipc.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_gui_client(stream, tx).await {
                                eprintln!("gui-host client: {e}");
                            }
                        });
                    }
                    Err(e) => {
                        eprintln!("gui-host accept: {e}");
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    }
                }
            }
        });
    });

    let result = tauri::Builder::default()
        .manage(HostState {
            tray_mode: Mutex::new(tray_mode),
            popover_visible: Mutex::new(false),
            current_task_id: Mutex::new(None),
            selected_workspace_cwd: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            commands::settings_get,
            commands::settings_set_tray_mode,
            commands::settings_set_history_limit,
            commands::agents_status,
            commands::agents_install,
            commands::agents_remove,
            commands::agents_workflow_enable,
            commands::agents_workflow_disable,
            commands::workspace_cwd,
            commands::doctor_report,
            commands::grok_cli_status,
            commands::daemon_status_text,
            commands::daemon_restart,
            commands::tasks_list,
            commands::tasks_show,
            commands::history_clear,
        ])
        .on_window_event(|window, event| {
            // Hide popover on focus loss (click outside); do not cancel tasks.
            if window.label() == window_label::POPOVER {
                if let WindowEvent::Focused(false) = event {
                    let _ = window.hide();
                    if let Some(state) = window.app_handle().try_state::<HostState>() {
                        if let Ok(mut g) = state.popover_visible.lock() {
                            *g = false;
                        }
                    }
                }
            }
        })
        .setup(move |app| {
            // Main window: hidden for `--gui-host` until nav; visible for .app launch.
            let _main = WebviewWindowBuilder::new(
                app,
                window_label::MAIN,
                WebviewUrl::App(surface_url("task", None).into()),
            )
            .title("GrokTask")
            .inner_size(1120.0, 760.0)
            .min_inner_size(900.0, 640.0)
            .visible(show_main_on_start)
            .build()?;

            // Initial launch honors stored trayMode; later Settings changes call
            // `apply_tray_mode_runtime` to create/remove without restart.
            reconcile_tray(app.handle(), tray_mode, popover_w, popover_h);

            #[cfg(target_os = "macos")]
            {
                if tray::tray_visible_for_mode(tray_mode) {
                    app.set_activation_policy(tauri::ActivationPolicy::Accessory);
                }
            }

            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                while let Some(cmd) = nav_rx.recv().await {
                    apply_nav(&handle, cmd);
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!());

    if let Err(e) = result {
        eprintln!("gui-host error: {e}");
        std::process::exit(1);
    }
    std::process::exit(0);
}

/// Apply a runtime tray-mode change: update host state, reconcile tray visibility,
/// and keep login-item sync as a separate best-effort step (caller or Settings).
pub fn apply_tray_mode_runtime(app: &AppHandle, mode: TrayMode) {
    if let Some(state) = app.try_state::<HostState>() {
        if let Ok(mut g) = state.tray_mode.lock() {
            *g = mode;
        }
    }
    let (w, h) = popover_size_from_config();
    reconcile_tray(app, mode, w, h);

    // Runtime tray-mode changes must also restore Regular when Off so the app
    // does not stay Accessory (no Dock) after the tray icon is removed.
    // Startup Off intentionally does not call set_activation_policy (default Regular).
    #[cfg(target_os = "macos")]
    {
        use tray::{macos_activation_policy_for_tray_presence, MacosActivationPolicy};
        match macos_activation_policy_for_tray_presence(tray::tray_presence_for_mode(mode)) {
            MacosActivationPolicy::Accessory => {
                let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            }
            MacosActivationPolicy::Regular => {
                let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
            }
        }
    }
}

/// Create, show, or remove the tray icon to match `mode` without leaking handles.
/// Tray icons are retained by Tauri's tray manager (`tray_by_id` / `remove_tray_by_id`).
fn reconcile_tray(app: &AppHandle, mode: TrayMode, popover_w: u32, popover_h: u32) {
    match tray::tray_presence_for_mode(mode) {
        TrayPresence::Present => {
            if app.tray_by_id(TRAY_ICON_ID).is_some() {
                if let Some(tray) = app.tray_by_id(TRAY_ICON_ID) {
                    let _ = tray.set_visible(true);
                }
                refresh_tray_menu(app);
            } else if let Err(e) = setup_tray(app, popover_w, popover_h) {
                eprintln!("gui-host tray setup: {e}");
                // Continue without tray (Linux no tray host, etc.).
            }
        }
        TrayPresence::Absent => {
            // Dropping the returned icon removes it from the system tray.
            let _ = app.remove_tray_by_id(TRAY_ICON_ID);
        }
    }
}

fn setup_tray(app: &AppHandle, popover_w: u32, popover_h: u32) -> Result<(), String> {
    let cap = detect_tray_capability();
    if !cap.tray_available {
        return Err(cap.detail.unwrap_or_else(|| "tray unavailable".into()));
    }

    // Idempotent: do not create a second icon if reconcile races.
    if app.tray_by_id(TRAY_ICON_ID).is_some() {
        return Ok(());
    }

    let menu = build_native_menu(app, &menu_input_from_state(app))?;
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| "no default window icon for tray".to_string())?;

    let tray = TrayIconBuilder::with_id(TRAY_ICON_ID)
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("GrokTask")
        .on_menu_event(|app, event| {
            handle_menu_event(app, event.id.as_ref());
        })
        .on_tray_icon_event(move |tray, event| {
            let app = tray.app_handle();
            match event {
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    position,
                    rect,
                    ..
                } => {
                    let tray_rect = tray_rect_from_event(&rect, &position);
                    toggle_popover(app, Some(tray_rect), popover_w, popover_h);
                }
                TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                } => {
                    open_or_focus_main(app, None);
                }
                _ => {}
            }
        })
        .build(app)
        .map_err(|e| e.to_string())?;

    // Tray is retained by Tauri's resources table / tray manager; do not leak.
    drop(tray);
    Ok(())
}

fn daemon_menu_status() -> (String, bool) {
    let daemon = crate::daemon::status_text()
        .unwrap_or_else(|_| "unknown".into())
        .to_lowercase();
    let running = daemon.contains("running") || daemon.contains("pid");
    let status = if running {
        "running".into()
    } else if daemon.contains("stop") || daemon.contains("not") {
        "stopped".into()
    } else {
        daemon.chars().take(40).collect()
    };
    (status, true)
}

fn base_menu_input() -> TrayMenuInput {
    let (daemon_status, can_restart_daemon) = daemon_menu_status();
    TrayMenuInput {
        running_count: 0,
        current_summary: None,
        has_current_task: false,
        daemon_status,
        can_restart_daemon,
    }
}

/// Build tray menu inputs from live host state (current task, daemon).
fn menu_input_from_state(app: &AppHandle) -> TrayMenuInput {
    let task_id = app
        .try_state::<HostState>()
        .and_then(|s| s.current_task_id.lock().ok().and_then(|g| g.clone()));
    tray::with_current_task(base_menu_input(), task_id.as_deref())
}

/// Rebuild and attach the native tray menu so current-task items stay accurate.
fn refresh_tray_menu(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ICON_ID) else {
        return;
    };
    let input = menu_input_from_state(app);
    match build_native_menu(app, &input) {
        Ok(menu) => {
            if let Err(e) = tray.set_menu(Some(menu)) {
                eprintln!("gui-host tray menu update: {e}");
            }
        }
        Err(e) => eprintln!("gui-host tray menu build: {e}"),
    }
}

fn build_native_menu(app: &AppHandle, input: &TrayMenuInput) -> Result<Menu<tauri::Wry>, String> {
    let model = tray::build_tray_menu(input);
    let menu = Menu::new(app).map_err(|e| e.to_string())?;
    for entry in &model {
        match entry {
            TrayMenuEntry::Separator { .. } => {
                let sep = PredefinedMenuItem::separator(app).map_err(|e| e.to_string())?;
                menu.append(&sep).map_err(|e| e.to_string())?;
                // Menu retains the item; forget local handle so Drop does not remove it.
                std::mem::forget(sep);
            }
            TrayMenuEntry::Status { id, text, enabled }
            | TrayMenuEntry::Action { id, text, enabled } => {
                let item = MenuItem::with_id(app, id, text, *enabled, None::<&str>)
                    .map_err(|e| e.to_string())?;
                menu.append(&item).map_err(|e| e.to_string())?;
                std::mem::forget(item);
            }
        }
    }
    Ok(menu)
}

/// Extract physical tray icon bounds from a Tauri tray event.
fn tray_rect_from_event(rect: &tauri::Rect, cursor: &PhysicalPosition<f64>) -> TrayIconRect {
    let (x, y) = match rect.position {
        Position::Physical(p) => (p.x as f64, p.y as f64),
        Position::Logical(p) => (p.x, p.y),
    };
    let (width, height) = match rect.size {
        Size::Physical(s) => (s.width as f64, s.height as f64),
        Size::Logical(s) => (s.width, s.height),
    };
    if width < 1.0 || height < 1.0 {
        // Some hosts report empty icon rect; fall back to cursor with a typical icon size.
        return TrayIconRect {
            x: cursor.x,
            y: cursor.y,
            width: 22.0,
            height: 22.0,
        };
    }
    TrayIconRect {
        x,
        y,
        width,
        height,
    }
}

fn handle_menu_event(app: &AppHandle, id: &str) {
    match id {
        menu_id::OPEN_CURRENT => {
            let task_id = app
                .try_state::<HostState>()
                .and_then(|s| s.current_task_id.lock().ok().and_then(|g| g.clone()));
            open_or_focus_main(app, task_id.as_deref());
        }
        menu_id::OPEN_APP | menu_id::SUMMARY | menu_id::CURRENT => {
            open_or_focus_main(app, None);
        }
        menu_id::OPEN_POPOVER => {
            let (w, h) = popover_size_from_config();
            toggle_popover(app, None, w, h);
        }
        // Backward-compatible for stale tray/menu events from older builds.
        menu_id::HISTORY => open_or_focus_main(app, None),
        menu_id::SETTINGS => open_or_focus_settings(app, None),
        menu_id::RESTART_DAEMON => {
            let _ = crate::daemon::restart(false);
        }
        menu_id::QUIT => {
            app.exit(0);
        }
        _ => {}
    }
}

fn popover_size_from_config() -> (u32, u32) {
    ConfigDocument::load()
        .map(|d| (d.config.ui.popover_width, d.config.ui.popover_height))
        .unwrap_or((420, 620))
}

fn toggle_popover(
    app: &AppHandle,
    tray_rect: Option<TrayIconRect>,
    preferred_w: u32,
    preferred_h: u32,
) {
    let visible = app
        .try_state::<HostState>()
        .and_then(|s| s.popover_visible.lock().ok().map(|g| *g))
        .unwrap_or(false);

    if visible {
        if let Some(w) = app.get_webview_window(window_label::POPOVER) {
            let _ = w.hide();
        }
        if let Some(state) = app.try_state::<HostState>() {
            if let Ok(mut g) = state.popover_visible.lock() {
                *g = false;
            }
        }
        return;
    }

    show_popover(app, tray_rect, preferred_w, preferred_h);
}

fn show_popover(
    app: &AppHandle,
    tray_rect: Option<TrayIconRect>,
    preferred_w: u32,
    preferred_h: u32,
) {
    let work = primary_work_area(app);
    let (pw, ph) = effective_popover_size(preferred_w, preferred_h, work);
    let force_fallback = tray_rect.is_none() || !tray_click_coords_reliable();
    let (x, y) = compute_popover_position(PopoverPositionInput {
        tray: tray_rect,
        work_area: work,
        popover_width: pw,
        popover_height: ph,
        scale_factor: primary_scale(app),
        fallback: default_anchor_fallback(),
        force_fallback,
    });
    let (x, y) = clamp_to_work_area(x, y, pw, ph, work);

    if let Some(w) = app.get_webview_window(window_label::POPOVER) {
        let _ = w.set_size(tauri::Size::Logical(tauri::LogicalSize {
            width: pw,
            height: ph,
        }));
        let _ = w.set_position(tauri::Position::Logical(tauri::LogicalPosition { x, y }));
        let _ = w.show();
        let _ = w.set_focus();
    } else {
        let builder = WebviewWindowBuilder::new(
            app,
            window_label::POPOVER,
            WebviewUrl::App(surface_url("popover", None).into()),
        )
        .title("GrokTask")
        .inner_size(pw, ph)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(true)
        .visible(true)
        .focused(true)
        .position(x, y);

        if let Err(e) = builder.build() {
            eprintln!("gui-host popover window: {e}");
            return;
        }
    }

    if let Some(state) = app.try_state::<HostState>() {
        if let Ok(mut g) = state.popover_visible.lock() {
            *g = true;
        }
    }
}

fn primary_work_area(app: &AppHandle) -> Rect {
    if let Ok(Some(m)) = app.primary_monitor() {
        let size = m.size();
        let pos = m.position();
        let scale = m.scale_factor();
        // Work area approximation: full monitor in logical coords.
        return Rect {
            x: pos.x as f64 / scale,
            y: pos.y as f64 / scale,
            width: size.width as f64 / scale,
            height: size.height as f64 / scale,
        };
    }
    Rect {
        x: 0.0,
        y: 0.0,
        width: 1440.0,
        height: 900.0,
    }
}

fn primary_scale(app: &AppHandle) -> f64 {
    app.primary_monitor()
        .ok()
        .flatten()
        .map(|m| m.scale_factor())
        .unwrap_or(1.0)
}

fn open_or_focus_main(app: &AppHandle, task_id: Option<&str>) {
    let mut refresh_menu = false;
    if let Some(id) = task_id {
        if let Some(state) = app.try_state::<HostState>() {
            if let Ok(mut g) = state.current_task_id.lock() {
                *g = Some(id.to_string());
                refresh_menu = true;
            }
        }
    }
    if let Some(w) = app.get_webview_window(window_label::MAIN) {
        let _ = w.show();
        let _ = w.set_focus();
    } else {
        let url = surface_url("task", None);
        let _ = WebviewWindowBuilder::new(app, window_label::MAIN, WebviewUrl::App(url.into()))
            .title("GrokTask")
            .inner_size(1120.0, 760.0)
            .min_inner_size(900.0, 640.0)
            .visible(true)
            .build();
    }
    // Native menu must reflect current_task_id (enable "Open current task").
    if refresh_menu {
        refresh_tray_menu(app);
    }
}

fn open_or_focus_settings(app: &AppHandle, section: Option<&str>) {
    // Whitelist before any eval / URL construction so IPC cannot inject script.
    let section = tray::sanitize_settings_section(section);
    if let Some(w) = app.get_webview_window(window_label::SETTINGS) {
        let _ = w.show();
        let _ = w.set_focus();
        // Safe JSON-escaped CustomEvent (never string-interpolate raw section).
        if let Some(sec) = section {
            if let Some(js) = tray::settings_section_dispatch_js(sec) {
                let _ = w.eval(&js);
            }
        }
        return;
    }
    let url = surface_url("settings", section);
    let _ = WebviewWindowBuilder::new(app, window_label::SETTINGS, WebviewUrl::App(url.into()))
        .title("GrokTask Settings")
        .inner_size(780.0, 620.0)
        .min_inner_size(560.0, 420.0)
        .visible(true)
        .build();
}

/// Remember a trusted project workspace path from navigation (e.g. `GrokTask setup`).
fn remember_workspace_cwd(app: &AppHandle, cwd: Option<&str>) {
    let Some(raw) = cwd.map(str::trim).filter(|s| !s.is_empty()) else {
        return;
    };
    if let Some(state) = app.try_state::<HostState>() {
        if let Ok(mut g) = state.selected_workspace_cwd.lock() {
            *g = Some(raw.to_string());
        }
    }
}

/// Trusted project cwd for Settings task/MCP context display. `None` when the
/// host was opened without `GrokTask setup` (Finder / tray) — never the process
/// cwd. Workflow instruction injection is global and does not use this path.
pub fn selected_workspace_cwd(app: &AppHandle) -> Option<String> {
    let state = app.try_state::<HostState>()?;
    let guard = state.selected_workspace_cwd.lock().ok()?;
    guard.clone()
}

/// Pure resolver used by `workspace_cwd` and unit tests: only the stored
/// project path is accepted; process current_dir is never used as a fallback.
pub fn resolve_trusted_workspace_cwd(selected: Option<&str>) -> Result<String, String> {
    match selected.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => Ok(s.to_string()),
        None => Err("无法解析工作区路径；请从项目目录运行 GrokTask setup".into()),
    }
}

fn apply_nav(app: &AppHandle, cmd: GuiNavCommand) {
    match cmd {
        GuiNavCommand::OpenPopover => {
            let (w, h) = popover_size_from_config();
            show_popover(app, None, w, h);
        }
        GuiNavCommand::Focus | GuiNavCommand::OpenTask { .. } => {
            let task_id = match &cmd {
                GuiNavCommand::OpenTask { task_id } => Some(task_id.as_str()),
                _ => None,
            };
            open_or_focus_main(app, task_id);
        }
        // ACP records are now folded into the main task timeline.
        GuiNavCommand::OpenHistory => open_or_focus_main(app, None),
        GuiNavCommand::OpenSettings { section, cwd } => {
            remember_workspace_cwd(app, cwd.as_deref());
            open_or_focus_settings(app, section.as_deref());
        }
        GuiNavCommand::Quit => {
            app.exit(0);
        }
    }
}

async fn handle_gui_client(
    stream: IpcStream,
    tx: mpsc::UnboundedSender<GuiNavCommand>,
) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        let unix = stream.into_unix()?;
        let (reader, mut writer) = unix.into_split();
        let mut reader = BufReader::new(reader);
        while let Some(cmd) = read_msg::<_, GuiNavCommand>(&mut reader).await? {
            let _ = tx.send(cmd);
            let ack = serde_json::json!({"type":"response","ok":true});
            write_msg(&mut writer, &ack).await?;
        }
        Ok(())
    }
    #[cfg(windows)]
    {
        match stream {
            IpcStream::WindowsServer(s) => {
                let (reader, mut writer) = tokio::io::split(s);
                let mut reader = BufReader::new(reader);
                while let Some(cmd) = read_msg::<_, GuiNavCommand>(&mut reader).await? {
                    let _ = tx.send(cmd);
                    let ack = serde_json::json!({"type":"response","ok":true});
                    write_msg(&mut writer, &ack).await?;
                }
                Ok(())
            }
            IpcStream::Windows(s) => {
                let (reader, mut writer) = tokio::io::split(s);
                let mut reader = BufReader::new(reader);
                while let Some(cmd) = read_msg::<_, GuiNavCommand>(&mut reader).await? {
                    let _ = tx.send(cmd);
                    let ack = serde_json::json!({"type":"response","ok":true});
                    write_msg(&mut writer, &ack).await?;
                }
                Ok(())
            }
            #[allow(unreachable_patterns)]
            _ => Ok(()),
        }
    }
}

/// Client helper: send a navigation command to a running GUI host.
pub async fn send_nav(cmd: GuiNavCommand) -> anyhow::Result<()> {
    let stream = transport::connect_gui_host().await?;
    #[cfg(unix)]
    {
        let unix = stream.into_unix()?;
        let (reader, mut writer) = unix.into_split();
        let mut reader = BufReader::new(reader);
        write_msg(&mut writer, &cmd).await?;
        let _: Option<serde_json::Value> = read_msg(&mut reader).await?;
        Ok(())
    }
    #[cfg(windows)]
    {
        match stream {
            IpcStream::Windows(s) => {
                let (reader, mut writer) = tokio::io::split(s);
                let mut reader = BufReader::new(reader);
                write_msg(&mut writer, &cmd).await?;
                let _: Option<serde_json::Value> = read_msg(&mut reader).await?;
                Ok(())
            }
            _ => anyhow::bail!("unexpected stream type"),
        }
    }
}

/// Try to open navigation on existing host; returns true if delivered.
pub fn try_navigate(cmd: GuiNavCommand) -> bool {
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(_) => return false,
    };
    rt.block_on(send_nav(cmd)).is_ok()
}

/// Spawn a detached GUI host process (`--gui-host`).
pub fn spawn_detached() -> std::io::Result<()> {
    use std::process::{Command, Stdio};
    let exe = std::env::current_exe()?;
    let mut cmd = Command::new(exe);
    cmd.arg("--gui-host")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Ok(home) = std::env::var(paths::GROKTASK_HOME_ENV) {
        cmd.env(paths::GROKTASK_HOME_ENV, home);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x00000008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    cmd.spawn()?;
    Ok(())
}

/// Ensure GUI host is running and deliver a navigation command.
pub fn ensure_and_navigate(cmd: GuiNavCommand) -> Result<(), String> {
    if try_navigate(cmd.clone()) {
        return Ok(());
    }
    spawn_detached().map_err(|e| format!("failed to spawn gui-host: {e}"))?;
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if try_navigate(cmd.clone()) {
            return Ok(());
        }
    }
    Err("gui-host started but navigation was not acknowledged".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::lifecycle;
    use crate::paths::GROKTASK_HOME_ENV;
    use tempfile::TempDir;

    #[test]
    fn main_window_starts_hidden_until_navigation() {
        assert!(
            !initial_main_window_visible(),
            "GUI host main window must start hidden"
        );
    }

    #[test]
    fn gui_host_lock_exclusive() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("gui-host.lock");
        let a = lifecycle::acquire_lock_at(&path).unwrap();
        assert!(matches!(a, LockResult::Acquired(_)));
        let b = lifecycle::acquire_lock_at(&path).unwrap();
        assert!(matches!(b, LockResult::AlreadyRunning));
    }

    #[cfg(unix)]
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn gui_nav_ipc_roundtrip() {
        let _g = paths::test_env_lock();
        let tmp = TempDir::new().unwrap();
        let prev = std::env::var_os(GROKTASK_HOME_ENV);
        std::env::set_var(GROKTASK_HOME_ENV, tmp.path());

        transport::remove_stale_gui_endpoint();
        let listener = transport::bind_gui_host().unwrap();
        let (tx, mut rx) = mpsc::unbounded_channel();

        let server = tokio::spawn(async move {
            let stream = listener.accept().await.unwrap();
            handle_gui_client(stream, tx).await.unwrap();
        });

        send_nav(GuiNavCommand::OpenHistory).await.unwrap();
        let cmd = rx.recv().await.unwrap();
        assert_eq!(cmd, GuiNavCommand::OpenHistory);
        drop(server);

        match prev {
            Some(v) => std::env::set_var(GROKTASK_HOME_ENV, v),
            None => std::env::remove_var(GROKTASK_HOME_ENV),
        }
    }

    #[test]
    fn open_settings_with_section_roundtrips() {
        let cmd = GuiNavCommand::OpenSettings {
            section: Some("integrations".into()),
            cwd: None,
        };
        let v = serde_json::to_value(&cmd).unwrap();
        assert_eq!(v["method"], "gui.open_settings");
        assert_eq!(v["section"], "integrations");
        assert!(v.get("cwd").is_none());
        let back: GuiNavCommand = serde_json::from_value(v).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn open_settings_with_section_and_cwd_roundtrips() {
        let cmd = GuiNavCommand::OpenSettings {
            section: Some("integrations".into()),
            cwd: Some("/Users/dev/my-project".into()),
        };
        let v = serde_json::to_value(&cmd).unwrap();
        assert_eq!(v["method"], "gui.open_settings");
        assert_eq!(v["section"], "integrations");
        assert_eq!(v["cwd"], "/Users/dev/my-project");
        let back: GuiNavCommand = serde_json::from_value(v).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn open_settings_legacy_without_cwd_deserializes() {
        // Backward compatibility: older CLI/clients omit cwd entirely.
        let v = serde_json::json!({
            "method": "gui.open_settings",
            "section": "integrations"
        });
        let cmd: GuiNavCommand = serde_json::from_value(v).unwrap();
        assert_eq!(
            cmd,
            GuiNavCommand::OpenSettings {
                section: Some("integrations".into()),
                cwd: None,
            }
        );
    }

    #[test]
    fn trusted_workspace_cwd_rejects_missing_and_never_uses_slash() {
        // Without a selected project, UI must not treat `/` or process cwd as writable.
        assert!(resolve_trusted_workspace_cwd(None).is_err());
        assert!(resolve_trusted_workspace_cwd(Some("")).is_err());
        assert!(resolve_trusted_workspace_cwd(Some("   ")).is_err());
        let err = resolve_trusted_workspace_cwd(None).unwrap_err();
        assert!(
            err.contains("GrokTask setup") || err.contains("无法解析"),
            "error should guide user to setup from project: {err}"
        );
        assert_eq!(
            resolve_trusted_workspace_cwd(Some("/Users/dev/proj")).unwrap(),
            "/Users/dev/proj"
        );
        // Explicitly not process cwd / root fallback.
        assert_ne!(
            resolve_trusted_workspace_cwd(None).unwrap_or_else(|_| String::new()),
            "/"
        );
    }

    #[test]
    fn linux_fallback_anchor_is_top_right() {
        use crate::app::windows::AnchorFallback;
        let work = Rect {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        let (x, y) = compute_popover_position(PopoverPositionInput {
            tray: None,
            work_area: work,
            popover_width: 420.0,
            popover_height: 620.0,
            scale_factor: 1.0,
            fallback: AnchorFallback::TopRight,
            force_fallback: true,
        });
        assert!(x > 1000.0);
        assert!(y < 50.0);
    }
}
