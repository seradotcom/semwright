//! Append-only metadata journal with bounded rotation and a SHA-256 hash chain.
//! This detects accidental corruption, not a malicious process with the same Unix UID.
#[cfg(unix)]
use semwright_protocol::current_uid;
use semwright_protocol::private_directory;
use semwright_types::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::{
    collections::VecDeque,
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Write},
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_request_id: Option<String>,
    pub session_tag: String,
    pub backend: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<InvocationProvenance>,
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
#[cfg(unix)]
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
#[cfg(target_os = "windows")]
fn open(path: &Path) -> Result<File> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::invalid("Audit path requires a parent directory"))?;
    semwright_protocol::private_directory(parent)?;
    let file = OpenOptions::new()
        .append(true)
        .read(true)
        .create(true)
        .open(path)?;
    let meta = std::fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Windows audit must be a regular non-link file inside the private state directory",
        ));
    }
    Ok(file)
}
fn hash(record: &Record) -> Result<String> {
    let mut unsigned = record.clone();
    unsigned.hash.clear();
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(&unsigned)?)))
}
fn safe_id(text: &str) -> String {
    if text.len() == 32 && text.bytes().all(|b| b.is_ascii_hexdigit()) {
        text.into()
    } else {
        "invalid-id".into()
    }
}
fn tag(session: &str) -> String {
    hex::encode(Sha256::digest(session.as_bytes()))[..16].into()
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
        self.begin_with_provenance(command, id, session, None)
    }
    pub fn begin_with_provenance(
        self: &Arc<Self>,
        command: &str,
        id: &str,
        session: &str,
        provenance: Option<InvocationProvenance>,
    ) -> Result<Scope> {
        self.begin_with_parent_provenance(command, id, session, provenance, None)
    }
    pub fn begin_with_parent_provenance(
        self: &Arc<Self>,
        command: &str,
        id: &str,
        session: &str,
        provenance: Option<InvocationProvenance>,
        parent_request_id: Option<&str>,
    ) -> Result<Scope> {
        let record = Record {
            sequence: 0,
            unix_ms: now(),
            phase: "start".into(),
            command: command.into(),
            request_id: safe_id(id),
            parent_request_id: parent_request_id.map(safe_id),
            session_tag: tag(session),
            backend: "unselected".into(),
            provenance,
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
    pub fn executed_provider(&mut self, provider: &str, generation: Option<u64>) {
        if let Some(provenance) = self.record.provenance.as_mut() {
            provenance.execution_provider = Some(provider.into());
            provenance.provider_generation = generation;
        }
    }
    pub fn provenance(&self) -> Option<InvocationProvenance> {
        self.record.provenance.clone()
    }
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
        let a = Audit::open(&d.path().join("audit"), 65536, 2).unwrap();
        let mut s = a.begin("clipboard.write", &unique_id(), "session").unwrap();
        s.finish(&Ok(serde_json::json!({"text":"SECRET-TEST"})))
            .unwrap();
        let text = std::fs::read_to_string(d.path().join("audit/audit.jsonl")).unwrap();
        assert!(!text.contains("SECRET-TEST"));
        assert_eq!(a.tail(9).unwrap().len(), 2);
    }
    #[test]
    fn restart_verifies_chain() {
        let d = tempfile::tempdir().unwrap();
        {
            let a = Audit::open(&d.path().join("audit"), 65536, 2).unwrap();
            let mut s = a.begin("doctor", &unique_id(), "s").unwrap();
            s.finish(&Ok(serde_json::json!({}))).unwrap();
        }
        let a = Audit::open(&d.path().join("audit"), 65536, 2).unwrap();
        assert_eq!(a.tail(1).unwrap()[0].sequence, 2);
    }
    #[test]
    fn abandoned_requests_are_recorded() {
        let d = tempfile::tempdir().unwrap();
        let a = Audit::open(&d.path().join("audit"), 65536, 2).unwrap();
        drop(a.begin("ui.invoke", &unique_id(), "s").unwrap());
        assert_eq!(a.tail(1).unwrap()[0].phase, "abandoned");
    }
    #[test]
    fn tampering_fails_closed() {
        let d = tempfile::tempdir().unwrap();
        {
            let a = Audit::open(&d.path().join("audit"), 65536, 2).unwrap();
            drop(a.begin("doctor", &unique_id(), "s").unwrap());
        }
        let p = d.path().join("audit/audit.jsonl");
        let s = std::fs::read_to_string(&p)
            .unwrap()
            .replace("doctor", "edited");
        std::fs::write(p, s).unwrap();
        assert!(Audit::open(&d.path().join("audit"), 65536, 2).is_err());
    }
    #[test]
    fn legacy_parentless_row_reopens_and_new_parent_link_is_hash_chained() {
        // Fixed legacy schema bytes, independently hashed without the new optional field.
        const LEGACY: &str = concat!(
            r#"{"sequence":1,"unix_ms":1,"phase":"finish","command":"doctor","request_id":"11111111111111111111111111111111","session_tag":"0123456789abcdef","backend":"unselected","decision":"not_evaluated","ok":true,"error_code":null,"outcome_known":true,"duration_ms":0,"previous":"","hash":"5869866073cad869a451f0124d91a987040ab1cf55c6d2f0466eaf703546d8c5"}"#,
            "\n"
        );
        let d = tempfile::tempdir().unwrap();
        let directory = d.path().join("audit");
        private_directory(&directory).unwrap();
        let path = directory.join("audit.jsonl");
        {
            let mut file = open(&path).unwrap();
            file.write_all(LEGACY.as_bytes()).unwrap();
            file.sync_all().unwrap();
        }
        {
            let a = Audit::open(&directory, 65536, 2).unwrap();
            let rows = a.tail(8).unwrap();
            assert_eq!(rows.len(), 1);
            assert!(rows[0].parent_request_id.is_none());
            assert_eq!(
                rows[0].hash,
                "5869866073cad869a451f0124d91a987040ab1cf55c6d2f0466eaf703546d8c5"
            );
            let mut scope = a
                .begin_with_parent_provenance(
                    "driver.fixture.ping",
                    "22222222222222222222222222222222",
                    "test-session",
                    None,
                    Some("33333333333333333333333333333333"),
                )
                .unwrap();
            scope.finish(&Ok(serde_json::json!({}))).unwrap();
        }
        {
            let a = Audit::open(&directory, 65536, 2).unwrap();
            let rows = a.tail(8).unwrap();
            assert_eq!(rows.len(), 3);
            assert_eq!(rows[1].previous, rows[0].hash);
            assert_eq!(rows[2].previous, rows[1].hash);
            assert_eq!(
                rows[2].parent_request_id.as_deref(),
                Some("33333333333333333333333333333333")
            );
        }
        let bytes = std::fs::read_to_string(&path).unwrap();
        assert!(bytes.starts_with(LEGACY));
        assert_eq!(
            bytes
                .matches("\"parent_request_id\":\"33333333333333333333333333333333\"")
                .count(),
            2
        );
        let tampered = bytes.replace(
            "\"parent_request_id\":\"33333333333333333333333333333333\"",
            "\"parent_request_id\":\"44444444444444444444444444444444\"",
        );
        std::fs::write(&path, tampered).unwrap();
        assert!(matches!(
            Audit::open(&directory, 65536, 2),
            Err(error) if error.code == ErrorCode::Conflict
        ));
    }
    fn restore_audit_test_file(directory: &Path, name: &str, bytes: &[u8]) {
        private_directory(directory).unwrap();
        let path = directory.join(name);
        assert!(
            !path.exists(),
            "restore fixture must never replace an existing file"
        );
        let mut file = open(&path).unwrap();
        file.write_all(bytes).unwrap();
        file.sync_all().unwrap();
        drop(file);
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }

    fn canonical_audit_test_segment(directory: &Path, bytes: &[u8]) -> Vec<Record> {
        // Audit::open currently reads only its active filename. This disposable
        // byte-identical copy exercises that same canonical reader for a retained
        // segment; it is not a new reader or an automatic archive validation API.
        restore_audit_test_file(directory, "audit.jsonl", bytes);
        let audit = Audit::open(directory, 65536, 2).unwrap();
        let rows = audit.tail(500).unwrap();
        assert_eq!(
            rows.len(),
            bytes.iter().filter(|byte| **byte == b'\n').count()
        );
        drop(audit);
        assert_eq!(std::fs::read(directory.join("audit.jsonl")).unwrap(), bytes);
        rows
    }

    #[test]
    fn retained_parent_links_survive_real_rotation_restore_and_typed_validation() {
        const PARENT: &str = "11111111111111111111111111111111";
        const ABANDONED: &str = "22222222222222222222222222222222";
        const FINISHED: &str = "33333333333333333333333333333333";
        const APPENDED: &str = "44444444444444444444444444444444";
        let temporary = tempfile::tempdir().unwrap();
        let source = temporary.path().join("original");
        let audit = Audit::open(&source, 65536, 2).unwrap();
        drop(
            audit
                .begin_with_parent_provenance(
                    "driver.fixture.mutate",
                    ABANDONED,
                    "original-session",
                    None,
                    Some(PARENT),
                )
                .unwrap(),
        );
        let mut completed = audit
            .begin_with_parent_provenance(
                "driver.fixture.read",
                FINISHED,
                "original-session",
                None,
                Some(PARENT),
            )
            .unwrap();
        completed.finish(&Ok(serde_json::json!({}))).unwrap();
        drop(completed);

        // Reach the real minimum rotation budget with real ordinary scopes.
        // Do not edit State.bytes or manually fabricate a rotated filename.
        for number in 0..256_u32 {
            let mut ordinary = audit
                .begin(
                    "doctor",
                    &format!("{:032x}", number + 1000),
                    "original-session",
                )
                .unwrap();
            ordinary.finish(&Ok(serde_json::json!({}))).unwrap();
            drop(ordinary);
            if source.join("audit.jsonl.1").exists() {
                break;
            }
        }
        drop(audit);
        assert!(source.join("audit.jsonl.1").exists());
        assert!(!source.join("audit.jsonl.2").exists());
        let active_bytes = std::fs::read(source.join("audit.jsonl")).unwrap();
        let retained_bytes = std::fs::read(source.join("audit.jsonl.1")).unwrap();
        assert!(retained_bytes.len() >= 65536);
        assert!(!active_bytes.is_empty());
        let retained = canonical_audit_test_segment(
            &temporary.path().join("compatible-retained-reader"),
            &retained_bytes,
        );
        assert_eq!(retained[0].sequence, 1);
        for (index, child, phase, known, ok) in [
            (0, ABANDONED, "start", true, None),
            (1, ABANDONED, "abandoned", false, Some(false)),
            (2, FINISHED, "start", true, None),
            (3, FINISHED, "finish", true, Some(true)),
        ] {
            let row = &retained[index];
            assert_eq!(row.request_id, child);
            assert_eq!(row.parent_request_id.as_deref(), Some(PARENT));
            assert_eq!(row.phase, phase);
            assert_eq!(row.outcome_known, known);
            assert_eq!(row.ok, ok);
        }
        assert_eq!(retained[1].error_code, Some(ErrorCode::Cancelled));

        // Restore the complete retained inventory, not a new-state rollback.
        let restored = temporary.path().join("restored-original-state");
        restore_audit_test_file(&restored, "audit.jsonl.1", &retained_bytes);
        restore_audit_test_file(&restored, "audit.jsonl", &active_bytes);
        let reopened = Audit::open(&restored, 65536, 2).unwrap();
        let active = reopened.tail(500).unwrap();
        assert!(active.iter().all(|row| row.parent_request_id.is_none()));
        assert_eq!(active[0].previous, retained.last().unwrap().hash);
        assert_eq!(active[0].sequence, retained.last().unwrap().sequence + 1);
        let previous = active.last().unwrap().clone();
        let mut appended = reopened
            .begin("doctor", APPENDED, "restored-session")
            .unwrap();
        appended.finish(&Ok(serde_json::json!({}))).unwrap();
        drop(appended);
        drop(reopened);
        let reopened = Audit::open(&restored, 65536, 2).unwrap();
        let rows = reopened.tail(2).unwrap();
        assert_eq!(rows[0].sequence, previous.sequence + 1);
        assert_eq!(rows[0].previous, previous.hash);
        assert_eq!(rows[1].sequence, rows[0].sequence + 1);
        assert_eq!(rows[1].previous, rows[0].hash);
        assert_eq!(rows[1].request_id, APPENDED);
        assert_eq!(rows[1].phase, "finish");
        drop(reopened);
        assert!(
            std::fs::read(restored.join("audit.jsonl"))
                .unwrap()
                .starts_with(&active_bytes)
        );
        assert_eq!(
            std::fs::read(restored.join("audit.jsonl.1")).unwrap(),
            retained_bytes
        );

        let original_text = std::str::from_utf8(&retained_bytes).unwrap();
        let field = format!(",\"parent_request_id\":\"{PARENT}\"");
        assert_eq!(original_text.matches(field.as_str()).count(), 4);
        for (case, replacement) in [
            (
                "changed-parent",
                ",\"parent_request_id\":\"55555555555555555555555555555555\"",
            ),
            ("removed-parent", ""),
        ] {
            // Only the parent field changes. The original stored hashes, sequence
            // and previous fields are deliberately retained, never repaired.
            let tampered = original_text.replacen(field.as_str(), replacement, 1);
            assert!(tampered.contains(format!("\"hash\":\"{}\"", retained[0].hash).as_str()));
            let case_directory = temporary.path().join(case);
            restore_audit_test_file(&case_directory, "audit.jsonl.1", tampered.as_bytes());
            restore_audit_test_file(&case_directory, "audit.jsonl", &active_bytes);

            // Opening only the active file does not validate retained history.
            drop(Audit::open(&case_directory, 65536, 2).unwrap());
            let checked_segment = case_directory.join("retained-segment-reader");
            restore_audit_test_file(&checked_segment, "audit.jsonl", tampered.as_bytes());
            assert!(matches!(
                Audit::open(&checked_segment, 65536, 2),
                Err(error) if error.code == ErrorCode::Conflict
            ));
            assert_eq!(
                std::fs::read(checked_segment.join("audit.jsonl")).unwrap(),
                tampered.as_bytes()
            );
            assert_eq!(
                std::fs::read(case_directory.join("audit.jsonl.1")).unwrap(),
                tampered.as_bytes()
            );
        }
        assert_eq!(
            std::fs::read(source.join("audit.jsonl")).unwrap(),
            active_bytes
        );
        assert_eq!(
            std::fs::read(source.join("audit.jsonl.1")).unwrap(),
            retained_bytes
        );
    }

    #[cfg(unix)]
    #[test]
    fn shared_existing_directory_is_rejected() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            matches!(Audit::open(directory.path(), 65536, 2), Err(error) if error.code == ErrorCode::PermissionDenied)
        );
    }
}
