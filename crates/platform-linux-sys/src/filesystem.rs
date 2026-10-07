//! Linux openat2 confinement. No canonicalize-then-open race for relative operations.
//! Kernel < 5.6 / blocked openat2 fails closed. No fallback to unsafe string paths.
use semwright_platform_api::filesystem::MAX_SCOPED_BINARY_BYTES;
use semwright_types::{Error, ErrorCode, Result};
use std::ffi::{CString, OsStr};
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path};

const RESOLVE_NO_XDEV: u64 = 0x01;
const RESOLVE_NO_MAGICLINKS: u64 = 0x02;
const RESOLVE_NO_SYMLINKS: u64 = 0x04;
const RESOLVE_BENEATH: u64 = 0x08;
#[repr(C)]
struct OpenHow {
    flags: u64,
    mode: u64,
    resolve: u64,
}
fn cstring(s: &OsStr) -> Result<CString> {
    CString::new(s.as_bytes()).map_err(|_| Error::invalid("NUL in filesystem path"))
}
pub fn validate_relative_path(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty() || path.as_os_str().as_bytes().len() > 4096 || path.is_absolute()
    {
        return Err(Error::invalid("Path must be a nonempty relative path"));
    }
    for component in path.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Path traversal is not permitted",
            ));
        }
    }
    // Path::components normalizes interior '.', so reject those lexically too.
    if path
        .as_os_str()
        .as_bytes()
        .split(|b| *b == b'/')
        .any(|c| c.is_empty() || c == b"." || c == b"..")
    {
        return Err(Error::invalid(
            "Empty, dot and parent path components are forbidden",
        ));
    }
    cstring(path.as_os_str())?;
    Ok(())
}
fn open_beneath(dir: &OwnedFd, path: &Path, flags: i32, mode: u32) -> Result<OwnedFd> {
    validate_relative_path(path)?;
    let name = cstring(path.as_os_str())?;
    let how = OpenHow {
        flags: (flags | libc::O_CLOEXEC) as u64,
        mode: mode as u64,
        resolve: RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS | RESOLVE_NO_MAGICLINKS | RESOLVE_NO_XDEV,
    };
    // SAFETY: dir is a live fd; C string and repr(C) OpenHow remain alive for this
    // synchronous syscall. The kernel only reads exactly size_of::<OpenHow>() bytes.
    let fd = unsafe {
        libc::syscall(
            libc::SYS_openat2,
            dir.as_raw_fd(),
            name.as_ptr(),
            &how,
            std::mem::size_of::<OpenHow>(),
        )
    };
    if fd < 0 {
        let e = std::io::Error::last_os_error();
        let code = match e.raw_os_error() {
            Some(libc::ENOSYS | libc::EPERM) => ErrorCode::Unavailable,
            Some(libc::ENOENT) => ErrorCode::NotFound,
            _ => ErrorCode::PolicyDenied,
        };
        return Err(Error::new(
            code,
            "Confined open failed; symlinks, mount crossings and unsupported kernels fail closed",
        ));
    }
    // SAFETY: successful openat2 returns a new fd owned uniquely by this function.
    Ok(unsafe { OwnedFd::from_raw_fd(fd as i32) })
}
pub struct Root {
    fd: OwnedFd,
    pub readable: bool,
    pub writable: bool,
}
impl Root {
    pub fn open(path: &Path, readable: bool, writable: bool) -> Result<Self> {
        if !path.is_absolute() || path == Path::new("/") {
            return Err(Error::invalid(
                "An explicit absolute workspace root is required",
            ));
        }
        let canonical = std::fs::canonicalize(path)?;
        if canonical != path {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Configured roots must be canonical, without symlinks",
            ));
        }
        let name = cstring(path.as_os_str())?;
        // SAFETY: name is a valid NUL-terminated C string. O_NOFOLLOW rejects a
        // replacement final symlink; the opened directory fd pins the actual root.
        let fd = unsafe {
            libc::open(
                name.as_ptr(),
                libc::O_PATH | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        // SAFETY: open returned a new owned descriptor.
        Ok(Self {
            // SAFETY: successful open returned a new, uniquely owned descriptor.
            fd: unsafe { OwnedFd::from_raw_fd(fd) },
            readable,
            writable,
        })
    }
    pub fn read(&self, path: &Path, limit: usize) -> Result<Vec<u8>> {
        if !self.readable {
            return Err(Error::new(ErrorCode::PolicyDenied, "Root is not readable"));
        }
        if limit == 0 || limit > MAX_SCOPED_BINARY_BYTES {
            return Err(Error::invalid("Read limit exceeds scoped binary budget"));
        }
        let fd = open_beneath(&self.fd, path, libc::O_RDONLY | libc::O_NONBLOCK, 0)?;
        let file = File::from(fd);
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.nlink() != 1 {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Only regular single-link files may be read",
            ));
        }
        if metadata.len() > limit as u64 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "File exceeds read budget",
            ));
        }
        let mut bytes = Vec::new();
        file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > limit {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "File grew beyond read budget",
            ));
        }
        Ok(bytes)
    }
    /// openat2-confined instance observation. Revalidation is best effort, not CAS.
    pub fn observe_file(
        &self,
        path: &Path,
        limit: usize,
    ) -> Result<semwright_platform_api::filesystem::ScopedFileObservation> {
        if !self.readable {
            return Err(Error::new(ErrorCode::PolicyDenied, "Root is not readable"));
        }
        if limit == 0 || limit > MAX_SCOPED_BINARY_BYTES {
            return Err(Error::invalid("Observation exceeds scoped binary budget"));
        }
        let fd = open_beneath(&self.fd, path, libc::O_RDONLY | libc::O_NONBLOCK, 0)?;
        let mut file = File::from(fd);
        let before = file.metadata()?;
        if !before.is_file() || before.nlink() != 1 {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Only regular single-link files may be observed",
            ));
        }
        if before.len() > limit as u64 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Observation exceeds read budget",
            ));
        }
        let born = before
            .created()
            .map_err(|_| {
                Error::new(
                    ErrorCode::Unsupported,
                    "Creation-time instance evidence unavailable",
                )
            })?
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| Error::new(ErrorCode::Unsupported, "Unsupported file creation epoch"))?;
        let mut bytes = Vec::new();
        Read::by_ref(&mut file)
            .take(limit as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > limit {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "File grew beyond read budget",
            ));
        }
        let after = file.metadata()?;
        let current = File::from(open_beneath(
            &self.fd,
            path,
            libc::O_RDONLY | libc::O_NONBLOCK,
            0,
        )?)
        .metadata()?;
        let stamp = |m: &std::fs::Metadata| {
            (
                m.dev(),
                m.ino(),
                m.len(),
                m.nlink(),
                m.mtime(),
                m.mtime_nsec(),
                m.ctime(),
                m.ctime_nsec(),
                m.created().ok(),
            )
        };
        if stamp(&before) != stamp(&after)
            || stamp(&after) != stamp(&current)
            || bytes.len() as u64 != after.len()
        {
            return Err(Error::new(
                ErrorCode::Conflict,
                "File changed during bounded observation",
            ));
        }
        Ok(semwright_platform_api::filesystem::ScopedFileObservation {
            bytes,
            instance_identity: format!(
                "linux-file-instance-v1:{}:{}:{}",
                before.dev(),
                before.ino(),
                born.as_nanos()
            ),
            method: "openat2-fd-btime-read-revalidate",
            method_version: 1,
        })
    }

    pub fn write_atomic(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        self.write_atomic_mode(path, bytes, false)
    }

    pub fn write_new_atomic(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        self.write_atomic_mode(path, bytes, true)
    }

    fn write_atomic_mode(&self, path: &Path, bytes: &[u8], no_replace: bool) -> Result<()> {
        if !self.writable {
            return Err(Error::new(ErrorCode::PolicyDenied, "Root is not writable"));
        }
        if bytes.len() > MAX_SCOPED_BINARY_BYTES {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Write exceeds scoped binary budget",
            ));
        }
        validate_relative_path(path)?;
        let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
        let parent_fd = match parent {
            Some(p) => open_beneath(&self.fd, p, libc::O_RDONLY | libc::O_DIRECTORY, 0)?,
            None => {
                // Opening '.' is internal and not derived from the caller. Obtain an
                // fsync-capable descriptor from the pinned O_PATH root.
                let name =
                    CString::new(".").map_err(|_| Error::invalid("Invalid constant path"))?;
                // SAFETY: root fd is live and name is a constant valid C string.
                let fd = unsafe {
                    libc::openat(
                        self.fd.as_raw_fd(),
                        name.as_ptr(),
                        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
                    )
                };
                if fd < 0 {
                    return Err(std::io::Error::last_os_error().into());
                }
                // SAFETY: openat returned a new uniquely owned descriptor.
                unsafe { OwnedFd::from_raw_fd(fd) }
            }
        };
        let basename = path
            .file_name()
            .ok_or_else(|| Error::invalid("File basename missing"))?;
        let name = cstring(basename)?;
        if !no_replace {
            // Existing symlinks/hardlinks are rejected, not followed. Rename remains
            // relative to this pinned directory even if ancestors are moved later.
            match open_beneath(
                &parent_fd,
                Path::new(basename),
                libc::O_RDONLY | libc::O_NONBLOCK,
                0,
            ) {
                Ok(fd) => {
                    let m = File::from(fd).metadata()?;
                    if !m.is_file() || m.nlink() != 1 {
                        return Err(Error::new(
                            ErrorCode::PolicyDenied,
                            "Unsafe replacement target",
                        ));
                    }
                }
                Err(e) if e.code == ErrorCode::NotFound => (),
                Err(e) => return Err(e),
            }
        }
        let temporary = CString::new(format!(".semwright-{}", uuid::Uuid::new_v4().simple()))
            .map_err(|_| Error::invalid("Invalid temporary name"))?;
        let temp_path = Path::new(OsStr::from_bytes(temporary.as_bytes()));
        let fd = open_beneath(
            &parent_fd,
            temp_path,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
            0o600,
        )?;
        let mut published = false;
        let result = (|| -> Result<()> {
            let mut file = File::from(fd);
            file.write_all(bytes)?;
            file.sync_all()?;
            if no_replace {
                #[cfg(test)]
                NO_REPLACE_BEFORE_PUBLISH.with(|hook| {
                    let callback = hook.borrow_mut().take();
                    if let Some(callback) = callback {
                        callback();
                    }
                });
                publish_new(&parent_fd, &temporary, &name)?;
            } else {
                // SAFETY: descriptors are live; renameat changes directory entries
                // and does not follow the destination basename.
                if unsafe {
                    libc::renameat(
                        parent_fd.as_raw_fd(),
                        temporary.as_ptr(),
                        parent_fd.as_raw_fd(),
                        name.as_ptr(),
                    )
                } != 0
                {
                    return Err(std::io::Error::last_os_error().into());
                }
            }
            published = true;
            #[cfg(test)]
            if no_replace && NO_REPLACE_SYNC_FAILURE.with(|fault| fault.replace(false)) {
                return Err(Error::new(
                    ErrorCode::BackendFailed,
                    "Injected directory sync failure",
                )
                .uncertain());
            }
            // SAFETY: the same pinned directory descriptor is fsync-capable.
            if unsafe { libc::fsync(parent_fd.as_raw_fd()) } != 0 {
                let error: Error = std::io::Error::last_os_error().into();
                return Err(if no_replace { error.uncertain() } else { error });
            }
            Ok(())
        })();
        if result.is_err() {
            // SAFETY: cleanup names only our temporary in the pinned parent.
            // A published destination is never deleted, even after a sync error.
            #[cfg(test)]
            let inject_cleanup_failure =
                no_replace && NO_REPLACE_CLEANUP_FAILURE.with(|fault| fault.replace(false));
            #[cfg(not(test))]
            let inject_cleanup_failure = false;
            let cleanup_errno = if inject_cleanup_failure {
                Some(libc::EACCES)
            } else {
                // SAFETY: parent_fd is live and temporary is our valid basename;
                // unlinkat only removes that entry in the pinned directory.
                let removed =
                    unsafe { libc::unlinkat(parent_fd.as_raw_fd(), temporary.as_ptr(), 0) } == 0;
                if removed {
                    None
                } else {
                    Some(
                        std::io::Error::last_os_error()
                            .raw_os_error()
                            .unwrap_or(libc::EIO),
                    )
                }
            };
            if no_replace && !published && cleanup_errno.is_some_and(|errno| errno != libc::ENOENT)
            {
                return result.map_err(|error| error.uncertain());
            }
        }
        result
    }
}
impl semwright_platform_api::filesystem::ScopedRoot for Root {
    fn observe_file(
        &self,
        path: &Path,
        limit: usize,
    ) -> Result<semwright_platform_api::filesystem::ScopedFileObservation> {
        Root::observe_file(self, path, limit)
    }
    fn confinement(&self) -> semwright_platform_api::filesystem::Confinement {
        semwright_platform_api::filesystem::Confinement::LinuxOpenat2NoSymlinksNoMounts
    }
    fn read(&self, path: &Path, limit: usize) -> Result<Vec<u8>> {
        Root::read(self, path, limit)
    }
    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        Root::write_atomic(self, path, bytes)
    }
    fn write_new_atomic(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        Root::write_new_atomic(self, path, bytes)
    }
}

// This primitive has no replacement fallback. Errors with an ambiguous storage
// outcome stay uncertain rather than authorizing another publication attempt.
fn publish_new(parent: &OwnedFd, temporary: &CString, name: &CString) -> Result<()> {
    #[cfg(test)]
    if let Some(errno) = NO_REPLACE_RENAME_ERRNO.with(|fault| fault.replace(None)) {
        return Err(no_replace_error(errno));
    }
    // SAFETY: live pinned directory, valid basenames, and the fixed kernel flag.
    let rc = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            parent.as_raw_fd(),
            temporary.as_ptr(),
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if rc == 0 {
        return Ok(());
    }
    Err(no_replace_error(
        std::io::Error::last_os_error()
            .raw_os_error()
            .unwrap_or(libc::EIO),
    ))
}
fn no_replace_error(errno: i32) -> Error {
    match errno {
        libc::EEXIST => Error::new(ErrorCode::Conflict, "Destination already exists"),
        libc::ENOSYS | libc::EINVAL | libc::EOPNOTSUPP => Error::new(
            ErrorCode::Unsupported,
            "Atomic no-replace publication is unsupported",
        ),
        libc::EPERM | libc::EACCES => Error::new(
            ErrorCode::PermissionDenied,
            "Atomic no-replace publication was denied",
        ),
        _ => Error::new(
            ErrorCode::BackendFailed,
            "Atomic no-replace publication failed",
        )
        .uncertain(),
    }
}
#[cfg(test)]
type BeforePublishHook = std::cell::RefCell<Option<Box<dyn FnOnce()>>>;

#[cfg(test)]
thread_local! {
    static NO_REPLACE_RENAME_ERRNO: std::cell::Cell<Option<i32>> = const { std::cell::Cell::new(None) };
    static NO_REPLACE_SYNC_FAILURE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static NO_REPLACE_CLEANUP_FAILURE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static NO_REPLACE_BEFORE_PUBLISH: BeforePublishHook = const { std::cell::RefCell::new(None) };
}

/// Factory passed through the platform contract; callers never inspect native fds.
pub struct LinuxFilesystem;
impl semwright_platform_api::filesystem::ScopedFilesystem for LinuxFilesystem {
    fn open_root(
        &self,
        path: &Path,
        read: bool,
        write: bool,
    ) -> Result<Box<dyn semwright_platform_api::filesystem::ScopedRoot>> {
        Ok(Box::new(Root::open(path, read, write)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    #[test]
    fn traversal_rejected() {
        for p in ["../secret", "a/../b", "/etc/passwd", "a/./b", "a//b", ""] {
            assert!(validate_relative_path(Path::new(p)).is_err(), "{p}");
        }
    }
    #[test]
    fn atomic_scoped_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let r = Root::open(dir.path(), true, true).unwrap();
        r.write_atomic(Path::new("sample"), b"hello").unwrap();
        assert_eq!(r.read(Path::new("sample"), 10).unwrap(), b"hello");
    }
    #[test]
    fn symlink_escape_fails() {
        let dir = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink("/etc/passwd", dir.path().join("escape")).unwrap();
        let r = Root::open(dir.path(), true, true).unwrap();
        assert!(r.read(Path::new("escape"), 10000).is_err());
        assert!(r.write_atomic(Path::new("escape"), b"x").is_err());
    }
    #[test]
    fn hardlinks_are_not_a_read_capability() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        std::fs::write(a.path().join("secret"), "x").unwrap();
        std::fs::hard_link(a.path().join("secret"), b.path().join("link")).unwrap();
        let r = Root::open(b.path(), true, false).unwrap();
        assert!(r.read(Path::new("link"), 10).is_err());
    }
    fn no_temporary_files(path: &Path) {
        assert!(std::fs::read_dir(path).unwrap().all(|e| {
            !e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".semwright-")
        }));
    }
    #[test]
    fn write_new_atomic_creates_and_never_replaces_existing_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let root = Root::open(dir.path(), true, true).unwrap();
        root.write_new_atomic(Path::new("result"), b"first")
            .unwrap();
        let error = root
            .write_new_atomic(Path::new("result"), b"second")
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Conflict);
        assert!(error.outcome_known);
        assert_eq!(root.read(Path::new("result"), 100).unwrap(), b"first");
        no_temporary_files(dir.path());
        root.write_atomic(Path::new("result"), b"legacy replacement")
            .unwrap();
        assert_eq!(
            root.read(Path::new("result"), 100).unwrap(),
            b"legacy replacement"
        );
    }
    #[test]
    fn write_new_atomic_concurrent_publishers_have_one_winner() {
        let dir = tempfile::tempdir().unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let handles = [b"first".to_vec(), b"second".to_vec()].map(|bytes| {
            let path = dir.path().to_path_buf();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let root = Root::open(&path, true, true).unwrap();
                NO_REPLACE_BEFORE_PUBLISH.with(|hook| {
                    *hook.borrow_mut() = Some(Box::new(move || {
                        barrier.wait();
                    }))
                });
                (
                    bytes.clone(),
                    root.write_new_atomic(Path::new("winner"), &bytes),
                )
            })
        });
        let results = handles.map(|h| h.join().unwrap());
        assert_eq!(results.iter().filter(|(_, r)| r.is_ok()).count(), 1);
        let (winner, _) = results.iter().find(|(_, r)| r.is_ok()).unwrap();
        let (_, loser) = results.iter().find(|(_, r)| r.is_err()).unwrap();
        assert_eq!(loser.as_ref().unwrap_err().code, ErrorCode::Conflict);
        assert_eq!(std::fs::read(dir.path().join("winner")).unwrap(), *winner);
        no_temporary_files(dir.path());
    }
    #[test]
    fn write_new_atomic_rejects_existing_symlink_hardlink_and_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("original"), b"preserved").unwrap();
        std::os::unix::fs::symlink("original", dir.path().join("symlink")).unwrap();
        std::fs::hard_link(dir.path().join("original"), dir.path().join("hardlink")).unwrap();
        std::fs::create_dir(dir.path().join("directory")).unwrap();
        let root = Root::open(dir.path(), true, true).unwrap();
        for name in ["symlink", "hardlink", "directory"] {
            assert_eq!(
                root.write_new_atomic(Path::new(name), b"new")
                    .unwrap_err()
                    .code,
                ErrorCode::Conflict
            );
        }
        assert_eq!(
            std::fs::read(dir.path().join("original")).unwrap(),
            b"preserved"
        );
        assert!(
            std::fs::symlink_metadata(dir.path().join("symlink"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(dir.path().join("directory").is_dir());
        no_temporary_files(dir.path());
    }
    #[test]
    fn write_new_atomic_refuses_ungranted_or_unconfined_parent() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let readonly = Root::open(dir.path(), true, false).unwrap();
        assert_eq!(
            readonly
                .write_new_atomic(Path::new("new"), b"new")
                .unwrap_err()
                .code,
            ErrorCode::PolicyDenied
        );
        std::os::unix::fs::symlink(outside.path(), dir.path().join("parent")).unwrap();
        let root = Root::open(dir.path(), true, true).unwrap();
        for name in ["../new", "parent/new", "absent/new"] {
            assert!(root.write_new_atomic(Path::new(name), b"new").is_err());
        }
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
        no_temporary_files(dir.path());
    }
    #[test]
    fn write_new_atomic_keeps_the_parent_descriptor_after_path_replacement() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("parent")).unwrap();
        let path = dir.path().to_path_buf();
        let root = Root::open(&path, true, true).unwrap();
        NO_REPLACE_BEFORE_PUBLISH.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                std::fs::rename(path.join("parent"), path.join("original-parent")).unwrap();
                std::fs::create_dir(path.join("parent")).unwrap();
            }))
        });
        root.write_new_atomic(Path::new("parent/result"), b"pinned")
            .unwrap();
        assert!(!dir.path().join("parent/result").exists());
        assert_eq!(
            std::fs::read(dir.path().join("original-parent/result")).unwrap(),
            b"pinned"
        );
        no_temporary_files(&dir.path().join("original-parent"));
    }
    #[test]
    fn write_new_atomic_unsupported_and_denied_never_fall_back() {
        let dir = tempfile::tempdir().unwrap();
        let root = Root::open(dir.path(), true, true).unwrap();
        for errno in [
            libc::ENOSYS,
            libc::EINVAL,
            libc::EOPNOTSUPP,
            libc::EPERM,
            libc::EIO,
        ] {
            NO_REPLACE_RENAME_ERRNO.with(|fault| fault.set(Some(errno)));
            let error = root
                .write_new_atomic(Path::new("result"), b"never published")
                .unwrap_err();
            assert_eq!(
                error.code,
                match errno {
                    libc::EPERM => ErrorCode::PermissionDenied,
                    libc::EIO => ErrorCode::BackendFailed,
                    _ => ErrorCode::Unsupported,
                }
            );
            assert_eq!(error.outcome_known, errno != libc::EIO);
            assert!(!dir.path().join("result").exists());
            no_temporary_files(dir.path());
        }
    }
    #[test]
    fn write_new_atomic_post_publication_sync_error_preserves_unknown_destination() {
        let dir = tempfile::tempdir().unwrap();
        let root = Root::open(dir.path(), true, true).unwrap();
        NO_REPLACE_SYNC_FAILURE.with(|fault| fault.set(true));
        let error = root
            .write_new_atomic(Path::new("result"), b"published")
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::BackendFailed);
        assert!(!error.outcome_known);
        assert_eq!(root.read(Path::new("result"), 100).unwrap(), b"published");
        assert_eq!(
            root.write_new_atomic(Path::new("result"), b"retry")
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        assert_eq!(root.read(Path::new("result"), 100).unwrap(), b"published");
        no_temporary_files(dir.path());
    }
    #[test]
    fn write_new_atomic_cleanup_failure_keeps_original_conflict_and_uncertainty() {
        let dir = tempfile::tempdir().unwrap();
        let root = Root::open(dir.path(), true, true).unwrap();
        std::fs::write(dir.path().join("result"), b"untouched").unwrap();
        NO_REPLACE_CLEANUP_FAILURE.with(|fault| fault.set(true));
        let error = root
            .write_new_atomic(Path::new("result"), b"temporary only")
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Conflict);
        assert_eq!(error.message, "Destination already exists");
        assert!(!error.outcome_known);
        assert_eq!(root.read(Path::new("result"), 100).unwrap(), b"untouched");
        let temporary: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".semwright-")
            })
            .collect();
        assert_eq!(temporary.len(), 1);
        assert_eq!(
            std::fs::read(temporary[0].path()).unwrap(),
            b"temporary only"
        );
    }
    proptest! {#[test]fn parent_prefix_never_passes(s in "[a-z]{1,100}"){let path = format!("../{s}"); prop_assert!(validate_relative_path(Path::new(&path)).is_err());}}
}
