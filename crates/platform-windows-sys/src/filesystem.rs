use semwright_platform_api::filesystem::{
    Confinement, ScopedFilesystem, ScopedRoot, validate_relative_path,
};
use semwright_types::{Error, ErrorCode, Result};
use std::{
    fs::File,
    io::Read,
    os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    path::{Component, Path, PathBuf},
};
use windows::Win32::{
    Foundation::HANDLE,
    Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_NAME_NORMALIZED, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, GETFINALPATHNAMEBYHANDLE_FLAGS, GetFileInformationByHandle,
        GetFinalPathNameByHandleW, VOLUME_NAME_DOS,
    },
};

const MAX_FILE: u64 = 64 * 1024 * 1024;

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

fn final_path(file: &File) -> Result<String> {
    let mut buf = vec![0u16; 32_768];
    // SAFETY: live handle and writable UTF-16 buffer. The API does not retain the buffer.
    let n = unsafe {
        GetFinalPathNameByHandleW(
            handle(file),
            &mut buf,
            GETFINALPATHNAMEBYHANDLE_FLAGS(FILE_NAME_NORMALIZED.0 | VOLUME_NAME_DOS.0),
        )
    } as usize;
    if n == 0 || n >= buf.len() {
        return Err(Error::new(
            ErrorCode::BackendFailed,
            "Final Windows path lookup failed",
        ));
    }
    String::from_utf16(&buf[..n])
        .map_err(|_| Error::new(ErrorCode::BackendFailed, "Final Windows path was invalid"))
}

fn safe_root_syntax(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        return Err(Error::invalid("Windows scoped root must be absolute"));
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

fn validate_single_child(path: &Path) -> Result<String> {
    validate_relative_path(path)?;
    let mut components = path.components();
    let Component::Normal(name) = components
        .next()
        .ok_or_else(|| Error::invalid("Missing child"))?
    else {
        return Err(Error::new(
            ErrorCode::PolicyDenied,
            "Invalid Windows child path",
        ));
    };
    if components.next().is_some() {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "Nested Windows traversal is fail-closed until root-relative open is proven",
        ));
    }
    let value = name.to_string_lossy().into_owned();
    if value.contains(':') || value.ends_with(['.', ' ']) || reserved_device(&value) {
        return Err(Error::new(
            ErrorCode::PolicyDenied,
            "ADS, reserved device names and trailing dot/space names are forbidden",
        ));
    }
    Ok(value)
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

pub struct Root {
    root_path: PathBuf,
    root: File,
    root_identity: FileIdentity,
    root_final: String,
}

impl Root {
    pub fn open(path: &Path, readable: bool, writable: bool) -> Result<Self> {
        if !readable || writable {
            return Err(Error::new(
                ErrorCode::SandboxDenied,
                "Initial Windows confinement is deliberately read-only",
            ));
        }
        safe_root_syntax(path)?;
        let root = open_no_reparse(path, true)?;
        let info = information(&root)?;
        let root_identity = identity(&info);
        let mut root_final = final_path(&root)?;
        while root_final.ends_with(['\\', '/']) {
            root_final.pop();
        }
        Ok(Self {
            root_path: path.to_path_buf(),
            root,
            root_identity,
            root_final,
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

    fn read_inner(&self, path: &Path, limit: usize) -> Result<Vec<u8>> {
        self.root_still_pinned()?;
        let child = validate_single_child(path)?;
        let file = open_no_reparse(&self.root_path.join(child), false)?;
        let info = information(&file)?;
        if info.nNumberOfLinks != 1 {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Hard-linked files are not accepted by Windows confinement",
            ));
        }
        if info.nFileSizeHigh != 0
            || info.nFileSizeLow as u64 > MAX_FILE
            || info.nFileSizeLow as usize > limit
        {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Confined file exceeds read budget",
            ));
        }
        if identity(&info).volume != self.root_identity.volume {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Volume boundary crossing is forbidden",
            ));
        }
        let child_final = final_path(&file)?;
        let expected = format!("{}\\", self.root_final).to_ascii_lowercase();
        if !child_final.to_ascii_lowercase().starts_with(&expected) {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Opened handle escaped its pinned root",
            ));
        }
        self.root_still_pinned()?;
        let mut bytes = Vec::with_capacity((info.nFileSizeLow as usize).min(limit));
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
}

impl ScopedRoot for Root {
    fn confinement(&self) -> Confinement {
        Confinement::WindowsPinnedRootSingleChildReadOnly
    }

    fn read(&self, path: &Path, limit: usize) -> Result<Vec<u8>> {
        self.read_inner(path, limit)
    }

    fn write_atomic(&self, _path: &Path, _bytes: &[u8]) -> Result<()> {
        Err(Error::new(
            ErrorCode::SandboxDenied,
            "Windows atomic confined writes are fail-closed until root-relative rename is proven",
        ))
    }
}

pub struct WindowsFilesystem;
impl ScopedFilesystem for WindowsFilesystem {
    fn open_root(&self, path: &Path, read: bool, write: bool) -> Result<Box<dyn ScopedRoot>> {
        Ok(Box::new(Root::open(path, read, write)?))
    }
}

#[cfg(test)]
mod lexical_tests {
    use super::*;
    #[test]
    fn rejects_windows_ambiguous_child_names() {
        for s in [
            "a/b",
            "a\\b",
            "file:stream",
            "CON",
            "LPT1.txt",
            "name.",
            "name ",
        ] {
            assert!(validate_single_child(Path::new(s)).is_err(), "{s}");
        }
        assert_eq!(
            validate_single_child(Path::new("report.txt")).unwrap(),
            "report.txt"
        );
    }
}
