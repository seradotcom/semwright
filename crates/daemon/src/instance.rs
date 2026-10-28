//! Owner-selected, private daemon instance directories.
//!
//! This option separates a development broker's audit/runtime from the installed
//! broker. It changes storage location only; policy, approvals and grants remain
//! independently enforced.
use semwright_types::{Error, ErrorCode, Result};
use std::path::{Component, Path, PathBuf};

fn validate_existing_root(root: &Path) -> Result<()> {
    if !root.is_absolute()
        || root
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err(Error::invalid(
            "Instance data directory must be an absolute normal path",
        ));
    }
    #[cfg(windows)]
    {
        use std::path::Prefix;
        if !matches!(root.components().next(), Some(Component::Prefix(p)) if matches!(p.kind(), Prefix::Disk(_)))
        {
            return Err(Error::invalid(
                "Instance data directory requires a local drive path; UNC/device paths are forbidden",
            ));
        }
        for component in root.components() {
            if let Component::Normal(name) = component {
                let text = name.to_string_lossy();
                if text.contains(':') || text.ends_with(['.', ' ']) {
                    return Err(Error::invalid(
                        "Instance data directory contains an ambiguous Windows component",
                    ));
                }
            }
        }
    }
    for ancestor in root.ancestors() {
        let metadata = std::fs::symlink_metadata(ancestor)?;
        let mut unsafe_path = !metadata.is_dir() || metadata.file_type().is_symlink();
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
            unsafe_path |= metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0;
        }
        if unsafe_path {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "Instance data directory cannot traverse a link or reparse point",
            ));
        }
    }
    let canonical = root.canonicalize()?;
    #[cfg(windows)]
    let matches = canonical
        .to_string_lossy()
        .strip_prefix(r"\\?\")
        .is_some_and(|normal| normal.eq_ignore_ascii_case(&root.to_string_lossy()));
    #[cfg(not(windows))]
    let matches = canonical == root;
    if !matches {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Instance data directory is not canonical",
        ));
    }
    Ok(())
}

pub fn private_paths(root: &Path, fake: bool) -> Result<(PathBuf, PathBuf)> {
    // The owner must create/select this root explicitly. Never recursively create
    // a path before examining all existing ancestors.
    validate_existing_root(root)?;
    semwright_platform_services::private_data_directory(root)?;
    let runtime = root.join("runtime");
    let state = root.join(if fake { "fake-state" } else { "state" });
    for child in [&runtime, &state] {
        if child.try_exists()? {
            validate_existing_root(child)?;
        }
        semwright_platform_services::private_data_directory(child)?;
        validate_existing_root(child)?;
    }
    Ok((runtime, state))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_directories_are_separate_and_private() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path();
        let (runtime, state) = private_paths(path, false).unwrap();
        assert_eq!(runtime, path.join("runtime"));
        assert_eq!(state, path.join("state"));
        assert!(runtime.is_dir() && state.is_dir());
        let (_, fake_state) = private_paths(path, true).unwrap();
        assert_ne!(state, fake_state);
    }

    #[test]
    fn relative_missing_and_parent_paths_are_rejected_before_creation() {
        assert!(private_paths(Path::new("relative-instance"), false).is_err());
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("not-created");
        assert!(private_paths(&missing, false).is_err());
        assert!(!missing.exists());
        assert!(private_paths(&root.path().join(".."), false).is_err());
    }

    #[test]
    #[cfg(windows)]
    fn windows_unc_device_and_ads_paths_are_rejected() {
        for path in [
            r"\\localhost\c$\Windows",
            r"\\?\C:\Windows",
            r"\\.\C:\Windows",
            r"C:\Windows:stream",
            r"C:\Windows.",
        ] {
            assert!(private_paths(Path::new(path), false).is_err(), "{path}");
        }
    }

    #[test]
    #[cfg(unix)]
    fn linked_instance_root_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("target");
        std::fs::create_dir(&target).unwrap();
        let link = root.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(private_paths(&link, false).is_err());
    }
}
