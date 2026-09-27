use crate::{identity::current_user_sid_bytes, job::ProcessJob, pe::require_native_architecture};
use async_trait::async_trait;
#[cfg(test)]
use semwright_platform_api::launch::MountClass;
use semwright_platform_api::launch::{
    ExecutableVerifier, MaterializedMount, Mount, SANDBOX_MOUNTS_ENV, SandboxChildControl,
    SandboxLauncher, SandboxProcess, SandboxSpec, encode_materialized_mounts,
};
use semwright_types::{Error, ErrorCode, Result, unique_id};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    ffi::OsStr,
    fs::File,
    io::Read,
    os::windows::{
        ffi::OsStrExt,
        fs::OpenOptionsExt,
        io::{AsRawHandle, FromRawHandle},
    },
    path::{Component, Path},
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
        Authorization::{
            ConvertSidToStringSidW, EXPLICIT_ACCESS_W, GRANT_ACCESS, GetNamedSecurityInfoW,
            GetSecurityInfo, NO_MULTIPLE_TRUSTEE, REVOKE_ACCESS, SE_FILE_OBJECT, SetEntriesInAclW,
            SetNamedSecurityInfoW, TRUSTEE_IS_SID, TRUSTEE_IS_USER, TRUSTEE_W,
        },
        CopySid, CreateWellKnownSid, DACL_SECURITY_INFORMATION, EqualSid, FreeSid, GetAce,
        GetAclInformation, GetLengthSid,
        Isolation::{
            CreateAppContainerProfile, DeleteAppContainerProfile, GetAppContainerFolderPath,
        },
        NO_INHERITANCE, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
        SECURITY_ATTRIBUTES, SECURITY_CAPABILITIES, SECURITY_MAX_SID_SIZE,
        SUB_CONTAINERS_AND_OBJECTS_INHERIT, WinBuiltinAdministratorsSid, WinLocalSystemSid,
        WinTrust::{
            WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0, WINTRUST_FILE_INFO,
            WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_FILE, WTD_REVOCATION_CHECK_NONE,
            WTD_REVOKE_NONE, WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY, WTD_UI_NONE,
            WinVerifyTrust,
        },
    },
    Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, CreateFileW, DELETE, FILE_APPEND_DATA,
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT,
        FILE_DELETE_CHILD, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_GENERIC_EXECUTE, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_READ_ATTRIBUTES,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_WRITE_ATTRIBUTES,
        FILE_WRITE_DATA, FILE_WRITE_EA, GetFileInformationByHandle, OPEN_EXISTING, WRITE_DAC,
        WRITE_OWNER,
    },
    System::{
        Com::CoTaskMemFree,
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
    unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut out) }.map_err(
        |_| {
            Error::new(
                ErrorCode::BackendFailed,
                "Windows file identity query failed",
            )
        },
    )?;
    Ok(out)
}

type FileIdentity = (u32, u32, u32);

fn file_identity(metadata: &BY_HANDLE_FILE_INFORMATION) -> FileIdentity {
    (
        metadata.dwVolumeSerialNumber,
        metadata.nFileIndexHigh,
        metadata.nFileIndexLow,
    )
}

fn path_info_no_reparse(path: &Path) -> Result<BY_HANDLE_FILE_INFORMATION> {
    let mut options = std::fs::OpenOptions::new();
    options
        .read(true)
        .access_mode(FILE_READ_ATTRIBUTES.0)
        .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0 | FILE_SHARE_DELETE.0)
        .custom_flags((FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS).0);
    let file = options.open(path)?;
    info(&file)
}

fn require_single_link_regular(metadata: &BY_HANDLE_FILE_INFORMATION) -> Result<()> {
    if metadata.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
        || metadata.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0
        || metadata.nNumberOfLinks != 1
    {
        return Err(Error::new(
            ErrorCode::PolicyDenied,
            "Windows sandbox mount files must be regular, non-reparse and single-linked",
        ));
    }
    Ok(())
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
        // SAFETY: a null attribute-list pointer is the documented sizing probe; bytes is a
        // valid writable SIZE_T out-parameter and no attribute storage is dereferenced.
        let _ = unsafe { InitializeProcThreadAttributeList(None, count, None, &mut bytes) };
        if bytes == 0 {
            return Err(Error::new(
                ErrorCode::SandboxDenied,
                "Windows process attribute sizing failed",
            ));
        }
        let word = std::mem::size_of::<usize>();
        let mut storage = vec![0usize; bytes.div_ceil(word)];
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

fn pwstr_to_string_bounded(value: PWSTR, max_units: usize) -> Result<String> {
    if value.0.is_null() {
        return Err(Error::new(
            ErrorCode::SandboxDenied,
            "Windows AppContainer returned a null path",
        ));
    }
    let mut len = 0usize;
    // SAFETY: caller supplies a Windows-owned NUL-terminated string. Reads are capped.
    unsafe {
        while len < max_units && *value.0.add(len) != 0 {
            len += 1;
        }
        if len == max_units {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Windows AppContainer path exceeds budget",
            ));
        }
        String::from_utf16(std::slice::from_raw_parts(value.0, len))
            .map_err(|_| Error::invalid("Windows AppContainer path is not valid UTF-16"))
    }
}

fn appcontainer_folder(sid: PSID) -> Result<String> {
    let mut sid_text = PWSTR::null();
    // SAFETY: sid is returned by CreateAppContainerProfile and sid_text is a writable out-pointer.
    unsafe { ConvertSidToStringSidW(sid, &mut sid_text) }.map_err(|_| {
        Error::new(
            ErrorCode::SandboxDenied,
            "Windows AppContainer SID string conversion failed",
        )
    })?;
    // SAFETY: sid_text is a NUL-terminated string allocated by LocalAlloc.
    let folder = unsafe { GetAppContainerFolderPath(PCWSTR(sid_text.0)) };
    // SAFETY: ConvertSidToStringSidW allocated sid_text with LocalAlloc.
    unsafe {
        let _ = LocalFree(Some(HLOCAL(sid_text.0.cast())));
    }
    let folder = folder.map_err(|_| {
        Error::new(
            ErrorCode::SandboxDenied,
            "Windows AppContainer local profile path lookup failed",
        )
    })?;
    let result = pwstr_to_string_bounded(folder, 32_767);
    // SAFETY: GetAppContainerFolderPath returns memory that must be released with CoTaskMemFree.
    unsafe {
        CoTaskMemFree(Some(folder.0.cast()));
    }
    result
}

fn copy_sid_bytes(sid: PSID) -> Result<Vec<u8>> {
    if sid.is_invalid() {
        return Err(Error::new(
            ErrorCode::SandboxDenied,
            "Windows AppContainer SID is invalid",
        ));
    }
    // SAFETY: sid points to a live SID returned by the AppContainer APIs.
    let len = unsafe { GetLengthSid(sid) };
    if len == 0 || len > SECURITY_MAX_SID_SIZE {
        return Err(Error::new(
            ErrorCode::SandboxDenied,
            "Windows AppContainer SID length is invalid",
        ));
    }
    let mut bytes = vec![0u8; len as usize];
    // SAFETY: destination is exactly len bytes and source remains live for the call.
    unsafe { CopySid(len, PSID(bytes.as_mut_ptr().cast()), sid) }.map_err(|_| {
        Error::new(
            ErrorCode::SandboxDenied,
            "Windows AppContainer SID copy failed",
        )
    })?;
    Ok(bytes)
}

struct LocalAcl(*mut ACL);
impl Drop for LocalAcl {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: SetEntriesInAclW allocates this ACL with LocalAlloc.
            unsafe {
                let _ = LocalFree(Some(HLOCAL(self.0.cast())));
            }
        }
    }
}

fn named_dacl(path: &[u16]) -> Result<(*mut ACL, SecurityDescriptor)> {
    let mut dacl: *mut ACL = std::ptr::null_mut();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: path is NUL-terminated and all out-pointers reference live locals.
    let status = unsafe {
        GetNamedSecurityInfoW(
            PCWSTR(path.as_ptr()),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(&mut dacl),
            None,
            &mut descriptor,
        )
    };
    if status.0 != 0 || descriptor.is_invalid() || dacl.is_null() {
        return Err(Error::new(
            ErrorCode::SandboxDenied,
            "Windows sandbox mount DACL could not be read",
        ));
    }
    Ok((dacl, SecurityDescriptor(descriptor)))
}

fn dacl_has_sid(path: &[u16], sid: PSID) -> Result<bool> {
    let (dacl, _descriptor) = named_dacl(path)?;
    let mut acl_info = ACL_SIZE_INFORMATION::default();
    // SAFETY: dacl belongs to the live descriptor guard and acl_info is a sized output buffer.
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
            ErrorCode::SandboxDenied,
            "Windows sandbox mount DACL is invalid",
        )
    })?;
    if acl_info.AceCount > 4_096 {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Windows sandbox mount DACL exceeds verification budget",
        ));
    }
    for index in 0..acl_info.AceCount {
        let mut raw: *mut core::ffi::c_void = std::ptr::null_mut();
        // SAFETY: index is bounded by AceCount and raw is a writable ACE out-pointer.
        unsafe { GetAce(dacl, index, &mut raw) }.map_err(|_| {
            Error::new(
                ErrorCode::SandboxDenied,
                "Windows sandbox mount DACL entry could not be inspected",
            )
        })?;
        if raw.is_null() {
            return Err(Error::new(
                ErrorCode::SandboxDenied,
                "Windows sandbox mount DACL contains a null ACE",
            ));
        }
        // SAFETY: GetAce returned storage owned by the live DACL/security descriptor.
        let header = unsafe { &*(raw.cast::<ACE_HEADER>()) };
        if header.AceType as u32 != ACCESS_ALLOWED_ACE_TYPE
            || usize::from(header.AceSize) < std::mem::size_of::<ACCESS_ALLOWED_ACE>()
        {
            continue;
        }
        // SAFETY: a simple access-allowed ACE is at least ACCESS_ALLOWED_ACE bytes.
        let ace = unsafe { &*(raw.cast::<ACCESS_ALLOWED_ACE>()) };
        let ace_sid = PSID((&ace.SidStart as *const u32).cast_mut().cast());
        // SAFETY: both SIDs are live for this comparison.
        if unsafe { EqualSid(ace_sid, sid).is_ok() } {
            return Ok(true);
        }
    }
    Ok(false)
}

fn dacl_has_grant(
    path: &[u16],
    sid: PSID,
    permissions: u32,
    inheritance: windows::Win32::Security::ACE_FLAGS,
) -> Result<bool> {
    let (dacl, _descriptor) = named_dacl(path)?;
    let mut acl_info = ACL_SIZE_INFORMATION::default();
    // SAFETY: dacl belongs to the live descriptor guard and acl_info is a sized output buffer.
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
            ErrorCode::SandboxDenied,
            "Windows sandbox mount DACL is invalid",
        )
    })?;
    if acl_info.AceCount > 4_096 {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Windows sandbox mount DACL exceeds verification budget",
        ));
    }
    let inheritance_mask = SUB_CONTAINERS_AND_OBJECTS_INHERIT.0;
    for index in 0..acl_info.AceCount {
        let mut raw: *mut core::ffi::c_void = std::ptr::null_mut();
        // SAFETY: index is bounded by AceCount and raw is a writable ACE out-pointer.
        unsafe { GetAce(dacl, index, &mut raw) }.map_err(|_| {
            Error::new(
                ErrorCode::SandboxDenied,
                "Windows sandbox mount DACL entry could not be inspected",
            )
        })?;
        if raw.is_null() {
            return Err(Error::new(
                ErrorCode::SandboxDenied,
                "Windows sandbox mount DACL contains a null ACE",
            ));
        }
        // SAFETY: GetAce returned storage owned by the live DACL/security descriptor.
        let header = unsafe { &*(raw.cast::<ACE_HEADER>()) };
        if header.AceType as u32 != ACCESS_ALLOWED_ACE_TYPE
            || usize::from(header.AceSize) < std::mem::size_of::<ACCESS_ALLOWED_ACE>()
        {
            continue;
        }
        // SAFETY: a simple access-allowed ACE is at least ACCESS_ALLOWED_ACE bytes.
        let ace = unsafe { &*(raw.cast::<ACCESS_ALLOWED_ACE>()) };
        let ace_sid = PSID((&ace.SidStart as *const u32).cast_mut().cast());
        // SAFETY: both SIDs are live for this comparison.
        if unsafe { EqualSid(ace_sid, sid).is_ok() }
            && ace.Mask & permissions == permissions
            && u32::from(header.AceFlags) & inheritance_mask == inheritance.0
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn set_mount_ace(
    path: &[u16],
    sid: PSID,
    access_mode: windows::Win32::Security::Authorization::ACCESS_MODE,
    permissions: u32,
    inheritance: windows::Win32::Security::ACE_FLAGS,
) -> Result<()> {
    let (dacl, _descriptor) = named_dacl(path)?;
    let entry = EXPLICIT_ACCESS_W {
        grfAccessPermissions: permissions,
        grfAccessMode: access_mode,
        grfInheritance: inheritance,
        Trustee: TRUSTEE_W {
            pMultipleTrustee: std::ptr::null_mut(),
            MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_USER,
            ptstrName: PWSTR(sid.0.cast()),
        },
    };
    let mut updated: *mut ACL = std::ptr::null_mut();
    // SAFETY: entry, dacl and updated out-pointer remain live for this synchronous call.
    let status =
        unsafe { SetEntriesInAclW(Some(std::slice::from_ref(&entry)), Some(dacl), &mut updated) };
    if status.0 != 0 || updated.is_null() {
        return Err(Error::new(
            ErrorCode::SandboxDenied,
            "Windows sandbox mount ACE could not be constructed",
        ));
    }
    let updated = LocalAcl(updated);
    // SAFETY: path is NUL-terminated and updated contains a valid ACL allocated above.
    let status = unsafe {
        SetNamedSecurityInfoW(
            PCWSTR(path.as_ptr()),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(updated.0),
            None,
        )
    };
    if status.0 != 0 {
        return Err(Error::new(
            ErrorCode::SandboxDenied,
            "Windows sandbox mount ACE could not be applied",
        ));
    }
    Ok(())
}

fn apply_mount_ace(
    path: &[u16],
    sid: PSID,
    access_mode: windows::Win32::Security::Authorization::ACCESS_MODE,
    permissions: u32,
    inheritance: windows::Win32::Security::ACE_FLAGS,
    expected_present: bool,
) -> Result<()> {
    set_mount_ace(path, sid, access_mode, permissions, inheritance)?;

    let verified = if expected_present {
        dacl_has_grant(path, sid, permissions, inheritance)
    } else {
        dacl_has_sid(path, sid).map(|present| !present)
    };
    if matches!(verified, Ok(true)) {
        return Ok(());
    }

    if expected_present {
        // A unique AppContainer SID must not survive a failed grant verification. Revoke it
        // immediately and prove absence before propagating the original verification failure.
        let rollback = set_mount_ace(path, sid, REVOKE_ACCESS, 0, NO_INHERITANCE)
            .and_then(|_| dacl_has_sid(path, sid))
            .map(|present| !present);
        if !matches!(rollback, Ok(true)) {
            return Err(Error::new(
                ErrorCode::SandboxDenied,
                "Windows sandbox mount grant verification failed and rollback could not be proven",
            ));
        }
    }

    Err(Error::new(
        ErrorCode::SandboxDenied,
        "Windows sandbox mount ACE verification failed",
    ))
}

const WINDOWS_MOUNT_MAX_ENTRIES: usize = 50_000;
const WINDOWS_MOUNT_MAX_DEPTH: usize = 128;

fn validate_mount_tree(root: &Path) -> Result<(FileIdentity, bool)> {
    if root
        .components()
        .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(Error::new(
            ErrorCode::PolicyDenied,
            "Windows sandbox mount paths may not contain dot or parent components",
        ));
    }

    // Validate every existing ancestor by HANDLE so an intermediate junction/symlink cannot
    // redirect the mount outside the owner-granted tree. DOS/8.3 spellings are allowed when
    // they resolve through a non-reparse ancestor chain.
    for ancestor in root.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        let metadata = path_info_no_reparse(ancestor)?;
        if metadata.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Windows sandbox mount paths may not traverse reparse-point ancestors",
            ));
        }
    }

    let root_metadata = path_info_no_reparse(root)?;
    let identity = file_identity(&root_metadata);
    if root_metadata.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err(Error::new(
            ErrorCode::PolicyDenied,
            "Windows sandbox mount roots may not be reparse points",
        ));
    }
    let root_is_dir = root_metadata.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0;
    if !root_is_dir {
        require_single_link_regular(&root_metadata)?;
        return Ok((identity, false));
    }

    let mut pending = vec![(root.to_path_buf(), 0usize)];
    let mut entries = 0usize;
    while let Some((directory, depth)) = pending.pop() {
        if depth > WINDOWS_MOUNT_MAX_DEPTH {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Windows sandbox mount tree exceeds depth budget",
            ));
        }
        for entry in std::fs::read_dir(&directory)? {
            let entry = entry?;
            entries = entries.saturating_add(1);
            if entries > WINDOWS_MOUNT_MAX_ENTRIES {
                return Err(Error::new(
                    ErrorCode::ResourceExhausted,
                    "Windows sandbox mount tree exceeds entry budget",
                ));
            }
            let path = entry.path();
            let metadata = path_info_no_reparse(&path)?;
            if metadata.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
                return Err(Error::new(
                    ErrorCode::PolicyDenied,
                    "Windows sandbox mount trees may not contain reparse points",
                ));
            }
            if metadata.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0 {
                pending.push((path, depth.saturating_add(1)));
            } else {
                require_single_link_regular(&metadata)?;
            }
        }
    }
    Ok((identity, true))
}

struct PreparedMountGrant {
    path: Vec<u16>,
    identity: FileIdentity,
    materialized: MaterializedMount,
    permissions: u32,
    inheritance: windows::Win32::Security::ACE_FLAGS,
}

fn prepare_mount_grant(mount: &Mount) -> Result<PreparedMountGrant> {
    mount.validate()?;
    if !mount.source.is_absolute() {
        return Err(Error::invalid(
            "Windows sandbox mount source must be absolute",
        ));
    }
    let spelling = mount.source.as_os_str().to_string_lossy();
    let lower = spelling.to_ascii_lowercase();
    if lower.starts_with(r"\\") || lower.starts_with(r"\\?\") || lower.starts_with(r"\\.\") {
        return Err(Error::new(
            ErrorCode::PolicyDenied,
            "UNC, extended and device sandbox mount paths are not accepted",
        ));
    }
    if mount.source.components().count() <= 2 {
        return Err(Error::new(
            ErrorCode::PolicyDenied,
            "Windows sandbox mounts may not expose an entire volume root",
        ));
    }
    let (identity, is_directory) = validate_mount_tree(&mount.source)?;
    let materialized_path = mount
        .source
        .to_str()
        .ok_or_else(|| Error::invalid("Windows sandbox mount path must be Unicode"))?
        .to_owned();
    let mut permissions = FILE_GENERIC_READ.0;
    if mount.execute {
        permissions |= FILE_GENERIC_EXECUTE.0;
    }
    if !mount.read_only {
        permissions |= FILE_GENERIC_WRITE.0;
        if is_directory {
            permissions |= FILE_DELETE_CHILD.0;
        }
    }
    Ok(PreparedMountGrant {
        path: wide_null(mount.source.as_os_str())?,
        identity,
        materialized: MaterializedMount {
            class: mount.class,
            logical_name: mount.logical_name.clone(),
            path: materialized_path,
            read_only: mount.read_only,
        },
        permissions,
        inheritance: if is_directory {
            SUB_CONTAINERS_AND_OBJECTS_INHERIT
        } else {
            NO_INHERITANCE
        },
    })
}

struct WindowsMountGrant {
    path: Vec<u16>,
    sid: Vec<u8>,
    active: bool,
}

impl WindowsMountGrant {
    fn grant(prepared: &PreparedMountGrant, sid: PSID) -> Result<Self> {
        let sid_bytes = copy_sid_bytes(sid)?;
        let owned_sid = PSID(sid_bytes.as_ptr().cast_mut().cast());
        if dacl_has_sid(&prepared.path, owned_sid)? {
            return Err(Error::new(
                ErrorCode::SandboxDenied,
                "Fresh Windows AppContainer SID unexpectedly already has mount authority",
            ));
        }
        apply_mount_ace(
            &prepared.path,
            PSID(sid_bytes.as_ptr().cast_mut().cast()),
            GRANT_ACCESS,
            prepared.permissions,
            prepared.inheritance,
            true,
        )?;
        Ok(Self {
            path: prepared.path.clone(),
            sid: sid_bytes,
            active: true,
        })
    }

    fn revoke(&mut self) -> Result<()> {
        if !self.active {
            return Ok(());
        }
        apply_mount_ace(
            &self.path,
            PSID(self.sid.as_ptr().cast_mut().cast()),
            REVOKE_ACCESS,
            0,
            NO_INHERITANCE,
            false,
        )?;
        self.active = false;
        Ok(())
    }
}

impl Drop for WindowsMountGrant {
    fn drop(&mut self) {
        let _ = self.revoke();
    }
}

fn revoke_mount_grants(grants: &mut [WindowsMountGrant]) -> Result<()> {
    let mut first_error = None;
    for grant in grants.iter_mut().rev() {
        if let Err(error) = grant.revoke()
            && first_error.is_none()
        {
            first_error = Some(error);
        }
    }
    if let Some(error) = first_error {
        Err(error)
    } else {
        Ok(())
    }
}

fn prepare_windows_mounts(
    spec: &SandboxSpec,
    profile: &AppContainerProfile,
) -> Result<(Vec<WindowsMountGrant>, Option<String>)> {
    if spec.mounts.is_empty() {
        return Ok((Vec::new(), None));
    }
    if spec.kind != semwright_platform_api::launch::SandboxKind::Driver {
        return Err(Error::new(
            ErrorCode::SandboxDenied,
            "Windows filesystem grants are currently limited to Driver children with mount-table semantics",
        ));
    }
    let mut seen_sources = BTreeSet::new();
    let mut prepared = Vec::with_capacity(spec.mounts.len());
    let mut materialized = Vec::with_capacity(spec.mounts.len());
    for mount in &spec.mounts {
        let plan = prepare_mount_grant(mount)?;
        if !seen_sources.insert(plan.identity) {
            return Err(Error::invalid(
                "Windows sandbox mount sources must refer to unique filesystem objects",
            ));
        }
        materialized.push(plan.materialized.clone());
        prepared.push(plan);
    }
    let encoded = encode_materialized_mounts(&materialized)?;
    let mut grants = Vec::with_capacity(prepared.len());
    for plan in &prepared {
        match WindowsMountGrant::grant(plan, profile.sid) {
            Ok(grant) => grants.push(grant),
            Err(error) => {
                if revoke_mount_grants(&mut grants).is_err() {
                    return Err(Error::new(
                        ErrorCode::SandboxDenied,
                        "Windows sandbox mount transaction failed and prior grants could not be fully revoked",
                    ));
                }
                return Err(error);
            }
        }
    }
    Ok((grants, Some(encoded)))
}

struct AppContainerProfile {
    name: Vec<u16>,
    sid: PSID,
    local_app_data: String,
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
        let local_app_data = match appcontainer_folder(sid) {
            Ok(path) => path,
            Err(error) => {
                // SAFETY: cleanup for resources created immediately above before ownership
                // transfers into AppContainerProfile.
                unsafe {
                    let _ = FreeSid(sid);
                    let _ = DeleteAppContainerProfile(PCWSTR(wide.as_ptr()));
                }
                return Err(error);
            }
        };
        Ok(Self {
            name: wide,
            sid,
            local_app_data,
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

fn environment_block(
    spec: &SandboxSpec,
    profile: &AppContainerProfile,
    mount_table: Option<&str>,
) -> Result<Vec<u16>> {
    let mut entries = spec.environment.clone();
    if entries.iter().any(|(name, _)| name == SANDBOX_MOUNTS_ENV) {
        return Err(Error::invalid(
            "Sandbox mount table environment is reserved to the platform host",
        ));
    }
    if let Some(table) = mount_table {
        entries.push((SANDBOX_MOUNTS_ENV.into(), table.to_owned()));
    }
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        entries.push((
            "SystemRoot".into(),
            system_root.to_string_lossy().into_owned(),
        ));
    }
    let temp = std::path::Path::new(&profile.local_app_data).join("Temp");
    std::fs::create_dir_all(&temp).map_err(|_| {
        Error::new(
            ErrorCode::SandboxDenied,
            "Windows AppContainer TEMP directory could not be prepared",
        )
    })?;
    entries.push(("LOCALAPPDATA".into(), profile.local_app_data.clone()));
    entries.push(("TEMP".into(), temp.to_string_lossy().into_owned()));
    entries.push(("TMP".into(), temp.to_string_lossy().into_owned()));
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
    // SAFETY: GetCurrentProcess returns a pseudo-handle for this process and needs no cleanup.
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
    mount_grants: Vec<WindowsMountGrant>,
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

    fn cleanup_authority(&mut self) -> Result<()> {
        let result = revoke_mount_grants(&mut self.mount_grants);
        self.mount_grants.clear();
        self.cleanup_profile();
        result
    }

    fn observed_exit(&mut self) -> Result<bool> {
        // SAFETY: process is a live owned process HANDLE.
        let wait = unsafe { WaitForSingleObject(self.process.raw(), 0) };
        if wait == WAIT_OBJECT_0 {
            self.exited = true;
            self.cleanup_authority()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

#[async_trait]
impl SandboxChildControl for NativeSandboxChild {
    fn id(&self) -> Option<u32> {
        Some(self.pid)
    }

    async fn kill(&mut self) -> Result<()> {
        if self.observed_exit()? {
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
        if self.observed_exit()? {
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
        self.cleanup_authority()
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
            let _ = self.cleanup_authority();
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
        if !spec.sealed_tools.is_empty() {
            return Err(Error::new(
                ErrorCode::SandboxDenied,
                "Windows sealed-tool mounts remain fail-closed until immutable executable grants are transactional",
            ));
        }
        if spec.network {
            return Err(Error::new(
                ErrorCode::SandboxDenied,
                "Windows sandbox network remains fail-closed until capability grants are explicit",
            ));
        }

        let profile = AppContainerProfile::create()?;
        let (mount_grants, mount_table) = prepare_windows_mounts(spec, &profile)?;
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
        let environment = environment_block(spec, &profile, mount_table.as_deref())?;
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
        .map_err(|error| {
            Error::new(
                ErrorCode::SandboxDenied,
                format!(
                    "Windows AppContainer process creation failed ({:#x})",
                    error.code().0
                ),
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
                mount_grants,
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
            "filesystem_mounts": "driver_appcontainer_sid_acl_v1",
            "plugin_mcp_mounts": "fail_closed_pending_portable_mount_lookup",
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

    #[test]
    fn windows_mount_preflight_rejects_parent_aliases() {
        let directory = tempfile::tempdir().expect("workspace directory");
        let child = directory.path().join("child");
        std::fs::create_dir(&child).expect("child directory");
        let aliased = child.join("..").join("child");
        let mount = Mount {
            source: aliased,
            class: MountClass::Workspace,
            logical_name: "fixture-data".into(),
            read_only: true,
            execute: false,
        };
        assert!(
            matches!(prepare_mount_grant(&mount), Err(error) if error.code == ErrorCode::PolicyDenied),
            "non-canonical parent aliases must fail closed"
        );
    }

    #[test]
    fn windows_mount_preflight_rejects_hardlinked_files() {
        let directory = tempfile::tempdir().expect("workspace directory");
        let source = directory.path().join("source.txt");
        let alias = directory.path().join("alias.txt");
        std::fs::write(&source, b"hardlink").expect("source file");
        std::fs::hard_link(&source, &alias).expect("hard link");
        let mount = Mount {
            source,
            class: MountClass::Workspace,
            logical_name: "fixture-data".into(),
            read_only: true,
            execute: false,
        };
        assert!(
            matches!(prepare_mount_grant(&mount), Err(error) if error.code == ErrorCode::PolicyDenied),
            "hard-linked files must not become AppContainer mounts"
        );
    }

    #[test]
    fn appcontainer_workspace_ace_is_verified_and_revoked() {
        let profile = AppContainerProfile::create().expect("AppContainer profile");
        let directory = tempfile::tempdir().expect("workspace directory");
        let mount = Mount {
            source: directory.path().to_path_buf(),
            class: MountClass::Workspace,
            logical_name: "fixture-data".into(),
            read_only: true,
            execute: false,
        };
        let prepared = prepare_mount_grant(&mount).expect("prepare workspace grant");
        assert!(
            !dacl_has_sid(&prepared.path, profile.sid).expect("initial workspace DACL"),
            "fresh unique AppContainer SID must not already own workspace authority"
        );

        let mut grant =
            WindowsMountGrant::grant(&prepared, profile.sid).expect("grant workspace authority");
        assert!(
            dacl_has_grant(
                &prepared.path,
                profile.sid,
                prepared.permissions,
                prepared.inheritance,
            )
            .expect("verify workspace grant"),
            "workspace grant must carry the requested rights and inheritance"
        );

        let inherited_file = directory.path().join("inherited.txt");
        std::fs::write(&inherited_file, b"inheritance").expect("create inherited fixture");
        let inherited_path = wide_null(inherited_file.as_os_str()).expect("wide inherited path");
        assert!(
            dacl_has_sid(&inherited_path, profile.sid).expect("verify inherited workspace ACE"),
            "files created under the active workspace grant must inherit the AppContainer SID"
        );

        grant.revoke().expect("revoke workspace authority");
        assert!(
            !dacl_has_sid(&prepared.path, profile.sid).expect("verify workspace revocation"),
            "workspace SID must be absent after revocation"
        );
        assert!(
            !dacl_has_sid(&inherited_path, profile.sid)
                .expect("verify inherited workspace revocation"),
            "revocation must remove inherited AppContainer authority from existing children"
        );
    }
}
