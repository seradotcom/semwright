use crate::{identity::current_user_sid_bytes, job::ProcessJob, pe::require_native_architecture};
use async_trait::async_trait;
use semwright_platform_api::launch::{
    ExecutableVerifier, SandboxChildControl, SandboxLauncher, SandboxProcess, SandboxSpec,
};
use semwright_types::{Error, ErrorCode, Result, unique_id};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsStr,
    fs::File,
    io::Read,
    os::windows::{
        ffi::OsStrExt,
        fs::OpenOptionsExt,
        io::{AsRawHandle, FromRawHandle},
    },
    path::Path,
};
use tokio::{fs::File as TokioFile, process::Command};
use windows::Win32::{
    Foundation::{
        CloseHandle, DUPLICATE_SAME_ACCESS, DuplicateHandle, GENERIC_ALL, GENERIC_WRITE, HANDLE,
        HANDLE_FLAG_INHERIT, HANDLE_FLAGS, HLOCAL, HWND, LocalFree, TRUST_E_EXPLICIT_DISTRUST,
        TRUST_E_NOSIGNATURE, WAIT_OBJECT_0,
    },
    Security::{
        ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_SIZE_INFORMATION, AclSizeInformation,
        Authorization::{GetSecurityInfo, SE_FILE_OBJECT},
        CreateWellKnownSid, DACL_SECURITY_INFORMATION, EqualSid, FreeSid, GetAce,
        GetAclInformation,
        Isolation::{CreateAppContainerProfile, DeleteAppContainerProfile},
        OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES,
        SECURITY_CAPABILITIES, SECURITY_MAX_SID_SIZE, WinBuiltinAdministratorsSid,
        WinLocalSystemSid,
        WinTrust::{
            WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0, WINTRUST_FILE_INFO,
            WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_FILE, WTD_REVOCATION_CHECK_NONE,
            WTD_REVOKE_NONE, WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY, WTD_UI_NONE,
            WinVerifyTrust,
        },
    },
    Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, CreateFileW, DELETE, FILE_APPEND_DATA, FILE_ATTRIBUTE_NORMAL,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_WRITE,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_WRITE_ATTRIBUTES,
        FILE_WRITE_DATA, FILE_WRITE_EA, GetFileInformationByHandle, OPEN_EXISTING, WRITE_DAC,
        WRITE_OWNER,
    },
    System::{
        Pipes::CreatePipe,
        SystemServices::{
            ACCESS_ALLOWED_ACE_TYPE, ACCESS_ALLOWED_CALLBACK_ACE_TYPE,
            ACCESS_ALLOWED_CALLBACK_OBJECT_ACE_TYPE, ACCESS_ALLOWED_OBJECT_ACE_TYPE,
            ACCESS_DENIED_ACE_TYPE, ACCESS_DENIED_CALLBACK_ACE_TYPE,
            ACCESS_DENIED_CALLBACK_OBJECT_ACE_TYPE, ACCESS_DENIED_OBJECT_ACE_TYPE,
        },
        Threading::{
            CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT,
            DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT, GetCurrentProcess,
            INFINITE, InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES,
            PROCESS_INFORMATION, ResumeThread, STARTF_USESTDHANDLES, STARTUPINFOEXW,
            TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject,
        },
    },
};
use windows::core::{BOOL, PCWSTR, PWSTR};

const MAX_EXECUTABLE: u64 = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AuthenticodeStatus {
    Trusted,
    Unsigned,
    ExplicitlyDistrusted,
    Untrusted(i32),
}

fn require_authenticode_policy(status: AuthenticodeStatus) -> Result<()> {
    match status {
        AuthenticodeStatus::Trusted | AuthenticodeStatus::Unsigned => Ok(()),
        AuthenticodeStatus::ExplicitlyDistrusted => Err(Error::new(
            ErrorCode::PermissionDenied,
            "Windows explicitly distrusts this executable signature or publisher",
        )),
        AuthenticodeStatus::Untrusted(code) => Err(Error::new(
            ErrorCode::PermissionDenied,
            format!("Windows executable Authenticode verification failed ({code:#x})"),
        )),
    }
}

fn authenticode_status(file: &File, path: &Path) -> Result<AuthenticodeStatus> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    if wide.len() > 32_768 {
        return Err(Error::invalid(
            "Windows executable path exceeds Authenticode budget",
        ));
    }
    let mut file_info = WINTRUST_FILE_INFO {
        cbStruct: std::mem::size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: PCWSTR(wide.as_ptr()),
        hFile: HANDLE(file.as_raw_handle()),
        pgKnownSubject: std::ptr::null_mut(),
    };
    let mut trust = WINTRUST_DATA {
        cbStruct: std::mem::size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 {
            pFile: &mut file_info,
        },
        dwStateAction: WTD_STATEACTION_VERIFY,
        dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL | WTD_REVOCATION_CHECK_NONE,
        ..Default::default()
    };
    let mut policy = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    // SAFETY: all WINTRUST structures and the UTF-16 path remain live for the synchronous call.
    let status = unsafe {
        WinVerifyTrust(
            HWND::default(),
            &mut policy,
            (&mut trust as *mut WINTRUST_DATA).cast(),
        )
    };
    trust.dwStateAction = WTD_STATEACTION_CLOSE;
    // SAFETY: closes state created by the immediately preceding verification call.
    let _ = unsafe {
        WinVerifyTrust(
            HWND::default(),
            &mut policy,
            (&mut trust as *mut WINTRUST_DATA).cast(),
        )
    };
    Ok(if status == 0 {
        AuthenticodeStatus::Trusted
    } else if status == TRUST_E_EXPLICIT_DISTRUST.0 {
        AuthenticodeStatus::ExplicitlyDistrusted
    } else if status == TRUST_E_NOSIGNATURE.0 {
        AuthenticodeStatus::Unsigned
    } else {
        AuthenticodeStatus::Untrusted(status)
    })
}

fn info(file: &File) -> Result<BY_HANDLE_FILE_INFORMATION> {
    let mut out = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the std File owns this live HANDLE and `out` is writable.
    unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut out) }
        .map_err(|_| Error::new(ErrorCode::BackendFailed, "PE file identity query failed"))?;
    Ok(out)
}

fn same_identity(a: &BY_HANDLE_FILE_INFORMATION, b: &BY_HANDLE_FILE_INFORMATION) -> bool {
    a.dwVolumeSerialNumber == b.dwVolumeSerialNumber
        && a.nFileIndexHigh == b.nFileIndexHigh
        && a.nFileIndexLow == b.nFileIndexLow
}

struct SecurityDescriptor(PSECURITY_DESCRIPTOR);
impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            // SAFETY: GetSecurityInfo allocates this descriptor with LocalAlloc.
            unsafe {
                let _ = LocalFree(Some(HLOCAL(self.0.0)));
            }
        }
    }
}

fn well_known_sid(kind: windows::Win32::Security::WELL_KNOWN_SID_TYPE) -> Result<Vec<u8>> {
    let mut bytes = vec![0u8; SECURITY_MAX_SID_SIZE as usize];
    let mut len = bytes.len() as u32;
    // SAFETY: writable buffer is SECURITY_MAX_SID_SIZE bytes; no domain SID is required.
    unsafe { CreateWellKnownSid(kind, None, Some(PSID(bytes.as_mut_ptr().cast())), &mut len) }
        .map_err(|_| {
            Error::new(
                ErrorCode::BackendFailed,
                "Well-known Windows SID lookup failed",
            )
        })?;
    if len == 0 || len as usize > bytes.len() {
        return Err(Error::new(
            ErrorCode::BackendFailed,
            "Well-known Windows SID size was invalid",
        ));
    }
    bytes.truncate(len as usize);
    Ok(bytes)
}

fn sid_matches(sid: PSID, expected: &[u8]) -> bool {
    if sid.is_invalid() || expected.is_empty() {
        return false;
    }
    // SAFETY: sid belongs to the live security descriptor; expected is stable for this call.
    unsafe { EqualSid(sid, PSID(expected.as_ptr().cast_mut().cast())).is_ok() }
}

fn verify_executable_acl(file: &File) -> Result<()> {
    let mut owner = PSID::default();
    let mut dacl: *mut ACL = std::ptr::null_mut();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // Use the already-open handle so path replacement cannot race the ACL/content checks.
    // SAFETY: the open file owns a valid handle for this call; all output pointers reference
    // live local variables and the returned security descriptor is released by RAII below.
    let status = unsafe {
        GetSecurityInfo(
            HANDLE(file.as_raw_handle()),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            Some(&mut owner),
            None,
            Some(&mut dacl),
            None,
            Some(&mut descriptor),
        )
    };
    if status.0 != 0 || descriptor.is_invalid() {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Windows executable security descriptor could not be verified",
        ));
    }
    let _descriptor = SecurityDescriptor(descriptor);
    if owner.is_invalid() || dacl.is_null() {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Windows executable must have an explicit trusted owner and DACL",
        ));
    }

    let current = current_user_sid_bytes()?;
    let system = well_known_sid(WinLocalSystemSid)?;
    let admins = well_known_sid(WinBuiltinAdministratorsSid)?;
    let trusted = [&current[..], &system[..], &admins[..]];
    if !trusted.iter().any(|expected| sid_matches(owner, expected)) {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Windows executable owner is not the current user, SYSTEM or Administrators",
        ));
    }

    let mut acl_info = ACL_SIZE_INFORMATION::default();
    // SAFETY: the DACL points into the live security descriptor retained by the RAII guard,
    // and acl_info is a correctly sized writable output buffer for AclSizeInformation.
    unsafe {
        GetAclInformation(
            dacl,
            (&mut acl_info as *mut ACL_SIZE_INFORMATION).cast(),
            std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
            AclSizeInformation,
        )
    }
    .map_err(|_| {
        Error::new(
            ErrorCode::PermissionDenied,
            "Windows executable DACL is invalid",
        )
    })?;
    if acl_info.AceCount > 4_096 {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Windows executable DACL exceeds verification budget",
        ));
    }

    let dangerous = FILE_WRITE_DATA.0
        | FILE_APPEND_DATA.0
        | FILE_WRITE_EA.0
        | FILE_WRITE_ATTRIBUTES.0
        | DELETE.0
        | WRITE_DAC.0
        | WRITE_OWNER.0
        | FILE_GENERIC_WRITE.0
        | GENERIC_WRITE.0
        | GENERIC_ALL.0;

    for index in 0..acl_info.AceCount {
        let mut raw: *mut core::ffi::c_void = std::ptr::null_mut();
        // SAFETY: the DACL is retained by the security-descriptor guard, index is bounded
        // by AceCount, and raw is a valid writable out-pointer for the ACE address.
        unsafe { GetAce(dacl, index, &mut raw) }.map_err(|_| {
            Error::new(
                ErrorCode::PermissionDenied,
                "Windows executable DACL entry could not be inspected",
            )
        })?;
        if raw.is_null() {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "Windows executable DACL contains a null ACE",
            ));
        }
        // SAFETY: GetAce returned storage owned by the live DACL/security descriptor.
        let header = unsafe { &*(raw.cast::<ACE_HEADER>()) };
        let ace_type = header.AceType as u32;
        if matches!(
            ace_type,
            ACCESS_ALLOWED_ACE_TYPE
                | ACCESS_ALLOWED_OBJECT_ACE_TYPE
                | ACCESS_ALLOWED_CALLBACK_ACE_TYPE
                | ACCESS_ALLOWED_CALLBACK_OBJECT_ACE_TYPE
        ) {
            if usize::from(header.AceSize) < std::mem::size_of::<ACCESS_ALLOWED_ACE>() {
                return Err(Error::new(
                    ErrorCode::PermissionDenied,
                    "Windows executable DACL contains a malformed allow ACE",
                ));
            }
            // Every allow ACE layout starts with ACE_HEADER followed by the access mask.
            // SAFETY: the common prefix size was checked above.
            let ace = unsafe { &*(raw.cast::<ACCESS_ALLOWED_ACE>()) };
            if ace.Mask & dangerous == 0 {
                continue;
            }
            if ace_type != ACCESS_ALLOWED_ACE_TYPE {
                return Err(Error::new(
                    ErrorCode::PermissionDenied,
                    "Complex Windows executable mutation ACEs are fail-closed",
                ));
            }
            let sid = PSID((&ace.SidStart as *const u32).cast_mut().cast());
            if !trusted.iter().any(|expected| sid_matches(sid, expected)) {
                return Err(Error::new(
                    ErrorCode::PermissionDenied,
                    "Windows executable DACL grants mutation rights to an untrusted principal",
                ));
            }
            continue;
        }
        match ace_type {
            ACCESS_DENIED_ACE_TYPE
            | ACCESS_DENIED_OBJECT_ACE_TYPE
            | ACCESS_DENIED_CALLBACK_ACE_TYPE
            | ACCESS_DENIED_CALLBACK_OBJECT_ACE_TYPE => {}
            _ => {
                return Err(Error::new(
                    ErrorCode::PermissionDenied,
                    "Unknown Windows executable DACL ACE type is fail-closed",
                ));
            }
        }
    }
    Ok(())
}

pub struct WindowsVerifier;
impl ExecutableVerifier for WindowsVerifier {
    fn verify(&self, path: &Path, digest: &str) -> Result<Vec<u8>> {
        if !path.is_absolute() {
            return Err(Error::invalid(
                "Pinned Windows executable path must be absolute",
            ));
        }
        let spelling = path.as_os_str().to_string_lossy().to_ascii_lowercase();
        if spelling.starts_with(r"\\")
            || spelling.starts_with(r"\\?\")
            || spelling.starts_with(r"\\.\")
        {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "UNC, extended and device executable paths are not accepted",
            ));
        }
        let mut options = std::fs::OpenOptions::new();
        options
            .read(true)
            .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_DELETE.0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0);
        let mut file = options.open(path)?;
        let before = info(&file)?;
        verify_executable_acl(&file)?;
        if before.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
            || before.nNumberOfLinks != 1
            || before.nFileSizeHigh != 0
            || before.nFileSizeLow as u64 > MAX_EXECUTABLE
        {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "Unsafe Windows executable type, link count or size",
            ));
        }
        let mut bytes = Vec::with_capacity(before.nFileSizeLow as usize);
        file.by_ref()
            .take(MAX_EXECUTABLE + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_EXECUTABLE {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Windows executable exceeds size budget",
            ));
        }
        let after = info(&file)?;
        if !same_identity(&before, &after)
            || before.nFileSizeHigh != after.nFileSizeHigh
            || before.nFileSizeLow != after.nFileSizeLow
            || before.ftLastWriteTime != after.ftLastWriteTime
        {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Windows executable changed while it was being verified",
            ));
        }
        let expected = digest.trim().to_ascii_lowercase();
        if expected.len() != 64 || format!("{:x}", Sha256::digest(&bytes)) != expected {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "Executable digest mismatch",
            ));
        }
        require_native_architecture(&bytes)?;
        require_authenticode_policy(authenticode_status(&file, path)?)?;
        Ok(bytes)
    }
}

struct NativeHandle(HANDLE);
// SAFETY: kernel HANDLE values are process-wide. This wrapper owns one handle and closes it once.
unsafe impl Send for NativeHandle {}

impl NativeHandle {
    fn raw(&self) -> HANDLE {
        self.0
    }

    fn into_file(mut self) -> File {
        let raw = self.0.0;
        self.0 = HANDLE::default();
        // SAFETY: ownership of the live HANDLE moves from this guard to std::fs::File.
        unsafe { File::from_raw_handle(raw) }
    }
}

impl Drop for NativeHandle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            // SAFETY: this guard exclusively owns the HANDLE.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

struct ProcAttributes {
    _storage: Vec<usize>,
    list: LPPROC_THREAD_ATTRIBUTE_LIST,
}

impl ProcAttributes {
    fn new(count: u32) -> Result<Self> {
        let mut bytes = 0usize;
        // The first call intentionally discovers the required allocation size.
        let _ = unsafe { InitializeProcThreadAttributeList(None, count, None, &mut bytes) };
        if bytes == 0 {
            return Err(Error::new(
                ErrorCode::SandboxDenied,
                "Windows process attribute sizing failed",
            ));
        }
        let word = std::mem::size_of::<usize>();
        let mut storage = vec![0usize; (bytes + word - 1) / word];
        let list = LPPROC_THREAD_ATTRIBUTE_LIST(storage.as_mut_ptr().cast());
        // SAFETY: storage is aligned, writable, and lives for the attribute-list lifetime.
        unsafe { InitializeProcThreadAttributeList(Some(list), count, None, &mut bytes) }.map_err(
            |_| {
                Error::new(
                    ErrorCode::SandboxDenied,
                    "Windows process attribute initialization failed",
                )
            },
        )?;
        Ok(Self {
            _storage: storage,
            list,
        })
    }

    fn set_value<T>(&mut self, attribute: u32, value: &T) -> Result<()> {
        // SAFETY: value remains live until CreateProcessW returns and its exact size is supplied.
        unsafe {
            UpdateProcThreadAttribute(
                LPPROC_THREAD_ATTRIBUTE_LIST(self.list.0),
                0,
                attribute as usize,
                Some((value as *const T).cast()),
                std::mem::size_of::<T>(),
                None,
                None,
            )
        }
        .map_err(|_| {
            Error::new(
                ErrorCode::SandboxDenied,
                "Windows process attribute could not be applied",
            )
        })
    }

    fn set_slice<T>(&mut self, attribute: u32, values: &[T]) -> Result<()> {
        // SAFETY: values remains live until CreateProcessW returns and its byte length is exact.
        unsafe {
            UpdateProcThreadAttribute(
                LPPROC_THREAD_ATTRIBUTE_LIST(self.list.0),
                0,
                attribute as usize,
                Some(values.as_ptr().cast()),
                std::mem::size_of_val(values),
                None,
                None,
            )
        }
        .map_err(|_| {
            Error::new(
                ErrorCode::SandboxDenied,
                "Windows process handle allowlist could not be applied",
            )
        })
    }
}

impl Drop for ProcAttributes {
    fn drop(&mut self) {
        if !self.list.is_invalid() {
            // SAFETY: the initialized list points into _storage, which is still live here.
            unsafe { DeleteProcThreadAttributeList(LPPROC_THREAD_ATTRIBUTE_LIST(self.list.0)) };
        }
    }
}

struct AppContainerProfile {
    name: Vec<u16>,
    sid: PSID,
    delete_on_drop: bool,
}

impl AppContainerProfile {
    fn create() -> Result<Self> {
        let suffix: String = unique_id()
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .take(32)
            .collect();
        let name = format!("Semwright.Sandbox.{suffix}");
        let wide = wide_null(OsStr::new(&name))?;
        // SAFETY: all strings are stable NUL-terminated buffers; no capabilities are requested.
        let sid = unsafe {
            CreateAppContainerProfile(
                PCWSTR(wide.as_ptr()),
                PCWSTR(wide.as_ptr()),
                PCWSTR(wide.as_ptr()),
                None,
            )
        }
        .map_err(|_| {
            Error::new(
                ErrorCode::SandboxDenied,
                "Windows AppContainer profile creation failed",
            )
        })?;
        Ok(Self {
            name: wide,
            sid,
            delete_on_drop: true,
        })
    }

    fn transfer_name(mut self) -> Vec<u16> {
        self.delete_on_drop = false;
        self.name.clone()
    }
}

impl Drop for AppContainerProfile {
    fn drop(&mut self) {
        if !self.sid.is_invalid() {
            // SAFETY: CreateAppContainerProfile allocated this SID for the caller.
            unsafe {
                let _ = FreeSid(self.sid);
            }
        }
        if self.delete_on_drop {
            // SAFETY: name is a stable NUL-terminated profile name.
            unsafe {
                let _ = DeleteAppContainerProfile(PCWSTR(self.name.as_ptr()));
            }
        }
    }
}

fn wide_null(value: &OsStr) -> Result<Vec<u16>> {
    let mut wide: Vec<u16> = value.encode_wide().collect();
    if wide.is_empty() || wide.len() >= 32_767 || wide.contains(&0) {
        return Err(Error::invalid(
            "Windows sandbox string is empty, oversized or contains NUL",
        ));
    }
    wide.push(0);
    Ok(wide)
}

fn push_quoted_arg(out: &mut Vec<u16>, arg: &OsStr) {
    let units: Vec<u16> = arg.encode_wide().collect();
    let quote = units.is_empty()
        || units
            .iter()
            .any(|unit| matches!(*unit, 9 | 10 | 13 | 32 | 34));
    if !quote {
        out.extend(units);
        return;
    }
    out.push(34);
    let mut slashes = 0usize;
    for unit in units {
        if unit == 92 {
            slashes += 1;
            continue;
        }
        if unit == 34 {
            for _ in 0..(slashes * 2 + 1) {
                out.push(92);
            }
            out.push(34);
        } else {
            for _ in 0..slashes {
                out.push(92);
            }
            out.push(unit);
        }
        slashes = 0;
    }
    for _ in 0..(slashes * 2) {
        out.push(92);
    }
    out.push(34);
}

fn command_line(spec: &SandboxSpec) -> Result<Vec<u16>> {
    let mut out = Vec::new();
    push_quoted_arg(&mut out, spec.staged_executable.as_os_str());
    for arg in &spec.args {
        out.push(32);
        push_quoted_arg(&mut out, OsStr::new(arg));
    }
    if out.len() >= 32_767 {
        return Err(Error::invalid(
            "Windows sandbox command line exceeds CreateProcess budget",
        ));
    }
    out.push(0);
    Ok(out)
}

fn environment_block(spec: &SandboxSpec) -> Result<Vec<u16>> {
    let mut entries = spec.environment.clone();
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        entries.push((
            "SystemRoot".into(),
            system_root.to_string_lossy().into_owned(),
        ));
    }
    entries.sort_by_key(|(name, _)| name.to_ascii_uppercase());
    let mut out = Vec::new();
    for (name, value) in entries {
        let entry = format!("{name}={value}");
        if entry.contains('\0') {
            return Err(Error::invalid("Windows sandbox environment contains NUL"));
        }
        out.extend(entry.encode_utf16());
        out.push(0);
    }
    if out.is_empty() {
        out.push(0);
    }
    out.push(0);
    if out.len() >= 32_767 {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Windows sandbox environment exceeds CreateProcess budget",
        ));
    }
    Ok(out)
}

fn inheritable_pipe() -> Result<(NativeHandle, NativeHandle)> {
    let attrs = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: BOOL(1),
    };
    let mut read = HANDLE::default();
    let mut write = HANDLE::default();
    // SAFETY: outputs are writable and attrs remains live for the synchronous call.
    unsafe { CreatePipe(&mut read, &mut write, Some(&attrs), 0) }.map_err(|_| {
        Error::new(
            ErrorCode::SandboxDenied,
            "Windows sandbox stdio pipe creation failed",
        )
    })?;
    Ok((NativeHandle(read), NativeHandle(write)))
}

fn clear_inheritance(handle: HANDLE) -> Result<()> {
    // SAFETY: handle is live and owned by the parent; only its inheritance flag changes.
    unsafe {
        windows::Win32::Foundation::SetHandleInformation(
            handle,
            HANDLE_FLAG_INHERIT.0,
            HANDLE_FLAGS(0),
        )
    }
    .map_err(|_| {
        Error::new(
            ErrorCode::SandboxDenied,
            "Windows parent pipe inheritance hardening failed",
        )
    })
}

fn duplicate_owned_handle(handle: HANDLE) -> Result<NativeHandle> {
    let current = unsafe { GetCurrentProcess() };
    let mut duplicate = HANDLE::default();
    // SAFETY: source/target are the current process and duplicate receives a separately owned
    // process handle with the same access mask. No pseudo handle is passed as the source object.
    unsafe {
        DuplicateHandle(
            current,
            handle,
            current,
            &mut duplicate,
            0,
            false,
            DUPLICATE_SAME_ACCESS,
        )
    }
    .map_err(|_| {
        Error::new(
            ErrorCode::BackendFailed,
            "Windows sandbox process handle duplication failed",
        )
    })?;
    Ok(NativeHandle(duplicate))
}

fn inherited_null() -> Result<NativeHandle> {
    let attrs = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: BOOL(1),
    };
    let name = wide_null(OsStr::new("NUL"))?;
    // SAFETY: name and attrs remain live for the synchronous open.
    let handle = unsafe {
        CreateFileW(
            PCWSTR(name.as_ptr()),
            GENERIC_WRITE.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            Some(&attrs),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
    }
    .map_err(|_| {
        Error::new(
            ErrorCode::SandboxDenied,
            "Windows NUL handle creation failed",
        )
    })?;
    Ok(NativeHandle(handle))
}

struct NativeSandboxChild {
    process: NativeHandle,
    _job: ProcessJob,
    pid: u32,
    profile_name: Option<Vec<u16>>,
    exited: bool,
}

impl NativeSandboxChild {
    fn cleanup_profile(&mut self) {
        if let Some(name) = self.profile_name.take() {
            // SAFETY: name is the NUL-terminated profile created for this child.
            unsafe {
                let _ = DeleteAppContainerProfile(PCWSTR(name.as_ptr()));
            }
        }
    }

    fn observed_exit(&mut self) -> bool {
        // SAFETY: process is a live owned process HANDLE.
        let wait = unsafe { WaitForSingleObject(self.process.raw(), 0) };
        if wait == WAIT_OBJECT_0 {
            self.exited = true;
            self.cleanup_profile();
            true
        } else {
            false
        }
    }
}

#[async_trait]
impl SandboxChildControl for NativeSandboxChild {
    fn id(&self) -> Option<u32> {
        Some(self.pid)
    }

    async fn kill(&mut self) -> Result<()> {
        if self.observed_exit() {
            return Ok(());
        }
        // SAFETY: process is a live child owned by this controller.
        unsafe { TerminateProcess(self.process.raw(), 1) }.map_err(|_| {
            Error::new(
                ErrorCode::SandboxDenied,
                "Windows sandbox child termination failed",
            )
        })?;
        self.wait().await
    }

    async fn wait(&mut self) -> Result<()> {
        if self.observed_exit() {
            return Ok(());
        }
        let wait_handle = duplicate_owned_handle(self.process.raw())?;
        let wait = tokio::task::spawn_blocking(move || {
            // SAFETY: wait_handle exclusively owns a duplicate process HANDLE.
            unsafe { WaitForSingleObject(wait_handle.raw(), INFINITE) }
        })
        .await
        .map_err(|_| Error::new(ErrorCode::Internal, "Windows sandbox wait task failed"))?;
        if wait != WAIT_OBJECT_0 {
            return Err(Error::new(
                ErrorCode::BackendFailed,
                "Windows sandbox wait returned an unexpected status",
            ));
        }
        self.exited = true;
        self.cleanup_profile();
        Ok(())
    }
}

impl Drop for NativeSandboxChild {
    fn drop(&mut self) {
        if !self.exited {
            // SAFETY: best-effort containment cleanup for the owned child.
            unsafe {
                let _ = TerminateProcess(self.process.raw(), 1);
                if WaitForSingleObject(self.process.raw(), 5_000) == WAIT_OBJECT_0 {
                    self.exited = true;
                }
            }
        }
        if self.exited {
            self.cleanup_profile();
        }
    }
}

/// Windows arbitrary-child launch is platform-owned: an AppContainer identity and explicit
/// inherited-handle list are attached at creation, the process starts suspended, enters a
/// kill-on-close Job Object, and is resumed only after that boundary exists.
pub struct WindowsSandbox;
impl SandboxLauncher for WindowsSandbox {
    fn command(&self, spec: &SandboxSpec) -> Result<Command> {
        spec.validate()?;
        Err(Error::new(
            ErrorCode::SandboxDenied,
            "Windows sandbox creation is only available through platform-owned spawn",
        ))
    }

    fn spawn(&self, spec: &SandboxSpec) -> Result<SandboxProcess> {
        spec.validate()?;
        if !spec.mounts.is_empty() {
            return Err(Error::new(
                ErrorCode::SandboxDenied,
                "Windows sandbox filesystem mounts remain fail-closed until AppContainer ACL grants are transactional",
            ));
        }
        if spec.network {
            return Err(Error::new(
                ErrorCode::SandboxDenied,
                "Windows sandbox network remains fail-closed until capability grants are explicit",
            ));
        }

        let profile = AppContainerProfile::create()?;
        let (child_stdin, parent_stdin) = inheritable_pipe()?;
        let (parent_stdout, child_stdout) = inheritable_pipe()?;
        clear_inheritance(parent_stdin.raw())?;
        clear_inheritance(parent_stdout.raw())?;
        let child_stderr = inherited_null()?;

        let handles = [child_stdin.raw(), child_stdout.raw(), child_stderr.raw()];
        let mut attributes = ProcAttributes::new(2)?;
        attributes.set_slice(PROC_THREAD_ATTRIBUTE_HANDLE_LIST, &handles)?;
        let capabilities = SECURITY_CAPABILITIES {
            AppContainerSid: profile.sid,
            ..Default::default()
        };
        attributes.set_value(PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES, &capabilities)?;

        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = child_stdin.raw();
        startup.StartupInfo.hStdOutput = child_stdout.raw();
        startup.StartupInfo.hStdError = child_stderr.raw();
        startup.lpAttributeList = LPPROC_THREAD_ATTRIBUTE_LIST(attributes.list.0);

        let limits = spec.limits.as_ref();
        let process_limit = limits
            .map(|limit| u32::try_from(limit.processes))
            .transpose()
            .map_err(|_| {
                Error::new(
                    ErrorCode::ResourceExhausted,
                    "Windows process limit exceeds Job budget",
                )
            })?;
        let memory_limit = limits
            .map(|limit| usize::try_from(limit.address_space_bytes))
            .transpose()
            .map_err(|_| {
                Error::new(
                    ErrorCode::ResourceExhausted,
                    "Windows memory limit exceeds Job budget",
                )
            })?;
        let cpu_seconds = limits.map(|limit| limit.cpu_seconds);
        let job = ProcessJob::new(process_limit, memory_limit, cpu_seconds)?;

        let application = wide_null(spec.staged_executable.as_os_str())?;
        let mut command = command_line(spec)?;
        let environment = environment_block(spec)?;
        let mut process_info = PROCESS_INFORMATION::default();
        let flags = CREATE_SUSPENDED
            | EXTENDED_STARTUPINFO_PRESENT
            | CREATE_UNICODE_ENVIRONMENT
            | CREATE_NO_WINDOW;
        // SAFETY: all pointed-to buffers, handles, attributes and security capabilities remain
        // live through CreateProcessW. Inheritance is restricted by HANDLE_LIST.
        unsafe {
            windows::Win32::System::Threading::CreateProcessW(
                PCWSTR(application.as_ptr()),
                Some(PWSTR(command.as_mut_ptr())),
                None,
                None,
                true,
                flags,
                Some(environment.as_ptr().cast()),
                PCWSTR::null(),
                (&startup as *const STARTUPINFOEXW).cast(),
                &mut process_info,
            )
        }
        .map_err(|_| {
            Error::new(
                ErrorCode::SandboxDenied,
                "Windows AppContainer process creation failed",
            )
        })?;

        let process = NativeHandle(process_info.hProcess);
        let thread = NativeHandle(process_info.hThread);
        if let Err(error) = job.assign_suspended_process(process.raw()) {
            // SAFETY: child is still suspended and must not survive a failed containment step.
            unsafe {
                let _ = TerminateProcess(process.raw(), 1);
            }
            return Err(error);
        }
        // SAFETY: containment is established; resuming now is the first point untrusted code runs.
        if unsafe { ResumeThread(thread.raw()) } == u32::MAX {
            // SAFETY: resume failed while we still own the process.
            unsafe {
                let _ = TerminateProcess(process.raw(), 1);
            }
            return Err(Error::new(
                ErrorCode::SandboxDenied,
                "Windows sandbox child could not be resumed",
            ));
        }
        drop(thread);
        drop(child_stdin);
        drop(child_stdout);
        drop(child_stderr);
        drop(attributes);

        let stdin = Box::new(TokioFile::from_std(parent_stdin.into_file()));
        let stdout = Box::new(TokioFile::from_std(parent_stdout.into_file()));
        let profile_name = profile.transfer_name();
        Ok(SandboxProcess::from_parts(
            stdin,
            stdout,
            Box::new(NativeSandboxChild {
                process,
                _job: job,
                pid: process_info.dwProcessId,
                profile_name: Some(profile_name),
                exited: false,
            }),
        ))
    }

    fn available(&self, _helper: &Path) -> bool {
        true
    }

    fn mechanism(&self) -> &'static str {
        "windows-appcontainer-job-handle-list-v1"
    }

    fn diagnostics(&self, _helper: &Path) -> serde_json::Value {
        serde_json::json!({
            "available": true,
            "mechanism": self.mechanism(),
            "pre_first_instruction_containment": true,
            "filesystem_mounts": "fail_closed_pending_transactional_acl_grants",
            "network": "fail_closed_pending_explicit_capabilities",
            "resource_limits": ["processes", "cpu_seconds", "process_memory"],
        })
    }
}

#[cfg(test)]
mod verifier_tests {
    use super::*;

    #[test]
    fn hardened_staged_executable_passes_acl_digest_and_pe_verification() {
        let source = std::env::current_exe().expect("current test executable");
        let bytes = std::fs::read(&source).expect("read current test executable");
        let dir = tempfile::tempdir().expect("temporary staging directory");
        let path = dir.path().join("semwright-trust-fixture.exe");
        std::fs::copy(&source, &path).expect("copy native PE fixture");

        // GitHub-hosted build directories intentionally inherit broader ACLs than Semwright
        // permits for staged executable bytes. Model the production staging boundary instead
        // of weakening the verifier to accommodate the runner workspace.
        let user = std::env::var("USERNAME").expect("Windows USERNAME");
        let principal = match std::env::var("USERDOMAIN") {
            Ok(domain) if !domain.is_empty() => format!(r"{domain}\{user}"),
            _ => user,
        };
        let acl = std::process::Command::new("icacls")
            .arg(&path)
            .arg("/inheritance:r")
            .arg("/grant:r")
            .arg(format!("{principal}:(F)"))
            .status()
            .expect("run icacls for hardened fixture");
        assert!(
            acl.success(),
            "icacls must establish the staged fixture DACL"
        );

        let digest = format!("{:x}", Sha256::digest(&bytes));
        let verified = WindowsVerifier
            .verify(&path, &digest)
            .expect("hardened staged Windows executable should satisfy trust policy");
        assert_eq!(verified, bytes);
    }

    #[test]
    fn authenticode_policy_allows_unsigned_pinned_bytes_but_rejects_broken_signatures() {
        assert!(require_authenticode_policy(AuthenticodeStatus::Trusted).is_ok());
        assert!(require_authenticode_policy(AuthenticodeStatus::Unsigned).is_ok());
        assert!(require_authenticode_policy(AuthenticodeStatus::ExplicitlyDistrusted).is_err());
        assert!(require_authenticode_policy(AuthenticodeStatus::Untrusted(-1)).is_err());
    }

    #[test]
    fn well_known_trusted_sids_are_distinct_and_nonempty() {
        let system = well_known_sid(WinLocalSystemSid).expect("SYSTEM sid");
        let admins = well_known_sid(WinBuiltinAdministratorsSid).expect("Administrators sid");
        assert!(!system.is_empty());
        assert!(!admins.is_empty());
        assert_ne!(system, admins);
    }
}
