use semwright_platform_api::filesystem::{Confinement, ScopedRoot, validate_relative_path};
use semwright_types::{Error, ErrorCode, Result};
use std::{
    ffi::CString,
    fs::File,
    io::Read,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    os::unix::ffi::OsStrExt,
    path::Path,
};
unsafe extern "C" {
    fn sw_root_open(path: *const libc::c_char) -> libc::c_int;
    fn sw_child_read_open(
        root: libc::c_int,
        child: *const libc::c_char,
        limit: usize,
    ) -> libc::c_int;
    fn sw_child_write_atomic(
        root: libc::c_int,
        child: *const libc::c_char,
        temporary: *const libc::c_char,
        data: *const u8,
        len: usize,
    ) -> libc::c_int;
}
fn result(code: i32) -> Result<i32> {
    if code >= 0 {
        return Ok(code);
    }
    let (c, m) = match code {
        -2 => (ErrorCode::InvalidArgument, "Invalid path or budget"),
        -3 => (ErrorCode::NotFound, "Child does not exist"),
        -4 => (ErrorCode::ResourceExhausted, "Filesystem budget exceeded"),
        -5 => (ErrorCode::BackendFailed, "Confined I/O failed"),
        -6 => (
            ErrorCode::Unsupported,
            "Nested paths require stronger confinement; grant the immediate directory",
        ),
        -7 => (ErrorCode::BackendFailed, "Commit durability is uncertain"),
        _ => (ErrorCode::PolicyDenied, "Confined filesystem access denied"),
    };
    let e = Error::new(c, m);
    Err(if code == -7 { e.uncertain() } else { e })
}
pub struct Root {
    fd: OwnedFd,
    pub readable: bool,
    pub writable: bool,
}
impl Root {
    pub fn open(path: &Path, readable: bool, writable: bool) -> Result<Self> {
        let s =
            CString::new(path.as_os_str().as_bytes()).map_err(|_| Error::invalid("NUL in root"))?;
        // SAFETY: the native function borrows a live NUL-terminated string, returns
        // a fresh unique fd or a negative stable error. It does not retain pointers.
        let fd = result(unsafe { sw_root_open(s.as_ptr()) })?;
        Ok(Self {
            // SAFETY: successful sw_root_open returns a newly owned descriptor
            // that has not been wrapped by any other Rust owner.
            fd: unsafe { OwnedFd::from_raw_fd(fd) },
            readable,
            writable,
        })
    }
    pub fn read(&self, path: &Path, limit: usize) -> Result<Vec<u8>> {
        if !self.readable {
            return Err(Error::new(ErrorCode::PolicyDenied, "Root is not readable"));
        }
        validate_relative_path(path)?;
        let s =
            CString::new(path.as_os_str().as_bytes()).map_err(|_| Error::invalid("NUL in path"))?;
        // SAFETY: live owned fd and C string. Native checks regular/single-link/same-device.
        let fd = result(unsafe { sw_child_read_open(self.fd.as_raw_fd(), s.as_ptr(), limit) })?;
        // SAFETY: successful call returns a new descriptor that this File uniquely owns.
        let file = unsafe { File::from_raw_fd(fd) };
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
    pub fn write_atomic(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        if !self.writable {
            return Err(Error::new(ErrorCode::PolicyDenied, "Root is not writable"));
        }
        validate_relative_path(path)?;
        let s =
            CString::new(path.as_os_str().as_bytes()).map_err(|_| Error::invalid("NUL in path"))?;
        let temp = CString::new(format!(".semwright-{}", uuid::Uuid::new_v4().simple()))
            .map_err(|_| Error::invalid("Invalid temporary name"))?;
        // SAFETY: live root fd and borrowed slices; C consumes all bytes synchronously,
        // never retains pointers and performs rename relative to this same pinned root.
        result(unsafe {
            sw_child_write_atomic(
                self.fd.as_raw_fd(),
                s.as_ptr(),
                temp.as_ptr(),
                bytes.as_ptr(),
                bytes.len(),
            )
        })?;
        Ok(())
    }
}
impl ScopedRoot for Root {
    fn confinement(&self) -> Confinement {
        Confinement::PinnedRootSingleChild
    }
    fn read(&self, path: &Path, limit: usize) -> Result<Vec<u8>> {
        Root::read(self, path, limit)
    }
    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        Root::write_atomic(self, path, bytes)
    }
}

/// Factory passed through the platform contract; callers never inspect native fds.
pub struct MacFilesystem;
impl semwright_platform_api::filesystem::ScopedFilesystem for MacFilesystem {
    fn open_root(
        &self,
        path: &Path,
        read: bool,
        write: bool,
    ) -> Result<Box<dyn semwright_platform_api::filesystem::ScopedRoot>> {
        Ok(Box::new(Root::open(path, read, write)?))
    }
}
