use crate::{identity::current_user_sid_bytes, pe::require_native_architecture};
use semwright_platform_api::launch::{ExecutableVerifier, SandboxLauncher, SandboxSpec};
use semwright_types::{Error, ErrorCode, Result};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::Read,
    os::windows::{ffi::OsStrExt, fs::OpenOptionsExt, io::AsRawHandle},
    path::Path,
};
use tokio::process::Command;
use windows::Win32::{
    Foundation::{
        GENERIC_ALL, GENERIC_WRITE, HANDLE, HLOCAL, HWND, LocalFree, TRUST_E_EXPLICIT_DISTRUST,
        TRUST_E_NOSIGNATURE,
    },
    Security::{
        ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_SIZE_INFORMATION, AclSizeInformation,
        Authorization::{GetSecurityInfo, SE_FILE_OBJECT},
        CreateWellKnownSid, DACL_SECURITY_INFORMATION, EqualSid, GetAce, GetAclInformation,
        OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SECURITY_MAX_SID_SIZE,
        WinBuiltinAdministratorsSid, WinLocalSystemSid,
        WinTrust::{
            WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0, WINTRUST_FILE_INFO,
            WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_FILE, WTD_REVOCATION_CHECK_NONE,
            WTD_REVOKE_NONE, WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY, WTD_UI_NONE,
            WinVerifyTrust,
        },
    },
    Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, DELETE, FILE_APPEND_DATA, FILE_ATTRIBUTE_REPARSE_POINT,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_WRITE, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_WRITE_ATTRIBUTES, FILE_WRITE_DATA, FILE_WRITE_EA, GetFileInformationByHandle,
        WRITE_DAC, WRITE_OWNER,
    },
    System::SystemServices::{
        ACCESS_ALLOWED_ACE_TYPE, ACCESS_ALLOWED_CALLBACK_ACE_TYPE,
        ACCESS_ALLOWED_CALLBACK_OBJECT_ACE_TYPE, ACCESS_ALLOWED_OBJECT_ACE_TYPE,
        ACCESS_DENIED_ACE_TYPE, ACCESS_DENIED_CALLBACK_ACE_TYPE,
        ACCESS_DENIED_CALLBACK_OBJECT_ACE_TYPE, ACCESS_DENIED_OBJECT_ACE_TYPE,
    },
};
use windows::core::PCWSTR;

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

/// The current shared `SandboxLauncher` contract returns a normal `Command`. On Windows that is
/// not sufficient to prove CREATE_SUSPENDED -> AppContainer/LPAC token -> Job assignment -> resume
/// ordering before untrusted code runs. Refuse arbitrary child execution rather than race it.
pub struct WindowsSandbox;
impl SandboxLauncher for WindowsSandbox {
    fn command(&self, spec: &SandboxSpec) -> Result<Command> {
        spec.validate()?;
        Err(Error::new(
            ErrorCode::SandboxDenied,
            "Windows arbitrary driver/plugin launch is fail-closed until secure pre-exec spawn is part of the platform contract",
        ))
    }

    fn available(&self, _helper: &Path) -> bool {
        false
    }

    fn mechanism(&self) -> &'static str {
        "unavailable:windows-appcontainer-lpac-job-preexec-contract"
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
