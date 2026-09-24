use crate::pe::require_native_architecture;
use semwright_platform_api::launch::{ExecutableVerifier, SandboxLauncher, SandboxSpec};
use semwright_types::{Error, ErrorCode, Result};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::Read,
    os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    path::Path,
};
use tokio::process::Command;
use windows::Win32::{
    Foundation::HANDLE,
    Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_SHARE_DELETE, FILE_SHARE_READ, GetFileInformationByHandle,
    },
};

const MAX_EXECUTABLE: u64 = 64 * 1024 * 1024;

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
