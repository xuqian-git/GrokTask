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

/// Ensure a daemon is reachable, starting one if needed.
pub fn ensure_daemon() -> Result<()> {
    match daemon::status_text() {
        Ok(s) if s.starts_with("running ") => Ok(()),
        _ => {
            daemon::start_detached().context("start daemon")?;
            // Wait for endpoint
            for _ in 0..50 {
                if matches!(daemon::status_text(), Ok(s) if s.starts_with("running ")) {
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Ok(())
        }
    }
}

/// Blocking request helper used by CLI/MCP.
pub fn request_blocking(role: ClientRole, method: &str, params: Value) -> Result<Response> {
    ensure_daemon()?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("tokio runtime")?;
    rt.block_on(request_async(role, method, params))
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
