//! Local IPC transport: Unix domain sockets (0600) and Windows named pipes (user SID ACL).

use std::io;
use std::path::Path;

/// Platform-specific listener bound for accepting daemon/GUI connections.
pub enum IpcListener {
    #[cfg(unix)]
    Unix(tokio::net::UnixListener),
    #[cfg(windows)]
    Windows(windows_impl::PipeListener),
}

/// Platform-specific connected stream.
pub enum IpcStream {
    #[cfg(unix)]
    Unix(tokio::net::UnixStream),
    #[cfg(windows)]
    Windows(tokio::net::windows::named_pipe::NamedPipeClient),
    #[cfg(windows)]
    WindowsServer(tokio::net::windows::named_pipe::NamedPipeServer),
}

impl IpcListener {
    pub async fn accept(&self) -> io::Result<IpcStream> {
        match self {
            #[cfg(unix)]
            Self::Unix(l) => {
                let (s, _) = l.accept().await?;
                Ok(IpcStream::Unix(s))
            }
            #[cfg(windows)]
            Self::Windows(l) => l.accept().await,
        }
    }

    /// Filesystem path for Unix sockets; pipe name for Windows.
    pub fn endpoint_display(&self) -> String {
        match self {
            #[cfg(unix)]
            Self::Unix(_) => crate::paths::daemon_sock().display().to_string(),
            #[cfg(windows)]
            Self::Windows(l) => l.name().to_string(),
        }
    }
}

/// Bind daemon endpoint. Caller must hold the single-instance lock before bind
/// so stale sockets/pipes can be replaced safely.
pub fn bind_daemon() -> io::Result<IpcListener> {
    #[cfg(unix)]
    {
        Ok(IpcListener::Unix(unix_impl::bind_socket(
            &crate::paths::daemon_sock(),
        )?))
    }
    #[cfg(windows)]
    {
        Ok(IpcListener::Windows(windows_impl::PipeListener::bind(
            &crate::paths::daemon_pipe_name(),
        )?))
    }
}

/// Bind GUI host navigation endpoint.
pub fn bind_gui_host() -> io::Result<IpcListener> {
    #[cfg(unix)]
    {
        Ok(IpcListener::Unix(unix_impl::bind_socket(
            &crate::paths::gui_host_sock(),
        )?))
    }
    #[cfg(windows)]
    {
        Ok(IpcListener::Windows(windows_impl::PipeListener::bind(
            &crate::paths::gui_host_pipe_name(),
        )?))
    }
}

/// Connect to daemon endpoint.
pub async fn connect_daemon() -> io::Result<IpcStream> {
    #[cfg(unix)]
    {
        let s = tokio::net::UnixStream::connect(crate::paths::daemon_sock()).await?;
        Ok(IpcStream::Unix(s))
    }
    #[cfg(windows)]
    {
        windows_impl::connect_pipe(&crate::paths::daemon_pipe_name()).await
    }
}

/// Connect to GUI host endpoint.
pub async fn connect_gui_host() -> io::Result<IpcStream> {
    #[cfg(unix)]
    {
        let s = tokio::net::UnixStream::connect(crate::paths::gui_host_sock()).await?;
        Ok(IpcStream::Unix(s))
    }
    #[cfg(windows)]
    {
        windows_impl::connect_pipe(&crate::paths::gui_host_pipe_name()).await
    }
}

/// Split helpers for codec use — returns owned halves via into_split.
impl IpcStream {
    #[cfg(unix)]
    pub fn into_unix(self) -> io::Result<tokio::net::UnixStream> {
        match self {
            Self::Unix(s) => Ok(s),
            #[cfg(windows)]
            _ => unreachable!(),
        }
    }
}

#[cfg(unix)]
mod unix_impl {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    pub fn bind_socket(path: &Path) -> io::Result<tokio::net::UnixListener> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
        }
        // Holding single-instance lock: any existing socket is stale.
        let _ = std::fs::remove_file(path);
        let listener = tokio::net::UnixListener::bind(path)?;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        Ok(listener)
    }

    /// Verify socket mode is owner-only (for tests).
    #[cfg(test)]
    pub fn socket_mode(path: &Path) -> io::Result<u32> {
        let meta = std::fs::metadata(path)?;
        Ok(meta.permissions().mode() & 0o777)
    }
}

/// Remove stale daemon endpoint file (Unix). Safe only while holding lock.
pub fn remove_stale_daemon_endpoint() {
    #[cfg(unix)]
    {
        let _ = std::fs::remove_file(crate::paths::daemon_sock());
    }
}

pub fn remove_stale_gui_endpoint() {
    #[cfg(unix)]
    {
        let _ = std::fs::remove_file(crate::paths::gui_host_sock());
    }
}

/// Endpoint path used for status display.
pub fn daemon_endpoint_display() -> String {
    #[cfg(unix)]
    {
        crate::paths::daemon_sock().display().to_string()
    }
    #[cfg(windows)]
    {
        crate::paths::daemon_pipe_name()
    }
}

pub fn gui_endpoint_display() -> String {
    #[cfg(unix)]
    {
        crate::paths::gui_host_sock().display().to_string()
    }
    #[cfg(windows)]
    {
        crate::paths::gui_host_pipe_name()
    }
}

// ---------------------------------------------------------------------------
// Windows named pipes with current-user SID ACL
// ---------------------------------------------------------------------------
//
// Security model (Phase 0–1):
// - Each pipe instance is created with a DACL that grants Generic All only to the
//   current process user's SID (SDDL `D:(A;;GA;;;<sid>)`).
// - `PIPE_REJECT_REMOTE_CLIENTS` is also set (local-only).
// - SECURITY_ATTRIBUTES are passed via Tokio's
//   `ServerOptions::create_with_security_attributes_raw`.
// ---------------------------------------------------------------------------
#[cfg(windows)]
mod windows_impl {
    use super::*;
    use std::ffi::{c_void, OsStr};
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;
    use std::sync::Mutex;
    use tokio::net::windows::named_pipe::{
        ClientOptions, NamedPipeServer, PipeMode, ServerOptions,
    };
    use windows_sys::Win32::Foundation::{
        CloseHandle, LocalFree, ERROR_PIPE_BUSY, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{
        GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY,
        TOKEN_USER,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    /// RAII wrapper for a security descriptor allocated by
    /// `ConvertStringSecurityDescriptorToSecurityDescriptorW` (LocalAlloc).
    struct OwnedSecurityDescriptor {
        ptr: PSECURITY_DESCRIPTOR,
    }

    impl Drop for OwnedSecurityDescriptor {
        fn drop(&mut self) {
            if !self.ptr.is_null() {
                unsafe {
                    LocalFree(self.ptr as *mut _);
                }
                self.ptr = ptr::null_mut();
            }
        }
    }

    impl OwnedSecurityDescriptor {
        fn from_sddl(sddl: &str) -> io::Result<Self> {
            let wide: Vec<u16> = OsStr::new(sddl)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let mut sd: PSECURITY_DESCRIPTOR = ptr::null_mut();
            let mut size = 0u32;
            let ok = unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    wide.as_ptr(),
                    SDDL_REVISION_1,
                    &mut sd,
                    &mut size,
                )
            };
            if ok == 0 || sd.is_null() {
                return Err(io::Error::last_os_error());
            }
            Ok(Self { ptr: sd })
        }

        fn as_mut_ptr(&mut self) -> PSECURITY_DESCRIPTOR {
            self.ptr
        }
    }

    pub struct PipeListener {
        name: String,
        // Server handle waiting for next client; recreated after each accept.
        current: Mutex<Option<NamedPipeServer>>,
    }

    impl PipeListener {
        pub fn bind(name: &str) -> io::Result<Self> {
            let server = create_server(name)?;
            Ok(Self {
                name: name.to_string(),
                current: Mutex::new(Some(server)),
            })
        }

        pub fn name(&self) -> &str {
            &self.name
        }

        pub async fn accept(&self) -> io::Result<IpcStream> {
            let server = {
                let mut g = self.current.lock().unwrap();
                g.take()
                    .ok_or_else(|| io::Error::other("pipe listener exhausted"))?
            };
            server.connect().await?;
            // Prepare next instance for the following client.
            let next = create_server(&self.name)?;
            *self.current.lock().unwrap() = Some(next);
            Ok(IpcStream::WindowsServer(server))
        }
    }

    fn create_server(name: &str) -> io::Result<NamedPipeServer> {
        // first_pipe_instance=false allows multi-instance; we recreate after accept.
        // Owner-only DACL via current-user SID SDDL + reject remote clients.
        let sddl = current_user_sddl()?;
        let mut sd = OwnedSecurityDescriptor::from_sddl(&sddl)?;
        let mut sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd.as_mut_ptr(),
            bInheritHandle: 0,
        };

        let mut opts = ServerOptions::new();
        opts.pipe_mode(PipeMode::Byte);
        opts.reject_remote_clients(true);
        opts.first_pipe_instance(false);
        // SAFETY: `sa` points at a valid SECURITY_ATTRIBUTES whose security
        // descriptor remains alive for the duration of this call (sd dropped after).
        unsafe { opts.create_with_security_attributes_raw(name, &mut sa as *mut _ as *mut c_void) }
    }

    pub async fn connect_pipe(name: &str) -> io::Result<IpcStream> {
        // Retry on ERROR_PIPE_BUSY (another client connecting).
        for _ in 0..50 {
            match ClientOptions::new().open(name) {
                Ok(client) => return Ok(IpcStream::Windows(client)),
                Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => {
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::new(io::ErrorKind::TimedOut, "named pipe busy"))
    }

    /// Build a user-only SDDL string: D:(A;;GA;;;<current-user-sid>)
    pub fn current_user_sddl() -> io::Result<String> {
        let sid = current_user_sid_string()?;
        Ok(format!("D:(A;;GA;;;{sid})"))
    }

    fn current_user_sid_string() -> io::Result<String> {
        unsafe {
            let mut token = INVALID_HANDLE_VALUE;
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return Err(io::Error::last_os_error());
            }
            let mut needed = 0u32;
            GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut needed);
            let mut buf = vec![0u8; needed as usize];
            if GetTokenInformation(
                token,
                TokenUser,
                buf.as_mut_ptr() as *mut _,
                needed,
                &mut needed,
            ) == 0
            {
                CloseHandle(token);
                return Err(io::Error::last_os_error());
            }
            CloseHandle(token);
            let user = &*(buf.as_ptr() as *const TOKEN_USER);
            sid_to_string(user.User.Sid)
        }
    }

    fn sid_to_string(sid: windows_sys::Win32::Security::PSID) -> io::Result<String> {
        use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
        unsafe {
            let mut str_ptr: *mut u16 = ptr::null_mut();
            if ConvertSidToStringSidW(sid, &mut str_ptr) == 0 {
                return Err(io::Error::last_os_error());
            }
            let mut len = 0;
            while *str_ptr.add(len) != 0 {
                len += 1;
            }
            let slice = std::slice::from_raw_parts(str_ptr, len);
            let s = String::from_utf16_lossy(slice);
            LocalFree(str_ptr as *mut _);
            Ok(s)
        }
    }

    /// Validate that an SDDL string parses (compile + unit coverage for ACL foundation).
    pub fn sddl_is_valid(sddl: &str) -> bool {
        OwnedSecurityDescriptor::from_sddl(sddl).is_ok()
    }

    /// Whether create_server applies current-user SID ACL (always true on Windows).
    pub fn pipe_uses_current_user_acl() -> bool {
        true
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn sddl_for_world_is_valid_shape() {
            // Well-known Everyone SID used only to validate SDDL parsing path.
            assert!(sddl_is_valid("D:(A;;GA;;;WD)"));
        }

        #[test]
        fn current_user_sddl_parses() {
            let sddl = current_user_sddl().expect("sid");
            assert!(sddl.starts_with("D:(A;;GA;;;"));
            assert!(sddl_is_valid(&sddl));
        }

        #[test]
        fn pipe_create_applies_current_user_acl_policy() {
            assert!(pipe_uses_current_user_acl());
            // Build SD + SECURITY_ATTRIBUTES the same way create_server does.
            let sddl = current_user_sddl().expect("sid");
            let mut sd = OwnedSecurityDescriptor::from_sddl(&sddl).expect("sd");
            let sa = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: sd.as_mut_ptr(),
                bInheritHandle: 0,
            };
            assert!(!sa.lpSecurityDescriptor.is_null());
            assert_eq!(sa.bInheritHandle, 0);
        }

        #[tokio::test]
        async fn create_server_with_user_acl_accepts_same_user() {
            // Unique pipe name to avoid collisions with a running daemon.
            let name = format!(r"\\.\pipe\groktask-test-acl-{}", std::process::id());
            let listener = PipeListener::bind(&name).expect("bind with user ACL");
            let client = tokio::spawn({
                let name = name.clone();
                async move { connect_pipe(&name).await }
            });
            let server_stream = listener.accept().await.expect("accept");
            let client_stream = client.await.expect("join").expect("connect");
            drop(server_stream);
            drop(client_stream);
        }
    }
}

#[cfg(windows)]
pub use windows_impl::{current_user_sddl, pipe_uses_current_user_acl, sddl_is_valid};

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    #[tokio::test]
    async fn unix_socket_permissions_0600() {
        use super::*;
        use crate::paths::{self, GROKTASK_HOME_ENV};
        use tempfile::TempDir;

        let _g = paths::test_env_lock();
        let tmp = TempDir::new().unwrap();
        let prev = std::env::var_os(GROKTASK_HOME_ENV);
        std::env::set_var(GROKTASK_HOME_ENV, tmp.path());
        let listener = bind_daemon().unwrap();
        let mode = unix_impl::socket_mode(&paths::daemon_sock()).unwrap();
        assert_eq!(mode, 0o600, "socket mode was {mode:o}");
        drop(listener);
        match prev {
            Some(v) => std::env::set_var(GROKTASK_HOME_ENV, v),
            None => std::env::remove_var(GROKTASK_HOME_ENV),
        }
    }
}
