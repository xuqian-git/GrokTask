//! Login-item adapters for tray mode `always`.
//!
//! - macOS: LaunchAgent plist under `~/Library/LaunchAgents`
//! - Windows: current-user Startup registry (Run key) — adapter writes to a
//!   configurable path in tests; production uses the registry
//! - Linux: XDG autostart `.desktop`
//!
//! Only `TrayMode::Always` installs/updates the login item. `off` and `active`
//! remove it. Tests must use temp directories / fake adapters — never install a
//! real login item during unit tests.

use crate::config::TrayMode;
use crate::version::{APP_IDENTIFIER, PRODUCT_NAME};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Stable label for LaunchAgent / desktop entry basenames.
pub fn login_item_label() -> String {
    format!("{APP_IDENTIFIER}.guihost")
}

/// Desired operation for a tray mode transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginItemOp {
    /// Install or refresh the login item to the current binary.
    InstallOrUpdate,
    /// Remove the login item if present.
    Remove,
}

/// Map tray mode → login-item operation. Pure; no I/O.
pub fn login_item_op_for_tray_mode(mode: TrayMode) -> LoginItemOp {
    match mode {
        TrayMode::Always => LoginItemOp::InstallOrUpdate,
        TrayMode::Off | TrayMode::Active => LoginItemOp::Remove,
    }
}

/// Abstract adapter so tests can use temp dirs without touching the real OS.
pub trait LoginItemAdapter {
    fn install(&self, exe: &Path) -> io::Result<()>;
    fn uninstall(&self) -> io::Result<()>;
    fn is_installed(&self) -> bool;
    /// True when installed content does not match `exe`.
    fn needs_update(&self, exe: &Path) -> bool;
}

/// Apply tray mode against an adapter (idempotent).
pub fn apply_tray_mode_login_item(
    adapter: &dyn LoginItemAdapter,
    mode: TrayMode,
    exe: &Path,
) -> io::Result<()> {
    match login_item_op_for_tray_mode(mode) {
        LoginItemOp::InstallOrUpdate => {
            if adapter.is_installed() && !adapter.needs_update(exe) {
                Ok(())
            } else {
                adapter.install(exe)
            }
        }
        LoginItemOp::Remove => adapter.uninstall(),
    }
}

// ---------------------------------------------------------------------------
// Pure content generators (testable without writing real login items)
// ---------------------------------------------------------------------------

/// macOS LaunchAgent plist body.
pub fn launch_agent_plist(label: &str, exe: &str) -> String {
    let exe = xml_escape(exe);
    let label = xml_escape(label);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe}</string>
        <string>--gui-host</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>ProcessType</key>
    <string>Interactive</string>
</dict>
</plist>
"#
    )
}

/// Linux XDG autostart desktop entry body.
pub fn xdg_autostart_desktop(name: &str, exe: &str) -> String {
    format!(
        "[Desktop Entry]\n\
Type=Application\n\
Name={name}\n\
Exec=\"{exe}\" --gui-host\n\
X-GNOME-Autostart-enabled=true\n\
NoDisplay=true\n\
Terminal=false\n"
    )
}

/// Windows registry Run-value payload (command line).
pub fn windows_run_command(exe: &str) -> String {
    format!("\"{exe}\" --gui-host")
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

// ---------------------------------------------------------------------------
// File-based adapter (temp-dir tests + Linux production)
// ---------------------------------------------------------------------------

/// Writes login-item files under configurable roots (no launchctl / registry).
#[derive(Debug, Clone)]
pub struct FileLoginItemAdapter {
    pub item_path: PathBuf,
    pub kind: FileLoginItemKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileLoginItemKind {
    LaunchAgentPlist,
    XdgDesktop,
    /// Simple text file holding the run command (Windows test stand-in).
    WindowsRunValue,
}

impl FileLoginItemAdapter {
    pub fn for_tests(dir: &Path, kind: FileLoginItemKind) -> Self {
        let name = match kind {
            FileLoginItemKind::LaunchAgentPlist => format!("{}.plist", login_item_label()),
            FileLoginItemKind::XdgDesktop => {
                format!("{}-guihost.desktop", PRODUCT_NAME.to_lowercase())
            }
            FileLoginItemKind::WindowsRunValue => "run-value.txt".into(),
        };
        Self {
            item_path: dir.join(name),
            kind,
        }
    }
}

impl LoginItemAdapter for FileLoginItemAdapter {
    fn install(&self, exe: &Path) -> io::Result<()> {
        if let Some(parent) = self.item_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let exe_s = exe.to_string_lossy();
        let body = match self.kind {
            FileLoginItemKind::LaunchAgentPlist => launch_agent_plist(&login_item_label(), &exe_s),
            FileLoginItemKind::XdgDesktop => {
                xdg_autostart_desktop(&format!("{PRODUCT_NAME} Menu Bar"), &exe_s)
            }
            FileLoginItemKind::WindowsRunValue => windows_run_command(&exe_s),
        };
        fs::write(&self.item_path, body)
    }

    fn uninstall(&self) -> io::Result<()> {
        if self.item_path.exists() {
            fs::remove_file(&self.item_path)?;
        }
        Ok(())
    }

    fn is_installed(&self) -> bool {
        self.item_path.exists()
    }

    fn needs_update(&self, exe: &Path) -> bool {
        if !self.is_installed() {
            return false;
        }
        let Ok(text) = fs::read_to_string(&self.item_path) else {
            return true;
        };
        let exe_s = exe.to_string_lossy();
        if !text.contains(exe_s.as_ref()) {
            return true;
        }
        // Legacy LaunchAgents used KeepAlive and respawned after Quit; rewrite.
        if self.kind == FileLoginItemKind::LaunchAgentPlist && text.contains("KeepAlive") {
            return true;
        }
        false
    }
}

// ---------------------------------------------------------------------------
// Platform production helpers (best-effort; failures do not block mode switch)
// ---------------------------------------------------------------------------

/// Sync login item with tray mode using the platform default adapter roots.
/// When `GROKTASK_LOGIN_ITEM_ROOT` is set (tests), uses that directory only.
pub fn sync_login_item_for_mode(mode: TrayMode) -> io::Result<()> {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from(PRODUCT_NAME));
    if let Ok(root) = std::env::var("GROKTASK_LOGIN_ITEM_ROOT") {
        if !root.is_empty() {
            let kind = default_file_kind();
            let adapter = FileLoginItemAdapter::for_tests(Path::new(&root), kind);
            return apply_tray_mode_login_item(&adapter, mode, &exe);
        }
    }
    platform_sync(mode, &exe)
}

fn default_file_kind() -> FileLoginItemKind {
    #[cfg(target_os = "macos")]
    {
        FileLoginItemKind::LaunchAgentPlist
    }
    #[cfg(target_os = "windows")]
    {
        FileLoginItemKind::WindowsRunValue
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        FileLoginItemKind::XdgDesktop
    }
}

#[cfg(target_os = "macos")]
fn platform_sync(mode: TrayMode, exe: &Path) -> io::Result<()> {
    let path = crate::paths::home()
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{}.plist", login_item_label()));
    let adapter = FileLoginItemAdapter {
        item_path: path.clone(),
        kind: FileLoginItemKind::LaunchAgentPlist,
    };
    apply_tray_mode_login_item(&adapter, mode, exe)?;
    // Best-effort launchctl refresh; ignore failures.
    let domain = format!("gui/{}", unsafe { libc::getuid() });
    match login_item_op_for_tray_mode(mode) {
        LoginItemOp::InstallOrUpdate => {
            let _ = run_silent(
                "launchctl",
                &["bootout", &domain, &path.display().to_string()],
            );
            let _ = run_silent(
                "launchctl",
                &["bootstrap", &domain, &path.display().to_string()],
            )
            .or_else(|_| run_silent("launchctl", &["load", "-w", &path.display().to_string()]));
        }
        LoginItemOp::Remove => {
            let _ = run_silent(
                "launchctl",
                &["bootout", &domain, &path.display().to_string()],
            );
            let _ = run_silent("launchctl", &["unload", &path.display().to_string()]);
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn platform_sync(mode: TrayMode, exe: &Path) -> io::Result<()> {
    let path = crate::paths::home()
        .join(".config")
        .join("autostart")
        .join(format!("{}-guihost.desktop", PRODUCT_NAME.to_lowercase()));
    let adapter = FileLoginItemAdapter {
        item_path: path,
        kind: FileLoginItemKind::XdgDesktop,
    };
    apply_tray_mode_login_item(&adapter, mode, exe)
}

#[cfg(target_os = "windows")]
fn platform_sync(mode: TrayMode, exe: &Path) -> io::Result<()> {
    // Prefer writing a Startup folder shortcut-like .cmd for testability without
    // requiring admin. Registry is attempted but file adapter under Startup is
    // the reliable cross-session path.
    let startup = windows_startup_dir()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "cannot resolve Startup folder"))?;
    let path = startup.join(format!("{PRODUCT_NAME}-guihost.cmd"));
    match login_item_op_for_tray_mode(mode) {
        LoginItemOp::InstallOrUpdate => {
            fs::create_dir_all(&startup)?;
            let body = format!("@echo off\r\nstart \"\" {} --gui-host\r\n", quote_win(exe));
            fs::write(&path, body)?;
        }
        LoginItemOp::Remove => {
            if path.exists() {
                fs::remove_file(&path)?;
            }
        }
    }
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn platform_sync(_mode: TrayMode, _exe: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(target_os = "windows")]
fn windows_startup_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| {
        PathBuf::from(a)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join("Startup")
    })
}

#[cfg(target_os = "windows")]
fn quote_win(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

#[cfg(target_os = "macos")]
fn run_silent(cmd: &str, args: &[&str]) -> io::Result<()> {
    use std::process::{Command, Stdio};
    let status = Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("{cmd} exited with {status}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn tray_mode_maps_to_login_item_op() {
        assert_eq!(
            login_item_op_for_tray_mode(TrayMode::Always),
            LoginItemOp::InstallOrUpdate
        );
        assert_eq!(
            login_item_op_for_tray_mode(TrayMode::Off),
            LoginItemOp::Remove
        );
        assert_eq!(
            login_item_op_for_tray_mode(TrayMode::Active),
            LoginItemOp::Remove
        );
    }

    #[test]
    fn file_adapter_always_installs_off_removes() {
        let tmp = TempDir::new().unwrap();
        let adapter =
            FileLoginItemAdapter::for_tests(tmp.path(), FileLoginItemKind::LaunchAgentPlist);
        let exe = Path::new("/opt/GrokTask");

        apply_tray_mode_login_item(&adapter, TrayMode::Always, exe).unwrap();
        assert!(adapter.is_installed());
        let text = fs::read_to_string(&adapter.item_path).unwrap();
        assert!(text.contains("/opt/GrokTask"));
        assert!(text.contains("--gui-host"));
        assert!(text.contains(&login_item_label()));

        apply_tray_mode_login_item(&adapter, TrayMode::Active, exe).unwrap();
        assert!(!adapter.is_installed());

        apply_tray_mode_login_item(&adapter, TrayMode::Always, exe).unwrap();
        apply_tray_mode_login_item(&adapter, TrayMode::Off, exe).unwrap();
        assert!(!adapter.is_installed());
    }

    #[test]
    fn always_updates_binary_path() {
        let tmp = TempDir::new().unwrap();
        let adapter = FileLoginItemAdapter::for_tests(tmp.path(), FileLoginItemKind::XdgDesktop);
        apply_tray_mode_login_item(&adapter, TrayMode::Always, Path::new("/old/GrokTask")).unwrap();
        assert!(adapter.needs_update(Path::new("/new/GrokTask")));
        apply_tray_mode_login_item(&adapter, TrayMode::Always, Path::new("/new/GrokTask")).unwrap();
        assert!(!adapter.needs_update(Path::new("/new/GrokTask")));
        let text = fs::read_to_string(&adapter.item_path).unwrap();
        assert!(text.contains("/new/GrokTask"));
        assert!(!text.contains("/old/GrokTask"));
    }

    #[test]
    fn launch_agent_rewrites_legacy_keepalive_plist() {
        let tmp = TempDir::new().unwrap();
        let adapter =
            FileLoginItemAdapter::for_tests(tmp.path(), FileLoginItemKind::LaunchAgentPlist);
        let exe = Path::new("/opt/GrokTask");
        // Simulate a previously installed KeepAlive plist with matching binary path.
        let legacy = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
    <key>Label</key><string>{}</string>
    <key>ProgramArguments</key><array>
        <string>/opt/GrokTask</string><string>--gui-host</string>
    </array>
    <key>RunAtLoad</key><true/>
    <key>KeepAlive</key><true/>
</dict></plist>
"#,
            login_item_label()
        );
        fs::write(&adapter.item_path, legacy).unwrap();
        assert!(adapter.needs_update(exe));
        apply_tray_mode_login_item(&adapter, TrayMode::Always, exe).unwrap();
        let text = fs::read_to_string(&adapter.item_path).unwrap();
        assert!(!text.contains("KeepAlive"));
        assert!(text.contains("RunAtLoad"));
        assert!(!adapter.needs_update(exe));
    }

    #[test]
    fn plist_and_desktop_generators() {
        let p = launch_agent_plist("ai.x.groktask.guihost", "/bin/GrokTask");
        assert!(p.contains("<string>/bin/GrokTask</string>"));
        assert!(p.contains("<string>--gui-host</string>"));
        assert!(p.contains("<key>RunAtLoad</key>"));
        // Quit must not immediately respawn: no KeepAlive.
        assert!(
            !p.contains("KeepAlive"),
            "LaunchAgent must not KeepAlive so Quit stays quit"
        );

        let d = xdg_autostart_desktop("GrokTask Menu Bar", "/bin/GrokTask");
        assert!(d.contains("Exec=\"/bin/GrokTask\" --gui-host"));
        assert!(d.contains("X-GNOME-Autostart-enabled=true"));

        let w = windows_run_command(r"C:\GrokTask.exe");
        assert_eq!(w, r#""C:\GrokTask.exe" --gui-host"#);
    }

    #[test]
    fn env_root_never_touches_real_login_items() {
        let _g = crate::paths::test_env_lock();
        let tmp = TempDir::new().unwrap();
        let prev = std::env::var_os("GROKTASK_LOGIN_ITEM_ROOT");
        std::env::set_var("GROKTASK_LOGIN_ITEM_ROOT", tmp.path());
        sync_login_item_for_mode(TrayMode::Always).unwrap();
        // Something was written under temp root.
        let entries: Vec<_> = fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert!(!entries.is_empty());
        sync_login_item_for_mode(TrayMode::Off).unwrap();
        let entries: Vec<_> = fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert!(entries.is_empty());
        match prev {
            Some(v) => std::env::set_var("GROKTASK_LOGIN_ITEM_ROOT", v),
            None => std::env::remove_var("GROKTASK_LOGIN_ITEM_ROOT"),
        }
    }
}
