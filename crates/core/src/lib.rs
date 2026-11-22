//! The single authorization and execution authority used by every frontend.
pub mod audit;
mod catalog;
mod jobs;
mod project_graph;
pub use project_graph::RebuildCatalogSnapshot;
mod providers;
mod workflows;
use async_trait::async_trait;
use providers::{Invocation, ProviderCatalog, ProviderLease};
use semwright_backend_api::{Backend, Context};
use semwright_plugin_host::Host;
use semwright_plugin_sdk::Manifest;
use semwright_policy::{Policy, confirmation_summary};
use semwright_recipes::{Executor, Recipe};
use semwright_registry::Registry;
use semwright_types::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex as StdMutex, RwLock as StdRwLock},
    time::{Duration, Instant},
};
use tokio::sync::{RwLock, broadcast};
use tokio_util::sync::CancellationToken;

pub struct Approval {
    pub command: String,
    pub summary: String,
    pub backend: String,
    pub risk: Risk,
}
#[async_trait]
pub trait Approver: Send + Sync {
    async fn approve(&self, approval: Approval, cancellation: CancellationToken) -> Result<bool>;
}
pub struct NoApprover;
#[async_trait]
impl Approver for NoApprover {
    async fn approve(&self, _: Approval, _: CancellationToken) -> Result<bool> {
        Err(Error::new(
            ErrorCode::ConsentRequired,
            "A trusted operator must approve this action in the broker's foreground console; no approval API is exposed",
        ))
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    pub sequence: u64,
    pub event: EventEnvelope,
    /// Internal delivery scope. Never serialized into the event payload.
    pub audience: Option<String>,
}
struct Events {
    sequence: u64,
    history: VecDeque<Event>,
}

/// One direct Broker execution belongs to one session until its result has
/// finished auditing. Dropping the caller future also unregisters it.
struct ActiveCall {
    broker: Arc<Broker>,
    session: String,
    key: String,
}
impl Drop for ActiveCall {
    fn drop(&mut self) {
        if let Ok(mut active) = self.broker.active_calls.lock()
            && let Some(calls) = active.get_mut(&self.session)
        {
            calls.remove(&self.key);
            if calls.is_empty() {
                active.remove(&self.session);
            }
        }
    }
}

pub struct Broker {
    registry: StdRwLock<ProviderCatalog>,
    policy: Policy,
    references: StdMutex<RefStore>,
    active_calls: StdMutex<BTreeMap<String, BTreeMap<String, CancellationToken>>>,
    audit: Arc<audit::Audit>,
    approver: Arc<dyn Approver>,
    plugins: Option<Arc<Host>>,
    execution_gate: RwLock<()>,
    features: RwLock<Option<(u64, Instant, Vec<Feature>)>>,
    runtime_stop: CancellationToken,
    provider_tasks: tokio_util::task::TaskTracker,
    job_tasks: tokio_util::task::TaskTracker,
    jobs: StdMutex<jobs::JobStore>,
    workflows: StdMutex<semwright_workflow::WorkflowManager>,
    project_graphs: StdMutex<Option<project_graph::ProjectGraphs>>,
    events: StdMutex<Events>,
    broadcast: broadcast::Sender<Event>,
    environment: Value,
    fake: bool,
    started: Instant,
}
fn validate_snapshot_scope(
    command: &str,
    args: &Value,
    backend: &str,
    reference_kind: Option<&str>,
) -> Result<()> {
    if command != "ui.snapshot" {
        return Ok(());
    }
    let has_ref = args.get("ref").is_some();
    let has_app = args.get("app").is_some();
    if has_ref && has_app {
        return Err(Error::invalid(
            "ui.snapshot accepts a live ref or app filter, never both",
        ));
    }
    if has_ref {
        if backend != "windows" {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "Reference-scoped ui.snapshot is currently supported only on Windows",
            ));
        }
        if !matches!(reference_kind, Some("win" | "ui")) {
            return Err(Error::invalid(
                "Windows ui.snapshot requires a window or UI element reference",
            ));
        }
    }
    if backend == "windows" && has_app {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "Windows ui.snapshot cannot apply an app filter; use an exact window ref",
        ));
    }
    Ok(())
}

impl Broker {
    pub fn new(
        policy: Policy,
        backends: Vec<Arc<dyn Backend>>,
        audit: Arc<audit::Audit>,
        approver: Arc<dyn Approver>,
        plugins: Option<Arc<Host>>,
        environment: Value,
        fake: bool,
    ) -> Result<Arc<Self>> {
        let mut map = BTreeMap::new();
        for backend in backends {
            if map.insert(backend.name().to_owned(), backend).is_some() {
                return Err(Error::invalid("Duplicate backend identity"));
            }
        }
        if !fake && map.contains_key("fake") {
            return Err(Error::invalid(
                "Fake backend cannot be mixed into a real broker",
            ));
        }
        if fake
            && map
                .keys()
                .any(|name| name != "fake" && name != "filesystem" && name != "artifacts")
        {
            return Err(Error::invalid(
                "Fake mode cannot instantiate live desktop/application backends",
            ));
        }
        let (sender, _) = broadcast::channel(256);
        let runtime_stop = CancellationToken::new();
        let catalog = ProviderCatalog::bootstrap(Registry::builtin()?, map, &runtime_stop);
        let broker = Arc::new(Self {
            registry: StdRwLock::new(catalog),
            policy,
            references: StdMutex::new(RefStore::new(Duration::from_secs(60), 65536)),
            active_calls: StdMutex::new(BTreeMap::new()),
            audit,
            approver,
            plugins,
            execution_gate: RwLock::new(()),
            features: RwLock::new(None),
            runtime_stop,
            provider_tasks: tokio_util::task::TaskTracker::new(),
            job_tasks: tokio_util::task::TaskTracker::new(),
            jobs: StdMutex::new(jobs::JobStore::default()),
            workflows: StdMutex::new(semwright_workflow::WorkflowManager::default()),
            project_graphs: StdMutex::new(None),
            events: StdMutex::new(Events {
                sequence: 0,
                history: VecDeque::new(),
            }),
            broadcast: sender,
            environment,
            fake,
            started: Instant::now(),
        });
        broker.start_builtin_provider_watchers()?;
        Ok(broker)
    }
    pub fn describe(&self, command: &str) -> Result<CommandDescriptor> {
        Ok(self
            .registry
            .read()
            .map_err(|_| Error::new(ErrorCode::Internal, "Command registry lock poisoned"))?
            .describe(command)?
            .clone())
    }
    pub fn revoke_session(&self, session: &str) {
        // Cancel direct in-flight requests as well as managed jobs. Their
        // providers receive the original cancellation signal; if an operation
        // was already dispatched its outcome must remain uncertain.
        let pending = self
            .active_calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(session)
            .unwrap_or_default();
        for cancellation in pending.into_values() {
            cancellation.cancel();
        }
        if let Ok(mut store) = self.references.lock() {
            store.revoke_session(session);
        }
        if let Ok(mut jobs) = self.jobs.lock() {
            jobs.revoke_session(session);
        }
        if let Ok(mut workflows) = self.workflows.lock() {
            workflows.revoke_session(session);
        }
        if let Ok(mut project_graphs) = self.project_graphs.lock()
            && let Some(project_graphs) = project_graphs.as_mut()
        {
            project_graphs.revoke_session(session);
        }
    }
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.broadcast.subscribe()
    }
    pub fn replay(&self, after: u64) -> Result<Vec<Event>> {
        self.replay_visible(None, after)
    }

    pub fn replay_for(&self, session: &str, after: u64) -> Result<Vec<Event>> {
        self.replay_visible(Some(session), after)
    }

    fn replay_visible(&self, session: Option<&str>, after: u64) -> Result<Vec<Event>> {
        let events = self
            .events
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "Event history lock poisoned"))?;
        if after > events.sequence {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Event cursor belongs to a different broker generation",
            ));
        }
        if events
            .history
            .front()
            .is_some_and(|event| after > 0 && event.sequence > after.saturating_add(1))
        {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Event cursor expired; re-snapshot and subscribe from zero",
            ));
        }
        Ok(events
            .history
            .iter()
            .filter(|event| {
                event.sequence > after
                    && event
                        .audience
                        .as_deref()
                        .is_none_or(|audience| Some(audience) == session)
            })
            .cloned()
            .collect())
    }

    fn event(&self, event: EventEnvelope) {
        self.publish_event(event, None);
    }

    fn session_event(&self, session: &str, event: EventEnvelope) {
        self.publish_event(event, Some(session.to_owned()));
    }

    fn publish_event(&self, event: EventEnvelope, audience: Option<String>) {
        if event.validate().is_err() {
            return;
        }
        let Ok(value) = serde_json::to_value(&event) else {
            return;
        };
        if semwright_registry::bounds::value_budget(&value).is_err()
            || serde_json::to_vec(&value).is_ok_and(|bytes| bytes.len() > 65_536)
        {
            return;
        }
        if let Ok(mut events) = self.events.lock() {
            events.sequence = events.sequence.saturating_add(1);
            let event = Event {
                sequence: events.sequence,
                event,
                audience,
            };
            if events.history.len() == 256 {
                events.history.pop_front();
            }
            events.history.push_back(event.clone());
            let _ = self.broadcast.send(event);
        }
    }

    fn core_event(&self, kind: &str) -> EventEnvelope {
        EventEnvelope::new(kind, "semwright-core", event_time())
    }
    pub async fn probe(&self) -> Vec<Feature> {
        let revision = self.catalog_revision().unwrap_or(u64::MAX);
        if let Some((cached_revision, when, features)) = &*self.features.read().await
            && *cached_revision == revision
            && when.elapsed() < Duration::from_secs(5)
        {
            return features.clone();
        }
        let mut jobs = tokio::task::JoinSet::new();
        for provider in self.providers() {
            jobs.spawn(async move {
                let name = provider.name().to_owned();
                if !provider.active() {
                    return vec![Feature {
                        backend: name,
                        capability: "provider".into(),
                        status: CapabilityStatus::Unavailable,
                        reason: "Provider generation is inactive".into(),
                        remediation:
                            "Reconnect or refresh the provider through owner configuration".into(),
                    }];
                }
                let mut features = tokio::time::timeout(Duration::from_secs(3), provider.probe())
                    .await
                    .unwrap_or_else(|_| {
                        vec![Feature {
                            backend: name.clone(),
                            capability: "probe".into(),
                            status: CapabilityStatus::Unavailable,
                            reason: "Provider probe exceeded three seconds".into(),
                            remediation: "Run the provider health check".into(),
                        }]
                    });
                features.truncate(2048);
                for feature in &mut features {
                    feature.backend = name.clone();
                    if feature.capability.len() > 128 || !provider.active() {
                        feature.status = CapabilityStatus::Unavailable;
                    }
                    if provider.identity.kind != SourceKind::Builtin {
                        // Application-supplied error/health text is not a diagnostic log or instruction channel.
                        feature.reason =
                            "Owner-configured external provider operation probe".into();
                        feature.remediation =
                            "Inspect the provider connection; availability is not authorization"
                                .into();
                    }
                }
                features
            });
        }
        let mut features = vec![];
        while let Some(result) = jobs.join_next().await {
            if let Ok(mut rows) = result {
                features.append(&mut rows);
            }
        }
        features.sort_by(|a, b| (&a.backend, &a.capability).cmp(&(&b.backend, &b.capability)));
        if self.catalog_revision().ok() == Some(revision) {
            *self.features.write().await = Some((revision, Instant::now(), features.clone()));
        }
        features
    }
    async fn choose(
        &self,
        descriptor: &CommandDescriptor,
        request: &ExecuteRequest,
        reference: Option<&Reference>,
        dynamic_provider: Option<&Arc<ProviderLease>>,
    ) -> Result<String> {
        if let Some(provider) = dynamic_provider {
            if request
                .backend
                .as_ref()
                .is_some_and(|route| route != &provider.identity.id)
            {
                return Err(Error::new(
                    ErrorCode::PolicyDenied,
                    "Provider capabilities cannot select another execution route",
                ));
            }
            if !provider.active() || !provider.supports(&descriptor.name) {
                return Err(Error::unavailable(
                    "Provider capability generation is inactive",
                ));
            }
            let features = self.probe().await;
            let key = provider
                .operation_feature(&descriptor.name)
                .ok_or_else(|| {
                    Error::unavailable("Provider operation has no availability contract")
                })?;
            if !provider.active()
                || !features.iter().any(|feature| {
                    feature.backend == provider.identity.id
                        && feature.capability == key
                        && feature.usable()
                })
            {
                return Err(Error::unavailable(
                    "Provider operation is not currently available",
                ));
            }
            return Ok(provider.identity.id.clone());
        }
        if descriptor.backends == ["core"] && request.command != "ui.find" {
            if request
                .backend
                .as_deref()
                .is_some_and(|name| name != "core")
            {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "This command is implemented by the core",
                ));
            }
            return Ok("core".into());
        }
        if descriptor.backends == ["plugin"] {
            if request
                .backend
                .as_deref()
                .is_some_and(|name| name != "plugin")
            {
                return Err(Error::new(
                    ErrorCode::PolicyDenied,
                    "Plugin commands cannot select a native backend",
                ));
            }
            if self.plugins.is_none() {
                return Err(Error::unavailable("Plugin host is absent"));
            }
            return Ok("plugin".into());
        }
        let is_input =
            request.command.starts_with("input.") || request.command.starts_with("pointer.");
        if let Some(reference) = reference
            && !is_input
        {
            if request
                .backend
                .as_ref()
                .is_some_and(|name| name != &reference.backend)
            {
                return Err(Error::new(
                    ErrorCode::PolicyDenied,
                    "Reference is bound to its origin backend; migration is forbidden",
                ));
            }
            let backend = self.provider(&reference.backend).ok_or_else(|| {
                Error::new(ErrorCode::StaleReference, "Origin backend disappeared")
            })?;
            let actual_command = if request.command == "ui.find" {
                "ui.snapshot"
            } else {
                &request.command
            };
            if !backend.supports(actual_command) {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "The reference's backend does not implement this command",
                ));
            }
            return Ok(reference.backend.clone());
        }
        let actual_command = if request.command == "ui.find" {
            "ui.snapshot"
        } else {
            &request.command
        };
        let candidates = if request.command == "ui.find" {
            self.describe("ui.snapshot")?.backends
        } else {
            descriptor.backends.clone()
        };
        if self.fake
            && self
                .provider("fake")
                .is_some_and(|b| b.supports(actual_command))
        {
            if request
                .backend
                .as_deref()
                .is_some_and(|name| name != "fake")
            {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "Fake mode only dispatches to the fixture",
                ));
            }
            return Ok("fake".into());
        }
        // A caller-specified route is a constraint, not an authority or a
        // fallback hint. A route absent from the descriptor is an invalid
        // request, even if a different live provider could serve the command.
        if let Some(preferred) = request.backend.as_deref()
            && !candidates.iter().any(|route| route == preferred)
        {
            let mut error = Error::invalid(
                "Explicit backend is not a declared route for this command; omit backend to select an available route",
            );
            error.candidates = candidates;
            return Err(error);
        }
        let features = self.probe().await;
        for name in candidates {
            if request
                .backend
                .as_ref()
                .is_some_and(|preferred| preferred != &name)
            {
                continue;
            }
            if self
                .provider(&name)
                .is_some_and(|backend| backend.supports(actual_command))
                && self
                    .provider(&name)
                    .and_then(|backend| backend.operation_feature(actual_command))
                    .is_some_and(|key| {
                        features.iter().any(|feature| {
                            feature.backend == name && feature.capability == key && feature.usable()
                        })
                    })
            {
                return Ok(name);
            }
        }
        Err(Error::unavailable(
            "No permitted live backend is available for this command; inspect doctor and capability status",
        ))
    }
    fn resolve_reference_field(
        &self,
        session: &str,
        args: &Value,
        field: &str,
    ) -> Result<Option<Reference>> {
        args.get(field)
            .map(|value| {
                let id = value
                    .as_str()
                    .ok_or_else(|| Error::invalid(format!("{field} must be a reference string")))?;
                self.references
                    .lock()
                    .map_err(|_| Error::new(ErrorCode::Internal, "Reference store lock poisoned"))?
                    .get(id, session)
            })
            .transpose()
    }
    fn resolve_reference(&self, session: &str, args: &Value) -> Result<Option<Reference>> {
        self.resolve_reference_field(session, args, "ref")
    }
    /// ui.find's ancestor is an origin-bound reference, just like a direct ref.
    /// Resolve it before choosing the backend and checking application policy;
    /// it must never become an untrusted hint for a desktop-wide search.
    fn resolve_request_reference(
        &self,
        session: &str,
        request: &ExecuteRequest,
    ) -> Result<Option<Reference>> {
        if request.command == "ui.find" {
            self.resolve_reference_field(session, &request.args["selector"], "ancestor")
        } else {
            self.resolve_reference(session, &request.args)
        }
    }
    fn application<'a>(
        &self,
        request: &'a ExecuteRequest,
        reference: Option<&'a Reference>,
        metadata: &'a semwright_registry::Metadata,
    ) -> Option<&'a str> {
        if metadata.untrusted_metadata {
            return metadata.app.as_deref();
        }
        metadata
            .app
            .as_deref()
            .or_else(|| reference.map(|r| r.target.app.as_str()))
            .or_else(|| request.args.get("app").and_then(Value::as_str))
            .or_else(|| {
                request
                    .args
                    .pointer("/selector/app")
                    .and_then(Value::as_str)
            })
    }
    fn materialize(&self, session: &str, backend: &str, value: &mut Value) -> Result<()> {
        fn walk(
            session: &str,
            backend: &str,
            value: &mut Value,
            store: &mut RefStore,
            seen: &mut BTreeMap<String, String>,
            depth: usize,
        ) -> Result<()> {
            if depth > 64 {
                return Err(Error::new(
                    ErrorCode::ResourceExhausted,
                    "Backend result nesting exceeds budget",
                ));
            }
            match value {
                Value::Object(map) if map.contains_key("$ref") => {
                    if map.len() != 1 {
                        return Err(Error::new(
                            ErrorCode::BackendFailed,
                            "Malformed internal reference marker",
                        ));
                    }
                    let target: NativeTarget = serde_json::from_value(map["$ref"].clone())?;
                    let key = serde_json::to_string(&target)?;
                    let id = if let Some(id) = seen.get(&key) {
                        id.clone()
                    } else {
                        let id = store.insert(session, backend, target)?;
                        seen.insert(key, id.clone());
                        id
                    };
                    *value = Value::String(id);
                }
                Value::Object(map) => {
                    for child in map.values_mut() {
                        walk(session, backend, child, store, seen, depth + 1)?;
                    }
                }
                Value::Array(values) => {
                    if values.len() > 10000 {
                        return Err(Error::new(
                            ErrorCode::ResourceExhausted,
                            "Backend result array too large",
                        ));
                    }
                    for child in values {
                        walk(session, backend, child, store, seen, depth + 1)?;
                    }
                }
                _ => (),
            }
            Ok(())
        }
        let mut store = self
            .references
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "Reference store lock poisoned"))?;
        walk(session, backend, value, &mut store, &mut BTreeMap::new(), 0)
    }
    fn filter_apps(&self, value: &mut Value) -> Result<()> {
        let apps = &self.policy.config().apps;
        if apps.is_empty() {
            return Ok(());
        }
        for key in ["nodes", "windows", "apps"] {
            if let Some(values) = value.get_mut(key).and_then(Value::as_array_mut) {
                values.retain(|row| {
                    row.get("app")
                        .and_then(Value::as_str)
                        .is_some_and(|app| apps.contains(app))
                });
            }
        }
        if let Some(node) = value.get("node").and_then(Value::as_object)
            && let Some(app) = node.get("app").and_then(Value::as_str)
            && !apps.contains(app)
        {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Observed semantic target is outside the configured application scope",
            ));
        }
        Ok(())
    }
    fn track_active_call(
        self: &Arc<Self>,
        session: &str,
        parent: &CancellationToken,
    ) -> Result<(ActiveCall, CancellationToken)> {
        // The local child can be cancelled on session revocation without
        // cancelling work owned by a different session or the transport itself.
        let cancellation = parent.child_token();
        let key = unique_id();
        let mut active = self.active_calls.lock().map_err(|_| {
            Error::new(
                ErrorCode::Internal,
                "Active execution registry is unavailable",
            )
        })?;
        let calls = active.entry(session.to_owned()).or_default();
        if calls.len() >= 256 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Session already has too many in-flight Core operations",
            ));
        }
        if calls.contains_key(&key) {
            return Err(Error::new(
                ErrorCode::Internal,
                "Duplicate active execution identity",
            ));
        }
        calls.insert(key.clone(), cancellation.clone());
        Ok((
            ActiveCall {
                broker: self.clone(),
                session: session.to_owned(),
                key,
            },
            cancellation,
        ))
    }
    pub fn execute(
        self: Arc<Self>,
        session: String,
        id: String,
        request: ExecuteRequest,
        cancellation: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Envelope> + Send>> {
        // Register synchronously, before the returned future is first polled,
        // so a revocation cannot miss an already-submitted pending operation.
        let tracked = self.track_active_call(&session, &cancellation);
        Box::pin(async move {
            let (_active_call, session_cancellation) = match tracked {
                Ok(tracked) => tracked,
                Err(error) => {
                    return Envelope::finish(
                        id,
                        request.command,
                        "core".into(),
                        Duration::ZERO,
                        request.dry_run,
                        Err(error),
                    );
                }
            };
            self.execute_inner(session, id, request, session_cancellation)
                .await
        })
    }
    async fn execute_inner(
        self: Arc<Self>,
        session: String,
        id: String,
        request: ExecuteRequest,
        cancellation: CancellationToken,
    ) -> Envelope {
        let started = Instant::now();
        let descriptor = self.invocation(&request.command);
        let trace_context = descriptor.as_ref().ok().map(|invocation| {
            (
                invocation.capability.descriptor.clone(),
                invocation.capability.metadata.clone(),
            )
        });
        let safe_name = descriptor
            .as_ref()
            .map(|d| d.capability.descriptor.name.clone())
            .unwrap_or_else(|_| "unregistered".into());
        let provenance = descriptor
            .as_ref()
            .ok()
            .map(|invocation| invocation.capability.provenance());
        let mut audit = match self
            .audit
            .begin_with_provenance(&safe_name, &id, &session, provenance)
        {
            Ok(scope) => scope,
            Err(error) => {
                return Envelope::finish(
                    id,
                    safe_name,
                    "core".into(),
                    started.elapsed(),
                    request.dry_run,
                    Err(error),
                );
            }
        };
        self.event(
            self.core_event("command_start")
                .with_attribute("command", json!(safe_name))
                .with_attribute("request_id", json!(id)),
        );
        let mut selected = "unselected".to_owned();
        let result = self
            .perform(
                &session,
                &id,
                &request,
                descriptor,
                cancellation,
                &mut selected,
                &mut audit,
            )
            .await;
        let result = match audit.finish(&result) {
            Ok(()) => result,
            Err(_) => Err(Error::new(
                ErrorCode::Internal,
                "Audit completion could not be persisted; action outcome may already have occurred",
            )
            .uncertain()),
        };
        if let Some((trace_descriptor, trace_metadata)) = &trace_context {
            self.record_workflow_execution(
                &session,
                &request,
                trace_descriptor,
                trace_metadata,
                &selected,
                started.elapsed().as_millis().min(u64::MAX as u128) as u64,
                &result,
            );
        }
        self.event(
            self.core_event("command_finish")
                .with_attribute("command", json!(safe_name))
                .with_attribute("request_id", json!(id))
                .with_attribute("backend", json!(selected))
                .with_attribute("ok", json!(result.is_ok()))
                .with_attribute("error_code", json!(result.as_ref().err().map(|e| e.code))),
        );
        let mut envelope = Envelope::finish(
            id,
            request.command,
            selected,
            started.elapsed(),
            request.dry_run,
            result,
        );
        envelope.execution.policy_decision = audit.policy_decision().into();
        envelope.execution.provenance = audit.provenance();
        envelope
    }
    async fn perform(
        self: &Arc<Self>,
        session: &str,
        request_id: &str,
        request: &ExecuteRequest,
        invocation: Result<Invocation>,
        cancellation: CancellationToken,
        selected: &mut String,
        audit: &mut audit::Scope,
    ) -> Result<Value> {
        let Invocation {
            capability,
            dynamic_provider,
        } = invocation?;
        let descriptor = &capability.descriptor;
        capability.validate_input(&request.args)?;
        let native_ref_capable = capability.metadata.source == SourceKind::Builtin
            || dynamic_provider
                .as_ref()
                .is_some_and(|provider| provider.emits_native_refs());
        if cancellation.is_cancelled() {
            return Err(Error::new(
                ErrorCode::Cancelled,
                "Cancelled before authorization",
            ));
        }
        let reference = if native_ref_capable {
            self.resolve_request_reference(session, request)?
        } else {
            None
        };
        let needs_confirmation = match self.policy.enforce(
            descriptor,
            &request.args,
            self.application(request, reference.as_ref(), &capability.metadata),
        ) {
            Ok(value) => value,
            Err(error) => {
                audit.decision("deny");
                return Err(error);
            }
        };
        audit.decision(if needs_confirmation {
            "require_confirmation"
        } else {
            "allow"
        });
        // Job control must remain outside the execution gate: cancelling/observing a long
        // mutation cannot wait behind the mutation it controls. The nested target is executed
        // through Broker::execute again and therefore receives its own full policy/audit pass.
        if request.command.starts_with("jobs.") {
            *selected = "core".into();
            audit.backend(selected);
            audit.executed_provider("semwright-core", None);
            if request.dry_run && !descriptor.dry_run {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "This job-control command has no dry-run contract",
                ));
            }
            let output = match request.command.as_str() {
                "jobs.start" => {
                    let nested: ExecuteRequest =
                        serde_json::from_value(request.args["request"].clone())?;
                    json!({"job":self.start_job(session, nested)?})
                }
                "jobs.list" => json!({"jobs":self.list_jobs(session)?}),
                "jobs.get" => {
                    json!({"job":self.get_job(session, arg_str(&request.args, "job_id")?)?})
                }
                "jobs.cancel" => {
                    json!({"job":self.cancel_job(session, arg_str(&request.args, "job_id")?)?})
                }
                _ => {
                    return Err(Error::new(
                        ErrorCode::Unsupported,
                        "Unknown job-control command",
                    ));
                }
            };
            capability.validate_output(&output)?;
            return Ok(output);
        }
        // Nested recipes re-enter this method for each step; never hold a gate across that recursion.
        if request.command == "recipe.run" {
            *selected = "core".into();
            audit.backend(selected);
            audit.executed_provider("semwright-core", None);
            let recipe: Recipe = serde_json::from_value(request.args["recipe"].clone())?;
            let bridge = BrokerSessionExecutor {
                broker: self.clone(),
                session: session.into(),
            };
            let output = recipe
                .run(
                    &bridge,
                    request
                        .args
                        .get("inputs")
                        .cloned()
                        .unwrap_or_else(|| json!({})),
                    request.dry_run,
                    cancellation,
                )
                .await?;
            capability.validate_output(&output)?;
            return Ok(output);
        }
        if request.command == "workflow.replay" {
            *selected = "core".into();
            audit.backend(selected);
            audit.executed_provider("semwright-core", None);
            let output = self
                .workflow_replay(
                    session,
                    arg_str(&request.args, "candidate_id")?,
                    request
                        .args
                        .get("inputs")
                        .cloned()
                        .unwrap_or_else(|| json!({})),
                    request.dry_run,
                    cancellation,
                )
                .await?;
            capability.validate_output(&output)?;
            return Ok(output);
        }
        if capability.metadata.source == SourceKind::Recipe {
            *selected = capability.metadata.provider.clone();
            audit.backend(selected);
            audit.executed_provider(&capability.metadata.provider, None);
            let output = self
                .execute_promoted_workflow(
                    session,
                    &request.command,
                    request.args.clone(),
                    request.dry_run,
                    cancellation,
                )
                .await?;
            capability.validate_output(&output)?;
            return Ok(output);
        }
        *selected = self
            .choose(
                descriptor,
                request,
                reference.as_ref(),
                dynamic_provider.as_ref(),
            )
            .await?;
        validate_snapshot_scope(
            &request.command,
            &request.args,
            selected,
            reference
                .as_ref()
                .map(|reference| reference.target.kind.as_str()),
        )?;
        audit.backend(selected);
        let selected_provider = dynamic_provider.or_else(|| self.provider(selected));
        let provider_stop = if let Some(provider) = &selected_provider {
            if !provider.active() {
                return Err(Error::unavailable(
                    "Provider generation changed before queuing",
                ));
            }
            audit.executed_provider(&provider.identity.id, Some(provider.generation));
            provider.epoch.clone()
        } else {
            audit.executed_provider(&capability.metadata.provider, None);
            self.runtime_stop.clone()
        };
        // All broker I/O is blocked during an operator prompt. The model cannot inspect or type approval.
        let exclusive = descriptor.risk.mutates()
            || descriptor.risk == Risk::SecretAccess
            || needs_confirmation;
        let read_guard;
        let write_guard;
        if exclusive {
            write_guard = Some(
                tokio::select! {biased; _=provider_stop.cancelled()=>return Err(Error::unavailable("Provider generation changed while queued")), _ = cancellation.cancelled()=>return Err(Error::new(ErrorCode::Cancelled,"Cancelled while queued")),guard=self.execution_gate.write()=>guard},
            );
            read_guard = None;
        } else {
            read_guard = Some(
                tokio::select! {biased; _=provider_stop.cancelled()=>return Err(Error::unavailable("Provider generation changed while queued")), _ = cancellation.cancelled()=>return Err(Error::new(ErrorCode::Cancelled,"Cancelled while queued")),guard=self.execution_gate.read()=>guard},
            );
            write_guard = None;
        }
        let _guards = (read_guard, write_guard);
        if request.dry_run {
            if !descriptor.dry_run {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "This command has no dry-run contract",
                ));
            }
            return Ok(
                json!({"dry_run":true,"command":descriptor.name,"backend":selected,"requires_confirmation":needs_confirmation,"capabilities":descriptor.requires,"side_effects":false,"target_live_validation":"not executed"}),
            );
        }
        if request.command == "plugin.install" {
            let manifest: Manifest = serde_json::from_value(request.args["manifest"].clone())?;
            self.plugins
                .as_ref()
                .ok_or_else(|| Error::unavailable("Plugin host not configured"))?
                .validate_install(&manifest)?;
        }
        if needs_confirmation {
            let summary = if request.command == "plugin.install" {
                let m = &request.args["manifest"];
                json!({"plugin":m["name"],"sha256":m["sha256"],"executable":m["executable"],"mounts":m["mounts"],"network":m["network"]}).to_string()
            } else {
                confirmation_summary(&request.command, &request.args)
            };
            if !self
                .approver
                .approve(
                    Approval {
                        command: request.command.clone(),
                        summary,
                        backend: selected.clone(),
                        risk: descriptor.risk,
                    },
                    cancellation.clone(),
                )
                .await?
            {
                return Err(Error::new(
                    ErrorCode::PolicyDenied,
                    "Operator declined or approval expired",
                ));
            }
        }
        if needs_confirmation {
            audit.decision("allow_after_confirmation");
        }
        let reference = if native_ref_capable {
            self.resolve_request_reference(session, request)?
        } else {
            None
        };
        let target_reference = if native_ref_capable {
            self.resolve_reference_field(session, &request.args, "target_ref")?
        } else {
            None
        };
        self.policy.enforce(
            descriptor,
            &request.args,
            self.application(request, reference.as_ref(), &capability.metadata),
        )?;
        let mut args = request.args.clone();
        let is_input =
            request.command.starts_with("input.") || request.command.starts_with("pointer.");
        if let Some(reference) = &reference {
            let backend = self.provider(&reference.backend).ok_or_else(|| {
                Error::new(ErrorCode::StaleReference, "Origin backend disappeared")
            })?;
            tokio::time::timeout(Duration::from_secs(3), backend.validate(&reference.target))
                .await
                .map_err(|_| {
                    Error::new(
                        ErrorCode::Timeout,
                        "Reference validation timed out before dispatch",
                    )
                })??;
            if is_input
                && (reference.target.kind != "win"
                    || !tokio::time::timeout(
                        Duration::from_secs(2),
                        backend.is_focused(&reference.target),
                    )
                    .await
                    .map_err(|_| {
                        Error::new(
                            ErrorCode::Timeout,
                            "Focus validation timed out before dispatch",
                        )
                    })??)
            {
                return Err(Error::new(
                    ErrorCode::Conflict,
                    "Input requires a still-focused window reference; no fallback was attempted",
                ));
            }
            args["_target"] = serde_json::to_value(&reference.target)?;
        } else if is_input {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Input without an explicit focused window is forbidden",
            ));
        }
        if let Some(target_reference) = &target_reference {
            let primary = reference.as_ref().ok_or_else(|| {
                Error::invalid("target_ref requires a primary ref in the same operation")
            })?;
            if target_reference.backend != primary.backend
                || target_reference.target.app != primary.target.app
            {
                return Err(Error::new(
                    ErrorCode::PolicyDenied,
                    "Secondary reference must belong to the same backend and application",
                ));
            }
            let backend = self.provider(&target_reference.backend).ok_or_else(|| {
                Error::new(
                    ErrorCode::StaleReference,
                    "Secondary reference backend disappeared",
                )
            })?;
            tokio::time::timeout(
                Duration::from_secs(3),
                backend.validate(&target_reference.target),
            )
            .await
            .map_err(|_| {
                Error::new(
                    ErrorCode::Timeout,
                    "Secondary reference validation timed out before dispatch",
                )
            })??;
            args["_target2"] = serde_json::to_value(&target_reference.target)?;
        }
        if cancellation.is_cancelled() {
            return Err(Error::new(
                ErrorCode::Cancelled,
                "Cancelled before dispatch",
            ));
        }
        let context = Context {
            session: session.into(),
            request_id: request_id.into(),
            cancellation: cancellation.child_token(),
        };
        let action = async {
            context.check_cancelled()?;
            if selected_provider.as_ref().is_some_and(|p| !p.active()) {
                return Err(Error::unavailable(
                    "Provider generation changed before dispatch",
                ));
            }
            let mut output = if request.command == "ui.find" {
                self.find(session, selected, &context, &args).await?
            } else if selected == "core" {
                self.core(&context, &request.command, &args).await?
            } else if selected == "plugin" {
                self.plugins
                    .as_ref()
                    .ok_or_else(|| Error::unavailable("Plugin host absent"))?
                    .execute(
                        &request.command,
                        request.args.clone(),
                        context.cancellation.clone(),
                    )
                    .await?
            } else {
                selected_provider
                    .as_ref()
                    .ok_or_else(|| Error::unavailable("Selected provider missing"))?
                    .execute(&context, descriptor, &args)
                    .await?
            };
            if request.command != "ui.find" {
                if capability.metadata.source == SourceKind::Builtin {
                    self.filter_apps(&mut output)?;
                }
                if selected_provider
                    .as_ref()
                    .is_some_and(|provider| provider.emits_native_refs())
                {
                    self.materialize(session, selected, &mut output)?;
                }
            }
            if serde_json::to_vec(&output)?.len() > MAX_FRAME - 8192 {
                return Err(Error::new(
                    ErrorCode::ResourceExhausted,
                    "Command output exceeds the broker frame budget; request a smaller snapshot",
                ));
            }
            capability.validate_output(&output)?;
            Ok(output)
        };
        tokio::pin!(action);
        // Signal cancellation before dropping a suspended provider invocation, including task aborts.
        let _cancel_on_drop = context.cancellation.clone().drop_guard();
        let result = tokio::select! {
            biased;
            _=provider_stop.cancelled()=>{
                context.cancellation.cancel();
                let _=tokio::time::timeout(Duration::from_millis(250), &mut action).await;
                Err(Error::unavailable("Provider generation invalidated during execution; inspect state before retrying").uncertain())
            },
            _=cancellation.cancelled()=>{
                context.cancellation.cancel();
                let _=tokio::time::timeout(Duration::from_millis(250), &mut action).await;
                Err(Error::new(ErrorCode::Cancelled,"Command cancelled; inspect target state before retrying").uncertain())
            },
            _=tokio::time::sleep(Duration::from_millis(descriptor.timeout_ms))=>{
                context.cancellation.cancel();
                let _=tokio::time::timeout(Duration::from_millis(250), &mut action).await;
                Err(Error::new(ErrorCode::Timeout,"Command timed out; inspect target state before retrying").uncertain())
            },
            result=&mut action=>result,
        };
        if descriptor.risk.mutates() {
            *self.features.write().await = None;
            result.map_err(|e| {
                if matches!(
                    e.code,
                    ErrorCode::BackendFailed | ErrorCode::Internal | ErrorCode::ResourceExhausted
                ) {
                    e.uncertain()
                } else {
                    e
                }
            })
        } else {
            result
        }
    }
    async fn find(
        &self,
        session: &str,
        backend: &str,
        context: &Context,
        args: &Value,
    ) -> Result<Value> {
        let mut selector: Selector = serde_json::from_value(args["selector"].clone())?;
        let max_nodes = args["max_nodes"].as_u64().unwrap_or(500).min(2_000) as usize;
        let max_depth = args["max_depth"].as_u64().unwrap_or(8).min(32) as usize;
        let mut snapshot_args = json!({"max_nodes":max_nodes,"max_depth":max_depth});
        // Windows cannot enforce an app filter during a native UIA snapshot.
        // A live, session-bound window/element ancestor is mandatory: it was
        // already validated by perform(), and the exact native target is
        // rederived from the broker's reference store rather than caller JSON.
        if backend == "windows" {
            let ancestor = self
                .resolve_reference_field(session, &args["selector"], "ancestor")?
                .ok_or_else(|| {
                    Error::invalid("Windows ui.find requires an exact live ancestor reference")
                })?;
            if ancestor.backend != backend {
                return Err(Error::new(
                    ErrorCode::PolicyDenied,
                    "UI discovery ancestor belongs to a different backend",
                ));
            }
            if !matches!(ancestor.target.kind.as_str(), "win" | "ui") {
                return Err(Error::invalid(
                    "Windows ui.find ancestor must be a window or UI element reference",
                ));
            }
            if selector
                .app
                .as_deref()
                .is_some_and(|app| app != ancestor.target.app)
            {
                return Err(Error::invalid(
                    "UI discovery app filter conflicts with its bound ancestor",
                ));
            }
            let allowed_apps = &self.policy.config().apps;
            if !allowed_apps.is_empty() && !allowed_apps.contains(&ancestor.target.app) {
                return Err(Error::new(
                    ErrorCode::PolicyDenied,
                    "UI discovery ancestor is outside the permitted application scope",
                ));
            }
            snapshot_args["_target"] = serde_json::to_value(&ancestor.target)?;
        } else if let Some(app) = &selector.app {
            snapshot_args["app"] = json!(app);
        }
        let provider = self
            .provider(backend)
            .ok_or_else(|| Error::unavailable("UI backend missing"))?;
        // Ancestor refs are broker-owned handles. Until a backend can prove an equivalent
        // native ancestry scope, preserve the existing bounded snapshot semantics.
        let pushed = if selector.ancestor.is_none() {
            provider
                .find_ui_candidates(context, &selector, max_nodes, max_depth)
                .await?
        } else {
            None
        };
        let mut output = match pushed {
            Some(output) => output,
            None => {
                provider
                    .execute(context, &self.describe("ui.snapshot")?, &snapshot_args)
                    .await?
            }
        };
        self.filter_apps(&mut output)?;
        self.materialize(session, backend, &mut output)?;
        let nodes: Vec<UiNode> = serde_json::from_value(output["nodes"].clone())?;
        if let Some(old) = selector.ancestor.clone() {
            let store = self
                .references
                .lock()
                .map_err(|_| Error::new(ErrorCode::Internal, "Reference lock poisoned"))?;
            let wanted = store.get(&old, session)?;
            let current = if backend == "windows"
                && wanted.target.kind == "win"
                && wanted.target.identity.starts_with("win:")
            {
                // window.list and UIA use different native identity namespaces.
                // Only the trusted Windows snapshot_hwnd path may rebind its
                // exact HWND to the first (root) UIA node. Never look for the
                // ancestor elsewhere in the desktop or accept another PID.
                let root = nodes.first().ok_or_else(|| {
                    Error::new(
                        ErrorCode::StaleReference,
                        "Scoped Windows snapshot returned no root element",
                    )
                })?;
                let root_ref = store.get(&root.reference, session)?;
                if root.parent_ref.is_some()
                    || root.app != wanted.target.app
                    || root_ref.backend != wanted.backend
                    || root_ref.target.app != wanted.target.app
                {
                    return Err(Error::new(
                        ErrorCode::PolicyDenied,
                        "Scoped Windows UIA root does not match its window ancestor",
                    ));
                }
                Some(root)
            } else {
                nodes.iter().find(|node| {
                    store.get(&node.reference, session).is_ok_and(|r| {
                        r.backend == wanted.backend
                            && r.target.identity == wanted.target.identity
                            && r.target.revision == wanted.target.revision
                            && r.target.fingerprint == wanted.target.fingerprint
                    })
                })
            };
            selector.ancestor = Some(
                current
                    .ok_or_else(|| {
                        Error::new(
                            ErrorCode::StaleReference,
                            "Ancestor reference is not present in the current bounded snapshot",
                        )
                    })?
                    .reference
                    .clone(),
            );
        }
        let matches = selector.select(&nodes)?;
        Ok(
            json!({"nodes":matches,"count":matches.len(),"partial":output["partial"],"revision":output["revision"],"mode":if selector.query.is_some(){"ranked_candidates"}else{"deterministic"},"ambiguity":"Discovery never invokes a candidate; use an explicit ref"}),
        )
    }
    async fn core(
        self: &Arc<Self>,
        context: &Context,
        command: &str,
        args: &Value,
    ) -> Result<Value> {
        let session = context.session.as_str();
        match command {
            "doctor" => Ok(
                json!({"project":"Semwright","version":env!("CARGO_PKG_VERSION"),"protocol":PROTOCOL_VERSION,"fake":self.fake,"environment":self.environment,"features":self.probe().await,"providers":self.provider_status()?,"policy":{"profile":self.policy.config().profile,"granted":self.policy.capabilities(),"apps":self.policy.config().apps,"policy_config_sha256":self.policy.grant_fingerprint(),"shell":false,"external_confirmation":"foreground operator only; unavailable in user service"},"uptime_seconds":self.started.elapsed().as_secs(),"verification":"Runtime capability probes are not a live desktop acceptance certificate","unimplemented":[]}),
            ),
            "capabilities.list" => Ok(
                json!({"granted":self.policy.capabilities(),"filesystem":self.policy.config().filesystem.iter().map(|r|json!({"name":r.name,"read":r.read,"write":r.write})).collect::<Vec<_>>(),"backends":self.probe().await,"fake":self.fake}),
            ),
            "capabilities.search" => {
                self.catalog_search(serde_json::from_value(args.clone())?)
                    .await
            }
            "capabilities.describe" => self.catalog_describe(arg_str(args, "name")?).await,
            "commands.search" => {
                let registry = self
                    .registry
                    .read()
                    .map_err(|_| Error::new(ErrorCode::Internal, "Registry lock poisoned"))?;
                Ok(
                    json!({"commands":registry.search(args["query"].as_str().unwrap_or(""),args["limit"].as_u64().unwrap_or(30)as usize)}),
                )
            }
            "commands.describe" => Ok(json!({"command":self.describe(arg_str(args,"name")?)?})),
            "audit.tail" => {
                Ok(json!({"events":self.audit.tail(args["limit"].as_u64().unwrap_or(30)as usize)?}))
            }
            "workflow.record.start" => self.workflow_record_start(
                session,
                arg_str(args, "name")?,
                args["intent"].as_str().unwrap_or(""),
                args["capture_values"].as_bool().unwrap_or(false),
            ),
            "workflow.record.stop" => {
                self.workflow_record_stop(session, args["successful"].as_bool().unwrap_or(true))
            }
            "workflow.traces.list" => self.workflow_traces(),
            "workflow.trace.get" => self.workflow_trace(arg_str(args, "trace_id")?),
            "workflow.trace.delete" => self.workflow_trace_delete(arg_str(args, "trace_id")?),
            "workflow.patterns.list" => {
                self.workflow_patterns(args["min_occurrences"].as_u64().unwrap_or(3) as usize)
            }
            "workflow.pattern.get" => self.workflow_pattern(arg_str(args, "pattern_id")?),
            "workflow.suggestions.list" => self.workflow_suggestions(
                args["min_occurrences"].as_u64().unwrap_or(3) as usize,
                args["include_dismissed"].as_bool().unwrap_or(false),
            ),
            "workflow.suggestion.get" => self.workflow_suggestion(arg_str(args, "suggestion_id")?),
            "workflow.suggestion.dismiss" => self.workflow_suggestion_dismiss(
                arg_str(args, "suggestion_id")?,
                args["permanent"].as_bool().unwrap_or(false),
            ),
            "workflow.suggestion.restore" => {
                self.workflow_suggestion_restore(arg_str(args, "suggestion_id")?)
            }
            "workflow.proposals.list" => self.workflow_proposals(
                session,
                args["min_occurrences"].as_u64().unwrap_or(3) as usize,
                args["include_dismissed"].as_bool().unwrap_or(false),
            ),
            "workflow.proposal.get" => {
                self.workflow_proposal(session, arg_str(args, "proposal_id")?)
            }
            "workflow.proposal.plan" => {
                self.workflow_proposal_plan(
                    session,
                    arg_str(args, "proposal_id")?,
                    args.get("inputs").cloned().unwrap_or_else(|| json!({})),
                    context.cancellation.clone(),
                )
                .await
            }
            "workflow.proposal.accept" => {
                self.workflow_proposal_accept(session, arg_str(args, "proposal_id")?)
            }
            "workflow.proposal.dismiss" => self.workflow_proposal_dismiss(
                session,
                arg_str(args, "proposal_id")?,
                args["permanent"].as_bool().unwrap_or(false),
            ),
            "workflow.suggestion.compile" => {
                let hints: Vec<semwright_workflow::ParameterHint> = serde_json::from_value(
                    args.get("parameters").cloned().unwrap_or_else(|| json!([])),
                )?;
                self.workflow_suggestion_compile(
                    arg_str(args, "suggestion_id")?,
                    args["name"].as_str(),
                    args["description"].as_str().unwrap_or(""),
                    hints,
                )
            }
            "workflow.compile" => {
                let trace_ids: Vec<String> = serde_json::from_value(args["trace_ids"].clone())?;
                let hints: Vec<semwright_workflow::ParameterHint> = serde_json::from_value(
                    args.get("parameters").cloned().unwrap_or_else(|| json!([])),
                )?;
                self.workflow_compile(
                    &trace_ids,
                    arg_str(args, "name")?,
                    args["description"].as_str().unwrap_or(""),
                    hints,
                )
            }
            "workflow.candidates.list" => self.workflow_candidates(),
            "workflow.candidate.get" => {
                Ok(json!({"candidate":self.workflow_candidate(arg_str(args,"candidate_id")?)?}))
            }
            "workflow.candidate.delete" => {
                self.workflow_candidate_delete(arg_str(args, "candidate_id")?)
            }
            "workflow.verify" => self.workflow_verify(session, arg_str(args, "candidate_id")?),
            "workflow.promote" => {
                self.workflow_promote(arg_str(args, "candidate_id")?, arg_str(args, "slug")?)
            }
            "workflow.promotions.list" => self.workflow_promotions(),
            "workflow.demote" => self.workflow_demote(arg_str(args, "slug")?),
            command if command.starts_with("project.") => {
                self.project_graph_command(context.clone(), command.into(), args.clone())
                    .await
            }
            "recipe.list" => Ok(
                json!({"recipes":["fake-export","fake-edit","blender-inspect","workspace-write"],"source":"Recipe files ship with the repository; clients load and submit their declarative contents"}),
            ),
            "recipe.validate" => {
                let recipe: Recipe = serde_json::from_value(args["recipe"].clone())?;
                recipe.validate(&BrokerSessionExecutor {
                    broker: self.clone(),
                    session: session.into(),
                })
            }
            "plugin.list" => match &self.plugins {
                Some(host) => host.list(),
                None => Ok(json!({"plugins":[],"available":false})),
            },
            "plugin.describe" => Ok(
                json!({"manifest":self.plugins.as_ref().ok_or_else(||Error::unavailable("Plugin host absent"))?.describe(arg_str(args,"name")?)?}),
            ),
            "plugin.doctor" => {
                let host = self
                    .plugins
                    .as_ref()
                    .ok_or_else(|| Error::unavailable("Plugin host absent"))?;
                let m = host.describe(arg_str(args, "name")?)?;
                host.validate_install(&m)?;
                Ok(
                    json!({"manifest_valid":true,"digest_valid":true,"host":host.list()?,"sandbox_execution":"not tested by this probe"}),
                )
            }
            "plugin.install" => {
                let manifest: Manifest = serde_json::from_value(args["manifest"].clone())?;
                self.install_manifest(manifest)?;
                self.event(self.core_event("registry_changed"));
                Ok(
                    json!({"installed":true,"persistence":"until broker shutdown; owner config is needed for subsequent starts"}),
                )
            }
            "plugin.remove" => {
                let host = self
                    .plugins
                    .as_ref()
                    .ok_or_else(|| Error::unavailable("Plugin host absent"))?;
                let manifest = host.remove(arg_str(args, "name")?)?;
                let mut registry = self
                    .registry
                    .write()
                    .map_err(|_| Error::new(ErrorCode::Internal, "Registry lock poisoned"))?;
                for command in &manifest.commands {
                    registry.remove_plugin_command(&command.name)?;
                }
                self.event(self.core_event("registry_changed"));
                Ok(json!({"removed":true}))
            }
            _ => Err(Error::new(ErrorCode::Unsupported, "Unknown core command")),
        }
    }
    /// Only owner-loaded configuration or the already-authorized install command calls this.
    pub fn install_manifest(&self, manifest: Manifest) -> Result<()> {
        let host = self
            .plugins
            .as_ref()
            .ok_or_else(|| Error::unavailable("Plugin host absent"))?;
        let mut registry = self
            .registry
            .write()
            .map_err(|_| Error::new(ErrorCode::Internal, "Registry lock poisoned"))?;
        let mut candidate = registry.clone();
        let identity =
            ProviderIdentity::external(SourceKind::Plugin, &manifest.name, &manifest.version)?;
        for command in &manifest.commands {
            candidate.register_with_metadata(
                command.clone(),
                semwright_registry::Metadata::for_provider(&identity),
            )?;
        }
        host.install(manifest)?;
        *registry = candidate;
        Ok(())
    }
    pub fn session_executor(self: &Arc<Self>, session: impl Into<String>) -> BrokerSessionExecutor {
        let session = session.into();
        // Keep an executor lease registered even between operations. A caller
        // holding an old executor must not be able to dispatch again after its
        // session has been revoked. New executor admission is the host's duty.
        let (guard, lease, lease_error) =
            match self.track_active_call(&session, &CancellationToken::new()) {
                Ok((guard, lease)) => (Some(guard), lease, None),
                Err(error) => {
                    // This constructor predates the bounded tracking contract
                    // and cannot return Result. Retain the precise error but
                    // fail closed rather than issuing an untracked executor.
                    let cancelled = CancellationToken::new();
                    cancelled.cancel();
                    (None, cancelled, Some(error))
                }
            };
        BrokerSessionExecutor {
            broker: self.clone(),
            session,
            lease,
            lease_error,
            _lease_guard: guard,
        }
    }

    pub async fn shutdown(&self) {
        self.runtime_stop.cancel();
        self.job_tasks.close();
        let _ = tokio::time::timeout(Duration::from_secs(5), self.job_tasks.wait()).await;
        self.provider_tasks.close();
        let _ = tokio::time::timeout(Duration::from_secs(3), self.provider_tasks.wait()).await;
        for backend in self.providers() {
            let _ = tokio::time::timeout(Duration::from_secs(5), backend.shutdown()).await;
        }
    }
}
fn event_time() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

/// Session-bound adapter into the existing Broker execution channel.
/// Controllers may describe and invoke capabilities through this value, but it
/// grants no authority: every operation still enters Broker policy/audit and the
/// selected provider with the original session identity.
pub struct BrokerSessionExecutor {
    broker: Arc<Broker>,
    session: String,
    lease: CancellationToken,
    lease_error: Option<Error>,
    _lease_guard: Option<ActiveCall>,
}
impl BrokerSessionExecutor {
    fn require_live_lease(&self) -> Result<()> {
        if let Some(error) = &self.lease_error {
            return Err(error.clone());
        }
        if self.lease.is_cancelled() {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Session executor was revoked; renew authorization through the host",
            ));
        }
        Ok(())
    }
}
#[async_trait]
impl Executor for BrokerSessionExecutor {
    fn describe(&self, command: &str) -> Result<CommandDescriptor> {
        self.require_live_lease()?;
        let descriptor = self.broker.describe(command)?;
        self.require_live_lease()?;
        Ok(descriptor)
    }
    async fn execute(
        &self,
        request: ExecuteRequest,
        cancellation: CancellationToken,
    ) -> Result<Value> {
        self.require_live_lease()?;
        // The caller keeps its original cancellation authority. Revocation
        // also cancels this separate child, including the narrow interval
        // between checking the executor lease and registering the new call.
        let call_cancellation = cancellation.child_token();
        let pending = self.broker.clone().execute(
            self.session.clone(),
            unique_id(),
            request,
            call_cancellation.clone(),
        );
        tokio::pin!(pending);
        let envelope = tokio::select! {
            biased;
            _ = self.lease.cancelled() => {
                call_cancellation.cancel();
                pending.await
            },
            result = &mut pending => result,
        };
        if let Some(error) = envelope.error {
            Err(error)
        } else {
            envelope.data.ok_or_else(|| {
                Error::new(ErrorCode::Internal, "Broker returned no recipe step output")
            })
        }
    }
}

#[cfg(test)]
mod windows_find_tests;

#[cfg(test)]
mod scoped_snapshot_tests {
    use super::*;

    #[test]
    fn windows_window_and_ui_references_are_supported() {
        let args = json!({"ref":"win:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"});
        assert!(validate_snapshot_scope("ui.snapshot", &args, "windows", Some("win")).is_ok());
        assert!(validate_snapshot_scope("ui.snapshot", &args, "windows", Some("ui")).is_ok());
        assert!(validate_snapshot_scope("ui.snapshot", &json!({}), "windows", None).is_ok());
    }

    #[test]
    fn unsupported_scopes_fail_closed() {
        let target = json!({"ref":"win:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"});
        for backend in ["macos", "atspi", "fake"] {
            assert_eq!(
                validate_snapshot_scope("ui.snapshot", &target, backend, Some("win"))
                    .unwrap_err()
                    .code,
                ErrorCode::Unsupported
            );
        }
        assert_eq!(
            validate_snapshot_scope("ui.snapshot", &target, "windows", Some("app"))
                .unwrap_err()
                .code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            validate_snapshot_scope("ui.snapshot", &target, "windows", None)
                .unwrap_err()
                .code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn windows_app_filter_is_not_silently_ignored() {
        let args = json!({"app":"notepad.exe"});
        assert_eq!(
            validate_snapshot_scope("ui.snapshot", &args, "windows", None)
                .unwrap_err()
                .code,
            ErrorCode::Unsupported
        );
        assert!(validate_snapshot_scope("ui.snapshot", &args, "atspi", None).is_ok());
        assert_eq!(
            validate_snapshot_scope(
                "ui.snapshot",
                &json!({"ref":"win:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","app":"notepad.exe"}),
                "windows",
                Some("win")
            )
            .unwrap_err()
            .code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn unrelated_commands_are_unchanged() {
        assert!(
            validate_snapshot_scope(
                "ui.inspect",
                &json!({"ref":"ui:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}),
                "atspi",
                Some("ui")
            )
            .is_ok()
        );
    }
}
