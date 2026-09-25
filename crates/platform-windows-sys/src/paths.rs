use semwright_platform_api::PlatformPaths;
use semwright_types::{Error, ErrorCode, Result};
use std::{
    os::windows::{ffi::OsStrExt, fs::MetadataExt},
    path::{Path, PathBuf},
};
use windows::{
    Win32::{
        Foundation::{HLOCAL, LocalFree},
        Security::{
            Authorization::{
                ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
            },
            DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
        },
        Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT,
        UI::Shell::{
            FOLDERID_LocalAppData, FOLDERID_RoamingAppData, KF_FLAG_DEFAULT, SHGetKnownFolderPath,
        },
    },
    core::{HSTRING, PWSTR},
};

fn known_folder(id: &windows::core::GUID) -> Result<PathBuf> {
    // SAFETY: documented known-folder GUID, current-user token (None), no retained caller pointers.
    let raw: PWSTR = unsafe { SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None) }
        .map_err(|_| Error::unavailable("Windows Known Folder lookup failed"))?;
    if raw.is_null() {
        return Err(Error::unavailable(
            "Windows Known Folder lookup returned null",
        ));
    }
    // SAFETY: the PWSTR was returned by the documented Known Folder API and remains allocated and valid until the matching CoTaskMemFree below.
    let value = unsafe { raw.to_string() }
        .map_err(|_| Error::new(ErrorCode::BackendFailed, "Known Folder path was invalid"))?;
    // SAFETY: SHGetKnownFolderPath returns CoTaskMem; Windows documents CoTaskMemFree. The
    // generated binding aliases this allocation to PWSTR; use CoTaskMemFree rather than LocalFree.
    unsafe {
        windows::Win32::System::Com::CoTaskMemFree(Some(raw.0.cast()));
    }
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err(Error::invalid("Known Folder path is not absolute"));
    }
    Ok(path)
}

pub fn ensure_private_directory(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path)?;
    let meta = std::fs::symlink_metadata(path)?;
    if !meta.is_dir()
        || meta.file_type().is_symlink()
        || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
    {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Windows private path must be a non-reparse directory",
        ));
    }
    // DACL/owner validation is performed by the Named Pipe and executable/file primitives at the
    // point where authority is consumed. Directory existence alone never grants authority.
    Ok(())
}

struct OwnedSecurityDescriptor(PSECURITY_DESCRIPTOR);

impl Drop for OwnedSecurityDescriptor {
    fn drop(&mut self) {
        // SAFETY: SDDL conversion allocates the descriptor with LocalAlloc.
        unsafe {
            let _ = LocalFree(Some(HLOCAL(self.0.0)));
        }
    }
}

/// Tighten a sensitive broker-owned directory to the current user plus LocalSystem.
///
/// The DACL is protected from parent inheritance and marks both ACEs inheritable so newly-created
/// capture artifacts inherit the same confidentiality boundary. No sensitive bytes are written
/// until this succeeds.
pub fn ensure_owner_only_directory(path: &Path) -> Result<()> {
    ensure_private_directory(path)?;
    let sid = crate::identity::current_user_sid()?;
    let sddl = HSTRING::from(format!("D:P(A;OICI;GA;;;SY)(A;OICI;GA;;;{sid})"));
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: input HSTRING remains live and output is LocalAlloc-owned on success.
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
            "Windows private-directory security descriptor creation failed",
        )
    })?;
    let descriptor = OwnedSecurityDescriptor(descriptor);
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: path is NUL-terminated for this synchronous call and descriptor stays live.
    unsafe {
        windows::Win32::Security::SetFileSecurityW(
            windows::core::PCWSTR(wide.as_ptr()),
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor.0,
        )
    }
    .ok()
    .map_err(|_| {
        Error::new(
            ErrorCode::PermissionDenied,
            "Windows private-directory DACL could not be applied",
        )
    })?;
    let meta = std::fs::symlink_metadata(path)?;
    if !meta.is_dir()
        || meta.file_type().is_symlink()
        || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
    {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Windows private directory changed during DACL hardening",
        ));
    }
    Ok(())
}

pub fn paths() -> Result<PlatformPaths> {
    let local = known_folder(&FOLDERID_LocalAppData)?.join("Semwright");
    let roaming = known_folder(&FOLDERID_RoamingAppData)?.join("Semwright");
    let paths = PlatformPaths {
        runtime: local.join("runtime"),
        state: local.join("state"),
        config: roaming.join("config"),
        cache: local.join("cache"),
    };
    for path in [&paths.runtime, &paths.state, &paths.config, &paths.cache] {
        ensure_private_directory(path)?;
    }
    Ok(paths)
}
