//! Daemon single-instance lock, meta file, and replacement barrier state.

use crate::fingerprint::BinaryFingerprint;
use crate::paths;
use crate::version::{APP_VERSION, PROTOCOL_VERSION};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// Result of attempting to become the unique daemon.
pub enum LockResult {
    Acquired(LockGuard),
    AlreadyRunning,
}

/// RAII single-instance lock. Drop releases the lock.
pub struct LockGuard {
    _file: File,
    path: PathBuf,
}

impl LockGuard {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Non-blocking exclusive lock on `daemon.lock`.
pub fn acquire_lock() -> io::Result<LockResult> {
    acquire_lock_at(&paths::daemon_lock())
}

pub fn acquire_lock_at(path: &Path) -> io::Result<LockResult> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
        }
    }
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)?;

    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let fd = file.as_raw_fd();
        let rc = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
        if rc != 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::WouldBlock
                || err.raw_os_error() == Some(libc::EWOULDBLOCK)
                || err.raw_os_error() == Some(libc::EAGAIN)
            {
                return Ok(LockResult::AlreadyRunning);
            }
            return Err(err);
        }
    }

    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::GetLastError;
        use windows_sys::Win32::Storage::FileSystem::{
            LockFileEx, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY,
        };
        use windows_sys::Win32::System::IO::OVERLAPPED;
        let handle = file.as_raw_handle();
        let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
        let ok = unsafe {
            LockFileEx(
                handle as _,
                LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
                0,
                u32::MAX,
                u32::MAX,
                &mut overlapped,
            )
        };
        if ok == 0 {
            let code = unsafe { GetLastError() };
            // ERROR_LOCK_VIOLATION = 33
            if code == 33 {
                return Ok(LockResult::AlreadyRunning);
            }
            return Err(io::Error::from_raw_os_error(code as i32));
        }
    }

    Ok(LockResult::Acquired(LockGuard {
        _file: file,
        path: path.to_path_buf(),
    }))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DaemonMeta {
    pub pid: u32,
    pub version: String,
    pub protocol_version: u32,
    pub started_at: u64,
    pub socket: String,
    pub fingerprint: BinaryFingerprint,
    pub daemon_instance_id: String,
    #[serde(default)]
    pub status: DaemonRunStatus,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DaemonRunStatus {
    #[default]
    Running,
    Draining,
    Restarting,
    ReplacementDeferred,
}

impl DaemonMeta {
    pub fn new(endpoint: impl Into<String>, fingerprint: BinaryFingerprint) -> Self {
        Self {
            pid: std::process::id(),
            version: APP_VERSION.to_string(),
            protocol_version: PROTOCOL_VERSION,
            started_at: now_secs(),
            socket: endpoint.into(),
            fingerprint,
            daemon_instance_id: Uuid::new_v4().to_string(),
            status: DaemonRunStatus::Running,
        }
    }
}

pub fn write_meta(meta: &DaemonMeta) -> io::Result<()> {
    write_meta_at(&paths::daemon_meta(), meta)
}

pub fn write_meta_at(path: &Path, meta: &DaemonMeta) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let data = serde_json::to_vec_pretty(meta)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    crate::config::atomic_write(path, &data)
}

pub fn read_meta() -> io::Result<Option<DaemonMeta>> {
    read_meta_at(&paths::daemon_meta())
}

pub fn read_meta_at(path: &Path) -> io::Result<Option<DaemonMeta>> {
    if !path.exists() {
        return Ok(None);
    }
    let data = std::fs::read(path)?;
    let meta =
        serde_json::from_slice(&data).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(Some(meta))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Replacement / result-delivery barrier
// ---------------------------------------------------------------------------

/// Tracks in-flight work that blocks graceful binary replacement.
#[derive(Debug, Default)]
pub struct ReplacementBarrier {
    /// Active turns in queued/starting/running/cancelling.
    pub active_turns: std::sync::atomic::AtomicUsize,
    pub active_recoveries: std::sync::atomic::AtomicUsize,
    pub inflight_requests: std::sync::atomic::AtomicUsize,
    /// Responses accepted but not yet write_all+flush'd to a live connection.
    pub undelivered_accepts: std::sync::atomic::AtomicUsize,
    pub draining: std::sync::atomic::AtomicBool,
}

impl ReplacementBarrier {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_safe_to_replace(&self) -> bool {
        use std::sync::atomic::Ordering::SeqCst;
        self.active_turns.load(SeqCst) == 0
            && self.active_recoveries.load(SeqCst) == 0
            && self.inflight_requests.load(SeqCst) == 0
            && self.undelivered_accepts.load(SeqCst) == 0
    }

    pub fn begin_drain(&self) {
        self.draining
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn is_draining(&self) -> bool {
        self.draining.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn cancel_drain(&self) {
        self.draining
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Max drain wait for automatic replacement (spec).
pub const REPLACEMENT_DRAIN_SECS: u64 = 600;
/// Explicit restart without activity: 30s.
pub const EXPLICIT_RESTART_WAIT_SECS: u64 = 30;

pub fn retry_until_rfc3339(from: SystemTime, wait_secs: u64) -> String {
    let t = from + std::time::Duration::from_secs(wait_secs);
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    chrono::DateTime::from_timestamp(secs, 0)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_else(|| "1970-01-01T00:00:00Z".into())
}

/// Append a line to daemon.log (best-effort). Diagnostics never go to stdout.
pub fn log_line(msg: &str) {
    let path = paths::daemon_log();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "[{}] {msg}", now_secs());
    }
    // Also to stderr for foreground `daemon run`.
    let _ = writeln!(std::io::stderr(), "{msg}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn concurrent_lock_only_one_succeeds() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("daemon.lock");
        let a = acquire_lock_at(&path).unwrap();
        assert!(matches!(a, LockResult::Acquired(_)));
        let b = acquire_lock_at(&path).unwrap();
        assert!(matches!(b, LockResult::AlreadyRunning));
        drop(a);
        let c = acquire_lock_at(&path).unwrap();
        assert!(matches!(c, LockResult::Acquired(_)));
    }

    #[test]
    fn meta_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("daemon.json");
        let meta = DaemonMeta::new("/tmp/x.sock", BinaryFingerprint::ZERO);
        write_meta_at(&path, &meta).unwrap();
        let got = read_meta_at(&path).unwrap().unwrap();
        assert_eq!(got.pid, meta.pid);
        assert_eq!(got.daemon_instance_id, meta.daemon_instance_id);
    }

    #[test]
    fn replacement_barrier_gates() {
        let b = ReplacementBarrier::new();
        assert!(b.is_safe_to_replace());
        b.active_turns
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        assert!(!b.is_safe_to_replace());
        b.active_turns
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        b.undelivered_accepts
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        assert!(!b.is_safe_to_replace());
    }
}
