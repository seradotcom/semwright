//! Private snapshot of one owner-authorized Blender runtime.
//! Source files are opened without write/delete sharing; output is a new owner-only tree.
use super::*;
use std::{
    collections::BTreeMap,
    io::Write,
    os::windows::ffi::OsStrExt,
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
    verify_trusted_file_acl(&file, WindowsAclPurpose::NativeRuntime)?;
    Ok((file, metadata))
}

// The approved Blender executable does not opt into long Win32 paths.
// Enabling the registry flag alone cannot make its Python importer compatible.
const MAX_NATIVE_PATH_UNITS: usize = 259;

fn check_native_path(path: &Path) -> Result<()> {
    let units = path.as_os_str().encode_wide().count();
    if units > MAX_NATIVE_PATH_UNITS {
        eprintln!(
            "{}",
            serde_json::json!({
                "component":"platform-windows",
                "phase":"native_runtime_path_too_long",
                "utf16_units":units,
                "maximum_utf16_units":MAX_NATIVE_PATH_UNITS,
                "remedy":"Choose a shorter owner-private SEMWRIGHT_DATA_DIR",
            })
        );
        return Err(Error::invalid(
            "Native Blender runtime path exceeds 259 UTF-16 units; choose a shorter SEMWRIGHT_DATA_DIR",
        ));
    }
    Ok(())
}

fn preflight_runtime_paths(
    source: &Path,
    destination: &Path,
    depth: usize,
    entries: &mut usize,
    budget: &Budget<'_>,
) -> Result<()> {
    budget.check()?;
    check_native_path(destination)?;
    if depth > WINDOWS_MOUNT_MAX_DEPTH {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Native runtime depth exceeded",
        ));
    }
    // Retain each ancestor against replacement while enumerating beneath it.
    let (_directory, metadata) = locked(source)?;
    if metadata.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 == 0 {
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
        let child_source = source.join(entry.file_name());
        let child_destination = destination.join(entry.file_name());
        check_native_path(&child_destination)?;
        let metadata = path_info_no_reparse(&child_source)?;
        if metadata.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Native runtime cannot contain reparse points",
            ));
        }
        if metadata.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0 {
            preflight_runtime_paths(
                &child_source,
                &child_destination,
                depth + 1,
                entries,
                budget,
            )?;
        } else {
            require_single_link_regular(&metadata)?;
        }
    }
    Ok(())
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
    check_native_path(destination)?;
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
        check_native_path(&child_destination)?;
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
    // Refuse the layout before copying any runtime bytes. The copy checks again
    // so a later source-tree change cannot evade the prospective path budget.
    let mut path_entries = 0;
    preflight_runtime_paths(source, destination, 0, &mut path_entries, &budget)?;
    // Blender is an owner-pinned application binary, not a generic <=64 MiB
    // sealed driver tool. Preserve its native-PE, digest, ACL and signature
    // checks using the already bounded 512 MiB application verifier.
    verify_application_executable(&source.join("blender.exe"), executable_sha256)?;
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
    // Independently verify copied executable bytes and identity before
    // publishing the transient snapshot; never trust the source check alone.
    verify_application_executable(&destination.join("blender.exe"), executable_sha256)?;
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
    use std::path::PathBuf;
    #[test]
    fn native_runtime_path_budget_counts_utf16_and_nul_boundary() {
        let accepted = PathBuf::from(format!(r"E:\a\{}", "a".repeat(254)));
        assert_eq!(accepted.as_os_str().encode_wide().count(), 259);
        check_native_path(&accepted).unwrap();
        let rejected = PathBuf::from(format!(r"E:\a\{}", "a".repeat(255)));
        assert_eq!(
            check_native_path(&rejected).unwrap_err().code,
            ErrorCode::InvalidArgument
        );
        let surrogate_pair = PathBuf::from(format!(r"E:\a\{}{}", "a".repeat(253), '\u{1f600}'));
        assert_eq!(surrogate_pair.to_string_lossy().chars().count(), 259);
        assert_eq!(surrogate_pair.as_os_str().encode_wide().count(), 260);
        assert_eq!(
            check_native_path(&surrogate_pair).unwrap_err().code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn native_runtime_long_import_path_is_refused_before_snapshot_creation() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        std::fs::create_dir(&source).unwrap();
        let user = std::env::var("USERNAME").unwrap();
        let principal = match std::env::var("USERDOMAIN") {
            Ok(domain) if !domain.is_empty() => format!(r"{domain}\{user}"),
            _ => user,
        };
        assert!(
            std::process::Command::new("icacls")
                .arg(&source)
                .args(["/inheritance:r", "/grant:r"])
                .arg(format!("{principal}:(OI)(CI)F"))
                .status()
                .unwrap()
                .success()
        );
        let relative = Path::new(
            "5.2/scripts/addons_core/io_scene_gltf2/blender/exp/material/pbr_metallic_roughness.py",
        );
        let original = source.join(relative);
        std::fs::create_dir_all(original.parent().unwrap()).unwrap();
        std::fs::write(&original, b"owner runtime module").unwrap();
        let base = dir.path().as_os_str().encode_wide().count();
        assert!(
            base < 200,
            "CI temporary directory exceeds this bounded fixture"
        );
        let parent = dir.path().join("d".repeat(210 - base - 1));
        std::fs::create_dir(&parent).unwrap();
        let destination = parent.join("runtime");
        check_native_path(&destination).unwrap();
        assert!(destination.join(relative).as_os_str().encode_wide().count() > 259);
        let error = stage(
            &source,
            &destination,
            &"0".repeat(64),
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidArgument);
        assert!(error.message.contains("shorter SEMWRIGHT_DATA_DIR"));
        assert!(
            !destination.exists(),
            "Preflight must not create a partial snapshot"
        );
        assert_eq!(std::fs::read(original).unwrap(), b"owner runtime module");
    }

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
    fn owner_pinned_large_blender_pe_survives_source_and_snapshot_verification() {
        // Blender 5.2.1 is >64 MiB. The generic sealed-tool verifier must
        // reject this source, but the owner-pinned application verifier can
        // accept up to 512 MiB without dropping ACL/PE/hash checks. CI-only
        // disposable fixture: append zero bytes to an unsigned native PE.
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source");
        let private_state = directory.path().join("private-state");
        std::fs::create_dir(&source).unwrap();
        std::fs::create_dir(&private_state).unwrap();
        let exe = source.join("blender.exe");
        std::fs::copy(std::env::current_exe().unwrap(), &exe).unwrap();
        let bytes = MAX_EXECUTABLE + 1;
        assert!(bytes < MAX_APPLICATION_EXECUTABLE);
        let file = std::fs::OpenOptions::new().write(true).open(&exe).unwrap();
        assert!(file.metadata().unwrap().len() < bytes);
        file.set_len(bytes).unwrap();
        drop(file);
        let principal = match std::env::var("USERDOMAIN") {
            Ok(domain) if !domain.is_empty() => {
                format!(r"{domain}\{}", std::env::var("USERNAME").unwrap())
            }
            _ => std::env::var("USERNAME").unwrap(),
        };
        for path in [&source, &exe, &private_state] {
            let acl = std::process::Command::new("icacls")
                .arg(path)
                .arg("/inheritance:r")
                .arg("/grant:r")
                .arg(format!("{principal}:(F)"))
                .status()
                .unwrap();
            assert!(
                acl.success(),
                "could not secure test fixture: {}",
                path.display()
            );
        }
        let digest = hex::encode(Sha256::digest(std::fs::read(&exe).unwrap()));
        assert!(
            verify_sealed_tool_executable(&exe, &digest).is_err(),
            "Generic <=64 MiB sealed tools may never accept this Blender fixture"
        );
        verify_application_executable(&exe, &digest)
            .expect("Owner-pinned native application supports the larger PE budget");
        let dest = private_state.join("snapshot");
        let receipt = stage(&source, &dest, &digest, &AtomicBool::new(false))
            .expect("Native Blender stage must verify both large source and copied PE");
        assert_eq!(receipt["bytes"], bytes);
        assert_eq!(receipt["entries"], 1);
        assert_eq!(receipt["files"]["blender.exe"], digest);
        assert!(dest.join("blender.exe").is_file());
        verify_application_executable(&dest.join("blender.exe"), &digest)
            .expect("Private native snapshot must preserve PE and executable digest");
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
    fn native_runtime_acl_accepts_only_exact_trusted_installer_and_inherit_only_templates() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("vendor-runtime");
        std::fs::create_dir(&source).unwrap();
        let user = std::env::var("USERNAME").expect("Windows test username");
        let principal = match std::env::var("USERDOMAIN") {
            Ok(domain) if !domain.is_empty() => format!(r"{domain}\{user}"),
            _ => user,
        };
        let run_icacls = |args: &[&str]| {
            let status = std::process::Command::new("icacls")
                .arg(&source)
                .args(args)
                .status()
                .expect("Windows icacls");
            assert!(
                status.success(),
                "icacls rejected fixture arguments: {args:?}"
            );
        };
        // No source-installation changes: all ACL operations target only a
        // fresh disposable CI directory owned by this test process.
        run_icacls(&["/inheritance:r"]);
        let owner = format!("{principal}:(F)");
        run_icacls(&["/grant:r", &owner]);
        run_icacls(&[
            "/grant",
            "*S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464:(F)",
        ]);
        run_icacls(&["/grant", "*S-1-3-0:(OI)(CI)(IO)(F)"]);
        let (handle, _) = locked(&source).expect(
            "TrustedInstaller plus inherit-only CREATOR OWNER must not block owner-pinned runtime",
        );
        assert_eq!(
            verify_trusted_file_acl(&handle, WindowsAclPurpose::Executable)
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied,
            "Generic executables must not inherit native TrustedInstaller exception"
        );
        assert_eq!(
            verify_trusted_file_acl(&handle, WindowsAclPurpose::PrivateData)
                .unwrap_err()
                .code,
            ErrorCode::PermissionDenied,
            "Private user data must never inherit vendor runtime ACL exception"
        );
        drop(handle);

        // The exception must never allow unrelated principals to mutate.
        run_icacls(&["/grant:r", "*S-1-5-32-545:(M)"]);
        let denied = locked(&source);
        assert_eq!(
            denied.err().unwrap().code,
            ErrorCode::PermissionDenied,
            "An effective Windows Users Modify ACE is untrusted in native runtime"
        );
        // SID equality must be exact, not any member of S-1-5-80-*.
        let exact = trusted_installer_sid_bytes();
        assert_eq!(exact.len(), 32);
        let mut different_service = exact.clone();
        *different_service.last_mut().unwrap() ^= 1;
        assert!(
            !sid_matches(PSID(different_service.as_mut_ptr().cast()), &exact,),
            "An unrelated service SID must not match TrustedInstaller"
        );
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
