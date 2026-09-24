use semwright_types::{Error, ErrorCode, Result};
use windows::Win32::{
    Foundation::{CloseHandle, HANDLE},
    Security::{GetTokenInformation, RevertToSelf, TOKEN_QUERY, TOKEN_USER, TokenUser},
    System::{
        Pipes::{
            GetNamedPipeClientProcessId, GetNamedPipeClientSessionId, ImpersonateNamedPipeClient,
        },
        Threading::{GetCurrentProcessId, GetCurrentThread, OpenThreadToken},
    },
};

fn token_sid_bytes(token: HANDLE) -> Result<Vec<u8>> {
    let mut required = 0u32;
    // SAFETY: first call intentionally provides no buffer to obtain required size.
    let _ = unsafe { GetTokenInformation(token, TokenUser, None, 0, &mut required) };
    if required == 0 || required > 64 * 1024 {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Windows token user SID unavailable",
        ));
    }
    let mut storage = vec![0u8; required as usize];
    // SAFETY: buffer is exactly the size requested by GetTokenInformation.
    unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            Some(storage.as_mut_ptr().cast()),
            required,
            &mut required,
        )
    }
    .map_err(|_| {
        Error::new(
            ErrorCode::PermissionDenied,
            "Windows token user SID query failed",
        )
    })?;
    // SAFETY: the named-pipe/token handle and SID/session storage are live for this synchronous Win32 call; impersonation is scoped by the revert guard where applicable.
    let user = unsafe { &*(storage.as_ptr().cast::<TOKEN_USER>()) };
    let sid = user.User.Sid;
    if sid.is_invalid() {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Windows peer SID is invalid",
        ));
    }
    // Copy the self-relative SID bytes while the TOKEN_USER backing buffer is alive.
    // SAFETY: the named-pipe/token handle and SID/session storage are live for this synchronous Win32 call; impersonation is scoped by the revert guard where applicable.
    let length = unsafe { windows::Win32::Security::GetLengthSid(sid) } as usize;
    if length == 0 || length > 4096 {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Windows SID length invalid",
        ));
    }
    // SAFETY: the named-pipe/token handle and SID/session storage are live for this synchronous Win32 call; impersonation is scoped by the revert guard where applicable.
    Ok(unsafe { std::slice::from_raw_parts(sid.0.cast::<u8>(), length) }.to_vec())
}

struct RevertGuard;
impl Drop for RevertGuard {
    fn drop(&mut self) {
        // SAFETY: this guard exists only after successful named-pipe impersonation.
        unsafe {
            let _ = RevertToSelf();
        }
    }
}

/// Authenticate a connected named-pipe client from the kernel identity, never client JSON.
/// `owner_sid` must be the host user's SID bytes captured from its access token.
pub fn authenticate_same_user(
    pipe: HANDLE,
    owner_sid: &[u8],
    expected_session: u32,
) -> Result<u32> {
    let mut pid = 0u32;
    let mut session = 0u32;
    // SAFETY: caller passes a live connected named-pipe server handle; outputs are stack u32s.
    unsafe { GetNamedPipeClientProcessId(pipe, &mut pid) }.map_err(|_| {
        Error::new(
            ErrorCode::PermissionDenied,
            "Named Pipe client PID unavailable",
        )
    })?;
    // SAFETY: the named-pipe/token handle and SID/session storage are live for this synchronous Win32 call; impersonation is scoped by the revert guard where applicable.
    unsafe { GetNamedPipeClientSessionId(pipe, &mut session) }.map_err(|_| {
        Error::new(
            ErrorCode::PermissionDenied,
            "Named Pipe client session unavailable",
        )
    })?;
    if pid == 0 || session != expected_session {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Named Pipe client is outside the intended user session",
        ));
    }
    // SAFETY: impersonation is scoped by RevertGuard and is always reverted on every return path.
    unsafe { ImpersonateNamedPipeClient(pipe) }.map_err(|_| {
        Error::new(
            ErrorCode::PermissionDenied,
            "Named Pipe impersonation failed",
        )
    })?;
    let _revert = RevertGuard;
    let mut token = HANDLE::default();
    // SAFETY: current thread is impersonating; TOKEN_QUERY is the only requested access.
    unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, true, &mut token) }.map_err(
        |_| {
            Error::new(
                ErrorCode::PermissionDenied,
                "Named Pipe client token unavailable",
            )
        },
    )?;
    let peer_sid = token_sid_bytes(token);
    // SAFETY: the named-pipe/token handle and SID/session storage are live for this synchronous Win32 call; impersonation is scoped by the revert guard where applicable.
    unsafe {
        let _ = CloseHandle(token);
    }
    if peer_sid? != owner_sid {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Named Pipe client SID does not match host owner",
        ));
    }
    Ok(pid)
}

/// A diagnostic helper only. It does not authenticate a peer.
pub fn host_process_id() -> u32 {
    // SAFETY: the named-pipe/token handle and SID/session storage are live for this synchronous Win32 call; impersonation is scoped by the revert guard where applicable.
    unsafe { GetCurrentProcessId() }
}

use windows::{
    Win32::{
        Foundation::{HLOCAL, LocalFree},
        Security::Authorization::{
            ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
        },
        Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES},
        Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX},
        System::Pipes::{
            CreateNamedPipeW, PIPE_READMODE_MESSAGE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_MESSAGE,
            PIPE_WAIT,
        },
    },
    core::HSTRING,
};

struct OwnedSecurityDescriptor(PSECURITY_DESCRIPTOR);
impl Drop for OwnedSecurityDescriptor {
    fn drop(&mut self) {
        // SAFETY: SDDL conversion allocates the returned security descriptor with LocalAlloc.
        unsafe {
            let _ = LocalFree(Some(HLOCAL(self.0.0)));
        }
    }
}

pub struct OwnedPipe(HANDLE);
impl Drop for OwnedPipe {
    fn drop(&mut self) {
        // SAFETY: the named-pipe/token handle and SID/session storage are live for this synchronous Win32 call; impersonation is scoped by the revert guard where applicable.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
impl OwnedPipe {
    pub fn handle(&self) -> HANDLE {
        self.0
    }
}

/// Create the first local broker pipe instance with an explicit protected DACL.
/// Only LocalSystem and the exact current-user SID receive generic-all; remote clients are rejected.
pub fn create_owner_only_server(name: &str) -> Result<OwnedPipe> {
    if name.is_empty() || name.len() > 240 || name.contains(['/', ':']) {
        return Err(Error::invalid("Invalid local Named Pipe name"));
    }
    let sid = crate::identity::current_user_sid()?;
    let sddl = HSTRING::from(format!("D:P(A;;GA;;;SY)(A;;GA;;;{sid})"));
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: input HSTRING remains live and output pointer is owned until LocalFree.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            &sddl,
            SDDL_REVISION_1,
            &mut descriptor,
            None,
        )
    }
    .map_err(|_| {
        Error::new(
            ErrorCode::PermissionDenied,
            "Named Pipe security descriptor creation failed",
        )
    })?;
    let descriptor = OwnedSecurityDescriptor(descriptor);
    let attrs = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0.0,
        bInheritHandle: Default::default(),
    };
    let full = HSTRING::from(format!(r"\\.\pipe\semwright-{name}"));
    // SAFETY: explicit non-inheritable security attributes remain live for the create call only.
    let handle = unsafe {
        CreateNamedPipeW(
            &full,
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            16,
            1024 * 1024,
            1024 * 1024,
            5_000,
            Some(&attrs),
        )
    };
    if handle.is_invalid() {
        return Err(Error::new(
            ErrorCode::BackendFailed,
            "Owner-only Named Pipe creation failed",
        ));
    }
    Ok(OwnedPipe(handle))
}

use std::{
    os::windows::io::AsRawHandle,
    path::{Path, PathBuf},
};
use tokio::net::windows::named_pipe::{
    ClientOptions, NamedPipeClient, NamedPipeServer, PipeMode, ServerOptions,
};

fn validate_kind(kind: &str) -> Result<()> {
    if kind.is_empty()
        || kind.len() > 32
        || !kind.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(Error::invalid("Invalid Windows pipe kind"));
    }
    Ok(())
}

pub fn pipe_path(kind: &str) -> Result<PathBuf> {
    validate_kind(kind)?;
    let sid = crate::identity::current_user_sid()?.replace('-', "_");
    Ok(PathBuf::from(format!(r"\\.\pipe\semwright-{sid}-{kind}")))
}

pub fn create_tokio_server(path: &Path, first_instance: bool) -> Result<NamedPipeServer> {
    let spelling = path.as_os_str().to_string_lossy();
    if !spelling.starts_with(r"\\.\pipe\semwright-") || spelling.len() > 256 {
        return Err(Error::invalid("Invalid Semwright Named Pipe path"));
    }
    let sid = crate::identity::current_user_sid()?;
    let sddl: Vec<u16> = format!("D:P(A;;GA;;;SY)(A;;GA;;;{sid})")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut raw = std::ptr::null_mut();
    // SAFETY: input is NUL-terminated and the API writes one LocalAlloc-owned descriptor pointer.
    if unsafe {
        windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            windows_sys::Win32::Security::Authorization::SDDL_REVISION_1,
            &mut raw,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Named Pipe security descriptor creation failed",
        ));
    }
    struct LocalDescriptor(*mut std::ffi::c_void);
    impl Drop for LocalDescriptor {
        fn drop(&mut self) {
            // SAFETY: the descriptor was allocated by ConvertStringSecurityDescriptor...
            unsafe {
                windows_sys::Win32::Foundation::LocalFree(self.0);
            }
        }
    }
    let descriptor = LocalDescriptor(raw);
    let mut attrs = windows_sys::Win32::Security::SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<windows_sys::Win32::Security::SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };
    let mut options = ServerOptions::new();
    options
        .pipe_mode(PipeMode::Byte)
        .reject_remote_clients(true)
        .max_instances(64)
        .first_pipe_instance(first_instance);
    // SAFETY: attrs and its security descriptor remain live for the synchronous CreateNamedPipe call.
    let server = unsafe {
        options.create_with_security_attributes_raw(
            path.as_os_str(),
            (&mut attrs as *mut windows_sys::Win32::Security::SECURITY_ATTRIBUTES).cast(),
        )
    }?;
    Ok(server)
}

pub fn open_tokio_client(path: &Path) -> Result<NamedPipeClient> {
    let spelling = path.as_os_str().to_string_lossy();
    if !spelling.starts_with(r"\\.\pipe\semwright-") || spelling.len() > 256 {
        return Err(Error::invalid("Invalid Semwright Named Pipe path"));
    }
    ClientOptions::new().open(path).map_err(Into::into)
}

pub fn validate_tokio_server_peer(pipe: &NamedPipeServer) -> Result<u32> {
    let owner = crate::identity::current_user_sid_bytes()?;
    let session = crate::identity::current_session_id()?;
    let raw = windows::Win32::Foundation::HANDLE(pipe.as_raw_handle());
    authenticate_same_user(raw, &owner, session)
}

pub fn validate_tokio_client_peer(pipe: &NamedPipeClient) -> Result<u32> {
    use windows::Win32::System::Pipes::{GetNamedPipeServerProcessId, GetNamedPipeServerSessionId};
    let raw = windows::Win32::Foundation::HANDLE(pipe.as_raw_handle());
    let mut pid = 0u32;
    let mut session = 0u32;
    // SAFETY: raw is a connected client pipe handle; outputs are live stack storage.
    unsafe { GetNamedPipeServerProcessId(raw, &mut pid) }.map_err(|_| {
        Error::new(
            ErrorCode::PermissionDenied,
            "Named Pipe server PID unavailable",
        )
    })?;
    // SAFETY: the named-pipe/token handle and SID/session storage are live for this synchronous Win32 call; impersonation is scoped by the revert guard where applicable.
    unsafe { GetNamedPipeServerSessionId(raw, &mut session) }.map_err(|_| {
        Error::new(
            ErrorCode::PermissionDenied,
            "Named Pipe server session unavailable",
        )
    })?;
    if pid == 0 || session != crate::identity::current_session_id()? {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Named Pipe server is outside the intended user session",
        ));
    }
    if crate::identity::process_user_sid_bytes(pid)? != crate::identity::current_user_sid_bytes()? {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Named Pipe server SID does not match current user",
        ));
    }
    Ok(pid)
}
