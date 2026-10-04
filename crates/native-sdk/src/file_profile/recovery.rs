//! Historical receipt lookup. A recorded result does not grant current authority.
use super::*;
use std::collections::BTreeSet;
const JOURNAL_SCHEMA: &str = "semwright-native-operation/1";
const MAX_JOURNAL_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OperationBinding {
    pub source_resource_id: String,
    pub operation: String,
    pub operation_key: String,
    pub request_digest: String,
    pub workspace_id: Option<String>,
    pub fork_workspace_id: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OperationRecord {
    schema_version: String,
    binding: OperationBinding,
    state: String,
    receipt: Option<Receipt>,
}
pub(super) struct History {
    pub reserved: bool,
    pub receipt: Option<Receipt>,
}

/// Calculate the digest of an unchanged mutation envelope. Preserve all fields,
/// including the original generation, revision, operation key, and workspace.
/// `operation_get` calculates this digest from the supplied original request.
pub fn request_digest(operation: &str, args: &Value) -> Result<String> {
    if !bounded_token(operation) || !args.is_object() {
        return Err(Error::invalid(
            "An operation and its original request object are required",
        ));
    }
    let bytes = serde_json::to_vec(&json!({"operation":operation,"args":args}))?;
    if bytes.len() as u64 > MAX_DOCUMENT {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Original request exceeds the byte limit",
        ));
    }
    Ok(sha256(&bytes))
}

pub(super) fn input_schema() -> Value {
    json!({"type":"object","properties":{"operation":{"type":"string","pattern":"^[A-Za-z0-9_-]{1,96}$"},"operation_key":{"type":"string","pattern":"^[A-Za-z0-9_-]{1,96}$"},"request":{"type":"object"},"workspace_id":{"type":"string","pattern":"^[A-Za-z0-9_-]{1,96}$"}},"required":["operation","operation_key","request"],"additionalProperties":false})
}

pub(super) fn output_schema() -> Value {
    json!({"type":"object","properties":{"schema_version":{"const":NATIVE_SCHEMA},"state":{"enum":["RECORDED","ABSENT_OUTCOME_UNKNOWN"]},"source_resource_id":{"type":"string"},"operation":{"type":"string"},"operation_key":{"type":"string"},"request_digest":{"type":"string","pattern":"^[0-9a-f]{64}$"},"workspace_id":{"type":["string","null"]},"result":{"type":["object","null"]},"error":{"type":["object","null"]},"historical_only":{"const":true},"current_authority":{"const":false},"replay_allowed":{"const":false}},"required":["schema_version","state","source_resource_id","operation","operation_key","request_digest","workspace_id","result","error","historical_only","current_authority","replay_allowed"],"additionalProperties":false})
}

impl<M: Model> NativeApp<M> {
    /// Read an exact historical result. This method does not execute the request.
    /// An absent receipt means that the outcome is unknown. It does not prove that
    /// the operation had no effect. Use a new observation for current state.
    pub fn operation_get(&self, args: &Value) -> Result<Value> {
        known_fields(
            args,
            &[
                "operation",
                "operation_key",
                "request",
                "workspace_id",
                "ref",
            ],
        )?;
        validate_optional_ref(args)?;
        if self.workspace_id.is_none() {
            if let Some(id) = args.get("workspace_id") {
                return self
                    .workspace(
                        id.as_str()
                            .ok_or_else(|| Error::invalid("Invalid workspace id"))?,
                    )?
                    .operation_get(args);
            }
        }
        if args.get("workspace_id").and_then(Value::as_str) != self.workspace_id.as_deref() {
            return Err(Error::invalid(
                "Lookup workspace differs from selected document",
            ));
        }
        let operation = text(args, "operation")?;
        let key = text(args, "operation_key")?;
        let request = args
            .get("request")
            .ok_or_else(|| Error::invalid("Original request required"))?;
        known_fields(
            request,
            &[
                "expected_revision",
                "expected_generation",
                "operation_key",
                "parameters",
                "wait_ms",
                "workspace_id",
                "ref",
            ],
        )?;
        validate_optional_ref(request)?;
        if !bounded_token(key)
            || text(request, "operation_key")? != key
            || request.get("workspace_id").and_then(Value::as_str) != self.workspace_id.as_deref()
        {
            return Err(Error::invalid("Original request key or workspace differs"));
        }
        text(request, "expected_revision")?;
        text(request, "expected_generation")?;
        if !request.get("parameters").is_some_and(Value::is_object) {
            return Err(Error::invalid("Original parameters required"));
        }
        let _lock = self.lock()?;
        let (doc, _) = self.read()?;
        let binding = OperationBinding {
            source_resource_id: doc.resource_id.clone(),
            operation: operation.into(),
            operation_key: key.into(),
            request_digest: request_digest(operation, request)?,
            workspace_id: self.workspace_id.clone(),
            fork_workspace_id: if operation == "fork" {
                Some(text(&request["parameters"], "workspace_id")?.into())
            } else {
                None
            },
        };
        let history = self.operation_history(&doc, &binding)?;
        Ok(
            json!({"schema_version":NATIVE_SCHEMA,"state":if history.receipt.is_some(){"RECORDED"}else{"ABSENT_OUTCOME_UNKNOWN"},"source_resource_id":binding.source_resource_id,"operation":operation,"operation_key":key,"request_digest":binding.request_digest,"workspace_id":binding.workspace_id,"result":history.receipt.as_ref().map(|r|r.result.clone()),"error":history.receipt.as_ref().and_then(|r|r.error.clone()),"historical_only":true,"current_authority":false,"replay_allowed":false}),
        )
    }
    fn journal_records(&self) -> Result<Vec<OperationRecord>> {
        let path = self.root.join("operations");
        if !path.try_exists()? {
            return Ok(vec![]);
        }
        let meta = fs::symlink_metadata(&path)?;
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return Err(Error::invalid("Invalid operation journal"));
        }
        let mut records = vec![];
        let mut total = 0u64;
        for entry in fs::read_dir(path)? {
            if records.len() >= MAX_RECEIPTS {
                return Err(Error::new(
                    ErrorCode::ResourceExhausted,
                    "Operation journal exceeds retention limit",
                ));
            }
            let entry = entry?;
            let name = entry.file_name();
            let name = name
                .to_str()
                .ok_or_else(|| Error::invalid("Invalid operation journal entry"))?;
            // Interrupted atomic writes are not receipts. They still count toward the byte cap.
            let bytes = read_bounded(&entry.path())?;
            total = total
                .checked_add(bytes.len() as u64)
                .ok_or_else(|| Error::invalid("Journal size overflow"))?;
            if total > MAX_JOURNAL_BYTES {
                return Err(Error::new(
                    ErrorCode::ResourceExhausted,
                    "Operation journal exceeds byte limit",
                ));
            }
            if name.starts_with(".pending-") {
                return Err(Error::new(
                    ErrorCode::Conflict,
                    "Interrupted operation journal requires owner review",
                )
                .uncertain());
            }
            let record: OperationRecord = serde_json::from_slice(&bytes)?;
            if record.schema_version != JOURNAL_SCHEMA
                || name != format!("{}.json", record.binding.operation_key)
                || !bounded_token(&record.binding.operation_key)
                || !["INTENT", "RECORDED"].contains(&record.state.as_str())
                || (record.state == "RECORDED") != record.receipt.is_some()
            {
                return Err(Error::invalid("Invalid operation journal record"));
            }
            records.push(record);
        }
        Ok(records)
    }
    fn legacy_origins(&self) -> Result<Vec<Receipt>> {
        let mut origins = vec![];
        let mut count = 0;
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let name = entry.file_name();
            if !name.to_string_lossy().starts_with("workspace-") {
                continue;
            }
            count += 1;
            if count > MAX_RECEIPTS {
                return Err(Error::new(
                    ErrorCode::ResourceExhausted,
                    "Workspace count exceeds retention limit",
                ));
            }
            let meta = fs::symlink_metadata(entry.path())?;
            if !meta.is_dir() || meta.file_type().is_symlink() {
                return Err(Error::invalid("Invalid workspace history"));
            }
            let origin = entry.path().join("origin.json");
            if !origin.try_exists()? {
                // New partial forks have a reserved key. Legacy partial forks have no usable key.
                let known = self.journal_records()?.iter().any(|r| {
                    r.binding.operation == "fork"
                        && r.binding.fork_workspace_id.as_deref()
                            == Some(name.to_string_lossy().trim_start_matches("workspace-"))
                });
                if !known {
                    return Err(Error::new(
                        ErrorCode::Conflict,
                        "Workspace origin is absent; preserve unknown history",
                    )
                    .uncertain());
                }
                continue;
            }
            let r: Receipt = serde_json::from_slice(&read_bounded(&origin)?)?;
            if !r
                .result
                .get("operation_key")
                .and_then(Value::as_str)
                .is_some_and(bounded_token)
            {
                return Err(Error::new(
                    ErrorCode::Conflict,
                    "Legacy fork key is unknown; owner review required",
                )
                .uncertain());
            }
            origins.push(r);
        }
        Ok(origins)
    }
    pub(super) fn operation_history(
        &self,
        doc: &Document,
        binding: &OperationBinding,
    ) -> Result<History> {
        let mut reserved = false;
        let mut found: Option<Receipt> = None;
        let mut accept = |receipt: &Receipt| -> Result<()> {
            reserved = true;
            if receipt.request_digest != binding.request_digest {
                return Err(Error::new(
                    ErrorCode::Conflict,
                    "Operation key was reused with another request",
                ));
            }
            if let Some(error) = &receipt.error {
                if error.code != ErrorCode::Cancelled
                    || !error.outcome_known
                    || !receipt.result.is_null()
                {
                    return Err(Error::invalid("Invalid recorded cancellation"));
                }
            }
            if let Some(actual) = &receipt.binding {
                if actual != binding
                    || (receipt.error.is_none()
                        && (receipt.result["operation_key"] != binding.operation_key
                            || receipt.result["operation"] != binding.operation))
                {
                    return Err(Error::new(
                        ErrorCode::Conflict,
                        "Historical operation binding differs",
                    ));
                }
                if let Some(previous) = &found {
                    if previous.result != receipt.result
                        || serde_json::to_value(&previous.error)?
                            != serde_json::to_value(&receipt.error)?
                    {
                        return Err(
                            Error::new(ErrorCode::Conflict, "Historical results conflict")
                                .uncertain(),
                        );
                    }
                }
                found = Some(receipt.clone());
            }
            Ok(())
        };
        if let Some(receipt) = doc.receipts.get(&binding.operation_key) {
            accept(receipt)?;
        }
        let records = self.journal_records()?;
        for record in &records {
            if record.binding.operation_key == binding.operation_key {
                if &record.binding != binding {
                    return Err(Error::new(
                        ErrorCode::Conflict,
                        "Operation key was reused with another binding",
                    ));
                }
                if let Some(receipt) = &record.receipt {
                    accept(receipt)?;
                }
            }
        }
        for origin in self.legacy_origins()? {
            if origin.result["operation_key"] == binding.operation_key {
                accept(&origin)?;
            }
        }
        drop(accept);
        reserved |= records
            .iter()
            .any(|r| r.binding.operation_key == binding.operation_key);
        Ok(History {
            reserved,
            receipt: found,
        })
    }
    pub(super) fn check_operation_capacity(
        &self,
        doc: &Document,
        binding: &OperationBinding,
        receipt: &Receipt,
    ) -> Result<()> {
        let records = self.journal_records()?;
        let mut keys: BTreeSet<String> = doc.receipts.keys().cloned().collect();
        let mut bytes = 0u64;
        for record in records {
            keys.insert(record.binding.operation_key.clone());
            bytes += serde_json::to_vec(&record)?.len() as u64;
        }
        for origin in self.legacy_origins()? {
            keys.insert(text(&origin.result, "operation_key")?.into());
        }
        keys.insert(binding.operation_key.clone());
        bytes += serde_json::to_vec(&OperationRecord {
            schema_version: JOURNAL_SCHEMA.into(),
            binding: binding.clone(),
            state: "RECORDED".into(),
            receipt: Some(receipt.clone()),
        })?
        .len() as u64;
        if keys.len() > MAX_RECEIPTS || bytes > MAX_JOURNAL_BYTES {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Operation retention reached; preserve this document and its unresolved history",
            ));
        }
        Ok(())
    }
    pub(super) fn reserve_operation(&self, binding: &OperationBinding) -> Result<()> {
        let dir = self.root.join("operations");
        if !dir.try_exists()? {
            fs::create_dir(&dir)?;
            sync_dir(&dir).map_err(|e| e.uncertain())?;
            sync_dir(&self.root).map_err(|e| e.uncertain())?;
        }
        let path = dir.join(format!("{}.json", binding.operation_key));
        let mut f = OpenOptions::new().write(true).create_new(true).open(path)?;
        let record = OperationRecord {
            schema_version: JOURNAL_SCHEMA.into(),
            binding: binding.clone(),
            state: "INTENT".into(),
            receipt: None,
        };
        (|| {
            f.write_all(&serde_json::to_vec(&record)?)?;
            f.sync_all()?;
            sync_dir(&dir)?;
            Ok(())
        })()
        .map_err(|e: Error| e.uncertain())
    }
    pub(super) fn cancel_reserved(&self, binding: &OperationBinding) -> Result<Value> {
        let error = Error::new(
            ErrorCode::Cancelled,
            "Cancelled after reservation and before SDK publication",
        );
        let receipt = Receipt {
            request_digest: binding.request_digest.clone(),
            result: Value::Null,
            binding: Some(binding.clone()),
            error: Some(error.clone()),
        };
        fault("cancel-before-record")?;
        self.record_operation(binding, &receipt)
            .map_err(|e| e.uncertain())?;
        Err(error)
    }
    pub(super) fn record_operation(
        &self,
        binding: &OperationBinding,
        receipt: &Receipt,
    ) -> Result<()> {
        let record = OperationRecord {
            schema_version: JOURNAL_SCHEMA.into(),
            binding: binding.clone(),
            state: "RECORDED".into(),
            receipt: Some(receipt.clone()),
        };
        write_atomic(
            &self.root.join("operations"),
            &format!("{}.json", binding.operation_key),
            &serde_json::to_vec(&record)?,
        )
    }
}
#[cfg(test)]
thread_local! {static FAULT:std::cell::Cell<Option<&'static str>>=const{std::cell::Cell::new(None)};}
pub(super) fn fault(_point: &str) -> Result<()> {
    #[cfg(test)]
    if FAULT.with(|v| v.get() == Some(_point)) {
        return Err(Error::new(
            ErrorCode::BackendFailed,
            "Injected storage boundary failure",
        )
        .uncertain());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Clone)]
    struct Example;
    impl Model for Example {
        fn id(&self) -> &'static str {
            "recovery-example"
        }
        fn initial(&self) -> Value {
            json!({"value":1})
        }
        fn validate(&self, v: &Value) -> Result<()> {
            if v["value"].as_i64().is_none() {
                return Err(Error::invalid("Integer required"));
            }
            Ok(())
        }
        fn operations(&self) -> Vec<Operation> {
            vec![Operation {
                name: "set",
                description: "Set value",
                input_schema: json!({"type":"object"}),
            }]
        }
        fn apply(&self, _: &Value, op: &str, p: &Value) -> Result<Value> {
            if op != "set" {
                return Err(Error::new(ErrorCode::PermissionDenied, "Unknown operation"));
            }
            self.validate(p)?;
            Ok(p.clone())
        }
    }
    fn request(app: &NativeApp<Example>, key: &str, parameters: Value) -> Value {
        let v = app.inspect().unwrap();
        json!({"expected_revision":v["revision"],"expected_generation":v["generation"],"operation_key":key,"parameters":parameters})
    }
    fn lookup(op: &str, r: &Value) -> Value {
        json!({"operation":op,"operation_key":r["operation_key"],"request":r})
    }
    fn set_fault(point: Option<&'static str>) {
        FAULT.with(|v| v.set(point));
    }
    #[test]
    fn committed_mutation_recovers_after_reopen_without_generation_renewal() {
        let t = tempfile::tempdir().unwrap();
        let app = NativeApp::create(t.path(), Example).unwrap();
        let r = request(&app, "one", json!({"value":2}));
        let result = app.apply("set", &r, || false).unwrap();
        let app = NativeApp::open(t.path(), Example).unwrap();
        let bytes = fs::read(t.path().join("document.json")).unwrap();
        assert_eq!(
            app.apply("set", &r, || false).unwrap_err().code,
            ErrorCode::StaleReference
        );
        let history = app.operation_get(&lookup("set", &r)).unwrap();
        assert_eq!(history["state"], "RECORDED");
        assert_eq!(history["result"], result);
        assert_eq!(history["current_authority"], false);
        assert_eq!(history["replay_allowed"], false);
        assert_ne!(
            history["result"]["generation"],
            app.inspect().unwrap()["generation"]
        );
        assert_eq!(bytes, fs::read(t.path().join("document.json")).unwrap());
        let mut altered = r.clone();
        altered["parameters"]["value"] = json!(3);
        assert_eq!(
            app.operation_get(&lookup("set", &altered))
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        let missing = request(&app, "missing", json!({"value":2}));
        assert_eq!(
            app.operation_get(&lookup("set", &missing)).unwrap()["state"],
            "ABSENT_OUTCOME_UNKNOWN"
        );
    }
    #[test]
    fn committed_document_survives_failed_final_journal_and_is_not_replayed() {
        let t = tempfile::tempdir().unwrap();
        let app = NativeApp::create(t.path(), Example).unwrap();
        let r = request(&app, "late", json!({"value":2}));
        set_fault(Some("document-before-journal"));
        assert!(!app.apply("set", &r, || false).unwrap_err().outcome_known);
        set_fault(None);
        let app = NativeApp::open(t.path(), Example).unwrap();
        assert_eq!(
            app.operation_get(&lookup("set", &r)).unwrap()["state"],
            "RECORDED"
        );
        assert_eq!(app.inspect().unwrap()["projection"]["value"], 2);
        assert_eq!(
            app.apply("set", &r, || false).unwrap_err().code,
            ErrorCode::StaleReference
        );
    }
    #[test]
    fn export_without_document_receipt_is_unknown_and_keeps_bytes() {
        let t = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let app = NativeApp::create(t.path(), Example)
            .unwrap()
            .with_output_root(output.path())
            .unwrap();
        let r = request(
            &app,
            "export",
            json!({"output_namespace":"one","slot":"data"}),
        );
        let before = fs::read(t.path().join("document.json")).unwrap();
        set_fault(Some("export-before-receipt"));
        assert!(!app.apply("export", &r, || false).unwrap_err().outcome_known);
        set_fault(None);
        assert!(output.path().join("one/data.json").is_file());
        assert_eq!(before, fs::read(t.path().join("document.json")).unwrap());
        let app = NativeApp::open(t.path(), Example).unwrap();
        let history = app.operation_get(&lookup("export", &r)).unwrap();
        assert_eq!(history["state"], "ABSENT_OUTCOME_UNKNOWN");
        assert_eq!(history["result"], Value::Null);
        let retry = request(
            &app,
            "export",
            json!({"output_namespace":"two","slot":"data"}),
        );
        assert_eq!(
            app.apply("export", &retry, || false).unwrap_err().code,
            ErrorCode::Conflict
        );
    }
    #[test]
    fn fork_key_is_global_and_history_survives_child_changes_and_deletion() {
        let t = tempfile::tempdir().unwrap();
        let app = NativeApp::create(t.path(), Example).unwrap();
        let r = request(&app, "fork", json!({"workspace_id":"child"}));
        let before = fs::read(t.path().join("document.json")).unwrap();
        let result = app.apply("fork", &r, || false).unwrap();
        assert_eq!(before, fs::read(t.path().join("document.json")).unwrap());
        let other = request(&app, "fork", json!({"workspace_id":"other"}));
        assert_eq!(
            app.apply("fork", &other, || false).unwrap_err().code,
            ErrorCode::Conflict
        );
        assert_eq!(
            app.apply("set", &request(&app, "fork", json!({"value":2})), || false)
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        let child = app.workspace("child").unwrap();
        let mut edit = request(&child, "edit", json!({"value":7}));
        edit["workspace_id"] = json!("child");
        child.apply("set", &edit, || false).unwrap();
        let mut nested = request(&child, "nested", json!({"workspace_id":"nested"}));
        nested["workspace_id"] = json!("child");
        assert_eq!(
            child.apply("fork", &nested, || false).unwrap_err().code,
            ErrorCode::Unsupported
        );
        assert_eq!(
            app.operation_get(&lookup("fork", &r)).unwrap()["result"],
            result
        );
        fs::remove_dir_all(t.path().join("workspace-child")).unwrap();
        assert_eq!(
            app.operation_get(&lookup("fork", &r)).unwrap()["result"],
            result
        );
    }
    #[test]
    fn fork_origin_recovers_failed_journal_but_partial_directory_does_not() {
        let t = tempfile::tempdir().unwrap();
        let app = NativeApp::create(t.path(), Example).unwrap();
        let r = request(&app, "fork", json!({"workspace_id":"child"}));
        set_fault(Some("fork-before-journal"));
        assert!(!app.apply("fork", &r, || false).unwrap_err().outcome_known);
        set_fault(None);
        let reopened = NativeApp::open(t.path(), Example).unwrap();
        assert_eq!(
            reopened.operation_get(&lookup("fork", &r)).unwrap()["state"],
            "RECORDED"
        );
        fs::remove_file(t.path().join("workspace-child/origin.json")).unwrap();
        assert_eq!(
            reopened.operation_get(&lookup("fork", &r)).unwrap()["state"],
            "ABSENT_OUTCOME_UNKNOWN"
        );
    }
    #[test]
    fn legacy_receipts_stay_readable_and_do_not_gain_new_binding() {
        let t = tempfile::tempdir().unwrap();
        let app = NativeApp::create(t.path(), Example).unwrap();
        let r = request(&app, "legacy", json!({"value":2}));
        app.apply("set", &r, || false).unwrap();
        fs::remove_dir_all(t.path().join("operations")).unwrap();
        let path = t.path().join("document.json");
        let mut doc: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        doc["receipts"]["legacy"]
            .as_object_mut()
            .unwrap()
            .remove("binding");
        fs::write(&path, serde_json::to_vec(&doc).unwrap()).unwrap();
        let app = NativeApp::open(t.path(), Example).unwrap();
        assert_eq!(
            app.operation_get(&lookup("set", &r)).unwrap()["state"],
            "ABSENT_OUTCOME_UNKNOWN"
        );
        assert!(!t.path().join("operations").exists());
        assert_eq!(
            app.apply("set", &request(&app, "legacy", json!({"value":9})), || {
                false
            })
            .unwrap_err()
            .code,
            ErrorCode::Conflict
        );
    }
    #[test]
    fn held_lock_refuses_read_and_mutation_without_wait_or_effect() {
        let t = tempfile::tempdir().unwrap();
        let app = NativeApp::create(t.path(), Example).unwrap();
        let r = request(&app, "busy", json!({"value":2}));
        let before = fs::read(t.path().join("document.json")).unwrap();
        let lock = app.lock().unwrap();
        assert_eq!(
            app.inspect().unwrap_err().code,
            ErrorCode::ResourceExhausted
        );
        assert_eq!(
            app.apply("set", &r, || false).unwrap_err().code,
            ErrorCode::ResourceExhausted
        );
        assert!(!t.path().join("operations").exists());
        assert_eq!(before, fs::read(t.path().join("document.json")).unwrap());
        drop(lock);
        app.inspect().unwrap();
    }
    #[test]
    fn invalid_or_cancelled_candidate_does_not_reserve_key() {
        let t = tempfile::tempdir().unwrap();
        let app = NativeApp::create(t.path(), Example).unwrap();
        let r = request(&app, "same", json!({"value":"invalid"}));
        assert!(app.apply("set", &r, || false).is_err());
        assert!(!t.path().join("operations").exists());
        let r = request(&app, "same", json!({"value":2}));
        let calls = std::cell::Cell::new(0);
        assert_eq!(
            app.apply("set", &r, || {
                calls.set(calls.get() + 1);
                calls.get() > 1
            })
            .unwrap_err()
            .code,
            ErrorCode::Cancelled
        );
        assert!(!t.path().join("operations").exists());
        app.apply("set", &r, || false).unwrap();
    }
    #[test]
    fn cancellation_after_intent_is_terminal_and_recoverable_without_replay() {
        for operation in ["set", "fork"] {
            let t = tempfile::tempdir().unwrap();
            let app = NativeApp::create(t.path(), Example).unwrap();
            let parameters = if operation == "set" {
                json!({"value":2})
            } else {
                json!({"workspace_id":"child"})
            };
            let r = request(&app, "cancel", parameters);
            let bytes = fs::read(t.path().join("document.json")).unwrap();
            let checks = std::cell::Cell::new(0);
            let error = app
                .apply(operation, &r, || {
                    checks.set(checks.get() + 1);
                    checks.get() >= 3
                })
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::Cancelled);
            assert!(error.outcome_known);
            assert_eq!(
                app.apply(operation, &r, || false).unwrap_err().code,
                ErrorCode::Cancelled
            );
            assert_eq!(bytes, fs::read(t.path().join("document.json")).unwrap());
            assert!(!t.path().join("workspace-child").exists());
            let reopened = NativeApp::open(t.path(), Example).unwrap();
            let history = reopened.operation_get(&lookup(operation, &r)).unwrap();
            assert_eq!(history["state"], "RECORDED");
            assert_eq!(history["error"]["code"], "Cancelled");
            assert_eq!(history["result"], Value::Null);
        }
    }
    #[test]
    fn failed_cancellation_record_retains_unknown_intent() {
        let t = tempfile::tempdir().unwrap();
        let app = NativeApp::create(t.path(), Example).unwrap();
        let r = request(&app, "cancel", json!({"value":2}));
        let checks = std::cell::Cell::new(0);
        set_fault(Some("cancel-before-record"));
        assert!(
            !app.apply("set", &r, || {
                checks.set(checks.get() + 1);
                checks.get() >= 3
            })
            .unwrap_err()
            .outcome_known
        );
        set_fault(None);
        assert_eq!(
            app.operation_get(&lookup("set", &r)).unwrap()["state"],
            "ABSENT_OUTCOME_UNKNOWN"
        );
        assert!(!app.apply("set", &r, || false).unwrap_err().outcome_known);
    }
    #[test]
    fn corrupt_existing_snapshot_is_rejected_before_new_intent() {
        let t = tempfile::tempdir().unwrap();
        let app = NativeApp::create(t.path(), Example).unwrap();
        let snap = app
            .apply("snapshot", &request(&app, "snapshot", json!({})), || false)
            .unwrap();
        let path = t.path().join(format!(
            "snapshot-{}.json",
            snap["snapshot_id"].as_str().unwrap()
        ));
        fs::write(path, b"{}").unwrap();
        let r = request(&app, "snapshot_again", json!({}));
        assert_eq!(
            app.apply("snapshot", &r, || false).unwrap_err().code,
            ErrorCode::Conflict
        );
        assert!(!t.path().join("operations/snapshot_again.json").exists());
    }

    #[derive(Clone)]
    struct CompatibleReadOnly;
    impl Model for CompatibleReadOnly {
        fn id(&self) -> &'static str {
            "recovery-example"
        }
        fn initial(&self) -> Value {
            Example.initial()
        }
        fn validate(&self, v: &Value) -> Result<()> {
            Example.validate(v)
        }
        fn operations(&self) -> Vec<Operation> {
            vec![]
        }
        fn apply(&self, _: &Value, _: &str, _: &Value) -> Result<Value> {
            panic!("Removed operation must not be dispatched")
        }
    }
    #[tokio::test]
    async fn removed_mutator_history_remains_readable_without_restoring_its_capability() {
        let t = tempfile::tempdir().unwrap();
        let app = NativeApp::create(t.path(), Example).unwrap();
        let r = request(&app, "old", json!({"value":2}));
        let result = app.apply("set", &r, || false).unwrap();
        let old_cap = app
            .capabilities_value()
            .into_iter()
            .find(|c| c.descriptor.name.ends_with(".set"))
            .unwrap();
        let mut changed = NativeApp::open(t.path(), CompatibleReadOnly).unwrap();
        let capabilities = changed.capabilities_value();
        assert!(
            !capabilities
                .iter()
                .any(|c| c.descriptor.name.ends_with(".set"))
        );
        let read = capabilities
            .iter()
            .find(|c| c.descriptor.name.ends_with(".operation.get"))
            .unwrap();
        let history = changed
            .execute(
                &read.descriptor.name,
                &descriptor_digest(&read.descriptor).unwrap(),
                lookup("set", &r),
            )
            .await
            .unwrap();
        assert_eq!(history["state"], "RECORDED");
        assert_eq!(history["result"], result);
        assert_eq!(history["current_authority"], false);
        assert_eq!(
            changed
                .execute(
                    &old_cap.descriptor.name,
                    &descriptor_digest(&old_cap.descriptor).unwrap(),
                    r.clone()
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::Unsupported
        );
        let mut missing = r;
        missing["operation_key"] = json!("unrecorded");
        assert_eq!(
            changed
                .operation_get(&lookup("removed_other_name", &missing))
                .unwrap()["state"],
            "ABSENT_OUTCOME_UNKNOWN"
        );
    }
}
