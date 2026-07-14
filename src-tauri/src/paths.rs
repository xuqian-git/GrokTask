//! Cross-platform user data paths under `~/.groktask/`.
//!
//! Override with absolute (or cwd-relative) `GROKTASK_HOME` for tests and isolation.

use std::path::PathBuf;
use std::sync::Mutex;

/// Environment variable that relocates the entire GrokTask home directory.
pub const GROKTASK_HOME_ENV: &str = "GROKTASK_HOME";

static ENV_TEST_LOCK: Mutex<()> = Mutex::new(());

/// User home directory (never panics; falls back to `.`).
pub fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// Config / runtime root: default `~/.groktask`, or `GROKTASK_HOME` when set.
pub fn config_dir() -> PathBuf {
    if let Ok(raw) = std::env::var(GROKTASK_HOME_ENV) {
        if !raw.is_empty() {
            let p = PathBuf::from(raw);
            if p.is_absolute() {
                return p;
            }
            return std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(p);
        }
    }
    home().join(".groktask")
}

/// Ensure config directory exists with owner-only permissions on Unix.
pub fn ensure_config_dir() -> std::io::Result<PathBuf> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    Ok(dir)
}

pub fn config_file() -> PathBuf {
    config_dir().join("config.json")
}

pub fn history_db() -> PathBuf {
    config_dir().join("history.sqlite3")
}

pub fn daemon_lock() -> PathBuf {
    config_dir().join("daemon.lock")
}

pub fn daemon_meta() -> PathBuf {
    config_dir().join("daemon.json")
}

pub fn daemon_sock() -> PathBuf {
    config_dir().join("daemon.sock")
}

pub fn daemon_log() -> PathBuf {
    config_dir().join("daemon.log")
}

pub fn gui_host_lock() -> PathBuf {
    config_dir().join("gui-host.lock")
}

pub fn gui_host_sock() -> PathBuf {
    config_dir().join("gui-host.sock")
}

pub fn gui_log() -> PathBuf {
    config_dir().join("gui.log")
}

/// Windows named-pipe name for the daemon (not a filesystem path).
pub fn daemon_pipe_name() -> String {
    format!(r"\\.\pipe\groktask-daemon-{}", sid_hash())
}

/// Windows named-pipe name for the GUI host.
pub fn gui_host_pipe_name() -> String {
    format!(r"\\.\pipe\groktask-gui-{}", sid_hash())
}

/// Stable short hash of the current user identity for pipe naming.
fn sid_hash() -> String {
    #[cfg(windows)]
    {
        windows_sid_hash().unwrap_or_else(|_| "user".into())
    }
    #[cfg(not(windows))]
    {
        let uid = unsafe { libc::getuid() };
        format!("{uid:x}")
    }
}

#[cfg(windows)]
fn windows_sid_hash() -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    // Use USERNAME + USERDOMAIN as a stable per-user token for pipe naming.
    // Full SID ACL is applied separately at pipe creation time.
    let user = std::env::var_os("USERNAME").unwrap_or_else(|| OsString::from("user"));
    let domain = std::env::var_os("USERDOMAIN").unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(user.to_string_lossy().as_bytes());
    hasher.update(b"\\");
    hasher.update(domain.to_string_lossy().as_bytes());
    let digest = hasher.finalize();
    Ok(hex::encode(&digest[..8]))
}

/// Hold the test env lock so concurrent tests do not race on `GROKTASK_HOME`.
#[cfg(test)]
pub fn test_env_lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn config_dir_respects_groktask_home() {
        let _g = test_env_lock();
        let tmp = TempDir::new().unwrap();
        let prev = std::env::var_os(GROKTASK_HOME_ENV);
        std::env::set_var(GROKTASK_HOME_ENV, tmp.path());
        assert_eq!(config_dir(), tmp.path());
        match prev {
            Some(v) => std::env::set_var(GROKTASK_HOME_ENV, v),
            None => std::env::remove_var(GROKTASK_HOME_ENV),
        }
    }

    #[test]
    fn history_db_lives_under_config_dir() {
        let _g = test_env_lock();
        let tmp = TempDir::new().unwrap();
        let prev = std::env::var_os(GROKTASK_HOME_ENV);
        std::env::set_var(GROKTASK_HOME_ENV, tmp.path());
        assert_eq!(history_db(), tmp.path().join("history.sqlite3"));
        match prev {
            Some(v) => std::env::set_var(GROKTASK_HOME_ENV, v),
            None => std::env::remove_var(GROKTASK_HOME_ENV),
        }
    }
}
