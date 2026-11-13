#[path = "launch/blender52_runtime.rs"]
mod blender52_runtime;
#[path = "launch/initial_attestation.rs"]
mod initial_attestation;
pub use blender52_runtime::{
    paths_overlap as windows_native_paths_overlap, stage as stage_blender52_runtime,
};

#[cfg(target_arch = "aarch64")]
use crate::pe::IMAGE_FILE_MACHINE_AMD64;
use crate::{
    identity::current_user_sid_bytes,
    job::ProcessJob,
    pe::{
        PeArchitecture, architecture_file, require_native_architecture,
        require_sealed_tool_architecture,
    },
};
use async_trait::async_trait;
use semwright_platform_api::launch::{
    ExecutableVerifier, MaterializedMount, MaterializedTool, Mount, MountClass,
    SANDBOX_HOST_TOOL_CHILD_ENV, SANDBOX_HOST_TOOL_CWD_ENV, SANDBOX_HOST_TOOL_TYPED_ARGS_ENV,
    SANDBOX_MOUNTS_ENV, SANDBOX_TOOLS_ENV, SandboxChildControl, SandboxCpuAccounting,
    SandboxLauncher, SandboxProcess, SandboxSpec, SealedToolSource, decode_materialized_mounts,
    decode_materialized_tools, encode_materialized_mounts, encode_materialized_tools,
    resolve_host_tool_args,
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
    sync::Arc,
};
use tokio::{fs::File as TokioFile, process::Command};
#[cfg(target_arch = "aarch64")]
use windows::Win32::System::Threading::{GetMachineTypeAttributes, UserEnabled};
use windows::Win32::{
    Foundation::{
        CloseHandle, DUPLICATE_SAME_ACCESS, DuplicateHandle, GENERIC_ALL, GENERIC_READ,
        GENERIC_WRITE, HANDLE, HANDLE_FLAG_INHERIT, HANDLE_FLAGS, HLOCAL, HWND, LocalFree,
        TRUST_E_EXPLICIT_DISTRUST, TRUST_E_NOSIGNATURE, WAIT_OBJECT_0,
    },
    Security::{
        ACCESS_ALLOWED_ACE, ACCESS_DENIED_ACE, ACE_HEADER, ACL, ACL_SIZE_INFORMATION,
        AclSizeInformation,
        Authorization::{
            ConvertSidToStringSidW, DENY_ACCESS, EXPLICIT_ACCESS_W, GRANT_ACCESS,
            GetNamedSecurityInfoW, GetSecurityInfo, NO_MULTIPLE_TRUSTEE, REVOKE_ACCESS,
            SE_FILE_OBJECT, SetEntriesInAclW, SetNamedSecurityInfoW, TRUSTEE_IS_SID,
            TRUSTEE_IS_USER, TRUSTEE_W,
        },
        CopySid, CreateWellKnownSid, DACL_SECURITY_INFORMATION, DeriveCapabilitySidsFromName,
        EqualSid, FreeSid, GetAce, GetAclInformation, GetLengthSid, GetSecurityDescriptorControl,
        Isolation::{
            CreateAppContainerProfile, DeleteAppContainerProfile, GetAppContainerFolderPath,
        },
        NO_INHERITANCE, OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
        PSECURITY_DESCRIPTOR, PSID, SE_DACL_PROTECTED, SECURITY_ATTRIBUTES, SECURITY_CAPABILITIES,
        SECURITY_MAX_SID_SIZE, SID_AND_ATTRIBUTES, SUB_CONTAINERS_AND_OBJECTS_INHERIT,
        WinBuiltinAdministratorsSid, WinCapabilityInternetClientSid, WinLocalSystemSid,
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
        FILE_READ_DATA, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        FILE_WRITE_ATTRIBUTES, FILE_WRITE_DATA, FILE_WRITE_EA, GetFileInformationByHandle,
        OPEN_EXISTING, WRITE_DAC, WRITE_OWNER,
    },
    System::{
        Com::CoTaskMemFree,
        Pipes::CreatePipe,
        SystemServices::{
            ACCESS_ALLOWED_ACE_TYPE, ACCESS_ALLOWED_CALLBACK_ACE_TYPE,
            ACCESS_ALLOWED_CALLBACK_OBJECT_ACE_TYPE, ACCESS_ALLOWED_OBJECT_ACE_TYPE,
            ACCESS_DENIED_ACE_TYPE, ACCESS_DENIED_CALLBACK_ACE_TYPE,
            ACCESS_DENIED_CALLBACK_OBJECT_ACE_TYPE, ACCESS_DENIED_OBJECT_ACE_TYPE,
            SE_GROUP_ENABLED,
        },
        Threading::{
            CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT,
            DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT, GetCurrentProcess,
            GetExitCodeProcess, INFINITE, InitializeProcThreadAttributeList,
            LPPROC_THREAD_ATTRIBUTE_LIST, PROC_THREAD_ATTRIBUTE_ALL_APPLICATION_PACKAGES_POLICY,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES,
            PROCESS_INFORMATION, ResumeThread, STARTF_USESTDHANDLES, STARTUPINFOEXW,
            TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject,
        },
        WindowsProgramming::PROCESS_CREATION_ALL_APPLICATION_PACKAGES_OPT_OUT,
    },
};
use windows::core::{BOOL, PCWSTR, PWSTR};

const MAX_EXECUTABLE: u64 = 64 * 1024 * 1024;
const MAX_APPLICATION_EXECUTABLE: u64 = 512 * 1024 * 1024;

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

fn free_derived_sid_array(array: *mut PSID, count: u32) {
    if array.is_null() {
        return;
    }
    // Capability derivation is expected to return a tiny list. Bound traversal defensively;
    // leaking an impossible oversized OS allocation is preferable to trusting an absurd count.
    let bounded = count.min(64) as usize;
    // SAFETY: DeriveCapabilitySidsFromName returns at least count LocalAlloc-owned SID
    // pointers followed by a LocalAlloc-owned pointer array on success.
    unsafe {
        for sid in std::slice::from_raw_parts(array, bounded) {
            if !sid.is_invalid() {
                let _ = LocalFree(Some(HLOCAL(sid.0)));
            }
        }
        let _ = LocalFree(Some(HLOCAL(array.cast())));
    }
}

fn named_capability_sid(name: &str) -> Result<Vec<u8>> {
    let wide = wide_null(OsStr::new(name))?;
    let mut group_sids: *mut PSID = std::ptr::null_mut();
    let mut group_count = 0u32;
    let mut capability_sids: *mut PSID = std::ptr::null_mut();
    let mut capability_count = 0u32;
    // SAFETY: all out-pointers reference live locals and wide is NUL-terminated.
    let derived = unsafe {
        DeriveCapabilitySidsFromName(
            PCWSTR(wide.as_ptr()),
            &mut group_sids,
            &mut group_count,
            &mut capability_sids,
            &mut capability_count,
        )
    };
    let result = match derived {
        Ok(()) if capability_count == 1 && !capability_sids.is_null() => {
            // SAFETY: the API reported exactly one capability SID.
            copy_sid_bytes(unsafe { *capability_sids })
        }
        Ok(()) => Err(Error::new(
            ErrorCode::SandboxDenied,
            format!("Windows capability {name} did not resolve to exactly one SID"),
        )),
        Err(_) => Err(Error::new(
            ErrorCode::SandboxDenied,
            format!("Windows capability {name} could not be resolved"),
        )),
    };
    free_derived_sid_array(group_sids, group_count);
    free_derived_sid_array(capability_sids, capability_count);
    result
}

fn sid_matches(sid: PSID, expected: &[u8]) -> bool {
    if sid.is_invalid() || expected.is_empty() {
        return false;
    }
    // SAFETY: sid belongs to the live security descriptor; expected is stable for this call.
    unsafe { EqualSid(sid, PSID(expected.as_ptr().cast_mut().cast())).is_ok() }
}

/// The Windows FILE_GENERIC_WRITE preset also contains READ_CONTROL and
/// SYNCHRONIZE, which are shared with FILE_GENERIC_READ/EXECUTE. Testing an
/// allow ACE by intersection with that composite falsely interprets normal
/// untrusted read/execute access on a vendor-installed application as write
/// authority. Test ONLY actual mutation and unmapped generic write bits.
fn untrusted_acl_restricted_mask(private_data: bool) -> u32 {
    let mutation = FILE_WRITE_DATA.0
        | FILE_APPEND_DATA.0
        | FILE_WRITE_EA.0
        | FILE_WRITE_ATTRIBUTES.0
        | DELETE.0
        | WRITE_DAC.0
        | WRITE_OWNER.0
        | GENERIC_WRITE.0
        | GENERIC_ALL.0;
    let confidential = FILE_READ_DATA.0 | FILE_GENERIC_READ.0 | GENERIC_READ.0 | GENERIC_ALL.0;
    if private_data {
        mutation | confidential
    } else {
        mutation
    }
}

/// Choose the accepted ACL authority explicitly. Vendor-installed native
/// runtimes may be serviced by exactly NT SERVICE\\TrustedInstaller; this
/// exception must never extend to private user data or generic executables.
#[derive(Clone, Copy, PartialEq, Eq)]
enum WindowsAclPurpose {
    PrivateData,
    Executable,
    NativeRuntime,
}

fn trusted_installer_sid_bytes() -> Vec<u8> {
    // Exact NT SERVICE\\TrustedInstaller service SID, not an S-1-5-80
    // service-SID prefix or a generic Windows service group.
    // S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464.
    let mut sid = vec![1u8, 6, 0, 0, 0, 0, 0, 5];
    for part in [
        80u32,
        956_008_885,
        3_418_522_649,
        1_831_038_044,
        1_853_292_631,
        2_271_478_464,
    ] {
        sid.extend_from_slice(&part.to_le_bytes());
    }
    sid
}

fn verify_trusted_file_acl(file: &File, purpose: WindowsAclPurpose) -> Result<()> {
    let (kind, private_data) = match purpose {
        WindowsAclPurpose::PrivateData => ("private data file", true),
        WindowsAclPurpose::Executable => ("executable", false),
        WindowsAclPurpose::NativeRuntime => ("native runtime", false),
    };
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
            format!("Windows {kind} security descriptor could not be verified"),
        ));
    }
    let _descriptor = SecurityDescriptor(descriptor);
    if owner.is_invalid() || dacl.is_null() {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            format!("Windows {kind} must have an explicit trusted owner and DACL"),
        ));
    }

    let current = current_user_sid_bytes()?;
    let system = well_known_sid(WinLocalSystemSid)?;
    let admins = well_known_sid(WinBuiltinAdministratorsSid)?;
    let trusted = [&current[..], &system[..], &admins[..]];
    let native_installer =
        (purpose == WindowsAclPurpose::NativeRuntime).then(trusted_installer_sid_bytes);
    // TrustedInstaller can service a vendor runtime, but may NOT become the
    // owner of Semwright private data or the broad executable verifier.
    if !trusted.iter().any(|expected| sid_matches(owner, expected)) {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            format!("Windows {kind} owner is not the current user, SYSTEM or Administrators"),
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
            format!("Windows {kind} DACL is invalid"),
        )
    })?;
    if acl_info.AceCount > 4_096 {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            format!("Windows {kind} DACL exceeds verification budget"),
        ));
    }

    let restricted = untrusted_acl_restricted_mask(private_data);

    for index in 0..acl_info.AceCount {
        let mut raw: *mut core::ffi::c_void = std::ptr::null_mut();
        // SAFETY: the DACL is retained by the security-descriptor guard, index is bounded
        // by AceCount, and raw is a valid writable out-pointer for the ACE address.
        unsafe { GetAce(dacl, index, &mut raw) }.map_err(|_| {
            Error::new(
                ErrorCode::PermissionDenied,
                format!("Windows {kind} DACL entry could not be inspected"),
            )
        })?;
        if raw.is_null() {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                format!("Windows {kind} DACL contains a null ACE"),
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
                    format!("Windows {kind} DACL contains a malformed allow ACE"),
                ));
            }
            // Every allow ACE layout starts with ACE_HEADER followed by the access mask.
            // SAFETY: the common prefix size was checked above.
            let ace = unsafe { &*(raw.cast::<ACCESS_ALLOWED_ACE>()) };
            if ace.Mask & restricted == 0 {
                continue;
            }
            if ace_type != ACCESS_ALLOWED_ACE_TYPE {
                return Err(Error::new(
                    ErrorCode::PermissionDenied,
                    format!("Complex Windows {kind} restricted ACEs are fail-closed"),
                ));
            }
            // An INHERIT_ONLY ACE (0x08) grants no access to this handle.
            // The native copy verifies every descendant ACL separately;
            // skipping this template does not authorize creating children.
            // Unknown/complex allow ACE types still fail closed above.
            const INHERIT_ONLY_ACE_FLAG: u8 = 0x08;
            if header.AceFlags & INHERIT_ONLY_ACE_FLAG != 0 {
                continue;
            }
            let sid = PSID((&ace.SidStart as *const u32).cast_mut().cast());
            if !trusted.iter().any(|expected| sid_matches(sid, expected))
                && !native_installer
                    .as_ref()
                    .is_some_and(|expected| sid_matches(sid, expected))
            {
                return Err(Error::new(
                    ErrorCode::PermissionDenied,
                    format!(
                        "Windows {kind} DACL grants restricted rights to an untrusted principal"
                    ),
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
                    format!("Unknown Windows {kind} DACL ACE type is fail-closed"),
                ));
            }
        }
    }
    Ok(())
}

fn verify_materialized_sealed_tool(path: &Path, digest: &str) -> Result<()> {
    if !path.is_absolute() {
        return Err(Error::invalid(
            "Materialized Windows sealed tool path must be absolute",
        ));
    }
    let mut options = std::fs::OpenOptions::new();
    options
        .read(true)
        .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_DELETE.0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0);
    let mut file = options.open(path)?;
    let before = info(&file)?;
    if before.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
        || before.nNumberOfLinks != 1
        || before.nFileSizeHigh != 0
        || before.nFileSizeLow as u64 > MAX_EXECUTABLE
    {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Unsafe materialized Windows sealed tool type, link count or size",
        ));
    }
    let mut bytes = Vec::with_capacity(before.nFileSizeLow as usize);
    file.by_ref()
        .take(MAX_EXECUTABLE + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_EXECUTABLE {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Materialized Windows sealed tool exceeds size budget",
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
            "Materialized Windows sealed tool changed while being verified",
        ));
    }
    let expected = digest.trim().to_ascii_lowercase();
    if expected.len() != 64 || hex::encode(Sha256::digest(&bytes)) != expected {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Materialized Windows sealed tool digest mismatch",
        ));
    }
    require_sealed_tool_architecture(&bytes)?;
    require_authenticode_policy(authenticode_status(&file, path)?)?;
    Ok(())
}

pub fn verify_private_data_file(path: &Path, max_bytes: u64) -> Result<()> {
    // Project Graph stores may use up to 64 MiB; caller-specific budgets such
    // as sealed driver state stay at their explicit smaller limits.
    if max_bytes == 0 || max_bytes > 64 * 1024 * 1024 {
        return Err(Error::invalid(
            "Windows private-file verification budget is invalid",
        ));
    }
    if !path.is_absolute() {
        return Err(Error::invalid(
            "Windows private data file path must be absolute",
        ));
    }
    let spelling = path.as_os_str().to_string_lossy().to_ascii_lowercase();
    if spelling.starts_with(r"\\") || spelling.starts_with(r"\\?\") || spelling.starts_with(r"\\.\")
    {
        return Err(Error::new(
            ErrorCode::PolicyDenied,
            "UNC, extended and device private-data paths are not accepted",
        ));
    }

    let (validated_identity, is_directory) = match validate_mount_tree(path) {
        Ok(validated) => validated,
        Err(error) if error.code == ErrorCode::PolicyDenied => {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "Unsafe Windows private data source path, type or link count",
            ));
        }
        Err(error) => return Err(error),
    };
    if is_directory {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Windows private data source must be a regular file",
        ));
    }

    let mut options = std::fs::OpenOptions::new();
    options
        .read(true)
        .share_mode(FILE_SHARE_READ.0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0);
    let mut file = options.open(path)?;
    let before = info(&file)?;
    if file_identity(&before) != validated_identity {
        return Err(Error::new(
            ErrorCode::Conflict,
            "Windows private data source identity changed during validation",
        ));
    }
    verify_trusted_file_acl(&file, WindowsAclPurpose::PrivateData)?;
    let size = (u64::from(before.nFileSizeHigh) << 32) | u64::from(before.nFileSizeLow);
    if before.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
        || before.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0
        || before.nNumberOfLinks != 1
        || size == 0
        || size > max_bytes
    {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Unsafe Windows private data file type, link count or size",
        ));
    }

    let mut bytes = Vec::with_capacity(size as usize);
    file.by_ref().take(max_bytes + 1).read_to_end(&mut bytes)?;
    if bytes.is_empty() || bytes.len() as u64 > max_bytes {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Windows private data file exceeds verification budget",
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
            "Windows private data file changed while it was being verified",
        ));
    }
    Ok(())
}

fn verify_windows_executable(
    path: &Path,
    digest: &str,
    sealed_tool: bool,
    max_bytes: u64,
) -> Result<Vec<u8>> {
    if !path.is_absolute() {
        return Err(Error::invalid(
            "Pinned Windows executable path must be absolute",
        ));
    }
    let spelling = path.as_os_str().to_string_lossy().to_ascii_lowercase();
    if spelling.starts_with(r"\\") || spelling.starts_with(r"\\?\") || spelling.starts_with(r"\\.\")
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
    verify_trusted_file_acl(&file, WindowsAclPurpose::Executable)?;
    if before.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
        || before.nNumberOfLinks != 1
        || before.nFileSizeHigh != 0
        || before.nFileSizeLow as u64 > max_bytes
    {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Unsafe Windows executable type, link count or size",
        ));
    }
    let mut bytes = Vec::with_capacity(before.nFileSizeLow as usize);
    file.by_ref().take(max_bytes + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max_bytes {
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
    if expected.len() != 64 || hex::encode(Sha256::digest(&bytes)) != expected {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Executable digest mismatch",
        ));
    }
    if sealed_tool {
        require_sealed_tool_architecture(&bytes)?;
    } else {
        require_native_architecture(&bytes)?;
    }
    require_authenticode_policy(authenticode_status(&file, path)?)?;
    Ok(bytes)
}

pub fn verify_sealed_tool_executable(path: &Path, digest: &str) -> Result<Vec<u8>> {
    verify_windows_executable(path, digest, true, MAX_EXECUTABLE)
}

pub fn verify_application_executable(path: &Path, digest: &str) -> Result<()> {
    verify_windows_executable(path, digest, false, MAX_APPLICATION_EXECUTABLE).map(|_| ())
}

pub struct WindowsVerifier;
impl ExecutableVerifier for WindowsVerifier {
    fn verify(&self, path: &Path, digest: &str) -> Result<Vec<u8>> {
        verify_windows_executable(path, digest, false, MAX_EXECUTABLE)
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

fn explicit_sid_grant(
    sid: PSID,
    permissions: u32,
    inheritance: windows::Win32::Security::ACE_FLAGS,
) -> EXPLICIT_ACCESS_W {
    EXPLICIT_ACCESS_W {
        grfAccessPermissions: permissions,
        grfAccessMode: GRANT_ACCESS,
        grfInheritance: inheritance,
        Trustee: TRUSTEE_W {
            pMultipleTrustee: std::ptr::null_mut(),
            MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_USER,
            ptstrName: PWSTR(sid.0.cast()),
        },
    }
}

fn protected_profile_acl_matches(
    path: &[u16],
    app_sid: PSID,
    permissions: u32,
    forbidden_permissions: u32,
    inheritance: windows::Win32::Security::ACE_FLAGS,
) -> Result<bool> {
    let (dacl, descriptor) = named_dacl(path)?;
    let mut control = 0u16;
    let mut revision = 0u32;
    // SAFETY: descriptor is live for this call and both outputs are writable locals.
    unsafe { GetSecurityDescriptorControl(descriptor.0, &mut control, &mut revision) }.map_err(
        |_| {
            Error::new(
                ErrorCode::SandboxDenied,
                "Windows sealed-tool protected DACL state could not be read",
            )
        },
    )?;
    if control & SE_DACL_PROTECTED.0 == 0 {
        return Ok(false);
    }

    let current = current_user_sid_bytes()?;
    let system = well_known_sid(WinLocalSystemSid)?;
    let admins = well_known_sid(WinBuiltinAdministratorsSid)?;
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
            "Windows sealed-tool protected DACL is invalid",
        )
    })?;
    if acl_info.AceCount > 16 {
        return Ok(false);
    }

    let inheritance_mask = SUB_CONTAINERS_AND_OBJECTS_INHERIT.0;
    let mut app_seen = false;
    for index in 0..acl_info.AceCount {
        let mut raw: *mut core::ffi::c_void = std::ptr::null_mut();
        // SAFETY: index is bounded by AceCount and raw is a writable ACE out-pointer.
        unsafe { GetAce(dacl, index, &mut raw) }.map_err(|_| {
            Error::new(
                ErrorCode::SandboxDenied,
                "Windows sealed-tool protected DACL entry could not be inspected",
            )
        })?;
        if raw.is_null() {
            return Ok(false);
        }
        // SAFETY: GetAce returned storage owned by the live DACL/security descriptor.
        let header = unsafe { &*(raw.cast::<ACE_HEADER>()) };
        if header.AceType as u32 != ACCESS_ALLOWED_ACE_TYPE
            || usize::from(header.AceSize) < std::mem::size_of::<ACCESS_ALLOWED_ACE>()
        {
            return Ok(false);
        }
        // SAFETY: a simple access-allowed ACE is at least ACCESS_ALLOWED_ACE bytes.
        let ace = unsafe { &*(raw.cast::<ACCESS_ALLOWED_ACE>()) };
        let sid = PSID((&ace.SidStart as *const u32).cast_mut().cast());
        // SAFETY: both SIDs are live for this comparison.
        if unsafe { EqualSid(sid, app_sid).is_ok() } {
            if ace.Mask & permissions != permissions
                || ace.Mask & forbidden_permissions != 0
                || u32::from(header.AceFlags) & inheritance_mask != inheritance.0
            {
                return Ok(false);
            }
            app_seen = true;
            continue;
        }
        if sid_matches(sid, &current) || sid_matches(sid, &system) || sid_matches(sid, &admins) {
            continue;
        }
        return Ok(false);
    }
    Ok(app_seen)
}

fn set_protected_profile_acl(
    path: &[u16],
    app_sid: PSID,
    permissions: u32,
    forbidden_permissions: u32,
    inheritance: windows::Win32::Security::ACE_FLAGS,
) -> Result<()> {
    let current = current_user_sid_bytes()?;
    let system = well_known_sid(WinLocalSystemSid)?;
    let admins = well_known_sid(WinBuiltinAdministratorsSid)?;
    let entries = [
        explicit_sid_grant(
            PSID(current.as_ptr().cast_mut().cast()),
            GENERIC_ALL.0,
            NO_INHERITANCE,
        ),
        explicit_sid_grant(
            PSID(system.as_ptr().cast_mut().cast()),
            GENERIC_ALL.0,
            NO_INHERITANCE,
        ),
        explicit_sid_grant(
            PSID(admins.as_ptr().cast_mut().cast()),
            GENERIC_ALL.0,
            NO_INHERITANCE,
        ),
        explicit_sid_grant(app_sid, permissions, inheritance),
    ];
    let mut updated: *mut ACL = std::ptr::null_mut();
    // SAFETY: all SID buffers and entries remain live through this synchronous call.
    let status = unsafe { SetEntriesInAclW(Some(&entries), None, &mut updated) };
    if status.0 != 0 || updated.is_null() {
        return Err(Error::new(
            ErrorCode::SandboxDenied,
            "Windows sealed-tool protected DACL could not be constructed",
        ));
    }
    let updated = LocalAcl(updated);
    // SAFETY: path is NUL-terminated and updated contains a valid ACL allocated above.
    let status = unsafe {
        SetNamedSecurityInfoW(
            PCWSTR(path.as_ptr()),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(updated.0),
            None,
        )
    };
    if status.0 != 0 {
        return Err(Error::new(
            ErrorCode::SandboxDenied,
            "Windows sealed-tool protected DACL could not be applied",
        ));
    }
    if !protected_profile_acl_matches(
        path,
        app_sid,
        permissions,
        forbidden_permissions,
        inheritance,
    )? {
        return Err(Error::new(
            ErrorCode::SandboxDenied,
            "Windows sealed-tool protected DACL verification failed",
        ));
    }
    Ok(())
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

fn dacl_has_deny(
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
        if header.AceType as u32 != ACCESS_DENIED_ACE_TYPE
            || usize::from(header.AceSize) < std::mem::size_of::<ACCESS_DENIED_ACE>()
        {
            continue;
        }
        // SAFETY: a simple access-denied ACE is at least ACCESS_DENIED_ACE bytes.
        let ace = unsafe { &*(raw.cast::<ACCESS_DENIED_ACE>()) };
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

fn revoke_mount_sid(path: &[u16], sid: PSID) -> Result<()> {
    set_mount_ace(path, sid, REVOKE_ACCESS, 0, NO_INHERITANCE)?;
    if dacl_has_sid(path, sid)? {
        return Err(Error::new(
            ErrorCode::SandboxDenied,
            "Windows sandbox mount SID revocation could not be proven",
        ));
    }
    Ok(())
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
            format!(
                "Windows sandbox mount ACE could not be constructed (Win32 status {}, null ACL {})",
                status.0,
                updated.is_null()
            ),
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
            format!(
                "Windows sandbox mount ACE could not be applied (Win32 status {})",
                status.0
            ),
        ));
    }
    Ok(())
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
    denied_permissions: u32,
    inheritance: windows::Win32::Security::ACE_FLAGS,
}

fn prepare_mount_grant(mount: &Mount) -> Result<PreparedMountGrant> {
    mount.validate()?;
    if mount.class == MountClass::Secret {
        verify_private_data_file(&mount.source, 4096)?;
    }
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
    let mut denied_permissions = WRITE_DAC.0 | WRITE_OWNER.0;
    if !mount.read_only {
        permissions |= FILE_GENERIC_WRITE.0;
        if is_directory {
            permissions |= FILE_DELETE_CHILD.0;
        }
    } else {
        // A specific deny for the unique AppContainer SID prevents broad group ACEs
        // (for example an application-package group) from accidentally upgrading a
        // read-only grant into mutation authority.
        denied_permissions |= FILE_WRITE_DATA.0
            | FILE_APPEND_DATA.0
            | FILE_WRITE_EA.0
            | FILE_WRITE_ATTRIBUTES.0
            | FILE_DELETE_CHILD.0
            | DELETE.0;
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
        denied_permissions,
        inheritance: if is_directory {
            SUB_CONTAINERS_AND_OBJECTS_INHERIT
        } else {
            NO_INHERITANCE
        },
    })
}

fn prepare_tool_staging_traverse(staging_root: &Path) -> Result<PreparedMountGrant> {
    if !staging_root.is_absolute() {
        return Err(Error::invalid(
            "Windows sealed-tool staging root must be absolute",
        ));
    }
    let (identity, is_directory) = validate_mount_tree(staging_root)?;
    if !is_directory {
        return Err(Error::new(
            ErrorCode::SandboxDenied,
            "Windows sealed-tool staging root must be a directory",
        ));
    }
    let materialized_path = staging_root
        .to_str()
        .ok_or_else(|| Error::invalid("Windows sealed-tool staging root must be Unicode"))?
        .to_owned();
    Ok(PreparedMountGrant {
        path: wide_null(staging_root.as_os_str())?,
        identity,
        materialized: MaterializedMount {
            class: MountClass::Workspace,
            logical_name: "__sealed-tool-staging__".into(),
            path: materialized_path,
            read_only: true,
        },
        // This is a dedicated per-profile directory containing only tools already named in
        // the host-controlled tool table. Windows image resolution may read/list this directory
        // on some hosts, so grant read + traverse while continuing to deny all mutation.
        permissions: FILE_GENERIC_READ.0 | FILE_GENERIC_EXECUTE.0,
        denied_permissions: FILE_WRITE_DATA.0
            | FILE_APPEND_DATA.0
            | FILE_WRITE_EA.0
            | FILE_WRITE_ATTRIBUTES.0
            | FILE_DELETE_CHILD.0
            | DELETE.0
            | WRITE_DAC.0
            | WRITE_OWNER.0,
        inheritance: NO_INHERITANCE,
    })
}

fn native_mount_root_security_context(root: &Path) -> serde_json::Value {
    use windows::Win32::Security::TOKEN_QUERY;
    use windows::Win32::System::Threading::{GetCurrentThread, OpenThreadToken};

    // A thread impersonation token can produce different effective access
    // from the normal broker process token used during private-tree staging.
    let mut token = HANDLE::default();
    let thread_impersonating =
        unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, true, &mut token) }.is_ok();
    if thread_impersonating {
        // SAFETY: successful OpenThreadToken transferred one owned token handle.
        unsafe {
            let _ = CloseHandle(token);
        }
    }

    let mut options = std::fs::OpenOptions::new();
    options
        .read(true)
        .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0 | FILE_SHARE_DELETE.0)
        .custom_flags((FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS).0);
    let file = match options.open(root) {
        Ok(file) => file,
        Err(error) => {
            return serde_json::json!({
                "thread_impersonation_token_present":thread_impersonating,
                "root_security_read_win32":error.raw_os_error(),
            });
        }
    };
    let owner_private_acl_verified =
        verify_trusted_file_acl(&file, WindowsAclPurpose::PrivateData).is_ok();

    let mut owner = PSID::default();
    let mut dacl: *mut ACL = std::ptr::null_mut();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: the read-only file handle remains live throughout this query.
    let result = unsafe {
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
    if result.0 != 0 || descriptor.is_invalid() {
        return serde_json::json!({
            "thread_impersonation_token_present":thread_impersonating,
            "owner_private_acl_verified":owner_private_acl_verified,
            "root_security_read_win32":result.0,
        });
    }
    let _descriptor = SecurityDescriptor(descriptor);
    let expected = current_user_sid_bytes();
    let owner_is_process_user = expected.as_ref().is_ok_and(|sid| sid_matches(owner, sid));
    let system = well_known_sid(WinLocalSystemSid);
    let owner_is_system = system.as_ref().is_ok_and(|sid| sid_matches(owner, sid));
    let admins = well_known_sid(WinBuiltinAdministratorsSid);
    let owner_is_administrators = admins.as_ref().is_ok_and(|sid| sid_matches(owner, sid));

    serde_json::json!({
        "thread_impersonation_token_present":thread_impersonating,
        "owner_private_acl_verified":owner_private_acl_verified,
        "owner_is_process_user":owner_is_process_user,
        "owner_is_system":owner_is_system,
        "owner_is_administrators":owner_is_administrators,
        "root_explicit_dacl":!dacl.is_null(),
        "root_security_read_win32":0,
    })
}

/// A bounded, read-only inspection after an owner-granted native-runtime
/// mount was denied by Windows. It NEVER alters a DACL, replays an operation,
/// follows reparse points, or publishes private paths or principal identities.
fn native_mount_write_dac_audit(root: &Path) -> serde_json::Value {
    use std::collections::BTreeMap;
    use std::os::windows::fs::MetadataExt;
    use std::time::{Duration, Instant};

    let started = Instant::now();
    let deadline = started + Duration::from_secs(12);
    let mut pending = vec![root.to_path_buf()];
    let mut checked = 0usize;
    let mut directories = 0usize;
    let mut failures = BTreeMap::<i32, usize>::new();
    let mut metadata_errors = 0usize;
    let mut enumeration_errors = 0usize;
    let mut refused_reparse = 0usize;
    let mut root_write_dac_status = None;
    const MAX_AUDIT_ENTRIES: usize = 10_000;
    while let Some(path) = pending.pop() {
        if checked >= MAX_AUDIT_ENTRIES || Instant::now() >= deadline {
            pending.push(path);
            break;
        }
        checked += 1;
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(value) => value,
            Err(_) => {
                metadata_errors += 1;
                continue;
            }
        };
        if metadata.file_type().is_symlink()
            || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
        {
            refused_reparse += 1;
            continue;
        }
        let mut options = std::fs::OpenOptions::new();
        options
            .read(true)
            .access_mode(WRITE_DAC.0)
            .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0 | FILE_SHARE_DELETE.0)
            .custom_flags((FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS).0);
        let status = match options.open(&path) {
            Ok(file) => {
                drop(file);
                0
            }
            Err(error) => {
                let status = error.raw_os_error().unwrap_or(-1);
                *failures.entry(status).or_default() += 1;
                status
            }
        };
        if checked == 1 {
            root_write_dac_status = Some(status);
        }
        if metadata.is_dir() {
            directories += 1;
            match std::fs::read_dir(&path) {
                Ok(entries) => {
                    for entry in entries {
                        match entry {
                            Ok(entry) => pending.push(entry.path()),
                            Err(_) => enumeration_errors += 1,
                        }
                    }
                }
                Err(_) => enumeration_errors += 1,
            }
        }
    }
    serde_json::json!({
        "root_write_dac_status":root_write_dac_status,
        "root_security_context":native_mount_root_security_context(root),
        "scanned":checked,
        "directories":directories,
        "write_dac_failures_by_win32":failures,
        "metadata_errors":metadata_errors,
        "enumeration_errors":enumeration_errors,
        "refused_reparse_points":refused_reparse,
        "bounded":!pending.is_empty(),
        "elapsed_ms":started.elapsed().as_millis(),
    })
}

struct WindowsMountGrant {
    path: Vec<u16>,
    sid: Vec<u8>,
    active: bool,
    require_sid_absence_after_revoke: bool,
}

impl WindowsMountGrant {
    fn grant(prepared: &PreparedMountGrant, sid: PSID) -> Result<Self> {
        let sid_bytes = copy_sid_bytes(sid)?;
        let owned_sid = PSID(sid_bytes.as_ptr().cast_mut().cast());
        if prepared.materialized.logical_name == "blender-runtime" {
            // This is a one-time, read-only pre-grant baseline inside the
            // broker's actual process security context. Compare it against
            // the identical probe on the denied mount (performed before
            // rollback), without revealing private paths or any SID.
            eprintln!(
                "{}",
                serde_json::json!({
                    "component":"platform-windows",
                    "phase":"native_runtime_mount_before_grant",
                    "logical_mount":"blender-runtime",
                    "observation":native_mount_write_dac_audit(
                        Path::new(&prepared.materialized.path)
                    ),
                })
            );
        }
        if dacl_has_sid(&prepared.path, owned_sid)? {
            return Err(Error::new(
                ErrorCode::SandboxDenied,
                "Fresh Windows AppContainer SID unexpectedly already has mount authority",
            ));
        }
        // Deny mutation rights for this exact AppContainer SID before granting the
        // requested authority. This keeps a broad allow ACE on a package/group from
        // silently widening a read-only mount. SetEntriesInAcl canonicalizes deny ACEs
        // ahead of allow ACEs in the DACL.
        if let Err(error) = set_mount_ace(
            &prepared.path,
            owned_sid,
            DENY_ACCESS,
            prepared.denied_permissions,
            prepared.inheritance,
        ) {
            // Avoid exposing private filesystem paths, principal SIDs or
            // AppContainer tokens in diagnostic logs.
            eprintln!(
                "{}",
                serde_json::json!({
                    "component": "platform-windows",
                    "phase": "sandbox_mount_ace_rejected",
                    "logical_mount": prepared.materialized.logical_name.as_str(),
                    "operation": "deny",
                    "error": error.to_string(),
                })
            );
            if prepared.materialized.logical_name == "blender-runtime" {
                // The child is not launched. Read only whether the broker can
                // open the failed private snapshot and its descendants with
                // WRITE_DAC; never log filenames, paths, or security IDs.
                eprintln!(
                    "{}",
                    serde_json::json!({
                        "component":"platform-windows",
                        "phase":"native_runtime_mount_dacl_diagnosis",
                        "logical_mount":"blender-runtime",
                        "observation":native_mount_write_dac_audit(
                            Path::new(&prepared.materialized.path)
                        ),
                    })
                );
            }
            let _ = revoke_mount_sid(&prepared.path, owned_sid);
            return Err(error);
        }
        if !dacl_has_deny(
            &prepared.path,
            owned_sid,
            prepared.denied_permissions,
            prepared.inheritance,
        )? {
            revoke_mount_sid(&prepared.path, owned_sid)?;
            return Err(Error::new(
                ErrorCode::SandboxDenied,
                "Windows sandbox mount deny ACE verification failed",
            ));
        }
        if let Err(error) = set_mount_ace(
            &prepared.path,
            owned_sid,
            GRANT_ACCESS,
            prepared.permissions,
            prepared.inheritance,
        ) {
            eprintln!(
                "{}",
                serde_json::json!({
                    "component": "platform-windows",
                    "phase": "sandbox_mount_ace_rejected",
                    "logical_mount": prepared.materialized.logical_name.as_str(),
                    "operation": "allow",
                    "error": error.to_string(),
                })
            );
            let _ = revoke_mount_sid(&prepared.path, owned_sid);
            return Err(error);
        }
        if !dacl_has_grant(
            &prepared.path,
            owned_sid,
            prepared.permissions,
            prepared.inheritance,
        )? || !dacl_has_deny(
            &prepared.path,
            owned_sid,
            prepared.denied_permissions,
            prepared.inheritance,
        )? {
            revoke_mount_sid(&prepared.path, owned_sid)?;
            return Err(Error::new(
                ErrorCode::SandboxDenied,
                "Windows sandbox mount allow/deny ACE verification failed",
            ));
        }
        Ok(Self {
            path: prepared.path.clone(),
            sid: sid_bytes,
            active: true,
            require_sid_absence_after_revoke: true,
        })
    }

    fn narrow_profile_authority(prepared: &PreparedMountGrant, sid: PSID) -> Result<Self> {
        let sid_bytes = copy_sid_bytes(sid)?;
        let owned_sid = PSID(sid_bytes.as_ptr().cast_mut().cast());
        // These objects are created inside the unique, disposable AppContainer profile.
        // Replace inherited profile ACLs entirely so broad package-group authority cannot
        // widen a sealed tool beyond the exact read/execute (or traverse) grant.
        set_protected_profile_acl(
            &prepared.path,
            owned_sid,
            prepared.permissions,
            prepared.denied_permissions,
            prepared.inheritance,
        )?;
        Ok(Self {
            path: prepared.path.clone(),
            sid: sid_bytes,
            active: true,
            require_sid_absence_after_revoke: true,
        })
    }

    fn revoke(&mut self) -> Result<()> {
        if !self.active {
            return Ok(());
        }
        let sid = PSID(self.sid.as_ptr().cast_mut().cast());
        if self.require_sid_absence_after_revoke {
            revoke_mount_sid(&self.path, sid)?;
        } else {
            // Profile-local objects inherit the AppContainer SID from the unique LPAC profile.
            // Remove only Semwright's explicit narrowing ACEs; inherited profile authority is
            // intentionally left for DeleteAppContainerProfile to dispose with the profile.
            set_mount_ace(&self.path, sid, REVOKE_ACCESS, 0, NO_INHERITANCE)?;
        }
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

/// A native Blender writer uses a new .blend@ file before publishing the
/// .blend from an LPAC child token. Windows checks both the user and the
/// AppContainer SID. A private workspace may grant the user Full Control
/// without inheritance; inherited AppContainer ACEs alone cannot authorize
/// the native file operation.
///
/// This strictly scoped native Blender writable-workspace fix requires an
/// already owner-approved directory and an existing full-control owner ACE.
/// It only extends those EXISTING user rights to the owner's newly generated
/// files. It does not change the allowed AppContainer rights, other driver
/// mounts, the native executable, or the installed Blender vendor tree.
/// Owner inheritance remains so written artifacts can be read after the
/// unique temporary AppContainer SID is revoked.
fn ensure_native_blender_workspace_owner_inheritance(prepared: &PreparedMountGrant) -> Result<()> {
    if prepared.materialized.logical_name != "workspace"
        || prepared.materialized.read_only
        || prepared.inheritance != SUB_CONTAINERS_AND_OBJECTS_INHERIT
    {
        return Err(Error::new(
            ErrorCode::PolicyDenied,
            "Native Blender owner inheritance requires an authorized writable workspace directory",
        ));
    }
    let mut owner = PSID::default();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: the path is NUL-terminated and both output slots are live.
    let status = unsafe {
        GetNamedSecurityInfoW(
            PCWSTR(prepared.path.as_ptr()),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            Some(&mut owner),
            None,
            None,
            None,
            &mut descriptor,
        )
    };
    if status.0 != 0 || descriptor.is_invalid() || owner.is_invalid() {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Native Blender writable workspace owner could not be verified",
        ));
    }
    let _descriptor = SecurityDescriptor(descriptor);
    let current_user = current_user_sid_bytes()?;
    if !sid_matches(owner, &current_user) {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Native Blender writable workspace must be owned by the active user",
        ));
    }
    let user = PSID(current_user.as_ptr().cast_mut().cast());
    // The user must have Full Control already. This path may NOT turn
    // deliberately restricted owner rights into unrestricted rights.
    let full_control = FILE_GENERIC_READ.0
        | FILE_GENERIC_WRITE.0
        | FILE_GENERIC_EXECUTE.0
        | FILE_DELETE_CHILD.0
        | DELETE.0
        | WRITE_DAC.0
        | WRITE_OWNER.0;
    if dacl_has_grant(
        &prepared.path,
        user,
        full_control,
        SUB_CONTAINERS_AND_OBJECTS_INHERIT,
    )? {
        return Ok(());
    }
    if !dacl_has_grant(&prepared.path, user, full_control, NO_INHERITANCE)? {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Native Blender workspace owner must already have explicit Full Control",
        ));
    }
    set_mount_ace(
        &prepared.path,
        user,
        GRANT_ACCESS,
        full_control,
        SUB_CONTAINERS_AND_OBJECTS_INHERIT,
    )?;
    if !dacl_has_grant(
        &prepared.path,
        user,
        full_control,
        SUB_CONTAINERS_AND_OBJECTS_INHERIT,
    )? {
        return Err(Error::new(
            ErrorCode::SandboxDenied,
            "Native Blender owner inheritance could not be verified",
        ));
    }
    Ok(())
}

fn prepare_windows_mounts(
    spec: &SandboxSpec,
    profile: &AppContainerProfile,
    native_blender_initial: bool,
) -> Result<(Vec<WindowsMountGrant>, Option<String>)> {
    if spec.mounts.is_empty() {
        return Ok((Vec::new(), None));
    }
    if !matches!(
        spec.kind,
        semwright_platform_api::launch::SandboxKind::Driver
            | semwright_platform_api::launch::SandboxKind::Plugin
    ) {
        return Err(Error::new(
            ErrorCode::SandboxDenied,
            "Windows filesystem grants require Driver or Plugin SDK mount-table semantics",
        ));
    }
    if spec.kind == semwright_platform_api::launch::SandboxKind::Plugin
        && spec.mounts.iter().any(|mount| {
            mount.class != semwright_platform_api::launch::MountClass::Workspace || mount.execute
        })
    {
        return Err(Error::new(
            ErrorCode::SandboxDenied,
            "Windows Plugin mounts are workspace-only and non-executable",
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
    // Validate ALL mounts before any native-workspace ACL adjustment. Do
    // not mutate other mounts or general Windows Plugin/Driver workspaces.
    if native_blender_initial {
        let owner_workspace = prepared
            .iter()
            .find(|plan| plan.materialized.logical_name == "workspace")
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::PolicyDenied,
                    "Native Blender workspace mount missing",
                )
            })?;
        ensure_native_blender_workspace_owner_inheritance(owner_workspace)?;
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

fn prepare_windows_tools(
    spec: &SandboxSpec,
    profile: &AppContainerProfile,
) -> Result<(Vec<WindowsMountGrant>, Option<String>)> {
    if spec.sealed_tools.is_empty() {
        return Ok((Vec::new(), None));
    }
    if spec.kind != semwright_platform_api::launch::SandboxKind::Driver {
        return Err(Error::new(
            ErrorCode::SandboxDenied,
            "Windows sealed tools are currently limited to Driver children",
        ));
    }
    let staging_root = spec.staged_executable.parent().ok_or_else(|| {
        Error::new(
            ErrorCode::SandboxDenied,
            "Windows Driver staging root is unavailable",
        )
    })?;
    let profile_tool_root = Path::new(&profile.local_app_data).join("SemwrightTools");
    std::fs::create_dir(&profile_tool_root).map_err(|_| {
        Error::new(
            ErrorCode::SandboxDenied,
            "Windows sealed-tool profile directory could not be created",
        )
    })?;
    let profile_traverse = prepare_tool_staging_traverse(&profile_tool_root)?;
    let mut identities = BTreeSet::new();
    let mut prepared = Vec::with_capacity(spec.sealed_tools.len());
    let mut materialized = Vec::with_capacity(spec.sealed_tools.len());
    for tool in &spec.sealed_tools {
        let (path, sha256) = match &tool.source {
            SealedToolSource::VerifiedFile { path, sha256 } => (path, sha256),
            SealedToolSource::UnixFd(_) => {
                return Err(Error::new(
                    ErrorCode::SandboxDenied,
                    "Windows sealed tools require Host-staged verified files",
                ));
            }
        };
        if path.parent() != Some(staging_root) {
            return Err(Error::new(
                ErrorCode::SandboxDenied,
                "Windows sealed tools must come from the private Driver staging root",
            ));
        }

        // Re-attest the Host-staged sealed-tool source, copy only verified bytes into
        // the unique AppContainer profile, sync them, and then re-attest the exact executable
        // that the LPAC child will receive. Sealed-tool verification deliberately permits the
        // supported x64-on-ARM64 compatibility case; primary driver processes remain native-only.
        let bytes = verify_sealed_tool_executable(path, sha256)?;
        let materialized_path = profile_tool_root.join(format!("{}.exe", tool.name));
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&materialized_path)
            .map_err(|_| {
                Error::new(
                    ErrorCode::SandboxDenied,
                    "Windows sealed-tool profile copy could not be created",
                )
            })?;
        std::io::Write::write_all(&mut output, &bytes)?;
        output.sync_all()?;
        drop(output);
        verify_materialized_sealed_tool(&materialized_path, sha256)?;

        let plan = prepare_mount_grant(&Mount {
            source: materialized_path,
            class: MountClass::Workspace,
            logical_name: tool.name.clone(),
            read_only: true,
            execute: true,
        })?;
        if !identities.insert(plan.identity) {
            return Err(Error::invalid(
                "Windows sealed tools must refer to unique materialized executables",
            ));
        }
        materialized.push(MaterializedTool {
            name: tool.name.clone(),
            path: plan.materialized.path.clone(),
        });
        prepared.push(plan);
    }

    let encoded = encode_materialized_tools(&materialized)?;
    let mut grants = Vec::with_capacity(prepared.len() + 1);
    grants.push(WindowsMountGrant::narrow_profile_authority(
        &profile_traverse,
        profile.sid,
    )?);
    for plan in &prepared {
        match WindowsMountGrant::narrow_profile_authority(plan, profile.sid) {
            Ok(grant) => grants.push(grant),
            Err(error) => {
                if revoke_mount_grants(&mut grants).is_err() {
                    return Err(Error::new(
                        ErrorCode::SandboxDenied,
                        "Windows sealed-tool transaction failed and prior grants could not be fully revoked",
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
    fn create(capabilities: &[SID_AND_ATTRIBUTES]) -> Result<Self> {
        let suffix: String = unique_id()
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .take(32)
            .collect();
        let name = format!("Semwright.Sandbox.{suffix}");
        let wide = wide_null(OsStr::new(&name))?;
        // SAFETY: all strings are stable NUL-terminated buffers and every capability SID
        // remains live for the duration of this synchronous profile-creation call.
        let sid = unsafe {
            CreateAppContainerProfile(
                PCWSTR(wide.as_ptr()),
                PCWSTR(wide.as_ptr()),
                PCWSTR(wide.as_ptr()),
                (!capabilities.is_empty()).then_some(capabilities),
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

fn command_line(spec: &SandboxSpec, args: &[String]) -> Result<Vec<u16>> {
    let mut out = Vec::new();
    push_quoted_arg(&mut out, spec.staged_executable.as_os_str());
    for arg in args {
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
    tool_table: Option<&str>,
    mode: &WindowsLaunchMode,
) -> Result<Vec<u16>> {
    let mut entries = spec
        .environment
        .iter()
        .filter(|(name, _)| {
            name != SANDBOX_HOST_TOOL_CHILD_ENV
                && name != SANDBOX_HOST_TOOL_CWD_ENV
                && name != SANDBOX_HOST_TOOL_TYPED_ARGS_ENV
        })
        .cloned()
        .collect::<Vec<_>>();
    if entries
        .iter()
        .any(|(name, _)| name == SANDBOX_MOUNTS_ENV || name == SANDBOX_TOOLS_ENV)
    {
        return Err(Error::invalid(
            "Sandbox mount/tool table environment is reserved to the platform host",
        ));
    }
    if let Some(table) = mount_table {
        entries.push((SANDBOX_MOUNTS_ENV.into(), table.to_owned()));
    }
    if let Some(table) = tool_table {
        entries.push((SANDBOX_TOOLS_ENV.into(), table.to_owned()));
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
    if let WindowsLaunchMode::Blender521Initial { runtime } = mode {
        let resources = runtime.join("5.2");
        let user = Path::new(&profile.local_app_data).join("BlenderUser");
        std::fs::create_dir(&user)?;
        let system =
            std::env::var_os("SystemRoot").ok_or_else(|| Error::invalid("SystemRoot required"))?;
        let search = std::env::join_paths([
            runtime.clone(),
            runtime.join("blender.shared"),
            runtime.join("blender.crt"),
            std::path::PathBuf::from(system).join("System32"),
        ])
        .map_err(|_| Error::invalid("Native runtime DLL search path"))?;
        entries.extend([
            ("PATH".into(), search.to_string_lossy().into_owned()),
            ("USERPROFILE".into(), profile.local_app_data.clone()),
            ("APPDATA".into(), profile.local_app_data.clone()),
            (
                "BLENDER_USER_RESOURCES".into(),
                user.to_string_lossy().into_owned(),
            ),
            (
                "BLENDER_SYSTEM_RESOURCES".into(),
                resources.to_string_lossy().into_owned(),
            ),
            (
                "BLENDER_SYSTEM_SCRIPTS".into(),
                resources.join("scripts").to_string_lossy().into_owned(),
            ),
            (
                "BLENDER_SYSTEM_EXTENSIONS".into(),
                resources.join("extensions").to_string_lossy().into_owned(),
            ),
            (
                "BLENDER_SYSTEM_DATAFILES".into(),
                resources.join("datafiles").to_string_lossy().into_owned(),
            ),
            (
                "BLENDER_SYSTEM_PYTHON".into(),
                resources.join("python").to_string_lossy().into_owned(),
            ),
            ("PYTHONNOUSERSITE".into(), "1".into()),
            ("PYTHONIOENCODING".into(), "utf-8".into()),
        ]);
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
    _job: Arc<ProcessJob>,
    pid: u32,
    mount_grants: Vec<WindowsMountGrant>,
    profile_name: Option<Vec<u16>>,
    exited: bool,
    exit_code: Option<i32>,
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

    fn capture_exit_code(&mut self) -> Result<()> {
        let mut code = 0u32;
        // SAFETY: process is a live owned process HANDLE and code is a writable DWORD.
        unsafe { GetExitCodeProcess(self.process.raw(), &mut code) }.map_err(|_| {
            Error::new(
                ErrorCode::BackendFailed,
                "Windows sandbox exit code query failed",
            )
        })?;
        self.exit_code = Some(i32::from_ne_bytes(code.to_ne_bytes()));
        Ok(())
    }

    fn observed_exit(&mut self) -> Result<bool> {
        // SAFETY: process is a live owned process HANDLE.
        let wait = unsafe { WaitForSingleObject(self.process.raw(), 0) };
        if wait == WAIT_OBJECT_0 {
            self.capture_exit_code()?;
            self._job.terminate(0)?;
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
        self._job.terminate(1)?;
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
        self.capture_exit_code()?;
        self._job.terminate(0)?;
        self.exited = true;
        self.cleanup_authority()
    }

    fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }
}

impl Drop for NativeSandboxChild {
    fn drop(&mut self) {
        if !self.exited {
            let _ = self._job.terminate(1);
            // SAFETY: best-effort wait on the owned root process after terminating the Job.
            unsafe {
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

#[cfg(target_arch = "aarch64")]
fn require_x64_user_mode_support() -> Result<()> {
    // SAFETY: this is a read-only capability query for a constant machine type and returns
    // a value by copy; no caller-owned pointers or handles are involved.
    let attributes =
        unsafe { GetMachineTypeAttributes(IMAGE_FILE_MACHINE_AMD64) }.map_err(|_| {
            Error::new(
                ErrorCode::Unsupported,
                "Windows x64 emulation capability query failed",
            )
        })?;
    if attributes.0 & UserEnabled.0 == 0 {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "Windows host does not enable x64 user-mode execution",
        ));
    }
    Ok(())
}

#[cfg(not(target_arch = "aarch64"))]
fn require_x64_user_mode_support() -> Result<()> {
    Ok(())
}

fn host_tool_cross_arch(spec: &SandboxSpec) -> Result<bool> {
    let markers = spec
        .environment
        .iter()
        .filter(|(name, _)| name == SANDBOX_HOST_TOOL_CHILD_ENV)
        .collect::<Vec<_>>();
    if markers.is_empty() {
        return Ok(false);
    }
    let cwd_markers = spec
        .environment
        .iter()
        .filter(|(name, _)| name == SANDBOX_HOST_TOOL_CWD_ENV)
        .collect::<Vec<_>>();
    let typed_markers = spec
        .environment
        .iter()
        .filter(|(name, _)| name == SANDBOX_HOST_TOOL_TYPED_ARGS_ENV)
        .collect::<Vec<_>>();
    let environment_is_internal = spec.environment.iter().all(|(name, _)| {
        matches!(
            name.as_str(),
            SANDBOX_HOST_TOOL_CHILD_ENV
                | SANDBOX_HOST_TOOL_CWD_ENV
                | SANDBOX_HOST_TOOL_TYPED_ARGS_ENV
        )
    });
    if markers.len() != 1
        || markers[0].1 != "1"
        || cwd_markers.len() > 1
        || typed_markers.len() > 1
        || typed_markers.first().is_some_and(|marker| marker.1 != "1")
        || !environment_is_internal
        || spec.kind != semwright_platform_api::launch::SandboxKind::Driver
        || (!spec.sealed_tools.is_empty() && typed_markers.len() != 1)
        || spec.network
    {
        return Err(Error::new(
            ErrorCode::SandboxDenied,
            "Windows Host-tool marker is only valid for isolated Host-mediated tool children",
        ));
    }
    let architecture = architecture_file(&spec.staged_executable)?;
    let dependency_cross_arch = if std::env::consts::ARCH == "aarch64" {
        spec.sealed_tools
            .iter()
            .try_fold(false, |cross_arch, tool| {
                let path = match &tool.source {
                    SealedToolSource::VerifiedFile { path, .. } => path,
                    SealedToolSource::UnixFd(_) => {
                        return Err(Error::new(
                            ErrorCode::SandboxDenied,
                            "Windows typed tool dependencies require Host-staged files",
                        ));
                    }
                };
                Ok(cross_arch || architecture_file(path)? == PeArchitecture::Amd64)
            })?
    } else {
        false
    };
    match (std::env::consts::ARCH, architecture) {
        ("aarch64", PeArchitecture::Amd64) => {
            // Windows 11 ARM64 transparently emulates x64 user-mode binaries. Verify support
            // only in ARM64 builds so older x64 hosts never import this Windows 11 API.
            require_x64_user_mode_support()?;
            Ok(true)
        }
        ("aarch64", PeArchitecture::Arm64) => {
            if dependency_cross_arch {
                require_x64_user_mode_support()?;
            }
            Ok(dependency_cross_arch)
        }
        ("x86_64", PeArchitecture::Amd64) => Ok(false),
        _ => Err(Error::new(
            ErrorCode::Unsupported,
            "Windows sealed-tool architecture is not supported by this host",
        )),
    }
}

/// Windows arbitrary-child launch is platform-owned: an AppContainer identity and explicit
/// inherited-handle list are attached at creation, the process starts suspended, enters a
/// kill-on-close Job Object, and is resumed only after that boundary exists.
pub struct WindowsSandbox;

/// Native Windows sandbox launch retains a separate, closed authority mode.
#[derive(Clone)]
enum WindowsLaunchMode {
    Default,
    Blender521Initial { runtime: std::path::PathBuf },
}

impl WindowsLaunchMode {
    fn registry_read_capability(&self) -> bool {
        matches!(self, Self::Blender521Initial { .. })
    }

    fn validate(&self, spec: &SandboxSpec, cross_arch_host_tool: bool) -> Result<()> {
        match self {
            Self::Default => Ok(()),
            Self::Blender521Initial { runtime } => {
                // Native-only, one process, no external network or GPU.
                if spec.network
                    || spec
                        .limits
                        .as_ref()
                        .is_none_or(|limits| limits.processes != 1)
                    || !cfg!(target_arch = "x86_64")
                    || cross_arch_host_tool
                {
                    return Err(Error::new(
                        ErrorCode::SandboxDenied,
                        "Native Blender initial launch requires CPU-only x64, no network or emulation, and one process",
                    ));
                }
                validate_blender52_initial_spec(spec, runtime)
            }
        }
    }
}

fn validate_blender52_initial_spec(spec: &SandboxSpec, runtime: &Path) -> Result<()> {
    use semwright_platform_api::launch::{HostToolArgRef, SandboxKind, encode_host_tool_arg_ref};
    let flags = [
        "--background",
        "--factory-startup",
        "--disable-autoexec",
        "--offline-mode",
        "-q",
        "--python-exit-code",
        "1",
        "--python",
        "bootstrap.py",
        "--",
    ];
    if spec.kind != SandboxKind::Driver
        || spec.nvidia_gpu
        || !spec.sealed_tools.is_empty()
        || spec.staged_executable != runtime.join("blender.exe")
        || spec.args.len() != 14
        || !spec.args[..10].iter().zip(flags).all(|(a, b)| a == b)
        || spec.mounts.len() != 3
        || spec.environment.len() != 3
        || !spec
            .environment
            .iter()
            .any(|(name, value)| name == SANDBOX_HOST_TOOL_CHILD_ENV && value == "1")
        || !spec
            .environment
            .iter()
            .any(|(name, value)| name == SANDBOX_HOST_TOOL_TYPED_ARGS_ENV && value == "1")
    {
        return Err(Error::new(
            ErrorCode::SandboxDenied,
            "Closed Windows Blender initial launch contract required",
        ));
    }
    let nonce = spec.args[10].strip_prefix("stdio:").unwrap_or("");
    if nonce.len() != 32
        || !nonce
            .bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    {
        return Err(Error::invalid("Host native nonce required"));
    }
    for (index, name, read_only, execute) in [
        (11, "workspace", false, false),
        (12, "native-bootstrap", true, false),
        (13, "blender-runtime", true, true),
    ] {
        let mount = spec
            .mounts
            .iter()
            .find(|m| m.logical_name == name)
            .ok_or_else(|| Error::new(ErrorCode::SandboxDenied, "Native mount role missing"))?;
        if mount.class != MountClass::Workspace
            || mount.read_only != read_only
            || mount.execute != execute
        {
            return Err(Error::new(
                ErrorCode::SandboxDenied,
                "Native mount authority mismatch",
            ));
        }
        let arg = encode_host_tool_arg_ref(&HostToolArgRef::MountPath {
            mount: name.into(),
            relative: String::new(),
        })?;
        if spec.args[index] != arg || (name == "blender-runtime" && mount.source != runtime) {
            return Err(Error::new(
                ErrorCode::SandboxDenied,
                "Native typed mount argument mismatch",
            ));
        }
        if name == "native-bootstrap"
            && !spec
                .environment
                .iter()
                .any(|(k, v)| k == SANDBOX_HOST_TOOL_CWD_ENV && Path::new(v) == mount.source)
        {
            return Err(Error::new(
                ErrorCode::SandboxDenied,
                "Native code CWD must be the read-only bootstrap",
            ));
        }
    }
    Ok(())
}

/// Production entry for the independently owner-granted, closed Blender 5.2.1 profile.
pub fn sandbox_spawn_blender52_initial(
    spec: &SandboxSpec,
    runtime: &Path,
    digest: &str,
) -> Result<SandboxProcess> {
    // Prove the exact closed launch request BEFORE opening or hashing any
    // executable. A malformed Host request cannot use this function as a
    // generic executable probe or fall through to a normal driver launch.
    spec.validate()?;
    let mode = WindowsLaunchMode::Blender521Initial {
        runtime: runtime.to_path_buf(),
    };
    mode.validate(spec, host_tool_cross_arch(spec)?)?;
    verify_application_executable(&spec.staged_executable, digest)?;
    eprintln!(
        "{}",
        serde_json::json!({"component":"platform-windows",
        "phase":"blender_5_2_1_initial_requested","profile":"blender_5_2_1_initial",
        "executable_sha256":digest,"requested_capabilities":["registryRead"],
        "network":false,"application_startup_verified":false,"test_only":false})
    );
    WindowsSandbox.spawn_with_mode(spec, mode)
}

impl WindowsSandbox {
    fn spawn_with_mode(
        &self,
        spec: &SandboxSpec,
        mode: WindowsLaunchMode,
    ) -> Result<SandboxProcess> {
        spec.validate()?;
        let cross_arch_host_tool = host_tool_cross_arch(spec)?;
        mode.validate(spec, cross_arch_host_tool)?;
        let typed_host_tool = spec
            .environment
            .iter()
            .any(|(name, value)| name == SANDBOX_HOST_TOOL_TYPED_ARGS_ENV && value == "1");
        if !spec.sealed_tools.is_empty() && !typed_host_tool {
            return Err(Error::new(
                ErrorCode::SandboxDenied,
                "Windows nested sealed-tool dependencies require a typed Host-tool child",
            ));
        }
        let network_sid = spec
            .network
            .then(|| well_known_sid(WinCapabilityInternetClientSid))
            .transpose()?;
        let mut capability_entries = Vec::with_capacity(2);
        if let Some(sid) = network_sid.as_ref() {
            capability_entries.push(SID_AND_ATTRIBUTES {
                Sid: PSID(sid.as_ptr().cast_mut().cast()),
                Attributes: SE_GROUP_ENABLED as u32,
            });
        }
        // Register internetClient on the unique AppContainer profile as well as on the
        // process token, matching Microsoft's AppContainer network-capability creation model.
        // The prior token-only path exposed the SID but did not yield real outbound connectivity.
        let profile = AppContainerProfile::create(&capability_entries)?;
        // LPAC strips the registry access that ordinary AppContainers inherit. Winsock
        // initialization depends on read-only system networking configuration, so grant the
        // documented LPAC registryRead capability only when ambient network was explicitly
        // owner-authorized. This remains process-scoped and does not change filesystem mounts,
        // loopback policy, or the AppContainer profile's network capability declaration.
        let network_registry_sid = (spec.network || mode.registry_read_capability())
            .then(|| named_capability_sid("registryRead"))
            .transpose()?;
        if let Some(sid) = network_registry_sid.as_ref() {
            capability_entries.push(SID_AND_ATTRIBUTES {
                Sid: PSID(sid.as_ptr().cast_mut().cast()),
                Attributes: SE_GROUP_ENABLED as u32,
            });
        }
        let (mut mount_grants, mount_table) = prepare_windows_mounts(
            spec,
            &profile,
            matches!(&mode, WindowsLaunchMode::Blender521Initial { .. }),
        )?;
        let (tool_grants, tool_table) = prepare_windows_tools(spec, &profile)?;
        mount_grants.extend(tool_grants);
        let materialized_mounts = mount_table
            .as_deref()
            .map(decode_materialized_mounts)
            .transpose()?
            .unwrap_or_default();
        let materialized_tools = tool_table
            .as_deref()
            .map(decode_materialized_tools)
            .transpose()?
            .unwrap_or_default();
        let resolved_args = resolve_host_tool_args(
            &spec.args,
            &materialized_mounts,
            &materialized_tools,
            typed_host_tool,
        )?;
        let (child_stdin, parent_stdin) = inheritable_pipe()?;
        let (parent_stdout, child_stdout) = inheritable_pipe()?;
        clear_inheritance(parent_stdin.raw())?;
        clear_inheritance(parent_stdout.raw())?;
        let child_stderr = inherited_null()?;

        let handles = [child_stdin.raw(), child_stdout.raw(), child_stderr.raw()];
        let mut attributes = ProcAttributes::new(3)?;
        attributes.set_slice(PROC_THREAD_ATTRIBUTE_HANDLE_LIST, &handles)?;
        let all_application_packages_policy = PROCESS_CREATION_ALL_APPLICATION_PACKAGES_OPT_OUT;
        attributes.set_value(
            PROC_THREAD_ATTRIBUTE_ALL_APPLICATION_PACKAGES_POLICY,
            &all_application_packages_policy,
        )?;

        // Windows-on-ARM x64 emulation performs compatibility initialization that needs
        // read-only registry access. Keep this capability scoped to the cross-architecture
        // Host-tool case; native drivers/tools and network-disabled children do not gain it.
        // It intentionally remains process-scoped rather than being registered on the profile;
        // this PR changes only ambient network authority.
        let emulation_registry_sid = cross_arch_host_tool
            .then(|| named_capability_sid("registryRead"))
            .transpose()?;
        if let Some(sid) = emulation_registry_sid.as_ref() {
            capability_entries.push(SID_AND_ATTRIBUTES {
                Sid: PSID(sid.as_ptr().cast_mut().cast()),
                Attributes: SE_GROUP_ENABLED as u32,
            });
        }
        let capabilities = SECURITY_CAPABILITIES {
            AppContainerSid: profile.sid,
            Capabilities: if capability_entries.is_empty() {
                std::ptr::null_mut()
            } else {
                capability_entries.as_mut_ptr()
            },
            CapabilityCount: u32::try_from(capability_entries.len()).map_err(|_| {
                Error::new(
                    ErrorCode::ResourceExhausted,
                    "Windows sandbox capability count exceeds platform budget",
                )
            })?,
            ..Default::default()
        };
        attributes.set_value(PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES, &capabilities)?;

        // Hold the exact identities requested for the still-suspended child.
        // The child must prove these before its first user-mode instruction.
        let expected_user = current_user_sid_bytes()?;
        let expected_container = copy_sid_bytes(profile.sid)?;
        let expected_capabilities = capability_entries
            .iter()
            .map(|entry| Ok((copy_sid_bytes(entry.Sid)?, entry.Attributes)))
            .collect::<Result<Vec<_>>>()?;

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
            })?
            .map(|limit| {
                if cross_arch_host_tool {
                    // x64-on-ARM64 emulation may require one runtime-support process. This
                    // extra Job slot is platform overhead, not delegated driver authority:
                    // the LPAC child remains network-denied and sees only Host-selected
                    // per-tool mounts; ordinary descendant spawn attempts must still fail.
                    limit.saturating_add(1)
                } else {
                    limit
                }
            });
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
        let job = Arc::new(ProcessJob::new(process_limit, memory_limit, cpu_seconds)?);

        let application = wide_null(spec.staged_executable.as_os_str())?;
        let requested_directory = spec
            .environment
            .iter()
            .find(|(name, _)| name == SANDBOX_HOST_TOOL_CWD_ENV)
            .map(|(_, value)| value.as_str())
            .unwrap_or(&profile.local_app_data);
        let requested_directory_path = Path::new(requested_directory);
        if !requested_directory_path.is_absolute() || !requested_directory_path.is_dir() {
            return Err(Error::new(
                ErrorCode::SandboxDenied,
                "Windows sandbox working directory must be an existing absolute directory",
            ));
        }
        let current_directory = wide_null(requested_directory_path.as_os_str())?;
        let mut command = command_line(spec, &resolved_args)?;
        let environment = environment_block(
            spec,
            &profile,
            mount_table.as_deref(),
            tool_table.as_deref(),
            &mode,
        )?;
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
                PCWSTR(current_directory.as_ptr()),
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

        // Assignment success is not proof that this exact child is a member
        // of this exact Job, or that its token matches our requested LPAC
        // identity and capability sets. Verify before any child code can run.
        let attestation = job.verify_member(process.raw()).and_then(|()| {
            initial_attestation::verify(
                process.raw().0,
                &expected_user,
                &expected_container,
                &expected_capabilities,
            )
            .map_err(|error| {
                Error::new(
                    ErrorCode::SandboxDenied,
                    format!("Initial process token attestation failed: {error}"),
                )
            })
        });
        let attestation = match attestation {
            Ok(value) => value,
            Err(error) => {
                let job_termination_ok = job.terminate(1).is_ok();
                // SAFETY: still-suspended child owned by this invocation.
                let process_termination_ok = unsafe { TerminateProcess(process.raw(), 1) }.is_ok();
                // SAFETY: bounded wait on the owned process handle.
                let exit_observed =
                    unsafe { WaitForSingleObject(process.raw(), 5_000) } == WAIT_OBJECT_0;
                eprintln!(
                    "{}",
                    serde_json::json!({
                        "component": "platform-windows",
                        "phase": "initial_process_attestation_rejected",
                        "pid": process_info.dwProcessId,
                        "resumed": false,
                        "job_termination_ok": job_termination_ok,
                        "process_termination_ok": process_termination_ok,
                        "exit_observed": exit_observed,
                        "error": error.to_string(),
                    })
                );
                if !exit_observed {
                    return Err(Error::new(
                        ErrorCode::SandboxDenied,
                        "Rejected initial process cleanup could not be confirmed",
                    ));
                }
                return Err(error);
            }
        };
        eprintln!(
            "{}",
            serde_json::json!({
                "component": "platform-windows",
                "phase": "initial_process_attested_before_resume",
                "pid": process_info.dwProcessId,
                "exact_job": true,
                "resumed": false,
                "application_startup_verified": false,
                "token": attestation,
            })
        );
        // SAFETY: containment and exact suspended-child attestation hold; resuming now is
        // the first point untrusted child code may run.
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
        let cpu_accounting: Arc<dyn SandboxCpuAccounting> = job.clone();
        Ok(SandboxProcess::from_parts_with_cpu_accounting(
            stdin,
            stdout,
            Box::new(NativeSandboxChild {
                process,
                _job: job,
                pid: process_info.dwProcessId,
                mount_grants,
                profile_name: Some(profile_name),
                exited: false,
                exit_code: None,
            }),
            cpu_accounting,
        ))
    }
}

impl SandboxLauncher for WindowsSandbox {
    fn command(&self, spec: &SandboxSpec) -> Result<Command> {
        spec.validate()?;
        Err(Error::new(
            ErrorCode::SandboxDenied,
            "Windows sandbox creation is only available through platform-owned spawn",
        ))
    }

    fn spawn(&self, spec: &SandboxSpec) -> Result<SandboxProcess> {
        self.spawn_with_mode(spec, WindowsLaunchMode::Default)
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
            "network": "internetClient_capability_only_when_requested",
            "sealed_tools": "host_mediated_driver_protocol_v4_plus",
            "driver_child_process_creation": "denied",
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

        let digest = hex::encode(Sha256::digest(&bytes));
        let verified = WindowsVerifier
            .verify(&path, &digest)
            .expect("hardened staged Windows executable should satisfy trust policy");
        assert_eq!(verified, bytes);
    }

    #[test]
    fn native_mount_write_dac_probe_only_reads_owner_private_fixture() {
        let temp = tempfile::tempdir().unwrap();
        let child = temp.path().join("native-tree");
        std::fs::create_dir(&child).unwrap();
        let content = b"original-private-fixture-bytes";
        let file = child.join("unchanged.dat");
        std::fs::write(&file, content).unwrap();
        let audit = native_mount_write_dac_audit(&child);
        assert_eq!(audit["root_write_dac_status"], 0);
        assert_eq!(
            audit["root_security_context"]["root_security_read_win32"],
            0
        );
        // Elevated runners may own a newly created directory through the
        // Administrators group, rather than their interactive user SID.
        assert!(
            audit["root_security_context"]["owner_is_process_user"] == true
                || audit["root_security_context"]["owner_is_administrators"] == true
                || audit["root_security_context"]["owner_is_system"] == true
        );
        assert_eq!(
            audit["root_security_context"]["thread_impersonation_token_present"],
            false
        );
        assert!(audit["scanned"].as_u64().unwrap() >= 2);
        assert_eq!(audit["metadata_errors"], 0);
        assert_eq!(audit["enumeration_errors"], 0);
        assert_eq!(audit["refused_reparse_points"], 0);
        assert_eq!(std::fs::read(&file).unwrap(), content);
        assert!(!audit.to_string().contains("unchanged.dat"));
    }

    #[test]
    fn executable_acl_allows_untrusted_read_execute_but_never_write_authority() {
        let readonly = FILE_GENERIC_READ.0 | FILE_GENERIC_EXECUTE.0;
        let executable_mask = untrusted_acl_restricted_mask(false);
        assert_eq!(
            readonly & executable_mask,
            0,
            "Read/execute rights (including shared READ_CONTROL and SYNCHRONIZE) are not mutation"
        );
        assert_ne!(
            readonly & untrusted_acl_restricted_mask(true),
            0,
            "Owner-private data must still reject untrusted read access"
        );
        for write in [
            FILE_WRITE_DATA.0,
            FILE_APPEND_DATA.0,
            FILE_WRITE_EA.0,
            FILE_WRITE_ATTRIBUTES.0,
            DELETE.0,
            WRITE_DAC.0,
            WRITE_OWNER.0,
            FILE_GENERIC_WRITE.0,
            GENERIC_WRITE.0,
            GENERIC_ALL.0,
        ] {
            assert_ne!(
                write & executable_mask,
                0,
                "Write bit {write:#x} escaped ACL rejection"
            );
        }
    }

    #[test]
    fn executable_acl_real_untrusted_rx_allows_read_but_denies_mutation() {
        let directory = tempfile::tempdir().expect("Windows ACL test directory");
        let file_path = directory.path().join("native-vendor-acl-fixture.bin");
        std::fs::write(&file_path, b"pinned-read-only-vendor-bytes").unwrap();
        let user = std::env::var("USERNAME").expect("Windows USERNAME");
        let owner = match std::env::var("USERDOMAIN") {
            Ok(domain) if !domain.is_empty() => format!(r"{domain}\{user}"),
            _ => user,
        };
        let harden = std::process::Command::new("icacls")
            .arg(&file_path)
            .arg("/inheritance:r")
            .arg("/grant:r")
            .arg(format!("{owner}:(F)"))
            .status()
            .expect("harden owner-only test source");
        assert!(harden.success());
        // BUILTIN\Users is intentionally *not* trusted to mutate. Its
        // read/execute ACE is expected on vendor-installed Windows binaries.
        let rx = std::process::Command::new("icacls")
            .arg(&file_path)
            .arg("/grant")
            .arg("*S-1-5-32-545:(RX)")
            .status()
            .expect("grant read/execute only to Windows Users");
        assert!(rx.success());
        let file = File::open(&file_path).unwrap();
        verify_trusted_file_acl(&file, WindowsAclPurpose::Executable)
            .expect("Untrusted read/execute is not executable mutation");
        assert!(
            verify_trusted_file_acl(&file, WindowsAclPurpose::PrivateData).is_err(),
            "Private data must still reject the same untrusted RX authority"
        );
        drop(file);

        // Replacing RX with Modify must fail closed. Never grant a
        // broader path permission to make an installed application pass.
        let modified = std::process::Command::new("icacls")
            .arg(&file_path)
            .arg("/grant:r")
            .arg("*S-1-5-32-545:(M)")
            .status()
            .expect("change synthetic test ACE only");
        assert!(modified.success());
        let file = File::open(&file_path).unwrap();
        assert_eq!(
            verify_trusted_file_acl(&file, WindowsAclPurpose::Executable)
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied
        );
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
        let profile = AppContainerProfile::create(&[]).expect("AppContainer profile");
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

#[cfg(test)]
mod native_blender_owner_inheritance_tests {
    use super::*;

    fn owner_private_workspace() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("isolated Windows workspace");
        let output = std::process::Command::new("whoami")
            .output()
            .expect("query current account");
        assert!(output.status.success());
        let user = String::from_utf8(output.stdout)
            .expect("valid principal")
            .trim()
            .to_string();
        assert!(!user.is_empty());

        let protected = std::process::Command::new("icacls")
            .arg(dir.path())
            .arg("/inheritance:r")
            .arg("/grant:r")
            .arg(format!("{user}:(F)"))
            .status()
            .expect("make temporary workspace owner-private");
        assert!(protected.success());
        // Hosted Windows runners may create temp directories owned by the
        // Administrators group. Explicitly bind the test-only owner to the
        // current user; production must NOT change an unrelated owner.
        let owner = std::process::Command::new("icacls")
            .arg(dir.path())
            .arg("/setowner")
            .arg(user)
            .status()
            .expect("set test-only directory owner");
        assert!(owner.success());
        dir
    }

    fn plan(directory: &Path, read_only: bool) -> PreparedMountGrant {
        prepare_mount_grant(&Mount {
            source: directory.to_path_buf(),
            class: MountClass::Workspace,
            logical_name: "workspace".into(),
            read_only,
            execute: false,
        })
        .expect("validate test-only owner workspace")
    }

    fn full_owner_mask() -> u32 {
        FILE_GENERIC_READ.0
            | FILE_GENERIC_WRITE.0
            | FILE_GENERIC_EXECUTE.0
            | FILE_DELETE_CHILD.0
            | DELETE.0
            | WRITE_DAC.0
            | WRITE_OWNER.0
    }

    #[test]
    fn blender_owner_inheritance_is_verified_and_keeps_the_native_sid_scoped() {
        let workspace = owner_private_workspace();
        let prepared = plan(workspace.path(), false);
        let owner = current_user_sid_bytes().expect("user SID");
        let sid = PSID(owner.as_ptr().cast_mut().cast());
        assert!(
            dacl_has_grant(&prepared.path, sid, full_owner_mask(), NO_INHERITANCE)
                .expect("noninheritable owner Full Control")
        );
        assert!(
            !dacl_has_grant(
                &prepared.path,
                sid,
                full_owner_mask(),
                SUB_CONTAINERS_AND_OBJECTS_INHERIT,
            )
            .expect("no inheritable owner Full Control before setup"),
            "the test must reproduce a noninheritable private owner ACL",
        );

        ensure_native_blender_workspace_owner_inheritance(&prepared)
            .expect("enable inheritance solely for this existing owner");
        ensure_native_blender_workspace_owner_inheritance(&prepared)
            .expect("idempotent owner ACL, no double expansion");
        assert!(
            dacl_has_grant(
                &prepared.path,
                sid,
                full_owner_mask(),
                SUB_CONTAINERS_AND_OBJECTS_INHERIT,
            )
            .expect("verify owner inheritance before native launch")
        );

        let profile = AppContainerProfile::create(&[]).expect("unique LPAC profile");
        let mut granted =
            WindowsMountGrant::grant(&prepared, profile.sid).expect("native workspace SID mount");
        let output = workspace.path().join("owner-readable-native.blend");
        std::fs::write(&output, b"owner-file-fixture")
            .expect("create sample under active owner+LPAC grant");
        let output_path = wide_null(output.as_os_str()).unwrap();
        assert!(dacl_has_sid(&output_path, sid).expect("owner ACE inherited"));
        assert!(dacl_has_sid(&output_path, profile.sid).expect("LPAC ACE inherited"));
        granted.revoke().expect("revoke LPAC identity");
        assert!(!dacl_has_sid(&output_path, profile.sid).expect("unique LPAC ACE revoked"));
        assert!(dacl_has_sid(&output_path, sid).expect("owner access retained after LPAC exit"));
        assert_eq!(std::fs::read(&output).unwrap(), b"owner-file-fixture");
        // Existing owner access was not expanded to any new principal.
    }

    #[test]
    fn owner_inheritance_denies_readonly_and_owner_restricted_workspaces() {
        let workspace = owner_private_workspace();
        let readonly = plan(workspace.path(), true);
        assert!(matches!(
            ensure_native_blender_workspace_owner_inheritance(&readonly),
            Err(error) if error.code == ErrorCode::PolicyDenied
        ));

        let output = std::process::Command::new("whoami")
            .output()
            .expect("whoami for temporary ACL");
        assert!(output.status.success());
        let user = String::from_utf8(output.stdout)
            .expect("principal")
            .trim()
            .to_string();
        let changed = std::process::Command::new("icacls")
            .arg(workspace.path())
            .arg("/grant:r")
            .arg(format!("{user}:(RX)"))
            .status()
            .expect("restrict test-only owner ACL");
        assert!(changed.success());
        let writable = plan(workspace.path(), false);
        assert!(matches!(
            ensure_native_blender_workspace_owner_inheritance(&writable),
            Err(error) if error.code == ErrorCode::PermissionDenied
        ));
        // Restore owner rights so tempfile can dispose its own directory.
        let restored = std::process::Command::new("icacls")
            .arg(workspace.path())
            .arg("/grant:r")
            .arg(format!("{user}:(F)"))
            .status()
            .expect("restore test fixture");
        assert!(restored.success());
    }
}

#[cfg(test)]
mod closed_blender_initial_launch_tests {
    use super::*;
    use semwright_platform_api::launch::{
        HostToolArgRef, ResourceLimits, SandboxKind, encode_host_tool_arg_ref,
    };

    fn native_fixture() -> (SandboxSpec, std::path::PathBuf) {
        let runtime = std::path::PathBuf::from(r"C:\semwright-private\runtime");
        let code = std::path::PathBuf::from(r"C:\semwright-private\bootstrap");
        let mounts = vec![
            Mount {
                source: std::path::PathBuf::from(r"C:\owner-approved\workspace"),
                class: MountClass::Workspace,
                logical_name: "workspace".into(),
                read_only: false,
                execute: false,
            },
            Mount {
                source: code.clone(),
                class: MountClass::Workspace,
                logical_name: "native-bootstrap".into(),
                read_only: true,
                execute: false,
            },
            Mount {
                source: runtime.clone(),
                class: MountClass::Workspace,
                logical_name: "blender-runtime".into(),
                read_only: true,
                execute: true,
            },
        ];
        let mut args = [
            "--background",
            "--factory-startup",
            "--disable-autoexec",
            "--offline-mode",
            "-q",
            "--python-exit-code",
            "1",
            "--python",
            "bootstrap.py",
            "--",
        ]
        .iter()
        .map(|value| (*value).to_string())
        .collect::<Vec<_>>();
        args.push(format!("stdio:{}", "a".repeat(32)));
        for name in ["workspace", "native-bootstrap", "blender-runtime"] {
            args.push(
                encode_host_tool_arg_ref(&HostToolArgRef::MountPath {
                    mount: name.into(),
                    relative: String::new(),
                })
                .unwrap(),
            );
        }
        (
            SandboxSpec {
                kind: SandboxKind::Driver,
                staged_executable: runtime.join("blender.exe"),
                helper: std::path::PathBuf::from(r"C:\semwright-private\sandbox.exe"),
                mounts,
                args,
                environment: vec![
                    (SANDBOX_HOST_TOOL_CHILD_ENV.into(), "1".into()),
                    (SANDBOX_HOST_TOOL_TYPED_ARGS_ENV.into(), "1".into()),
                    (
                        SANDBOX_HOST_TOOL_CWD_ENV.into(),
                        code.to_string_lossy().into_owned(),
                    ),
                ],
                sealed_tools: vec![],
                network: false,
                nvidia_gpu: false,
                limits: Some(ResourceLimits {
                    open_files: 128,
                    processes: 1,
                    cpu_seconds: 30,
                    address_space_bytes: 1_073_741_824,
                    file_size_bytes: 1_073_741_824,
                }),
            },
            runtime,
        )
    }

    fn check(spec: &SandboxSpec, runtime: &Path) -> Result<()> {
        spec.validate()?;
        WindowsLaunchMode::Blender521Initial {
            runtime: runtime.to_path_buf(),
        }
        .validate(spec, false)
    }

    #[test]
    fn closed_native_blender_contract_accepts_only_the_host_shape() {
        let (spec, runtime) = native_fixture();
        check(&spec, &runtime).unwrap();
        assert!(!WindowsLaunchMode::Default.registry_read_capability());
        assert!(WindowsLaunchMode::Blender521Initial { runtime }.registry_read_capability());
    }

    #[test]
    fn native_launch_rejects_extra_arguments_mounts_capabilities_and_network() {
        let (original, runtime) = native_fixture();
        for variant in 0..13 {
            let mut spec = original.clone();
            match variant {
                0 => spec.network = true,
                1 => spec.nvidia_gpu = true,
                2 => spec.limits.as_mut().unwrap().processes = 2,
                3 => spec.args[3] = "--enable-autoexec".into(),
                4 => spec.args[10] = "stdio:".to_owned() + &"z".repeat(32),
                5 => spec.args[10] = "stdio:".to_owned() + &"A".repeat(32),
                6 => spec.args.push("extra".into()),
                7 => spec.mounts[0].read_only = true,
                8 => spec.mounts[1].execute = true,
                9 => spec.mounts[2].read_only = false,
                10 => spec.mounts[2].source.push("other"),
                11 => spec.environment[2].1 = r"C:\different-bootstrap".into(),
                _ => spec.kind = SandboxKind::ExternalMcp,
            }
            assert!(
                check(&spec, &runtime).is_err(),
                "variant {variant} admitted"
            );
        }
        let (spec, runtime) = native_fixture();
        assert!(
            WindowsLaunchMode::Blender521Initial { runtime }
                .validate(&spec, true)
                .is_err(),
            "Cross-architecture tool emulation must not be admitted"
        );
    }

    #[test]
    fn no_native_request_is_accepted_as_a_generic_mode_selector() {
        let (mut spec, runtime) = native_fixture();
        spec.environment.clear();
        assert!(
            WindowsLaunchMode::Blender521Initial { runtime }
                .validate(&spec, false)
                .is_err()
        );
        // Generic launch uses its existing method and never selects registryRead
        // solely because the caller supplied Blender-style arguments.
        assert!(!WindowsLaunchMode::Default.registry_read_capability());
    }
}
