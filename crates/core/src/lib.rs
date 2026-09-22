//! The single authorization and execution authority used by every frontend.
pub mod audit;
mod catalog;
mod jobs;
mod providers;
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
pub struct Broker {
    registry: StdRwLock<ProviderCatalog>,
    policy: Policy,
    references: StdMutex<RefStore>,
    audit: Arc<audit::Audit>,
    approver: Arc<dyn Approver>,
    plugins: Option<Arc<Host>>,
    execution_gate: RwLock<()>,
    features: RwLock<Option<(u64, Instant, Vec<Feature>)>>,
    runtime_stop: CancellationToken,
    provider_tasks: tokio_util::task::TaskTracker,
    job_tasks: tokio_util::task::TaskTracker,
    jobs: StdMutex<jobs::JobStore>,
    events: StdMutex<Events>,
    broadcast: broadcast::Sender<Event>,
    environment: Value,
    fake: bool,
    started: Instant,
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
                .any(|name| name != "fake" && name != "filesystem")
        {
            return Err(Error::invalid(
                "Fake mode cannot instantiate live desktop/application backends",
            ));
        }
        let (sender, _) = broadcast::channel(256);
        let runtime_stop = CancellationToken::new();
        let catalog = ProviderCatalog::bootstrap(Registry::builtin()?, map, &runtime_stop);
        Ok(Arc::new(Self {
            registry: StdRwLock::new(catalog),
            policy,
            references: StdMutex::new(RefStore::new(Duration::from_secs(60), 65536)),
            audit,
            approver,
            plugins,
            execution_gate: RwLock::new(()),
            features: RwLock::new(None),
            runtime_stop,
            provider_tasks: tokio_util::task::TaskTracker::new(),
            job_tasks: tokio_util::task::TaskTracker::new(),
            jobs: StdMutex::new(jobs::JobStore::default()),
            events: StdMutex::new(Events {
                sequence: 0,
                history: VecDeque::new(),
            }),
            broadcast: sender,
            environment,
            fake,
            started: Instant::now(),
        }))
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
        if let Ok(mut store) = self.references.lock() {
            store.revoke_session(session);
        }
        if let Ok(mut jobs) = self.jobs.lock() {
            jobs.revoke_session(session);
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
            if !backend.supports(&request.command) {
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
    fn resolve_reference(&self, session: &str, args: &Value) -> Result<Option<Reference>> {
        args.get("ref")
            .map(|value| {
                let id = value
                    .as_str()
                    .ok_or_else(|| Error::invalid("Reference must be a string"))?;
                self.references
                    .lock()
                    .map_err(|_| Error::new(ErrorCode::Internal, "Reference store lock poisoned"))?
                    .get(id, session)
            })
            .transpose()
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
    fn filter_apps(&self, value: &mut Value) {
        let apps = &self.policy.config().apps;
        if apps.is_empty() {
            return;
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
    }
    pub fn execute(
        self: Arc<Self>,
        session: String,
        id: String,
        request: ExecuteRequest,
        cancellation: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Envelope> + Send>> {
        Box::pin(async move { self.execute_inner(session, id, request, cancellation).await })
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
        if cancellation.is_cancelled() {
            return Err(Error::new(
                ErrorCode::Cancelled,
                "Cancelled before authorization",
            ));
        }
        let reference = if capability.metadata.source == SourceKind::Builtin {
            self.resolve_reference(session, &request.args)?
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
            let bridge = RecipeBridge {
                broker: self.clone(),
                session: session.into(),
            };
            return recipe
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
                .await;
        }
        *selected = self
            .choose(
                descriptor,
                request,
                reference.as_ref(),
                dynamic_provider.as_ref(),
            )
            .await?;
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
        let reference = if capability.metadata.source == SourceKind::Builtin {
            self.resolve_reference(session, &request.args)?
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
        if cancellation.is_cancelled() {
            return Err(Error::new(
                ErrorCode::Cancelled,
                "Cancelled before dispatch",
            ));
        }
        let context = Context {
            session: session.into(),
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
                self.core(
                    session,
                    &request.command,
                    &args,
                    context.cancellation.clone(),
                )
                .await?
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
            if request.command != "ui.find" && capability.metadata.source == SourceKind::Builtin {
                self.filter_apps(&mut output);
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
        let mut snapshot_args = json!({"max_nodes":args["max_nodes"].as_u64().unwrap_or(500),"max_depth":args["max_depth"].as_u64().unwrap_or(8)});
        if let Some(app) = &selector.app {
            snapshot_args["app"] = json!(app);
        }
        let mut output = self
            .provider(backend)
            .ok_or_else(|| Error::unavailable("UI backend missing"))?
            .execute(context, &self.describe("ui.snapshot")?, &snapshot_args)
            .await?;
        self.filter_apps(&mut output);
        self.materialize(session, backend, &mut output)?;
        let nodes: Vec<UiNode> = serde_json::from_value(output["nodes"].clone())?;
        if let Some(old) = selector.ancestor.clone() {
            let store = self
                .references
                .lock()
                .map_err(|_| Error::new(ErrorCode::Internal, "Reference lock poisoned"))?;
            let wanted = store.get(&old, session)?;
            let current = nodes.iter().find(|node| {
                store.get(&node.reference, session).is_ok_and(|r| {
                    r.backend == wanted.backend
                        && r.target.identity == wanted.target.identity
                        && r.target.revision == wanted.target.revision
                        && r.target.fingerprint == wanted.target.fingerprint
                })
            });
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
        session: &str,
        command: &str,
        args: &Value,
        _cancellation: CancellationToken,
    ) -> Result<Value> {
        match command {
            "doctor" => Ok(
                json!({"project":"Semwright","version":env!("CARGO_PKG_VERSION"),"protocol":PROTOCOL_VERSION,"fake":self.fake,"environment":self.environment,"features":self.probe().await,"providers":self.provider_status()?,"policy":{"profile":self.policy.config().profile,"granted":self.policy.capabilities(),"apps":self.policy.config().apps,"shell":false,"external_confirmation":"foreground operator only; unavailable in user service"},"uptime_seconds":self.started.elapsed().as_secs(),"verification":"Runtime capability probes are not a live desktop acceptance certificate","unimplemented":["EIS/libei input transport","PipeWire pixel-stream decoder","AT-SPI delta snapshots"]}),
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
            "recipe.list" => Ok(
                json!({"recipes":["fake-export","fake-edit","blender-inspect","workspace-write"],"source":"Recipe files ship with the repository; clients load and submit their declarative contents"}),
            ),
            "recipe.validate" => {
                let recipe: Recipe = serde_json::from_value(args["recipe"].clone())?;
                recipe.validate(&RecipeBridge {
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

struct RecipeBridge {
    broker: Arc<Broker>,
    session: String,
}
#[async_trait]
impl Executor for RecipeBridge {
    fn describe(&self, command: &str) -> Result<CommandDescriptor> {
        self.broker.describe(command)
    }
    async fn execute(
        &self,
        request: ExecuteRequest,
        cancellation: CancellationToken,
    ) -> Result<Value> {
        let envelope = self
            .broker
            .clone()
            .execute(self.session.clone(), unique_id(), request, cancellation)
            .await;
        if let Some(error) = envelope.error {
            Err(error)
        } else {
            envelope.data.ok_or_else(|| {
                Error::new(ErrorCode::Internal, "Broker returned no recipe step output")
            })
        }
    }
}
