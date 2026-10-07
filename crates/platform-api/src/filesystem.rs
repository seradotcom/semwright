use semwright_types::{Error, ErrorCode, Result};
use std::path::{Component, Path};

/// Bounded binary handoff budget. Text filesystem commands remain schema-limited to 1 MiB.
pub const MAX_SCOPED_BINARY_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confinement {
    LinuxOpenat2NoSymlinksNoMounts,
    /// Pinned root, single child only. NOT an openat2-equivalent traversal claim.
    PinnedRootSingleChild,
    /// Historical narrow Windows HANDLE-verified root: read-only and one immediate child.
    WindowsPinnedRootSingleChildReadOnly,
    /// Windows HANDLE-relative traversal with every component opened no-reparse and pinned.
    /// This is deliberately not claimed to be equivalent to Linux openat2 semantics.
    WindowsHandleRelativeNoReparse,
}

/// A bounded read plus native instance evidence. Bytes alone are not logical identity.
/// No caller may treat this as a grant or a compare-and-swap reservation.
#[derive(Debug)]
pub struct ScopedFileObservation {
    pub bytes: Vec<u8>,
    pub instance_identity: String,
    pub method: &'static str,
    pub method_version: u32,
}

pub trait ScopedRoot: Send + Sync {
    /// Backends without instance evidence must fail closed, not infer identity from a path.
    fn observe_file(&self, _path: &Path, _limit: usize) -> Result<ScopedFileObservation> {
        Err(Error::new(
            ErrorCode::Unsupported,
            "Native file-instance observation is unavailable",
        ))
    }
    fn confinement(&self) -> Confinement;
    fn read(&self, path: &Path, limit: usize) -> Result<Vec<u8>>;
    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> Result<()>;
    /// Publish a new file atomically without replacing any existing destination.
    /// Unsupported backends must refuse; replacement is never a fallback.
    fn write_new_atomic(&self, _path: &Path, _bytes: &[u8]) -> Result<()> {
        Err(Error::new(
            ErrorCode::Unsupported,
            "Atomic no-replace publication is unavailable",
        ))
    }
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

#[cfg(test)]
mod no_replace_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct ReplacementOnly(AtomicUsize);
    impl ScopedRoot for ReplacementOnly {
        fn confinement(&self) -> Confinement {
            Confinement::PinnedRootSingleChild
        }
        fn read(&self, _: &Path, _: usize) -> Result<Vec<u8>> {
            panic!("read must not run")
        }
        fn write_atomic(&self, _: &Path, _: &[u8]) -> Result<()> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }
    #[test]
    fn default_no_replace_refuses_without_replacement_io() {
        let root = ReplacementOnly(AtomicUsize::new(0));
        let error = root
            .write_new_atomic(Path::new("owned.bin"), b"bytes")
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Unsupported);
        assert!(error.outcome_known);
        assert_eq!(root.0.load(Ordering::SeqCst), 0);
    }
}
