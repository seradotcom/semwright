//! Application cooperation over canonical Driver SDK v4. This is not a Broker,
//! an application framework, or a sandbox for application code.
pub use async_trait::async_trait;
pub use semwright_driver_sdk::{
    self as driver_sdk, Capability, Driver, DriverExecutionContext, DriverInterfaces,
    descriptor_digest, serve,
};
pub use semwright_types::{
    self as types, CommandDescriptor, Error, ErrorCode, Idempotency, NativeTarget, Result, Risk,
};
use serde::{Deserialize, Serialize};
pub use serde_json::{self, Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
pub use tokio;

pub const NATIVE_SCHEMA: &str = "semwright-native/0.3";
pub const SDK_VERSION: &str = env!("CARGO_PKG_VERSION");
const MAX_DOCUMENT: u64 = 1024 * 1024;
const MAX_RECEIPTS: usize = 512;
const MAX_EVENTS: usize = 128;

/// A bounded operation that the application implements.
/// The SDK uses canonical mutation risk and idempotency descriptor values.
#[derive(Clone)]
pub struct Operation {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
}

/// Implement the application model through this trait. The SDK has no private
/// Platform imports. Return the same candidate state for the same inputs.
/// Do not perform I/O in `apply`. The SDK checks the revision under the document
/// lock before it commits the candidate state.
pub trait Model: Send + Sync + Clone + 'static {
    fn id(&self) -> &'static str;
    fn initial(&self) -> Value;
    fn validate(&self, state: &Value) -> Result<()>;
    fn operations(&self) -> Vec<Operation>;
    fn apply(&self, state: &Value, operation: &str, parameters: &Value) -> Result<Value>;
    fn projection(&self, state: &Value) -> Result<Value> {
        Ok(state.clone())
    }
    /// Report model relations. The default does not declare complete coverage.
    fn dependencies(&self, _state: &Value) -> Result<Value> {
        Ok(json!({"relations":[],"coverage":"not_declared"}))
    }
    fn export(&self, state: &Value) -> Result<(String, Vec<u8>)> {
        Ok(("application/json".into(), serde_json::to_vec_pretty(state)?))
    }
    fn snapshot_supported(&self) -> bool {
        true
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    request_digest: String,
    result: Value,
    #[serde(default)]
    binding: Option<OperationBinding>,
    #[serde(default)]
    error: Option<Error>,
}

mod recovery;
use recovery::OperationBinding;
pub use recovery::request_digest;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    schema: String,
    resource_id: String,
    revision: u64,
    state: Value,
    sequence: u64,
    events: Vec<Value>,
    receipts: BTreeMap<String, Receipt>,
}

pub fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn bounded_token(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= 96
        && v.bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
}
fn validate_optional_ref(value: &Value) -> Result<()> {
    if let Some(reference) = value.get("ref") {
        let valid = reference.as_str().is_some_and(|s| {
            s.strip_prefix("native:").is_some_and(|tail| {
                tail.len() == 32
                    && tail
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
        });
        if !valid {
            return Err(Error::invalid(
                "ref must have the canonical native reference shape",
            ));
        }
    }
    Ok(())
}
fn ref_schema() -> Value {
    json!({"type":"string","pattern":"^native:[0-9a-f]{32}$","maxLength":39})
}
fn known_fields(value: &Value, fields: &[&str]) -> Result<()> {
    let o = value
        .as_object()
        .ok_or_else(|| Error::invalid("An object is required"))?;
    if o.keys().any(|k| !fields.contains(&k.as_str())) {
        return Err(Error::invalid("Unknown field"));
    }
    Ok(())
}
fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::invalid(format!("Missing string {key}")))
}
fn read_bounded(path: &Path) -> Result<Vec<u8>> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > MAX_DOCUMENT {
        return Err(Error::invalid("Native file is not a bounded regular file"));
    }
    let mut out = Vec::new();
    File::open(path)?
        .take(MAX_DOCUMENT + 1)
        .read_to_end(&mut out)?;
    if out.len() as u64 > MAX_DOCUMENT {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Native document exceeds limit",
        ));
    }
    Ok(out)
}
fn storage_supported() -> Result<()> {
    if !cfg!(unix) {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "This storage profile requires Unix file and directory synchronization",
        ));
    }
    Ok(())
}
fn sync_dir(path: &Path) -> Result<()> {
    File::open(path)?.sync_all()?;
    Ok(())
}
fn try_lock(file: &File) -> Result<()> {
    #[cfg(unix)]
    {
        // Preserve an open-file-description advisory lock on MSRV 1.88.
        // fcntl process locks would not protect competing in-process instances.
        rustix::fs::flock(file, rustix::fs::FlockOperation::NonBlockingLockExclusive).map_err(
            |_| {
                Error::new(
                    ErrorCode::ResourceExhausted,
                    "BUSY: document lock is unavailable",
                )
            },
        )
    }
    #[cfg(not(unix))]
    {
        let _ = file;
        Err(Error::new(
            ErrorCode::Unsupported,
            "File profile locking is not implemented on this platform",
        ))
    }
}
fn write_atomic(dir: &Path, name: &str, bytes: &[u8]) -> Result<()> {
    if bytes.len() as u64 > MAX_DOCUMENT {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Native document exceeds limit",
        ));
    }
    let pending = dir.join(format!(".pending-{}", types::unique_id()));
    let result = (|| {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&pending)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        fs::rename(&pending, dir.join(name))?;
        sync_dir(dir).map_err(|e| e.uncertain())?;
        Ok(())
    })();
    if pending.exists() {
        let _ = fs::remove_file(pending);
    }
    result
}

/// The SDK locks one document during a transaction. It writes the replacement
/// document atomically. The file lock is advisory. Application code can bypass it.
/// The revision includes a digest of the exact document bytes and detects ordinary
/// external edits. A busy lock is rejected without waiting.
pub struct NativeApp<M: Model> {
    model: M,
    root: PathBuf,
    output_root: PathBuf,
    generation: String,
    workspace_id: Option<String>,
    observed_children: Arc<Mutex<BTreeMap<String, String>>>,
}
impl<M: Model> NativeApp<M> {
    pub fn create(root: impl AsRef<Path>, model: M) -> Result<Self> {
        storage_supported()?;
        fs::create_dir_all(root.as_ref())?;
        let root = root.as_ref().canonicalize()?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join("document.lock"))?;
        try_lock(&lock)?;
        if root.join("document.json").exists() {
            return Err(Error::new(ErrorCode::Conflict, "Document already exists"));
        }
        let state = model.initial();
        model.validate(&state)?;
        let doc = Document {
            schema: NATIVE_SCHEMA.into(),
            resource_id: types::unique_id(),
            revision: 1,
            state,
            sequence: 0,
            events: vec![],
            receipts: BTreeMap::new(),
        };
        write_atomic(&root, "document.json", &serde_json::to_vec(&doc)?)?;
        if let Some(parent) = root.parent() {
            sync_dir(parent).map_err(|e| e.uncertain())?;
        }
        drop(lock);
        Self::open(root, model)
    }
    pub fn open(root: impl AsRef<Path>, model: M) -> Result<Self> {
        storage_supported()?;
        let root = root.as_ref().canonicalize()?;
        // Lock file must be provisioned by create; inspect/reopen never creates documents.
        if fs::symlink_metadata(root.join("document.lock"))?
            .file_type()
            .is_symlink()
        {
            return Err(Error::invalid("Invalid native lock"));
        }
        let app = Self {
            model,
            output_root: root.join("outputs"),
            root,
            generation: types::unique_id(),
            workspace_id: None,
            observed_children: Arc::new(Mutex::new(BTreeMap::new())),
        };
        app.read()?;
        Ok(app)
    }
    pub fn with_output_root(mut self, root: impl AsRef<Path>) -> Result<Self> {
        self.output_root = root.as_ref().canonicalize()?;
        Ok(self)
    }
    fn workspace(&self, key: &str) -> Result<Self> {
        if !bounded_token(key) {
            return Err(Error::invalid("Invalid workspace id"));
        }
        let path = self.root.join(format!("workspace-{key}"));
        if fs::symlink_metadata(&path)?.file_type().is_symlink() {
            return Err(Error::invalid("Invalid workspace root"));
        }
        let mut app = Self::open(path, self.model.clone())?;
        app.output_root = self.output_root.clone();
        app.generation = sha256(format!("{}:{key}", self.generation).as_bytes());
        app.workspace_id = Some(key.into());
        app.observed_children = Arc::clone(&self.observed_children);
        Ok(app)
    }
    fn lock(&self) -> Result<File> {
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .open(self.root.join("document.lock"))?;
        try_lock(&f)?;
        Ok(f)
    }
    fn read(&self) -> Result<(Document, String)> {
        let bytes = read_bounded(&self.root.join("document.json"))?;
        let doc: Document = serde_json::from_slice(&bytes)?;
        if doc.schema != NATIVE_SCHEMA || doc.revision == 0 || !bounded_token(&doc.resource_id) {
            return Err(Error::new(
                ErrorCode::ProtocolMismatch,
                "Unsupported native document",
            ));
        }
        self.model.validate(&doc.state)?;
        Ok((doc, sha256(&bytes)))
    }
    fn revision(doc: &Document, fingerprint: &str) -> String {
        format!("{}:{}", doc.revision, fingerprint)
    }
    pub fn inspect(&self) -> Result<Value> {
        let _lock = self.lock()?;
        let (doc, fp) = self.read()?;
        let dependencies = self.model.dependencies(&doc.state)?;
        let dependencies_digest = format!("sha256:{}", sha256(&serde_json::to_vec(&dependencies)?));
        let coverage = dependencies
            .get("coverage")
            .and_then(Value::as_str)
            .unwrap_or("not_declared")
            .to_owned();
        let mut view = json!({"schema_version":NATIVE_SCHEMA,"resource_id":doc.resource_id,"revision":Self::revision(&doc,&fp),"generation":self.generation,"sequence":doc.sequence.to_string(),"model":self.model.id(),"projection":self.model.projection(&doc.state)?,"dependencies":dependencies,"source_digest":format!("sha256:{fp}"),"dependencies_digest":dependencies_digest,"source":{"provider":format!("driver:{}",self.model.id()),"sdk_version":SDK_VERSION,"observed_at_ms":SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis().to_string(),"freshness":"observed","coverage":coverage},"native_target":NativeTarget{kind:"native".into(), identity:format!("{}:{}",self.generation,doc.resource_id),revision:doc.revision,fingerprint:fp,app:self.model.id().into()}});
        view["ref"] = types::target_marker(serde_json::from_value(view["native_target"].clone())?);
        if let Some(workspace) = &self.workspace_id {
            let mut observed = self.observed_children.lock().map_err(|_| {
                Error::new(
                    ErrorCode::Internal,
                    "Observed target registry is unavailable",
                )
            })?;
            if !observed.contains_key(workspace) && observed.len() >= MAX_RECEIPTS {
                return Err(Error::new(
                    ErrorCode::ResourceExhausted,
                    "Observed workspace limit reached",
                ));
            }
            observed.insert(
                workspace.clone(),
                text(&view["native_target"], "identity")?.into(),
            );
        }
        Ok(view)
    }
    pub fn interfaces_value(&self) -> Value {
        json!({"schema_version":NATIVE_SCHEMA,"driver_protocol":4,"identity":true,"revision":"exact_document_bytes_and_counter","projection":true,"operations":true,"snapshot":self.model.snapshot_supported(),"workspace":true,"publication":false,"observations":true,"dependencies":true,"events":true,"cooperative_cancellation":true,"historical_operation_lookup":true,"trust":"external_driver","in_process_sandbox":false})
    }
    pub fn events(&self, after: u64) -> Result<Value> {
        let _lock = self.lock()?;
        let (doc, _) = self.read()?;
        let oldest = doc.sequence.saturating_sub(doc.events.len() as u64);
        if after < oldest || after > doc.sequence {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Event cursor requires resync",
            ));
        }
        Ok(
            json!({"schema_version":NATIVE_SCHEMA,"generation":self.generation,"watermark":doc.sequence.to_string(),"events":doc.events.into_iter().filter(|e| e["sequence"].as_str().and_then(|x|x.parse::<u64>().ok()).is_some_and(|s|s>after)).collect::<Vec<_>>(),"events_are_invalidation_hints":true}),
        )
    }
    pub fn validate_target(&self, target: &NativeTarget) -> Result<()> {
        let _lock = self.lock()?;
        let (doc, fp) = self.read()?;
        let expected = NativeTarget {
            kind: "native".into(),
            identity: format!("{}:{}", self.generation, doc.resource_id),
            revision: doc.revision,
            fingerprint: fp,
            app: self.model.id().into(),
        };
        if target.kind != expected.kind
            || target.identity != expected.identity
            || target.revision != expected.revision
            || target.fingerprint != expected.fingerprint
            || target.app != expected.app
        {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Native reference is stale; inspect again",
            ));
        }
        Ok(())
    }
    /// Compare the target with the document selected by these arguments.
    /// This check does not create authority or authenticate a caller.
    pub fn validate_execution_target(&self, args: &Value, target: &NativeTarget) -> Result<()> {
        if let Some(workspace) = args.get("workspace_id") {
            return self
                .workspace(
                    workspace
                        .as_str()
                        .ok_or_else(|| Error::invalid("Invalid workspace id"))?,
                )?
                .validate_target(target);
        }
        self.validate_target(target)
    }
    fn validate_observed_target(&self, target: &NativeTarget) -> Result<()> {
        if target
            .identity
            .starts_with(&format!("{}:", self.generation))
        {
            return self.validate_target(target);
        }
        let workspace = {
            let observed = self.observed_children.lock().map_err(|_| {
                Error::new(
                    ErrorCode::Internal,
                    "Observed target registry is unavailable",
                )
            })?;
            observed
                .iter()
                .find(|(_, identity)| identity.as_str() == target.identity)
                .map(|(workspace, _)| workspace.clone())
        }
        .ok_or_else(|| {
            Error::new(
                ErrorCode::StaleReference,
                "Native child reference was not observed in this app generation",
            )
        })?;
        self.workspace(&workspace)?.validate_target(target)
    }
    pub fn apply(&self, op: &str, args: &Value, cancelled: impl Fn() -> bool) -> Result<Value> {
        storage_supported()?;
        if self.workspace_id.is_none() {
            if let Some(id) = args.get("workspace_id") {
                return self
                    .workspace(
                        id.as_str()
                            .ok_or_else(|| Error::invalid("Invalid workspace id"))?,
                    )?
                    .apply(op, args, cancelled);
            }
        }
        known_fields(
            args,
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
        validate_optional_ref(args)?;
        if args.get("workspace_id").and_then(Value::as_str) != self.workspace_id.as_deref() {
            return Err(Error::invalid(
                "Request workspace differs from selected document",
            ));
        }
        let key = text(args, "operation_key")?;
        if !bounded_token(key) {
            return Err(Error::invalid("Invalid operation key"));
        }
        let digest = request_digest(op, args)?;
        if text(args, "expected_generation")? != self.generation {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Native generation changed",
            ));
        }
        if cancelled() {
            return Err(Error::new(ErrorCode::Cancelled, "Cancelled before effects"));
        }
        let _lock = self.lock()?;
        let (mut doc, fp) = self.read()?;
        let binding = OperationBinding {
            source_resource_id: doc.resource_id.clone(),
            operation: op.into(),
            operation_key: key.into(),
            request_digest: digest.clone(),
            workspace_id: self.workspace_id.clone(),
            fork_workspace_id: if op == "fork" {
                Some(text(&args["parameters"], "workspace_id")?.into())
            } else {
                None
            },
        };
        let history = self.operation_history(&doc, &binding)?;
        if let Some(receipt) = history.receipt {
            return match receipt.error {
                Some(error) => Err(error),
                None => Ok(receipt.result),
            };
        }
        if history.reserved {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Operation outcome is unknown; read operation.get and do not replay",
            )
            .uncertain());
        }
        if text(args, "expected_revision")? != Self::revision(&doc, &fp) {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Native document changed; inspect again",
            ));
        }
        let parameters = args
            .get("parameters")
            .ok_or_else(|| Error::invalid("Parameters are required"))?;
        if op == "fork" {
            return self.fork_document(&doc, &fp, &binding, parameters, cancelled);
        }
        let mut artifact = None;
        let mut snapshot_id = None;
        let mut extra_write: Option<(String, Vec<u8>)> = None;
        let mut exported: Option<(PathBuf, Vec<u8>)> = None;
        let state = match op {
            "snapshot" => {
                if !self.model.snapshot_supported() {
                    return Err(Error::new(
                        ErrorCode::Unsupported,
                        "Snapshot interface is not supported",
                    ));
                }
                known_fields(parameters, &[])?;
                let bytes = serde_json::to_vec(&doc.state)?;
                let id = sha256(&bytes);
                let target = self.root.join(format!("snapshot-{id}.json"));
                if target.exists() {
                    if read_bounded(&target)? != bytes {
                        return Err(Error::new(
                            ErrorCode::Conflict,
                            "Existing snapshot bytes differ from their digest",
                        ));
                    }
                } else {
                    extra_write = Some((format!("snapshot-{id}.json"), bytes));
                }
                snapshot_id = Some(id);
                doc.state.clone()
            }
            "restore" => {
                if !self.model.snapshot_supported() {
                    return Err(Error::new(
                        ErrorCode::Unsupported,
                        "Snapshot interface is not supported",
                    ));
                }
                known_fields(parameters, &["snapshot_id"])?;
                let id = text(parameters, "snapshot_id")?;
                if id.len() != 64 || !id.bytes().all(|x| x.is_ascii_hexdigit()) {
                    return Err(Error::invalid("Invalid snapshot id"));
                }
                let bytes = read_bounded(&self.root.join(format!("snapshot-{id}.json")))?;
                if sha256(&bytes) != id {
                    return Err(Error::new(ErrorCode::Conflict, "Snapshot integrity failed"));
                }
                serde_json::from_slice(&bytes)?
            }
            "export" => {
                known_fields(parameters, &["output_namespace", "slot"])?;
                let ns = text(parameters, "output_namespace")?;
                let slot = text(parameters, "slot")?;
                if !bounded_token(ns) || !bounded_token(slot) {
                    return Err(Error::invalid("Invalid pinned output namespace"));
                }
                let (media_type, bytes) = self.model.export(&doc.state)?;
                if bytes.len() as u64 > MAX_DOCUMENT {
                    return Err(Error::new(
                        ErrorCode::ResourceExhausted,
                        "Export exceeds the byte limit",
                    ));
                }
                let extension = match media_type.as_str() {
                    "text/csv" => "csv",
                    "application/json" => "json",
                    "text/html" => "html",
                    _ => {
                        return Err(Error::new(
                            ErrorCode::Unsupported,
                            "Model export media type is not supported",
                        ));
                    }
                };
                let relative = format!("{ns}/{slot}.{extension}");
                let destination = self.output_root.join(&relative);
                if destination.exists() {
                    return Err(Error::new(
                        ErrorCode::Conflict,
                        "Output namespace already contains this slot",
                    ));
                }
                for dir in [
                    &self.output_root,
                    destination
                        .parent()
                        .ok_or_else(|| Error::invalid("Invalid export destination"))?,
                ] {
                    if fs::symlink_metadata(dir)
                        .is_ok_and(|m| m.file_type().is_symlink() || !m.is_dir())
                    {
                        return Err(Error::invalid("Export directory is invalid"));
                    }
                }
                artifact = Some(
                    json!({"mime_type":media_type,"sha256":sha256(&bytes),"bytes":bytes.len().to_string(),"path":relative,"root":"native-output","slot":slot}),
                );
                exported = Some((destination, bytes));
                doc.state.clone()
            }
            _ => self.model.apply(&doc.state, op, parameters)?,
        };
        self.model.validate(&state)?;
        let before = Self::revision(&doc, &fp);
        doc.state = state;
        doc.revision = doc
            .revision
            .checked_add(1)
            .ok_or_else(|| Error::invalid("Revision exhausted"))?;
        doc.sequence = doc
            .sequence
            .checked_add(1)
            .ok_or_else(|| Error::invalid("Sequence exhausted"))?;
        let result = json!({"schema_version":NATIVE_SCHEMA,"resource_id":doc.resource_id,"generation":self.generation,"operation_key":key,"operation":op,"before_revision":before,"committed_revision":doc.revision.to_string(),"state_sha256":sha256(&serde_json::to_vec(&doc.state)?),"effect_state":"observed","verification_state":"not_run","snapshot_id":snapshot_id,"workspace_id":self.workspace_id,"artifact":artifact,"readback_required":true});
        doc.events.push(json!({"sequence":doc.sequence.to_string(),"generation":self.generation,"operation_key":key,"kind":"native.changed","resource_id":doc.resource_id,"revision":doc.revision.to_string(),"readback_required":true}));
        if doc.events.len() > MAX_EVENTS {
            doc.events.remove(0);
        }
        let receipt = Receipt {
            request_digest: digest,
            result: result.clone(),
            binding: Some(binding.clone()),
            error: None,
        };
        doc.receipts.insert(key.into(), receipt.clone());
        let bytes = serde_json::to_vec(&doc)?;
        if bytes.len() as u64 > MAX_DOCUMENT {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Native document exceeds limit",
            ));
        }
        self.check_operation_capacity(&doc, &binding, &receipt)?;
        if cancelled() {
            return Err(Error::new(
                ErrorCode::Cancelled,
                "Cancelled before SDK effects",
            ));
        }
        self.reserve_operation(&binding)?;
        if cancelled() {
            return self.cancel_reserved(&binding);
        }
        (|| {
            if let Some((name, bytes)) = extra_write {
                write_atomic(&self.root, &name, &bytes)?;
            }
            if let Some((destination, bytes)) = exported {
                let parent = destination
                    .parent()
                    .ok_or_else(|| Error::invalid("Invalid export destination"))?;
                fs::create_dir_all(parent)?;
                let mut f = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&destination)?;
                f.write_all(&bytes)?;
                f.sync_all()?;
                sync_dir(parent)?;
                // The granted output root is the persistence boundary. Its parent
                // can be a synthetic/read-only Driver Host mount namespace and is
                // neither application storage nor owner-granted authority.
                sync_dir(&self.output_root)?;
                recovery::fault("export-before-receipt")?;
            }
            write_atomic(&self.root, "document.json", &bytes)?;
            recovery::fault("document-before-journal")?;
            self.record_operation(&binding, &receipt)?;
            Ok(result)
        })()
        .map_err(|e: Error| e.uncertain())
    }
    fn fork_document(
        &self,
        doc: &Document,
        fp: &str,
        binding: &OperationBinding,
        parameters: &Value,
        cancelled: impl Fn() -> bool,
    ) -> Result<Value> {
        if self.workspace_id.is_some() {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "Nested workspaces are not supported",
            ));
        }
        known_fields(parameters, &["workspace_id"])?;
        let id = text(parameters, "workspace_id")?;
        if !bounded_token(id) {
            return Err(Error::invalid("Invalid workspace id"));
        }
        let destination = self.root.join(format!("workspace-{id}"));
        if destination.exists() {
            return Err(Error::new(ErrorCode::Conflict, "Workspace already exists"));
        }
        let copied = Document {
            schema: NATIVE_SCHEMA.into(),
            resource_id: types::unique_id(),
            revision: 1,
            state: doc.state.clone(),
            sequence: 0,
            events: vec![],
            receipts: BTreeMap::new(),
        };
        let bytes = serde_json::to_vec(&copied)?;
        let result = json!({"schema_version":NATIVE_SCHEMA,"resource_id":copied.resource_id,"generation":sha256(format!("{}:{id}",self.generation).as_bytes()),"operation_key":binding.operation_key,"operation":"fork","before_revision":Self::revision(doc,fp),"committed_revision":"1","state_sha256":sha256(&serde_json::to_vec(&copied.state)?),"effect_state":"observed","verification_state":"not_run","snapshot_id":null,"workspace_id":id,"artifact":null,"readback_required":true,"initial_document_sha256":sha256(&bytes)});
        let receipt = Receipt {
            request_digest: binding.request_digest.clone(),
            result: result.clone(),
            binding: Some(binding.clone()),
            error: None,
        };
        self.check_operation_capacity(doc, binding, &receipt)?;
        if cancelled() {
            return Err(Error::new(
                ErrorCode::Cancelled,
                "Cancelled before workspace publication",
            ));
        }
        self.reserve_operation(binding)?;
        if cancelled() {
            return self.cancel_reserved(binding);
        }
        (|| {
            // create_dir is exclusive. A partial destination remains under the reserved key.
            fs::create_dir(&destination)?;
            let lock = OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(destination.join("document.lock"))?;
            try_lock(&lock)?;
            lock.sync_all()?;
            write_atomic(&destination, "document.json", &bytes)?;
            sync_dir(&destination)?;
            sync_dir(&self.root)?;
            // This immutable origin is written only after the copied document is durable.
            write_atomic(&destination, "origin.json", &serde_json::to_vec(&receipt)?)?;
            recovery::fault("fork-before-journal")?;
            self.record_operation(binding, &receipt)?;
            Ok(result)
        })()
        .map_err(|e: Error| e.uncertain())
    }
    pub fn describe_model(model: M) -> Vec<Capability> {
        Self {
            model,
            root: PathBuf::new(),
            output_root: PathBuf::new(),
            generation: String::new(),
            workspace_id: None,
            observed_children: Arc::new(Mutex::new(BTreeMap::new())),
        }
        .capabilities_value()
    }
    pub fn capabilities_value(&self) -> Vec<Capability> {
        let mut ops = vec![
            (
                "inspect",
                "Observe the persistent native model",
                json!({"type":"object","properties":{"workspace_id":{"type":"string","pattern":"^[A-Za-z0-9_-]{1,96}$"}},"additionalProperties":false}),
                true,
            ),
            (
                "interfaces",
                "Negotiate Native SDK interfaces",
                json!({"type":"object","properties":{"workspace_id":{"type":"string","pattern":"^[A-Za-z0-9_-]{1,96}$"}},"additionalProperties":false}),
                true,
            ),
            (
                "events",
                "Read bounded invalidation hints",
                json!({"type":"object","properties":{"workspace_id":{"type":"string","pattern":"^[A-Za-z0-9_-]{1,96}$"},"after":{"type":"string","pattern":"^[0-9]+$"}},"required":["after"],"additionalProperties":false}),
                true,
            ),
        ];
        ops.push((
            "operation.get",
            "Read historical operation evidence",
            recovery::input_schema(),
            true,
        ));
        for op in self.model.operations() {
            ops.push((
                op.name,
                op.description,
                mutation_schema(op.input_schema),
                false,
            ));
        }
        for (name, description, params) in [
            (
                "snapshot",
                "Persist immutable snapshot",
                json!({"type":"object","properties":{},"additionalProperties":false}),
            ),
            (
                "restore",
                "Restore a snapshot with native CAS",
                json!({"type":"object","properties":{"snapshot_id":{"type":"string","pattern":"^[a-f0-9]{64}$"}},"required":["snapshot_id"],"additionalProperties":false}),
            ),
            (
                "fork",
                "Fork into an isolated named native workspace",
                json!({"type":"object","properties":{"workspace_id":{"type":"string","pattern":"^[A-Za-z0-9_-]{1,96}$"}},"required":["workspace_id"],"additionalProperties":false}),
            ),
            (
                "export",
                "Export the native model",
                json!({"type":"object","properties":{"output_namespace":{"type":"string","pattern":"^[A-Za-z0-9_-]{1,96}$"},"slot":{"type":"string","pattern":"^[A-Za-z0-9_-]{1,96}$"}},"required":["output_namespace","slot"],"additionalProperties":false}),
            ),
        ] {
            if self.model.snapshot_supported() || name == "export" || name == "fork" {
                ops.push((name, description, mutation_schema(params), false));
            }
        }
        ops.into_iter().map(|(name,desc,mut input_schema,read)|{input_schema["properties"].as_object_mut().expect("Native input object properties").insert("ref".into(),ref_schema());Capability{descriptor:CommandDescriptor{name:format!("driver.{}.{name}",self.model.id()),version:SDK_VERSION.into(),description:desc.into(),input_schema,output_schema:if name=="operation.get"{recovery::output_schema()}else{json!({"type":"object","required":["schema_version"],"properties":{"schema_version":{"const":NATIVE_SCHEMA}},"additionalProperties":true})},requires:vec![format!("driver:{}",self.model.id())],risk:if read {Risk::ReadOnly}else{Risk::Mutating},idempotency:if read{Idempotency::ReadOnly}else{Idempotency::Idempotent},timeout_ms:30000,dry_run:false,interactive_consent:false,backends:vec![format!("driver:{}",self.model.id())]},aliases:vec![],tags:vec!["native-sdk".into()],object_types:vec![self.model.id().into()]}}).collect()
    }
}
pub fn mutation_schema(mut parameters: Value) -> Value {
    fn relocate(value: &mut Value) {
        match value {
            Value::Object(map) => {
                for (k, v) in map {
                    if k == "$ref" {
                        if let Some(reference) = v.as_str() {
                            if let Some(tail) = reference.strip_prefix("#/") {
                                *v = Value::String(format!("#/properties/parameters/{tail}"));
                            }
                        }
                    } else {
                        relocate(v);
                    }
                }
            }
            Value::Array(items) => {
                for item in items {
                    relocate(item)
                }
            }
            _ => {}
        }
    }
    relocate(&mut parameters);
    json!({"type":"object","properties":{"workspace_id":{"type":"string","pattern":"^[A-Za-z0-9_-]{1,96}$"},"expected_revision":{"type":"string","maxLength":128},"expected_generation":{"type":"string","maxLength":96},"operation_key":{"type":"string","pattern":"^[A-Za-z0-9_-]{1,96}$"},"parameters":parameters,"wait_ms":{"type":"integer","minimum":0,"maximum":2000}},"required":["expected_revision","expected_generation","operation_key","parameters"],"additionalProperties":false})
}

#[async_trait]
impl<M: Model> Driver for NativeApp<M> {
    fn id(&self) -> &str {
        self.model.id()
    }
    fn version(&self) -> &str {
        SDK_VERSION
    }
    fn interfaces(&self) -> DriverInterfaces {
        DriverInterfaces {
            dynamic_capabilities: true,
            cooperative_cancellation: true,
            events: true,
            progress: false,
            artifacts: false,
            health: true,
            native_refs: true,
            host_tools: false,
            ..Default::default()
        }
    }
    async fn capabilities(&mut self) -> Result<Vec<Capability>> {
        Ok(self.capabilities_value())
    }
    async fn execute(&mut self, command: &str, hash: &str, args: Value) -> Result<Value> {
        self.dispatch(command, hash, args, None).await
    }
    async fn execute_with_context(
        &mut self,
        command: &str,
        hash: &str,
        args: Value,
        context: DriverExecutionContext,
    ) -> Result<Value> {
        self.dispatch(command, hash, args, Some(context)).await
    }
    async fn validate_native_ref(&mut self, target: &NativeTarget) -> Result<()> {
        self.validate_observed_target(target)
    }
}
impl<M: Model> NativeApp<M> {
    async fn dispatch(
        &self,
        command: &str,
        hash: &str,
        args: Value,
        context: Option<DriverExecutionContext>,
    ) -> Result<Value> {
        validate_optional_ref(&args)?;
        if args.get("ref").is_some() && context.as_ref().and_then(|c| c.native_target()).is_none() {
            return Err(Error::invalid(
                "ref requires a resolved native execution context",
            ));
        }
        let selected = if let Some(id) = args.get("workspace_id") {
            Some(
                self.workspace(
                    id.as_str()
                        .ok_or_else(|| Error::invalid("Workspace id must be a string"))?,
                )?,
            )
        } else {
            None
        };
        let app = selected.as_ref().unwrap_or(self);
        let caps = self.capabilities_value();
        let cap = caps
            .iter()
            .find(|c| c.descriptor.name == command)
            .ok_or_else(|| Error::new(ErrorCode::Unsupported, "Operation is not implemented"))?;
        if descriptor_digest(&cap.descriptor)? != hash {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Operation descriptor changed",
            ));
        }
        if let Some(c) = &context {
            c.check_cancelled()?;
            if let Some(t) = c.native_target() {
                self.validate_execution_target(&args, t)?;
            }
        }
        let op = command
            .strip_prefix(&format!("driver.{}.", self.model.id()))
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::PermissionDenied,
                    "Operation belongs to another application",
                )
            })?;
        match op {
            "inspect" => {
                known_fields(&args, &["workspace_id", "ref"])?;
                app.inspect()
            }
            "interfaces" => {
                known_fields(&args, &["workspace_id", "ref"])?;
                Ok(app.interfaces_value())
            }
            "operation.get" => app.operation_get(&args),
            "events" => {
                known_fields(&args, &["after", "workspace_id", "ref"])?;
                app.events(
                    text(&args, "after")?
                        .parse()
                        .map_err(|_| Error::invalid("Invalid event cursor"))?,
                )
            }
            _ => {
                let delay = args
                    .get("wait_ms")
                    .map(|v| {
                        v.as_u64()
                            .filter(|x| *x <= 2000)
                            .ok_or_else(|| Error::invalid("Wait exceeds cancellation test bound"))
                    })
                    .transpose()?
                    .unwrap_or(0);
                if delay > 0 {
                    if let Some(c) = &context {
                        let cancel = c.cancellation();
                        tokio::select! { _=tokio::time::sleep(std::time::Duration::from_millis(delay))=>{}, _=cancel.cancelled()=>return Err(Error::new(ErrorCode::Cancelled,"Cancelled before effects")) }
                    } else {
                        tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                    }
                }
                let result = app.apply(op, &args, || {
                    context
                        .as_ref()
                        .is_some_and(|c| c.cancellation().is_cancelled())
                })?;
                if let Some(c) = &context {
                    let _=c.emit_event("native.changed",json!({"resource_id":result["resource_id"],"generation":result["generation"],"readback_required":true}));
                }
                Ok(result)
            }
        }
    }
}

/// Start the application lifecycle from the command line. Supply document paths
/// through CLI installation arguments. Do not accept document paths from model
/// input. At runtime, Core supplies the `native-data` mount that the owner approved.
pub async fn run<M: Model>(model: M) -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 2 && args[1] == "--catalog" {
        let app = NativeApp {
            model,
            root: PathBuf::new(),
            output_root: PathBuf::new(),
            generation: String::new(),
            workspace_id: None,
            observed_children: Arc::new(Mutex::new(BTreeMap::new())),
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&app.capabilities_value())?
        );
        return Ok(());
    }
    let root = if args.len() == 3 && (args[1] == "--init" || args[1] == "--document") {
        PathBuf::from(&args[2])
    } else if args.len() == 1 {
        driver_sdk::workspace_mount("native-data")?
    } else {
        return Err(Error::invalid(
            "Use --init DIRECTORY or --document DIRECTORY",
        ));
    };
    if args.get(1).is_some_and(|v| v == "--init") {
        NativeApp::create(root, model)?;
        return Ok(());
    }
    let mut app = NativeApp::open(root, model)?;
    if args.len() == 1 {
        app = app.with_output_root(driver_sdk::workspace_mount("native-output")?)?;
    }
    serve(app).await
}
