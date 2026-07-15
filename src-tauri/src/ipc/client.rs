//! Client helpers for CLI/MCP to talk to the daemon over NDJSON IPC.

use crate::daemon::{self};
use crate::fingerprint::BinaryFingerprint;
use crate::ipc::codec::{read_msg, write_msg};
use crate::ipc::protocol::{ClientRole, Hello, HelloAck, HelloStatus, Request, Response};
use crate::ipc::transport;
use crate::version::{APP_VERSION, PRODUCT_NAME};
use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use std::time::Duration;
use tokio::io::{AsyncWriteExt, BufReader};
use uuid::Uuid;

const HELLO_RETRY_ATTEMPTS: usize = 8;
const HELLO_RETRY_DELAY: Duration = Duration::from_millis(250);

/// Ensure a daemon is reachable, starting one if needed.
///
/// Delegates to [`daemon::start_detached`], which reclaims stale `daemon.json`
/// (dead/zombie PID or missing socket) before trusting metadata, so clients
/// never no-op on a defunct "Running" claim.
pub fn ensure_daemon() -> Result<()> {
    use crate::daemon::lifecycle::{self, DaemonPresence};

    daemon::start_detached().context("start daemon")?;
    for _ in 0..50 {
        if matches!(lifecycle::inspect_daemon()?, DaemonPresence::Running(_)) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

/// Blocking request helper used by CLI/MCP.
pub fn request_blocking(role: ClientRole, method: &str, params: Value) -> Result<Response> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("tokio runtime")?;
    let mut last_restart_reason: Option<String> = None;
    for attempt in 0..HELLO_RETRY_ATTEMPTS {
        ensure_daemon()?;
        match rt.block_on(request_async(role, method, params.clone())) {
            Ok(resp) => return Ok(resp),
            Err(e) if hello_restart_reason(&e).is_some() => {
                last_restart_reason = hello_restart_reason(&e);
                // A pre-fix daemon may report Restarting but keep holding the
                // lock forever. Because the daemon only returns Restarting
                // after its replacement barrier says it is safe, the client can
                // actively request a restart and then retry the original call.
                let _ = daemon::restart(false);
                std::thread::sleep(HELLO_RETRY_DELAY);
                if attempt + 1 == HELLO_RETRY_ATTEMPTS {
                    break;
                }
            }
            Err(e) => return Err(e),
        }
    }
    Err(anyhow!(
        "daemon is still restarting{}; retry shortly",
        last_restart_reason
            .map(|r| format!(" ({r})"))
            .unwrap_or_default()
    ))
}

pub async fn request_async(role: ClientRole, method: &str, params: Value) -> Result<Response> {
    let stream = transport::connect_daemon()
        .await
        .context("connect daemon (is it running?)")?;
    let request_id = Uuid::new_v4().to_string();
    let binary_path = std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| PRODUCT_NAME.into());
    let hello = Hello::new(
        format!("h-{request_id}"),
        role,
        APP_VERSION,
        binary_path,
        BinaryFingerprint::current(),
        std::process::id(),
    );

    #[cfg(unix)]
    {
        let unix = stream.into_unix()?;
        let (r, mut w) = unix.into_split();
        let mut r = BufReader::new(r);
        write_msg(&mut w, &hello).await?;
        let ack: HelloAck = read_msg(&mut r)
            .await?
            .ok_or_else(|| anyhow!("EOF during hello"))?;
        if ack.status != HelloStatus::Ok {
            return Err(anyhow!(
                "daemon hello status {:?}: {}",
                ack.status,
                ack.reason.unwrap_or_default()
            ));
        }
        let req = Request::new(&request_id, method, params);
        write_msg(&mut w, &req).await?;
        let resp: Response = read_msg(&mut r)
            .await?
            .ok_or_else(|| anyhow!("EOF waiting for response"))?;
        // Drop write half to close cleanly
        let _ = w.shutdown().await;
        Ok(resp)
    }
    #[cfg(windows)]
    {
        match stream {
            IpcStream::Windows(s) | IpcStream::WindowsServer(s) => {
                let (mut r, mut w) = tokio::io::split(s);
                let mut r = BufReader::new(r);
                write_msg(&mut w, &hello).await?;
                let ack: HelloAck = read_msg(&mut r)
                    .await?
                    .ok_or_else(|| anyhow!("EOF during hello"))?;
                if ack.status != HelloStatus::Ok {
                    return Err(anyhow!(
                        "daemon hello status {:?}: {}",
                        ack.status,
                        ack.reason.unwrap_or_default()
                    ));
                }
                let req = Request::new(&request_id, method, params);
                write_msg(&mut w, &req).await?;
                let resp: Response = read_msg(&mut r)
                    .await?
                    .ok_or_else(|| anyhow!("EOF waiting for response"))?;
                let _ = w.shutdown().await;
                Ok(resp)
            }
            #[cfg(unix)]
            IpcStream::Unix(_) => unreachable!(),
        }
    }
}

fn hello_restart_reason(e: &anyhow::Error) -> Option<String> {
    let msg = format!("{e:#}");
    msg.strip_prefix("daemon hello status Restarting: ")
        .map(|s| s.to_string())
}

/// Extract result or map IPC error.
pub fn unwrap_result(resp: Response) -> Result<Value> {
    if resp.ok {
        Ok(resp.result.unwrap_or(Value::Null))
    } else {
        let err = resp.error.unwrap_or(crate::ipc::protocol::RpcError {
            code: "unknown".into(),
            message: "request failed".into(),
            retryable: false,
        });
        Err(anyhow!("{}: {}", err.code, err.message))
    }
}
