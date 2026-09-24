use semwright_types::{Error, ErrorCode, Result};
use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree},
        Security::Authorization::ConvertSidToStringSidW,
        Security::{GetLengthSid, GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser},
        System::{
            RemoteDesktop::ProcessIdToSessionId,
            Threading::{
                GetCurrentProcess, GetCurrentProcessId, OpenProcess, OpenProcessToken,
                PROCESS_QUERY_LIMITED_INFORMATION,
            },
        },
    },
    core::PWSTR,
};

struct OwnedHandle(HANDLE);
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: this type is constructed only from a successful API call returning ownership.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn current_token() -> Result<OwnedHandle> {
    let mut token = HANDLE::default();
    // SAFETY: the pseudo process handle is valid and `token` is writable for the call duration.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }.map_err(|_| {
        Error::new(
            ErrorCode::PermissionDenied,
            "Current access token is unavailable",
        )
    })?;
    Ok(OwnedHandle(token))
}

fn token_user_sid(token: HANDLE) -> Result<Vec<u8>> {
    let mut needed = 0u32;
    // Querying with no output buffer intentionally returns insufficient-buffer and fills `needed`.
    let _ = unsafe { GetTokenInformation(token, TokenUser, None, 0, &mut needed) };
    if needed == 0 || needed > 65_536 {
        return Err(Error::new(
            ErrorCode::BackendFailed,
            "Invalid token user size",
        ));
    }
    let mut bytes = vec![0u8; needed as usize];
    // SAFETY: the vector is live, writable and exactly `needed` bytes long.
    unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            Some(bytes.as_mut_ptr().cast()),
            needed,
            &mut needed,
        )
    }
    .map_err(|_| Error::new(ErrorCode::PermissionDenied, "Token user lookup failed"))?;
    Ok(bytes)
}

fn sid_string_from_token(token: HANDLE) -> Result<String> {
    let bytes = token_user_sid(token)?;
    // TOKEN_USER is at the beginning of the returned buffer and points into that same live buffer.
    let user = unsafe { &*(bytes.as_ptr().cast::<TOKEN_USER>()) };
    let mut out = PWSTR::null();
    // SAFETY: User.Sid belongs to `bytes`, which remains live through conversion.
    unsafe { ConvertSidToStringSidW(user.User.Sid, &mut out) }
        .map_err(|_| Error::new(ErrorCode::BackendFailed, "SID conversion failed"))?;
    if out.is_null() {
        return Err(Error::new(
            ErrorCode::BackendFailed,
            "SID conversion returned null",
        ));
    }
    let value = unsafe { out.to_string() }
        .map_err(|_| Error::new(ErrorCode::BackendFailed, "SID string was invalid"))?;
    // SAFETY: ConvertSidToStringSidW allocates this buffer with LocalAlloc.
    unsafe {
        let _ = LocalFree(Some(HLOCAL(out.0.cast())));
    }
    Ok(value)
}

pub fn current_user_sid_bytes() -> Result<Vec<u8>> {
    let token = current_token()?;
    let bytes = token_user_sid(token.0)?;
    let user = unsafe { &*(bytes.as_ptr().cast::<TOKEN_USER>()) };
    let len = unsafe { GetLengthSid(user.User.Sid) } as usize;
    if len == 0 || len > 4096 {
        return Err(Error::new(
            ErrorCode::BackendFailed,
            "Invalid current user SID length",
        ));
    }
    Ok(unsafe { std::slice::from_raw_parts(user.User.Sid.0.cast::<u8>(), len) }.to_vec())
}

pub fn current_user_sid() -> Result<String> {
    let token = current_token()?;
    sid_string_from_token(token.0)
}

pub fn process_user_sid_bytes(pid: u32) -> Result<Vec<u8>> {
    // SAFETY: query-only access to an existing process; no handle inheritance or mutation rights.
    let process =
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.map_err(|_| {
            Error::new(
                ErrorCode::PermissionDenied,
                "Peer process cannot be queried",
            )
        })?;
    let process = OwnedHandle(process);
    let mut token = HANDLE::default();
    // SAFETY: query-only token access for the live process handle.
    unsafe { OpenProcessToken(process.0, TOKEN_QUERY, &mut token) }.map_err(|_| {
        Error::new(
            ErrorCode::PermissionDenied,
            "Peer process token cannot be queried",
        )
    })?;
    let token = OwnedHandle(token);
    let bytes = token_user_sid(token.0)?;
    let user = unsafe { &*(bytes.as_ptr().cast::<TOKEN_USER>()) };
    let len = unsafe { GetLengthSid(user.User.Sid) } as usize;
    if len == 0 || len > 4096 {
        return Err(Error::new(
            ErrorCode::BackendFailed,
            "Invalid peer SID length",
        ));
    }
    Ok(unsafe { std::slice::from_raw_parts(user.User.Sid.0.cast::<u8>(), len) }.to_vec())
}

pub fn current_session_id() -> Result<u32> {
    let mut session = 0u32;
    // SAFETY: output storage is valid; the PID is the live current process.
    unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session) }
        .map_err(|_| Error::unavailable("Current Windows session ID is unavailable"))?;
    Ok(session)
}

/// Stable semantic principal used by platform services; never expose a raw token handle.
pub fn current_principal() -> Result<String> {
    Ok(format!(
        "sid:{};session:{}",
        current_user_sid()?,
        current_session_id()?
    ))
}
