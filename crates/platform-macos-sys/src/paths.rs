use semwright_platform_api::PlatformPaths;
use semwright_types::{Error, ErrorCode, Result};
use std::{
    ffi::CStr,
    os::unix::{
        ffi::OsStrExt,
        fs::{DirBuilderExt, MetadataExt},
    },
    path::{Path, PathBuf},
};
fn private(path: &Path) -> Result<()> {
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)?;
    let m = std::fs::symlink_metadata(path)?;
    // SAFETY: getuid has no pointer arguments.
    if !m.is_dir()
        || m.file_type().is_symlink()
        || m.uid() != unsafe { libc::getuid() }
        || m.mode() & 0o777 != 0o700
    {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Unsafe private macOS directory",
        ));
    }
    Ok(())
}
fn home() -> Result<PathBuf> {
    let mut buf = vec![0u8; 65536];
    // SAFETY: zero initialization is valid for the C passwd struct; no field is read
    // until getpwuid_r succeeds and returns a non-null result within the live buffer.
    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut ptr = std::ptr::null_mut();
    // SAFETY: all output buffers have their declared lengths, remain live through copying.
    let code = unsafe {
        libc::getpwuid_r(
            libc::getuid(),
            &mut entry,
            buf.as_mut_ptr().cast(),
            buf.len(),
            &mut ptr,
        )
    };
    if code != 0 || ptr.is_null() || entry.pw_dir.is_null() {
        return Err(Error::unavailable("Account home lookup failed"));
    }
    // SAFETY: pw_dir is a NUL-terminated string in the still-live getpwuid_r buffer.
    let bytes = unsafe { CStr::from_ptr(entry.pw_dir) }.to_bytes();
    let p = PathBuf::from(std::ffi::OsStr::from_bytes(bytes));
    if !p.is_absolute() {
        return Err(Error::invalid("Account home is not absolute"));
    }
    Ok(p)
}
pub fn paths() -> Result<PlatformPaths> {
    let h = home()?;
    let mut temp = vec![0u8; 4096];
    // SAFETY: confstr writes at most the specified live buffer length.
    let n = unsafe {
        libc::confstr(
            libc::_CS_DARWIN_USER_TEMP_DIR,
            temp.as_mut_ptr().cast(),
            temp.len(),
        )
    };
    if n == 0 || n > temp.len() {
        return Err(Error::unavailable(
            "Per-user Darwin temporary directory unavailable",
        ));
    }
    let base = std::fs::canonicalize(Path::new(std::ffi::OsStr::from_bytes(&temp[..n - 1])))?;
    let m = std::fs::symlink_metadata(&base)?;
    // SAFETY: getuid has no pointer arguments.
    if !m.is_dir() || m.uid() != unsafe { libc::getuid() } || m.mode() & 0o077 != 0 {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Temporary directory is not private",
        ));
    }
    let runtime = base.join("semwright");
    if runtime
        .join("broker.sock")
        .as_os_str()
        .as_encoded_bytes()
        .len()
        > 103
    {
        return Err(Error::invalid("Darwin Unix socket path exceeds sun_path"));
    }
    let p = PlatformPaths {
        runtime,
        state: h.join("Library/Application Support/Semwright"),
        config: h.join("Library/Application Support/Semwright/config"),
        cache: h.join("Library/Caches/Semwright"),
    };
    for d in [&p.runtime, &p.state, &p.config, &p.cache] {
        private(d)?;
    }
    Ok(p)
}
