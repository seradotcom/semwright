use semwright_types::{Error, ErrorCode, Result};
use std::path::{Component, Path};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confinement {
    LinuxOpenat2NoSymlinksNoMounts,
    /// Pinned root, single child only. NOT an openat2-equivalent traversal claim.
    PinnedRootSingleChild,
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
/// Deliberately does not perform Unicode/case normalization of filenames.
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
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lexical_paths() {
        for p in ["", "../x", "a/../b", "/x", "a/./b", "a//b", "a/", "a\0b"] {
            assert!(validate_relative_path(Path::new(p)).is_err(), "{p:?}");
        }
        for p in ["a", "a/b", "á", "a\\b"] {
            assert!(validate_relative_path(Path::new(p)).is_ok());
        }
    }
}
