//! Provider-owned derivation manifests, not another Project Graph or scheduler.
use super::{compiler, io::Directory, model::*, validate};
use crate::config::AuthoringConfig;
use semwright_project_graph::{AssetRevision, LogicalAssetId, ProjectId};
use semwright_semantic_composition::{Digest, canonical_digest, strict_decode};
use semwright_types::{Error, ErrorCode, Result};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

const MANIFEST: &str = "project.semwright.json";
const MAX_FILES: usize = 256;
const MAX_BYTES: u64 = 67_108_864;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedFile {
    pub asset: LogicalAssetId,
    pub revision: AssetRevision,
    pub sha256: Digest,
    pub kind: String,
    pub active: bool,
    pub logical_key: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DerivationRecord {
    pub project: ProjectId,
    pub slug: String,
    pub revision: u64,
    pub generator: String,
    pub intent: GodotAuthoringSpec,
    pub intent_digest: Digest,
    pub files: BTreeMap<String, ManagedFile>,
    pub bindings: BTreeMap<String, LogicalAssetId>,
    pub manifest_digest: Digest,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    version: u32,
    target: DerivationRecord,
    previous: Option<DerivationRecord>,
    pending: bool,
    staging: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
pub struct FileObservation {
    pub path: String,
    pub expected: Option<Digest>,
    pub actual: Option<Digest>,
    pub state: String,
}
#[derive(Debug, Clone)]
pub struct Snapshot {
    journal: Option<Journal>,
    state_digest: Option<Digest>,
    pub fingerprint: Digest,
    pub files: Vec<FileObservation>,
    pub status: String,
    pub exists: bool,
}
impl Snapshot {
    pub fn record(&self) -> Option<&DerivationRecord> {
        self.journal.as_ref().map(|j| &j.target)
    }
    pub fn pending(&self) -> bool {
        self.journal.as_ref().is_some_and(|j| j.pending)
    }
}
#[derive(Debug, Clone)]
pub struct PreparedFiles {
    pub before: Snapshot,
    pub target: DerivationRecord,
    pub bytes: BTreeMap<String, Vec<u8>>,
    pub writes: Vec<String>,
    pub manifest: Vec<u8>,
    pub recovery: bool,
}
#[derive(Debug, Clone, Serialize)]
pub struct WriteReceipt {
    pub project: ProjectId,
    pub revision: u64,
    pub written: Vec<String>,
    pub unchanged: Vec<String>,
    pub post_fingerprint: Digest,
    pub source_state: String,
}
pub(crate) fn contract(error: impl std::fmt::Display) -> Error {
    Error::invalid(error.to_string())
}
fn conflict(message: &str) -> Error {
    Error::new(ErrorCode::Conflict, message)
}
fn file_error(error: std::io::Error) -> Error {
    match error.kind() {
        std::io::ErrorKind::AlreadyExists => {
            conflict("DIVERGED: file or directory write precondition changed")
        }
        std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::InvalidInput => Error::new(
            ErrorCode::PermissionDenied,
            "Managed path failed containment/type validation",
        ),
        _ => error.into(),
    }
}
pub struct Store {
    config: AuthoringConfig,
    output: Directory,
    state: Directory,
    inputs: Option<Directory>,
}
impl Store {
    pub fn new(config: AuthoringConfig) -> Result<Self> {
        config.validate()?;
        let output = Directory::open(&config.output_root).map_err(file_error)?;
        let state = Directory::open(&config.state_root).map_err(file_error)?;
        let inputs = config
            .input_root
            .as_ref()
            .map(|p| Directory::open(p))
            .transpose()
            .map_err(file_error)?;
        Ok(Self {
            config,
            output,
            state,
            inputs,
        })
    }
    pub fn project_path(&self, slug: &str) -> Result<PathBuf> {
        validate::id(slug).map_err(contract)?;
        Ok(self.config.output_root.join(slug))
    }
    pub fn snapshot(&self, slug: &str) -> Result<Snapshot> {
        validate::id(slug).map_err(contract)?;
        let state_bytes = self
            .state
            .read(&format!("{slug}.json"), 524_288)
            .map_err(file_error)?;
        let state_digest = state_bytes.as_deref().map(Digest::of_bytes);
        let journal: Option<Journal> = state_bytes
            .as_deref()
            .map(strict_decode)
            .transpose()
            .map_err(contract)?;
        if let Some(journal) = &journal {
            if journal.version != 1
                || journal.target.slug != slug
                || journal.target.generator != GENERATOR_VERSION
            {
                return Err(conflict("Unsupported or mismatched derivation journal"));
            }
            validate::validate(&journal.target.intent).map_err(contract)?;
            if journal.target.files.len() > MAX_FILES || journal.target.bindings.len() > 4096 {
                return Err(Error::new(
                    ErrorCode::ResourceExhausted,
                    "Derivation manifest budget",
                ));
            }
        }
        let entries = self.output.entries().map_err(file_error)?;
        let exists = entries.iter().any(|entry| entry == slug);
        let directory = if exists {
            Some(self.output.child(slug, false).map_err(file_error)?)
        } else if let Some(stage) = journal.as_ref().and_then(|j| j.staging.as_ref()) {
            if !stage.starts_with(".sw-stage-") {
                return Err(conflict("Invalid staging journal"));
            }
            if entries.contains(stage) {
                Some(self.output.child(stage, false).map_err(file_error)?)
            } else {
                None
            }
        } else {
            None
        };
        let mut files = Vec::new();
        let mut bytes_read = 0u64;
        if let Some(journal) = &journal {
            for (path, file) in &journal.target.files {
                let bytes = directory
                    .as_ref()
                    .map(|d| d.read(path, MAX_BYTES))
                    .transpose()
                    .map_err(file_error)?
                    .flatten();
                bytes_read += bytes.as_ref().map_or(0, |v| v.len() as u64);
                if bytes_read > MAX_BYTES {
                    return Err(Error::new(
                        ErrorCode::ResourceExhausted,
                        "Managed observation byte budget",
                    ));
                }
                let actual = bytes.as_deref().map(Digest::of_bytes);
                let previous = journal
                    .previous
                    .as_ref()
                    .and_then(|r| r.files.get(path))
                    .map(|f| &f.sha256);
                let state = if actual.as_ref() == Some(&file.sha256) {
                    "current"
                } else if journal.pending && actual.as_ref() == previous {
                    "pending"
                } else if actual.is_none() {
                    "missing"
                } else {
                    "diverged"
                };
                files.push(FileObservation {
                    path: path.clone(),
                    expected: Some(file.sha256.clone()),
                    actual,
                    state: state.into(),
                });
            }
            let bytes = directory
                .as_ref()
                .map(|d| d.read(MANIFEST, 524_288))
                .transpose()
                .map_err(file_error)?
                .flatten();
            let actual = bytes.as_deref().map(Digest::of_bytes);
            let previous = journal.previous.as_ref().map(|r| &r.manifest_digest);
            let state = if actual.as_ref() == Some(&journal.target.manifest_digest) {
                "current"
            } else if journal.pending && actual.as_ref() == previous {
                "pending"
            } else if actual.is_none() {
                "missing"
            } else {
                "diverged"
            };
            files.push(FileObservation {
                path: MANIFEST.into(),
                expected: Some(journal.target.manifest_digest.clone()),
                actual,
                state: state.into(),
            });
        }
        let status = if journal.is_none() {
            if exists { "UNMANAGED" } else { "EMPTY" }
        } else if files.iter().any(|f| f.state == "diverged") {
            "DIVERGED"
        } else if journal.as_ref().is_some_and(|j| j.pending) {
            "PARTIAL"
        } else if files.iter().any(|f| f.state != "current") {
            "DIVERGED"
        } else {
            "IN_SYNC"
        }
        .to_owned();
        let fingerprint =
            canonical_digest(&(&state_digest, exists, &files, &status)).map_err(contract)?;
        Ok(Snapshot {
            journal,
            state_digest,
            fingerprint,
            files,
            status,
            exists,
        })
    }
    pub(crate) fn execution_files(
        &self,
        slug: &str,
    ) -> Result<(Snapshot, BTreeMap<String, Vec<u8>>)> {
        let snapshot = self.snapshot(slug)?;
        if snapshot.status != "IN_SYNC" {
            return Err(conflict(
                "Managed project must be IN_SYNC before native execution",
            ));
        }
        let record = snapshot
            .record()
            .ok_or_else(|| conflict("Managed project ownership record is unavailable"))?;
        let directory = self.output.child(slug, false).map_err(file_error)?;
        let mut total = 0u64;
        let mut files = BTreeMap::new();
        for (path, file) in &record.files {
            if !file.active {
                continue;
            }
            let bytes = directory
                .read(path, MAX_BYTES)
                .map_err(file_error)?
                .ok_or_else(|| conflict("Managed execution source disappeared"))?;
            total = total.checked_add(bytes.len() as u64).ok_or_else(|| {
                Error::new(
                    ErrorCode::ResourceExhausted,
                    "Managed execution byte overflow",
                )
            })?;
            if total > MAX_BYTES {
                return Err(Error::new(
                    ErrorCode::ResourceExhausted,
                    "Managed execution byte budget",
                ));
            }
            if Digest::of_bytes(&bytes) != file.sha256 {
                return Err(Error::new(
                    ErrorCode::StaleReference,
                    "Managed execution source changed after observation",
                ));
            }
            files.insert(path.clone(), bytes);
        }
        if !files.contains_key("project.godot") {
            return Err(conflict(
                "Managed execution source is missing project.godot",
            ));
        }
        Ok((snapshot, files))
    }

    pub fn prepare(
        &self,
        spec: &GodotAuthoringSpec,
        repair_missing: bool,
        recover: bool,
    ) -> Result<PreparedFiles> {
        validate::validate(spec).map_err(contract)?;
        let before = self.snapshot(&spec.project)?;
        if before.status == "UNMANAGED" {
            return Err(conflict(
                "Output directory exists without provider ownership; no overwrite",
            ));
        }
        if before.files.iter().any(|f| f.state == "diverged") {
            return Err(conflict("DIVERGED: managed source was edited externally"));
        }
        if before.status == "DIVERGED" && !repair_missing {
            return Err(conflict(
                "Missing managed sources require an explicit repair plan",
            ));
        }
        if before.pending() && !recover {
            return Err(conflict(
                "Partial publication requires explicit reconciliation",
            ));
        }
        if recover && !before.pending() {
            return Err(Error::invalid("No partial journal to reconcile"));
        }
        if (recover || repair_missing)
            && before
                .record()
                .is_some_and(|r| canonical_digest(spec).ok().as_ref() != Some(&r.intent_digest))
        {
            return Err(conflict(
                "Repair/reconciliation cannot change the pinned intent",
            ));
        }
        let old = before.record();
        let project = old.map(|r| r.project.clone()).unwrap_or_default();
        let previous_keys: std::collections::BTreeSet<String> = old
            .into_iter()
            .flat_map(|r| r.intent.scenes.iter())
            .flat_map(|scene| {
                std::iter::once(format!("scene:{}", scene.id))
                    .chain(
                        scene
                            .entities
                            .iter()
                            .map(|e| format!("entity:{}/{}", scene.id, e.id)),
                    )
                    .chain(
                        scene
                            .materials
                            .iter()
                            .map(|material| format!("material:{}/{}", scene.id, material.id)),
                    )
                    .chain(
                        scene
                            .animation_graphs
                            .iter()
                            .map(|graph| format!("animation_graph:{}/{}", scene.id, graph.id)),
                    )
            })
            .collect();
        let mut bindings: BTreeMap<String, LogicalAssetId> = old
            .map(|r| {
                r.bindings
                    .iter()
                    .filter(|(key, _)| previous_keys.contains(*key))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect()
            })
            .unwrap_or_default();
        for scene in &spec.scenes {
            bindings.entry(format!("scene:{}", scene.id)).or_default();
            for entity in &scene.entities {
                bindings
                    .entry(format!("entity:{}/{}", scene.id, entity.id))
                    .or_default();
            }
            for material in &scene.materials {
                bindings
                    .entry(format!("material:{}/{}", scene.id, material.id))
                    .or_default();
            }
            for graph in &scene.animation_graphs {
                bindings
                    .entry(format!("animation_graph:{}/{}", scene.id, graph.id))
                    .or_default();
            }
        }
        let mut compiled = compiler::compile(spec).map_err(contract)?;
        bind_native_identities(spec, &bindings, &mut compiled)?;
        let mut bytes: BTreeMap<_, _> = compiled
            .files
            .iter()
            .map(|(k, v)| (k.clone(), v.as_bytes().to_vec()))
            .collect();
        bytes.insert(
            "authoring-spec.json".into(),
            semwright_semantic_composition::canonical_bytes(spec).map_err(contract)?,
        );
        for asset in &spec.assets {
            let input = self.inputs.as_ref().ok_or_else(|| {
                Error::new(
                    ErrorCode::PermissionDenied,
                    "Local input root is not granted",
                )
            })?;
            let data = input
                .read(&asset.file, 16_777_216)
                .map_err(file_error)?
                .ok_or_else(|| {
                    Error::new(ErrorCode::NotFound, "Declared input asset is missing")
                })?;
            if Digest::of_bytes(&data) != asset.sha256 {
                return Err(conflict("Declared input asset hash changed"));
            }
            validate_asset(asset.kind, &data)?;
            bytes.insert(format!("assets/{}", asset.file), data);
        }
        if bytes.len() > MAX_FILES
            || bytes.values().map(|v| v.len() as u64).sum::<u64>() > MAX_BYTES
        {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Managed project generation budget",
            ));
        }
        let mut files = old.map(|r| r.files.clone()).unwrap_or_default();
        for file in files.values_mut() {
            file.active = false;
        }
        let mut writes = Vec::new();
        let directory = if before.exists {
            Some(
                self.output
                    .child(&spec.project, false)
                    .map_err(file_error)?,
            )
        } else {
            None
        };
        for (path, data) in &bytes {
            let sha256 = Digest::of_bytes(data);
            let previous = old.and_then(|r| r.files.get(path));
            if previous.is_none()
                && directory
                    .as_ref()
                    .map(|d| d.read(path, MAX_BYTES))
                    .transpose()
                    .map_err(file_error)?
                    .flatten()
                    .is_some()
            {
                return Err(conflict("New managed path collides with an unowned file"));
            }
            let actual = before
                .files
                .iter()
                .find(|f| f.path == *path)
                .and_then(|f| f.actual.as_ref());
            if actual != Some(&sha256) {
                writes.push(path.clone());
            }
            let same_bytes = previous.is_some_and(|file| file.sha256 == sha256);
            let asset = previous
                .filter(|f| f.active)
                .map(|f| f.asset.clone())
                .unwrap_or_default();
            let revision = previous
                .filter(|f| same_bytes && f.active)
                .map(|f| f.revision.clone())
                .unwrap_or_default();
            let provenance = compiled.provenance.get(path);
            files.insert(
                path.clone(),
                ManagedFile {
                    asset,
                    revision,
                    sha256,
                    active: true,
                    kind: provenance.map_or_else(
                        || {
                            if path.starts_with("assets/") {
                                "supplied_asset".into()
                            } else {
                                "intent".into()
                            }
                        },
                        |f| f.kind.clone(),
                    ),
                    logical_key: provenance.map_or_else(|| path.clone(), |f| f.logical_id.clone()),
                },
            );
        }
        let intent_digest = canonical_digest(spec).map_err(contract)?;
        let changed = !writes.is_empty() || old.is_none_or(|r| r.intent_digest != intent_digest);
        let revision = old.map_or(1, |r| {
            if changed && !recover {
                r.revision.saturating_add(1)
            } else {
                r.revision
            }
        });
        let manifest = semwright_semantic_composition::canonical_bytes(&serde_json::json!({"version":1,"project":project,"revision":revision,"generator":GENERATOR_VERSION,"intent_digest":intent_digest,"bindings":bindings,"files":files})).map_err(contract)?;
        if files.len() > MAX_FILES {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Retained managed-file budget; explicit cleanup required",
            ));
        }
        let manifest_digest = Digest::of_bytes(&manifest);
        if before
            .files
            .iter()
            .find(|f| f.path == MANIFEST)
            .and_then(|f| f.actual.as_ref())
            != Some(&manifest_digest)
        {
            writes.push(MANIFEST.into());
        }
        let target = DerivationRecord {
            project,
            slug: spec.project.clone(),
            revision,
            generator: GENERATOR_VERSION.into(),
            intent: spec.clone(),
            intent_digest,
            files,
            bindings,
            manifest_digest,
        };
        // Prove that the bounded journal is serializable before any output write.
        let check = Journal {
            version: 1,
            target: target.clone(),
            previous: old.cloned(),
            pending: true,
            staging: None,
        };
        semwright_semantic_composition::canonical_bytes(&check).map_err(contract)?;
        Ok(PreparedFiles {
            before,
            target,
            bytes,
            writes,
            manifest,
            recovery: recover,
        })
    }
    pub fn apply(
        &self,
        prepared: &PreparedFiles,
        check_cancelled: impl Fn() -> Result<()>,
    ) -> Result<WriteReceipt> {
        let slug = &prepared.target.slug;
        let _guard = self
            .state
            .lock(&format!("{slug}.lock"))
            .map_err(file_error)?;
        check_cancelled()?;
        let current = self.snapshot(slug)?;
        if current.fingerprint != prepared.before.fingerprint {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Authoring base changed after plan",
            ));
        }
        if prepared.writes.is_empty() && !current.pending() {
            return Ok(WriteReceipt {
                project: prepared.target.project.clone(),
                revision: prepared.target.revision,
                written: vec![],
                unchanged: prepared.target.files.keys().cloned().collect(),
                post_fingerprint: current.fingerprint,
                source_state: current.status,
            });
        }
        let state_path = format!("{slug}.json");
        let stage = if current.exists {
            None
        } else {
            Some(
                current
                    .journal
                    .as_ref()
                    .and_then(|j| j.staging.clone())
                    .unwrap_or_else(|| format!(".sw-stage-{}", semwright_types::unique_id())),
            )
        };
        let previous = if current.pending() {
            current.journal.as_ref().and_then(|j| j.previous.clone())
        } else {
            current.record().cloned()
        };
        let pending = Journal {
            version: 1,
            target: prepared.target.clone(),
            previous,
            pending: true,
            staging: stage.clone(),
        };
        let pending_bytes =
            semwright_semantic_composition::canonical_bytes(&pending).map_err(contract)?;
        self.state
            .write(&state_path, &pending_bytes, current.state_digest.as_ref())
            .map_err(file_error)?;
        let pending_digest = Digest::of_bytes(&pending_bytes);
        let operation = || -> Result<WriteReceipt> {
            check_cancelled()?;
            let directory = if let Some(stage) = &stage {
                if self.output.entries().map_err(file_error)?.contains(stage) {
                    self.output.child(stage, false).map_err(file_error)?
                } else {
                    self.output.create_child(stage).map_err(file_error)?
                }
            } else {
                self.output.child(slug, false).map_err(file_error)?
            };
            let mut written = Vec::new();
            for path in &prepared.writes {
                check_cancelled()?;
                let data = if path == MANIFEST {
                    &prepared.manifest
                } else {
                    prepared.bytes.get(path).ok_or_else(|| {
                        Error::new(ErrorCode::Internal, "Prepared file disappeared")
                    })?
                };
                let expected = current
                    .files
                    .iter()
                    .find(|f| f.path == *path)
                    .and_then(|f| f.actual.as_ref());
                directory.write(path, data, expected).map_err(file_error)?;
                written.push(path.clone());
            }
            for (path, file) in &prepared.target.files {
                check_cancelled()?;
                let actual = directory
                    .read(path, MAX_BYTES)
                    .map_err(file_error)?
                    .as_deref()
                    .map(Digest::of_bytes);
                if actual.as_ref() != Some(&file.sha256) {
                    return Err(conflict(
                        "Concurrent managed-file change during publication",
                    ));
                }
            }
            if directory
                .read(MANIFEST, 524_288)
                .map_err(file_error)?
                .as_deref()
                .map(Digest::of_bytes)
                .as_ref()
                != Some(&prepared.target.manifest_digest)
            {
                return Err(conflict("Concurrent derivation manifest change"));
            }
            if let Some(stage) = &stage {
                self.output.publish_child(stage, slug).map_err(file_error)?;
            }
            let completed = Journal {
                version: 1,
                target: prepared.target.clone(),
                previous: None,
                pending: false,
                staging: None,
            };
            let completed_bytes =
                semwright_semantic_composition::canonical_bytes(&completed).map_err(contract)?;
            self.state
                .write(&state_path, &completed_bytes, Some(&pending_digest))
                .map_err(file_error)?;
            let after = self.snapshot(slug)?;
            if after.status != "IN_SYNC" {
                return Err(conflict("Concurrent source change after publication"));
            }
            let unchanged = prepared
                .target
                .files
                .keys()
                .filter(|p| !written.contains(p))
                .cloned()
                .collect();
            Ok(WriteReceipt {
                project: prepared.target.project.clone(),
                revision: prepared.target.revision,
                written,
                unchanged,
                post_fingerprint: after.fingerprint,
                source_state: after.status,
            })
        };
        operation().map_err(Error::uncertain)
    }
}

fn bind_native_identities(
    spec: &GodotAuthoringSpec,
    bindings: &BTreeMap<String, LogicalAssetId>,
    compiled: &mut compiler::CompiledProject,
) -> Result<()> {
    for scene in &spec.scenes {
        let path = format!("scenes/{}.tscn", scene.id);
        let text = compiled
            .files
            .get_mut(&path)
            .ok_or_else(|| Error::new(ErrorCode::Internal, "Compiled scene missing"))?;
        for (key, binding_key) in std::iter::once((scene.id.clone(), format!("scene:{}", scene.id)))
            .chain(scene.entities.iter().map(|e| {
                (
                    format!("{}/{}", scene.id, e.id),
                    format!("entity:{}/{}", scene.id, e.id),
                )
            }))
            .chain(scene.animation_graphs.iter().map(|graph| {
                (
                    format!("animation_graph/{}/{}", scene.id, graph.id),
                    format!("animation_graph:{}/{}", scene.id, graph.id),
                )
            }))
        {
            let id = bindings.get(&binding_key).ok_or_else(|| {
                Error::new(ErrorCode::Internal, "Native identity binding missing")
            })?;
            let old = format!("metadata/semwright_logical_id={}\n", compiler::quoted(&key));
            let new = format!(
                "metadata/semwright_logical_key={}\nmetadata/semwright_logical_id={}\n",
                compiler::quoted(&key),
                compiler::quoted(id.as_str())
            );
            if text.matches(&old).count() != 1 {
                return Err(Error::new(
                    ErrorCode::Internal,
                    "Native metadata codegen mismatch",
                ));
            }
            *text = text.replace(&old, &new);
        }
        compiled
            .provenance
            .get_mut(&path)
            .ok_or_else(|| Error::new(ErrorCode::Internal, "Scene provenance missing"))?
            .sha256 = Digest::of_bytes(text.as_bytes());
    }
    Ok(())
}
fn validate_asset(kind: AssetKind, bytes: &[u8]) -> Result<()> {
    if bytes.len() > 16_777_216 {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Input asset byte budget",
        ));
    }
    match kind {
        AssetKind::Glb => {
            if bytes.len() < 20
                || &bytes[..4] != b"glTF"
                || bytes[4..8] != 2u32.to_le_bytes()
                || bytes[8..12] != (bytes.len() as u32).to_le_bytes()
                || &bytes[16..20] != b"JSON"
            {
                return Err(Error::invalid(
                    "Expected a self-contained GLB version 2 asset",
                ));
            }
            let length = u32::from_le_bytes(
                bytes[12..16]
                    .try_into()
                    .map_err(|_| Error::invalid("GLB chunk length"))?,
            ) as usize;
            if length > 524_288 || !length.is_multiple_of(4) || 20 + length > bytes.len() {
                return Err(Error::invalid("GLB JSON chunk bounds"));
            }
            let json: serde_json::Value =
                strict_decode(&bytes[20..20 + length]).map_err(contract)?;
            let mut stack = vec![(&json, 0usize)];
            let mut count = 0;
            while let Some((value, depth)) = stack.pop() {
                count += 1;
                if depth > 32 || count > 16_384 {
                    return Err(Error::invalid("GLB JSON structural budget"));
                }
                match value {
                    serde_json::Value::Object(object) => {
                        if object.contains_key("uri") {
                            return Err(Error::new(
                                ErrorCode::PermissionDenied,
                                "External and data URI GLB dependencies are not accepted; embed buffer views",
                            ));
                        }
                        stack.extend(object.values().map(|v| (v, depth + 1)));
                    }
                    serde_json::Value::Array(values) => {
                        stack.extend(values.iter().map(|v| (v, depth + 1)))
                    }
                    _ => {}
                }
            }
            let binary = 20 + length;
            if binary < bytes.len() {
                if binary + 8 > bytes.len() || &bytes[binary + 4..binary + 8] != b"BIN\0" {
                    return Err(Error::invalid("GLB binary chunk type"));
                }
                let size = u32::from_le_bytes(
                    bytes[binary..binary + 4]
                        .try_into()
                        .map_err(|_| Error::invalid("GLB binary length"))?,
                ) as usize;
                if binary + 8 + size != bytes.len() || !size.is_multiple_of(4) {
                    return Err(Error::invalid("GLB binary bounds"));
                }
            }
        }
        AssetKind::Texture => {
            if bytes.len() < 33 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" || &bytes[12..16] != b"IHDR"
            {
                return Err(Error::invalid("Expected a bounded PNG asset"));
            }
            let width = u32::from_be_bytes(
                bytes[16..20]
                    .try_into()
                    .map_err(|_| Error::invalid("PNG width"))?,
            );
            let height = u32::from_be_bytes(
                bytes[20..24]
                    .try_into()
                    .map_err(|_| Error::invalid("PNG height"))?,
            );
            if !(1..=4096).contains(&width) || !(1..=4096).contains(&height) {
                return Err(Error::invalid("PNG dimension budget"));
            }
        }
        AssetKind::Audio => {
            let wav = bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WAVE";
            let ogg = bytes.len() >= 27 && &bytes[..4] == b"OggS";
            if !wav && !ogg {
                return Err(Error::invalid("Expected a WAV or Ogg local audio asset"));
            }
        }
    }
    Ok(())
}
