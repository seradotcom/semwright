use semwright_platform_api::filesystem::{
    Confinement, MAX_SCOPED_BINARY_BYTES, RetainedFileStage, RetainedStageError,
    RetainedStageResult, RetainedStageState, ScopedFileObservation, ScopedFilesystem, ScopedRoot,
    validate_relative_path,
};
use semwright_types::{Error, ErrorCode, Result};
use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    mem::{offset_of, size_of},
    os::windows::{
        ffi::OsStrExt,
        fs::OpenOptionsExt,
        io::{AsRawHandle, FromRawHandle},
    },
    path::{Component, Path, PathBuf, Prefix},
    sync::Mutex,
};
use windows::{
    Wdk::{
        Foundation::OBJECT_ATTRIBUTES,
        Storage::FileSystem::{
            FILE_CREATE as NT_FILE_CREATE, FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE,
            FILE_OPEN as NT_FILE_OPEN, FILE_OPEN_REPARSE_POINT as NT_FILE_OPEN_REPARSE_POINT,
            FILE_RENAME_INFORMATION, FILE_SYNCHRONOUS_IO_NONALERT, FileRenameInformation,
            NTCREATEFILE_CREATE_DISPOSITION, NTCREATEFILE_CREATE_OPTIONS, NtCreateFile,
            NtSetInformationFile,
        },
    },
    Win32::{
        Foundation::{
            HANDLE, OBJ_CASE_INSENSITIVE, STATUS_OBJECT_NAME_COLLISION,
            STATUS_OBJECT_NAME_NOT_FOUND, STATUS_OBJECT_PATH_NOT_FOUND, UNICODE_STRING,
        },
        Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, DELETE, FILE_ACCESS_RIGHTS, FILE_ADD_FILE,
            FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT,
            FILE_DISPOSITION_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE,
            FILE_SHARE_MODE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TRAVERSE, FileDispositionInfo,
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
    created_100ns: u64,
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
        created_100ns: filetime_ticks(info.ftCreationTime),
    }
}

fn filetime_ticks(time: windows::Win32::Foundation::FILETIME) -> u64 {
    ((time.dwHighDateTime as u64) << 32) | time.dwLowDateTime as u64
}

fn file_size(info: &BY_HANDLE_FILE_INFORMATION) -> u64 {
    ((info.nFileSizeHigh as u64) << 32) | info.nFileSizeLow as u64
}

fn native_identity(kind: &str, value: FileIdentity) -> String {
    format!(
        "windows-{kind}:{:08x}:{:016x}:{:016x}",
        value.volume, value.index, value.created_100ns
    )
}

/// Use the same retained HANDLE identity for local roots and Host mount capsules.
/// The encoding preserves the existing volume/index/creation-FILETIME contract.
pub(crate) fn retained_directory_identity(root: &File) -> Result<String> {
    let observed = information(root)?;
    if observed.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 == 0
        || observed.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
    {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "A retained non-reparse directory instance is required",
        ));
    }
    Ok(native_identity("directory", identity(&observed)))
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
    } else {
        ErrorCode::PolicyDenied
    };
    Error::new(
        code,
        format!("{context} (NTSTATUS 0x{:08x})", status.0 as u32),
    )
}

fn open_no_reparse(path: &Path, directory: bool, writable_directory: bool) -> Result<File> {
    let mut options = std::fs::OpenOptions::new();
    let access = if directory {
        let mut access = (FILE_READ_ATTRIBUTES | FILE_TRAVERSE | SYNCHRONIZE).0;
        if writable_directory {
            access |= FILE_ADD_FILE.0;
        }
        access
    } else {
        FILE_GENERIC_READ.0
    };
    // access_mode overrides the generic OpenOptions read/write flags so directory handles
    // receive only traversal/metadata authority plus FILE_ADD_FILE when owner policy is writable.
    options.access_mode(access);
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
    share_access: FILE_SHARE_MODE,
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
            share_access,
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

/// Open a drive anchor, then each component relative to the previously retained
/// directory. Opening the final absolute pathname would follow ancestor reparses.
fn open_pinned_root(path: &Path, writable: bool) -> Result<(Vec<File>, File, FileIdentity)> {
    safe_root_syntax(path)?;
    let mut parts = path.components();
    let Some(Component::Prefix(prefix)) = parts.next() else {
        return Err(Error::invalid("Windows scoped root has no drive anchor"));
    };
    let Prefix::Disk(drive) = prefix.kind() else {
        return Err(Error::new(
            ErrorCode::PolicyDenied,
            "Only drive-absolute roots are supported",
        ));
    };
    if !matches!(parts.next(), Some(Component::RootDir)) {
        return Err(Error::invalid("Windows scoped root is not drive-absolute"));
    }
    let mut names = Vec::new();
    for part in parts {
        let Component::Normal(name) = part else {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Root traversal is forbidden",
            ));
        };
        validate_component(name)?;
        names.push(name.to_os_string());
        if names.len() > MAX_COMPONENTS {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Scoped root exceeds component budget",
            ));
        }
    }
    if names.is_empty() {
        return Err(Error::invalid("Scoped root must be below the drive anchor"));
    }
    let anchor_path = PathBuf::from(format!("{}:\\", char::from(drive)));
    let anchor = open_no_reparse(&anchor_path, true, false)?;
    let volume = information(&anchor)?.dwVolumeSerialNumber;
    validate_directory(&anchor, volume)?;
    let count = names.len();
    let mut ancestors = vec![anchor];
    for (index, name) in names.into_iter().enumerate() {
        let mut access = FILE_READ_ATTRIBUTES | FILE_TRAVERSE | SYNCHRONIZE;
        if writable && index + 1 == count {
            access |= FILE_ADD_FILE;
        }
        let next = nt_open_relative(
            ancestors.last().expect("drive anchor is retained"),
            &name,
            access,
            NT_FILE_OPEN,
            FILE_DIRECTORY_FILE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            "Windows root component open failed",
        )?;
        validate_directory(&next, volume)?;
        ancestors.push(next);
    }
    let root = ancestors.pop().expect("root component is retained");
    let root_identity = identity(&information(&root)?);
    Ok((ancestors, root, root_identity))
}

fn mark_delete_on_close(file: &File) -> Result<()> {
    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: file is a live temp HANDLE and disposition is a correctly sized input buffer.
    unsafe {
        SetFileInformationByHandle(
            handle(file),
            FileDispositionInfo,
            (&disposition as *const FILE_DISPOSITION_INFO).cast(),
            size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    }
    .map_err(|_| {
        Error::new(
            ErrorCode::BackendFailed,
            "Windows confined temporary cleanup could not be confirmed",
        )
        .uncertain()
    })
}

fn rename_relative(file: &File, parent: &File, name: &OsStr, replace: bool) -> Result<()> {
    validate_component(name)?;
    let wide = name.encode_wide().collect::<Vec<_>>();
    let name_bytes = wide
        .len()
        .checked_mul(2)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| Error::invalid("Windows rename target exceeds Unicode budget"))?;
    let header = offset_of!(FILE_RENAME_INFORMATION, FileName);
    let payload = header
        .checked_add(wide.len().saturating_mul(2))
        .ok_or_else(|| {
            Error::new(
                ErrorCode::ResourceExhausted,
                "Windows rename buffer overflow",
            )
        })?;
    let total = payload.max(size_of::<FILE_RENAME_INFORMATION>());
    let words = total.div_ceil(size_of::<usize>());
    let mut storage = vec![0usize; words];
    let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
    let total_u32 = u32::try_from(total).map_err(|_| {
        Error::new(
            ErrorCode::ResourceExhausted,
            "Windows rename buffer too large",
        )
    })?;
    let mut io_status = IO_STATUS_BLOCK::default();
    // SAFETY: storage is pointer-aligned and large enough for FILE_RENAME_INFORMATION plus
    // every UTF-16 unit. The destination parent HANDLE remains live for the synchronous call.
    let status = unsafe {
        (*info).Anonymous.ReplaceIfExists = replace;
        (*info).RootDirectory = handle(parent);
        (*info).FileNameLength = name_bytes;
        std::ptr::copy_nonoverlapping(wide.as_ptr(), (*info).FileName.as_mut_ptr(), wide.len());
        NtSetInformationFile(
            handle(file),
            &mut io_status,
            storage.as_ptr().cast(),
            total_u32,
            FileRenameInformation,
        )
    };
    if status.is_err() {
        return Err(nt_error(status, "Windows root-relative rename failed"));
    }
    #[cfg(test)]
    if take_stage_fault(StageFault::AfterNativeRenameBeforeAcknowledgment) {
        // Synthetic lost acknowledgment after the real native effect. This
        // exercises the caller's attempted=true / renamed=false branch; it
        // is not evidence that the operating system returned this error.
        return Err(Error::new(
            ErrorCode::BackendFailed,
            "Injected native rename acknowledgment loss",
        ));
    }
    Ok(())
}

pub struct Root {
    root: File,
    root_identity: FileIdentity,
    ancestors: Vec<File>,
    original_path: PathBuf,
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
        let (ancestors, root, root_identity) = open_pinned_root(path, writable)?;
        Ok(Self {
            root,
            root_identity,
            ancestors,
            original_path: path.to_path_buf(),
            readable,
            writable,
        })
    }

    fn root_still_pinned(&self) -> Result<()> {
        validate_directory(&self.root, self.root_identity.volume)?;
        if identity(&information(&self.root)?) != self.root_identity {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Scoped root identity changed",
            ));
        }
        Ok(())
    }

    fn open_parent(&self, path: &Path, for_write: bool) -> Result<(File, OsString)> {
        let (mut parents, basename) = self.open_parent_chain(path, for_write)?;
        Ok((parents.pop().expect("root parent is retained"), basename))
    }

    fn open_parent_chain(&self, path: &Path, for_write: bool) -> Result<(Vec<File>, OsString)> {
        let mut components = validated_components(path)?;
        let basename = components
            .pop()
            .ok_or_else(|| Error::invalid("Confined path basename missing"))?;
        let parent_count = components.len();
        let mut parents = vec![self.root.try_clone()?];
        for (index, component) in components.into_iter().enumerate() {
            self.root_still_pinned()?;
            let mut desired = FILE_READ_ATTRIBUTES | FILE_TRAVERSE | SYNCHRONIZE;
            if for_write && index + 1 == parent_count {
                desired |= FILE_ADD_FILE;
            }
            let next = nt_open_relative(
                parents.last().expect("root parent is retained"),
                &component,
                desired,
                NT_FILE_OPEN,
                FILE_DIRECTORY_FILE,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                "Windows root-relative directory open failed",
            )?;
            validate_directory(&next, self.root_identity.volume)?;
            parents.push(next);
        }
        self.root_still_pinned()?;
        Ok((parents, basename))
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
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
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
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
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
        let (parent, basename) = self.open_parent(path, false)?;
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

    fn observe_inner(&self, path: &Path, limit: usize) -> Result<ScopedFileObservation> {
        if !self.readable {
            return Err(Error::new(ErrorCode::PolicyDenied, "Root is not readable"));
        }
        if limit == 0 || limit > MAX_SCOPED_BINARY_BYTES {
            return Err(Error::invalid("Read limit exceeds scoped binary budget"));
        }
        self.root_still_pinned()?;
        let (parents, basename) = self.open_parent_chain(path, false)?;
        let parent = parents.last().expect("observation parent is retained");
        let mut file = nt_open_relative(
            parent,
            &basename,
            FILE_GENERIC_READ | SYNCHRONIZE,
            NT_FILE_OPEN,
            FILE_NON_DIRECTORY_FILE,
            // Permit observation beside an existing owner-retained stage's
            // WRITE-access file object without dropping that custody. Its own
            // share mode still denies third-party writes/deletion. Standalone
            // observation does not exclude writers or seal their bytes.
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            "Windows file-instance observation open failed",
        )?;
        let before = validate_regular_file(&file, self.root_identity.volume, true)?;
        let expected = identity(&before);
        let size = file_size(&before);
        if size > limit as u64 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Observed file exceeds read budget",
            ));
        }
        #[cfg(test)]
        run_observation_open_hook();
        let mut bytes = Vec::with_capacity(size as usize);
        (&mut file).take(limit as u64 + 1).read_to_end(&mut bytes)?;
        let after = validate_regular_file(&file, self.root_identity.volume, true)?;
        if bytes.len() > limit {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Observed file grew beyond read budget",
            ));
        }
        if bytes.len() as u64 != size
            || file_size(&after) != size
            || identity(&after) != expected
            || filetime_ticks(after.ftLastWriteTime) != filetime_ticks(before.ftLastWriteTime)
        {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Observed file changed during its retained read",
            ));
        }
        let (_, entry) = self.inspect_existing_file(parent, &basename, true)?;
        if identity(&entry) != expected {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Observed entry changed during its retained read",
            ));
        }
        self.root_still_pinned()?;
        // Size/time checks are change detectors, not a proof of immutable
        // snapshot bytes: writable mappings can outlive their creating handle.
        Ok(ScopedFileObservation {
            bytes,
            instance_identity: native_identity("file", expected),
            method: "windows-handle-relative-file-instance",
            method_version: 1,
        })
    }

    fn write_inner(&self, path: &Path, bytes: &[u8], replace: bool) -> Result<()> {
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
        let (parent, basename) = self.open_parent(path, true)?;

        if replace {
            match self.inspect_existing_file(&parent, &basename, true) {
                Ok(_) => {}
                Err(error) if error.code == ErrorCode::NotFound => {}
                Err(error) => return Err(error),
            }
        }

        let temporary = OsString::from(format!(".semwright-{}.tmp", uuid::Uuid::new_v4().simple()));
        let mut temp = nt_open_relative(
            &parent,
            &temporary,
            FILE_GENERIC_WRITE | FILE_READ_ATTRIBUTES | DELETE | SYNCHRONIZE,
            NT_FILE_CREATE,
            FILE_NON_DIRECTORY_FILE,
            FILE_SHARE_READ | FILE_SHARE_DELETE,
            "Windows confined temporary file creation failed",
        )?;
        let temp_info = match validate_regular_file(&temp, self.root_identity.volume, true) {
            Ok(info) => info,
            Err(error) => {
                mark_delete_on_close(&temp)?;
                return Err(error);
            }
        };
        let temp_identity = identity(&temp_info);
        let mut renamed = false;
        let mut publication_attempted = false;
        let result = (|| -> Result<()> {
            temp.write_all(bytes)?;
            temp.sync_all()?;
            self.root_still_pinned()?;
            publication_attempted = true;
            rename_relative(&temp, &parent, &basename, replace).map_err(|error| {
                // A collision confirms no replacement. Other publication errors
                // preserve the original object and remain unresolved.
                if error.code == ErrorCode::Conflict {
                    error
                } else {
                    error.uncertain()
                }
            })?;
            renamed = true;
            #[cfg(test)]
            if take_stage_fault(StageFault::AfterPublication) {
                return Err(Error::new(
                    ErrorCode::BackendFailed,
                    "Injected publication verification failure",
                )
                .uncertain());
            }

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
            self.root_still_pinned().map_err(Error::uncertain)?;
            Ok(())
        })();
        let known_no_publication = !publication_attempted
            || result
                .as_ref()
                .is_err_and(|error| error.code == ErrorCode::Conflict && error.outcome_known);
        if result.is_err() && !renamed && known_no_publication {
            mark_delete_on_close(&temp)?;
        }
        result
    }

    fn stage_new_inner(&self, path: &Path, bytes: &[u8]) -> RetainedStageResult {
        let prepared = (|| -> Result<(Vec<File>, Vec<File>, OsString)> {
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
            let (parents, basename) = self.open_parent_chain(path, true)?;
            let ancestors = self
                .ancestors
                .iter()
                .map(File::try_clone)
                .collect::<std::io::Result<Vec<_>>>()?;
            Ok((ancestors, parents, basename))
        })();
        let (ancestors, parents, basename) = prepared.map_err(|error| RetainedStageError {
            error,
            retained: None,
        })?;
        let file = nt_open_relative(
            parents.last().expect("parent is retained"),
            &basename,
            FILE_GENERIC_READ | FILE_GENERIC_WRITE | SYNCHRONIZE,
            NT_FILE_CREATE,
            FILE_NON_DIRECTORY_FILE,
            // Protect newly owned bytes and the leaf identity while custody is
            // retained. No write/delete sharing or delete-on-close is granted.
            FILE_SHARE_READ,
            "Windows retained stage creation failed",
        )
        .map_err(|error| RetainedStageError {
            error,
            retained: None,
        })?;
        // Construct custody immediately after successful creation. Every later
        // failure returns this exact object; no path-based cleanup occurs.
        let mut stage = RetainedStage {
            file,
            file_identity: None,
            _ancestors: ancestors,
            parents,
            root_identity: self.root_identity,
            root_path: self.original_path.clone(),
            relative_path: path.to_path_buf(),
            basename,
            read_lock: Mutex::new(()),
            state: RetainedStageState {
                created: true,
                requested_bytes: bytes.len() as u64,
                confirmed_written_bytes: 0,
                partial: !bytes.is_empty(),
                synced: false,
                entry_verified: false,
            },
        };
        match stage.populate(bytes) {
            Ok(()) => Ok(Box::new(stage)),
            Err(error) => Err(RetainedStageError {
                error: error.uncertain(),
                retained: Some(Box::new(stage)),
            }),
        }
    }
}

struct RetainedStage {
    file: File,
    file_identity: Option<FileIdentity>,
    _ancestors: Vec<File>,
    parents: Vec<File>,
    root_identity: FileIdentity,
    root_path: PathBuf,
    relative_path: PathBuf,
    basename: OsString,
    read_lock: Mutex<()>,
    state: RetainedStageState,
}

impl RetainedStage {
    fn populate(&mut self, bytes: &[u8]) -> Result<()> {
        self.file_identity = Some(identity(&validate_regular_file(
            &self.file,
            self.root_identity.volume,
            true,
        )?));
        #[cfg(test)]
        if take_stage_fault(StageFault::PartialWrite) {
            let prefix = bytes.len().min(3);
            self.file.write_all(&bytes[..prefix])?;
            self.state.confirmed_written_bytes = prefix as u64;
            self.state.partial = prefix != bytes.len();
            return Err(Error::new(
                ErrorCode::BackendFailed,
                "Injected partial write failure",
            ));
        }
        while (self.state.confirmed_written_bytes as usize) < bytes.len() {
            match self
                .file
                .write(&bytes[self.state.confirmed_written_bytes as usize..])
            {
                Ok(0) => {
                    return Err(Error::new(
                        ErrorCode::BackendFailed,
                        "Retained stage write made no progress",
                    ));
                }
                Ok(count) => {
                    self.state.confirmed_written_bytes += count as u64;
                    self.state.partial = self.state.confirmed_written_bytes != bytes.len() as u64;
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error.into()),
            }
        }
        #[cfg(test)]
        if take_stage_fault(StageFault::BeforeSync) {
            return Err(Error::new(
                ErrorCode::BackendFailed,
                "Injected stage sync failure",
            ));
        }
        self.file.sync_all()?;
        self.state.synced = true;
        #[cfg(test)]
        if take_stage_fault(StageFault::BeforeEntryVerification) {
            return Err(Error::new(
                ErrorCode::BackendFailed,
                "Injected stage verification failure",
            ));
        }
        self.verify_entry()?;
        self.state.entry_verified = true;
        Ok(())
    }

    fn pinned_identity(&self) -> Result<FileIdentity> {
        let expected = self.file_identity.ok_or_else(|| {
            Error::new(
                ErrorCode::StaleReference,
                "Retained stage identity was not confirmed",
            )
        })?;
        if identity(&information(self.parents.first().expect("root retained"))?)
            != self.root_identity
        {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Retained stage root changed",
            ));
        }
        let observed = identity(&validate_regular_file(
            &self.file,
            self.root_identity.volume,
            true,
        )?);
        if observed != expected {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Retained stage identity changed",
            ));
        }
        Ok(expected)
    }
}

impl RetainedFileStage for RetainedStage {
    fn state(&self) -> RetainedStageState {
        self.state
    }

    fn observe(&self, limit: usize) -> Result<ScopedFileObservation> {
        if limit == 0 || limit > MAX_SCOPED_BINARY_BYTES {
            return Err(Error::invalid("Read limit exceeds scoped binary budget"));
        }
        let _reader = self.read_lock.lock().map_err(|_| {
            Error::new(
                ErrorCode::BackendFailed,
                "Retained stage reader lock is unavailable",
            )
        })?;
        let expected = self.pinned_identity()?;
        let before = information(&self.file)?;
        let size = file_size(&before);
        if size > limit as u64 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Retained stage exceeds read budget",
            ));
        }
        let mut file = &self.file;
        file.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::with_capacity(size as usize);
        file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > limit {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Retained stage grew beyond read budget",
            ));
        }
        if bytes.len() as u64 != size || self.pinned_identity()? != expected {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Retained stage changed during observation",
            ));
        }
        Ok(ScopedFileObservation {
            bytes,
            instance_identity: native_identity("file", expected),
            method: "windows-retained-stage-handle",
            method_version: 1,
        })
    }

    fn verify_entry(&self) -> Result<()> {
        let expected = self.pinned_identity()?;
        let entry = nt_open_relative(
            self.parents.last().expect("parent retained"),
            &self.basename,
            FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            NT_FILE_OPEN,
            FILE_NON_DIRECTORY_FILE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            "Windows retained stage entry verification failed",
        )?;
        let actual = identity(&validate_regular_file(
            &entry,
            self.root_identity.volume,
            true,
        )?);
        if expected != actual {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Retained stage entry names a different file",
            ));
        }
        Ok(())
    }

    fn verify_handoff_path(&self) -> Result<()> {
        let expected = self.pinned_identity()?;
        // This intentionally differs from verify_entry: downstream pathname
        // consumers must not silently follow a renamed/replaced root.
        let current = Root::open(&self.root_path, true, false)?;
        if current.root_identity != self.root_identity {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Retained stage handoff root was replaced",
            ));
        }
        let (parent, name) = current.open_parent(&self.relative_path, false)?;
        let (_, info) = current.inspect_existing_file(&parent, &name, true)?;
        if identity(&info) != expected {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Retained stage handoff path was replaced",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum StageFault {
    PartialWrite,
    BeforeSync,
    BeforeEntryVerification,
    AfterNativeRenameBeforeAcknowledgment,
    AfterPublication,
}
#[cfg(test)]
thread_local! {
    static STAGE_FAULT: std::cell::Cell<Option<StageFault>> = const { std::cell::Cell::new(None) };
    static OBSERVATION_OPEN_HOOK: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        const { std::cell::RefCell::new(None) };
}
#[cfg(test)]
fn run_observation_open_hook() {
    let hook = OBSERVATION_OPEN_HOOK.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}
#[cfg(test)]
fn take_stage_fault(expected: StageFault) -> bool {
    STAGE_FAULT.with(|fault| {
        if fault.get() == Some(expected) {
            fault.set(None);
            true
        } else {
            false
        }
    })
}

impl ScopedRoot for Root {
    fn root_instance_identity(&self) -> Result<String> {
        self.root_still_pinned()?;
        let identity = retained_directory_identity(&self.root)?;
        self.root_still_pinned()?;
        Ok(identity)
    }

    fn observe_file(&self, path: &Path, limit: usize) -> Result<ScopedFileObservation> {
        self.observe_inner(path, limit)
    }

    fn confinement(&self) -> Confinement {
        Confinement::WindowsHandleRelativeNoReparse
    }

    fn read(&self, path: &Path, limit: usize) -> Result<Vec<u8>> {
        self.read_inner(path, limit)
    }

    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        self.write_inner(path, bytes, true)
    }

    fn write_new_atomic(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        self.write_inner(path, bytes, false)
    }

    fn stage_new_retained(&self, path: &Path, bytes: &[u8]) -> RetainedStageResult {
        self.stage_new_inner(path, bytes)
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

    struct FaultGuard;
    impl FaultGuard {
        fn set(fault: StageFault) -> Self {
            STAGE_FAULT.with(|slot| assert!(slot.replace(Some(fault)).is_none()));
            Self
        }
    }
    impl Drop for FaultGuard {
        fn drop(&mut self) {
            STAGE_FAULT.with(|slot| slot.set(None));
        }
    }
    struct ObservationHookGuard;
    impl ObservationHookGuard {
        fn set(hook: impl FnOnce() + 'static) -> Self {
            OBSERVATION_OPEN_HOOK.with(|slot| {
                assert!(slot.borrow_mut().replace(Box::new(hook)).is_none());
            });
            Self
        }
    }
    impl Drop for ObservationHookGuard {
        fn drop(&mut self) {
            OBSERVATION_OPEN_HOOK.with(|slot| *slot.borrow_mut() = None);
        }
    }
    fn failed_stage(result: RetainedStageResult) -> RetainedStageError {
        match result {
            Ok(_) => panic!("expected a failed retained stage"),
            Err(error) => error,
        }
    }

    #[test]
    fn root_open_rejects_a_reparse_ancestor_not_only_the_leaf() {
        let holder = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir(outside.path().join("workspace")).unwrap();
        let link = holder.path().join("redirect");
        junction(&link, outside.path());
        assert!(Root::open(&link.join("workspace"), true, true).is_err());
    }

    #[test]
    fn retained_root_ancestor_swap_does_not_redirect_reads_or_new_staging() {
        let holder = tempfile::tempdir().unwrap();
        let original = holder.path().join("container");
        let retained = holder.path().join("retained-container");
        let workspace = original.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::write(workspace.join("data.txt"), b"owned").unwrap();
        let foreign = tempfile::tempdir().unwrap();
        std::fs::create_dir(foreign.path().join("workspace")).unwrap();
        std::fs::write(foreign.path().join("workspace/data.txt"), b"foreign").unwrap();
        let root = Root::open(&workspace, true, true).unwrap();
        std::fs::rename(&original, &retained).unwrap();
        junction(&original, foreign.path());
        assert_eq!(root.read(Path::new("data.txt"), 16).unwrap(), b"owned");
        let stage = root
            .stage_new_retained(Path::new("new.bin"), b"new-owned")
            .unwrap();
        assert!(stage.verify_entry().is_ok());
        assert!(stage.verify_handoff_path().is_err());
        assert_eq!(stage.observe(64).unwrap().bytes, b"new-owned");
        assert!(!foreign.path().join("workspace/new.bin").exists());
        drop(stage);
        assert_eq!(
            std::fs::read(retained.join("workspace/new.bin")).unwrap(),
            b"new-owned"
        );
    }

    #[test]
    fn atomic_no_replace_preserves_existing_destination() {
        let directory = tempfile::tempdir().unwrap();
        let root = Root::open(directory.path(), true, true).unwrap();
        root.write_new_atomic(Path::new("owned.bin"), b"first")
            .unwrap();
        let error = root
            .write_new_atomic(Path::new("owned.bin"), b"second")
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Conflict);
        assert!(error.outcome_known);
        assert_eq!(root.read(Path::new("owned.bin"), 64).unwrap(), b"first");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn atomic_no_replace_race_has_one_winner_and_preserves_its_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let root = std::sync::Arc::new(Root::open(directory.path(), true, true).unwrap());
        let start = std::sync::Arc::new(std::sync::Barrier::new(2));
        let workers: Vec<_> = [b"first".as_slice(), b"second".as_slice()]
            .into_iter()
            .map(|bytes| {
                let root = root.clone();
                let start = start.clone();
                std::thread::spawn(move || {
                    start.wait();
                    (bytes, root.write_new_atomic(Path::new("race.bin"), bytes))
                })
            })
            .collect();
        let results: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect();
        assert_eq!(
            results.iter().filter(|(_, result)| result.is_ok()).count(),
            1
        );
        let winner = results.iter().find(|(_, result)| result.is_ok()).unwrap().0;
        let error = results
            .iter()
            .find_map(|(_, result)| result.as_ref().err())
            .unwrap();
        assert_eq!(error.code, ErrorCode::Conflict);
        assert!(error.outcome_known);
        assert_eq!(root.read(Path::new("race.bin"), 64).unwrap(), winner);
    }

    #[test]
    fn atomic_no_replace_after_publication_failure_is_unknown_and_preserved() {
        let directory = tempfile::tempdir().unwrap();
        let root = Root::open(directory.path(), true, true).unwrap();
        let _fault = FaultGuard::set(StageFault::AfterPublication);
        let error = root
            .write_new_atomic(Path::new("committed.bin"), b"committed")
            .unwrap_err();
        assert!(!error.outcome_known);
        assert_eq!(
            root.read(Path::new("committed.bin"), 64).unwrap(),
            b"committed"
        );
    }

    #[test]
    fn atomic_no_replace_lost_native_acknowledgment_preserves_published_object() {
        let directory = tempfile::tempdir().unwrap();
        let root = Root::open(directory.path(), true, true).unwrap();
        let _fault = FaultGuard::set(StageFault::AfterNativeRenameBeforeAcknowledgment);
        let error = root
            .write_new_atomic(Path::new("unacknowledged.bin"), b"committed")
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::BackendFailed);
        assert!(!error.outcome_known);
        // The helper failed before the caller set `renamed`. Deleting the
        // original temporary HANDLE here would delete this published file.
        assert_eq!(
            std::fs::read(directory.path().join("unacknowledged.bin")).unwrap(),
            b"committed"
        );
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn readonly_root_refuses_both_new_write_paths_without_creation() {
        let directory = tempfile::tempdir().unwrap();
        let root = Root::open(directory.path(), true, false).unwrap();
        assert_eq!(
            root.write_new_atomic(Path::new("a.bin"), b"x")
                .unwrap_err()
                .code,
            ErrorCode::PolicyDenied
        );
        let error = failed_stage(root.stage_new_retained(Path::new("b.bin"), b"x"));
        assert_eq!(error.error.code, ErrorCode::PolicyDenied);
        assert!(error.error.outcome_known);
        assert!(error.retained.is_none());
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    #[test]
    fn retained_stage_roundtrip_does_not_grant_other_file_reads() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("other.bin"), b"other").unwrap();
        let root = Root::open(directory.path(), false, true).unwrap();
        let stage = root
            .stage_new_retained(Path::new("new.bin"), b"retained")
            .unwrap();
        assert_eq!(
            stage.state(),
            RetainedStageState {
                created: true,
                requested_bytes: 8,
                confirmed_written_bytes: 8,
                partial: false,
                synced: true,
                entry_verified: true,
            }
        );
        assert_eq!(stage.observe(8).unwrap().bytes, b"retained");
        assert!(stage.verify_entry().is_ok());
        assert!(stage.verify_handoff_path().is_ok());
        assert!(root.read(Path::new("other.bin"), 32).is_err());
        assert_eq!(
            stage.observe(7).unwrap_err().code,
            ErrorCode::ResourceExhausted
        );
        drop(stage);
        assert_eq!(
            std::fs::read(directory.path().join("new.bin")).unwrap(),
            b"retained"
        );
    }

    #[test]
    fn partial_stage_failure_returns_exact_retained_prefix_and_never_deletes_it() {
        let directory = tempfile::tempdir().unwrap();
        let root = Root::open(directory.path(), true, true).unwrap();
        let _fault = FaultGuard::set(StageFault::PartialWrite);
        let error = failed_stage(root.stage_new_retained(Path::new("partial.bin"), b"abcdefgh"));
        assert!(!error.error.outcome_known);
        let stage = error.retained.expect("partial file custody");
        assert_eq!(
            stage.state(),
            RetainedStageState {
                created: true,
                requested_bytes: 8,
                confirmed_written_bytes: 3,
                partial: true,
                synced: false,
                entry_verified: false,
            }
        );
        assert_eq!(stage.observe(8).unwrap().bytes, b"abc");
        assert!(stage.verify_entry().is_ok());
        drop(stage);
        assert_eq!(
            std::fs::read(directory.path().join("partial.bin")).unwrap(),
            b"abc"
        );
    }

    #[test]
    fn stage_sync_failure_retains_complete_unsynced_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let root = Root::open(directory.path(), true, true).unwrap();
        let _fault = FaultGuard::set(StageFault::BeforeSync);
        let error = failed_stage(root.stage_new_retained(Path::new("unsynced.bin"), b"complete"));
        assert!(!error.error.outcome_known);
        let stage = error.retained.unwrap();
        assert_eq!(stage.state().confirmed_written_bytes, 8);
        assert!(!stage.state().partial);
        assert!(!stage.state().synced);
        assert!(!stage.state().entry_verified);
        assert_eq!(stage.observe(8).unwrap().bytes, b"complete");
    }

    #[test]
    fn stage_verification_failure_retains_synced_original_without_claiming_entry() {
        let directory = tempfile::tempdir().unwrap();
        let root = Root::open(directory.path(), true, true).unwrap();
        let _fault = FaultGuard::set(StageFault::BeforeEntryVerification);
        let error = failed_stage(root.stage_new_retained(Path::new("verify.bin"), b"complete"));
        assert!(!error.error.outcome_known);
        let stage = error.retained.unwrap();
        assert!(stage.state().synced);
        assert!(!stage.state().entry_verified);
        assert_eq!(stage.observe(8).unwrap().bytes, b"complete");
        assert!(stage.verify_entry().is_ok());
    }

    #[test]
    fn retained_stage_collision_returns_no_borrowed_custody() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("existing.bin"), b"original").unwrap();
        let root = Root::open(directory.path(), true, true).unwrap();
        let error = failed_stage(root.stage_new_retained(Path::new("existing.bin"), b"changed"));
        assert_eq!(error.error.code, ErrorCode::Conflict);
        assert!(error.error.outcome_known);
        assert!(error.retained.is_none());
        assert_eq!(
            std::fs::read(directory.path().join("existing.bin")).unwrap(),
            b"original"
        );
    }

    #[test]
    fn retained_stage_rejects_junction_parent_without_outside_creation() {
        let directory = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        junction(&directory.path().join("escape"), outside.path());
        let root = Root::open(directory.path(), true, true).unwrap();
        let error = failed_stage(root.stage_new_retained(Path::new("escape/new.bin"), b"blocked"));
        assert!(error.retained.is_none());
        assert!(!outside.path().join("new.bin").exists());
    }

    #[test]
    fn retained_stage_prevents_external_write_until_custody_is_released() {
        let directory = tempfile::tempdir().unwrap();
        let root = Root::open(directory.path(), true, true).unwrap();
        let stage = root
            .stage_new_retained(Path::new("held.bin"), b"original")
            .unwrap();
        assert!(std::fs::write(directory.path().join("held.bin"), b"replacement").is_err());
        assert_eq!(stage.observe(8).unwrap().bytes, b"original");
        drop(stage);
        assert_eq!(
            std::fs::read(directory.path().join("held.bin")).unwrap(),
            b"original"
        );
    }

    #[test]
    fn retained_stage_outlives_the_root_and_rejects_external_delete_and_rename() {
        let directory = tempfile::tempdir().unwrap();
        let root = Root::open(directory.path(), false, true).unwrap();
        let stage = root
            .stage_new_retained(Path::new("held.bin"), b"owned")
            .unwrap();
        drop(root);
        let original = directory.path().join("held.bin");
        let renamed = directory.path().join("renamed.bin");
        assert!(std::fs::remove_file(&original).is_err());
        assert!(std::fs::rename(&original, &renamed).is_err());
        assert!(!renamed.exists());
        assert!(stage.verify_entry().is_ok());
        assert!(stage.verify_handoff_path().is_ok());
        assert_eq!(stage.observe(5).unwrap().bytes, b"owned");
        drop(stage);
        assert_eq!(std::fs::read(&original).unwrap(), b"owned");
        // Releasing custody only closes handles; ordinary deletion/rename is
        // possible afterward and the file was not deleted by Drop.
        std::fs::rename(&original, &renamed).unwrap();
        assert_eq!(std::fs::read(&renamed).unwrap(), b"owned");
        std::fs::remove_file(&renamed).unwrap();
    }

    #[test]
    fn retained_stage_keeps_custody_during_root_rename_attempt() {
        let holder = tempfile::tempdir().unwrap();
        let original = holder.path().join("workspace");
        let retained = holder.path().join("retained-workspace");
        std::fs::create_dir(&original).unwrap();
        let root = Root::open(&original, false, true).unwrap();
        let stage = root
            .stage_new_retained(Path::new("held.bin"), b"owned")
            .unwrap();
        drop(root);
        assert_stage_custody_during_parent_rename(stage, &original, &retained);
    }

    #[test]
    fn retained_stage_keeps_custody_during_nested_parent_rename_attempt() {
        let directory = tempfile::tempdir().unwrap();
        let original = directory.path().join("nested");
        let retained = directory.path().join("retained-nested");
        std::fs::create_dir(&original).unwrap();
        let root = Root::open(directory.path(), false, true).unwrap();
        let stage = root
            .stage_new_retained(Path::new("nested/held.bin"), b"owned")
            .unwrap();
        drop(root);
        assert_stage_custody_during_parent_rename(stage, &original, &retained);
    }

    fn assert_stage_custody_during_parent_rename(
        stage: Box<dyn RetainedFileStage>,
        original: &Path,
        retained: &Path,
    ) {
        match std::fs::rename(original, retained) {
            Ok(()) => {
                eprintln!("retained-stage parent rename: moved while retained");
                std::fs::create_dir(original).unwrap();
                std::fs::write(original.join("held.bin"), b"foreign").unwrap();
                assert!(stage.verify_entry().is_ok());
                assert!(stage.verify_handoff_path().is_err());
                assert_eq!(stage.observe(5).unwrap().bytes, b"owned");
                drop(stage);
                assert_eq!(std::fs::read(retained.join("held.bin")).unwrap(), b"owned");
                assert_eq!(
                    std::fs::read(original.join("held.bin")).unwrap(),
                    b"foreign"
                );
            }
            Err(error) => {
                // Windows may deny parent rename while a descendant forbids
                // delete sharing. Accept only that concrete protection branch,
                // and prove causality by releasing solely the retained capsule.
                assert!(
                    matches!(error.raw_os_error(), Some(5 | 32)),
                    "unexpected rename error: {error}"
                );
                eprintln!(
                    "retained-stage parent rename: denied while retained (error={:?})",
                    error.raw_os_error()
                );
                assert!(original.is_dir());
                assert!(!retained.exists());
                assert!(stage.verify_entry().is_ok());
                assert!(stage.verify_handoff_path().is_ok());
                assert_eq!(stage.observe(5).unwrap().bytes, b"owned");
                drop(stage);
                std::fs::rename(original, retained).unwrap();
                assert_eq!(std::fs::read(retained.join("held.bin")).unwrap(), b"owned");
                assert!(!original.exists());
            }
        }
    }

    #[test]
    fn retained_empty_stage_has_complete_zero_byte_state() {
        let directory = tempfile::tempdir().unwrap();
        let root = Root::open(directory.path(), true, true).unwrap();
        let stage = root
            .stage_new_retained(Path::new("empty.bin"), b"")
            .unwrap();
        assert_eq!(stage.state().confirmed_written_bytes, 0);
        assert!(!stage.state().partial);
        assert!(stage.state().synced);
        assert!(stage.state().entry_verified);
        assert!(stage.observe(1).unwrap().bytes.is_empty());
    }

    #[test]
    fn expected_identity_compares_the_opened_root_before_any_write() {
        let parent = tempfile::tempdir().unwrap();
        let original = parent.path().join("workspace");
        let retained = parent.path().join("owned-workspace");
        std::fs::create_dir(&original).unwrap();
        let owned = Root::open(&original, true, true).unwrap();
        let expected = owned.root_instance_identity().unwrap();
        let reopened = WindowsFilesystem
            .open_root_with_identity(&original, true, true, &expected)
            .unwrap();
        assert_eq!(reopened.root_instance_identity().unwrap(), expected);
        drop(reopened);
        std::fs::rename(&original, &retained).unwrap();
        std::fs::create_dir(&original).unwrap();
        let result = WindowsFilesystem.open_root_with_identity(&original, true, true, &expected);
        let Err(error) = result else {
            panic!("a replacement directory must be rejected")
        };
        assert_eq!(error.code, ErrorCode::StaleReference);
        assert_eq!(owned.root_instance_identity().unwrap(), expected);
        assert_eq!(std::fs::read_dir(&original).unwrap().count(), 0);
        // The previously captured object remains usable even after the path
        // names a different directory. No reopening occurs on this path.
        owned
            .write_new_atomic(Path::new("owned.bin"), b"owned")
            .unwrap();
        assert_eq!(std::fs::read(retained.join("owned.bin")).unwrap(), b"owned");
        assert!(!original.join("owned.bin").exists());
    }

    #[test]
    fn file_instance_observation_binds_equal_bytes_to_the_actual_file() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.bin");
        std::fs::write(&source, b"same-bytes").unwrap();
        let root = Root::open(directory.path(), true, false).unwrap();
        let first = root.observe_file(Path::new("source.bin"), 32).unwrap();
        assert_eq!(first.bytes, b"same-bytes");
        assert_eq!(first.method, "windows-handle-relative-file-instance");
        assert_eq!(first.method_version, 1);
        std::fs::rename(&source, directory.path().join("old-source.bin")).unwrap();
        std::fs::write(&source, b"same-bytes").unwrap();
        let replacement = root.observe_file(Path::new("source.bin"), 32).unwrap();
        assert_eq!(replacement.bytes, first.bytes);
        assert_ne!(replacement.instance_identity, first.instance_identity);
    }

    #[test]
    fn file_instance_observation_keeps_original_root_after_path_replacement() {
        let holder = tempfile::tempdir().unwrap();
        let original = holder.path().join("workspace");
        let retained = holder.path().join("retained-workspace");
        std::fs::create_dir(&original).unwrap();
        std::fs::write(original.join("source.bin"), b"owned").unwrap();
        let root = Root::open(&original, true, false).unwrap();
        let first = root.observe_file(Path::new("source.bin"), 32).unwrap();
        let directory_identity = root.root_instance_identity().unwrap();
        std::fs::rename(&original, &retained).unwrap();
        std::fs::create_dir(&original).unwrap();
        std::fs::write(original.join("source.bin"), b"foreign").unwrap();
        let second = root.observe_file(Path::new("source.bin"), 32).unwrap();
        assert_eq!(second.instance_identity, first.instance_identity);
        assert_eq!(second.bytes, b"owned");
        assert_eq!(root.root_instance_identity().unwrap(), directory_identity);
        assert_eq!(
            std::fs::read(original.join("source.bin")).unwrap(),
            b"foreign"
        );
    }

    #[test]
    fn file_instance_observation_enforces_read_authority_and_exact_budget() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("source.bin"), b"owned").unwrap();
        let write_only = Root::open(directory.path(), false, true).unwrap();
        assert_eq!(
            write_only
                .observe_file(Path::new("source.bin"), 5)
                .unwrap_err()
                .code,
            ErrorCode::PolicyDenied
        );
        let readable = Root::open(directory.path(), true, false).unwrap();
        assert_eq!(
            readable
                .observe_file(Path::new("source.bin"), 4)
                .unwrap_err()
                .code,
            ErrorCode::ResourceExhausted
        );
        assert!(readable.observe_file(Path::new("source.bin"), 0).is_err());
        assert!(
            readable
                .observe_file(Path::new("source.bin"), MAX_SCOPED_BINARY_BYTES + 1)
                .is_err()
        );
        assert_eq!(
            readable
                .observe_file(Path::new("source.bin"), 5)
                .unwrap()
                .bytes,
            b"owned"
        );
    }

    #[test]
    fn file_instance_observation_rejects_hardlinks_and_junction_traversal() {
        let directory = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("source.bin"), b"foreign").unwrap();
        junction(&directory.path().join("escape"), outside.path());
        std::fs::hard_link(
            outside.path().join("source.bin"),
            directory.path().join("hard.bin"),
        )
        .unwrap();
        let root = Root::open(directory.path(), true, false).unwrap();
        assert!(
            root.observe_file(Path::new("escape/source.bin"), 32)
                .is_err()
        );
        assert!(root.observe_file(Path::new("hard.bin"), 32).is_err());
        assert_eq!(
            std::fs::read(outside.path().join("source.bin")).unwrap(),
            b"foreign"
        );
    }

    #[test]
    fn file_instance_observation_detects_concurrent_length_change() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.bin");
        let renamed = directory.path().join("renamed.bin");
        std::fs::write(&source, b"owned").unwrap();
        let root = Root::open(directory.path(), true, false).unwrap();
        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = attempts.clone();
        let during_source = source.clone();
        let during_renamed = renamed.clone();
        let _hook = ObservationHookGuard::set(move || {
            std::thread::spawn(move || {
                // A generic observer must remain share-compatible with a
                // legitimate retained stage; it does not exclude writers.
                std::fs::write(&during_source, b"replacement").unwrap();
                count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let errors = [
                    std::fs::remove_file(&during_source).unwrap_err(),
                    std::fs::rename(&during_source, &during_renamed).unwrap_err(),
                ];
                for error in errors {
                    assert!(
                        matches!(error.raw_os_error(), Some(5 | 32)),
                        "unexpected mutation error: {error}"
                    );
                    count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }
            })
            .join()
            .unwrap();
        });
        let error = root.observe_file(Path::new("source.bin"), 32).unwrap_err();
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 3);
        assert_eq!(error.code, ErrorCode::StaleReference);
        assert!(!renamed.exists());
        assert_eq!(std::fs::read(&source).unwrap(), b"replacement");
        // The observer's no-delete sharing ends when the failed observation
        // releases its handle. Metadata checks are not an immutable file seal.
        std::fs::rename(&source, &renamed).unwrap();
        std::fs::remove_file(&renamed).unwrap();
    }

    #[test]
    fn retained_stage_is_observable_from_readonly_root_without_releasing_custody() {
        let directory = tempfile::tempdir().unwrap();
        let output = Root::open(directory.path(), false, true).unwrap();
        let media = Root::open(directory.path(), true, false).unwrap();
        let stage = output
            .stage_new_retained(Path::new("owned.bin"), b"owned")
            .unwrap();
        let owned = stage.observe(32).unwrap();
        drop(output);
        let source = directory.path().join("owned.bin");
        let renamed = directory.path().join("renamed.bin");
        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = attempts.clone();
        let during_source = source.clone();
        let during_renamed = renamed.clone();
        let _hook = ObservationHookGuard::set(move || {
            std::thread::spawn(move || {
                let errors = [
                    std::fs::write(&during_source, b"replacement").unwrap_err(),
                    std::fs::remove_file(&during_source).unwrap_err(),
                    std::fs::rename(&during_source, &during_renamed).unwrap_err(),
                ];
                for error in errors {
                    assert!(
                        matches!(error.raw_os_error(), Some(5 | 32)),
                        "unexpected mutation error: {error}"
                    );
                    count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }
            })
            .join()
            .unwrap();
        });
        // This independently opened read-only root must read the same actual
        // file while the stage retains its original WRITE-access file object.
        let observed = media.observe_file(Path::new("owned.bin"), 32).unwrap();
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 3);
        assert_eq!(observed.bytes, owned.bytes);
        assert_eq!(observed.instance_identity, owned.instance_identity);
        assert!(stage.verify_entry().is_ok());
        assert_eq!(stage.observe(32).unwrap().bytes, b"owned");
        assert!(std::fs::write(&source, b"replacement").is_err());
        assert!(std::fs::remove_file(&source).is_err());
        assert!(std::fs::rename(&source, &renamed).is_err());
        drop(stage);
        // Only releasing the original stage permits these mutations. No
        // handle reopening or custody downgrade was used for observation.
        std::fs::write(&source, b"replacement").unwrap();
        std::fs::rename(&source, &renamed).unwrap();
        std::fs::remove_file(&renamed).unwrap();
    }
}

#[cfg(test)]
mod retained_directory_identity_tests {
    use super::*;

    #[test]
    fn shared_directory_identity_preserves_the_existing_encoding_and_rejects_files() {
        let directory = tempfile::tempdir().unwrap();
        let root = Root::open(directory.path(), true, false).unwrap();
        let expected = native_identity("directory", root.root_identity);
        assert_eq!(retained_directory_identity(&root.root).unwrap(), expected);
        assert_eq!(root.root_instance_identity().unwrap(), expected);
        let path = directory.path().join("not-a-directory");
        std::fs::write(&path, b"file").unwrap();
        let file = File::open(path).unwrap();
        assert_eq!(
            retained_directory_identity(&file).unwrap_err().code,
            ErrorCode::Unsupported
        );
    }
}
