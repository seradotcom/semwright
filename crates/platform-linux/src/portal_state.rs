//! Owner-private persistence for single-use XDG portal restore tokens.
use semwright_protocol::{current_uid, private_directory};
use semwright_types::{Error, ErrorCode, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs::OpenOptions,
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

const MAX_STATE_BYTES: u64 = 8192;
const MAX_TOKEN_BYTES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreRecord {
    pub version: u32,
    pub restore_token: String,
    pub persist_mode: u32,
    pub devices: u32,
    pub clipboard: bool,
}

impl RestoreRecord {
    pub fn validate(&self) -> Result<()> {
        if self.version != 1
            || !matches!(self.persist_mode, 1 | 2)
            || self.devices == 0
            || self.devices & !3 != 0
            || self.restore_token.is_empty()
            || self.restore_token.len() > MAX_TOKEN_BYTES
            || self.restore_token.chars().any(char::is_control)
        {
            return Err(Error::invalid("Invalid portal restore-token record"));
        }
        Ok(())
    }

    pub fn matches(&self, persist_mode: u32, devices: u32, clipboard: bool) -> bool {
        self.persist_mode == persist_mode && self.devices == devices && self.clipboard == clipboard
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RestoreStatus {
    Absent,
    Available,
    Invalid,
}

pub struct RestoreStore {
    directory: PathBuf,
    path: PathBuf,
}

impl RestoreStore {
    pub fn new(directory: PathBuf) -> Result<Self> {
        private_directory(&directory)?;
        Ok(Self {
            path: directory.join("remote-desktop-restore.json"),
            directory,
        })
    }

    fn sync_directory(&self) -> Result<()> {
        OpenOptions::new()
            .read(true)
            .open(&self.directory)?
            .sync_all()?;
        Ok(())
    }

    fn metadata_ok(path: &Path, metadata: &std::fs::Metadata) -> bool {
        metadata.is_file()
            && !metadata.file_type().is_symlink()
            && metadata.uid() == current_uid()
            && metadata.permissions().mode() & 0o777 == 0o600
            && metadata.nlink() == 1
            && metadata.len() <= MAX_STATE_BYTES
            && path.file_name().is_some()
    }

    pub fn load(&self) -> Result<Option<RestoreRecord>> {
        let metadata = match std::fs::symlink_metadata(&self.path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if !Self::metadata_ok(&self.path, &metadata) {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "Portal restore state must be an owner-only regular file",
            ));
        }
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&self.path)?;
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        file.take(MAX_STATE_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_STATE_BYTES {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Portal restore state exceeds its size budget",
            ));
        }
        let record: RestoreRecord = serde_json::from_slice(&bytes)
            .map_err(|_| Error::invalid("Portal restore state is malformed"))?;
        record.validate()?;
        Ok(Some(record))
    }

    pub fn status(&self) -> RestoreStatus {
        match self.load() {
            Ok(Some(_)) => RestoreStatus::Available,
            Ok(None) => RestoreStatus::Absent,
            Err(_) => RestoreStatus::Invalid,
        }
    }

    /// Removes a matching token before returning it. A crash after this point cannot reuse a
    /// portal token that may already have been invalidated by SelectDevices.
    pub fn take_matching(
        &self,
        persist_mode: u32,
        devices: u32,
        clipboard: bool,
    ) -> Result<Option<RestoreRecord>> {
        let Some(record) = self.load()? else {
            return Ok(None);
        };
        if !record.matches(persist_mode, devices, clipboard) {
            return Ok(None);
        }
        std::fs::remove_file(&self.path)?;
        self.sync_directory()?;
        Ok(Some(record))
    }

    pub fn save(&self, record: &RestoreRecord) -> Result<()> {
        record.validate()?;
        private_directory(&self.directory)?;
        let bytes = serde_json::to_vec(record)?;
        if bytes.len() as u64 > MAX_STATE_BYTES {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Portal restore state exceeds its size budget",
            ));
        }
        let temporary = self.directory.join(format!(
            ".remote-desktop-restore-{}.tmp",
            uuid::Uuid::new_v4().simple()
        ));
        let result = (|| -> Result<()> {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&temporary)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            std::fs::rename(&temporary, &self.path)?;
            self.sync_directory()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result
    }

    pub fn clear(&self) -> Result<bool> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => {
                self.sync_directory()?;
                Ok(true)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(token: &str) -> RestoreRecord {
        RestoreRecord {
            version: 1,
            restore_token: token.into(),
            persist_mode: 2,
            devices: 3,
            clipboard: true,
        }
    }

    #[test]
    fn token_is_owner_private_atomic_and_single_use() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("portal");
        let store = RestoreStore::new(directory.clone()).unwrap();
        let value = record("opaque-token");
        store.save(&value).unwrap();
        let metadata =
            std::fs::symlink_metadata(directory.join("remote-desktop-restore.json")).unwrap();
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        assert_eq!(store.status(), RestoreStatus::Available);
        assert_eq!(
            store.take_matching(2, 3, true).unwrap(),
            Some(value.clone())
        );
        assert_eq!(store.status(), RestoreStatus::Absent);
        assert_eq!(store.take_matching(2, 3, true).unwrap(), None);
    }

    #[test]
    fn mismatched_scope_does_not_consume_token() {
        let root = tempfile::tempdir().unwrap();
        let store = RestoreStore::new(root.path().join("portal")).unwrap();
        store.save(&record("token")).unwrap();
        assert_eq!(store.take_matching(2, 1, true).unwrap(), None);
        assert_eq!(store.status(), RestoreStatus::Available);
        assert!(store.take_matching(2, 3, true).unwrap().is_some());
    }

    #[test]
    fn malformed_or_overpermissive_state_fails_closed() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("portal");
        let store = RestoreStore::new(directory.clone()).unwrap();
        let path = directory.join("remote-desktop-restore.json");
        std::fs::write(&path, b"{}").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(store.status(), RestoreStatus::Invalid);
        std::fs::write(&path, serde_json::to_vec(&record("token")).unwrap()).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(store.load().is_err());
    }

    #[test]
    fn symlink_restore_state_is_rejected() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("portal");
        let store = RestoreStore::new(directory.clone()).unwrap();
        let target = root.path().join("target");
        std::fs::write(&target, serde_json::to_vec(&record("token")).unwrap()).unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
        symlink(&target, directory.join("remote-desktop-restore.json")).unwrap();
        assert!(store.load().is_err());
    }

    #[test]
    fn invalid_token_record_is_rejected() {
        let mut value = record("token");
        value.persist_mode = 0;
        assert!(value.validate().is_err());
        value.persist_mode = 2;
        value.devices = 8;
        assert!(value.validate().is_err());
        value.devices = 3;
        value.restore_token = "\n".into();
        assert!(value.validate().is_err());
    }
}
