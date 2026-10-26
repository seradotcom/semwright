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
/// This is an observation through one retained object, not an immutable file
/// seal or a guarantee against every concurrent memory-mapped modification.
#[derive(Debug)]
pub struct ScopedFileObservation {
    pub bytes: Vec<u8>,
    pub instance_identity: String,
    pub method: &'static str,
    pub method_version: u32,
}

/// Facts confirmed while creating one new file. They are observations, not a
/// grant or a promise that a pathname will still name the file at a later time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetainedStageState {
    pub created: bool,
    pub requested_bytes: u64,
    pub confirmed_written_bytes: u64,
    pub partial: bool,
    pub synced: bool,
    pub entry_verified: bool,
}

/// Owner-held custody of a newly created file, including a partial write.
/// Implementations retain native file and directory identities. Dropping the
/// capsule closes handles; it must not unlink a possibly published object.
pub trait RetainedFileStage: Send + Sync {
    fn state(&self) -> RetainedStageState;
    fn observe(&self, limit: usize) -> Result<ScopedFileObservation>;
    /// Confirm the entry relative to the retained parent, independently of the
    /// original absolute pathname. A root rename need not invalidate this check.
    fn verify_entry(&self) -> Result<()>;
    /// Separately check the original handoff pathname against retained identity.
    /// This observation is not a reservation of a future pathname-based open.
    fn verify_handoff_path(&self) -> Result<()>;
}

pub struct RetainedStageError {
    pub error: Error,
    /// Present whenever creation succeeded, even if a later step failed.
    pub retained: Option<Box<dyn RetainedFileStage>>,
}

impl std::fmt::Debug for RetainedStageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RetainedStageError")
            .field("error", &self.error)
            .field(
                "retained_state",
                &self.retained.as_ref().map(|file| file.state()),
            )
            .finish()
    }
}

pub type RetainedStageResult = std::result::Result<Box<dyn RetainedFileStage>, RetainedStageError>;

pub trait ScopedRoot: Send + Sync {
    /// Opaque native directory-instance evidence from this retained root.
    /// An expected value can bind a later owner-authorized open to the same
    /// directory; neither the value nor a pathname grants filesystem access.
    /// Keep the original root alive through later comparisons when available.
    /// Native identifiers and timestamps are instance evidence, not permanent
    /// anti-reuse proof. After reopening, an owner declaration supplies policy;
    /// its saved identity string never substitutes for fresh live authority.
    fn root_instance_identity(&self) -> Result<String> {
        Err(Error::new(
            ErrorCode::Unsupported,
            "Native root-instance identity is unavailable",
        ))
    }
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
    /// Create a new final entry without replacement and retain its custody on
    /// success or partial failure. This is not an atomic-publication operation.
    /// Reading through the returned capsule is limited to this newly owned file;
    /// it does not grant access to other files under a write-only root.
    fn stage_new_retained(&self, _path: &Path, _bytes: &[u8]) -> RetainedStageResult {
        Err(RetainedStageError {
            error: Error::new(
                ErrorCode::Unsupported,
                "Retained file staging is unavailable",
            ),
            retained: None,
        })
    }
}
pub trait ScopedFilesystem: Send + Sync {
    fn open_root(&self, path: &Path, read: bool, write: bool) -> Result<Box<dyn ScopedRoot>>;
    /// Compare identity on the actual opened root and return that same object.
    /// There is no check-then-reopen window and no serialized live capability.
    /// The owner must separately authorize this open. A persisted owner policy
    /// can be revalidated against the fresh retained object; a saved identity
    /// string alone is neither an authorization nor a serialized capability.
    fn open_root_with_identity(
        &self,
        path: &Path,
        read: bool,
        write: bool,
        expected_identity: &str,
    ) -> Result<Box<dyn ScopedRoot>> {
        if expected_identity.is_empty()
            || expected_identity.len() > 256
            || expected_identity.chars().any(char::is_control)
        {
            return Err(Error::invalid("Expected root identity is malformed"));
        }
        let root = self.open_root(path, read, write)?;
        if root.root_instance_identity()? != expected_identity {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Opened root differs from the owner-selected directory",
            ));
        }
        Ok(root)
    }
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

    #[test]
    fn default_retained_stage_refuses_without_replacement_io() {
        let root = ReplacementOnly(AtomicUsize::new(0));
        let Err(error) = root.stage_new_retained(Path::new("owned.bin"), b"bytes") else {
            panic!("unsupported backend must not create a retained stage");
        };
        assert_eq!(error.error.code, ErrorCode::Unsupported);
        assert!(error.error.outcome_known);
        assert!(error.retained.is_none());
        assert_eq!(root.0.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn default_root_identity_cannot_infer_authority_from_a_path() {
        struct ReplacementFilesystem;
        impl ScopedFilesystem for ReplacementFilesystem {
            fn open_root(&self, _: &Path, _: bool, _: bool) -> Result<Box<dyn ScopedRoot>> {
                Ok(Box::new(ReplacementOnly(AtomicUsize::new(0))))
            }
        }
        let result = ReplacementFilesystem.open_root_with_identity(
            Path::new("configured-root"),
            true,
            true,
            "unproven-directory",
        );
        let Err(error) = result else {
            panic!("unobserved identity must be refused")
        };
        assert_eq!(error.code, ErrorCode::Unsupported);
    }
}
