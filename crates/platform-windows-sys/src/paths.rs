use semwright_platform_api::PlatformPaths;
use semwright_types::{Error, ErrorCode, Result};
use std::path::{Path, PathBuf};
use windows::{
    Win32::UI::Shell::{
        FOLDERID_LocalAppData, FOLDERID_RoamingAppData, KF_FLAG_DEFAULT, SHGetKnownFolderPath,
    },
    core::PWSTR,
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
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Windows private path must be a non-reparse directory",
        ));
    }
    // DACL/owner validation is performed by the Named Pipe and executable/file primitives at the
    // point where authority is consumed. Directory existence alone never grants authority.
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
