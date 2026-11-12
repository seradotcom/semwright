//! Owner-private crash leftovers for the closed Windows Blender native profile.
//!
//! Only a completed, source-bound staging tree can be reclaimed. Active Host
//! instances hold an OS byte-range lock; unconfirmed native exits are retained.
//! This is not a general user-cache sweeper or a recovery grant.

use semwright_types::{Error, ErrorCode, Result, unique_id};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::Write,
    os::windows::fs::MetadataExt,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

const NAMESPACE: &str = "blender-stage-v1";
const PREFIX: &str = "blender-native-";
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

fn source_manifests_match(stage: &Path) -> bool {
    let Some(marker_metadata) = plain_metadata(&stage.join(READY)) else {
        return false;
    };
    if !marker_metadata.is_file() || marker_metadata.len() > 4096 {
        return false;
    }
    let Some(lock_metadata) = plain_metadata(&stage.join(LOCK)) else {
        return false;
    };
    if !lock_metadata.is_file() || lock_metadata.len() != LOCK_CONTENT.len() as u64 {
        return false;
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
    let Ok(entries) = fs::read_dir(stage) else {
        return false;
    };
    let mut actual = BTreeSet::new();
    for entry in entries {
        let Ok(entry) = entry else {
            return false;
        };
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            return false;
        };
        actual.insert(name);
        if actual.len() > expected.len() {
            return false;
        }
    }
    if actual != expected {
        return false;
    }

    let read_private = |name: &str, bound: u64| -> Option<Vec<u8>> {
        let path = stage.join(name);
        let metadata = plain_metadata(&path)?;
        if !metadata.is_file() || metadata.len() > bound {
            return None;
        }
        semwright_platform_services::verify_private_data_file(&path, bound).ok()?;
        fs::read(&path).ok()
    };

    // The caller already verified the nonempty lease file before acquiring
    // its exclusive byte-range lock. Re-reading it here would conflict with
    // that very lock on Windows.
    let Some(marker_bytes) = read_private(READY, 4096) else {
        return false;
    };
    let Some(runtime_bytes) = read_private("runtime-inventory.json", 2 * 1024 * 1024) else {
        return false;
    };
    let Some(bootstrap_bytes) = read_private("bootstrap-inventory.json", 8192) else {
        return false;
    };
    let Ok(marker) = serde_json::from_slice::<serde_json::Value>(&marker_bytes) else {
        return false;
    };
    let Ok(inventory) = serde_json::from_slice::<serde_json::Value>(&runtime_bytes) else {
        return false;
    };
    let runtime_digest = hex::encode(Sha256::digest(&runtime_bytes));
    let bootstrap_digest = hex::encode(Sha256::digest(&bootstrap_bytes));
    if marker["format"].as_str() != Some(FORMAT)
        || marker["runtime_receipt_sha256"].as_str() != Some(runtime_digest.as_str())
        || marker["bootstrap_receipt_sha256"].as_str() != Some(bootstrap_digest.as_str())
        || marker["executable_sha256"].as_str() != inventory["executable_sha256"].as_str()
        || inventory["inventory_sha256"].as_str().is_none()
    {
        return false;
    }
    // A snapshot may contain many vendor files. Check every directory and file
    // before unlinking any. Never traverse junctions, symlinks, or other reparse
    // points and never interpret an unknown root entry as disposable.
    let mut pending = vec![stage.join("runtime"), stage.join("code")];
    let mut dirs = 0usize;
    let mut files = 0usize;
    let mut total = 0u64;
    while let Some(directory) = pending.pop() {
        let Some(metadata) = plain_metadata(&directory) else {
            return false;
        };
        if !metadata.is_dir() {
            return false;
        }
        dirs += 1;
        if dirs > MAX_STAGE_DIRECTORIES {
            return false;
        }
        let Ok(children) = fs::read_dir(&directory) else {
            return false;
        };
        for item in children {
            let Ok(item) = item else {
                return false;
            };
            let path = item.path();
            let Some(metadata) = plain_metadata(&path) else {
                return false;
            };
            if metadata.is_dir() {
                pending.push(path);
            } else if metadata.is_file() {
                files += 1;
                let Some(next) = total.checked_add(metadata.len()) else {
                    return false;
                };
                total = next;
                if files > MAX_STAGE_FILES || total > MAX_STAGE_BYTES {
                    return false;
                }
            } else {
                return false;
            }
        }
    }
    true
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
    // Bound startup work even when the owner has many unrelated directories.
    for entry in entries.take(256) {
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
        if !source_manifests_match(&stage) {
            continue;
        }
        // The original Host cannot retake this unique lock: its process has
        // exited. Close our handle before removing the private stage directory.
        drop(lock);
        if fs::remove_dir_all(&stage).is_ok() {
            reclaimed += 1;
            // A single source-bound runtime can approach a gigabyte. Do not
            // consume the entire 180-second native bootstrap budget sweeping
            // a backlog: the next authorized start can recover the next one.
            break;
        }
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
        fs::write(stage.join("runtime/example"), b"native").unwrap();
        fs::write(stage.join("code/first_party.py"), b"print('ok')").unwrap();
        let runtime_receipt = br#"{"executable_sha256":"abc","inventory_sha256":"digest"}"#;
        let bootstrap_receipt = br#"{"first_party.py":"digest"}"#;
        fs::write(stage.join("runtime-inventory.json"), runtime_receipt).unwrap();
        fs::write(stage.join("bootstrap-inventory.json"), bootstrap_receipt).unwrap();
        let mark = serde_json::json!({
            "format": FORMAT,
            "runtime_receipt_sha256": hex::encode(Sha256::digest(runtime_receipt)),
            "bootstrap_receipt_sha256": hex::encode(Sha256::digest(bootstrap_receipt)),
            "executable_sha256": "abc",
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
