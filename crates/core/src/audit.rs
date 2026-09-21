//! Append-only metadata journal with bounded rotation and a SHA-256 hash chain.
//! This detects accidental corruption, not a malicious process with the same Unix UID.
use semwright_protocol::{current_uid, private_directory};
use semwright_types::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Instant, SystemTime, UNIX_EPOCH},
};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub sequence: u64,
    pub unix_ms: u64,
    pub phase: String,
    pub command: String,
    pub request_id: String,
    pub session_tag: String,
    pub backend: String,
    pub decision: String,
    pub ok: Option<bool>,
    pub error_code: Option<ErrorCode>,
    pub outcome_known: bool,
    pub duration_ms: u64,
    pub previous: String,
    pub hash: String,
}
struct State {
    file: File,
    bytes: u64,
    sequence: u64,
    previous: String,
    tail: VecDeque<Record>,
}
pub struct Audit {
    path: PathBuf,
    max_bytes: u64,
    retention: usize,
    state: Mutex<State>,
}
fn open(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .append(true)
        .read(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file()
        || meta.uid() != current_uid()
        || meta.mode() & 0o777 != 0o600
        || meta.nlink() != 1
    {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Audit must be a single-link user-owned regular file with mode 0600",
        ));
    }
    Ok(file)
}
fn hash(record: &Record) -> Result<String> {
    let mut unsigned = record.clone();
    unsigned.hash.clear();
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&unsigned)?)
    ))
}
fn safe_id(text: &str) -> String {
    if text.len() == 32 && text.bytes().all(|b| b.is_ascii_hexdigit()) {
        text.into()
    } else {
        "invalid-id".into()
    }
}
fn tag(session: &str) -> String {
    format!("{:x}", Sha256::digest(session.as_bytes()))[..16].into()
}
impl Audit {
    pub fn open(directory: &Path, max_bytes: u64, retention: usize) -> Result<Arc<Self>> {
        private_directory(directory)?;
        if !(65536..=67_108_864).contains(&max_bytes) || !(1..=16).contains(&retention) {
            return Err(Error::invalid("Audit rotation limits are out of range"));
        }
        let path = directory.join("audit.jsonl");
        let file = open(&path)?;
        let meta = file.metadata()?;
        if meta.len() > max_bytes + 8192 {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Audit file exceeds the configured bound; inspect it offline",
            ));
        }
        let mut tail = VecDeque::new();
        let mut previous = String::new();
        let mut sequence = 0;
        let reader = BufReader::new(file.try_clone()?);
        for line in reader.lines() {
            let line = line?;
            if line.len() > 8192 {
                return Err(Error::new(ErrorCode::Conflict, "Audit line exceeds budget"));
            }
            let record: Record = serde_json::from_str(&line).map_err(|_| {
                Error::new(
                    ErrorCode::Conflict,
                    "Audit is corrupt; refusing to overwrite evidence",
                )
            })?;
            if hash(&record)? != record.hash
                || (sequence > 0
                    && (record.sequence != sequence + 1 || record.previous != previous))
            {
                return Err(Error::new(
                    ErrorCode::Conflict,
                    "Audit chain is corrupt; inspect before restarting",
                ));
            }
            sequence = record.sequence;
            previous = record.hash.clone();
            if tail.len() == 500 {
                tail.pop_front();
            }
            tail.push_back(record);
        }
        Ok(Arc::new(Self {
            path,
            max_bytes,
            retention,
            state: Mutex::new(State {
                file,
                bytes: meta.len(),
                sequence,
                previous,
                tail,
            }),
        }))
    }
    fn append(&self, mut record: Record) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "Audit lock poisoned"))?;
        if state.bytes >= self.max_bytes {
            state.file.sync_all()?;
            for index in (1..self.retention).rev() {
                let old = self.path.with_extension(format!("jsonl.{index}"));
                let new = self.path.with_extension(format!("jsonl.{}", index + 1));
                if old.exists() {
                    std::fs::rename(old, new)?;
                }
            }
            std::fs::rename(&self.path, self.path.with_extension("jsonl.1"))?;
            state.file = open(&self.path)?;
            state.bytes = 0;
        }
        record.sequence = state.sequence + 1;
        record.previous = state.previous.clone();
        record.hash = hash(&record)?;
        let mut line = serde_json::to_vec(&record)?;
        line.push(b'\n');
        state.file.write_all(&line)?;
        state.file.sync_data()?;
        state.bytes += line.len() as u64;
        state.sequence = record.sequence;
        state.previous = record.hash.clone();
        if state.tail.len() == 500 {
            state.tail.pop_front();
        }
        state.tail.push_back(record);
        Ok(())
    }
    pub fn tail(&self, limit: usize) -> Result<Vec<Record>> {
        let state = self
            .state
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "Audit lock poisoned"))?;
        Ok(state
            .tail
            .iter()
            .skip(state.tail.len().saturating_sub(limit.min(500)))
            .cloned()
            .collect())
    }
    pub fn begin(self: &Arc<Self>, command: &str, id: &str, session: &str) -> Result<Scope> {
        let record = Record {
            sequence: 0,
            unix_ms: now(),
            phase: "start".into(),
            command: command.into(),
            request_id: safe_id(id),
            session_tag: tag(session),
            backend: "unselected".into(),
            decision: "not_evaluated".into(),
            ok: None,
            error_code: None,
            outcome_known: true,
            duration_ms: 0,
            previous: String::new(),
            hash: String::new(),
        };
        self.append(record.clone())?;
        Ok(Scope {
            audit: self.clone(),
            record,
            start: Instant::now(),
            finished: false,
        })
    }
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
pub struct Scope {
    audit: Arc<Audit>,
    record: Record,
    start: Instant,
    finished: bool,
}
impl Scope {
    pub fn backend(&mut self, backend: &str) {
        self.record.backend = backend.into();
    }
    pub fn decision(&mut self, decision: &str) {
        self.record.decision = decision.into();
    }
    pub fn policy_decision(&self) -> &str {
        &self.record.decision
    }
    pub fn finish(&mut self, result: &Result<serde_json::Value>) -> Result<()> {
        self.record.phase = "finish".into();
        self.record.unix_ms = now();
        self.record.duration_ms = self.start.elapsed().as_millis().min(u64::MAX as u128) as u64;
        self.record.ok = Some(result.is_ok());
        self.record.error_code = result.as_ref().err().map(|e| e.code);
        self.record.outcome_known = result.as_ref().err().is_none_or(|e| e.outcome_known);
        self.record.decision = match self.record.error_code {
            Some(ErrorCode::PolicyDenied | ErrorCode::PermissionDenied) => "deny",
            Some(ErrorCode::ConsentRequired) => "require_confirmation",
            _ => self.record.decision.as_str(),
        }
        .into();
        self.audit.append(self.record.clone())?;
        self.finished = true;
        Ok(())
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        if !self.finished {
            self.record.phase = "abandoned".into();
            self.record.ok = Some(false);
            self.record.error_code = Some(ErrorCode::Cancelled);
            self.record.outcome_known = false;
            self.record.unix_ms = now();
            self.record.duration_ms = self.start.elapsed().as_millis().min(u64::MAX as u128) as u64;
            let _ = self.audit.append(self.record.clone());
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn journal_contains_no_arguments() {
        let d = tempfile::tempdir().unwrap();
        let a = Audit::open(d.path(), 65536, 2).unwrap();
        let mut s = a.begin("clipboard.write", &unique_id(), "session").unwrap();
        s.finish(&Ok(serde_json::json!({"text":"SECRET-TEST"})))
            .unwrap();
        let text = std::fs::read_to_string(d.path().join("audit.jsonl")).unwrap();
        assert!(!text.contains("SECRET-TEST"));
        assert_eq!(a.tail(9).unwrap().len(), 2);
    }
    #[test]
    fn restart_verifies_chain() {
        let d = tempfile::tempdir().unwrap();
        {
            let a = Audit::open(d.path(), 65536, 2).unwrap();
            let mut s = a.begin("doctor", &unique_id(), "s").unwrap();
            s.finish(&Ok(serde_json::json!({}))).unwrap();
        }
        let a = Audit::open(d.path(), 65536, 2).unwrap();
        assert_eq!(a.tail(1).unwrap()[0].sequence, 2);
    }
    #[test]
    fn abandoned_requests_are_recorded() {
        let d = tempfile::tempdir().unwrap();
        let a = Audit::open(d.path(), 65536, 2).unwrap();
        drop(a.begin("ui.invoke", &unique_id(), "s").unwrap());
        assert_eq!(a.tail(1).unwrap()[0].phase, "abandoned");
    }
    #[test]
    fn tampering_fails_closed() {
        let d = tempfile::tempdir().unwrap();
        {
            let a = Audit::open(d.path(), 65536, 2).unwrap();
            drop(a.begin("doctor", &unique_id(), "s").unwrap());
        }
        let p = d.path().join("audit.jsonl");
        let s = std::fs::read_to_string(&p)
            .unwrap()
            .replace("doctor", "edited");
        std::fs::write(p, s).unwrap();
        assert!(Audit::open(d.path(), 65536, 2).is_err());
    }
}
