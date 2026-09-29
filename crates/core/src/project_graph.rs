//! Project Graph routes reuse the Broker's authenticated context and policy decision.
//! There is no client receipt admission route and no alternate execution scheduler.
//! Durable owner identity is supplied once by the trusted host from the OS user principal;
//! request sessions remain ephemeral and are never persisted as ownership.
use super::*;
use g::composition::{self as c, Digest, Owner, PrincipalBinding};
use semwright_platform_api::filesystem::ScopedFilesystem;
use semwright_project_graph as g;
use std::path::{Path, PathBuf};
const MAX_OPEN_PROJECTS: usize = 8;
const MAX_STORED_PROJECTS: usize = 32;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectBoundary {
    version: u32,
    root: String,
    root_binding: Digest,
}
pub(super) struct ProjectGraphs {
    home: PathBuf,
    principal: PrincipalBinding,
    stores: BTreeMap<g::ProjectId, g::GraphStore>,
    cursors: g::QueryCursors,
}
fn graph_error(error: g::GraphError) -> Error {
    let (code, message) = match error {
        g::GraphError::Denied => (ErrorCode::PolicyDenied, "Project access denied"),
        g::GraphError::Conflict => (
            ErrorCode::Conflict,
            "Project snapshot or resource binding changed; reconcile explicitly",
        ),
        g::GraphError::Limit(_) => (
            ErrorCode::ResourceExhausted,
            "Project graph operation exceeded its declared budget",
        ),
        g::GraphError::Cancelled => (ErrorCode::Cancelled, "Project operation cancelled"),
        g::GraphError::Invalid(_) | g::GraphError::Contract(_) => (
            ErrorCode::InvalidArgument,
            "Project identity, query or evidence validation failed",
        ),
        _ => (
            ErrorCode::BackendFailed,
            "Private project storage failed; original evidence preserved",
        ),
    };
    Error::new(code, message)
}
fn decode<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T> {
    c::strict_decode(
        &c::canonical_bytes(value).map_err(|_| Error::invalid("Project request budget"))?,
    )
    .map_err(|_| Error::invalid("Invalid project request"))
}
fn project_id(args: &Value) -> Result<g::ProjectId> {
    g::ProjectId::parse(arg_str(args, "project")?.into()).map_err(graph_error)
}
fn asset_id(args: &Value) -> Result<g::LogicalAssetId> {
    g::LogicalAssetId::parse(arg_str(args, "asset")?.into()).map_err(graph_error)
}
fn root_binding(root: &semwright_policy::FilesystemGrant) -> Result<Digest> {
    let resolved = std::fs::canonicalize(&root.path)
        .map_err(|_| Error::unavailable("Configured project root cannot be resolved"))?;
    let metadata = std::fs::symlink_metadata(&resolved)?;
    if !metadata.is_dir() {
        return Err(Error::invalid(
            "Configured project root must resolve to a directory",
        ));
    }
    c::canonical_digest(&(&root.name, &resolved))
        .map_err(|_| Error::invalid("Configured project root cannot be fingerprinted"))
}
struct Cancel(CancellationToken);
impl g::CancellationCheck for Cancel {
    fn cancelled(&self) -> bool {
        self.0.is_cancelled()
    }
}
impl ProjectGraphs {
    fn new(home: &Path, principal: String) -> Result<Self> {
        if !home.is_absolute() || principal.is_empty() || principal.len() > 256 {
            return Err(Error::invalid(
                "Canonical private project state and authenticated principal required",
            ));
        }
        semwright_platform_services::private_directory(home)?;
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        if std::fs::canonicalize(home)? != home {
            return Err(Error::invalid(
                "Canonical private project state path required",
            ));
        }
        Ok(Self {
            home: home.into(),
            principal: PrincipalBinding::Named(principal),
            stores: BTreeMap::new(),
            cursors: g::QueryCursors::default(),
        })
    }
    pub(super) fn revoke_session(&mut self, session: &str) {
        self.cursors.revoke_session(session);
    }
    fn evict_one(&mut self) {
        if self.stores.len() >= MAX_OPEN_PROJECTS
            && let Some(id) = self.stores.keys().next().cloned()
        {
            self.stores.remove(&id);
        }
    }
    fn create(
        &mut self,
        root: &semwright_policy::FilesystemGrant,
        context: &Context,
    ) -> Result<g::ProjectId> {
        context.check_cancelled()?;
        let mut count = 0;
        for entry in std::fs::read_dir(&self.home)? {
            let entry = entry?;
            if g::ProjectId::parse(entry.file_name().to_string_lossy().into_owned()).is_ok() {
                count += 1;
            }
            if count >= MAX_STORED_PROJECTS {
                return Err(Error::new(
                    ErrorCode::ResourceExhausted,
                    "Private project count limit reached",
                ));
            }
        }
        let boundary = ProjectBoundary {
            version: 1,
            root: root.name.clone(),
            root_binding: root_binding(root)?,
        };
        let id = g::ProjectId::new();
        let directory = self.home.join(id.as_str());
        semwright_platform_services::private_directory(&directory)?;
        let private =
            semwright_platform_services::filesystem().open_root(&directory, true, true)?;
        private.write_atomic(
            Path::new("scope.json"),
            &c::canonical_bytes(&boundary).map_err(|_| Error::invalid("Project scope encoding"))?,
        )?;
        let store = g::GraphStore::open(&directory, id.clone(), self.principal.clone(), false)
            .map_err(graph_error)?;
        self.evict_one();
        self.stores.insert(id.clone(), store);
        Ok(id)
    }
    fn open_existing(
        &mut self,
        id: &g::ProjectId,
        root: &semwright_policy::FilesystemGrant,
    ) -> Result<()> {
        let directory = self.home.join(id.as_str());
        let private = semwright_platform_services::filesystem()
            .open_root(&directory, true, false)
            .map_err(|_| Error::new(ErrorCode::PolicyDenied, "Project access denied"))?;
        let bytes = private
            .read(Path::new("scope.json"), 4096)
            .map_err(|_| Error::new(ErrorCode::PolicyDenied, "Project access denied"))?;
        let boundary: ProjectBoundary = c::strict_decode(&bytes)
            .map_err(|_| Error::new(ErrorCode::PolicyDenied, "Project access denied"))?;
        if boundary.version != 1
            || boundary.root != root.name
            || boundary.root_binding != root_binding(root)?
        {
            return Err(Error::new(ErrorCode::PolicyDenied, "Project access denied"));
        }
        if !self.stores.contains_key(id) {
            if !directory.join("project.sqlite3").try_exists()? {
                return Err(Error::new(
                    ErrorCode::Conflict,
                    "Incomplete private project creation; evidence preserved",
                ));
            }
            let store = g::GraphStore::open(&directory, id.clone(), self.principal.clone(), false)
                .map_err(graph_error)?;
            self.evict_one();
            self.stores.insert(id.clone(), store);
        }
        Ok(())
    }
    fn execute(
        &mut self,
        policy: &Policy,
        context: &Context,
        command: &str,
        args: &Value,
    ) -> Result<Value> {
        context.check_cancelled()?;
        let root_name = arg_str(args, "root")?;
        let root = policy
            .config()
            .filesystem
            .iter()
            .find(|r| r.name == root_name && r.read)
            .ok_or_else(|| Error::new(ErrorCode::PolicyDenied, "Project root is not granted"))?;
        if command == "project.create" {
            let id = self.create(root, context)?;
            return Ok(
                json!({"project":id,"graph_schema":1,"result":{"created":true,"snapshot":0}}),
            );
        }
        let id = project_id(args)?;
        self.open_existing(&id, root)?;
        let owner = Owner {
            session: context.session.clone(),
            principal: self.principal.clone(),
        };
        let write = !matches!(
            command,
            "project.query"
                | "project.asset.inspect"
                | "project.revisions"
                | "project.impact"
                | "project.manifest.export"
        );
        let full = g::ProjectAccess::authorized(
            owner.clone(),
            id.clone(),
            None,
            write,
            Digest::parse(policy.grant_fingerprint())
                .map_err(|_| Error::invalid("Grant fingerprint"))?,
        )
        .map_err(graph_error)?;
        let store = self
            .stores
            .get_mut(&id)
            .ok_or_else(|| Error::new(ErrorCode::Internal, "Project store unavailable"))?;
        let scoped = store
            .graph()
            .map_err(graph_error)?
            .file_scope(&full, root_name)
            .map_err(graph_error)?;
        let result = match command {
            "project.asset.register" => {
                let asset = g::Asset {
                    id: g::LogicalAssetId::new(),
                    label: arg_str(args, "label")?.into(),
                    resource_type: arg_str(args, "resource_type")?.into(),
                    locator: Some(g::DurableLocator::ScopedFile {
                        root: root_name.into(),
                        relative_path: arg_str(args, "path")?.into(),
                    }),
                };
                let view = store
                    .transact(&full, |graph| {
                        graph.register(&full, asset.clone())?;
                        reconcile(
                            graph,
                            &full,
                            context,
                            &owner,
                            root,
                            &asset.id,
                            byte_limit(args),
                            true,
                        )?;
                        graph.inspect(&full, &asset.id)
                    })
                    .map_err(graph_error)?;
                serde_json::to_value(view)?
            }
            "project.asset.inspect" => serde_json::to_value(
                store
                    .graph()
                    .map_err(graph_error)?
                    .inspect(&scoped, &asset_id(args)?)
                    .map_err(graph_error)?,
            )?,
            "project.asset.rename" => {
                let asset = asset_id(args)?;
                let label = arg_str(args, "label")?.to_owned();
                store
                    .transact(&scoped, |graph| {
                        graph.rename(&scoped, &asset, label)?;
                        graph.inspect(&scoped, &asset)
                    })
                    .map_err(graph_error)
                    .and_then(|view| Ok(serde_json::to_value(view)?))?
            }
            "project.asset.reconcile" => {
                let asset = asset_id(args)?;
                let view = store
                    .transact(&scoped, |graph| {
                        reconcile(
                            graph,
                            &scoped,
                            context,
                            &owner,
                            root,
                            &asset,
                            byte_limit(args),
                            false,
                        )?;
                        graph.inspect(&scoped, &asset)
                    })
                    .map_err(graph_error)?;
                serde_json::to_value(view)?
            }
            "project.asset.rebind" => {
                let asset = asset_id(args)?;
                let generation = args["expected_generation"]
                    .as_u64()
                    .ok_or_else(|| Error::invalid("Binding generation required"))?;
                let locator = g::DurableLocator::ScopedFile {
                    root: root_name.into(),
                    relative_path: arg_str(args, "path")?.into(),
                };
                let reason = arg_str(args, "reason")?.into();
                let view = store
                    .transact(&scoped, |graph| {
                        graph.rebind(&scoped, &asset, generation, locator, reason)?;
                        reconcile(
                            graph,
                            &scoped,
                            context,
                            &owner,
                            root,
                            &asset,
                            byte_limit(args),
                            true,
                        )?;
                        graph.inspect(&scoped, &asset)
                    })
                    .map_err(graph_error)?;
                serde_json::to_value(view)?
            }
            "project.asset.tombstone" => {
                let asset = asset_id(args)?;
                store
                    .transact(&scoped, |graph| graph.tombstone(&scoped, &asset))
                    .map_err(graph_error)?;
                json!({"tombstoned":true,"user_files_deleted":false,"snapshot":store.graph().map_err(graph_error)?.snapshot_revision()})
            }
            "project.query" => {
                let query: g::AssetQuery = decode(args.get("query").unwrap_or(&json!({})))?;
                let limit = args["limit"].as_u64().unwrap_or(50) as usize;
                serde_json::to_value(
                    self.cursors
                        .page(
                            store.graph().map_err(graph_error)?,
                            &scoped,
                            &query,
                            args["cursor"].as_str(),
                            limit,
                        )
                        .map_err(graph_error)?,
                )?
            }
            "project.impact" => {
                let budget: g::TraversalBudget = decode(
                    args.get("budget")
                        .unwrap_or(&json!({"nodes":1000,"edges":5000,"depth":32,"results":256})),
                )?;
                serde_json::to_value(
                    store
                        .graph()
                        .map_err(graph_error)?
                        .impact(
                            &scoped,
                            &asset_id(args)?,
                            budget,
                            &Cancel(context.cancellation.clone()),
                        )
                        .map_err(graph_error)?,
                )?
            }
            "project.revisions" => {
                let asset = asset_id(args)?;
                let graph = store.graph().map_err(graph_error)?;
                graph.inspect(&scoped, &asset).map_err(graph_error)?;
                let after = args["after"]
                    .as_str()
                    .map(|s| g::AssetRevision::parse(s.into()))
                    .transpose()
                    .map_err(graph_error)?;
                let revisions = graph
                    .revisions(
                        &full,
                        &asset,
                        after.as_ref(),
                        args["limit"].as_u64().unwrap_or(50) as usize,
                    )
                    .map_err(graph_error)?;
                let summaries: Vec<_> = revisions.into_iter().map(|r| json!({"pin":r.pin,"observed_unix_ms":r.observed_unix_ms,"binding_generation":r.binding_generation,"method":r.observation.method,"method_version":r.observation.method_version,"coverage":r.coverage})).collect();
                json!({"revisions":summaries,"snapshot":graph.snapshot_revision()})
            }
            "project.edge.declare" => {
                let from: g::LogicalAssetId = decode(&args["from"])?;
                let to: g::LogicalAssetId = decode(&args["to"])?;
                let relation: g::Relation = decode(&args["relation"])?;
                let edge = g::Edge {
                    from: g::Vertex::Asset(from),
                    to: g::Vertex::Asset(to),
                    relation,
                    evidence: g::EdgeEvidence::Declared {
                        declaration: g::ReceiptId::new(),
                    },
                };
                store
                    .transact(&scoped, |graph| graph.declare(&scoped, edge))
                    .map_err(graph_error)?;
                json!({"declared":true,"execution_certified":false,"snapshot":store.graph().map_err(graph_error)?.snapshot_revision()})
            }
            "project.manifest.export" => {
                let ids: Vec<g::LogicalAssetId> = decode(&args["assets"])?;
                serde_json::to_value(
                    store
                        .graph()
                        .map_err(graph_error)?
                        .export_manifest(&scoped, &ids)
                        .map_err(graph_error)?,
                )?
            }
            "project.manifest.import" => {
                let manifest: g::PortableManifest = decode(&args["manifest"])?;
                let mapping = store
                    .transact(&full, |graph| graph.import_manifest(&full, &manifest))
                    .map_err(graph_error)?;
                let mapping: Vec<_> = mapping
                    .into_iter()
                    .map(|(source, local)| json!({"source":source,"local":local}))
                    .collect();
                json!({"mapping":mapping,"imported_trust":"declarations_only","snapshot":store.graph().map_err(graph_error)?.snapshot_revision()})
            }
            _ => {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "Unknown Project Graph route",
                ));
            }
        };
        Ok(json!({"project":id,"graph_schema":1,"result":result}))
    }
}
fn byte_limit(args: &Value) -> usize {
    args["max_bytes"].as_u64().unwrap_or(4 * 1024 * 1024) as usize
}
fn reconcile(
    graph: &mut g::ProjectGraph,
    access: &g::ProjectAccess,
    context: &Context,
    owner: &Owner,
    grant: &semwright_policy::FilesystemGrant,
    id: &g::LogicalAssetId,
    limit: usize,
    explicit_binding: bool,
) -> g::Result<()> {
    if context.cancellation.is_cancelled() {
        return Err(g::GraphError::Cancelled);
    }
    let view = graph.inspect(access, id)?;
    let Some(g::DurableLocator::ScopedFile {
        root,
        relative_path,
    }) = &view.asset.locator
    else {
        return Err(g::GraphError::Invalid("Native resolver not registered"));
    };
    if root != &grant.name || !grant.read {
        return Err(g::GraphError::Denied);
    }
    let observed = (|| -> Result<semwright_platform_api::filesystem::ScopedFileObservation> {
        let native = semwright_platform_services::filesystem()
            .open_root(&grant.path, true, false)
            .map_err(|_| Error::unavailable("Project root unavailable"))?;
        native.observe_file(Path::new(relative_path), limit)
    })();
    if context.cancellation.is_cancelled() {
        return Err(g::GraphError::Cancelled);
    }
    let observation = match observed {
        Ok(value) => value,
        Err(error) => {
            let outcome = match error.code {
                ErrorCode::NotFound => g::ProbeOutcome::ConclusiveNotFound,
                ErrorCode::PolicyDenied
                | ErrorCode::PermissionDenied
                | ErrorCode::SandboxDenied => g::ProbeOutcome::Denied,
                ErrorCode::Unavailable | ErrorCode::Timeout => g::ProbeOutcome::Offline,
                ErrorCode::Conflict | ErrorCode::Unsupported => g::ProbeOutcome::Ambiguous,
                _ => g::ProbeOutcome::Failed,
            };
            return graph.record_probe(access, id, outcome);
        }
    };
    let instance = Digest::of_bytes(observation.instance_identity.as_bytes());
    match graph.bound_instance(access, id)? {
        Some(bound) if bound != instance => {
            return graph.record_probe(access, id, g::ProbeOutcome::Ambiguous);
        }
        None if explicit_binding => {
            graph.bind_instance(access, id, view.binding_generation, instance)?
        }
        None => return graph.record_probe(access, id, g::ProbeOutcome::Ambiguous),
        _ => (),
    }
    let digest = Digest::of_bytes(&observation.bytes);
    let resource = c::ResourceKey {
        provider: "core".into(),
        resource: id.as_str().into(),
    };
    let base = c::BaseStateSet(vec![c::BaseState {
        key: resource.clone(),
        document_id: graph.project_id().as_str().into(),
        provider_session: graph.observation_epoch().into(),
        generation: view.binding_generation.to_string(),
        revision: c::Revision::Fingerprint(digest.clone()),
        concurrency: c::Concurrency::BestEffortRevalidate,
    }]);
    let now = u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| g::GraphError::Invalid("Observation clock"))?
            .as_millis(),
    )
    .map_err(|_| g::GraphError::Invalid("Observation clock range"))?;
    let method = observation.method.to_owned();
    let adapter = g::RevisionAdapter::registered(
        resource.clone(),
        c::EvidenceSource::FileRead,
        method.clone(),
        observation.method_version,
    )?;
    let candidate = g::RevisionCandidate {
        asset: id.clone(),
        fingerprint: g::Fingerprint {
            bytes: Some(digest.clone()),
            projection: None,
        },
        equivalence: g::Equivalence::ExactBytes,
        observed_unix_ms: now,
        binding_generation: view.binding_generation,
        observation: c::ObservationRef {
            id: format!("file-observation-{}", uuid::Uuid::new_v4()),
            base,
            source: c::EvidenceSource::FileRead,
            method,
            method_version: observation.method_version,
            scope: vec![c::Address {
                resource,
                logical_id: id.as_str().into(),
                property: "bytes".into(),
            }],
            artifact: Some(digest),
            exhaustive: true,
        },
        // Full bytes are observed; native dependency extraction is NOT provided by a generic read.
        coverage: g::Coverage::unknown(),
    };
    let admitted = adapter.admit(
        owner,
        graph.project_id(),
        id,
        view.binding_generation,
        candidate,
    )?;
    graph.accept_revision(access, admitted)
}
fn prospective_canonical_directory(directory: &Path) -> Result<PathBuf> {
    if !directory.is_absolute() {
        return Err(Error::invalid("Project state directory must be absolute"));
    }
    match std::fs::canonicalize(directory) {
        Ok(resolved) => {
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            if resolved != directory {
                return Err(Error::invalid(
                    "Project state path must use canonical spelling",
                ));
            }
            Ok(resolved)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = directory
                .parent()
                .ok_or_else(|| Error::invalid("Project state directory needs a parent"))?;
            let parent_resolved = std::fs::canonicalize(parent)?;
            let name = directory
                .file_name()
                .ok_or_else(|| Error::invalid("Project state directory needs a final component"))?;
            let candidate = parent_resolved.join(name);
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            if candidate != directory {
                return Err(Error::invalid(
                    "Project state parent must use canonical spelling",
                ));
            }
            Ok(candidate)
        }
        Err(error) => Err(error.into()),
    }
}
impl Broker {
    /// Trusted host initialization. `authenticated_principal` must come from OS/server
    /// authentication, never from a command argument, project manifest or stored session ref.
    pub fn configure_project_graphs(
        &self,
        directory: &Path,
        authenticated_principal: String,
    ) -> Result<()> {
        let resolved_state = prospective_canonical_directory(directory)?;
        // Reject both lexical and resolved overlap before creating private state.
        for grant in &self.policy.config().filesystem {
            if directory.starts_with(&grant.path) || grant.path.starts_with(directory) {
                return Err(Error::new(
                    ErrorCode::PolicyDenied,
                    "Project state overlaps an application filesystem grant",
                ));
            }
            if let Ok(resolved_grant) = std::fs::canonicalize(&grant.path)
                && (resolved_state.starts_with(&resolved_grant)
                    || resolved_grant.starts_with(&resolved_state))
            {
                return Err(Error::new(
                    ErrorCode::PolicyDenied,
                    "Project state resolves inside an application filesystem grant",
                ));
            }
        }
        let service = ProjectGraphs::new(directory, authenticated_principal)?;
        let mut state = self
            .project_graphs
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "Project service lock poisoned"))?;
        if state.is_some() {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Project service already configured",
            ));
        }
        *state = Some(service);
        Ok(())
    }
    pub(super) async fn project_graph_command(
        self: &Arc<Self>,
        context: Context,
        command: String,
        args: Value,
    ) -> Result<Value> {
        let broker = Arc::clone(self);
        tokio::task::spawn_blocking(move || {
            context.check_cancelled()?;
            let mut state = broker
                .project_graphs
                .lock()
                .map_err(|_| Error::new(ErrorCode::Internal, "Project service lock poisoned"))?;
            let manager = state.as_mut().ok_or_else(|| {
                Error::unavailable("Private Project Graph service is not configured by this host")
            })?;
            manager.execute(&broker.policy, &context, &command, &args)
        })
        .await
        .map_err(|_| Error::new(ErrorCode::Internal, "Project service task failed"))?
    }
}
