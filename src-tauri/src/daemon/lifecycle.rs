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

/// Remove `daemon.json` (best-effort).
pub fn remove_meta() {
    let _ = std::fs::remove_file(paths::daemon_meta());
}

/// Remove daemon meta + Unix socket endpoint. Does **not** signal any process.
pub fn clear_daemon_runtime_files() {
    remove_meta();
    #[cfg(unix)]
    {
        let _ = std::fs::remove_file(paths::daemon_sock());
    }
}

/// Why a meta file does not represent a healthy serving daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaleReason {
    /// PID is dead, does not exist, or is a zombie/defunct entry.
    ProcessNotLive,
    /// Process appears live but the IPC endpoint is missing or not connectable.
    EndpointUnusable,
}

/// Result of inspecting on-disk daemon metadata against process/endpoint reality.
#[derive(Debug, Clone)]
pub enum DaemonPresence {
    /// No `daemon.json`.
    Absent,
    /// Meta present and daemon is considered healthy (live, non-zombie PID + usable endpoint).
    Running(DaemonMeta),
    /// Meta present but not a healthy daemon. Runtime files may still be on disk until reclaimed.
    Stale {
        meta: DaemonMeta,
        reason: StaleReason,
    },
}

/// True when the process table has a non-zombie entry for `pid`.
///
/// `kill(pid, 0)` alone is insufficient on Unix: zombie/defunct PIDs still
/// succeed that probe, which previously left CLI lifecycle stuck on stale meta.
pub fn process_is_live(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(unix)]
    {
        let rc = unsafe { libc::kill(pid as i32, 0) };
        if rc != 0 {
            return false;
        }
        // Defunct/zombie entries must not count as a running daemon.
        !process_is_zombie(pid).unwrap_or(false)
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h == 0 || h == -1isize as _ {
                return false;
            }
            CloseHandle(h);
            true
        }
    }
}

/// Whether the daemon IPC endpoint appears present and accept()ing connections.
pub fn daemon_endpoint_usable() -> bool {
    #[cfg(unix)]
    {
        let path = paths::daemon_sock();
        if !path.exists() {
            return false;
        }
        // A leftover socket inode with no listener is not usable.
        match std::os::unix::net::UnixStream::connect(&path) {
            Ok(_stream) => true,
            Err(_) => false,
        }
    }
    #[cfg(windows)]
    {
        daemon_endpoint_usable_windows()
    }
}

#[cfg(windows)]
fn daemon_endpoint_usable_windows() -> bool {
    // Best-effort: try opening the named pipe once. Busy/open failures that are
    // not "not found" still imply a server side exists.
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::Storage::FileSystem::{FILE_GENERIC_READ, FILE_GENERIC_WRITE};

    let name = paths::daemon_pipe_name();
    let wide: Vec<u16> = OsStr::new(&name)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let h = CreateFileW(
            wide.as_ptr(),
            FILE_GENERIC_READ | FILE_GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            0,
        );
        if h == 0 || h == INVALID_HANDLE_VALUE {
            return false;
        }
        CloseHandle(h);
        true
    }
}

/// Live non-zombie process **and** a connectable IPC endpoint.
pub fn is_daemon_healthy(meta: &DaemonMeta) -> bool {
    process_is_live(meta.pid) && daemon_endpoint_usable()
}

/// Inspect meta against process table and endpoint without mutating disk.
pub fn inspect_daemon() -> io::Result<DaemonPresence> {
    match read_meta()? {
        None => Ok(DaemonPresence::Absent),
        Some(meta) => {
            if !process_is_live(meta.pid) {
                return Ok(DaemonPresence::Stale {
                    meta,
                    reason: StaleReason::ProcessNotLive,
                });
            }
            if !daemon_endpoint_usable() {
                return Ok(DaemonPresence::Stale {
                    meta,
                    reason: StaleReason::EndpointUnusable,
                });
            }
            Ok(DaemonPresence::Running(meta))
        }
    }
}

/// If meta claims a daemon that is not healthy, remove meta + socket.
/// Never signals processes (safe when PID identity cannot be verified).
///
/// Returns `true` when stale state was cleared.
pub fn reclaim_stale_daemon_state() -> io::Result<bool> {
    match inspect_daemon()? {
        DaemonPresence::Stale { meta, reason } => {
            log_line(&format!(
                "reclaiming stale daemon state pid={} reason={reason:?} endpoint={}",
                meta.pid, meta.socket
            ));
            clear_daemon_runtime_files();
            Ok(true)
        }
        DaemonPresence::Absent | DaemonPresence::Running(_) => Ok(false),
    }
}

/// Whether `pid` is a zombie/defunct process table entry.
///
/// Returns `None` when the platform cannot determine state (treat as non-zombie
/// only if other health signals still pass).
#[cfg(unix)]
fn process_is_zombie(pid: u32) -> Option<bool> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        process_is_zombie_linux(pid)
    }
    #[cfg(target_os = "macos")]
    {
        process_is_zombie_macos(pid)
    }
    #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos")))]
    {
        let _ = pid;
        None
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn process_is_zombie_linux(pid: u32) -> Option<bool> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // Format: "pid (comm) state ..." — comm may contain spaces/parens; state follows the last ')'.
    let after_comm = stat.rsplit_once(')')?.1;
    let state = after_comm.trim_start().chars().next()?;
    Some(state == 'Z')
}

#[cfg(target_os = "macos")]
fn process_is_zombie_macos(pid: u32) -> Option<bool> {
    // libproc PROC_PIDTBSDINFO — `pbi_status == SZOMB (5)` means defunct.
    // We only need the first two u32 fields (`pbi_flags`, `pbi_status`); the
    // buffer is oversized to match Darwin's `struct proc_bsdinfo`.
    const PROC_PIDTBSDINFO: i32 = 3;
    const SZOMB: u32 = 5;
    // Historical sizeof(struct proc_bsdinfo) is 296–304 depending on SDK; 512 is safe.
    const BUF_LEN: usize = 512;

    extern "C" {
        fn proc_pidinfo(
            pid: i32,
            flavor: i32,
            arg: u64,
            buffer: *mut libc::c_void,
            buffersize: i32,
        ) -> i32;
    }

    let mut buf = [0u8; BUF_LEN];
    let n = unsafe {
        proc_pidinfo(
            pid as i32,
            PROC_PIDTBSDINFO,
            0,
            buf.as_mut_ptr() as *mut libc::c_void,
            BUF_LEN as i32,
        )
    };
    if n < 8 {
        return None;
    }
    let status = u32::from_ne_bytes(buf[4..8].try_into().ok()?);
    Some(status == SZOMB)
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
    use crate::paths::GROKTASK_HOME_ENV;
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

    #[test]
    fn process_is_live_rejects_zero_and_missing_pid() {
        assert!(!process_is_live(0));
        // Extremely unlikely to be allocated; kill(0)/OpenProcess should fail.
        assert!(!process_is_live(u32::MAX - 1));
        assert!(process_is_live(std::process::id()));
    }

    fn with_temp_home<F: FnOnce()>(f: F) {
        let _g = paths::test_env_lock();
        let tmp = TempDir::new().unwrap();
        let prev = std::env::var_os(GROKTASK_HOME_ENV);
        std::env::set_var(GROKTASK_HOME_ENV, tmp.path());
        f();
        match prev {
            Some(v) => std::env::set_var(GROKTASK_HOME_ENV, v),
            None => std::env::remove_var(GROKTASK_HOME_ENV),
        }
    }

    #[test]
    fn inspect_dead_pid_is_stale_not_running() {
        with_temp_home(|| {
            let mut meta = DaemonMeta::new(
                paths::daemon_sock().display().to_string(),
                BinaryFingerprint::ZERO,
            );
            meta.pid = u32::MAX - 1;
            write_meta(&meta).unwrap();

            match inspect_daemon().unwrap() {
                DaemonPresence::Stale {
                    reason: StaleReason::ProcessNotLive,
                    meta: got,
                } => assert_eq!(got.pid, meta.pid),
                other => panic!("expected ProcessNotLive stale, got {other:?}"),
            }
            assert!(!is_daemon_healthy(&meta));
        });
    }

    #[test]
    fn inspect_live_pid_without_socket_is_stale() {
        with_temp_home(|| {
            let mut meta = DaemonMeta::new(
                paths::daemon_sock().display().to_string(),
                BinaryFingerprint::ZERO,
            );
            meta.pid = std::process::id();
            write_meta(&meta).unwrap();
            // No daemon.sock — must not report healthy Running.
            assert!(!paths::daemon_sock().exists());
            assert!(!daemon_endpoint_usable());

            match inspect_daemon().unwrap() {
                DaemonPresence::Stale {
                    reason: StaleReason::EndpointUnusable,
                    ..
                } => {}
                other => panic!("expected EndpointUnusable stale, got {other:?}"),
            }
        });
    }

    #[test]
    fn reclaim_clears_stale_meta_and_socket_files() {
        with_temp_home(|| {
            let mut meta = DaemonMeta::new(
                paths::daemon_sock().display().to_string(),
                BinaryFingerprint::ZERO,
            );
            meta.pid = u32::MAX - 1;
            write_meta(&meta).unwrap();
            // Leftover socket inode (not listening).
            std::fs::write(paths::daemon_sock(), b"").unwrap();

            assert!(reclaim_stale_daemon_state().unwrap());
            assert!(!paths::daemon_meta().exists());
            assert!(!paths::daemon_sock().exists());
            assert!(matches!(inspect_daemon().unwrap(), DaemonPresence::Absent));
            // Second reclaim is a no-op.
            assert!(!reclaim_stale_daemon_state().unwrap());
        });
    }

    #[cfg(unix)]
    #[test]
    fn inspect_running_requires_connectable_socket() {
        with_temp_home(|| {
            use std::os::unix::net::UnixListener;

            // Bound listener keeps the socket connectable; health probes only
            // connect+drop and do not require an active accept loop.
            let _listener = UnixListener::bind(paths::daemon_sock()).unwrap();
            let mut meta = DaemonMeta::new(
                paths::daemon_sock().display().to_string(),
                BinaryFingerprint::ZERO,
            );
            meta.pid = std::process::id();
            write_meta(&meta).unwrap();

            match inspect_daemon().unwrap() {
                DaemonPresence::Running(got) => assert_eq!(got.pid, meta.pid),
                other => panic!("expected Running, got {other:?}"),
            }
            assert!(is_daemon_healthy(&meta));
            clear_daemon_runtime_files();
        });
    }
}
