//! Linux openat2 confinement. No canonicalize-then-open race for relative operations.
//! Kernel < 5.6 / blocked openat2 fails closed. No fallback to unsafe string paths.
use semwright_types::{Error,ErrorCode,Result};
use std::ffi::{CString,OsStr};
use std::fs::File;
use std::io::{Read,Write};
use std::os::fd::{AsRawFd,FromRawFd,OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Component,Path};

const RESOLVE_NO_XDEV:u64=0x01;
const RESOLVE_NO_MAGICLINKS:u64=0x02;
const RESOLVE_NO_SYMLINKS:u64=0x04;
const RESOLVE_BENEATH:u64=0x08;
#[repr(C)]struct OpenHow{flags:u64,mode:u64,resolve:u64}
fn cstring(s:&OsStr)->Result<CString>{CString::new(s.as_bytes()).map_err(|_|Error::invalid("NUL in filesystem path"))}
pub fn validate_relative_path(path:&Path)->Result<()>{
    if path.as_os_str().is_empty() || path.as_os_str().as_bytes().len()>4096 || path.is_absolute(){return Err(Error::invalid("Path must be a nonempty relative path"));}
    for component in path.components(){if !matches!(component,Component::Normal(_)){return Err(Error::new(ErrorCode::PolicyDenied,"Path traversal is not permitted"));}}
    // Path::components normalizes interior '.', so reject those lexically too.
    if path.as_os_str().as_bytes().split(|b|*b==b'/').any(|c|c.is_empty()||c==b"."||c==b".."){return Err(Error::invalid("Empty, dot and parent path components are forbidden"));}
    cstring(path.as_os_str())?;Ok(())
}
fn open_beneath(dir:&OwnedFd,path:&Path,flags:i32,mode:u32)->Result<OwnedFd>{
    validate_relative_path(path)?;let name=cstring(path.as_os_str())?;
    let how=OpenHow{flags:(flags|libc::O_CLOEXEC)as u64,mode:mode as u64,
        resolve:RESOLVE_BENEATH|RESOLVE_NO_SYMLINKS|RESOLVE_NO_MAGICLINKS|RESOLVE_NO_XDEV};
    // SAFETY: dir is a live fd; C string and repr(C) OpenHow remain alive for this
    // synchronous syscall. The kernel only reads exactly size_of::<OpenHow>() bytes.
    let fd=unsafe{libc::syscall(libc::SYS_openat2,dir.as_raw_fd(),name.as_ptr(),&how,std::mem::size_of::<OpenHow>())};
    if fd<0 {
        let e=std::io::Error::last_os_error();
        let code=match e.raw_os_error(){Some(libc::ENOSYS|libc::EPERM)=>ErrorCode::Unavailable,Some(libc::ENOENT)=>ErrorCode::NotFound,_=>ErrorCode::PolicyDenied};
        return Err(Error::new(code,"Confined open failed; symlinks, mount crossings and unsupported kernels fail closed"));
    }
    // SAFETY: successful openat2 returns a new fd owned uniquely by this function.
    Ok(unsafe{OwnedFd::from_raw_fd(fd as i32)})
}
pub struct Root{fd:OwnedFd,pub readable:bool,pub writable:bool}
impl Root{
    pub fn open(path:&Path,readable:bool,writable:bool)->Result<Self>{
        if !path.is_absolute() || path==Path::new("/"){return Err(Error::invalid("An explicit absolute workspace root is required"));}
        let canonical=std::fs::canonicalize(path)?;
        if canonical!=path{return Err(Error::new(ErrorCode::PolicyDenied,"Configured roots must be canonical, without symlinks"));}
        let name=cstring(path.as_os_str())?;
        // SAFETY: name is a valid NUL-terminated C string. O_NOFOLLOW rejects a
        // replacement final symlink; the opened directory fd pins the actual root.
        let fd=unsafe{libc::open(name.as_ptr(),libc::O_PATH|libc::O_DIRECTORY|libc::O_NOFOLLOW|libc::O_CLOEXEC)};
        if fd<0{return Err(std::io::Error::last_os_error().into());}
        // SAFETY: open returned a new owned descriptor.
        Ok(Self{fd:unsafe{OwnedFd::from_raw_fd(fd)},readable,writable})
    }
    pub fn read(&self,path:&Path,limit:usize)->Result<Vec<u8>>{
        if !self.readable{return Err(Error::new(ErrorCode::PolicyDenied,"Root is not readable"));}
        if limit==0 || limit>1_048_576{return Err(Error::invalid("Read limit must be 1..1048576"));}
        let fd=open_beneath(&self.fd,path,libc::O_RDONLY|libc::O_NONBLOCK,0)?;
        let file=File::from(fd);let metadata=file.metadata()?;
        if !metadata.is_file() || metadata.nlink()!=1{return Err(Error::new(ErrorCode::PolicyDenied,"Only regular single-link files may be read"));}
        if metadata.len()>limit as u64{return Err(Error::new(ErrorCode::ResourceExhausted,"File exceeds read budget"));}
        let mut bytes=Vec::new();file.take(limit as u64+1).read_to_end(&mut bytes)?;
        if bytes.len()>limit{return Err(Error::new(ErrorCode::ResourceExhausted,"File grew beyond read budget"));}Ok(bytes)
    }
    pub fn write_atomic(&self,path:&Path,bytes:&[u8])->Result<()>{
        if !self.writable{return Err(Error::new(ErrorCode::PolicyDenied,"Root is not writable"));}
        if bytes.len()>1_048_576{return Err(Error::new(ErrorCode::ResourceExhausted,"Write exceeds 1 MiB budget"));}
        validate_relative_path(path)?;
        let parent=path.parent().filter(|p|!p.as_os_str().is_empty());
        let parent_fd=match parent{Some(p)=>open_beneath(&self.fd,p,libc::O_RDONLY|libc::O_DIRECTORY,0)?,None=>{
            // Opening '.' is internal and not derived from the caller. Obtain an
            // fsync-capable descriptor from the pinned O_PATH root.
            let name=CString::new(".").map_err(|_|Error::invalid("Invalid constant path"))?;
            // SAFETY: root fd is live and name is a constant valid C string.
            let fd=unsafe{libc::openat(self.fd.as_raw_fd(),name.as_ptr(),libc::O_RDONLY|libc::O_DIRECTORY|libc::O_CLOEXEC)};
            if fd<0{return Err(std::io::Error::last_os_error().into());}
            // SAFETY: openat returned a new uniquely owned descriptor.
            unsafe{OwnedFd::from_raw_fd(fd)}
        }};
        let basename=path.file_name().ok_or_else(||Error::invalid("File basename missing"))?;
        let name=cstring(basename)?;
        // Existing symlinks/hardlinks are rejected, not followed. Rename remains
        // relative to this pinned directory even if ancestors are moved later.
        match open_beneath(&parent_fd,Path::new(basename),libc::O_RDONLY|libc::O_NONBLOCK,0){
            Ok(fd)=>{let m=File::from(fd).metadata()?;if !m.is_file()||m.nlink()!=1{return Err(Error::new(ErrorCode::PolicyDenied,"Unsafe replacement target"));}},
            Err(e) if e.code==ErrorCode::NotFound=>(),Err(e)=>return Err(e),
        }
        let temporary=CString::new(format!(".semwright-{}",uuid::Uuid::new_v4().simple())).map_err(|_|Error::invalid("Invalid temporary name"))?;
        let temp_path=Path::new(OsStr::from_bytes(temporary.as_bytes()));
        let fd=open_beneath(&parent_fd,temp_path,libc::O_WRONLY|libc::O_CREAT|libc::O_EXCL,0o600)?;
        let result=(||->Result<()>{
            let mut file=File::from(fd);file.write_all(bytes)?;file.sync_all()?;
            // SAFETY: all fds are live, both names are valid C strings. renameat
            // changes directory entries only and does not follow target symlinks.
            let rc=unsafe{libc::renameat(parent_fd.as_raw_fd(),temporary.as_ptr(),parent_fd.as_raw_fd(),name.as_ptr())};
            if rc!=0{return Err(std::io::Error::last_os_error().into());}
            // SAFETY: parent_fd refers to an open directory suitable for fsync.
            if unsafe{libc::fsync(parent_fd.as_raw_fd())}!=0{return Err(std::io::Error::last_os_error().into());}
            Ok(())
        })();
        if result.is_err(){
            // SAFETY: delete only our unpredictable temporary basename relative
            // to the same pinned directory. Failure is harmless if rename succeeded.
            unsafe{libc::unlinkat(parent_fd.as_raw_fd(),temporary.as_ptr(),0)};
        }
        result
    }
}
#[cfg(test)]mod tests{
    use super::*;use proptest::prelude::*;
    #[test]fn traversal_rejected(){for p in ["../secret","a/../b","/etc/passwd","a/./b","a//b",""]{assert!(validate_relative_path(Path::new(p)).is_err(),"{p}");}}
    #[test]fn atomic_scoped_roundtrip(){let dir=tempfile::tempdir().unwrap();let r=Root::open(dir.path(),true,true).unwrap();r.write_atomic(Path::new("sample"),b"hello").unwrap();assert_eq!(r.read(Path::new("sample"),10).unwrap(),b"hello");}
    #[test]fn symlink_escape_fails(){let dir=tempfile::tempdir().unwrap();std::os::unix::fs::symlink("/etc/passwd",dir.path().join("escape")).unwrap();let r=Root::open(dir.path(),true,true).unwrap();assert!(r.read(Path::new("escape"),10000).is_err());assert!(r.write_atomic(Path::new("escape"),b"x").is_err());}
    #[test]fn hardlinks_are_not_a_read_capability(){let a=tempfile::tempdir().unwrap();let b=tempfile::tempdir().unwrap();std::fs::write(a.path().join("secret"),"x").unwrap();std::fs::hard_link(a.path().join("secret"),b.path().join("link")).unwrap();let r=Root::open(b.path(),true,false).unwrap();assert!(r.read(Path::new("link"),10).is_err());}
    proptest!{#[test]fn parent_prefix_never_passes(s in "[a-z]{1,100}"){prop_assert!(validate_relative_path(Path::new(&format!("../{s}"))).is_err());}}
}
