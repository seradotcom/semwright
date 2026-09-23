//! Driver Protocol v1 adapter for bounded, Semwright-managed Motion Canvas projects.
use crate::{
    Result,
    diff::SemanticDiff,
    edit::{self, Operation},
    model::*,
    refs::{self, Kind, ObjectRef, Reference},
    renderer::{JobView, RenderManager, RendererRuntime},
    security,
    store::{ProjectStore, Snapshot},
    validate::{self, RenderPlan},
};
use async_trait::async_trait;
use schemars::{JsonSchema, schema_for};
use semwright_driver_sdk::{Capability, Driver, descriptor_digest};
use semwright_types::{CommandDescriptor, Error, ErrorCode, Idempotency, Risk};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
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
struct SceneScopeArgs {
    #[serde(default)]
    scene_ref: Option<String>,
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
    output_mounted: bool,
    runtime_mounted: bool,
    render_available: bool,
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

pub struct MotionDriver {
    roots: BTreeMap<String, PathBuf>,
    store: Option<ProjectStore>,
    renderer: RenderManager,
}
impl MotionDriver {
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
        let store = roots.get("project").map(ProjectStore::open).transpose()?;
        let runtime = roots
            .get("runtime")
            .map(|root| RendererRuntime::from_root(root))
            .transpose()
            .ok()
            .flatten();
        let output = roots
            .get("output")
            .cloned()
            .unwrap_or_else(|| PathBuf::from("/workspace/output"));
        let renderer = RenderManager::new(runtime, output);
        Ok(Self {
            roots,
            store,
            renderer,
        })
    }
    #[cfg(test)]
    pub fn for_project_root(root: &Path) -> Result<Self> {
        let mut roots = BTreeMap::new();
        roots.insert("project".into(), root.to_path_buf());
        let renderer = RenderManager::new(None, root.join("test-output"));
        Ok(Self {
            roots,
            store: Some(ProjectStore::open(root)?),
            renderer,
        })
    }
    fn store(&self) -> Result<&ProjectStore> {
        self.store
            .as_ref()
            .ok_or_else(|| Error::new(ErrorCode::Unavailable, "Project grant is not mounted"))
    }
    fn load(&self) -> Result<Snapshot> {
        self.store()?.load()
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
    fn catalog() -> Result<Vec<Capability>> {
        Ok(vec![
            Self::cap::<EmptyArgs, DoctorOutput>(
                "driver.motion-canvas.doctor",
                "Inspect bounded Motion Canvas driver and renderer prerequisites",
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
    fn mutation_output(
        source: Option<String>,
        snapshot: &Snapshot,
        diff: SemanticDiff,
        applied: bool,
    ) -> Result<Value> {
        Ok(serde_json::to_value(MutationOutput {
            applied,
            source_fingerprint: source,
            resulting_fingerprint: snapshot.source_sha256.clone(),
            revision: snapshot.project.revision,
            diff,
            generated: snapshot.generated.inventory(),
            refs: refs::all(&snapshot.project, &snapshot.source_sha256),
        })?)
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
                    output_mounted: self.roots.contains_key("output"),
                    runtime_mounted: self.roots.contains_key("runtime"),
                    render_available: self.renderer.available(),
                    active_jobs: self.renderer.active_count().await,
                    capability_count: Self::catalog()?.len(),
                })?)
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
                validate::project_valid(&input.project)?;
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
            "driver.motion-canvas.project.diff" | "driver.motion-canvas.project.apply" => {
                let input: ApplyArgs = Self::parse(args)?;
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
                        property: a.property,
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
                let input: RenderStartArgs = Self::parse(args)?;
                let mut snapshot = self.load()?;
                if input.expected_fingerprint != snapshot.source_sha256 {
                    return Err(Error::new(
                        ErrorCode::StaleReference,
                        "Managed project fingerprint changed; inspect again",
                    ));
                }
                let generated = self.store()?.materialize(&snapshot)?;
                snapshot.generated_dir = Some(generated);
                Ok(serde_json::to_value(
                    self.renderer.start(&snapshot, input.profile).await?,
                )?)
            }
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
    async fn health(&mut self) -> Result<Value> {
        self.dispatch("driver.motion-canvas.doctor", json!({}))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_curated_and_descriptor_names_are_owned() {
        let catalog = MotionDriver::catalog().unwrap();
        assert_eq!(catalog.len(), 16);
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

    #[tokio::test]
    async fn catalog_is_strict_bounded_and_namespace_owned() {
        let temp = tempfile::tempdir().unwrap();
        let mut driver =
            MotionDriver::for_project_root(&std::fs::canonicalize(temp.path()).unwrap()).unwrap();
        let caps = driver.capabilities().await.unwrap();
        assert_eq!(caps.len(), 16);
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
        assert_eq!(out["capability_count"], 16);
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
