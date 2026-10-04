//! Keep an owned log tail before session cleanup; public diagnostics omit text/paths.
use semwright_types::{NativeDiagnostic, NativeFailurePhase, NativeFailureReason};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{self, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{fs::FileExt, process::ExitStatusExt},
    },
    path::Path,
    process::ExitStatus,
};

pub const MAX_LOG_TAIL: usize = 8192;

fn private_audit_dir(workspace: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    let parent = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(workspace)?;
    let name = c".semwright-native-failures";
    // SAFETY: the owned directory descriptor and fixed NUL-terminated child name
    // remain live for mkdirat. The inherited sandbox still applies.
    let rc = unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) };
    if rc != 0 && io::Error::last_os_error().kind() != io::ErrorKind::AlreadyExists {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: openat receives the same live descriptor and fixed child name.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful openat returned a new descriptor owned by this function.
    let dir = unsafe { File::from_raw_fd(fd) };
    use std::os::unix::fs::MetadataExt;
    let metadata = dir.metadata()?;
    // SAFETY: geteuid takes no arguments and only reads the process identity.
    let uid = unsafe { libc::geteuid() };
    if metadata.uid() != uid || metadata.mode() & 0o077 != 0 {
        return Err(io::Error::other(
            "Native audit directory is not private and owned",
        ));
    }
    Ok(dir)
}

pub fn capture(
    log: &File,
    workspace: &Path,
    status: Option<ExitStatus>,
    phase: NativeFailurePhase,
    error: &io::Error,
) -> io::Result<NativeDiagnostic> {
    let length = log.metadata()?.len();
    let offset = length.saturating_sub(MAX_LOG_TAIL as u64);
    let mut tail = vec![0; (length - offset) as usize];
    log.read_exact_at(&mut tail, offset)?;
    let diagnostic = NativeDiagnostic {
        phase,
        reason: if status.is_some() {
            NativeFailureReason::ChildExit
        } else {
            match error.kind() {
                io::ErrorKind::UnexpectedEof => NativeFailureReason::UnexpectedEof,
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => NativeFailureReason::Timeout,
                io::ErrorKind::InvalidData => NativeFailureReason::InvalidFrame,
                _ => NativeFailureReason::IoFailure,
            }
        },
        exit_code: status.and_then(|s| s.code()),
        signal: status.and_then(|s| s.signal()),
        stderr_tail_sha256: hex::encode(Sha256::digest(&tail)),
        stderr_tail_bytes: tail.len() as u16,
        stderr_tail_truncated: length > MAX_LOG_TAIL as u64,
    };
    if !diagnostic.is_valid() {
        return Err(io::Error::other("Invalid native diagnostic"));
    }
    let directory = private_audit_dir(workspace)?;
    let id = semwright_types::unique_id();
    for (extension, bytes) in [("log", tail), ("json", serde_json::to_vec(&diagnostic)?)] {
        let name = std::ffi::CString::new(format!("blender-{id}.{extension}"))
            .map_err(io::Error::other)?;
        // SAFETY: the owned directory and validated NUL-terminated filename
        // remain live; O_EXCL/O_NOFOLLOW prevent replacing an existing entry.
        let fd = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: successful openat returned a newly owned descriptor.
        let mut artifact = unsafe { File::from_raw_fd(fd) };
        artifact.write_all(&bytes)?;
        artifact.sync_all()?;
    }
    directory.sync_all()?;
    Ok(diagnostic)
}
