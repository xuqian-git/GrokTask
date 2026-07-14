//! Hidden `--gui-host` role: single-instance Tauri event loop + navigation IPC.

use crate::daemon::lifecycle::{acquire_lock_at, LockResult};
use crate::ipc::codec::{read_msg, write_msg};
use crate::ipc::protocol::GuiNavCommand;
use crate::ipc::transport::{self, IpcStream};
use crate::paths;
use crate::version::window_label;
use tokio::io::BufReader;
use tokio::sync::mpsc;

/// Main window visibility at process start, before any navigation command.
///
/// Phase 0–1: the host process may run for IPC/single-instance reasons without
/// showing UI. Navigation (`OpenPopover`, `Focus`, etc.) calls `show()`.
pub fn initial_main_window_visible() -> bool {
    false
}

/// Run the GUI host. Acquires `gui-host.lock`; if already held, exits immediately.
pub fn run() -> ! {
    // Single-instance lock before any Tauri init.
    match acquire_lock_at(&paths::gui_host_lock()) {
        Ok(LockResult::Acquired(_guard)) => {
            // Leak the guard for process lifetime.
            std::mem::forget(_guard);
        }
        Ok(LockResult::AlreadyRunning) => {
            eprintln!("gui-host already running");
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

    // Navigation channel: IPC thread → Tauri main thread.
    let (nav_tx, mut nav_rx) = mpsc::unbounded_channel::<GuiNavCommand>();

    // Bind GUI host IPC in a background runtime before starting Tauri.
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
        .setup(move |app| {
            // Create main window hidden until navigation asks for it.
            let _webview = tauri::WebviewWindowBuilder::new(
                app,
                window_label::MAIN,
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("GrokTask")
            .inner_size(1120.0, 760.0)
            .min_inner_size(900.0, 640.0)
            .visible(initial_main_window_visible())
            .build()?;

            // Poll navigation commands on the main thread via async runtime.
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

fn apply_nav(app: &tauri::AppHandle, cmd: GuiNavCommand) {
    use tauri::Manager;
    match cmd {
        GuiNavCommand::OpenPopover | GuiNavCommand::Focus | GuiNavCommand::OpenHistory => {
            if let Some(w) = app.get_webview_window(window_label::MAIN) {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }
        GuiNavCommand::OpenTask { .. } | GuiNavCommand::OpenSettings => {
            if let Some(w) = app.get_webview_window(window_label::MAIN) {
                let _ = w.show();
                let _ = w.set_focus();
            }
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
            // Ack
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::lifecycle;
    use crate::paths::GROKTASK_HOME_ENV;
    use tempfile::TempDir;

    #[test]
    fn main_window_starts_hidden_until_navigation() {
        // Tauri window state is not unit-testable without a display; pin the
        // policy helper that `run()` passes to WebviewWindowBuilder::visible.
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
}
