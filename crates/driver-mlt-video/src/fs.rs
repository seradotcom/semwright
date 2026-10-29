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
    // Exact canonical mount path received at Root::open, never inferred from a
    // descriptor or exposed as a general Root-to-path conversion. Native argv
    // for a closed owned child must still resolve to that child's pinned FD.
    materialized_path: PathBuf,
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
            materialized_path: path.to_path_buf(),
            readable,
            writable,
        })
    }
    /// Create only the two closed, operation-owned directories beneath this
    /// already granted output Root. Existing names are never reopened/reused.
    /// Both are retained from creation, including preflight/staging failures.
    #[doc(hidden)]
    pub fn create_original_workspaces(
        &self,
        output_path: &str,
    ) -> Result<(PrivateDir, PrivateDir)> {
        if !self.readable || !self.writable {
            return Err(Error::new(
                "PermissionDenied",
                "Owned originals require the existing read/write output Root",
            ));
        }
        validate_relative(output_path)?;
        self.instance_identity()?;
        let inputs = format!("{output_path}.raw-original-inputs");
        let work = format!("{output_path}.raw-original-work");
        let receipt = format!("{output_path}.raw-original-workspaces.json");
        for relative in [&inputs, &work, &receipt] {
            validate_relative(relative)?;
            if relative
                .rsplit('/')
                .next()
                .is_none_or(|name| name.len() > 255)
            {
                return Err(Error::limit(
                    "Owned original basename exceeds the filesystem component bound",
                ));
            }
            // O_PATH does not read a FIFO/device. Any existing entry, including
            // a symlink or directory, denies this original attempt before mkdir.
            match beneath(&self.dir, relative, libc::O_PATH | O_NOFOLLOW, 0) {
                Ok(_) => {
                    return Err(Error::new(
                        "Conflict",
                        "Owned original destination already exists; never retry or reuse",
                    ));
                }
                Err(error) if error.code == "NotFound" => {}
                Err(error) => return Err(error),
            }
        }
        let inputs = self.create_original_directory(&inputs)?;
        let work = self.create_original_directory(&work)?;
        Ok((inputs, work))
    }

    fn create_original_directory(&self, relative: &str) -> Result<PrivateDir> {
        self.instance_identity()?;
        let output_root = self.dir.try_clone()?;
        let (parent, name) = self.parent(relative)?;
        let name = cname(&name)?;
        // SAFETY: the parent is reached only by openat2 beneath the granted FD;
        // mkdirat is exclusive and does not follow an existing final symlink.
        if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
            let error = std::io::Error::last_os_error();
            return Err(if error.kind() == std::io::ErrorKind::AlreadyExists {
                Error::new("Conflict", "Owned original directory already exists")
            } else {
                error.into()
            });
        }
        // After successful mkdir no error path removes or reuses this name.
        // Even a failed open/readback/fsync leaves this declared location intact.
        let created = (|| -> Result<PrivateDir> {
            let dir = beneath(&self.dir, relative, O_DIRECTORY | O_NOFOLLOW, 0)?;
            let path = self.materialized_path.join(relative);
            verify_directory_binding(&dir, &path)?;
            let metadata = dir.metadata()?;
            if metadata.mode() & 0o777 != 0o700 {
                return Err(Error::new(
                    "PermissionDenied",
                    "Owned original directory is not private mode 0700",
                ));
            }
            self.instance_identity()?;
            dir.sync_all()?;
            parent.sync_all()?;
            Ok(PrivateDir {
                path,
                retain_originals: true,
                output_owned: Some(OutputOwnedDirectory {
                    dir,
                    output_root,
                    output_path: self.materialized_path.clone(),
                    relative: relative.to_owned(),
                }),
            })
        })();
        created.map_err(|mut error| {
            error.outcome_known = false;
            error
        })
    }

    /// An observation of the current, still-bound descriptor; not a grant or a
    /// substitute for the Host/Core capsule's own lifetime validation.
    #[doc(hidden)]
    pub fn instance_identity(&self) -> Result<(u64, u64)> {
        verify_directory_binding(&self.dir, &self.materialized_path)?;
        let metadata = self.dir.metadata()?;
        Ok((metadata.dev(), metadata.ino()))
    }

    /// Duplicate only this already-held descriptor and its unchanged rights.
    #[doc(hidden)]
    pub fn try_clone(&self) -> Result<Self> {
        self.instance_identity()?;
        Ok(Self {
            dir: self.dir.try_clone()?,
            materialized_path: self.materialized_path.clone(),
            readable: self.readable,
            writable: self.writable,
        })
    }

    /// Initial Host handoff only: reopen the exact two predeclared owned
    /// instances using the retained receipt identity from the original plan.
    /// This does not admit another operation or a retry after UNKNOWN.
    #[doc(hidden)]
    pub fn reopen_original_workspaces(
        &self,
        output_path: &str,
        expected_receipt_sha256: &str,
    ) -> Result<(PrivateDir, PrivateDir)> {
        if !self.readable || !self.writable || !crate::hash::valid_digest(expected_receipt_sha256) {
            return Err(Error::new(
                "PermissionDenied",
                "Owned original handoff requires the same read/write Root and receipt identity",
            ));
        }
        validate_relative(output_path)?;
        let identity = self.instance_identity()?;
        let bytes = self.read(
            &format!("{output_path}.raw-original-workspaces.json"),
            crate::color::REPORT_LIMIT,
        )?;
        if crate::hash::sha256(&bytes) != expected_receipt_sha256 {
            return Err(Error::new(
                "StaleReference",
                "Owned workspace receipt differs from the original declared plan",
            ));
        }
        let receipt: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|_| Error::invalid("Owned workspace receipt is not JSON"))?;
        let pair = |value: &serde_json::Value| -> Result<(u64, u64)> {
            Ok((
                value["device"]
                    .as_u64()
                    .ok_or_else(|| Error::invalid("Owned directory device is absent"))?,
                value["inode"]
                    .as_u64()
                    .ok_or_else(|| Error::invalid("Owned directory inode is absent"))?,
            ))
        };
        if receipt["schema"] != "semwright.native-video-owned-workspaces/1"
            || receipt["output_path"] != output_path
            || receipt["output_root"]["root"] != "output"
            || pair(&receipt["output_root"])? != identity
        {
            return Err(Error::new(
                "StaleReference",
                "Owned workspace handoff does not bind this output Root and operation",
            ));
        }
        let reopen = |role: &str, suffix: &str| -> Result<PrivateDir> {
            let relative = format!("{output_path}{suffix}");
            if receipt[role]["relative_path"] != relative {
                return Err(Error::new(
                    "StaleReference",
                    "Owned workspace role differs from its closed relative name",
                ));
            }
            let dir = beneath(&self.dir, &relative, O_DIRECTORY | O_NOFOLLOW, 0)?;
            let metadata = dir.metadata()?;
            if (metadata.dev(), metadata.ino()) != pair(&receipt[role])?
                || metadata.mode() & 0o777 != 0o700
            {
                return Err(Error::new(
                    "StaleReference",
                    "Owned workspace instance or private mode changed",
                ));
            }
            let path = self.materialized_path.join(&relative);
            verify_directory_binding(&dir, &path)?;
            Ok(PrivateDir {
                path,
                retain_originals: true,
                output_owned: Some(OutputOwnedDirectory {
                    dir,
                    output_root: self.dir.try_clone()?,
                    output_path: self.materialized_path.clone(),
                    relative,
                }),
            })
        };
        let inputs = reopen("inputs", ".raw-original-inputs")?;
        let work = reopen("work", ".raw-original-work")?;
        self.instance_identity()?;
        Ok((inputs, work))
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
    /// Read-only free-space preflight on the exact existing output parent.
    /// This is not a quota reservation; the existing Host scheduler owns concurrency.
    pub fn available_bytes_for(&self, path: &str) -> Result<u64> {
        let (parent, _) = self.parent(path)?;
        let mut raw = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        // SAFETY: parent pins an open directory; fstatvfs initializes the struct on success.
        if unsafe { libc::fstatvfs(parent.as_raw_fd(), raw.as_mut_ptr()) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        // SAFETY: the successful fstatvfs call above initialized every field of raw.
        let observed = unsafe { raw.assume_init() };
        let block = u128::from(observed.f_frsize);
        if block == 0 {
            return Err(Error::new(
                "Unavailable",
                "Filesystem block size unavailable",
            ));
        }
        u128::from(observed.f_bavail)
            .checked_mul(block)
            .and_then(|bytes| u64::try_from(bytes).ok())
            .ok_or_else(|| Error::limit("Filesystem available-byte overflow"))
    }

    pub fn write_new(&self, root_name: &str, path: &str, bytes: &[u8]) -> Result<Artifact> {
        self.publish(root_name, path, bytes, bytes.len() as u64)
    }
    /// fsync(temp), renameat2(RENAME_NOREPLACE), fsync(parent). No path-based overwrite race.
    pub fn publish(
        &self,
        root_name: &str,
        path: &str,
        reader: impl Read,
        limit: u64,
    ) -> Result<Artifact> {
        self.publish_inner(root_name, path, reader, limit, None)
    }
    /// Verify measured source identity before the same no-replace publication.
    /// A hash/size disagreement never publishes the final target.
    pub fn publish_verified(
        &self,
        root_name: &str,
        path: &str,
        reader: impl Read,
        limit: u64,
        expected_sha256: &str,
        expected_bytes: u64,
    ) -> Result<Artifact> {
        if !crate::hash::valid_digest(expected_sha256)
            || expected_bytes == 0
            || expected_bytes > limit
        {
            return Err(Error::invalid(
                "Verified publication requires bounded measured identity",
            ));
        }
        self.publish_inner(
            root_name,
            path,
            reader,
            limit,
            Some((expected_sha256, expected_bytes)),
        )
    }
    fn publish_inner(
        &self,
        root_name: &str,
        path: &str,
        mut reader: impl Read,
        limit: u64,
        expected: Option<(&str, u64)>,
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
            let digest = hash.finish();
            if expected
                .is_some_and(|(want_sha, want_bytes)| digest != want_sha || size != want_bytes)
            {
                return Err(Error::new(
                    "StaleReference",
                    "Source changed before no-replace publication",
                ));
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
                sha256: digest,
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
fn verify_directory_binding(dir: &File, path: &Path) -> Result<()> {
    let pinned = dir.metadata()?;
    let actual = std::fs::symlink_metadata(path)?;
    if !actual.is_dir()
        || actual.file_type().is_symlink()
        || std::fs::canonicalize(path)? != path
        || (actual.dev(), actual.ino()) != (pinned.dev(), pinned.ino())
    {
        return Err(Error::new(
            "StaleReference",
            "Owned directory path no longer names its original pinned instance",
        ));
    }
    Ok(())
}

#[derive(Debug)]
struct OutputOwnedDirectory {
    dir: File,
    output_root: File,
    output_path: PathBuf,
    relative: String,
}

/// Private workspace. Ordinary scratch remains disposable; the two closed
/// output-owned original locations remain retained for explicit owner review.
#[derive(Debug)]
pub struct PrivateDir {
    path: PathBuf,
    retain_originals: bool,
    output_owned: Option<OutputOwnedDirectory>,
}
impl PrivateDir {
    pub fn new(base: &Path) -> Result<Self> {
        let path = base.join(format!("semwright-video-{}", random_id()?));
        std::fs::DirBuilder::new().mode(0o700).create(&path)?;
        Ok(Self {
            path,
            retain_originals: false,
            output_owned: None,
        })
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Reuse the already-open child FD. This is not an ambient path reopen and
    /// cannot add permissions beyond the read/write output Root that created it.
    #[doc(hidden)]
    pub fn owned_root(&self, readable: bool, writable: bool) -> Result<Root> {
        match &self.output_owned {
            Some(owned) => {
                verify_directory_binding(&owned.output_root, &owned.output_path)?;
                verify_directory_binding(&owned.dir, &self.path)?;
                Ok(Root {
                    dir: owned.dir.try_clone()?,
                    materialized_path: self.path.clone(),
                    readable,
                    writable,
                })
            }
            None => Root::open(&self.path, readable, writable),
        }
    }

    /// Metadata only. A consumer must derive the relative names from its own
    /// admitted output_path and compare its existing Root, never grant by JSON.
    #[doc(hidden)]
    pub fn owned_output_identity(&self) -> Result<Option<serde_json::Value>> {
        let Some(owned) = &self.output_owned else {
            return Ok(None);
        };
        verify_directory_binding(&owned.output_root, &owned.output_path)?;
        verify_directory_binding(&owned.dir, &self.path)?;
        let root = owned.output_root.metadata()?;
        let child = owned.dir.metadata()?;
        Ok(Some(serde_json::json!({
            "relative_path":owned.relative,"device":child.dev(),"inode":child.ino(),
            "output_root":{"root":"output","device":root.dev(),"inode":root.ino()},
        })))
    }

    pub(crate) fn output_owned_for_originals(&self) -> bool {
        self.output_owned.is_some()
    }

    /// Only the original-custody finalizer may keep this exact owned workspace
    /// when its sole original bytes could not be durably copied. This does not
    /// disable cleanup for any other PrivateDir or expose a filesystem scan.
    pub(crate) fn retain_originals_on_drop(&mut self) {
        self.retain_originals = true;
    }
    pub fn create(&self, name: &str) -> Result<File> {
        validate_relative(name)?;
        if name.contains('/') {
            return Err(Error::invalid("Private scratch uses direct basenames"));
        }
        if let Some(owned) = &self.output_owned {
            self.owned_output_identity()?;
            return beneath(&owned.dir, name, O_WRONLY | O_CREAT | O_EXCL, 0o600);
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
        if let Some(owned) = &self.output_owned {
            self.owned_output_identity()?;
            if name.contains('/') {
                return Err(Error::invalid(
                    "Owned original scratch uses direct basenames",
                ));
            }
            let file = beneath(&owned.dir, name, O_NONBLOCK, 0)?;
            let metadata = file.metadata()?;
            if !metadata.is_file() || metadata.nlink() != 1 {
                return Err(Error::new(
                    "PermissionDenied",
                    "Only an owned regular single-link original can be sealed",
                ));
            }
            file.set_permissions(std::fs::Permissions::from_mode(0o400))?;
            return Ok(());
        }
        std::fs::set_permissions(self.path.join(name), std::fs::Permissions::from_mode(0o400))?;
        Ok(())
    }
}
impl Drop for PrivateDir {
    fn drop(&mut self) {
        if !self.retain_originals {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

#[cfg(test)]
mod output_owned_original_tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn output_owned_bytes_survive_only_their_local_wrappers_drop() {
        let output = tempfile::tempdir().unwrap();
        let path = std::fs::canonicalize(output.path()).unwrap();
        let root = Root::open(&path, true, true).unwrap();
        let (inputs, work) = root.create_original_workspaces("case.mkv").unwrap();
        let bound = work.owned_output_identity().unwrap().unwrap();
        assert_eq!(bound["relative_path"], "case.mkv.raw-original-work");
        assert_eq!(
            std::fs::metadata(work.path()).unwrap().mode() & 0o777,
            0o700
        );
        let mut writer = work.create("mezzanine.mkv").unwrap();
        writer.write_all(b"sole-partial-original").unwrap();
        writer.sync_all().unwrap();
        drop(writer);
        let input_root = inputs.owned_root(true, false).unwrap();
        assert!(!input_root.writable);
        drop(input_root);
        drop(inputs);
        drop(work);
        drop(root);
        let reopened = Root::open(&path, true, false).unwrap();
        assert_eq!(
            reopened
                .read("case.mkv.raw-original-work/mezzanine.mkv", 64)
                .unwrap(),
            b"sole-partial-original"
        );
        // This tests local ownership only; it does not simulate Host teardown.
    }

    #[test]
    fn owned_creation_rejects_prior_entries_and_foreign_paths_without_reuse() {
        let output = tempfile::tempdir().unwrap();
        let path = std::fs::canonicalize(output.path()).unwrap();
        let root = Root::open(&path, true, true).unwrap();
        for name in ["../outside.mkv", "/outside.mkv", "a\\b.mkv"] {
            assert!(root.create_original_workspaces(name).is_err());
        }
        let read_only = Root::open(&path, true, false).unwrap();
        assert!(
            read_only
                .create_original_workspaces("readonly.mkv")
                .is_err()
        );
        assert!(!path.join("readonly.mkv.raw-original-inputs").exists());
        std::fs::write(
            path.join("prior.mkv.raw-original-workspaces.json"),
            b"prior-unknown",
        )
        .unwrap();
        assert!(root.create_original_workspaces("prior.mkv").is_err());
        assert_eq!(
            std::fs::read(path.join("prior.mkv.raw-original-workspaces.json")).unwrap(),
            b"prior-unknown"
        );
        assert!(!path.join("prior.mkv.raw-original-inputs").exists());
        let foreign = tempfile::tempdir().unwrap();
        std::fs::write(foreign.path().join("original"), b"unrelated").unwrap();
        symlink(foreign.path(), path.join("linked.mkv.raw-original-work")).unwrap();
        assert!(root.create_original_workspaces("linked.mkv").is_err());
        assert!(!path.join("linked.mkv.raw-original-inputs").exists());
        assert_eq!(
            std::fs::read(foreign.path().join("original")).unwrap(),
            b"unrelated"
        );
        std::fs::create_dir(path.join("existing.mkv.raw-original-work")).unwrap();
        assert!(root.create_original_workspaces("existing.mkv").is_err());
        assert!(!path.join("existing.mkv.raw-original-inputs").exists());
    }

    #[test]
    fn replaced_output_path_cannot_redirect_owned_directory_creation() {
        let parent = tempfile::tempdir().unwrap();
        let original = parent.path().join("output");
        let retained = parent.path().join("retained-output");
        std::fs::create_dir(&original).unwrap();
        let original = std::fs::canonicalize(&original).unwrap();
        let root = Root::open(&original, true, true).unwrap();
        std::fs::rename(&original, &retained).unwrap();
        std::fs::create_dir(&original).unwrap();
        std::fs::write(original.join("foreign"), b"replacement-must-survive").unwrap();
        assert!(root.create_original_workspaces("case.mkv").is_err());
        assert!(!original.join("case.mkv.raw-original-inputs").exists());
        assert!(!retained.join("case.mkv.raw-original-inputs").exists());
        assert_eq!(
            std::fs::read(original.join("foreign")).unwrap(),
            b"replacement-must-survive"
        );
    }

    #[test]
    fn owned_staging_never_seals_a_foreign_symlink_or_hardlink() {
        let output = tempfile::tempdir().unwrap();
        let path = std::fs::canonicalize(output.path()).unwrap();
        let root = Root::open(&path, true, true).unwrap();
        let (inputs, _work) = root.create_original_workspaces("case.mkv").unwrap();
        let external = path.join("external-original.bin");
        std::fs::write(&external, b"foreign-original").unwrap();
        std::fs::set_permissions(&external, std::fs::Permissions::from_mode(0o600)).unwrap();
        symlink(&external, inputs.path().join("symlink.bin")).unwrap();
        assert!(inputs.create("symlink.bin").is_err());
        assert!(inputs.seal("symlink.bin").is_err());
        std::fs::hard_link(&external, inputs.path().join("hardlink.bin")).unwrap();
        assert!(inputs.seal("hardlink.bin").is_err());
        assert_eq!(std::fs::metadata(&external).unwrap().mode() & 0o777, 0o600);
        assert_eq!(std::fs::read(&external).unwrap(), b"foreign-original");
    }
}
