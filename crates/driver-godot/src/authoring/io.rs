//! Linux descriptor-relative managed file operations. Never follow caller symlinks.
//! Hash revalidation is best-effort against other writers, not filesystem CAS.
use semwright_semantic_composition::Digest;
use std::{
    ffi::CString,
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::Path,
};
pub(crate) struct Directory {
    file: File,
}
fn component(s: &str) -> io::Result<CString> {
    if s.is_empty()
        || s == "."
        || s == ".."
        || s.len() > 240
        || s.bytes().any(|b| b == 0 || b == b'/' || b == b'\\')
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsafe managed path component",
        ));
    }
    CString::new(s).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL path"))
}
fn owned(raw: i32) -> io::Result<File> {
    if raw < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful openat returned a new, uniquely owned file descriptor.
    Ok(File::from(unsafe { OwnedFd::from_raw_fd(raw) }))
}
impl Directory {
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        if !path.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "absolute granted root required",
            ));
        }
        for part in path.components() {
            match part {
                std::path::Component::RootDir => {}
                std::path::Component::Normal(name) => {
                    let name = name.to_str().ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidInput, "UTF-8 granted root required")
                    })?;
                    component(name)?;
                }
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "noncanonical granted root",
                    ));
                }
            }
        }

        // Driver Host intentionally grants only the exact materialized
        // /workspace/<logical-root> mount, not ReadDir over its /workspace parent.
        // Walking from "/" with openat therefore asks Landlock for authority the
        // driver does not have. AuthoringConfig already requires the owner path to
        // canonicalize to itself; repeat that invariant here, then open the exact
        // granted root in one operation while refusing a final symlink.
        let canonical = std::fs::canonicalize(path)?;
        if canonical != path {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "granted root must be canonical and non-symlinked",
            ));
        }
        let before = std::fs::symlink_metadata(path)?;
        if !before.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "granted root must be a directory",
            ));
        }
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)?;
        let opened = file.metadata()?;
        let after = std::fs::symlink_metadata(path)?;
        if !opened.is_dir()
            || !after.is_dir()
            || opened.dev() != before.dev()
            || opened.ino() != before.ino()
            || opened.dev() != after.dev()
            || opened.ino() != after.ino()
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "granted root changed while opening",
            ));
        }
        Ok(Self { file })
    }
    fn clone_dir(&self) -> io::Result<Self> {
        Ok(Self {
            file: self.file.try_clone()?,
        })
    }
    pub(crate) fn child(&self, name: &str, create: bool) -> io::Result<Self> {
        let c = component(name)?;
        if create {
            // SAFETY: live directory descriptor and a NUL-terminated single component.
            let rc = unsafe { libc::mkdirat(self.file.as_raw_fd(), c.as_ptr(), 0o700) };
            if rc < 0 {
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::AlreadyExists {
                    return Err(error);
                }
            }
        }
        // SAFETY: live parent descriptor, valid component, no-follow directory open.
        let fd = unsafe {
            libc::openat(
                self.file.as_raw_fd(),
                c.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        Ok(Self { file: owned(fd)? })
    }
    pub(crate) fn create_child(&self, name: &str) -> io::Result<Self> {
        let c = component(name)?;
        // SAFETY: live directory descriptor and a NUL-terminated single component.
        if unsafe { libc::mkdirat(self.file.as_raw_fd(), c.as_ptr(), 0o700) } < 0 {
            return Err(io::Error::last_os_error());
        }
        self.child(name, false)
    }
    fn parent(&self, path: &str, create: bool) -> io::Result<(Self, String)> {
        let parts: Vec<_> = path.split('/').collect();
        if parts.is_empty() || parts.len() > 16 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "managed path depth",
            ));
        }
        let mut directory = self.clone_dir()?;
        for part in &parts[..parts.len() - 1] {
            directory = directory.child(part, create)?;
        }
        component(parts[parts.len() - 1])?;
        Ok((directory, parts[parts.len() - 1].into()))
    }
    pub(crate) fn read(&self, path: &str, limit: u64) -> io::Result<Option<Vec<u8>>> {
        let (dir, name) = match self.parent(path, false) {
            Ok(value) => value,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let c = component(&name)?;
        // SAFETY: live parent descriptor and valid single component; no links are followed.
        let raw = unsafe {
            libc::openat(
                dir.file.as_raw_fd(),
                c.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            )
        };
        let file = match owned(raw) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let meta = file.metadata()?;
        if !meta.is_file() || meta.nlink() != 1 || meta.len() > limit {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "managed file must be bounded, regular and single-linked",
            ));
        }
        let mut bytes = Vec::new();
        file.take(limit + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "file grew during read",
            ));
        }
        Ok(Some(bytes))
    }
    pub(crate) fn entries(&self) -> io::Result<Vec<String>> {
        // The proc path names this process's own directory descriptor, not a client locator.
        let path = format!("/proc/self/fd/{}", self.file.as_raw_fd());
        let mut names = Vec::new();
        for entry in std::fs::read_dir(path)? {
            if names.len() >= 4096 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "directory entry budget",
                ));
            }
            let name = entry?.file_name().into_string().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "non-UTF8 directory entry")
            })?;
            names.push(name);
        }
        names.sort();
        Ok(names)
    }
    pub(crate) fn write(
        &self,
        path: &str,
        bytes: &[u8],
        expected: Option<&Digest>,
    ) -> io::Result<()> {
        if bytes.len() > 67_108_864 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "write byte budget",
            ));
        }
        let (dir, name) = self.parent(path, true)?;
        let old = dir.read(&name, 67_108_864)?;
        if old.as_deref().map(Digest::of_bytes).as_ref() != expected {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "DIVERGED: write precondition changed",
            ));
        }
        let temporary = format!(".sw-{}", semwright_types::unique_id());
        let tmp = component(&temporary)?;
        let dest = component(&name)?;
        // SAFETY: valid directory descriptor, single component, exclusive private file creation.
        let fd = unsafe {
            libc::openat(
                dir.file.as_raw_fd(),
                tmp.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        let mut file = owned(fd)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        if dir
            .read(&name, 67_108_864)?
            .as_deref()
            .map(Digest::of_bytes)
            .as_ref()
            != expected
        {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "DIVERGED: concurrent file change",
            ));
        }
        let flags = if expected.is_none() {
            libc::RENAME_NOREPLACE
        } else {
            0
        };
        // SAFETY: both names are validated components under a live directory descriptor.
        // RENAME_NOREPLACE prevents bootstrap/new-file replacement, including symlink races.
        let result = unsafe {
            libc::renameat2(
                dir.file.as_raw_fd(),
                tmp.as_ptr(),
                dir.file.as_raw_fd(),
                dest.as_ptr(),
                flags,
            )
        };
        if result < 0 {
            return Err(io::Error::last_os_error());
        }
        dir.file.sync_all()?;
        if dir.read(&name, bytes.len() as u64 + 1)?.as_deref() != Some(bytes) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "concurrent post-write change",
            ));
        }
        Ok(())
    }
    pub(crate) fn publish_child(&self, temporary: &str, destination: &str) -> io::Result<()> {
        let source = component(temporary)?;
        let destination = component(destination)?;
        // SAFETY: both validated names are relative to the same live granted directory.
        let result = unsafe {
            libc::renameat2(
                self.file.as_raw_fd(),
                source.as_ptr(),
                self.file.as_raw_fd(),
                destination.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if result < 0 {
            return Err(io::Error::last_os_error());
        }
        self.file.sync_all()
    }
}

impl Directory {
    pub(crate) fn lock(&self, name: &str) -> io::Result<File> {
        let name = component(name)?;
        // SAFETY: a validated component under an already granted directory descriptor.
        let raw = unsafe {
            libc::openat(
                self.file.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDWR
                    | libc::O_CREAT
                    | libc::O_NOFOLLOW
                    | libc::O_CLOEXEC
                    | libc::O_NONBLOCK,
                0o600,
            )
        };
        let file = owned(raw)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.nlink() != 1 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "invalid writer lock",
            ));
        }
        // SAFETY: flock receives a live file descriptor and valid lock flags.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } < 0 {
            return Err(io::Error::last_os_error());
        }
        // Closing this unique descriptor releases the cooperative writer lock.
        Ok(file)
    }
}
