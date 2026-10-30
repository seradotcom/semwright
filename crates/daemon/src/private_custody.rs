//! Shared private, owner-held, no-replace custody publication.
use semwright_types::{Error, ErrorCode, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub(super) struct PrivateDirectory {
    pub(super) path: PathBuf,
    #[cfg(target_os = "linux")]
    root: semwright_platform_services::Root,
}

impl PrivateDirectory {
    pub(super) fn open(path: &Path) -> Result<Self> {
        #[cfg(not(target_os = "linux"))]
        {
            let _ = path;
            Err(Error::new(
                ErrorCode::Unsupported,
                "Private no-replace Host stdin custody is supported only on Linux",
            ))
        }
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::MetadataExt;
            if !path.is_absolute()
                || path.parent().is_none()
                || std::fs::canonicalize(path)? != path
            {
                return Err(Error::invalid("Custody directory must be canonical"));
            }
            let uid = semwright_platform_services::current_uid();
            let metadata = std::fs::symlink_metadata(path)?;
            if !metadata.is_dir()
                || metadata.file_type().is_symlink()
                || metadata.uid() != uid
                || metadata.mode() & 0o777 != 0o700
            {
                return Err(Error::new(
                    ErrorCode::PermissionDenied,
                    "Custody directory must already be private and owner-held",
                ));
            }
            let parent = path
                .parent()
                .ok_or_else(|| Error::invalid("Custody parent missing"))?;
            let parent_metadata = std::fs::symlink_metadata(parent)?;
            if !parent_metadata.is_dir()
                || parent_metadata.file_type().is_symlink()
                || parent_metadata.uid() != uid
                || parent_metadata.mode() & 0o777 != 0o700
            {
                return Err(Error::new(
                    ErrorCode::PermissionDenied,
                    "Custody requires a dedicated private owner parent",
                ));
            }
            // Reject untrusted writable ancestors. This does not protect against
            // the trusted owner renaming its own tree during setup. Subsequent
            // writes use only the retained SDK Root.
            for ancestor in path.ancestors().skip(1) {
                let metadata = std::fs::symlink_metadata(ancestor)?;
                let sticky_root_directory = metadata.uid() == 0 && metadata.mode() & 0o1000 != 0;
                if !metadata.is_dir()
                    || metadata.file_type().is_symlink()
                    || (metadata.uid() != uid && metadata.uid() != 0)
                    || (metadata.mode() & 0o022 != 0 && !sticky_root_directory)
                {
                    return Err(Error::new(
                        ErrorCode::PermissionDenied,
                        "Custody ancestor is not protected from replacement",
                    ));
                }
            }
            if std::fs::read_dir(path)?.next().is_some() {
                return Err(Error::new(
                    ErrorCode::Conflict,
                    "Custody directory must be empty and dedicated to this process",
                ));
            }
            let root = semwright_platform_services::Root::open(path, true, true)?;
            Ok(Self {
                path: path.to_path_buf(),
                root,
            })
        }
    }

    pub(super) fn write_new(&self, name: &str, bytes: &[u8]) -> Result<()> {
        // All callers supply fixed internal names; no provider/request/job ID
        // participates in a path. The SDK primitive retains the directory fd.
        if name.contains('/') || name.contains('\\') || name.is_empty() {
            return Err(Error::invalid("Invalid internal custody filename"));
        }
        #[cfg(target_os = "linux")]
        {
            self.root.write_new_atomic(Path::new(name), bytes)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = bytes;
            Err(Error::new(
                ErrorCode::Unsupported,
                "Private no-replace custody publication is unavailable",
            ))
        }
    }
}

pub(super) fn bounded_json(value: &Value, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec(value)
        .map_err(|_| Error::invalid("Custody metadata encoding failed"))?;
    bytes.push(b'\n');
    if bytes.len() > limit {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Custody metadata exceeds its fixed budget",
        ));
    }
    Ok(bytes)
}
