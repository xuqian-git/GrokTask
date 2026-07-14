//! Per-user daemon: single instance, IPC listener, storage, lifecycle.

pub mod lifecycle;
pub mod spawn;

use crate::config::ConfigHandle;
use crate::fingerprint::BinaryFingerprint;
use crate::ipc::codec::{read_msg, write_msg};
use crate::ipc::protocol::{ClientRole, Hello, HelloAck, HelloStatus, Request, Response};
use crate::ipc::transport::{self, IpcStream};
use crate::storage::{self, DeletionGuards};
use crate::version::{APP_VERSION, PROTOCOL_VERSION};
use anyhow::{anyhow, Context, Result};
use lifecycle::{
    acquire_lock, read_meta, write_meta, DaemonMeta, DaemonRunStatus, LockResult,
    ReplacementBarrier, REPLACEMENT_DRAIN_SECS,
};
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::sync::watch;

/// Foreground `daemon run` entry — never initializes Tauri.
pub fn run_foreground() -> Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("tokio runtime")?;
    rt.block_on(run_async())
}

async fn run_async() -> Result<()> {
    crate::paths::ensure_config_dir()?;
    let lock = match acquire_lock()? {
        LockResult::Acquired(g) => g,
        LockResult::AlreadyRunning => {
            return Err(anyhow!("daemon already running (lock held)"));
        }
    };

    // Holding lock: safe to clear stale endpoint.
    transport::remove_stale_daemon_endpoint();

    let config = ConfigHandle::load_default().context("load config")?;
    let db_path = crate::paths::history_db();
    // Open once to migrate; connections opened per request in foundations.
    {
        let _conn = storage::open_path(&db_path).context("open history db")?;
    }

    let listener = transport::bind_daemon().context("bind daemon endpoint")?;
    let endpoint = transport::daemon_endpoint_display();
    let fingerprint = BinaryFingerprint::current();
    let mut meta = DaemonMeta::new(&endpoint, fingerprint);
    write_meta(&meta)?;
    lifecycle::log_line(&format!(
        "daemon listening on {endpoint} instance={}",
        meta.daemon_instance_id
    ));

    let barrier = Arc::new(ReplacementBarrier::new());
    let guards = Arc::new(DeletionGuards::new());
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
    let state = Arc::new(DaemonState {
        meta: Mutex::new(meta.clone()),
        config,
        barrier: barrier.clone(),
        guards,
        db_path,
    });

    // Accept loop
    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    break;
                }
            }
            accept = listener.accept() => {
                match accept {
                    Ok(stream) => {
                        let st = state.clone();
                        let stop = shutdown_tx.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(stream, st, stop).await {
                                lifecycle::log_line(&format!("connection error: {e:#}"));
                            }
                        });
                    }
                    Err(e) => {
                        lifecycle::log_line(&format!("accept error: {e}"));
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                }
            }
        }
    }

    meta.status = DaemonRunStatus::Draining;
    {
        let mut m = state.meta.lock();
        m.status = DaemonRunStatus::Draining;
        let _ = write_meta(&m);
    }
    lifecycle::log_line("daemon shutting down");
    drop(lock);
    Ok(())
}

struct DaemonState {
    meta: Mutex<DaemonMeta>,
    config: ConfigHandle,
    barrier: Arc<ReplacementBarrier>,
    guards: Arc<DeletionGuards>,
    db_path: std::path::PathBuf,
}

async fn handle_connection(
    stream: IpcStream,
    state: Arc<DaemonState>,
    _shutdown: watch::Sender<bool>,
) -> Result<()> {
    #[cfg(unix)]
    {
        let unix = stream.into_unix()?;
        let (reader, mut writer) = unix.into_split();
        let mut reader = BufReader::new(reader);
        serve_client(&mut reader, &mut writer, state).await
    }
    #[cfg(windows)]
    {
        match stream {
            IpcStream::WindowsServer(s) => {
                let (mut reader, mut writer) = tokio::io::split(s);
                let mut reader = BufReader::new(reader);
                serve_client(&mut reader, &mut writer, state).await
            }
            IpcStream::Windows(s) => {
                let (mut reader, mut writer) = tokio::io::split(s);
                let mut reader = BufReader::new(reader);
                serve_client(&mut reader, &mut writer, state).await
            }
            #[cfg(unix)]
            IpcStream::Unix(_) => unreachable!(),
        }
    }
}

async fn serve_client<R, W>(reader: &mut R, writer: &mut W, state: Arc<DaemonState>) -> Result<()>
where
    R: tokio::io::AsyncBufRead + Unpin,
    W: AsyncWriteExt + Unpin,
{
    // First frame must be hello.
    let hello: Option<Hello> = read_msg(reader).await?;
    let Some(hello) = hello else {
        return Ok(());
    };
    if hello.r#type != "hello" {
        return Err(anyhow!("first frame must be hello"));
    }

    let meta = state.meta.lock().clone();
    let ack = if hello.protocol_version != PROTOCOL_VERSION {
        HelloAck::incompatible(
            &hello.request_id,
            format!(
                "protocol version {} unsupported (daemon={})",
                hello.protocol_version, PROTOCOL_VERSION
            ),
        )
    } else if hello.binary_fingerprint != meta.fingerprint
        && hello.binary_fingerprint != BinaryFingerprint::ZERO
        && meta.fingerprint != BinaryFingerprint::ZERO
    {
        // Fingerprint mismatch → graceful replacement path.
        if state.barrier.is_safe_to_replace() {
            HelloAck {
                r#type: "hello_ack".into(),
                request_id: hello.request_id.clone(),
                protocol_version: PROTOCOL_VERSION,
                daemon_version: APP_VERSION.into(),
                status: HelloStatus::Restarting,
                reason: Some("binary fingerprint changed".into()),
                retry_until: Some(lifecycle::retry_until_rfc3339(
                    SystemTime::now(),
                    REPLACEMENT_DRAIN_SECS,
                )),
                daemon_instance_id: Some(meta.daemon_instance_id.clone()),
            }
        } else {
            state.barrier.begin_drain();
            HelloAck {
                r#type: "hello_ack".into(),
                request_id: hello.request_id.clone(),
                protocol_version: PROTOCOL_VERSION,
                daemon_version: APP_VERSION.into(),
                status: HelloStatus::ReplacementDeferred,
                reason: Some("active turns or undelivered accepts".into()),
                retry_until: None,
                daemon_instance_id: Some(meta.daemon_instance_id.clone()),
            }
        }
    } else {
        HelloAck::ok(
            &hello.request_id,
            APP_VERSION,
            meta.daemon_instance_id.clone(),
        )
    };

    write_msg(writer, &ack).await?;
    if ack.status == HelloStatus::Incompatible {
        return Ok(());
    }

    // Business messages (Phase 1 foundations: health + settings.get).
    loop {
        let req: Option<Request> = match read_msg(reader).await {
            Ok(v) => v,
            Err(e) => {
                lifecycle::log_line(&format!("read error: {e}"));
                break;
            }
        };
        let Some(req) = req else {
            break;
        };
        if req.r#type != "request" {
            continue;
        }
        let response = dispatch_request(&req, &state, hello.role);
        write_msg(writer, &response).await?;
    }
    Ok(())
}

fn dispatch_request(req: &Request, state: &DaemonState, _role: ClientRole) -> Response {
    match req.method.as_str() {
        "health.get" => {
            let meta = state.meta.lock().clone();
            let cfg = state.config.snapshot();
            Response::ok(
                &req.request_id,
                serde_json::json!({
                    "status": "ok",
                    "daemonVersion": meta.version,
                    "daemonInstanceId": meta.daemon_instance_id,
                    "protocolVersion": meta.protocol_version,
                    "trayMode": cfg.general.tray_mode,
                    "draining": state.barrier.is_draining(),
                }),
            )
        }
        "settings.get" => {
            let cfg = state.config.snapshot();
            match serde_json::to_value(cfg) {
                Ok(v) => Response::ok(&req.request_id, v),
                Err(e) => Response::err(&req.request_id, "internal", e.to_string(), false),
            }
        }
        other => Response::err(
            &req.request_id,
            "method_not_found",
            format!("unknown method `{other}` (Phase 0–1 foundation)"),
            false,
        ),
    }
}

pub fn start_detached() -> Result<()> {
    // If already running, no-op success.
    if let Ok(Some(meta)) = read_meta() {
        if process_alive(meta.pid) {
            return Ok(());
        }
    }
    spawn::spawn_detached()?;
    // Brief wait for lock/meta.
    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(100));
        if let Ok(Some(meta)) = read_meta() {
            if process_alive(meta.pid) {
                return Ok(());
            }
        }
    }
    Ok(())
}

pub fn stop() -> Result<()> {
    let meta = read_meta()?.ok_or_else(|| anyhow!("daemon not running"))?;
    if !process_alive(meta.pid) {
        transport::remove_stale_daemon_endpoint();
        return Ok(());
    }
    terminate_pid(meta.pid)?;
    for _ in 0..50 {
        if !process_alive(meta.pid) {
            transport::remove_stale_daemon_endpoint();
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    force_kill_pid(meta.pid)?;
    transport::remove_stale_daemon_endpoint();
    Ok(())
}

pub fn restart(force: bool) -> Result<()> {
    if let Ok(Some(meta)) = read_meta() {
        if process_alive(meta.pid) {
            if !force {
                // Without force, refuse if we cannot know barrier — foundation: attempt stop.
                lifecycle::log_line("restart without --force: requesting stop then start");
            }
            let _ = stop();
        }
    }
    start_detached()
}

pub fn status_text() -> Result<String> {
    match read_meta()? {
        Some(meta) if process_alive(meta.pid) => Ok(format!(
            "running pid={} version={} instance={} endpoint={} status={:?}",
            meta.pid, meta.version, meta.daemon_instance_id, meta.socket, meta.status
        )),
        Some(meta) => Ok(format!(
            "stale meta pid={} (process not alive) endpoint={}",
            meta.pid, meta.socket
        )),
        None => Ok("not running".into()),
    }
}

fn process_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(unix)]
    {
        let rc = unsafe { libc::kill(pid as i32, 0) };
        rc == 0
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

fn terminate_pid(pid: u32) -> Result<()> {
    #[cfg(unix)]
    {
        let rc = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
        if rc != 0 {
            return Err(anyhow!(
                "SIGTERM failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }
    #[cfg(windows)]
    {
        force_kill_pid(pid)
    }
}

fn force_kill_pid(pid: u32) -> Result<()> {
    #[cfg(unix)]
    {
        let rc = unsafe { libc::kill(pid as i32, libc::SIGKILL) };
        if rc != 0 {
            return Err(anyhow!(
                "SIGKILL failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, TerminateProcess, PROCESS_TERMINATE,
        };
        unsafe {
            let h = OpenProcess(PROCESS_TERMINATE, 0, pid);
            if h == 0 {
                return Err(anyhow!("OpenProcess failed"));
            }
            let ok = TerminateProcess(h, 1);
            CloseHandle(h);
            if ok == 0 {
                return Err(anyhow!("TerminateProcess failed"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::GROKTASK_HOME_ENV;
    use tempfile::TempDir;

    #[test]
    fn concurrent_daemon_lock() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("daemon.lock");
        let a = lifecycle::acquire_lock_at(&path).unwrap();
        assert!(matches!(a, LockResult::Acquired(_)));
        let b = lifecycle::acquire_lock_at(&path).unwrap();
        assert!(matches!(b, LockResult::AlreadyRunning));
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn hello_health_roundtrip_unix() {
        #[cfg(unix)]
        {
            use crate::ipc::protocol::Hello;
            use crate::paths;

            let _g = paths::test_env_lock();
            let tmp = TempDir::new().unwrap();
            let prev = std::env::var_os(GROKTASK_HOME_ENV);
            std::env::set_var(GROKTASK_HOME_ENV, tmp.path());

            // Run a short-lived accept once.
            let lock = match acquire_lock().unwrap() {
                LockResult::Acquired(g) => g,
                LockResult::AlreadyRunning => panic!("lock"),
            };
            transport::remove_stale_daemon_endpoint();
            let listener = transport::bind_daemon().unwrap();
            let endpoint = transport::daemon_endpoint_display();
            let meta = DaemonMeta::new(&endpoint, BinaryFingerprint::ZERO);
            write_meta(&meta).unwrap();
            let state = Arc::new(DaemonState {
                meta: Mutex::new(meta.clone()),
                config: ConfigHandle::new(crate::config::ConfigDocument::default()),
                barrier: Arc::new(ReplacementBarrier::new()),
                guards: Arc::new(DeletionGuards::new()),
                db_path: tmp.path().join("history.sqlite3"),
            });
            let _ = storage::open_path(&state.db_path).unwrap();

            let server = tokio::spawn({
                let state = state.clone();
                async move {
                    let stream = listener.accept().await.unwrap();
                    handle_connection(stream, state, watch::channel(false).0)
                        .await
                        .unwrap();
                }
            });

            let stream = transport::connect_daemon().await.unwrap();
            let unix = stream.into_unix().unwrap();
            let (r, mut w) = unix.into_split();
            let mut r = BufReader::new(r);
            let hello = Hello::new(
                "h1",
                ClientRole::Cli,
                APP_VERSION,
                "/tmp/GrokTask",
                BinaryFingerprint::ZERO,
                std::process::id(),
            );
            write_msg(&mut w, &hello).await.unwrap();
            let ack: HelloAck = read_msg(&mut r).await.unwrap().unwrap();
            assert_eq!(ack.status, HelloStatus::Ok);

            let req = Request::new("r1", "health.get", serde_json::json!({}));
            write_msg(&mut w, &req).await.unwrap();
            let resp: Response = read_msg(&mut r).await.unwrap().unwrap();
            assert!(resp.ok);
            drop(w);
            let _ = server.await;
            drop(lock);

            match prev {
                Some(v) => std::env::set_var(GROKTASK_HOME_ENV, v),
                None => std::env::remove_var(GROKTASK_HOME_ENV),
            }
        }
    }
}
