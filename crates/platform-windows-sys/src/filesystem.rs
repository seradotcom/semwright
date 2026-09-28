use semwright_platform_api::filesystem::{
    Confinement, MAX_SCOPED_BINARY_BYTES, ScopedFilesystem, ScopedRoot, validate_relative_path,
};
use semwright_types::{Error, ErrorCode, Result};
use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io::{Read, Write},
    mem::{offset_of, size_of},
    os::windows::{
        ffi::OsStrExt,
        fs::OpenOptionsExt,
        io::{AsRawHandle, FromRawHandle},
    },
    path::{Component, Path},
};
use windows::{
    Wdk::{
        Foundation::OBJECT_ATTRIBUTES,
        Storage::FileSystem::{
            FILE_CREATE as NT_FILE_CREATE, FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE,
            FILE_OPEN as NT_FILE_OPEN, FILE_OPEN_REPARSE_POINT as NT_FILE_OPEN_REPARSE_POINT,
            FILE_SYNCHRONOUS_IO_NONALERT, NTCREATEFILE_CREATE_DISPOSITION,
            NTCREATEFILE_CREATE_OPTIONS, NtCreateFile,
        },
    },
    Win32::{
        Foundation::{
            HANDLE, OBJ_CASE_INSENSITIVE, STATUS_ACCESS_DENIED, STATUS_OBJECT_NAME_COLLISION,
            STATUS_OBJECT_NAME_NOT_FOUND, STATUS_OBJECT_PATH_NOT_FOUND, UNICODE_STRING,
        },
        Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, DELETE, FILE_ACCESS_RIGHTS, FILE_ATTRIBUTE_DIRECTORY,
            FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT, FILE_DISPOSITION_INFO,
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ,
            FILE_GENERIC_WRITE, FILE_READ_ATTRIBUTES, FILE_RENAME_INFO, FILE_SHARE_DELETE,
            FILE_SHARE_READ, FILE_SHARE_WRITE, FileDispositionInfo, FileRenameInfo,
            GetFileInformationByHandle, SYNCHRONIZE, SetFileInformationByHandle,
        },
        System::IO::IO_STATUS_BLOCK,
    },
    core::PWSTR,
};

const MAX_COMPONENTS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileIdentity {
    volume: u32,
    index: u64,
}

fn handle(file: &File) -> HANDLE {
    HANDLE(file.as_raw_handle())
}

fn information(file: &File) -> Result<BY_HANDLE_FILE_INFORMATION> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `file` owns a live kernel handle and `info` is writable for the duration.
    unsafe { GetFileInformationByHandle(handle(file), &mut info) }.map_err(|_| {
        Error::new(
            ErrorCode::BackendFailed,
            "Windows file identity query failed",
        )
    })?;
    Ok(info)
}

fn identity(info: &BY_HANDLE_FILE_INFORMATION) -> FileIdentity {
    FileIdentity {
        volume: info.dwVolumeSerialNumber,
        index: ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64,
    }
}

fn safe_root_syntax(path: &Path) -> Result<()> {
    if !path.is_absolute() || path.file_name().is_none() {
        return Err(Error::invalid(
            "Windows scoped root must be an explicit directory below a volume root",
        ));
    }
    let s = path.as_os_str().to_string_lossy();
    let lower = s.to_ascii_lowercase();
    if lower.starts_with(r"\\") || lower.starts_with(r"\\?\") || lower.starts_with(r"\\.\") {
        return Err(Error::new(
            ErrorCode::PolicyDenied,
            "UNC, extended and device roots are not accepted by Windows confinement",
        ));
    }
    Ok(())
}

fn reserved_device(name: &str) -> bool {
    let stem = name
        .trim_end_matches(['.', ' '])
        .split('.')
        .next()
        .unwrap_or("");
    let upper = stem.to_ascii_uppercase();
    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$")
        || upper
            .strip_prefix("COM")
            .is_some_and(|n| n.parse::<u8>().is_ok_and(|n| (1..=9).contains(&n)))
        || upper
            .strip_prefix("LPT")
            .is_some_and(|n| n.parse::<u8>().is_ok_and(|n| (1..=9).contains(&n)))
}

fn validate_component(name: &OsStr) -> Result<()> {
    let wide = name.encode_wide().collect::<Vec<_>>();
    if wide.is_empty() || wide.len() > 255 || wide.contains(&0) {
        return Err(Error::invalid("Invalid Windows path component"));
    }
    let value = name.to_string_lossy();
    if value.contains(':') || value.ends_with(['.', ' ']) || reserved_device(&value) {
        return Err(Error::new(
            ErrorCode::PolicyDenied,
            "ADS, reserved device names and trailing dot/space names are forbidden",
        ));
    }
    Ok(())
}

fn validated_components(path: &Path) -> Result<Vec<OsString>> {
    validate_relative_path(path)?;
    let mut out = Vec::new();
    for component in path.components() {
        let Component::Normal(name) = component else {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Windows path traversal is forbidden",
            ));
        };
        validate_component(name)?;
        out.push(name.to_os_string());
        if out.len() > MAX_COMPONENTS {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Windows confined path exceeds component budget",
            ));
        }
    }
    if out.is_empty() {
        return Err(Error::invalid("Missing confined path"));
    }
    Ok(out)
}

fn nt_error(status: windows::Win32::Foundation::NTSTATUS, context: &'static str) -> Error {
    let code = if status == STATUS_OBJECT_NAME_NOT_FOUND || status == STATUS_OBJECT_PATH_NOT_FOUND {
        ErrorCode::NotFound
    } else if status == STATUS_OBJECT_NAME_COLLISION {
        ErrorCode::Conflict
    } else if status == STATUS_ACCESS_DENIED {
        ErrorCode::PolicyDenied
    } else {
        ErrorCode::PolicyDenied
    };
    Error::new(
        code,
        format!("{context} (NTSTATUS 0x{:08x})", status.0 as u32),
    )
}

fn open_no_reparse(path: &Path, directory: bool) -> Result<File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    options.share_mode((FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE).0);
    let mut flags = FILE_FLAG_OPEN_REPARSE_POINT.0;
    if directory {
        flags |= FILE_FLAG_BACKUP_SEMANTICS.0;
    }
    options.custom_flags(flags);
    let file = options.open(path)?;
    let info = information(&file)?;
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err(Error::new(
            ErrorCode::PolicyDenied,
            "Reparse points are forbidden",
        ));
    }
    Ok(file)
}

fn nt_open_relative(
    parent: &File,
    name: &OsStr,
    desired_access: FILE_ACCESS_RIGHTS,
    disposition: NTCREATEFILE_CREATE_DISPOSITION,
    options: NTCREATEFILE_CREATE_OPTIONS,
    context: &'static str,
) -> Result<File> {
    validate_component(name)?;
    let mut wide = name.encode_wide().collect::<Vec<_>>();
    let byte_len = wide
        .len()
        .checked_mul(2)
        .and_then(|value| u16::try_from(value).ok())
        .ok_or_else(|| Error::invalid("Windows path component exceeds Unicode budget"))?;
    let unicode = UNICODE_STRING {
        Length: byte_len,
        MaximumLength: byte_len,
        Buffer: PWSTR(wide.as_mut_ptr()),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: handle(parent),
        ObjectName: &unicode,
        Attributes: OBJ_CASE_INSENSITIVE,
        SecurityDescriptor: std::ptr::null(),
        SecurityQualityOfService: std::ptr::null(),
    };
    let mut raw = HANDLE::default();
    let mut io_status = IO_STATUS_BLOCK::default();
    // SAFETY: parent is a live directory HANDLE; the UTF-16 buffer, UNICODE_STRING,
    // OBJECT_ATTRIBUTES and IO_STATUS_BLOCK remain live for this synchronous call.
    let status = unsafe {
        NtCreateFile(
            &mut raw,
            desired_access,
            &attributes,
            &mut io_status,
            None,
            FILE_ATTRIBUTE_NORMAL,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            disposition,
            options | NT_FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
            None,
            0,
        )
    };
    if status.is_err() {
        return Err(nt_error(status, context));
    }
    if raw.is_invalid() {
        return Err(Error::new(
            ErrorCode::BackendFailed,
            "Windows relative open returned an invalid handle",
        ));
    }
    // SAFETY: successful NtCreateFile returned a fresh HANDLE owned by the caller.
    Ok(unsafe { File::from_raw_handle(raw.0) })
}

fn validate_directory(file: &File, root_volume: u32) -> Result<()> {
    let info = information(file)?;
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
        || info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 == 0
        || info.dwVolumeSerialNumber != root_volume
    {
        return Err(Error::new(
            ErrorCode::PolicyDenied,
            "Windows confined directory failed identity/reparse/volume checks",
        ));
    }
    Ok(())
}

fn validate_regular_file(
    file: &File,
    root_volume: u32,
    require_single_link: bool,
) -> Result<BY_HANDLE_FILE_INFORMATION> {
    let info = information(file)?;
    if info.dwFileAttributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY).0 != 0
        || info.dwVolumeSerialNumber != root_volume
        || (require_single_link && info.nNumberOfLinks != 1)
    {
        return Err(Error::new(
            ErrorCode::PolicyDenied,
            "Windows confined file failed type/link/volume checks",
        ));
    }
    Ok(info)
}

fn mark_delete_on_close(file: &File) {
    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: file is a live temp HANDLE and disposition is a correctly sized input buffer.
    let _ = unsafe {
        SetFileInformationByHandle(
            handle(file),
            FileDispositionInfo,
            (&disposition as *const FILE_DISPOSITION_INFO).cast(),
            size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    };
}

fn rename_relative(file: &File, parent: &File, name: &OsStr) -> Result<()> {
    validate_component(name)?;
    let wide = name.encode_wide().collect::<Vec<_>>();
    let name_bytes = wide
        .len()
        .checked_mul(2)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| Error::invalid("Windows rename target exceeds Unicode budget"))?;
    let header = offset_of!(FILE_RENAME_INFO, FileName);
    let payload = header
        .checked_add(wide.len().saturating_mul(2))
        .ok_or_else(|| {
            Error::new(
                ErrorCode::ResourceExhausted,
                "Windows rename buffer overflow",
            )
        })?;
    let total = payload.max(size_of::<FILE_RENAME_INFO>());
    let words = total.div_ceil(size_of::<usize>());
    let mut storage = vec![0usize; words];
    let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    // SAFETY: storage is pointer-aligned and large enough for the fixed prefix plus every UTF-16 unit.
    unsafe {
        (*info).Anonymous.ReplaceIfExists = true;
        (*info).RootDirectory = handle(parent);
        (*info).FileNameLength = name_bytes;
        std::ptr::copy_nonoverlapping(wide.as_ptr(), (*info).FileName.as_mut_ptr(), wide.len());
        SetFileInformationByHandle(
            handle(file),
            FileRenameInfo,
            storage.as_ptr().cast(),
            u32::try_from(total).map_err(|_| {
                Error::new(
                    ErrorCode::ResourceExhausted,
                    "Windows rename buffer too large",
                )
            })?,
        )
    }
    .map_err(|_| {
        Error::new(
            ErrorCode::BackendFailed,
            "Windows root-relative rename failed",
        )
    })
}

pub struct Root {
    root: File,
    root_identity: FileIdentity,
    readable: bool,
    writable: bool,
}

impl Root {
    pub fn open(path: &Path, readable: bool, writable: bool) -> Result<Self> {
        if !readable && !writable {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Windows scoped root must grant read or write authority",
            ));
        }
        safe_root_syntax(path)?;
        let root = open_no_reparse(path, true)?;
        let info = information(&root)?;
        if info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 == 0 {
            return Err(Error::invalid("Windows scoped root must be a directory"));
        }
        let root_identity = identity(&info);
        Ok(Self {
            root,
            root_identity,
            readable,
            writable,
        })
    }

    fn root_still_pinned(&self) -> Result<()> {
        if identity(&information(&self.root)?) != self.root_identity {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Scoped root identity changed",
            ));
        }
        Ok(())
    }

    fn open_parent(&self, path: &Path) -> Result<(File, OsString)> {
        let mut components = validated_components(path)?;
        let basename = components
            .pop()
            .ok_or_else(|| Error::invalid("Confined path basename missing"))?;
        let mut parent = self.root.try_clone()?;
        for component in components {
            self.root_still_pinned()?;
            let next = nt_open_relative(
                &parent,
                &component,
                FILE_GENERIC_READ | SYNCHRONIZE,
                NT_FILE_OPEN,
                FILE_DIRECTORY_FILE,
                "Windows root-relative directory open failed",
            )?;
            validate_directory(&next, self.root_identity.volume)?;
            parent = next;
        }
        self.root_still_pinned()?;
        Ok((parent, basename))
    }

    fn open_existing_file(
        &self,
        parent: &File,
        name: &OsStr,
        require_single_link: bool,
    ) -> Result<(File, BY_HANDLE_FILE_INFORMATION)> {
        let file = nt_open_relative(
            parent,
            name,
            FILE_GENERIC_READ | SYNCHRONIZE,
            NT_FILE_OPEN,
            FILE_NON_DIRECTORY_FILE,
            "Windows root-relative file open failed",
        )?;
        let info = validate_regular_file(&file, self.root_identity.volume, require_single_link)?;
        Ok((file, info))
    }

    fn inspect_existing_file(
        &self,
        parent: &File,
        name: &OsStr,
        require_single_link: bool,
    ) -> Result<(File, BY_HANDLE_FILE_INFORMATION)> {
        let file = nt_open_relative(
            parent,
            name,
            FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            NT_FILE_OPEN,
            FILE_NON_DIRECTORY_FILE,
            "Windows root-relative metadata open failed",
        )?;
        let info = validate_regular_file(&file, self.root_identity.volume, require_single_link)?;
        Ok((file, info))
    }

    fn read_inner(&self, path: &Path, limit: usize) -> Result<Vec<u8>> {
        if !self.readable {
            return Err(Error::new(ErrorCode::PolicyDenied, "Root is not readable"));
        }
        if limit == 0 || limit > MAX_SCOPED_BINARY_BYTES {
            return Err(Error::invalid("Read limit exceeds scoped binary budget"));
        }
        self.root_still_pinned()?;
        let (parent, basename) = self.open_parent(path)?;
        let (file, info) = self.open_existing_file(&parent, &basename, true)?;
        let size = ((info.nFileSizeHigh as u64) << 32) | info.nFileSizeLow as u64;
        if size > limit as u64 || size > MAX_SCOPED_BINARY_BYTES as u64 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Confined file exceeds read budget",
            ));
        }
        self.root_still_pinned()?;
        let mut bytes = Vec::with_capacity((size as usize).min(limit));
        file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > limit {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "File grew beyond read budget",
            ));
        }
        self.root_still_pinned()?;
        Ok(bytes)
    }

    fn write_inner(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        if !self.writable {
            return Err(Error::new(ErrorCode::PolicyDenied, "Root is not writable"));
        }
        if bytes.len() > MAX_SCOPED_BINARY_BYTES {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Write exceeds scoped binary budget",
            ));
        }
        self.root_still_pinned()?;
        let (parent, basename) = self.open_parent(path)?;

        match self.inspect_existing_file(&parent, &basename, true) {
            Ok(_) => {}
            Err(error) if error.code == ErrorCode::NotFound => {}
            Err(error) => return Err(error),
        }

        let temporary = OsString::from(format!(".semwright-{}.tmp", uuid::Uuid::new_v4().simple()));
        let mut temp = nt_open_relative(
            &parent,
            &temporary,
            FILE_GENERIC_WRITE | FILE_READ_ATTRIBUTES | DELETE | SYNCHRONIZE,
            NT_FILE_CREATE,
            FILE_NON_DIRECTORY_FILE,
            "Windows confined temporary file creation failed",
        )?;
        let temp_info = validate_regular_file(&temp, self.root_identity.volume, true)?;
        let temp_identity = identity(&temp_info);
        let mut renamed = false;
        let result = (|| -> Result<()> {
            temp.write_all(bytes)?;
            temp.sync_all()?;
            self.root_still_pinned()?;
            rename_relative(&temp, &parent, &basename)?;
            renamed = true;

            let (committed, committed_info) = self
                .inspect_existing_file(&parent, &basename, true)
                .map_err(|error| {
                    Error::new(
                        ErrorCode::BackendFailed,
                        format!(
                            "Windows committed file verification failed: {}",
                            error.message
                        ),
                    )
                    .uncertain()
                })?;
            if identity(&committed_info) != temp_identity {
                return Err(Error::new(
                    ErrorCode::BackendFailed,
                    "Windows atomic rename committed an unexpected file identity",
                )
                .uncertain());
            }
            drop(committed);
            self.root_still_pinned()?;
            Ok(())
        })();
        if result.is_err() && !renamed {
            mark_delete_on_close(&temp);
        }
        result
    }
}

impl ScopedRoot for Root {
    fn confinement(&self) -> Confinement {
        Confinement::WindowsHandleRelativeNoReparse
    }

    fn read(&self, path: &Path, limit: usize) -> Result<Vec<u8>> {
        self.read_inner(path, limit)
    }

    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        self.write_inner(path, bytes)
    }
}

pub struct WindowsFilesystem;
impl ScopedFilesystem for WindowsFilesystem {
    fn open_root(&self, path: &Path, read: bool, write: bool) -> Result<Box<dyn ScopedRoot>> {
        Ok(Box::new(Root::open(path, read, write)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn junction(link: &Path, target: &Path) {
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .status()
            .expect("run mklink /J");
        assert!(status.success(), "junction fixture creation must succeed");
    }

    #[test]
    fn rejects_windows_ambiguous_components() {
        for value in [
            "file:stream",
            "CON",
            "nested/LPT1.txt",
            "name.",
            "nested/name ",
            "../escape",
            "a/../b",
            "a/./b",
            "a//b",
        ] {
            assert!(validated_components(Path::new(value)).is_err(), "{value}");
        }
        let components = validated_components(Path::new(r"nested\report.txt")).unwrap();
        assert_eq!(
            components,
            [OsString::from("nested"), OsString::from("report.txt")]
        );
    }

    #[test]
    fn nested_read_and_atomic_write_roundtrip() {
        let root_dir = tempfile::tempdir().unwrap();
        let nested = root_dir.path().join("one").join("two");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("read.txt"), b"nested-data").unwrap();

        let root = Root::open(root_dir.path(), true, true).unwrap();
        assert_eq!(
            root.read(Path::new(r"one\two\read.txt"), 1024).unwrap(),
            b"nested-data"
        );
        root.write_atomic(Path::new(r"one\two\write.txt"), b"first")
            .unwrap();
        root.write_atomic(Path::new(r"one\two\write.txt"), b"second")
            .unwrap();
        assert_eq!(
            root.read(Path::new(r"one\two\write.txt"), 1024).unwrap(),
            b"second"
        );
        assert_eq!(std::fs::read(nested.join("write.txt")).unwrap(), b"second");
    }

    #[test]
    fn read_and_write_authority_are_independent() {
        let root_dir = tempfile::tempdir().unwrap();
        std::fs::write(root_dir.path().join("data.txt"), b"data").unwrap();

        let read_only = Root::open(root_dir.path(), true, false).unwrap();
        assert!(read_only.write_atomic(Path::new("new.txt"), b"x").is_err());

        let write_only = Root::open(root_dir.path(), false, true).unwrap();
        assert!(write_only.read(Path::new("data.txt"), 16).is_err());
        write_only
            .write_atomic(Path::new("new.txt"), b"written")
            .unwrap();
        assert_eq!(
            std::fs::read(root_dir.path().join("new.txt")).unwrap(),
            b"written"
        );
    }

    #[test]
    fn hardlinked_file_is_not_a_read_or_replacement_capability() {
        let root_dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let secret = outside.path().join("secret.txt");
        std::fs::write(&secret, b"secret").unwrap();
        let link = root_dir.path().join("link.txt");
        std::fs::hard_link(&secret, &link).unwrap();

        let root = Root::open(root_dir.path(), true, true).unwrap();
        assert!(root.read(Path::new("link.txt"), 1024).is_err());
        assert!(
            root.write_atomic(Path::new("link.txt"), b"replacement")
                .is_err()
        );
        assert_eq!(std::fs::read(&secret).unwrap(), b"secret");
    }

    #[test]
    fn junction_escape_is_rejected_for_nested_read_and_write() {
        let root_dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.txt"), b"secret").unwrap();
        let link = root_dir.path().join("escape");
        junction(&link, outside.path());

        let root = Root::open(root_dir.path(), true, true).unwrap();
        assert!(root.read(Path::new(r"escape\secret.txt"), 1024).is_err());
        assert!(
            root.write_atomic(Path::new(r"escape\created.txt"), b"no")
                .is_err()
        );
        assert!(!outside.path().join("created.txt").exists());
    }

    #[test]
    fn pinned_root_survives_pathname_rename() {
        let parent = tempfile::tempdir().unwrap();
        let original = parent.path().join("workspace");
        let renamed = parent.path().join("workspace-renamed");
        std::fs::create_dir(&original).unwrap();
        std::fs::create_dir(original.join("nested")).unwrap();
        std::fs::write(original.join("nested").join("data.txt"), b"stable").unwrap();

        let root = Root::open(&original, true, true).unwrap();
        std::fs::rename(&original, &renamed).unwrap();
        assert_eq!(
            root.read(Path::new(r"nested\data.txt"), 64).unwrap(),
            b"stable"
        );
        root.write_atomic(Path::new(r"nested\new.txt"), b"new")
            .unwrap();
        assert_eq!(
            std::fs::read(renamed.join("nested").join("new.txt")).unwrap(),
            b"new"
        );
    }
}
