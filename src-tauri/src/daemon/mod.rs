//! Per-user daemon: single instance, IPC listener, storage, lifecycle.

pub mod lifecycle;
pub mod spawn;
pub mod task_manager;

use crate::config::ConfigHandle;
use crate::dto::{
    validate_submission_id, validate_task_input, validate_uuid_like, DEFAULT_WAIT_TIMEOUT_MS,
    MAX_WAIT_TIMEOUT_MS,
};
use crate::fingerprint::BinaryFingerprint;
use crate::ipc::codec::{read_msg, write_msg};
use crate::ipc::protocol::{ClientRole, Hello, HelloAck, HelloStatus, Request, Response};
use crate::ipc::transport::{self, IpcStream};
use crate::storage::{self, DeletionGuards};
use crate::version::{APP_VERSION, PROTOCOL_VERSION};
use anyhow::{anyhow, Context, Result};
use lifecycle::{
    acquire_lock, write_meta, DaemonMeta, DaemonRunStatus, LockResult, ReplacementBarrier,
    REPLACEMENT_DRAIN_SECS,
};
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use task_manager::{TaskError, TaskManager};
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
    let tasks = Arc::new(TaskManager::new(
        db_path.clone(),
        config.snapshot().general.grok_executable.clone(),
    ));
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
    let state = Arc::new(DaemonState {
        meta: Mutex::new(meta.clone()),
        config,
        barrier: barrier.clone(),
        guards,
        db_path,
        tasks,
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
    tasks: Arc<TaskManager>,
}

async fn handle_connection(
    stream: IpcStream,
    state: Arc<DaemonState>,
    shutdown: watch::Sender<bool>,
) -> Result<()> {
    #[cfg(unix)]
    {
        let unix = stream.into_unix()?;
        let (reader, mut writer) = unix.into_split();
        let mut reader = BufReader::new(reader);
        serve_client(&mut reader, &mut writer, state, shutdown).await
    }
    #[cfg(windows)]
    {
        match stream {
            IpcStream::WindowsServer(s) => {
                let (mut reader, mut writer) = tokio::io::split(s);
                let mut reader = BufReader::new(reader);
                serve_client(&mut reader, &mut writer, state, shutdown).await
            }
            IpcStream::Windows(s) => {
                let (mut reader, mut writer) = tokio::io::split(s);
                let mut reader = BufReader::new(reader);
                serve_client(&mut reader, &mut writer, state, shutdown).await
            }
            #[cfg(unix)]
            IpcStream::Unix(_) => unreachable!(),
        }
    }
}

async fn serve_client<R, W>(
    reader: &mut R,
    writer: &mut W,
    state: Arc<DaemonState>,
    shutdown: watch::Sender<bool>,
) -> Result<()>
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
    match ack.status {
        HelloStatus::Ok => {}
        HelloStatus::Restarting => {
            // Safe replacement means this daemon must actually give way.
            // Without this, every newer client sees Restarting forever while
            // the old process keeps the daemon lock.
            let _ = shutdown.send(true);
            return Ok(());
        }
        HelloStatus::ReplacementDeferred | HelloStatus::Incompatible => {
            return Ok(());
        }
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
        let response = dispatch_request(&req, &state, hello.role).await;
        write_msg(writer, &response).await?;
    }
    Ok(())
}

async fn dispatch_request(req: &Request, state: &DaemonState, role: ClientRole) -> Response {
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
        "task.start" => handle_task_start(req, state, role, false).await,
        "task.run" => handle_task_start(req, state, role, true).await,
        "task.continue" => handle_task_continue(req, state, role).await,
        "task.status" => {
            let task_id = match req.params.get("taskId").and_then(|v| v.as_str()) {
                Some(id) => match validate_uuid_like(id, "taskId") {
                    Ok(id) => id,
                    Err(e) => {
                        return Response::err(&req.request_id, &e.code, e.message, false);
                    }
                },
                None => {
                    return Response::err(
                        &req.request_id,
                        "invalid_params",
                        "taskId is required",
                        false,
                    );
                }
            };
            match state.tasks.status(&task_id) {
                Ok(s) => Response::ok(&req.request_id, serde_json::to_value(s).unwrap_or_default()),
                Err(e) => task_err_response(&req.request_id, e),
            }
        }
        "task.wait" => {
            let task_id = match req.params.get("taskId").and_then(|v| v.as_str()) {
                Some(id) => match validate_uuid_like(id, "taskId") {
                    Ok(id) => id,
                    Err(e) => {
                        return Response::err(&req.request_id, &e.code, e.message, false);
                    }
                },
                None => {
                    return Response::err(
                        &req.request_id,
                        "invalid_params",
                        "taskId is required",
                        false,
                    );
                }
            };
            let turn_id = match req.params.get("turnId").and_then(|v| v.as_str()) {
                Some(id) => match validate_uuid_like(id, "turnId") {
                    Ok(id) => id,
                    Err(e) => {
                        return Response::err(&req.request_id, &e.code, e.message, false);
                    }
                },
                None => {
                    return Response::err(
                        &req.request_id,
                        "invalid_params",
                        "turnId is required",
                        false,
                    );
                }
            };
            let timeout_ms = req
                .params
                .get("timeoutMs")
                .and_then(|v| v.as_u64())
                .unwrap_or(DEFAULT_WAIT_TIMEOUT_MS)
                .min(MAX_WAIT_TIMEOUT_MS);
            match state.tasks.wait(&task_id, &turn_id, timeout_ms).await {
                Ok(r) => Response::ok(&req.request_id, serde_json::to_value(r).unwrap_or_default()),
                Err(TaskError::WaitTimeout(w)) => {
                    Response::ok(&req.request_id, serde_json::to_value(w).unwrap_or_default())
                }
                Err(e) => task_err_response(&req.request_id, e),
            }
        }
        "task.cancel" => {
            let task_id = match req.params.get("taskId").and_then(|v| v.as_str()) {
                Some(id) => match validate_uuid_like(id, "taskId") {
                    Ok(id) => id,
                    Err(e) => {
                        return Response::err(&req.request_id, &e.code, e.message, false);
                    }
                },
                None => {
                    return Response::err(
                        &req.request_id,
                        "invalid_params",
                        "taskId is required",
                        false,
                    );
                }
            };
            if let Some(turn_id) = req.params.get("turnId").and_then(|v| v.as_str()) {
                let turn_id = match validate_uuid_like(turn_id, "turnId") {
                    Ok(id) => id,
                    Err(e) => {
                        return Response::err(&req.request_id, &e.code, e.message, false);
                    }
                };
                let cause = match role {
                    ClientRole::Mcp => "mcp_cancel",
                    _ => "user_cancel",
                };
                // cancel may block briefly — run blocking in spawn_blocking
                let tasks = state.tasks.clone();
                let res = tokio::task::spawn_blocking(move || {
                    tasks.cancel_turn(&task_id, &turn_id, cause)
                })
                .await;
                match res {
                    Ok(Ok(r)) => {
                        Response::ok(&req.request_id, serde_json::to_value(r).unwrap_or_default())
                    }
                    Ok(Err(e)) => task_err_response(&req.request_id, e),
                    Err(e) => Response::err(&req.request_id, "internal", e.to_string(), false),
                }
            } else if let Some(recovery_id) = req.params.get("recoveryId").and_then(|v| v.as_str())
            {
                let _ = recovery_id;
                Response::err(
                    &req.request_id,
                    "not_found",
                    "recovery cancel not available (no active recovery)",
                    false,
                )
            } else {
                Response::err(
                    &req.request_id,
                    "invalid_params",
                    "turnId or recoveryId is required",
                    false,
                )
            }
        }
        "tasks.list" => {
            let limit = req
                .params
                .get("limit")
                .and_then(|v| v.as_i64())
                .unwrap_or(50);
            match state.tasks.list(limit) {
                Ok(list) => Response::ok(
                    &req.request_id,
                    serde_json::to_value(list).unwrap_or_default(),
                ),
                Err(e) => task_err_response(&req.request_id, e),
            }
        }
        "tasks.show" | "task.detail" => {
            let task_id = match req.params.get("taskId").and_then(|v| v.as_str()) {
                Some(id) => match validate_uuid_like(id, "taskId") {
                    Ok(id) => id,
                    Err(e) => {
                        return Response::err(&req.request_id, &e.code, e.message, false);
                    }
                },
                None => {
                    return Response::err(
                        &req.request_id,
                        "invalid_params",
                        "taskId is required",
                        false,
                    );
                }
            };
            match state.tasks.detail(&task_id) {
                Ok(d) => Response::ok(&req.request_id, serde_json::to_value(d).unwrap_or_default()),
                Err(e) => task_err_response(&req.request_id, e),
            }
        }
        other => Response::err(
            &req.request_id,
            "method_not_found",
            format!("unknown method `{other}`"),
            false,
        ),
    }
}

async fn handle_task_continue(req: &Request, state: &DaemonState, role: ClientRole) -> Response {
    let task_id = match req.params.get("taskId").and_then(|v| v.as_str()) {
        Some(id) => match validate_uuid_like(id, "taskId") {
            Ok(id) => id,
            Err(e) => return Response::err(&req.request_id, &e.code, e.message, false),
        },
        None => {
            return Response::err(
                &req.request_id,
                "invalid_params",
                "taskId is required",
                false,
            );
        }
    };
    let prompt = match req.params.get("prompt").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => {
            return Response::err(
                &req.request_id,
                "invalid_params",
                "prompt is required",
                false,
            );
        }
    };
    let connection_id = Some(format!("{role:?}"));
    match state.tasks.continue_task(
        &task_id,
        prompt,
        "client",
        connection_id,
        Some(req.request_id.clone()),
    ) {
        Ok(r) => Response::ok(&req.request_id, serde_json::to_value(r).unwrap_or_default()),
        Err(e) => task_err_response(&req.request_id, e),
    }
}

async fn handle_task_start(
    req: &Request,
    state: &DaemonState,
    role: ClientRole,
    blocking_run: bool,
) -> Response {
    let task = match req.params.get("task").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => {
            return Response::err(&req.request_id, "invalid_params", "task is required", false);
        }
    };
    let cwd = match req.params.get("cwd").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => {
            return Response::err(&req.request_id, "invalid_params", "cwd is required", false);
        }
    };
    let mode = match req.params.get("mode").and_then(|v| v.as_str()) {
        Some(m) => m,
        None => {
            return Response::err(
                &req.request_id,
                "invalid_params",
                "mode must be explicit `read` or `write`",
                false,
            );
        }
    };
    let model = req.params.get("model").and_then(|v| v.as_str());
    let effort = req.params.get("effort").and_then(|v| v.as_str());
    let title = req.params.get("title").and_then(|v| v.as_str());
    let input = match validate_task_input(task, cwd, mode, model, effort, title) {
        Ok(i) => i,
        Err(e) => return Response::err(&req.request_id, &e.code, e.message, false),
    };

    let owner_kind = if blocking_run { "client" } else { "daemon" };
    let connection_id = Some(format!("{role:?}"));

    if blocking_run {
        let tasks = state.tasks.clone();
        let req_id = req.request_id.clone();
        match tasks
            .run_blocking(input, connection_id, Some(req_id.clone()))
            .await
        {
            Ok(r) => Response::ok(&req.request_id, serde_json::to_value(r).unwrap_or_default()),
            Err(e) => task_err_response(&req.request_id, e),
        }
    } else {
        let submission_id = match req.params.get("submissionId").and_then(|v| v.as_str()) {
            Some(s) => match validate_submission_id(s) {
                Ok(s) => s,
                Err(e) => return Response::err(&req.request_id, &e.code, e.message, false),
            },
            None => repository_new_id(),
        };
        match state.tasks.start(
            input,
            submission_id,
            owner_kind,
            connection_id,
            Some(req.request_id.clone()),
        ) {
            Ok(r) => Response::ok(&req.request_id, serde_json::to_value(r).unwrap_or_default()),
            Err(e) => task_err_response(&req.request_id, e),
        }
    }
}

fn repository_new_id() -> String {
    crate::storage::repository::new_id()
}

fn task_err_response(request_id: &str, e: TaskError) -> Response {
    if let TaskError::WaitTimeout(w) = e {
        return Response::ok(request_id, serde_json::to_value(w).unwrap_or_default());
    }
    Response::err(request_id, e.code(), e.message(), e.retryable())
}

/// Reclaim stale runtime state and decide whether a detached spawn is required.
///
/// Returns `true` when no healthy daemon is present after reclaim (caller should
/// spawn). Returns `false` when a live PID + usable endpoint already exist
/// (start may no-op). Never launches a process.
///
/// Extracted so unit tests can cover the stale-meta regression without spawning
/// a long-lived detached daemon.
pub fn prepare_detached_start() -> Result<bool> {
    // Drop zombie/dead/missing-socket metadata so we never no-op on stale Running.
    let _ = lifecycle::reclaim_stale_daemon_state();
    Ok(!matches!(
        lifecycle::inspect_daemon()?,
        lifecycle::DaemonPresence::Running(_)
    ))
}

/// Clear non-healthy daemon state (and stop a healthy daemon when present).
/// Does **not** spawn a replacement process — callers invoke
/// [`start_detached`] / [`prepare_detached_start`] afterward.
pub fn prepare_restart(force: bool) -> Result<()> {
    // Always drop stale meta first so force-restart cannot leave "Running" without a socket.
    let _ = lifecycle::reclaim_stale_daemon_state();
    match lifecycle::inspect_daemon()? {
        lifecycle::DaemonPresence::Running(_) => {
            if !force {
                lifecycle::log_line("restart without --force: requesting stop then start");
            }
            let _ = stop();
        }
        lifecycle::DaemonPresence::Stale { .. } => {
            lifecycle::clear_daemon_runtime_files();
        }
        lifecycle::DaemonPresence::Absent => {}
    }
    Ok(())
}

pub fn start_detached() -> Result<()> {
    if !prepare_detached_start()? {
        return Ok(());
    }
    spawn::spawn_detached()?;
    // Brief wait for lock/meta + usable endpoint.
    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(100));
        if matches!(
            lifecycle::inspect_daemon()?,
            lifecycle::DaemonPresence::Running(_)
        ) {
            return Ok(());
        }
    }
    Ok(())
}

pub fn stop() -> Result<()> {
    // Reclaim pure stale state first (dead/zombie/missing socket).
    if lifecycle::reclaim_stale_daemon_state()? {
        return Ok(());
    }
    let meta = match lifecycle::inspect_daemon()? {
        lifecycle::DaemonPresence::Running(m) => m,
        lifecycle::DaemonPresence::Absent => {
            return Err(anyhow!("daemon not running"));
        }
        lifecycle::DaemonPresence::Stale { .. } => {
            // Race: became stale after reclaim check.
            lifecycle::clear_daemon_runtime_files();
            return Ok(());
        }
    };

    // Endpoint is usable and process is live — safe enough to signal this PID.
    // (Unverified PIDs are handled by reclaim without kill.)
    terminate_pid(meta.pid)?;
    for _ in 0..50 {
        if !lifecycle::process_is_live(meta.pid) {
            lifecycle::clear_daemon_runtime_files();
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    force_kill_pid(meta.pid)?;
    lifecycle::clear_daemon_runtime_files();
    Ok(())
}

pub fn restart(force: bool) -> Result<()> {
    prepare_restart(force)?;
    start_detached()
}

pub fn status_text() -> Result<String> {
    // Surface and opportunistically clear stale metadata so CLI status matches reality.
    match lifecycle::inspect_daemon()? {
        lifecycle::DaemonPresence::Running(meta) => Ok(format!(
            "running pid={} version={} instance={} endpoint={} status={:?}",
            meta.pid, meta.version, meta.daemon_instance_id, meta.socket, meta.status
        )),
        lifecycle::DaemonPresence::Stale { meta, reason } => {
            let detail = match reason {
                lifecycle::StaleReason::ProcessNotLive => "process not alive",
                lifecycle::StaleReason::EndpointUnusable => "endpoint missing or unusable",
            };
            // Clear so subsequent start/run/ensure do not treat this as Running.
            lifecycle::clear_daemon_runtime_files();
            Ok(format!(
                "stale meta pid={} ({detail}) endpoint={}",
                meta.pid, meta.socket
            ))
        }
        lifecycle::DaemonPresence::Absent => Ok("not running".into()),
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

    fn with_temp_home<F: FnOnce()>(f: F) {
        let _g = crate::paths::test_env_lock();
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
    fn status_text_rejects_dead_pid_meta_as_running() {
        with_temp_home(|| {
            let mut meta = DaemonMeta::new(
                crate::paths::daemon_sock().display().to_string(),
                BinaryFingerprint::ZERO,
            );
            meta.pid = u32::MAX - 1;
            write_meta(&meta).unwrap();

            let text = status_text().unwrap();
            assert!(
                !text.starts_with("running "),
                "dead pid must not look healthy: {text}"
            );
            assert!(text.contains("stale meta"), "got: {text}");
            // status opportunistically clears stale files
            assert!(!crate::paths::daemon_meta().exists());
        });
    }

    #[test]
    fn status_text_rejects_live_pid_without_socket() {
        with_temp_home(|| {
            let mut meta = DaemonMeta::new(
                crate::paths::daemon_sock().display().to_string(),
                BinaryFingerprint::ZERO,
            );
            meta.pid = std::process::id();
            write_meta(&meta).unwrap();

            let text = status_text().unwrap();
            assert!(
                !text.starts_with("running "),
                "missing socket must not look healthy: {text}"
            );
            assert!(
                text.contains("endpoint missing") || text.contains("stale meta"),
                "got: {text}"
            );
            assert!(!crate::paths::daemon_meta().exists());
        });
    }

    #[test]
    fn prepare_detached_start_does_not_noop_on_stale_running_meta() {
        with_temp_home(|| {
            // Simulate the smoke failure: daemon.json claims Running for a dead PID,
            // no socket. Start decision must reclaim and request spawn — never treat
            // stale meta as healthy (and never launch a real detached daemon here).
            let mut meta = DaemonMeta::new(
                crate::paths::daemon_sock().display().to_string(),
                BinaryFingerprint::ZERO,
            );
            meta.pid = u32::MAX - 1;
            meta.status = DaemonRunStatus::Running;
            write_meta(&meta).unwrap();

            let need_spawn = prepare_detached_start().unwrap();
            assert!(
                need_spawn,
                "stale Running meta must not be treated as healthy no-op"
            );
            assert!(
                !crate::paths::daemon_meta().exists(),
                "stale meta must be reclaimed before spawn decision"
            );
            assert!(matches!(
                lifecycle::inspect_daemon().unwrap(),
                lifecycle::DaemonPresence::Absent
            ));
            // Absent home: second prepare still requests spawn.
            assert!(prepare_detached_start().unwrap());
        });
    }

    #[test]
    fn prepare_restart_clears_stale_meta_without_spawn() {
        with_temp_home(|| {
            let mut meta = DaemonMeta::new(
                crate::paths::daemon_sock().display().to_string(),
                BinaryFingerprint::ZERO,
            );
            meta.pid = u32::MAX - 1;
            meta.status = DaemonRunStatus::Running;
            write_meta(&meta).unwrap();
            std::fs::write(crate::paths::daemon_sock(), b"").unwrap();

            prepare_restart(true).unwrap();
            assert!(
                !crate::paths::daemon_meta().exists(),
                "restart prep must drop stale dead-pid meta"
            );
            assert!(
                !crate::paths::daemon_sock().exists(),
                "restart prep must drop leftover socket without signaling PID"
            );
            assert!(matches!(
                lifecycle::inspect_daemon().unwrap(),
                lifecycle::DaemonPresence::Absent
            ));
            // Spawn is a separate step; after prep, start still needs a daemon.
            assert!(prepare_detached_start().unwrap());
        });
    }

    #[cfg(unix)]
    #[test]
    fn prepare_detached_start_noops_when_healthy() {
        with_temp_home(|| {
            use std::os::unix::net::UnixListener;

            let _listener = UnixListener::bind(crate::paths::daemon_sock()).unwrap();
            let mut meta = DaemonMeta::new(
                crate::paths::daemon_sock().display().to_string(),
                BinaryFingerprint::ZERO,
            );
            meta.pid = std::process::id();
            write_meta(&meta).unwrap();

            assert!(
                !prepare_detached_start().unwrap(),
                "healthy live PID + connectable socket must no-op start"
            );
            assert!(crate::paths::daemon_meta().exists());
            lifecycle::clear_daemon_runtime_files();
        });
    }

    #[test]
    fn stop_clears_stale_meta_without_error() {
        with_temp_home(|| {
            let mut meta = DaemonMeta::new(
                crate::paths::daemon_sock().display().to_string(),
                BinaryFingerprint::ZERO,
            );
            meta.pid = u32::MAX - 1;
            write_meta(&meta).unwrap();
            stop().unwrap();
            assert!(!crate::paths::daemon_meta().exists());
        });
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
            let db_path = tmp.path().join("history.sqlite3");
            let _ = storage::open_path(&db_path).unwrap();
            let state = Arc::new(DaemonState {
                meta: Mutex::new(meta.clone()),
                config: ConfigHandle::new(crate::config::ConfigDocument::default()),
                barrier: Arc::new(ReplacementBarrier::new()),
                guards: Arc::new(DeletionGuards::new()),
                db_path: db_path.clone(),
                tasks: Arc::new(TaskManager::new(db_path, None)),
            });

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

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn fingerprint_mismatch_restart_ack_requests_shutdown_unix() {
        #[cfg(unix)]
        {
            use crate::ipc::protocol::Hello;
            use crate::paths;

            let _g = paths::test_env_lock();
            let tmp = TempDir::new().unwrap();
            let prev = std::env::var_os(GROKTASK_HOME_ENV);
            std::env::set_var(GROKTASK_HOME_ENV, tmp.path());

            let lock = match acquire_lock().unwrap() {
                LockResult::Acquired(g) => g,
                LockResult::AlreadyRunning => panic!("lock"),
            };
            transport::remove_stale_daemon_endpoint();
            let listener = transport::bind_daemon().unwrap();
            let endpoint = transport::daemon_endpoint_display();
            let meta = DaemonMeta::new(
                &endpoint,
                BinaryFingerprint {
                    size: 1,
                    mtime_ns: 1,
                },
            );
            write_meta(&meta).unwrap();
            let db_path = tmp.path().join("history.sqlite3");
            let _ = storage::open_path(&db_path).unwrap();
            let state = Arc::new(DaemonState {
                meta: Mutex::new(meta),
                config: ConfigHandle::new(crate::config::ConfigDocument::default()),
                barrier: Arc::new(ReplacementBarrier::new()),
                guards: Arc::new(DeletionGuards::new()),
                db_path: db_path.clone(),
                tasks: Arc::new(TaskManager::new(db_path, None)),
            });
            let (shutdown_tx, mut shutdown_rx) = watch::channel(false);

            let server = tokio::spawn({
                let state = state.clone();
                async move {
                    let stream = listener.accept().await.unwrap();
                    handle_connection(stream, state, shutdown_tx).await.unwrap();
                }
            });

            let stream = transport::connect_daemon().await.unwrap();
            let unix = stream.into_unix().unwrap();
            let (r, mut w) = unix.into_split();
            let mut r = BufReader::new(r);
            let hello = Hello::new(
                "h-restart",
                ClientRole::Cli,
                APP_VERSION,
                "/tmp/GrokTask-new",
                BinaryFingerprint {
                    size: 2,
                    mtime_ns: 2,
                },
                std::process::id(),
            );
            write_msg(&mut w, &hello).await.unwrap();
            let ack: HelloAck = read_msg(&mut r).await.unwrap().unwrap();
            assert_eq!(ack.status, HelloStatus::Restarting);

            tokio::time::timeout(Duration::from_secs(1), shutdown_rx.changed())
                .await
                .unwrap()
                .unwrap();
            assert!(*shutdown_rx.borrow());

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
