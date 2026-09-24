use semwright_types::{Error, ErrorCode, Result};
use std::path::{Component, Path};

/// Bounded binary handoff budget. Text filesystem commands remain schema-limited to 1 MiB.
pub const MAX_SCOPED_BINARY_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confinement {
    LinuxOpenat2NoSymlinksNoMounts,
    /// Pinned root, single child only. NOT an openat2-equivalent traversal claim.
    PinnedRootSingleChild,
    /// Windows HANDLE-verified pinned root; initial implementation is read-only and one child.
    WindowsPinnedRootSingleChildReadOnly,
}

pub trait ScopedRoot: Send + Sync {
    fn confinement(&self) -> Confinement;
    fn read(&self, path: &Path, limit: usize) -> Result<Vec<u8>>;
    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> Result<()>;
}
pub trait ScopedFilesystem: Send + Sync {
    fn open_root(&self, path: &Path, read: bool, write: bool) -> Result<Box<dyn ScopedRoot>>;
}

/// Lexical validation is a precondition, never the enforcement boundary.
pub fn validate_relative_path(path: &Path) -> Result<()> {
    let bytes = path.as_os_str().as_encoded_bytes();
    if bytes.is_empty() || bytes.len() > 4096 || path.is_absolute() || bytes.contains(&0) {
        return Err(Error::invalid("Expected bounded nonempty relative path"));
    }
    if path
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(Error::new(
            ErrorCode::PolicyDenied,
            "Path traversal is forbidden",
        ));
    }
    // '/' is portable protocol syntax. Windows adds its own '\\', ADS and device-name checks.
    if bytes
        .split(|b| *b == b'/')
        .any(|c| c.is_empty() || c == b"." || c == b"..")
    {
        return Err(Error::invalid(
            "Empty, dot and parent components are forbidden",
        ));
    }
    Ok(())
}
