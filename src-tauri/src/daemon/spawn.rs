//! Detached daemon process launch (Unix setsid; Windows DETACHED_PROCESS).

use crate::daemon::lifecycle;
use crate::paths;
use std::io;
use std::process::{Command, Stdio};

/// Spawn `GrokTask daemon run` detached from the current terminal.
pub fn spawn_detached() -> io::Result<()> {
    let exe = std::env::current_exe()?;
    if let Some(dir) = paths::daemon_log().parent() {
        std::fs::create_dir_all(dir)?;
    }
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths::daemon_log())?;
    let log_err = log.try_clone()?;

    let mut cmd = Command::new(exe);
    cmd.arg("daemon")
        .arg("run")
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err));

    // Propagate isolation home for tests / multi-instance.
    if let Ok(home) = std::env::var(paths::GROKTASK_HOME_ENV) {
        cmd.env(paths::GROKTASK_HOME_ENV, home);
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                // New session — survive terminal hangup.
                if libc::setsid() == -1 {
                    return Err(io::Error::last_os_error());
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
    lifecycle::log_line("spawned detached daemon");
    Ok(())
}
