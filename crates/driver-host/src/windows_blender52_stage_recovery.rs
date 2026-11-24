//! Owner-private crash leftovers for the closed Windows Blender native profile.
//!
//! Only a completed, source-bound staging tree can be reclaimed. Active Host
//! instances hold an OS byte-range lock; unconfirmed native exits are retained.
//! This is not a general user-cache sweeper or a recovery grant.

use semwright_types::{Error, ErrorCode, Result, unique_id};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::windows::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

// Compact names leave room for Blender's deep Python module paths. The nonce,
// lease and receipt format are unchanged; the old namespace is not swept.
const NAMESPACE: &str = "bn1";
const PREFIX: &str = "b-";
const LOCK: &str = ".stage-lock";
const LOCK_CONTENT: &[u8] = b"semwright-native-stage-lease-v1\n";
const READY: &str = ".stage-ready.json";
const UNCONFIRMED: &str = ".stage-unconfirmed-exit";
const FORMAT: &str = "semwright-blender-stage-v1";
const MIN_RECLAIM_AGE: Duration = Duration::from_secs(15 * 60);
const MAX_STAGE_FILES: usize = 12_000;
const MAX_STAGE_DIRECTORIES: usize = 2_000;
const MAX_STAGE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const REPARSE_ATTRIBUTE: u32 = 0x400;
const MAX_RECLAIM_INSPECTION: Duration = Duration::from_secs(20);
const FILE_SHARE_READ: u32 = 1;
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;

pub(super) struct Lease {
    path: PathBuf,
    lock: Option<File>,
    uncertain_exit: bool,
}

impl Lease {
    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn retain_unconfirmed_exit(&mut self) {
        self.uncertain_exit = true;
    }

    pub(super) fn publish(
        &self,
        runtime_receipt: &[u8],
        bootstrap_receipt: &[u8],
        executable_sha256: &str,
    ) -> Result<()> {
        let marker = serde_json::json!({
            "format": FORMAT,
            "runtime_receipt_sha256": hex::encode(Sha256::digest(runtime_receipt)),
            "bootstrap_receipt_sha256": hex::encode(Sha256::digest(bootstrap_receipt)),
            "executable_sha256": executable_sha256,
        });
        let mut handle = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.path.join(READY))?;
        handle.write_all(&serde_json::to_vec(&marker)?)?;
        handle.sync_all()?;
        Ok(())
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        if self.uncertain_exit {
            // A bounded kill without observed exit is not permission to remove
            // the filesystem backing a possibly live native child.
            let _ = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(self.path.join(UNCONFIRMED))
                .and_then(|mut file| file.write_all(b"native exit unconfirmed\n"));
        }
        // Windows cannot remove the lock file while this handle is still open.
        // Closing it also releases LockFileEx's kernel-owned byte-range lease.
        self.lock.take();
        if !self.uncertain_exit {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

fn plain_metadata(path: &Path) -> Option<fs::Metadata> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if metadata.file_type().is_symlink() || metadata.file_attributes() & REPARSE_ATTRIBUTE != 0 {
        None
    } else {
        Some(metadata)
    }
}

#[derive(Default)]
struct ReclaimPlan {
    files: Vec<PathBuf>,
    directories: Vec<PathBuf>,
    bytes: u64,
}

fn expected_directories(files: &BTreeMap<String, String>) -> Option<BTreeSet<String>> {
    if files.is_empty() || files.len() > MAX_STAGE_FILES {
        return None;
    }
    let mut directories = BTreeSet::new();
    for (name, digest) in files {
        if name.contains(['\\', ':'])
            || name
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
            || digest.len() != 64
            || !digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return None;
        }
        let mut child = name.as_str();
        while let Some((parent, _)) = child.rsplit_once('/') {
            directories.insert(parent.to_owned());
            if directories.len() > MAX_STAGE_DIRECTORIES {
                return None;
            }
            child = parent;
        }
    }
    Some(directories)
}

fn verify_payload_file(
    path: &Path,
    digest: &str,
    size: u64,
    deadline: Instant,
    buffer: &mut [u8],
) -> Option<()> {
    let mut file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .ok()?;
    let before = file.metadata().ok()?;
    if !before.is_file()
        || before.file_attributes() & REPARSE_ATTRIBUTE != 0
        || before.len() != size
    {
        return None;
    }
    let mut hash = Sha256::new();
    let mut bytes = 0u64;
    loop {
        if Instant::now() >= deadline {
            return None;
        }
        let count = file.read(buffer).ok()?;
        if count == 0 {
            break;
        }
        bytes = bytes.checked_add(count as u64)?;
        if bytes > size {
            return None;
        }
        hash.update(&buffer[..count]);
    }
    let after = file.metadata().ok()?;
    (bytes == size
        && after.len() == before.len()
        && after.last_write_time() == before.last_write_time()
        && hex::encode(hash.finalize()) == digest)
        .then_some(())
}

fn verify_tree(
    root: &Path,
    files: &BTreeMap<String, String>,
    directories: &BTreeSet<String>,
    deadline: Instant,
    plan: &mut ReclaimPlan,
) -> Option<()> {
    let mut pending = vec![root.to_path_buf()];
    let mut found_files = 0usize;
    let mut found_directories = 0usize;
    let mut buffer = vec![0u8; 64 * 1024];
    while let Some(directory) = pending.pop() {
        if Instant::now() >= deadline || !plain_metadata(&directory)?.is_dir() {
            return None;
        }
        plan.directories.push(directory.clone());
        if plan.directories.len() > MAX_STAGE_DIRECTORIES {
            return None;
        }
        for item in fs::read_dir(&directory).ok()? {
            if Instant::now() >= deadline {
                return None;
            }
            let path = item.ok()?.path();
            let name = path.strip_prefix(root).ok()?.to_str()?.replace('\\', "/");
            let metadata = plain_metadata(&path)?;
            if metadata.is_dir() {
                if !directories.contains(&name) {
                    return None;
                }
                found_directories += 1;
                if found_directories > directories.len() {
                    return None;
                }
                pending.push(path);
            } else if metadata.is_file() {
                let digest = files.get(&name)?;
                plan.bytes = plan.bytes.checked_add(metadata.len())?;
                if plan.files.len() >= MAX_STAGE_FILES || plan.bytes > MAX_STAGE_BYTES {
                    return None;
                }
                verify_payload_file(&path, digest, metadata.len(), deadline, &mut buffer)?;
                plan.files.push(path);
                found_files += 1;
                if found_files > files.len() {
                    return None;
                }
            } else {
                return None;
            }
        }
    }
    (found_files == files.len() && found_directories == directories.len()).then_some(())
}

fn verified_stage(stage: &Path, deadline: Instant) -> Option<ReclaimPlan> {
    let marker_metadata = plain_metadata(&stage.join(READY))?;
    let lock_metadata = plain_metadata(&stage.join(LOCK))?;
    if !marker_metadata.is_file()
        || marker_metadata.len() > 4096
        || !lock_metadata.is_file()
        || lock_metadata.len() != LOCK_CONTENT.len() as u64
    {
        return None;
    }

    let expected = BTreeSet::from(
        [
            LOCK,
            READY,
            "runtime",
            "code",
            "runtime-inventory.json",
            "bootstrap-inventory.json",
        ]
        .map(str::to_owned),
    );
    let mut actual = BTreeSet::new();
    for entry in fs::read_dir(stage).ok()? {
        let name = entry.ok()?.file_name().to_str()?.to_owned();
        actual.insert(name);
        if actual.len() > expected.len() {
            return None;
        }
    }
    if actual != expected {
        return None;
    }

    let read_private = |name: &str, bound: u64| -> Option<Vec<u8>> {
        let path = stage.join(name);
        let metadata = plain_metadata(&path)?;
        if !metadata.is_file() || metadata.len() > bound || Instant::now() >= deadline {
            return None;
        }
        semwright_platform_services::verify_private_data_file(&path, bound).ok()?;
        fs::read(&path).ok()
    };

    // The caller holds the original stage lease. Re-reading its byte range
    // here would conflict with the lock on Windows.
    let marker_bytes = read_private(READY, 4096)?;
    let runtime_bytes = read_private("runtime-inventory.json", 2 * 1024 * 1024)?;
    let bootstrap_bytes = read_private("bootstrap-inventory.json", 8192)?;
    let marker: serde_json::Value = serde_json::from_slice(&marker_bytes).ok()?;
    let inventory: serde_json::Value = serde_json::from_slice(&runtime_bytes).ok()?;
    let runtime_digest = hex::encode(Sha256::digest(&runtime_bytes));
    let bootstrap_digest = hex::encode(Sha256::digest(&bootstrap_bytes));
    let runtime_files: BTreeMap<String, String> =
        serde_json::from_value(inventory.get("files")?.clone()).ok()?;
    let bootstrap_files: BTreeMap<String, String> =
        serde_json::from_slice(&bootstrap_bytes).ok()?;
    let executable_digest = runtime_files.get("blender.exe")?;
    let files_digest = hex::encode(Sha256::digest(serde_json::to_vec(&runtime_files).ok()?));
    if marker["format"].as_str() != Some(FORMAT)
        || marker["runtime_receipt_sha256"].as_str() != Some(runtime_digest.as_str())
        || marker["bootstrap_receipt_sha256"].as_str() != Some(bootstrap_digest.as_str())
        || inventory["schema"].as_str() != Some("semwright-private-blender-runtime/v1")
        || marker["executable_sha256"].as_str() != Some(executable_digest.as_str())
        || inventory["executable_sha256"].as_str() != Some(executable_digest.as_str())
        || inventory["inventory_sha256"].as_str() != Some(files_digest.as_str())
    {
        return None;
    }
    let runtime_dirs = expected_directories(&runtime_files)?;
    let bootstrap_dirs = expected_directories(&bootstrap_files)?;
    // The v1 receipt binds file paths, not names of empty directories.
    // Refuse such an ambiguous tree rather than guessing ownership.
    if inventory["entries"].as_u64()? != (runtime_files.len() + runtime_dirs.len()) as u64 {
        return None;
    }
    let mut plan = ReclaimPlan::default();
    verify_tree(
        &stage.join("runtime"),
        &runtime_files,
        &runtime_dirs,
        deadline,
        &mut plan,
    )?;
    if plan.bytes != inventory["bytes"].as_u64()? {
        return None;
    }
    verify_tree(
        &stage.join("code"),
        &bootstrap_files,
        &bootstrap_dirs,
        deadline,
        &mut plan,
    )?;
    Some(plan)
}

fn remove_verified_stage(stage: &Path, lock: File, plan: ReclaimPlan) -> std::io::Result<()> {
    // Withdraw readiness before any deletion and keep the lease throughout
    // payload removal. Never recursively delete an abandoned tree: a new
    // unlisted file causes remove_dir to fail instead of consuming it.
    fs::remove_file(stage.join(READY))?;
    for path in plan.files {
        fs::remove_file(path)?;
    }
    for path in plan.directories.into_iter().rev() {
        fs::remove_dir(path)?;
    }
    fs::remove_file(stage.join("runtime-inventory.json"))?;
    fs::remove_file(stage.join("bootstrap-inventory.json"))?;
    drop(lock);
    fs::remove_file(stage.join(LOCK))?;
    fs::remove_dir(stage)
}

fn native_stage_name(name: &str) -> bool {
    let Some(id) = name.strip_prefix(PREFIX) else {
        return false;
    };
    id.len() == 32 && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn collect_abandoned(namespace: &Path, min_age: Duration) -> Result<usize> {
    let mut reclaimed = 0usize;
    let entries = fs::read_dir(namespace)?;
    let deadline = Instant::now() + MAX_RECLAIM_INSPECTION;
    // Bound inspection across all candidates, not just each individual tree.
    for entry in entries.take(256) {
        if Instant::now() >= deadline {
            break;
        }
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !native_stage_name(&name) {
            continue;
        }
        let stage = entry.path();
        if !plain_metadata(&stage).is_some_and(|metadata| metadata.is_dir()) {
            continue;
        }
        let Some(ready) = plain_metadata(&stage.join(READY)) else {
            continue;
        };
        if !ready.is_file()
            || SystemTime::now()
                .duration_since(ready.modified().unwrap_or(SystemTime::now()))
                .unwrap_or_default()
                < min_age
        {
            continue;
        }
        // Unknown or unconfirmed native exits require human reconciliation.
        if stage.join(UNCONFIRMED).exists() {
            continue;
        }
        let lock_path = stage.join(LOCK);
        if !plain_metadata(&lock_path).is_some_and(|metadata| metadata.is_file())
            || semwright_platform_services::verify_private_data_file(&lock_path, 4096).is_err()
            || fs::read(&lock_path).ok().as_deref() != Some(LOCK_CONTENT)
        {
            continue;
        }
        let Ok(lock) = OpenOptions::new().read(true).write(true).open(&lock_path) else {
            continue;
        };
        if !matches!(
            semwright_platform_services::try_lock_session_file(&lock),
            Ok(true)
        ) {
            continue;
        }
        let Some(plan) = verified_stage(&stage, deadline) else {
            continue;
        };
        if Instant::now() >= deadline {
            break;
        }
        if remove_verified_stage(&stage, lock, plan).is_ok() {
            reclaimed += 1;
        }
        // Attempt deletion at most once, even if the operation was only
        // partially completed. A later start can inspect the next candidate.
        break;
    }
    Ok(reclaimed)
}

/// Called only after native owner grants and the exact Blender tool were
/// validated. Legacy unmarked state and other drivers are never scanned.
pub(super) fn new_stage(state: &Path) -> Result<Lease> {
    let namespace = state.join(NAMESPACE);
    semwright_platform_services::validate_private_data_directory(&namespace)?;
    semwright_platform_services::private_data_directory(&namespace)?;
    let reclaimed = collect_abandoned(&namespace, MIN_RECLAIM_AGE)?;
    if reclaimed != 0 {
        eprintln!(
            "{}",
            serde_json::json!({
                "component":"driver-host",
                "phase":"native_owner_stage_recovered",
                "stages":reclaimed,
            })
        );
    }
    let stage = namespace.join(format!("{PREFIX}{}", unique_id()));
    fs::create_dir(&stage)?;
    let mut lease = Lease {
        path: stage,
        lock: None,
        uncertain_exit: false,
    };
    semwright_platform_services::private_data_directory(lease.path())?;
    let lock_path = lease.path.join(LOCK);
    let mut lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&lock_path)?;
    lock.write_all(LOCK_CONTENT)?;
    lock.sync_all()?;
    semwright_platform_services::protect_owner_only_file(&lock_path)?;
    if !semwright_platform_services::try_lock_session_file(&lock)? {
        return Err(Error::new(
            ErrorCode::Conflict,
            "Native stage lease was already held",
        ));
    }
    lease.lock = Some(lock);
    Ok(lease)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_stage(namespace: &Path, suffix: &str) -> (PathBuf, File) {
        semwright_platform_services::private_data_directory(namespace).unwrap();
        let stage = namespace.join(format!("{PREFIX}{suffix}"));
        fs::create_dir(&stage).unwrap();
        semwright_platform_services::private_data_directory(&stage).unwrap();
        fs::create_dir(stage.join("runtime")).unwrap();
        fs::create_dir(stage.join("code")).unwrap();
        fs::create_dir(stage.join("runtime/deep")).unwrap();
        fs::create_dir(stage.join("code/sub")).unwrap();
        let runtime_files = BTreeMap::from([
            ("blender.exe".to_owned(), b"native".as_slice()),
            ("deep/asset.bin".to_owned(), b"asset".as_slice()),
        ]);
        let bootstrap_files = BTreeMap::from([
            ("first_party.py".to_owned(), b"print('ok')".as_slice()),
            ("sub/__init__.py".to_owned(), b"".as_slice()),
        ]);
        let mut pins = BTreeMap::new();
        let mut runtime_size = 0u64;
        for (name, bytes) in &runtime_files {
            fs::write(stage.join("runtime").join(name), bytes).unwrap();
            runtime_size += bytes.len() as u64;
            pins.insert(name.clone(), hex::encode(Sha256::digest(bytes)));
        }
        let executable_digest = pins["blender.exe"].clone();
        let files_digest = hex::encode(Sha256::digest(serde_json::to_vec(&pins).unwrap()));
        let runtime_receipt = serde_json::to_vec(&serde_json::json!({
            "schema": "semwright-private-blender-runtime/v1",
            "executable_sha256": executable_digest,
            "files": pins,
            "bytes": runtime_size,
            "entries": 3,
            "inventory_sha256": files_digest,
        }))
        .unwrap();
        let mut bootstrap_pins = BTreeMap::new();
        for (name, bytes) in &bootstrap_files {
            fs::write(stage.join("code").join(name), bytes).unwrap();
            bootstrap_pins.insert(name.clone(), hex::encode(Sha256::digest(bytes)));
        }
        let bootstrap_receipt = serde_json::to_vec(&bootstrap_pins).unwrap();
        fs::write(stage.join("runtime-inventory.json"), &runtime_receipt).unwrap();
        fs::write(stage.join("bootstrap-inventory.json"), &bootstrap_receipt).unwrap();
        let mark = serde_json::json!({
            "format": FORMAT,
            "runtime_receipt_sha256": hex::encode(Sha256::digest(&runtime_receipt)),
            "bootstrap_receipt_sha256": hex::encode(Sha256::digest(&bootstrap_receipt)),
            "executable_sha256": executable_digest,
        });
        fs::write(stage.join(READY), serde_json::to_vec(&mark).unwrap()).unwrap();
        let lock_path = stage.join(LOCK);
        let mut lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&lock_path)
            .unwrap();
        lock.write_all(LOCK_CONTENT).unwrap();
        lock.sync_all().unwrap();
        semwright_platform_services::protect_owner_only_file(&lock_path).unwrap();
        (stage, lock)
    }

    #[test]
    fn active_owner_lease_blocks_reclamation_but_released_complete_stage_reclaims() {
        let temp = tempfile::tempdir().unwrap();
        let namespace = temp.path().join(NAMESPACE);
        let (stage, lock) = test_stage(&namespace, "a".repeat(32).as_str());
        assert!(semwright_platform_services::try_lock_session_file(&lock).unwrap());
        assert_eq!(collect_abandoned(&namespace, Duration::ZERO).unwrap(), 0);
        assert!(stage.exists());
        drop(lock);
        assert_eq!(collect_abandoned(&namespace, Duration::ZERO).unwrap(), 1);
        assert!(!stage.exists());
    }

    #[test]
    fn missing_or_tampered_markers_and_unconfirmed_exit_never_reclaim() {
        let temp = tempfile::tempdir().unwrap();
        let namespace = temp.path().join(NAMESPACE);
        let (bad, lock) = test_stage(&namespace, "b".repeat(32).as_str());
        drop(lock);
        fs::write(bad.join(READY), b"not a signed source receipt").unwrap();
        let (uncertain, second_lock) = test_stage(&namespace, "c".repeat(32).as_str());
        drop(second_lock);
        fs::write(uncertain.join(UNCONFIRMED), b"needs reconciliation").unwrap();
        let unrelated = namespace.join("user-owned-not-native");
        fs::create_dir(&unrelated).unwrap();
        fs::write(unrelated.join("notes.txt"), b"keep").unwrap();
        assert_eq!(collect_abandoned(&namespace, Duration::ZERO).unwrap(), 0);
        assert!(bad.exists() && uncertain.exists());
        assert_eq!(
            fs::read(unrelated.join("notes.txt")).unwrap().as_slice(),
            b"keep"
        );
    }

    #[test]
    fn foreign_files_and_reparse_trees_are_never_deleted() {
        let temp = tempfile::tempdir().unwrap();
        let namespace = temp.path().join(NAMESPACE);
        let (stage, lock) = test_stage(&namespace, "d".repeat(32).as_str());
        drop(lock);
        fs::write(stage.join("unexpected-owner-data"), b"keep").unwrap();
        assert_eq!(collect_abandoned(&namespace, Duration::ZERO).unwrap(), 0);
        assert_eq!(
            fs::read(stage.join("unexpected-owner-data")).unwrap(),
            b"keep"
        );

        // Windows Developer Mode or elevated CI permits a symlink; without
        // that privilege the unexpected-file negative above still applies.
        fs::remove_file(stage.join("unexpected-owner-data")).unwrap();
        let outside = temp.path().join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("sentinel"), b"outside unchanged").unwrap();
        let link = stage.join("runtime/escape");
        if std::os::windows::fs::symlink_dir(&outside, &link).is_ok() {
            assert_eq!(collect_abandoned(&namespace, Duration::ZERO).unwrap(), 0);
            assert!(stage.exists());
            assert_eq!(
                fs::read(outside.join("sentinel")).unwrap(),
                b"outside unchanged"
            );
        }
    }

    #[test]
    fn nested_unlisted_files_and_directories_preserve_abandoned_stage() {
        let temp = tempfile::tempdir().unwrap();
        let namespace = temp.path().join(NAMESPACE);
        for (suffix, relative) in [
            ("e", "runtime/deep/owner-note.txt"),
            ("f", "code/sub/owner-note.txt"),
        ] {
            let (stage, lock) = test_stage(&namespace, &suffix.repeat(32));
            drop(lock);
            let path = stage.join(relative);
            fs::write(&path, b"owner data").unwrap();
            assert_eq!(collect_abandoned(&namespace, Duration::ZERO).unwrap(), 0);
            assert_eq!(fs::read(&path).unwrap().as_slice(), b"owner data");
            assert!(stage.join(READY).is_file());
            assert!(stage.join("runtime/blender.exe").is_file());
        }
        let (stage, lock) = test_stage(&namespace, &"1".repeat(32));
        drop(lock);
        fs::create_dir(stage.join("runtime/deep/unknown")).unwrap();
        assert_eq!(collect_abandoned(&namespace, Duration::ZERO).unwrap(), 0);
        assert!(stage.join("runtime/deep/unknown").is_dir());
        assert!(stage.join(READY).is_file());
    }

    #[test]
    fn missing_or_modified_inventory_files_preserve_abandoned_stage() {
        let temp = tempfile::tempdir().unwrap();
        let namespace = temp.path().join(NAMESPACE);
        let (runtime_changed, lock) = test_stage(&namespace, &"2".repeat(32));
        drop(lock);
        // Same size and path are not enough to prove content ownership.
        fs::write(runtime_changed.join("runtime/deep/asset.bin"), b"owner").unwrap();
        assert_eq!(collect_abandoned(&namespace, Duration::ZERO).unwrap(), 0);
        assert_eq!(
            fs::read(runtime_changed.join("runtime/deep/asset.bin"))
                .unwrap()
                .as_slice(),
            b"owner"
        );
        assert!(runtime_changed.join(READY).is_file());

        let (code_changed, lock) = test_stage(&namespace, &"3".repeat(32));
        drop(lock);
        fs::write(code_changed.join("code/first_party.py"), b"print('no')").unwrap();
        assert_eq!(collect_abandoned(&namespace, Duration::ZERO).unwrap(), 0);
        assert_eq!(
            fs::read(code_changed.join("code/first_party.py"))
                .unwrap()
                .as_slice(),
            b"print('no')"
        );
        assert!(code_changed.join(READY).is_file());

        let (missing, lock) = test_stage(&namespace, &"4".repeat(32));
        drop(lock);
        fs::remove_file(missing.join("runtime/deep/asset.bin")).unwrap();
        assert_eq!(collect_abandoned(&namespace, Duration::ZERO).unwrap(), 0);
        assert!(missing.join("runtime/blender.exe").is_file());
        assert!(missing.join(READY).is_file());
    }

    #[test]
    fn late_unlisted_file_is_not_removed_by_verified_plan() {
        let temp = tempfile::tempdir().unwrap();
        let namespace = temp.path().join(NAMESPACE);
        let (stage, lock) = test_stage(&namespace, &"5".repeat(32));
        assert!(semwright_platform_services::try_lock_session_file(&lock).unwrap());
        let plan = verified_stage(&stage, Instant::now() + Duration::from_secs(5)).unwrap();
        let foreign = stage.join("runtime/deep/late-owner.txt");
        fs::write(&foreign, b"keep even after verification").unwrap();
        assert!(remove_verified_stage(&stage, lock, plan).is_err());
        assert_eq!(
            fs::read(&foreign).unwrap().as_slice(),
            b"keep even after verification"
        );
        assert!(!stage.join(READY).exists());
        assert!(stage.join("runtime-inventory.json").is_file());
        assert_eq!(collect_abandoned(&namespace, Duration::ZERO).unwrap(), 0);
    }

    #[test]
    fn expired_verification_budget_cannot_authorize_removal() {
        let temp = tempfile::tempdir().unwrap();
        let namespace = temp.path().join(NAMESPACE);
        let (stage, lock) = test_stage(&namespace, &"6".repeat(32));
        assert!(semwright_platform_services::try_lock_session_file(&lock).unwrap());
        assert!(verified_stage(&stage, Instant::now()).is_none());
        assert!(stage.join(READY).is_file());
        assert_eq!(
            fs::read(stage.join("runtime/blender.exe"))
                .unwrap()
                .as_slice(),
            b"native"
        );
    }

    #[test]
    fn compact_stage_leaves_import_headroom_and_preserves_legacy_namespace() {
        use std::os::windows::ffi::OsStrExt;

        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().as_os_str().encode_wide().count();
        assert!(
            base < 103,
            "CI temporary directory exceeds this bounded fixture"
        );
        let state = temp.path().join("d".repeat(104 - base - 1));
        semwright_platform_services::private_data_directory(&state).unwrap();
        let legacy = state.join("blender-stage-v1");
        fs::create_dir(&legacy).unwrap();
        fs::write(legacy.join("owner-sentinel"), b"preserve old state").unwrap();
        let lease = new_stage(&state).unwrap();
        let module = lease.path().join(
            "runtime/5.2/scripts/addons_core/io_scene_gltf2/blender/exp/animation/sampled/shapekeys/channel_target.py",
        );
        assert!(module.as_os_str().encode_wide().count() < 260);
        assert!(lease.path().starts_with(&state));
        assert!(native_stage_name(
            lease.path().file_name().unwrap().to_str().unwrap()
        ));
        drop(lease);
        assert_eq!(
            fs::read(legacy.join("owner-sentinel")).unwrap(),
            b"preserve old state"
        );
    }

    #[test]
    fn new_stage_is_private_and_failed_stages_are_cleaned_on_drop() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path().join("drivers");
        semwright_platform_services::private_data_directory(&state).unwrap();
        let lease = new_stage(&state).unwrap();
        let path = lease.path().to_path_buf();
        assert!(path.join(LOCK).exists());
        assert!(!path.join(READY).exists());
        drop(lease);
        assert!(!path.exists());

        let mut uncertain = new_stage(&state).unwrap();
        let retained = uncertain.path().to_path_buf();
        uncertain.retain_unconfirmed_exit();
        drop(uncertain);
        assert!(retained.join(UNCONFIRMED).is_file());
        assert_eq!(
            collect_abandoned(&state.join(NAMESPACE), Duration::ZERO).unwrap(),
            0
        );
        assert!(
            retained.exists(),
            "Unconfirmed exit cannot authorize cleanup"
        );
    }
}
