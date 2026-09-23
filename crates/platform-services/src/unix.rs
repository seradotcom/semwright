use semwright_types::{Error, ErrorCode, Result};
use std::{
    os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt},
    path::Path,
};
use tokio::net::UnixStream;
pub fn current_uid() -> u32 {
    // SAFETY: getuid has no pointer parameters.
    unsafe { libc::getuid() }
}
pub fn private_directory(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(m) => {
            if !m.is_dir()
                || m.file_type().is_symlink()
                || m.uid() != current_uid()
                || m.permissions().mode() & 0o777 != 0o700
            {
                return Err(Error::new(
                    ErrorCode::PermissionDenied,
                    "Directory must be owner-only mode 0700",
                ));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::DirBuilder::new().mode(0o700).create(path)?;
        }
        Err(e) => return Err(e.into()),
    };
    Ok(())
}
#[cfg(target_os = "linux")]
pub fn validate_peer(stream: &UnixStream) -> Result<()> {
    if stream.peer_cred()?.uid() != current_uid() {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Unix peer UID does not match owner",
        ));
    }
    Ok(())
}
#[cfg(target_os = "macos")]
pub fn validate_peer(stream: &UnixStream) -> Result<()> {
    use std::os::fd::AsRawFd;
    let (mut uid, mut gid) = (0, 0);
    // SAFETY: connected live socket and writable uid/gid stack storage. getpeereid
    // authenticates the peer identity captured by the kernel, not client JSON.
    if unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) } != 0 {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Peer credentials unavailable",
        ));
    }
    if uid != current_uid() {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Unix peer UID does not match owner",
        ));
    }
    Ok(())
}
