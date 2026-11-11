//! Private snapshot of one owner-authorized Blender runtime.
//! Source files are opened without write/delete sharing; output is a new owner-only tree.
use super::*;
use std::{
    collections::BTreeMap,
    io::Write,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

struct Budget<'a> {
    cancel: &'a AtomicBool,
    deadline: Instant,
}
impl Budget<'_> {
    fn check(&self) -> Result<()> {
        if self.cancel.load(Ordering::Acquire) {
            return Err(Error::new(
                ErrorCode::Cancelled,
                "Native runtime staging cancelled",
            ));
        }
        if Instant::now() >= self.deadline {
            return Err(Error::new(
                ErrorCode::Timeout,
                "Native runtime staging deadline exceeded",
            ));
        }
        Ok(())
    }
}

/// Compare physical identities along both ancestor chains, including alternate casing.
pub fn paths_overlap(left: &Path, right: &Path) -> Result<bool> {
    if !left.is_absolute() || !right.is_absolute() {
        return Err(Error::invalid("Native roots must be absolute"));
    }
    let left_id = file_identity(&path_info_no_reparse(left)?);
    let right_id = file_identity(&path_info_no_reparse(right)?);
    let mut overlap = false;
    for (path, other) in [(left, right_id), (right, left_id)] {
        if path
            .components()
            .any(|c| matches!(c, Component::CurDir | Component::ParentDir))
        {
            return Err(Error::invalid("Native roots cannot contain dot components"));
        }
        for ancestor in path.ancestors().filter(|p| !p.as_os_str().is_empty()) {
            let metadata = path_info_no_reparse(ancestor)?;
            if metadata.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
                return Err(Error::new(
                    ErrorCode::PolicyDenied,
                    "Native roots cannot traverse reparse points",
                ));
            }
            overlap |= file_identity(&metadata) == other;
        }
    }
    Ok(overlap)
}

const MAX_RUNTIME_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const MAX_RUNTIME_FILE_BYTES: u64 = 512 * 1024 * 1024;

fn locked(path: &Path) -> Result<(File, BY_HANDLE_FILE_INFORMATION)> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ.0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0 | FILE_FLAG_BACKUP_SEMANTICS.0)
        .open(path)?;
    let metadata = info(&file)?;
    if metadata.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err(Error::new(
            ErrorCode::PolicyDenied,
            "Native runtime cannot contain reparse points",
        ));
    }
    verify_trusted_file_acl(&file, "native runtime", false)?;
    Ok((file, metadata))
}

fn copy_tree(
    source: &Path,
    destination: &Path,
    relative: &Path,
    depth: usize,
    total: &mut u64,
    entries: &mut usize,
    hashes: &mut BTreeMap<String, String>,
    budget: &Budget<'_>,
) -> Result<()> {
    budget.check()?;
    if depth > WINDOWS_MOUNT_MAX_DEPTH {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Native runtime depth exceeded",
        ));
    }
    let (directory, before) = locked(source)?;
    if before.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 == 0 {
        return Err(Error::invalid("Native runtime directory required"));
    }
    for entry in std::fs::read_dir(source)? {
        budget.check()?;
        let entry = entry?;
        *entries += 1;
        if *entries > WINDOWS_MOUNT_MAX_ENTRIES {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Native runtime entry budget exceeded",
            ));
        }
        let name = entry.file_name();
        let child_source = source.join(&name);
        let child_destination = destination.join(&name);
        let child_relative = relative.join(&name);
        let (mut input, metadata) = locked(&child_source)?;
        if metadata.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0 {
            drop(input);
            std::fs::create_dir(&child_destination)?;
            copy_tree(
                &child_source,
                &child_destination,
                &child_relative,
                depth + 1,
                total,
                entries,
                hashes,
                budget,
            )?;
        } else {
            require_single_link_regular(&metadata)?;
            let length = ((metadata.nFileSizeHigh as u64) << 32) | metadata.nFileSizeLow as u64;
            *total = total
                .checked_add(length)
                .ok_or_else(|| Error::invalid("Native runtime size overflow"))?;
            if length > MAX_RUNTIME_FILE_BYTES || *total > MAX_RUNTIME_BYTES {
                return Err(Error::new(
                    ErrorCode::ResourceExhausted,
                    "Native runtime byte budget exceeded",
                ));
            }
            let mut output = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&child_destination)?;
            let mut hash = Sha256::new();
            let mut buffer = vec![0u8; 1024 * 1024];
            let mut copied = 0u64;
            loop {
                budget.check()?;
                let count = input.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                copied += count as u64;
                if copied > length {
                    return Err(Error::new(
                        ErrorCode::Conflict,
                        "Native runtime source grew",
                    ));
                }
                hash.update(&buffer[..count]);
                output.write_all(&buffer[..count])?;
            }
            // This is a new, transient private snapshot, never a reusable durable cache.
            // File::write_all is unbuffered in userspace; close the writer before any
            // verification/launch. Per-file FlushFileBuffers adds no launch authority
            // and forced 6,500 serial disk barriers on the measured Windows runtime.
            drop(output);
            let after = info(&input)?;
            if copied != length
                || !same_identity(&metadata, &after)
                || metadata.ftLastWriteTime != after.ftLastWriteTime
                || metadata.nFileSizeHigh != after.nFileSizeHigh
                || metadata.nFileSizeLow != after.nFileSizeLow
            {
                return Err(Error::new(
                    ErrorCode::Conflict,
                    "Native runtime source changed during snapshot",
                ));
            }
            let rel = child_relative
                .to_str()
                .ok_or_else(|| Error::invalid("Native runtime path must be Unicode"))?
                .replace('\\', "/");
            hashes.insert(rel, hex::encode(hash.finalize()));
        }
    }
    if !same_identity(&before, &info(&directory)?) {
        return Err(Error::new(
            ErrorCode::Conflict,
            "Native runtime directory identity changed",
        ));
    }
    Ok(())
}

/// Called only by Driver Host after the independent owner grant and exact tool/mount contract.
pub fn stage(
    source: &Path,
    destination: &Path,
    executable_sha256: &str,
    cancel: &AtomicBool,
) -> Result<serde_json::Value> {
    let budget = Budget {
        cancel,
        deadline: Instant::now() + Duration::from_secs(180),
    };
    budget.check()?;
    if !source.is_absolute()
        || !destination.is_absolute()
        || destination.exists()
        || destination.starts_with(source)
        || source.starts_with(destination)
    {
        return Err(Error::invalid(
            "Native runtime staging requires a new disjoint absolute directory",
        ));
    }
    let parent = destination
        .parent()
        .ok_or_else(|| Error::invalid("Native staging parent"))?;
    if paths_overlap(source, parent)? {
        return Err(Error::new(
            ErrorCode::PolicyDenied,
            "Native staging cannot overlap its source runtime",
        ));
    }
    let (_, is_directory) = validate_mount_tree(source)?;
    if !is_directory {
        return Err(Error::invalid("Native runtime root must be a directory"));
    }
    verify_sealed_tool_executable(&source.join("blender.exe"), executable_sha256)?;
    std::fs::create_dir(destination)?;
    crate::paths::ensure_owner_only_directory(destination)?;
    let mut bytes = 0;
    let mut entries = 0;
    let mut hashes = BTreeMap::new();
    let copied = copy_tree(
        source,
        destination,
        Path::new(""),
        0,
        &mut bytes,
        &mut entries,
        &mut hashes,
        &budget,
    );
    if let Err(error) = copied {
        let _ = std::fs::remove_dir_all(destination);
        return Err(error);
    }
    verify_sealed_tool_executable(&destination.join("blender.exe"), executable_sha256)?;
    let encoded = serde_json::to_vec(&hashes)
        .map_err(|_| Error::invalid("Native runtime inventory encoding"))?;
    Ok(
        serde_json::json!({"schema":"semwright-private-blender-runtime/v1",
        "executable_sha256":executable_sha256,"files":hashes,"bytes":bytes,"entries":entries,
        "inventory_sha256":hex::encode(Sha256::digest(&encoded)),
        "source_kind":"owner-authorized runtime; copied files individually hashed",
        "distribution_signature_claim":false}),
    )
}

#[cfg(test)]
mod native_runtime_tests {
    use super::*;
    #[test]
    fn windows_native_runtime_cancel_precedes_io() {
        let source = std::env::temp_dir().join(format!("absent-source-{}", unique_id()));
        let destination = std::env::temp_dir().join(format!("absent-destination-{}", unique_id()));
        let cancel = AtomicBool::new(true);
        assert_eq!(
            stage(&source, &destination, &"a".repeat(64), &cancel)
                .unwrap_err()
                .code,
            ErrorCode::Cancelled
        );
        assert!(!destination.exists());
    }
    #[test]
    fn native_runtime_requires_exact_pinned_application_before_publication() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("owner-runtime");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("unexpected.dll"), b"not-blender").unwrap();
        let destination = dir.path().join("private-copy");
        let cancel = AtomicBool::new(false);
        let rejected = stage(&source, &destination, &"0".repeat(64), &cancel);
        assert!(rejected.is_err(), "{rejected:?}");
        assert!(
            !destination.exists(),
            "Failed verification must not publish a runtime"
        );
        assert_eq!(
            std::fs::read(source.join("unexpected.dll")).unwrap(),
            b"not-blender"
        );
    }

    #[test]
    fn native_runtime_will_not_replace_an_existing_destination() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("runtime");
        let destination = dir.path().join("already-owned");
        std::fs::create_dir(&source).unwrap();
        std::fs::create_dir(&destination).unwrap();
        let file = destination.join("existing.txt");
        std::fs::write(&file, b"preserve").unwrap();
        let rejected = stage(
            &source,
            &destination,
            &"0".repeat(64),
            &AtomicBool::new(false),
        );
        assert!(rejected.is_err(), "{rejected:?}");
        assert_eq!(std::fs::read(file).unwrap(), b"preserve");
    }

    #[test]
    fn windows_native_runtime_overlap_uses_physical_identity() {
        let root = std::env::temp_dir().join(format!("native-path-proof-{}", unique_id()));
        let nested = root.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        assert!(paths_overlap(&root, &nested).unwrap());
        assert!(paths_overlap(&nested, &root).unwrap());
        let upper = std::path::PathBuf::from(root.to_string_lossy().to_uppercase());
        assert!(paths_overlap(&upper, &root).unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }
}
