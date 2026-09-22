//! Linux openat2 FD-relative grants. No symlink/canonicalize-then-open fallback.
//! Output publication is always no-replace. In-place editing of active GUI projects is not offered.
use crate::{
    Error, Result,
    hash::{Sha256, random_id},
};
use std::{
    ffi::CString,
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
};
#[cfg(not(target_os = "linux"))]
compile_error!("The driver currently requires Linux openat2 and process containment");
const O_CLOEXEC: i32 = libc::O_CLOEXEC;
const O_NOFOLLOW: i32 = libc::O_NOFOLLOW;
const O_NONBLOCK: i32 = libc::O_NONBLOCK;
const O_DIRECTORY: i32 = libc::O_DIRECTORY;
const O_CREAT: i32 = libc::O_CREAT;
const O_EXCL: i32 = libc::O_EXCL;
const O_WRONLY: i32 = libc::O_WRONLY;
const SYS_OPENAT2: isize = libc::SYS_openat2 as isize;
#[repr(C)]
struct OpenHow {
    flags: u64,
    mode: u64,
    resolve: u64,
}
unsafe extern "C" {
    fn syscall(number: isize, ...) -> isize;
    fn renameat2(
        olddir: i32,
        old: *const std::ffi::c_char,
        newdir: i32,
        new: *const std::ffi::c_char,
        flags: u32,
    ) -> i32;
    fn unlinkat(dir: i32, path: *const std::ffi::c_char, flags: i32) -> i32;
    fn fsync(fd: i32) -> i32;
}
pub fn validate_relative(path: &str) -> Result<()> {
    if path.is_empty()
        || path.len() > 4096
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains(':')
        || path.chars().any(char::is_control)
        || path
            .split('/')
            .any(|s| s.is_empty() || s == "." || s == "..")
    {
        return Err(Error::new(
            "PermissionDenied",
            "Path must be a bounded, control-free relative path without traversal or URI syntax",
        ));
    }
    Ok(())
}
fn cname(path: &str) -> Result<CString> {
    CString::new(path).map_err(|_| Error::invalid("NUL in path"))
}
fn beneath(dir: &File, path: &str, flags: i32, mode: u32) -> Result<File> {
    validate_relative(path)?;
    let name = cname(path)?;
    let how = OpenHow {
        flags: (flags | O_CLOEXEC) as u64,
        mode: u64::from(mode),
        resolve: 0x0f,
    };
    // SAFETY: live directory FD, NUL-terminated path and correctly sized repr(C) arguments.
    let fd = unsafe {
        syscall(
            SYS_OPENAT2,
            dir.as_raw_fd(),
            name.as_ptr(),
            &how,
            std::mem::size_of::<OpenHow>(),
        )
    };
    if fd < 0 {
        let e = std::io::Error::last_os_error();
        return Err(Error::new(
            if e.kind() == std::io::ErrorKind::NotFound {
                "NotFound"
            } else {
                "PermissionDenied"
            },
            "Confined open denied; unsupported openat2 also fails closed",
        ));
    }
    // SAFETY: a successful openat2 returns a new, uniquely owned file descriptor.
    Ok(unsafe { File::from_raw_fd(fd as i32) })
}
#[derive(Debug)]
pub struct Root {
    dir: File,
    pub readable: bool,
    pub writable: bool,
}
#[derive(Clone, Debug)]
pub struct Artifact {
    pub root: String,
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
}
impl Artifact {
    pub fn json(&self) -> crate::json::Value {
        crate::json::obj([
            ("root", self.root.clone().into()),
            ("path", self.path.clone().into()),
            ("sha256", self.sha256.clone().into()),
            ("bytes", self.bytes.into()),
        ])
    }
}
impl Root {
    pub fn open(path: &Path, readable: bool, writable: bool) -> Result<Self> {
        if !path.is_absolute() || path == Path::new("/") || std::fs::canonicalize(path)? != path {
            return Err(Error::new(
                "PermissionDenied",
                "Owner grant must be an explicit canonical directory",
            ));
        }
        let dir = OpenOptions::new()
            .read(true)
            .custom_flags(O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
            .open(path)?;
        if !dir.metadata()?.is_dir() {
            return Err(Error::invalid("Grant is not a directory"));
        }
        Ok(Self {
            dir,
            readable,
            writable,
        })
    }
    pub fn read_file(&self, path: &str, limit: u64) -> Result<File> {
        if !self.readable {
            return Err(Error::new("PermissionDenied", "Root is not readable"));
        }
        let file = beneath(&self.dir, path, O_NONBLOCK, 0)?;
        let m = file.metadata()?;
        if !m.is_file() || m.nlink() != 1 {
            return Err(Error::new(
                "PermissionDenied",
                "Only regular single-link files are readable",
            ));
        }
        if m.len() > limit {
            return Err(Error::limit("File exceeds read budget"));
        }
        Ok(file)
    }
    pub fn read(&self, path: &str, limit: usize) -> Result<Vec<u8>> {
        let file = self.read_file(path, limit as u64)?;
        let mut data = vec![];
        file.take(limit as u64 + 1).read_to_end(&mut data)?;
        if data.len() > limit {
            return Err(Error::limit("File grew past limit"));
        }
        Ok(data)
    }
    pub fn digest(&self, path: &str, limit: u64) -> Result<String> {
        crate::hash::reader_hash(self.read_file(path, limit)?, limit).map(|v| v.0)
    }
    pub fn exists(&self, path: &str) -> Result<bool> {
        match self.read_file(path, u64::MAX) {
            Ok(_) => Ok(true),
            Err(e) if e.code == "NotFound" => Ok(false),
            Err(e) => Err(e),
        }
    }
    fn parent(&self, path: &str) -> Result<(File, String)> {
        validate_relative(path)?;
        if let Some((parent, name)) = path.rsplit_once('/') {
            Ok((beneath(&self.dir, parent, O_DIRECTORY, 0)?, name.into()))
        } else {
            Ok((self.dir.try_clone()?, path.into()))
        }
    }
    pub fn write_new(&self, root_name: &str, path: &str, bytes: &[u8]) -> Result<Artifact> {
        self.publish(root_name, path, bytes, bytes.len() as u64)
    }
    /// fsync(temp), renameat2(RENAME_NOREPLACE), fsync(parent). No path-based overwrite race.
    pub fn publish(
        &self,
        root_name: &str,
        path: &str,
        mut reader: impl Read,
        limit: u64,
    ) -> Result<Artifact> {
        if !self.writable {
            return Err(Error::new("PermissionDenied", "Root is not writable"));
        }
        let (parent, name) = self.parent(path)?;
        let temporary = format!(".semwright-{}", random_id()?);
        let temp_c = cname(&temporary)?;
        let name_c = cname(&name)?;
        let mut file = beneath(&parent, &temporary, O_WRONLY | O_CREAT | O_EXCL, 0o600)?;
        let result = (|| -> Result<Artifact> {
            let mut hash = Sha256::new();
            let mut size = 0u64;
            let mut buf = [0; 65536];
            loop {
                let n = reader.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                size = size
                    .checked_add(n as u64)
                    .ok_or_else(|| Error::limit("Output size overflow"))?;
                if size > limit {
                    return Err(Error::limit("Output size exceeds budget"));
                }
                file.write_all(&buf[..n])?;
                hash.update(&buf[..n]);
            }
            file.sync_all()?;
            // SAFETY: both names are live C strings, FDs pin the same parent; NOREPLACE never follows or overwrites a target.
            if unsafe {
                renameat2(
                    parent.as_raw_fd(),
                    temp_c.as_ptr(),
                    parent.as_raw_fd(),
                    name_c.as_ptr(),
                    1,
                )
            } != 0
            {
                return Err(Error::new(
                    "Conflict",
                    "Output exists or atomic no-replace rename failed",
                ));
            }
            // SAFETY: parent is an open directory FD suitable for fsync.
            if unsafe { fsync(parent.as_raw_fd()) } != 0 {
                let mut error = Error::new(
                    "BackendFailed",
                    "Output was published, but directory durability is uncertain",
                );
                error.outcome_known = false;
                return Err(error);
            }
            Ok(Artifact {
                root: root_name.into(),
                path: path.into(),
                sha256: hash.finish(),
                bytes: size,
            })
        })();
        if result.is_err() {
            // SAFETY: unlink only our random basename relative to the pinned parent; target output is never removed.
            unsafe { unlinkat(parent.as_raw_fd(), temp_c.as_ptr(), 0) };
        }
        result
    }
}
/// Private disposable workspace. Used only for driver-owned synthetic/staged data, not user config.
#[derive(Debug)]
pub struct PrivateDir {
    path: PathBuf,
}
impl PrivateDir {
    pub fn new(base: &Path) -> Result<Self> {
        let path = base.join(format!("semwright-video-{}", random_id()?));
        std::fs::DirBuilder::new().mode(0o700).create(&path)?;
        Ok(Self { path })
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn create(&self, name: &str) -> Result<File> {
        validate_relative(name)?;
        if name.contains('/') {
            return Err(Error::invalid("Private scratch uses direct basenames"));
        }
        Ok(OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(O_NOFOLLOW | O_CLOEXEC)
            .open(self.path.join(name))?)
    }
    pub fn seal(&self, name: &str) -> Result<()> {
        validate_relative(name)?;
        std::fs::set_permissions(self.path.join(name), std::fs::Permissions::from_mode(0o400))?;
        Ok(())
    }
}
impl Drop for PrivateDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
