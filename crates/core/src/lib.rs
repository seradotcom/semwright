//! The single authorization and execution authority used by every frontend.
pub mod audit;
use async_trait::async_trait;
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
    pub event: Value,
}
struct Events {
    sequence: u64,
    history: VecDeque<Event>,
}
pub struct Broker {
    registry: StdRwLock<Registry>,
    policy: Policy,
    backends: BTreeMap<String, Arc<dyn Backend>>,
    references: StdMutex<RefStore>,
    audit: Arc<audit::Audit>,
    approver: Arc<dyn Approver>,
    plugins: Option<Arc<Host>>,
    execution_gate: RwLock<()>,
    features: RwLock<Option<(Instant, Vec<Feature>)>>,
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
        Ok(Arc::new(Self {
            registry: StdRwLock::new(Registry::builtin()?),
            policy,
            backends: map,
            references: StdMutex::new(RefStore::new(Duration::from_secs(60), 65536)),
            audit,
            approver,
            plugins,
            execution_gate: RwLock::new(()),
            features: RwLock::new(None),
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
    }
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.broadcast.subscribe()
    }
    pub fn replay(&self, after: u64) -> Result<Vec<Event>> {
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
            .filter(|event| event.sequence > after)
            .cloned()
            .collect())
    }
    fn event(&self, value: Value) {
        if let Ok(mut events) = self.events.lock() {
            events.sequence = events.sequence.saturating_add(1);
            let event = Event {
                sequence: events.sequence,
                event: value,
            };
            if events.history.len() == 256 {
                events.history.pop_front();
            }
            events.history.push_back(event.clone());
            let _ = self.broadcast.send(event);
        }
    }
    pub async fn probe(&self) -> Vec<Feature> {
        if let Some((when, features)) = &*self.features.read().await {
            if when.elapsed() < Duration::from_secs(5) {
                return features.clone();
            }
        }
        let mut jobs = tokio::task::JoinSet::new();
        for backend in self.backends.values() {
            let backend = backend.clone();
            jobs.spawn(async move {
                let name = backend.name();
                tokio::time::timeout(Duration::from_secs(3), backend.probe())
                    .await
                    .unwrap_or_else(|_| {
                        vec![Feature {
                            backend: name.into(),
                            capability: "probe".into(),
                            status: CapabilityStatus::Unavailable,
                            reason: "Backend probe exceeded three seconds".into(),
                            remediation: "Check the session service or application manually".into(),
                        }]
                    })
            });
        }
        let mut features = vec![];
        while let Some(result) = jobs.join_next().await {
            if let Ok(mut rows) = result {
                features.append(&mut rows);
            }
        }
        features.sort_by(|a, b| (&a.backend, &a.capability).cmp(&(&b.backend, &b.capability)));
        *self.features.write().await = Some((Instant::now(), features.clone()));
        features
    }
    async fn choose(
        &self,
        descriptor: &CommandDescriptor,
        request: &ExecuteRequest,
        reference: Option<&Reference>,
    ) -> Result<String> {
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
        if let Some(reference) = reference {
            if !is_input {
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
                let backend = self.backends.get(&reference.backend).ok_or_else(|| {
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
                .backends
                .get("fake")
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
                .backends
                .get(&name)
                .is_some_and(|backend| backend.supports(actual_command))
                && features
                    .iter()
                    .any(|feature| feature.backend == name && feature.usable())
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
    ) -> Option<&'a str> {
        reference
            .map(|r| r.target.app.as_str())
            .or_else(|| request.args.get("app").and_then(Value::as_str))
            .or_else(|| {
                request
                    .args
                    .pointer("/selector/app")
                    .and_then(Value::as_str)
            })
            .or_else(|| {
                if request.command.starts_with("blender.") {
                    Some("org.blender.Blender")
                } else if request.command.starts_with("browser.") {
                    Some("org.semwright.Chromium")
                } else {
                    None
                }
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
        let descriptor = self.describe(&request.command);
        let safe_name = descriptor
            .as_ref()
            .map(|d| d.name.clone())
            .unwrap_or_else(|_| "unregistered".into());
        let mut audit = match self.audit.begin(&safe_name, &id, &session) {
            Ok(scope) => scope,
            Err(error) => {
                return Envelope::finish(
                    id,
                    safe_name.into(),
                    "core".into(),
                    started.elapsed(),
                    request.dry_run,
                    Err(error),
                );
            }
        };
        self.event(json!({"kind":"command_start","command":safe_name,"request_id":id}));
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
        self.event(json!({"kind":"command_finish","command":safe_name,"request_id":id,"backend":selected,"ok":result.is_ok(),"error_code":result.as_ref().err().map(|e|e.code)}));
        let mut envelope = Envelope::finish(
            id,
            request.command,
            selected,
            started.elapsed(),
            request.dry_run,
            result,
        );
        envelope.execution.policy_decision = audit.policy_decision().into();
        envelope
    }
    async fn perform(
        self: &Arc<Self>,
        session: &str,
        request: &ExecuteRequest,
        descriptor: Result<CommandDescriptor>,
        cancellation: CancellationToken,
        selected: &mut String,
        audit: &mut audit::Scope,
    ) -> Result<Value> {
        let descriptor = descriptor?;
        self.registry
            .read()
            .map_err(|_| Error::new(ErrorCode::Internal, "Registry lock poisoned"))?
            .validate_input(&request.command, &request.args)?;
        if cancellation.is_cancelled() {
            return Err(Error::new(
                ErrorCode::Cancelled,
                "Cancelled before authorization",
            ));
        }
        let reference = self.resolve_reference(session, &request.args)?;
        let needs_confirmation = match self.policy.enforce(
            &descriptor,
            &request.args,
            self.application(request, reference.as_ref()),
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
        // Nested recipes re-enter this method for each step; never hold a gate across that recursion.
        if request.command == "recipe.run" {
            *selected = "core".into();
            audit.backend(selected);
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
            .choose(&descriptor, request, reference.as_ref())
            .await?;
        audit.backend(selected);
        // All broker I/O is blocked during an operator prompt. The model cannot inspect or type approval.
        let exclusive = descriptor.risk.mutates()
            || descriptor.risk == Risk::SecretAccess
            || needs_confirmation;
        let read_guard;
        let write_guard;
        if exclusive {
            write_guard = Some(
                tokio::select! {_ = cancellation.cancelled()=>return Err(Error::new(ErrorCode::Cancelled,"Cancelled while queued")),guard=self.execution_gate.write()=>guard},
            );
            read_guard = None;
        } else {
            read_guard = Some(
                tokio::select! {_ = cancellation.cancelled()=>return Err(Error::new(ErrorCode::Cancelled,"Cancelled while queued")),guard=self.execution_gate.read()=>guard},
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
        let reference = self.resolve_reference(session, &request.args)?;
        self.policy.enforce(
            &descriptor,
            &request.args,
            self.application(request, reference.as_ref()),
        )?;
        let mut args = request.args.clone();
        let is_input =
            request.command.starts_with("input.") || request.command.starts_with("pointer.");
        if let Some(reference) = &reference {
            let backend = self.backends.get(&reference.backend).ok_or_else(|| {
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
                self.backends
                    .get(selected.as_str())
                    .ok_or_else(|| Error::unavailable("Selected backend missing"))?
                    .execute(&context, &request.command, &args)
                    .await?
            };
            if request.command != "ui.find" {
                self.filter_apps(&mut output);
                self.materialize(session, selected, &mut output)?;
            }
            if serde_json::to_vec(&output)?.len() > MAX_FRAME - 8192 {
                return Err(Error::new(
                    ErrorCode::ResourceExhausted,
                    "Command output exceeds the broker frame budget; request a smaller snapshot",
                ));
            }
            self.registry
                .read()
                .map_err(|_| Error::new(ErrorCode::Internal, "Registry lock poisoned"))?
                .validate_output(&request.command, &output)?;
            Ok(output)
        };
        let result = tokio::select! {
            _=cancellation.cancelled()=>{context.cancellation.cancel();Err(Error::new(ErrorCode::Cancelled,"Command cancelled after dispatch; inspect state before another mutation").uncertain())},
            result=tokio::time::timeout(Duration::from_millis(descriptor.timeout_ms),action)=>match result{Ok(result)=>result,Err(_)=>{context.cancellation.cancel();Err(Error::new(ErrorCode::Timeout,"Command exceeded its action timeout; no retry was attempted").uncertain())}},
        };
        if descriptor.risk.mutates() {
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
            .backends
            .get(backend)
            .ok_or_else(|| Error::unavailable("UI backend missing"))?
            .execute(context, "ui.snapshot", &snapshot_args)
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
                json!({"project":"Semwright","version":env!("CARGO_PKG_VERSION"),"protocol":PROTOCOL_VERSION,"fake":self.fake,"environment":self.environment,"features":self.probe().await,"policy":{"profile":self.policy.config().profile,"granted":self.policy.capabilities(),"apps":self.policy.config().apps,"shell":false,"external_confirmation":"foreground operator only; unavailable in user service"},"uptime_seconds":self.started.elapsed().as_secs(),"verification":"Runtime capability probes are not a live desktop acceptance certificate","unimplemented":["EIS/libei input transport","PipeWire pixel-stream decoder","AT-SPI delta snapshots"]}),
            ),
            "capabilities.list" => Ok(
                json!({"granted":self.policy.capabilities(),"filesystem":self.policy.config().filesystem.iter().map(|r|json!({"name":r.name,"read":r.read,"write":r.write})).collect::<Vec<_>>(),"backends":self.probe().await,"fake":self.fake}),
            ),
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
                self.event(json!({"kind":"registry_changed"}));
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
                self.event(json!({"kind":"registry_changed"}));
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
        for command in &manifest.commands {
            candidate.register(command.clone())?;
        }
        host.install(manifest)?;
        *registry = candidate;
        Ok(())
    }
    pub async fn shutdown(&self) {
        for backend in self.backends.values() {
            let _ = tokio::time::timeout(Duration::from_secs(5), backend.shutdown()).await;
        }
    }
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
