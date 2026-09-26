//! Driver Protocol v1 adapter for bounded, Semwright-managed Motion Canvas projects.
use crate::{
    Result,
    diff::SemanticDiff,
    edit::{self, Operation},
    model::*,
    refs::{self, Kind, ObjectRef, Reference},
    renderer::{JobView, RenderManager, RenderState, RendererRuntime},
    security,
    semantic::{self, NodeTypeDescriptor},
    store::{ProjectStore, Snapshot},
    validate::{self, RenderPlan},
};
use async_trait::async_trait;
use schemars::{JsonSchema, schema_for};
use semwright_driver_sdk::{
    Capability, Driver, DriverExecutionContext, DriverInterfaces, artifact_output_tag,
    descriptor_digest,
};
use semwright_types::{
    CommandDescriptor, Error, ErrorCode, Idempotency, JobArtifact, JobProgress, Risk,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

const ID: &str = "motion-canvas";
const SCOPE: &str = "driver:motion-canvas";

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct EmptyArgs {}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CreateArgs {
    project: Project,
    #[serde(default)]
    dry_run: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ApplyArgs {
    expected_fingerprint: String,
    operations: Vec<Operation>,
    #[serde(default)]
    dry_run: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AssetImportArgs {
    expected_fingerprint: String,
    id: String,
    kind: AssetKind,
    source: String,
    #[serde(default)]
    provenance: Option<String>,
    #[serde(default)]
    license: Option<String>,
    #[serde(default)]
    dry_run: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SceneScopeArgs {
    #[serde(default)]
    scene_ref: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SemanticDescribeArgs {
    kind: NodeKind,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct NodeInspectArgs {
    node_ref: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct NodePropertyGetArgs {
    node_ref: String,
    property: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
enum NodePropertyEdit {
    Set { value: Value },
    Reset,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct NodePropertySetArgs {
    expected_fingerprint: String,
    node_ref: String,
    property: String,
    edit: NodePropertyEdit,
    #[serde(default)]
    dry_run: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RenderPlanArgs {
    profile: RenderProfile,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RenderStartArgs {
    expected_fingerprint: String,
    profile: RenderProfile,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct JobArgs {
    job_ref: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct DoctorOutput {
    driver_version: String,
    motion_canvas_version: String,
    node_version: String,
    managed_schema_version: u32,
    renderer_mode: String,
    network: bool,
    project_mounted: bool,
    media_mounted: bool,
    output_mounted: bool,
    runtime_mounted: bool,
    render_available: bool,
    renderer_reason: String,
    active_jobs: usize,
    capability_count: usize,
}
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SnapshotOutput {
    project: Project,
    fingerprint: String,
    refs: Vec<ObjectRef>,
    generated: Vec<crate::compiler::GeneratedFile>,
}
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ValidationOutput {
    valid: bool,
    fingerprint: String,
    revision: u64,
    generated_fingerprint: String,
}
#[derive(Debug, Clone, Copy, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ProjectMode {
    Managed,
    ManagedIsland,
    External,
    Empty,
}
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ProjectDetectOutput {
    mode: ProjectMode,
    package_json: bool,
    project_entry: bool,
    motion_canvas_version: Option<String>,
    exact_runtime_match: bool,
    mutation_supported: bool,
    managed_root: Option<String>,
    bridge: Option<String>,
}
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct MutationOutput {
    applied: bool,
    source_fingerprint: Option<String>,
    resulting_fingerprint: String,
    revision: u64,
    diff: SemanticDiff,
    generated: Vec<crate::compiler::GeneratedFile>,
    refs: Vec<ObjectRef>,
}
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct IslandCreateOutput {
    applied: bool,
    managed_root: String,
    semantic_file: String,
    bridge: String,
    import_example: Option<String>,
    mutation: MutationOutput,
}
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SceneItem {
    id: String,
    name: String,
    duration_ms: u64,
    reference: String,
}
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct NodeItem {
    id: String,
    name: String,
    kind: NodeKind,
    parent: Option<String>,
    reference: String,
}
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SemanticTypesOutput {
    motion_canvas_version: String,
    managed_type_count: usize,
    items: Vec<NodeTypeDescriptor>,
}
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct NodeInspectOutput {
    fingerprint: String,
    revision: u64,
    reference: String,
    node: Node,
    semantic_type: NodeTypeDescriptor,
}
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct NodePropertyOutput {
    fingerprint: String,
    revision: u64,
    node_ref: String,
    descriptor: crate::semantic::PropertyDescriptor,
    is_set: bool,
    value: Value,
}
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AssetItem {
    id: String,
    kind: AssetKind,
    path: String,
    sha256: String,
    bytes: u64,
    reference: String,
}
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CueItem {
    id: String,
    name: String,
    at_ms: u64,
    reference: String,
}
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AnimationItem {
    id: String,
    target: String,
    property: AnimatedProperty,
    reference: String,
}
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ListOutput<T> {
    fingerprint: String,
    revision: u64,
    items: Vec<T>,
}

fn imported_asset(
    id: &str,
    kind: AssetKind,
    source: &str,
    bytes: &[u8],
    provenance: Option<String>,
    license: Option<String>,
) -> Result<Asset> {
    if bytes.is_empty() || bytes.len() > MAX_ASSET_BYTES {
        return Err(Error::invalid("Imported asset byte size is invalid"));
    }
    let extension = Path::new(source)
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| Error::invalid("Imported asset requires a supported file extension"))?;
    let dimensions = match kind {
        AssetKind::Image if extension == "png" => {
            let evidence = security::inspect_png(bytes)?;
            Some([evidence.width, evidence.height])
        }
        AssetKind::Svg if extension == "svg" => {
            let text = std::str::from_utf8(bytes)
                .map_err(|_| Error::invalid("SVG asset must be UTF-8"))?;
            security::validate_svg(text)?;
            None
        }
        AssetKind::Video if extension == "mp4" => {
            if bytes.len() < 12 || &bytes[4..8] != b"ftyp" {
                return Err(Error::invalid(
                    "MP4 asset has an invalid container signature",
                ));
            }
            None
        }
        AssetKind::Video if extension == "webm" => {
            if !bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
                return Err(Error::invalid(
                    "WebM asset has an invalid container signature",
                ));
            }
            None
        }
        AssetKind::Audio if extension == "wav" => {
            if bytes.len() < 12 || !bytes.starts_with(b"RIFF") || &bytes[8..12] != b"WAVE" {
                return Err(Error::invalid(
                    "WAV asset has an invalid container signature",
                ));
            }
            None
        }
        AssetKind::Audio if extension == "ogg" => {
            if !bytes.starts_with(b"OggS") {
                return Err(Error::invalid(
                    "Ogg asset has an invalid container signature",
                ));
            }
            None
        }
        AssetKind::Audio if extension == "mp3" => {
            let frame_sync = bytes.len() >= 2 && bytes[0] == 0xff && bytes[1] & 0xe0 == 0xe0;
            if !bytes.starts_with(b"ID3") && !frame_sync {
                return Err(Error::invalid("MP3 asset has an invalid stream signature"));
            }
            None
        }
        _ => {
            return Err(Error::invalid(
                "Asset kind and extension are not in the managed import allowlist",
            ));
        }
    };
    Ok(Asset {
        id: id.into(),
        kind,
        path: format!("assets/{id}.{extension}"),
        sha256: security::sha256(bytes),
        bytes: bytes.len() as u64,
        dimensions,
        duration_ms: None,
        provenance,
        license,
    })
}

fn external_motion_canvas_version(bytes: &[u8]) -> Result<Option<String>> {
    let package: Value = serde_json::from_slice(bytes)?;
    let object = package
        .as_object()
        .ok_or_else(|| Error::invalid("External package.json must contain an object"))?;
    for section in ["dependencies", "devDependencies"] {
        if let Some(dependencies) = object.get(section).and_then(Value::as_object)
            && let Some(value) = dependencies.get("@motion-canvas/core")
        {
            let version = value.as_str().ok_or_else(|| {
                Error::invalid("Motion Canvas dependency version must be a string")
            })?;
            if version.is_empty() || version.len() > 64 || version.chars().any(char::is_control) {
                return Err(Error::invalid(
                    "Motion Canvas dependency version exceeds bounds",
                ));
            }
            return Ok(Some(version.into()));
        }
    }
    Ok(None)
}

pub struct MotionDriver {
    roots: BTreeMap<String, PathBuf>,
    store: Option<ProjectStore>,
    renderer: RenderManager,
    renderer_reason: String,
}
impl MotionDriver {
    fn store_for_project_root(root: &Path) -> Result<ProjectStore> {
        let island = root.join(crate::store::ISLAND_DIR);
        if !root.join(crate::store::SEMANTIC_FILE).try_exists()?
            && island.join(crate::store::SEMANTIC_FILE).try_exists()?
        {
            ProjectStore::open_island(fs::canonicalize(island)?)
        } else {
            ProjectStore::open(root)
        }
    }

    pub fn production() -> Result<Self> {
        let roots: BTreeMap<String, PathBuf> = [
            ("project", "/workspace/project"),
            ("media", "/workspace/media"),
            ("output", "/workspace/output"),
            ("runtime", "/workspace/runtime"),
        ]
        .into_iter()
        .filter(|(_, path)| Path::new(path).is_dir())
        .map(|(name, path)| (name.into(), PathBuf::from(path)))
        .collect();
        let store = roots
            .get("project")
            .map(|root| Self::store_for_project_root(root))
            .transpose()?;
        let (runtime, renderer_reason) = match roots.get("runtime") {
            Some(root) => match RendererRuntime::from_root(root) {
                Ok(runtime) => (
                    Some(runtime),
                    "Pinned runtime tools verified by SHA-256".into(),
                ),
                Err(error) => (
                    None,
                    format!(
                        "Runtime unavailable: {:?}: {}",
                        error.code,
                        error
                            .message
                            .chars()
                            .filter(|ch| !ch.is_control())
                            .take(512)
                            .collect::<String>()
                    ),
                ),
            },
            None => (None, "Owner-approved runtime mount is absent".into()),
        };
        let output = roots
            .get("output")
            .cloned()
            .unwrap_or_else(|| PathBuf::from("/workspace/output"));
        let renderer = RenderManager::new(runtime, output);
        Ok(Self {
            roots,
            store,
            renderer,
            renderer_reason,
        })
    }
    #[cfg(test)]
    pub fn for_project_root(root: &Path) -> Result<Self> {
        let mut roots = BTreeMap::new();
        roots.insert("project".into(), root.to_path_buf());
        let renderer = RenderManager::new(None, root.join("test-output"));
        Ok(Self {
            roots,
            store: Some(Self::store_for_project_root(root)?),
            renderer,
            renderer_reason: "Test harness has no owner-approved render runtime".into(),
        })
    }
    #[cfg(test)]
    pub fn for_project_and_media_roots(project: &Path, media: &Path) -> Result<Self> {
        let mut driver = Self::for_project_root(project)?;
        driver.roots.insert("media".into(), media.to_path_buf());
        Ok(driver)
    }
    fn store(&self) -> Result<&ProjectStore> {
        self.store
            .as_ref()
            .ok_or_else(|| Error::new(ErrorCode::Unavailable, "Project grant is not mounted"))
    }
    fn load(&self) -> Result<Snapshot> {
        self.store()?.load()
    }
    fn optional_project_file(&self, relative: &str, limit: usize) -> Result<Option<Vec<u8>>> {
        let root = self
            .roots
            .get("project")
            .ok_or_else(|| Error::new(ErrorCode::Unavailable, "Project grant is not mounted"))?;
        match crate::store::read_granted_file(root, relative, limit) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.code == ErrorCode::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }
    fn detect_project(&self) -> Result<ProjectDetectOutput> {
        let semantic =
            self.optional_project_file(crate::store::SEMANTIC_FILE, MAX_PROJECT_BYTES)?;
        let island_semantic = self.optional_project_file(
            &format!(
                "{}/{}",
                crate::store::ISLAND_DIR,
                crate::store::SEMANTIC_FILE
            ),
            MAX_PROJECT_BYTES,
        )?;
        let package = self.optional_project_file("package.json", 65_536)?;
        let project_entry = self
            .optional_project_file("src/project.ts", 262_144)?
            .is_some();
        let version = package
            .as_deref()
            .map(external_motion_canvas_version)
            .transpose()?
            .flatten();
        if let Some(bytes) = semantic {
            validate::parse(&bytes)?;
            return Ok(ProjectDetectOutput {
                mode: ProjectMode::Managed,
                package_json: package.is_some(),
                project_entry,
                motion_canvas_version: Some(MOTION_CANVAS_VERSION.into()),
                exact_runtime_match: true,
                mutation_supported: true,
                managed_root: Some(".".into()),
                bridge: None,
            });
        }
        if let Some(bytes) = island_semantic {
            validate::parse(&bytes)?;
            let exact = version.as_deref() == Some(MOTION_CANVAS_VERSION);
            return Ok(ProjectDetectOutput {
                mode: ProjectMode::ManagedIsland,
                package_json: package.is_some(),
                project_entry,
                motion_canvas_version: version,
                exact_runtime_match: exact,
                mutation_supported: exact && project_entry,
                managed_root: Some(crate::store::ISLAND_DIR.into()),
                bridge: Some(format!(
                    "{}/{}",
                    crate::store::ISLAND_DIR,
                    crate::store::ISLAND_BRIDGE
                )),
            });
        }
        Ok(ProjectDetectOutput {
            mode: if package.is_some() || project_entry {
                ProjectMode::External
            } else {
                ProjectMode::Empty
            },
            package_json: package.is_some(),
            project_entry,
            exact_runtime_match: version.as_deref() == Some(MOTION_CANVAS_VERSION),
            motion_canvas_version: version,
            mutation_supported: false,
            managed_root: None,
            bridge: None,
        })
    }

    fn ensure_island_runtime_compatible(&self) -> Result<()> {
        let Some(store) = &self.store else {
            return Err(Error::new(
                ErrorCode::Unavailable,
                "Project grant is not mounted",
            ));
        };
        if !store.is_island() {
            return Ok(());
        }
        let detected = self.detect_project()?;
        if detected.mode == ProjectMode::ManagedIsland && detected.mutation_supported {
            return Ok(());
        }
        Err(Error::new(
            ErrorCode::Conflict,
            "Managed island host is no longer an exact Motion Canvas 3.17.2 project; mutation and new renders are refused",
        ))
    }

    fn parse<T: DeserializeOwned>(args: Value) -> Result<T> {
        serde_json::from_value(args).map_err(Into::into)
    }
    fn schema<T: JsonSchema>() -> Result<Value> {
        Ok(serde_json::to_value(schema_for!(T))?)
    }
    fn cap<T: JsonSchema, O: JsonSchema>(
        name: &str,
        description: &str,
        risk: Risk,
        idempotency: Idempotency,
        dry_run: bool,
    ) -> Result<Capability> {
        Ok(Capability {
            descriptor: CommandDescriptor {
                name: name.into(),
                version: "1".into(),
                description: description.into(),
                input_schema: Self::schema::<T>()?,
                output_schema: Self::schema::<O>()?,
                requires: vec![SCOPE.into()],
                risk,
                idempotency,
                timeout_ms: 30_000,
                dry_run,
                interactive_consent: false,
                backends: vec![SCOPE.into()],
            },
            aliases: vec![],
            tags: vec!["motion-canvas".into(), "managed".into()],
            object_types: vec!["motion-project".into()],
        })
    }
    fn render_execute_capability() -> Result<Capability> {
        let mut capability = Self::cap::<RenderStartArgs, JobView>(
            "driver.motion-canvas.render.execute",
            "Render a bounded Motion Canvas frame sequence synchronously with protocol progress, artifacts and cooperative cancellation",
            Risk::MutatingReversible,
            Idempotency::NonIdempotent,
            false,
        )?;
        capability.descriptor.timeout_ms = 300_000;
        capability
            .tags
            .push(artifact_output_tag("image/png-sequence")?);
        Ok(capability)
    }

    fn catalog() -> Result<Vec<Capability>> {
        Ok(vec![
            Self::cap::<EmptyArgs, DoctorOutput>(
                "driver.motion-canvas.doctor",
                "Inspect bounded Motion Canvas driver and renderer prerequisites",
                Risk::ReadOnly,
                Idempotency::ReadOnly,
                true,
            )?,
            Self::cap::<EmptyArgs, SemanticTypesOutput>(
                "driver.motion-canvas.semantic.types",
                "List the version-pinned managed Motion Canvas node types and typed property surface",
                Risk::ReadOnly,
                Idempotency::ReadOnly,
                true,
            )?,
            Self::cap::<SemanticDescribeArgs, NodeTypeDescriptor>(
                "driver.motion-canvas.semantic.describe",
                "Describe one managed node type and its safe upstream property mapping",
                Risk::ReadOnly,
                Idempotency::ReadOnly,
                true,
            )?,
            Self::cap::<EmptyArgs, ProjectDetectOutput>(
                "driver.motion-canvas.project.detect",
                "Detect managed or external Motion Canvas project metadata without executing project code",
                Risk::ReadOnly,
                Idempotency::ReadOnly,
                true,
            )?,
            Self::cap::<EmptyArgs, SnapshotOutput>(
                "driver.motion-canvas.project.inspect",
                "Inspect the authoritative managed semantic project, refs and deterministic generated inventory",
                Risk::ReadOnly,
                Idempotency::ReadOnly,
                true,
            )?,
            Self::cap::<EmptyArgs, ValidationOutput>(
                "driver.motion-canvas.project.validate",
                "Validate the managed project and deterministic compiler without writing",
                Risk::ReadOnly,
                Idempotency::ReadOnly,
                true,
            )?,
            Self::cap::<CreateArgs, MutationOutput>(
                "driver.motion-canvas.project.create",
                "Create a managed semwright-motion.json project or truthfully dry-run creation",
                Risk::MutatingReversible,
                Idempotency::NonIdempotent,
                true,
            )?,
            Self::cap::<CreateArgs, IslandCreateOutput>(
                "driver.motion-canvas.project.island.create",
                "Create an isolated Semwright-managed scene island inside an exact-version external Motion Canvas project without editing human TypeScript",
                Risk::MutatingReversible,
                Idempotency::NonIdempotent,
                true,
            )?,
            Self::cap::<ApplyArgs, MutationOutput>(
                "driver.motion-canvas.project.diff",
                "Validate refs and compute semantic/generated impact without writing",
                Risk::ReadOnly,
                Idempotency::ReadOnly,
                true,
            )?,
            Self::cap::<ApplyArgs, MutationOutput>(
                "driver.motion-canvas.project.apply",
                "Apply an atomic bounded semantic transaction or dry-run it",
                Risk::MutatingReversible,
                Idempotency::NonIdempotent,
                true,
            )?,
            Self::cap::<EmptyArgs, ListOutput<SceneItem>>(
                "driver.motion-canvas.scene.list",
                "List managed scenes with revision-bound refs",
                Risk::ReadOnly,
                Idempotency::ReadOnly,
                true,
            )?,
            Self::cap::<SceneScopeArgs, ListOutput<NodeItem>>(
                "driver.motion-canvas.node.list",
                "List managed nodes, optionally scoped to a scene ref",
                Risk::ReadOnly,
                Idempotency::ReadOnly,
                true,
            )?,
            Self::cap::<NodeInspectArgs, NodeInspectOutput>(
                "driver.motion-canvas.node.inspect",
                "Inspect one revision-bound managed node together with its semantic type descriptor",
                Risk::ReadOnly,
                Idempotency::ReadOnly,
                true,
            )?,
            Self::cap::<NodePropertyGetArgs, NodePropertyOutput>(
                "driver.motion-canvas.node.property.get",
                "Read one canonical typed property from a revision-bound managed node",
                Risk::ReadOnly,
                Idempotency::ReadOnly,
                true,
            )?,
            Self::cap::<NodePropertySetArgs, MutationOutput>(
                "driver.motion-canvas.node.property.set",
                "Set or reset one registry-approved managed node property atomically",
                Risk::MutatingReversible,
                Idempotency::NonIdempotent,
                true,
            )?,
            Self::cap::<AssetImportArgs, MutationOutput>(
                "driver.motion-canvas.asset.import",
                "Import a bounded local media file from the owner-granted media root into the managed project",
                Risk::MutatingReversible,
                Idempotency::NonIdempotent,
                true,
            )?,
            Self::cap::<EmptyArgs, ListOutput<AssetItem>>(
                "driver.motion-canvas.asset.list",
                "List local managed media assets with hashes and refs",
                Risk::ReadOnly,
                Idempotency::ReadOnly,
                true,
            )?,
            Self::cap::<SceneScopeArgs, ListOutput<CueItem>>(
                "driver.motion-canvas.cue.list",
                "List named timing cues, optionally scoped to a scene ref",
                Risk::ReadOnly,
                Idempotency::ReadOnly,
                true,
            )?,
            Self::cap::<SceneScopeArgs, ListOutput<AnimationItem>>(
                "driver.motion-canvas.animation.list",
                "List declarative animations, optionally scoped to a scene ref",
                Risk::ReadOnly,
                Idempotency::ReadOnly,
                true,
            )?,
            Self::cap::<RenderPlanArgs, RenderPlan>(
                "driver.motion-canvas.render.plan",
                "Validate a bounded deterministic frame render plan",
                Risk::ReadOnly,
                Idempotency::ReadOnly,
                true,
            )?,
            Self::cap::<RenderStartArgs, JobView>(
                "driver.motion-canvas.render.start",
                "Start a bounded driver-local Motion Canvas render job",
                Risk::MutatingReversible,
                Idempotency::NonIdempotent,
                false,
            )?,
            Self::render_execute_capability()?,
            Self::cap::<JobArgs, JobView>(
                "driver.motion-canvas.render.status",
                "Inspect the observed phase of a driver-local render job",
                Risk::ReadOnly,
                Idempotency::ReadOnly,
                true,
            )?,
            Self::cap::<JobArgs, JobView>(
                "driver.motion-canvas.render.cancel",
                "Request cancellation of a driver-local render job and its child process tree",
                Risk::MutatingReversible,
                Idempotency::Idempotent,
                false,
            )?,
            Self::cap::<JobArgs, JobView>(
                "driver.motion-canvas.render.result",
                "Return validated render artifact metadata for a terminal job",
                Risk::ReadOnly,
                Idempotency::ReadOnly,
                true,
            )?,
        ])
    }
    fn ref_for(snapshot: &Snapshot, kind: Kind, id: &str) -> String {
        Reference::new(&snapshot.project, &snapshot.source_sha256, kind, id).encode()
    }
    fn checked_scene(snapshot: &Snapshot, value: &str) -> Result<String> {
        let reference = Reference::decode(value)?;
        reference.check(&snapshot.project, &snapshot.source_sha256, Kind::Scene)?;
        Ok(reference.id)
    }
    fn mutation_struct(
        source: Option<String>,
        snapshot: &Snapshot,
        diff: SemanticDiff,
        applied: bool,
    ) -> MutationOutput {
        MutationOutput {
            applied,
            source_fingerprint: source,
            resulting_fingerprint: snapshot.source_sha256.clone(),
            revision: snapshot.project.revision,
            diff,
            generated: snapshot.generated.inventory(),
            refs: refs::all(&snapshot.project, &snapshot.source_sha256),
        }
    }
    fn mutation_output(
        source: Option<String>,
        snapshot: &Snapshot,
        diff: SemanticDiff,
        applied: bool,
    ) -> Result<Value> {
        Ok(serde_json::to_value(Self::mutation_struct(
            source, snapshot, diff, applied,
        ))?)
    }
    fn prospective_output(
        source: Option<String>,
        prepared: edit::PreparedTransaction,
    ) -> Result<Value> {
        let fingerprint = security::sha256(&serde_json::to_vec_pretty(&prepared.project)?);
        let snapshot = Snapshot {
            project: prepared.project,
            source_sha256: fingerprint,
            generated: prepared.generated,
            generated_dir: None,
        };
        Self::mutation_output(source, &snapshot, prepared.diff, false)
    }
    fn checked_render_input(&self, args: Value) -> Result<RenderStartArgs> {
        Self::parse(args)
    }

    async fn start_render_from_input(&self, input: RenderStartArgs) -> Result<JobView> {
        self.ensure_island_runtime_compatible()?;
        let mut snapshot = self.load()?;
        if input.expected_fingerprint != snapshot.source_sha256 {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Managed project fingerprint changed; inspect again",
            ));
        }
        let generated = self.store()?.materialize(&snapshot)?;
        snapshot.generated_dir = Some(generated);
        self.renderer.start(&snapshot, input.profile).await
    }

    async fn execute_render_with_context(
        &self,
        input: RenderStartArgs,
        context: &DriverExecutionContext,
    ) -> Result<JobView> {
        context.check_cancelled()?;
        let started = self.start_render_from_input(input).await?;
        let job_ref = started.job_ref.clone();
        context.report_progress(
            JobProgress {
                completed: 0,
                total: None,
                message: Some("Motion Canvas render queued".into()),
            },
            vec![],
        )?;
        let cancellation = context.cancellation();
        let mut last_state = Some(started.state);
        loop {
            tokio::select! {
                _ = cancellation.cancelled() => {
                    let _ = self.renderer.cancel(&job_ref).await;
                    return Err(Error::new(ErrorCode::Cancelled, "Motion Canvas render cancelled by request"));
                }
                _ = tokio::time::sleep(Duration::from_millis(50)) => {}
            }
            let view = self.renderer.status(&job_ref).await?;
            if last_state.as_ref() != Some(&view.state) {
                last_state = Some(view.state.clone());
                let message = match view.state {
                    RenderState::Queued => "Motion Canvas render queued",
                    RenderState::Starting => "Motion Canvas renderer starting",
                    RenderState::Rendering => "Motion Canvas frames rendering",
                    RenderState::Succeeded => "Motion Canvas render succeeded",
                    RenderState::Failed => "Motion Canvas render failed",
                    RenderState::Cancelled => "Motion Canvas render cancelled",
                };
                context.report_progress(
                    JobProgress {
                        completed: u64::from(matches!(view.state, RenderState::Succeeded)),
                        total: matches!(view.state, RenderState::Succeeded).then_some(1),
                        message: Some(message.into()),
                    },
                    if let (RenderState::Succeeded, Some(artifact)) = (&view.state, &view.artifact)
                    {
                        vec![JobArtifact {
                            name: "motion-canvas-frame-sequence-manifest".into(),
                            reference: artifact.manifest.clone(),
                            media_type: Some("application/json".into()),
                            sha256: Some(artifact.manifest_sha256.clone()),
                            bytes: None,
                        }]
                    } else {
                        vec![]
                    },
                )?;
            }
            match view.state {
                RenderState::Succeeded => return Ok(view),
                RenderState::Cancelled => {
                    return Err(Error::new(
                        ErrorCode::Cancelled,
                        "Motion Canvas render cancelled",
                    ));
                }
                RenderState::Failed => {
                    return Err(Error::new(
                        ErrorCode::BackendFailed,
                        view.error
                            .unwrap_or_else(|| "Motion Canvas render failed".into()),
                    ));
                }
                RenderState::Queued | RenderState::Starting | RenderState::Rendering => {}
            }
        }
    }

    async fn dispatch(&mut self, command: &str, args: Value) -> Result<Value> {
        match command {
            "driver.motion-canvas.doctor" => {
                let _: EmptyArgs = Self::parse(args)?;
                Ok(serde_json::to_value(DoctorOutput {
                    driver_version: env!("CARGO_PKG_VERSION").into(),
                    motion_canvas_version: MOTION_CANVAS_VERSION.into(),
                    node_version: NODE_VERSION.into(),
                    managed_schema_version: SCHEMA_VERSION,
                    renderer_mode: "pinned_vite_playwright_harness".into(),
                    network: false,
                    project_mounted: self.roots.contains_key("project"),
                    media_mounted: self.roots.contains_key("media"),
                    output_mounted: self.roots.contains_key("output"),
                    runtime_mounted: self.roots.contains_key("runtime"),
                    render_available: self.renderer.available(),
                    renderer_reason: self.renderer_reason.clone(),
                    active_jobs: self.renderer.active_count().await,
                    capability_count: Self::catalog()?.len(),
                })?)
            }
            "driver.motion-canvas.semantic.types" => {
                let _: EmptyArgs = Self::parse(args)?;
                let items = semantic::node_types();
                Ok(serde_json::to_value(SemanticTypesOutput {
                    motion_canvas_version: MOTION_CANVAS_VERSION.into(),
                    managed_type_count: items.len(),
                    items,
                })?)
            }
            "driver.motion-canvas.semantic.describe" => {
                let input: SemanticDescribeArgs = Self::parse(args)?;
                let item = semantic::node_types()
                    .into_iter()
                    .find(|item| item.kind == input.kind)
                    .ok_or_else(|| Error::invalid("Unknown managed Motion Canvas node type"))?;
                Ok(serde_json::to_value(item)?)
            }
            "driver.motion-canvas.project.detect" => {
                let _: EmptyArgs = Self::parse(args)?;
                Ok(serde_json::to_value(self.detect_project()?)?)
            }
            "driver.motion-canvas.project.inspect" => {
                let _: EmptyArgs = Self::parse(args)?;
                let snapshot = self.load()?;
                Ok(serde_json::to_value(SnapshotOutput {
                    refs: refs::all(&snapshot.project, &snapshot.source_sha256),
                    generated: snapshot.generated.inventory(),
                    fingerprint: snapshot.source_sha256,
                    project: snapshot.project,
                })?)
            }
            "driver.motion-canvas.project.validate" => {
                let _: EmptyArgs = Self::parse(args)?;
                let snapshot = self.load()?;
                Ok(serde_json::to_value(ValidationOutput {
                    valid: true,
                    fingerprint: snapshot.source_sha256,
                    revision: snapshot.project.revision,
                    generated_fingerprint: snapshot.generated.fingerprint()?,
                })?)
            }
            "driver.motion-canvas.project.create" => {
                let input: CreateArgs = Self::parse(args)?;
                if matches!(
                    self.detect_project()?.mode,
                    ProjectMode::External | ProjectMode::ManagedIsland
                ) {
                    return Err(Error::new(
                        ErrorCode::Conflict,
                        "External or managed-island Motion Canvas projects require project.island.create; root takeover is refused",
                    ));
                }
                validate::project_valid(&input.project)?;
                if !input.project.assets.is_empty() || !input.project.audio.is_empty() {
                    return Err(Error::invalid(
                        "Create projects without media; import bounded media through asset.import before attaching audio",
                    ));
                }
                let generated = crate::compiler::compile(&input.project)?;
                let fingerprint = security::sha256(&serde_json::to_vec_pretty(&input.project)?);
                let provisional = Snapshot {
                    project: input.project.clone(),
                    source_sha256: fingerprint,
                    generated,
                    generated_dir: None,
                };
                if input.dry_run {
                    return Self::mutation_output(
                        None,
                        &provisional,
                        SemanticDiff::default(),
                        false,
                    );
                }
                let saved = self.store()?.create(&input.project)?;
                Self::mutation_output(None, &saved, SemanticDiff::default(), true)
            }
            "driver.motion-canvas.project.island.create" => {
                let input: CreateArgs = Self::parse(args)?;
                let detected = self.detect_project()?;
                if detected.mode != ProjectMode::External
                    || !detected.project_entry
                    || !detected.exact_runtime_match
                {
                    return Err(Error::new(
                        ErrorCode::Conflict,
                        "Managed island requires an external project with src/project.ts and exact Motion Canvas 3.17.2",
                    ));
                }
                validate::project_valid(&input.project)?;
                if !input.project.assets.is_empty() || !input.project.audio.is_empty() {
                    return Err(Error::invalid(
                        "Create islands without media; import bounded media through asset.import after creation",
                    ));
                }
                let generated = crate::compiler::compile(&input.project)?;
                let fingerprint = security::sha256(&serde_json::to_vec_pretty(&input.project)?);
                let provisional = Snapshot {
                    project: input.project.clone(),
                    source_sha256: fingerprint,
                    generated,
                    generated_dir: None,
                };
                let mutation = if input.dry_run {
                    Self::mutation_struct(None, &provisional, SemanticDiff::default(), false)
                } else {
                    let project_root = self.roots.get("project").ok_or_else(|| {
                        Error::new(ErrorCode::Unavailable, "Project grant is not mounted")
                    })?;
                    let island_root = ProjectStore::create_island_root(project_root)?;
                    let island_store = ProjectStore::open_island(island_root)?;
                    let saved = island_store.create(&input.project)?;
                    self.store = Some(island_store);
                    Self::mutation_struct(None, &saved, SemanticDiff::default(), true)
                };
                let import_example = (!input.project.scenes.is_empty()).then(|| {
                    "import {scene0} from '../.semwright/motion/managed-scenes';".to_string()
                });
                Ok(serde_json::to_value(IslandCreateOutput {
                    applied: !input.dry_run,
                    managed_root: crate::store::ISLAND_DIR.into(),
                    semantic_file: format!(
                        "{}/{}",
                        crate::store::ISLAND_DIR,
                        crate::store::SEMANTIC_FILE
                    ),
                    bridge: format!(
                        "{}/{}",
                        crate::store::ISLAND_DIR,
                        crate::store::ISLAND_BRIDGE
                    ),
                    import_example,
                    mutation,
                })?)
            }
            "driver.motion-canvas.project.diff" | "driver.motion-canvas.project.apply" => {
                let input: ApplyArgs = Self::parse(args)?;
                if command.ends_with(".apply") {
                    self.ensure_island_runtime_compatible()?;
                }
                let current = self.load()?;
                if input.expected_fingerprint != current.source_sha256 {
                    return Err(Error::new(
                        ErrorCode::StaleReference,
                        "Managed project fingerprint changed; inspect again",
                    ));
                }
                let prepared =
                    edit::prepare(&current.project, &current.source_sha256, &input.operations)?;
                if command.ends_with(".diff") || input.dry_run {
                    return Self::prospective_output(Some(current.source_sha256), prepared);
                }
                let source = current.source_sha256;
                let diff = prepared.diff.clone();
                let saved = self.store()?.commit(&source, &prepared.project)?;
                Self::mutation_output(Some(source), &saved, diff, true)
            }
            "driver.motion-canvas.scene.list" => {
                let _: EmptyArgs = Self::parse(args)?;
                let s = self.load()?;
                let items = s
                    .project
                    .scenes
                    .iter()
                    .map(|x| SceneItem {
                        id: x.id.clone(),
                        name: x.name.clone(),
                        duration_ms: x.duration_ms,
                        reference: Self::ref_for(&s, Kind::Scene, &x.id),
                    })
                    .collect();
                Ok(serde_json::to_value(ListOutput {
                    fingerprint: s.source_sha256,
                    revision: s.project.revision,
                    items,
                })?)
            }
            "driver.motion-canvas.node.list" => {
                let input: SceneScopeArgs = Self::parse(args)?;
                let s = self.load()?;
                let scene = input
                    .scene_ref
                    .as_deref()
                    .map(|r| Self::checked_scene(&s, r))
                    .transpose()?;
                let items = s
                    .project
                    .scenes
                    .iter()
                    .filter(|x| scene.as_ref().is_none_or(|id| &x.id == id))
                    .flat_map(|x| x.nodes.iter())
                    .map(|n| NodeItem {
                        id: n.id.clone(),
                        name: n.name.clone(),
                        kind: n.kind,
                        parent: n.parent.clone(),
                        reference: Self::ref_for(&s, Kind::Node, &n.id),
                    })
                    .collect();
                Ok(serde_json::to_value(ListOutput {
                    fingerprint: s.source_sha256,
                    revision: s.project.revision,
                    items,
                })?)
            }
            "driver.motion-canvas.node.inspect" => {
                let input: NodeInspectArgs = Self::parse(args)?;
                let snapshot = self.load()?;
                let reference = Reference::decode(&input.node_ref)?;
                reference.check(&snapshot.project, &snapshot.source_sha256, Kind::Node)?;
                let node = snapshot
                    .project
                    .scenes
                    .iter()
                    .flat_map(|scene| scene.nodes.iter())
                    .find(|node| node.id == reference.id)
                    .cloned()
                    .ok_or_else(|| {
                        Error::new(ErrorCode::StaleReference, "Managed node no longer exists")
                    })?;
                let semantic_type = semantic::node_types()
                    .into_iter()
                    .find(|item| item.kind == node.kind)
                    .expect("all managed node kinds are registered");
                Ok(serde_json::to_value(NodeInspectOutput {
                    fingerprint: snapshot.source_sha256.clone(),
                    revision: snapshot.project.revision,
                    reference: Self::ref_for(&snapshot, Kind::Node, &node.id),
                    node,
                    semantic_type,
                })?)
            }
            "driver.motion-canvas.node.property.get" => {
                let input: NodePropertyGetArgs = Self::parse(args)?;
                let snapshot = self.load()?;
                let reference = Reference::decode(&input.node_ref)?;
                reference.check(&snapshot.project, &snapshot.source_sha256, Kind::Node)?;
                let node = snapshot
                    .project
                    .scenes
                    .iter()
                    .flat_map(|scene| scene.nodes.iter())
                    .find(|node| node.id == reference.id)
                    .ok_or_else(|| {
                        Error::new(ErrorCode::StaleReference, "Managed node no longer exists")
                    })?;
                let descriptor = semantic::property(node.kind, &input.property)
                    .ok_or_else(|| Error::invalid("Unknown canonical property for node type"))?;
                let value = if descriptor.storage == crate::semantic::PropertyStorage::Semantic {
                    node.properties
                        .semantic
                        .get(&input.property)
                        .map(serde_json::to_value)
                        .transpose()?
                        .unwrap_or(Value::Null)
                } else {
                    serde_json::to_value(&node.properties)?
                        .as_object()
                        .and_then(|object| object.get(&input.property))
                        .cloned()
                        .unwrap_or(Value::Null)
                };
                Ok(serde_json::to_value(NodePropertyOutput {
                    fingerprint: snapshot.source_sha256.clone(),
                    revision: snapshot.project.revision,
                    node_ref: Self::ref_for(&snapshot, Kind::Node, &node.id),
                    descriptor,
                    is_set: !value.is_null(),
                    value,
                })?)
            }
            "driver.motion-canvas.node.property.set" => {
                let input: NodePropertySetArgs = Self::parse(args)?;
                self.ensure_island_runtime_compatible()?;
                let current = self.load()?;
                if input.expected_fingerprint != current.source_sha256 {
                    return Err(Error::new(
                        ErrorCode::StaleReference,
                        "Managed project fingerprint changed; inspect again",
                    ));
                }
                let reference = Reference::decode(&input.node_ref)?;
                reference.check(&current.project, &current.source_sha256, Kind::Node)?;
                let node = current
                    .project
                    .scenes
                    .iter()
                    .flat_map(|scene| scene.nodes.iter())
                    .find(|node| node.id == reference.id)
                    .ok_or_else(|| {
                        Error::new(ErrorCode::StaleReference, "Managed node no longer exists")
                    })?;
                let descriptor = semantic::property(node.kind, &input.property)
                    .ok_or_else(|| Error::invalid("Unknown canonical property for node type"))?;
                let mut patch = serde_json::Map::new();
                if descriptor.storage == crate::semantic::PropertyStorage::Semantic {
                    let mut values = node.properties.semantic.clone();
                    match input.edit {
                        NodePropertyEdit::Set { value } => {
                            let value: SemanticValue = serde_json::from_value(value)?;
                            values.insert(input.property.clone(), value);
                        }
                        NodePropertyEdit::Reset => {
                            values.remove(&input.property);
                        }
                    }
                    patch.insert("semantic".into(), serde_json::to_value(values)?);
                } else {
                    let value = match input.edit {
                        NodePropertyEdit::Set { value } => value,
                        NodePropertyEdit::Reset if input.property == "filters" => json!([]),
                        NodePropertyEdit::Reset => Value::Null,
                    };
                    patch.insert(input.property.clone(), value);
                }
                let prepared = edit::prepare(
                    &current.project,
                    &current.source_sha256,
                    &[Operation::NodePatch {
                        node_ref: input.node_ref,
                        patch: Value::Object(patch),
                        name: None,
                    }],
                )?;
                if input.dry_run {
                    return Self::prospective_output(Some(current.source_sha256), prepared);
                }
                let source = current.source_sha256;
                let diff = prepared.diff.clone();
                let saved = self.store()?.commit(&source, &prepared.project)?;
                Self::mutation_output(Some(source), &saved, diff, true)
            }
            "driver.motion-canvas.asset.import" => {
                let input: AssetImportArgs = Self::parse(args)?;
                self.ensure_island_runtime_compatible()?;
                let current = self.load()?;
                if input.expected_fingerprint != current.source_sha256 {
                    return Err(Error::new(
                        ErrorCode::StaleReference,
                        "Managed project fingerprint changed; inspect again",
                    ));
                }
                if !security::identifier(&input.id) {
                    return Err(Error::invalid("Asset ID is not canonical"));
                }
                security::relative_path(&input.source)?;
                let media = self.roots.get("media").ok_or_else(|| {
                    Error::new(ErrorCode::Unavailable, "Media grant is not mounted")
                })?;
                let bytes = crate::store::read_granted_file(media, &input.source, MAX_ASSET_BYTES)?;
                let asset = imported_asset(
                    &input.id,
                    input.kind,
                    &input.source,
                    &bytes,
                    input.provenance,
                    input.license,
                )?;
                if current
                    .project
                    .assets
                    .iter()
                    .any(|existing| existing.id == asset.id || existing.path == asset.path)
                {
                    return Err(Error::new(
                        ErrorCode::Conflict,
                        "Managed asset ID or destination already exists",
                    ));
                }
                if self.store()?.root().join(&asset.path).try_exists()? {
                    return Err(Error::new(
                        ErrorCode::Conflict,
                        "Managed asset destination already exists on disk",
                    ));
                }
                let mut project = current.project.clone();
                project.assets.push(asset.clone());
                project.revision = project.revision.checked_add(1).ok_or_else(|| {
                    Error::new(ErrorCode::ResourceExhausted, "Project revision exhausted")
                })?;
                validate::project_valid(&project)?;
                let generated = crate::compiler::compile(&project)?;
                let diff = crate::diff::between(&current.project, &project)?;
                if input.dry_run {
                    return Self::prospective_output(
                        Some(current.source_sha256),
                        edit::PreparedTransaction {
                            project,
                            diff,
                            generated,
                            render_invalidated: true,
                        },
                    );
                }
                let source = current.source_sha256;
                self.store()?.install_asset(&asset.path, &bytes)?;
                match self.store()?.commit(&source, &project) {
                    Ok(saved) => Self::mutation_output(Some(source), &saved, diff, true),
                    Err(error) => match self.store()?.load() {
                        Ok(observed)
                            if observed.project.revision == project.revision
                                && observed.project.assets.iter().any(|existing| {
                                    existing.id == asset.id
                                        && existing.path == asset.path
                                        && existing.sha256 == asset.sha256
                                }) =>
                        {
                            Err(error.uncertain())
                        }
                        Ok(observed)
                            if !observed
                                .project
                                .assets
                                .iter()
                                .any(|existing| existing.path == asset.path) =>
                        {
                            match self
                                .store()?
                                .remove_asset_if_matches(&asset.path, &asset.sha256)
                            {
                                Ok(()) => Err(error),
                                Err(rollback_error) => Err(Error::new(
                                    ErrorCode::Conflict,
                                    format!(
                                        "Asset import failed and rollback could not be proven: {rollback_error}"
                                    ),
                                )
                                .uncertain()),
                            }
                        }
                        Ok(_) | Err(_) => Err(error.uncertain()),
                    },
                }
            }
            "driver.motion-canvas.asset.list" => {
                let _: EmptyArgs = Self::parse(args)?;
                let s = self.load()?;
                let items = s
                    .project
                    .assets
                    .iter()
                    .map(|a| AssetItem {
                        id: a.id.clone(),
                        kind: a.kind,
                        path: a.path.clone(),
                        sha256: a.sha256.clone(),
                        bytes: a.bytes,
                        reference: Self::ref_for(&s, Kind::Asset, &a.id),
                    })
                    .collect();
                Ok(serde_json::to_value(ListOutput {
                    fingerprint: s.source_sha256,
                    revision: s.project.revision,
                    items,
                })?)
            }
            "driver.motion-canvas.cue.list" => {
                let input: SceneScopeArgs = Self::parse(args)?;
                let s = self.load()?;
                let scene = input
                    .scene_ref
                    .as_deref()
                    .map(|r| Self::checked_scene(&s, r))
                    .transpose()?;
                let items = s
                    .project
                    .scenes
                    .iter()
                    .filter(|x| scene.as_ref().is_none_or(|id| &x.id == id))
                    .flat_map(|x| x.cues.iter())
                    .map(|c| CueItem {
                        id: c.id.clone(),
                        name: c.name.clone(),
                        at_ms: c.time_ms,
                        reference: Self::ref_for(&s, Kind::Cue, &c.id),
                    })
                    .collect();
                Ok(serde_json::to_value(ListOutput {
                    fingerprint: s.source_sha256,
                    revision: s.project.revision,
                    items,
                })?)
            }
            "driver.motion-canvas.animation.list" => {
                let input: SceneScopeArgs = Self::parse(args)?;
                let s = self.load()?;
                let scene = input
                    .scene_ref
                    .as_deref()
                    .map(|r| Self::checked_scene(&s, r))
                    .transpose()?;
                let items = s
                    .project
                    .scenes
                    .iter()
                    .filter(|x| scene.as_ref().is_none_or(|id| &x.id == id))
                    .flat_map(|x| x.animations.iter())
                    .map(|a| AnimationItem {
                        id: a.id.clone(),
                        target: a.target.clone(),
                        property: a.property.clone(),
                        reference: Self::ref_for(&s, Kind::Animation, &a.id),
                    })
                    .collect();
                Ok(serde_json::to_value(ListOutput {
                    fingerprint: s.source_sha256,
                    revision: s.project.revision,
                    items,
                })?)
            }
            "driver.motion-canvas.render.plan" => {
                let input: RenderPlanArgs = Self::parse(args)?;
                let s = self.load()?;
                Ok(serde_json::to_value(validate::render_plan(
                    &s.project,
                    &input.profile,
                )?)?)
            }
            "driver.motion-canvas.render.start" => {
                let input = self.checked_render_input(args)?;
                Ok(serde_json::to_value(
                    self.start_render_from_input(input).await?,
                )?)
            }
            "driver.motion-canvas.render.execute" => Err(Error::new(
                ErrorCode::Unsupported,
                "render.execute requires Driver Protocol v3 execution context",
            )),
            "driver.motion-canvas.render.status"
            | "driver.motion-canvas.render.cancel"
            | "driver.motion-canvas.render.result" => {
                let input: JobArgs = Self::parse(args)?;
                let reference = Reference::decode(&input.job_ref)?;
                if reference.kind != Kind::RenderJob {
                    return Err(Error::invalid("Expected a render job ref"));
                }
                let view = if command.ends_with(".status") {
                    self.renderer.status(&input.job_ref).await?
                } else if command.ends_with(".cancel") {
                    self.renderer.cancel(&input.job_ref).await?
                } else {
                    self.renderer.result(&input.job_ref).await?
                };
                Ok(serde_json::to_value(view)?)
            }
            _ => Err(Error::new(
                ErrorCode::Unsupported,
                "Unsupported Motion Canvas capability",
            )),
        }
    }
}
#[async_trait]
impl Driver for MotionDriver {
    fn id(&self) -> &str {
        ID
    }
    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }
    fn interfaces(&self) -> DriverInterfaces {
        DriverInterfaces {
            cooperative_cancellation: true,
            progress: true,
            artifacts: true,
            health: true,
            ..DriverInterfaces::default()
        }
    }
    async fn capabilities(&mut self) -> Result<Vec<Capability>> {
        Self::catalog()
    }
    async fn execute(&mut self, command: &str, digest: &str, args: Value) -> Result<Value> {
        let capability = Self::catalog()?
            .into_iter()
            .find(|c| c.descriptor.name == command)
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::Unsupported,
                    "Unsupported Motion Canvas capability",
                )
            })?;
        if descriptor_digest(&capability.descriptor)? != digest {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Pinned capability descriptor changed",
            ));
        }
        let validator = jsonschema::validator_for(&capability.descriptor.input_schema)
            .map_err(|_| Error::new(ErrorCode::Internal, "Invalid embedded capability schema"))?;
        if !validator.is_valid(&args) {
            return Err(Error::invalid(
                "Capability arguments do not match the strict schema",
            ));
        }
        let value = self.dispatch(command, args).await?;
        let validator = jsonschema::validator_for(&capability.descriptor.output_schema)
            .map_err(|_| Error::new(ErrorCode::Internal, "Invalid embedded output schema"))?;
        if !validator.is_valid(&value) {
            return Err(Error::new(
                ErrorCode::PluginProtocolError,
                "Motion Canvas driver produced output outside its descriptor schema",
            ));
        }
        Ok(value)
    }
    async fn execute_with_context(
        &mut self,
        command: &str,
        digest: &str,
        args: Value,
        context: DriverExecutionContext,
    ) -> Result<Value> {
        context.check_cancelled()?;
        if command != "driver.motion-canvas.render.execute" {
            return self.execute(command, digest, args).await;
        }
        let capability = Self::render_execute_capability()?;
        if descriptor_digest(&capability.descriptor)? != digest {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Pinned capability descriptor changed",
            ));
        }
        let validator = jsonschema::validator_for(&capability.descriptor.input_schema)
            .map_err(|_| Error::new(ErrorCode::Internal, "Invalid embedded capability schema"))?;
        if !validator.is_valid(&args) {
            return Err(Error::invalid(
                "Capability arguments do not match the strict schema",
            ));
        }
        let input = self.checked_render_input(args)?;
        let view = self.execute_render_with_context(input, &context).await?;
        let value = serde_json::to_value(view)?;
        let validator = jsonschema::validator_for(&capability.descriptor.output_schema)
            .map_err(|_| Error::new(ErrorCode::Internal, "Invalid embedded output schema"))?;
        if !validator.is_valid(&value) {
            return Err(Error::new(
                ErrorCode::PluginProtocolError,
                "Motion Canvas driver produced output outside its descriptor schema",
            ));
        }
        Ok(value)
    }
    async fn health(&mut self) -> Result<Value> {
        self.dispatch("driver.motion-canvas.doctor", json!({}))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use semwright_recipes::Executor;

    #[test]
    fn catalog_response_fits_driver_protocol_frame_budget() {
        let capabilities = MotionDriver::catalog().unwrap();
        let digest = semwright_driver_sdk::capabilities_digest(&capabilities).unwrap();
        let response = semwright_driver_sdk::Response::Capabilities {
            id: "catalog-size".into(),
            capabilities,
            digest,
        };
        let bytes = serde_json::to_vec(&response).unwrap();
        assert!(
            bytes.len() <= semwright_types::MAX_FRAME,
            "Motion Canvas catalog is {} bytes but protocol budget is {}",
            bytes.len(),
            semwright_types::MAX_FRAME
        );
    }

    #[test]
    fn capability_catalog_and_manifest_are_stable_goldens() {
        let catalog = MotionDriver::catalog().unwrap();
        let digest = semwright_driver_sdk::capabilities_digest(&catalog).unwrap();
        assert_eq!(
            digest,
            "824bd2edb781f8d952e5ce09caf0bd866547f8aa055d22fef4c690a8282f1c29"
        );

        let manifest_bytes = include_bytes!("../driver.manifest.example.json");
        assert_eq!(
            crate::security::sha256(manifest_bytes),
            "b31318bb7a4264ea06cd62d2edec062a1e6df710ff352932e048591473d9f8f1"
        );
        let manifest: semwright_driver_sdk::Manifest =
            serde_json::from_slice(manifest_bytes).unwrap();
        manifest.validate().unwrap();
    }

    struct RecipeCatalog {
        descriptors: BTreeMap<String, CommandDescriptor>,
    }

    #[async_trait::async_trait]
    impl semwright_recipes::Executor for RecipeCatalog {
        fn describe(&self, command: &str) -> semwright_types::Result<CommandDescriptor> {
            self.descriptors.get(command).cloned().ok_or_else(|| {
                Error::new(
                    ErrorCode::NotFound,
                    "Recipe references an unknown Motion Canvas capability",
                )
            })
        }

        async fn execute(
            &self,
            _request: semwright_types::ExecuteRequest,
            _cancellation: tokio_util::sync::CancellationToken,
        ) -> semwright_types::Result<Value> {
            Err(Error::new(
                ErrorCode::Unsupported,
                "Recipe golden executor validates only; it never executes",
            ))
        }
    }

    #[test]
    fn launch_film_recipe_validates_against_real_driver_catalog() {
        let recipe =
            semwright_recipes::parse(include_str!("../../../recipes/demo/launch-film.yaml"))
                .unwrap();
        let descriptors = MotionDriver::catalog()
            .unwrap()
            .into_iter()
            .map(|capability| (capability.descriptor.name.clone(), capability.descriptor))
            .collect::<BTreeMap<_, _>>();
        let executor = RecipeCatalog { descriptors };

        for step in &recipe.steps {
            let descriptor = executor.describe(&step.command).unwrap();
            let validator = jsonschema::validator_for(&descriptor.input_schema).unwrap();
            assert!(
                validator.is_valid(&step.args),
                "recipe step {} arguments do not match {}",
                step.id,
                step.command
            );
        }
        let plan = recipe.validate(&executor).unwrap();
        assert_eq!(plan["valid"], true);
        assert_eq!(plan["name"], "launch-film-source-check");
        assert_eq!(recipe.steps.len(), 3);
    }

    #[test]
    fn catalog_is_curated_and_descriptor_names_are_owned() {
        let catalog = MotionDriver::catalog().unwrap();
        assert_eq!(catalog.len(), 25);
        assert!(
            catalog
                .iter()
                .all(|c| c.descriptor.name.starts_with("driver.motion-canvas."))
        );
        assert!(
            catalog
                .iter()
                .all(|c| c.descriptor.backends == [SCOPE.to_string()])
        );
        assert!(
            catalog
                .iter()
                .all(|c| c.descriptor.requires == [SCOPE.to_string()])
        );
    }

    #[tokio::test]
    async fn inspect_is_read_only_and_returns_revision_bound_refs() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        std::fs::write(
            root.join(crate::store::SEMANTIC_FILE),
            include_bytes!("../../../fixtures/motion-canvas/hello-text/semwright-motion.json"),
        )
        .unwrap();
        let mut driver = MotionDriver::for_project_root(&root).unwrap();
        let value = driver
            .dispatch("driver.motion-canvas.project.inspect", json!({}))
            .await
            .unwrap();
        assert_eq!(value["project"]["id"], "hello-text");
        assert!(
            value["refs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["id"] == "title")
        );
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
    }
}

#[cfg(test)]
mod driver_tests {
    use super::*;
    use crate::model::{Node, NodeKind, Properties, Scene};
    use semwright_driver_sdk::Driver;

    fn fixture() -> Project {
        let mut p = Project::empty("driver-fixture".into());
        p.generation = "0123456789abcdef0123456789abcdef".into();
        p.scenes.push(Scene {
            id: "main".into(),
            name: "Main".into(),
            duration_ms: 1000,
            nodes: vec![Node {
                id: "title".into(),
                name: "Title".into(),
                kind: NodeKind::Text,
                parent: None,
                properties: Properties {
                    text: Some("driver".into()),
                    ..Default::default()
                },
            }],
            animations: vec![],
            cues: vec![],
            transition: None,
        });
        p
    }
    fn png_asset() -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[0x23, 0x42, 0x61, 0xff]).unwrap();
        }
        bytes
    }
    async fn call(driver: &mut MotionDriver, name: &str, args: Value) -> Result<Value> {
        let cap = driver
            .capabilities()
            .await?
            .into_iter()
            .find(|c| c.descriptor.name == name)
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "test capability missing"))?;
        let digest = descriptor_digest(&cap.descriptor)?;
        driver.execute(name, &digest, args).await
    }

    #[test]
    fn protocol_v3_render_execute_contract_is_explicit() {
        let temp = tempfile::tempdir().unwrap();
        let driver =
            MotionDriver::for_project_root(&std::fs::canonicalize(temp.path()).unwrap()).unwrap();
        let interfaces = Driver::interfaces(&driver);
        assert!(interfaces.cooperative_cancellation);
        assert!(interfaces.progress);
        assert!(interfaces.artifacts);
        assert!(!interfaces.dynamic_capabilities);
        assert!(!interfaces.events);
        assert!(!interfaces.native_refs);

        let capability = MotionDriver::catalog()
            .unwrap()
            .into_iter()
            .find(|capability| capability.descriptor.name == "driver.motion-canvas.render.execute")
            .unwrap();
        assert_eq!(capability.descriptor.timeout_ms, 300_000);
        assert!(
            capability
                .tags
                .iter()
                .any(|tag| tag == "artifact-out:image/png-sequence")
        );

        let manifest: semwright_driver_sdk::Manifest =
            serde_json::from_slice(include_bytes!("../driver.manifest.example.json")).unwrap();
        assert_eq!(manifest.protocol, 3);
        assert_eq!(manifest.request_timeout_ms, 300_000);
        assert!(manifest.interfaces.cooperative_cancellation);
        assert!(manifest.interfaces.progress);
        assert!(manifest.interfaces.artifacts);
        assert!(!manifest.interfaces.native_refs);
        manifest.validate().unwrap();
    }

    #[tokio::test]
    async fn catalog_is_strict_bounded_and_namespace_owned() {
        let temp = tempfile::tempdir().unwrap();
        let mut driver =
            MotionDriver::for_project_root(&std::fs::canonicalize(temp.path()).unwrap()).unwrap();
        let caps = driver.capabilities().await.unwrap();
        assert_eq!(caps.len(), 25);
        let mut names = std::collections::BTreeSet::new();
        for cap in caps {
            assert!(cap.descriptor.name.starts_with("driver.motion-canvas."));
            assert_eq!(cap.descriptor.backends, [SCOPE.to_string()]);
            assert!(names.insert(cap.descriptor.name));
        }
    }

    #[tokio::test]
    async fn doctor_reports_fail_closed_renderer_and_no_network() {
        let temp = tempfile::tempdir().unwrap();
        let mut driver =
            MotionDriver::for_project_root(&std::fs::canonicalize(temp.path()).unwrap()).unwrap();
        let out = call(&mut driver, "driver.motion-canvas.doctor", json!({}))
            .await
            .unwrap();
        assert_eq!(out["motion_canvas_version"], MOTION_CANVAS_VERSION);
        assert_eq!(out["node_version"], NODE_VERSION);
        assert_eq!(out["network"], false);
        assert_eq!(out["render_available"], false);
        assert_eq!(out["capability_count"], 25);
    }

    #[tokio::test]
    async fn external_detection_never_enables_mutation() {
        let temp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(temp.path()).unwrap();
        std::fs::create_dir(root.join("src")).unwrap();
        std::fs::write(root.join("src/project.ts"), b"").unwrap();
        std::fs::write(
            root.join("package.json"),
            serde_json::to_vec(&json!({"dependencies":{"@motion-canvas/core":"3.17.2"}})).unwrap(),
        )
        .unwrap();
        let mut driver = MotionDriver::for_project_root(&root).unwrap();
        let out = call(
            &mut driver,
            "driver.motion-canvas.project.detect",
            json!({}),
        )
        .await
        .unwrap();
        assert_eq!(out["mode"], "external");
        assert_eq!(out["project_entry"], true);
        assert_eq!(out["motion_canvas_version"], MOTION_CANVAS_VERSION);
        assert_eq!(out["exact_runtime_match"], true);
        assert_eq!(out["mutation_supported"], false);
    }

    #[tokio::test]
    async fn create_dry_run_is_pure_then_atomic_create_and_inspect() {
        let temp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(temp.path()).unwrap();
        let mut driver = MotionDriver::for_project_root(&root).unwrap();
        let p = fixture();
        let dry = call(
            &mut driver,
            "driver.motion-canvas.project.create",
            json!({"project":p,"dry_run":true}),
        )
        .await
        .unwrap();
        assert_eq!(dry["applied"], false);
        assert!(!root.join(crate::store::SEMANTIC_FILE).exists());
        let p = fixture();
        let created = call(
            &mut driver,
            "driver.motion-canvas.project.create",
            json!({"project":p,"dry_run":false}),
        )
        .await
        .unwrap();
        assert_eq!(created["applied"], true);
        let inspected = call(
            &mut driver,
            "driver.motion-canvas.project.inspect",
            json!({}),
        )
        .await
        .unwrap();
        assert_eq!(created["resulting_fingerprint"], inspected["fingerprint"]);
        assert_eq!(inspected["project"]["id"], "driver-fixture");
    }

    #[tokio::test]
    async fn apply_uses_exact_fingerprint_and_real_dry_run() {
        let temp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(temp.path()).unwrap();
        let mut driver = MotionDriver::for_project_root(&root).unwrap();
        call(
            &mut driver,
            "driver.motion-canvas.project.create",
            json!({"project":fixture(),"dry_run":false}),
        )
        .await
        .unwrap();
        let inspect = call(
            &mut driver,
            "driver.motion-canvas.project.inspect",
            json!({}),
        )
        .await
        .unwrap();
        let fp = inspect["fingerprint"].as_str().unwrap().to_string();
        let op = json!({"op":"settings_patch","patch":{"fps":60}});
        let dry = call(
            &mut driver,
            "driver.motion-canvas.project.apply",
            json!({"expected_fingerprint":fp,"operations":[op],"dry_run":true}),
        )
        .await
        .unwrap();
        assert_eq!(dry["applied"], false);
        assert_eq!(
            call(
                &mut driver,
                "driver.motion-canvas.project.inspect",
                json!({})
            )
            .await
            .unwrap()["project"]["settings"]["fps"],
            30
        );
        let stale = call(
            &mut driver,
            "driver.motion-canvas.project.apply",
            json!({"expected_fingerprint":"f".repeat(64),"operations":[],"dry_run":false}),
        )
        .await
        .unwrap_err();
        assert_eq!(stale.code, ErrorCode::StaleReference);
    }

    #[tokio::test]
    async fn asset_import_is_bounded_atomic_and_dry_run_is_pure() {
        let project_dir = tempfile::tempdir().unwrap();
        let media_dir = tempfile::tempdir().unwrap();
        let project_root = std::fs::canonicalize(project_dir.path()).unwrap();
        let media_root = std::fs::canonicalize(media_dir.path()).unwrap();
        let bytes = png_asset();
        std::fs::write(media_root.join("pixel.png"), &bytes).unwrap();
        std::fs::write(
            media_root.join("active.svg"),
            b"<svg><script>alert(1)</script></svg>",
        )
        .unwrap();
        let mut driver =
            MotionDriver::for_project_and_media_roots(&project_root, &media_root).unwrap();
        call(
            &mut driver,
            "driver.motion-canvas.project.create",
            json!({"project":fixture(),"dry_run":false}),
        )
        .await
        .unwrap();
        let inspected = call(
            &mut driver,
            "driver.motion-canvas.project.inspect",
            json!({}),
        )
        .await
        .unwrap();
        let fingerprint = inspected["fingerprint"].as_str().unwrap().to_owned();
        let dry = call(&mut driver, "driver.motion-canvas.asset.import", json!({
            "expected_fingerprint":fingerprint,"id":"pixel","kind":"image","source":"pixel.png","dry_run":true
        })).await.unwrap();
        assert_eq!(dry["applied"], false);
        assert!(!project_root.join("assets/pixel.png").exists());
        let saved = call(&mut driver, "driver.motion-canvas.asset.import", json!({
            "expected_fingerprint":fingerprint,"id":"pixel","kind":"image","source":"pixel.png","dry_run":false
        })).await.unwrap();
        assert_eq!(saved["applied"], true);
        assert_eq!(
            std::fs::read(project_root.join("assets/pixel.png")).unwrap(),
            bytes
        );
        let list = call(&mut driver, "driver.motion-canvas.asset.list", json!({}))
            .await
            .unwrap();
        assert_eq!(list["items"].as_array().unwrap().len(), 1);
        assert_eq!(list["items"][0]["sha256"], security::sha256(&png_asset()));
        let stale = call(&mut driver, "driver.motion-canvas.asset.import", json!({
            "expected_fingerprint":fingerprint,"id":"second","kind":"image","source":"pixel.png","dry_run":false
        })).await.unwrap_err();
        assert_eq!(stale.code, ErrorCode::StaleReference);
        let current = call(
            &mut driver,
            "driver.motion-canvas.project.inspect",
            json!({}),
        )
        .await
        .unwrap();
        let current_fp = current["fingerprint"].as_str().unwrap();
        let traversal = call(&mut driver, "driver.motion-canvas.asset.import", json!({
            "expected_fingerprint":current_fp,"id":"escape","kind":"image","source":"../pixel.png","dry_run":true
        })).await.unwrap_err();
        assert_eq!(traversal.code, ErrorCode::InvalidArgument);
        let active_svg = call(&mut driver, "driver.motion-canvas.asset.import", json!({
            "expected_fingerprint":current_fp,"id":"active","kind":"svg","source":"active.svg","dry_run":true
        })).await.unwrap_err();
        assert_eq!(active_svg.code, ErrorCode::InvalidArgument);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn asset_import_rejects_media_symlink_escape() {
        use std::os::unix::fs::symlink;
        let project_dir = tempfile::tempdir().unwrap();
        let media_dir = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(outside.path(), png_asset()).unwrap();
        symlink(outside.path(), media_dir.path().join("outside.png")).unwrap();
        let project_root = std::fs::canonicalize(project_dir.path()).unwrap();
        let media_root = std::fs::canonicalize(media_dir.path()).unwrap();
        let mut driver =
            MotionDriver::for_project_and_media_roots(&project_root, &media_root).unwrap();
        call(
            &mut driver,
            "driver.motion-canvas.project.create",
            json!({"project":fixture(),"dry_run":false}),
        )
        .await
        .unwrap();
        let inspected = call(
            &mut driver,
            "driver.motion-canvas.project.inspect",
            json!({}),
        )
        .await
        .unwrap();
        let error = call(&mut driver, "driver.motion-canvas.asset.import", json!({
            "expected_fingerprint":inspected["fingerprint"],"id":"outside","kind":"image","source":"outside.png","dry_run":true
        })).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::PermissionDenied);
    }

    #[tokio::test]
    async fn semantic_introspection_and_property_mutation_are_revision_bound() {
        let temp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(temp.path()).unwrap();
        std::fs::write(
            root.join(crate::store::SEMANTIC_FILE),
            include_bytes!(
                "../../../fixtures/motion-canvas/semantic-complete/semwright-motion.json"
            ),
        )
        .unwrap();
        let mut driver = MotionDriver::for_project_root(&root).unwrap();

        let types = call(
            &mut driver,
            "driver.motion-canvas.semantic.types",
            json!({}),
        )
        .await
        .unwrap();
        assert_eq!(types["motion_canvas_version"], MOTION_CANVAS_VERSION);
        assert_eq!(types["managed_type_count"], 20);
        assert!(
            types["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["kind"] == "polygon" && item["upstream_class"] == "Polygon")
        );

        let described = call(
            &mut driver,
            "driver.motion-canvas.semantic.describe",
            json!({"kind":"polygon"}),
        )
        .await
        .unwrap();
        assert!(
            described["properties"]
                .as_array()
                .unwrap()
                .iter()
                .any(|property| property["semantic_name"] == "sides"
                    && property["upstream_name"] == "sides")
        );

        let listed = call(&mut driver, "driver.motion-canvas.node.list", json!({}))
            .await
            .unwrap();
        let polygon = listed["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == "polygon")
            .unwrap();
        let old_ref = polygon["reference"].as_str().unwrap().to_owned();
        let fingerprint = listed["fingerprint"].as_str().unwrap().to_owned();

        let inspected = call(
            &mut driver,
            "driver.motion-canvas.node.inspect",
            json!({"node_ref":old_ref}),
        )
        .await
        .unwrap();
        assert_eq!(inspected["node"]["kind"], "polygon");
        assert_eq!(inspected["semantic_type"]["upstream_class"], "Polygon");

        let before = call(
            &mut driver,
            "driver.motion-canvas.node.property.get",
            json!({"node_ref":old_ref,"property":"sides"}),
        )
        .await
        .unwrap();
        assert_eq!(before["value"], 6.0);
        assert_eq!(before["descriptor"]["storage"], "semantic");

        let dry = call(
            &mut driver,
            "driver.motion-canvas.node.property.set",
            json!({
                "expected_fingerprint":fingerprint,
                "node_ref":old_ref,
                "property":"sides",
                "edit":{"mode":"set","value":8.0},
                "dry_run":true
            }),
        )
        .await
        .unwrap();
        assert_eq!(dry["applied"], false);

        let applied = call(
            &mut driver,
            "driver.motion-canvas.node.property.set",
            json!({
                "expected_fingerprint":fingerprint,
                "node_ref":old_ref,
                "property":"sides",
                "edit":{"mode":"set","value":8.0},
                "dry_run":false
            }),
        )
        .await
        .unwrap();
        assert_eq!(applied["applied"], true);
        assert_eq!(applied["revision"], 2);

        let stale = call(
            &mut driver,
            "driver.motion-canvas.node.property.get",
            json!({"node_ref":old_ref,"property":"sides"}),
        )
        .await
        .unwrap_err();
        assert_eq!(stale.code, ErrorCode::StaleReference);

        let refreshed = call(&mut driver, "driver.motion-canvas.node.list", json!({}))
            .await
            .unwrap();
        let new_ref = refreshed["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == "polygon")
            .unwrap()["reference"]
            .as_str()
            .unwrap()
            .to_owned();
        let after = call(
            &mut driver,
            "driver.motion-canvas.node.property.get",
            json!({"node_ref":new_ref,"property":"sides"}),
        )
        .await
        .unwrap();
        assert_eq!(after["value"], 8.0);

        let invalid = call(
            &mut driver,
            "driver.motion-canvas.node.property.set",
            json!({
                "expected_fingerprint":refreshed["fingerprint"],
                "node_ref":new_ref,
                "property":"smooth_corners",
                "edit":{"mode":"set","value":true},
                "dry_run":true
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(invalid.code, ErrorCode::InvalidArgument);
    }

    fn write_external_project(root: &Path, version: &str, source: &[u8]) {
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/project.ts"), source).unwrap();
        std::fs::write(
            root.join("package.json"),
            serde_json::to_vec_pretty(&json!({
                "dependencies": {"@motion-canvas/core": version}
            }))
            .unwrap(),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn managed_island_preserves_external_typescript_and_refreshes_owned_bridge() {
        let temp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(temp.path()).unwrap();
        let human_source = b"import {makeProject} from '@motion-canvas/core';\nexport default makeProject({scenes: []});\n";
        write_external_project(&root, "3.17.1", human_source);
        let mut driver = MotionDriver::for_project_root(&root).unwrap();

        let mismatch = call(
            &mut driver,
            "driver.motion-canvas.project.island.create",
            json!({"project":fixture(),"dry_run":true}),
        )
        .await
        .unwrap_err();
        assert_eq!(mismatch.code, ErrorCode::Conflict);
        assert!(!root.join(".semwright").exists());
        assert_eq!(
            std::fs::read(root.join("src/project.ts")).unwrap(),
            human_source
        );

        write_external_project(&root, MOTION_CANVAS_VERSION, human_source);
        let dry = call(
            &mut driver,
            "driver.motion-canvas.project.island.create",
            json!({"project":fixture(),"dry_run":true}),
        )
        .await
        .unwrap();
        assert_eq!(dry["applied"], false);
        assert_eq!(dry["managed_root"], crate::store::ISLAND_DIR);
        assert!(!root.join(".semwright").exists());

        let created = call(
            &mut driver,
            "driver.motion-canvas.project.island.create",
            json!({"project":fixture(),"dry_run":false}),
        )
        .await
        .unwrap();
        assert_eq!(created["applied"], true);
        let island = root.join(crate::store::ISLAND_DIR);
        assert!(island.join(crate::store::SEMANTIC_FILE).is_file());
        let bridge_path = island.join(crate::store::ISLAND_BRIDGE);
        let bridge_before = std::fs::read_to_string(&bridge_path).unwrap();
        assert!(bridge_before.contains("export {default as scene0}"));
        assert!(bridge_before.contains("semwrightMotionFingerprint"));
        assert_eq!(
            std::fs::read(root.join("src/project.ts")).unwrap(),
            human_source
        );

        let detected = call(
            &mut driver,
            "driver.motion-canvas.project.detect",
            json!({}),
        )
        .await
        .unwrap();
        assert_eq!(detected["mode"], "managed_island");
        assert_eq!(detected["mutation_supported"], true);
        assert_eq!(detected["managed_root"], crate::store::ISLAND_DIR);

        let listed = call(&mut driver, "driver.motion-canvas.node.list", json!({}))
            .await
            .unwrap();
        let title = listed["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == "title")
            .unwrap()["reference"]
            .as_str()
            .unwrap()
            .to_owned();
        let changed = call(
            &mut driver,
            "driver.motion-canvas.node.property.set",
            json!({
                "expected_fingerprint": listed["fingerprint"],
                "node_ref": title,
                "property": "opacity",
                "edit": {"mode":"set","value":0.5},
                "dry_run": false
            }),
        )
        .await
        .unwrap();
        assert_eq!(changed["applied"], true);
        let bridge_after = std::fs::read_to_string(&bridge_path).unwrap();
        assert_ne!(bridge_before, bridge_after);
        assert!(bridge_after.contains(changed["resulting_fingerprint"].as_str().unwrap()));
        assert_eq!(
            std::fs::read(root.join("src/project.ts")).unwrap(),
            human_source
        );

        let takeover = call(
            &mut driver,
            "driver.motion-canvas.project.create",
            json!({"project":fixture(),"dry_run":true}),
        )
        .await
        .unwrap_err();
        assert_eq!(takeover.code, ErrorCode::Conflict);

        write_external_project(&root, "3.18.0", human_source);
        let detected = call(
            &mut driver,
            "driver.motion-canvas.project.detect",
            json!({}),
        )
        .await
        .unwrap();
        assert_eq!(detected["mode"], "managed_island");
        assert_eq!(detected["mutation_supported"], false);

        let inspect = call(
            &mut driver,
            "driver.motion-canvas.project.inspect",
            json!({}),
        )
        .await
        .unwrap();
        let fingerprint = inspect["fingerprint"].as_str().unwrap().to_owned();
        let blocked = call(
            &mut driver,
            "driver.motion-canvas.project.apply",
            json!({"expected_fingerprint":fingerprint,"operations":[],"dry_run":true}),
        )
        .await
        .unwrap_err();
        assert_eq!(blocked.code, ErrorCode::Conflict);

        let blocked_render = call(
            &mut driver,
            "driver.motion-canvas.render.start",
            json!({
                "expected_fingerprint": inspect["fingerprint"],
                "profile": {
                    "first_frame": 0,
                    "end_frame_exclusive": 1,
                    "scale": "full",
                    "transparent": false,
                    "timeout_ms": 1000
                }
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(blocked_render.code, ErrorCode::Conflict);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn managed_island_rejects_symlinked_control_directory() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(temp.path()).unwrap();
        write_external_project(
            &root,
            MOTION_CANVAS_VERSION,
            b"export default {human: true};\n",
        );
        symlink(outside.path(), root.join(".semwright")).unwrap();
        let mut driver = MotionDriver::for_project_root(&root).unwrap();
        let error = call(
            &mut driver,
            "driver.motion-canvas.project.island.create",
            json!({"project":fixture(),"dry_run":false}),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::PermissionDenied);
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn schema_and_descriptor_digest_fail_closed() {
        let temp = tempfile::tempdir().unwrap();
        let mut driver =
            MotionDriver::for_project_root(&std::fs::canonicalize(temp.path()).unwrap()).unwrap();
        let cap = driver
            .capabilities()
            .await
            .unwrap()
            .into_iter()
            .find(|c| c.descriptor.name.ends_with(".doctor"))
            .unwrap();
        let err = driver
            .execute(&cap.descriptor.name, &"0".repeat(64), json!({}))
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::StaleReference);
        let digest = descriptor_digest(&cap.descriptor).unwrap();
        let err = driver
            .execute(&cap.descriptor.name, &digest, json!({"shell":"echo nope"}))
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidArgument);
    }
}
