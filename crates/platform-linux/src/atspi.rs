//! AT-SPI2 over its dedicated accessibility bus. Public API types never leak zbus.
//! Invalidates all UI generations conservatively on object/window events or bus loss.
use async_trait::async_trait;
use futures_util::StreamExt;
use semwright_backend_api::{Backend, Context, feature};
use semwright_types::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{
    Arc, Mutex as StdMutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::Duration;
use tokio::sync::Mutex as AsyncMutex;
use zbus::{Connection, Proxy, zvariant::OwnedObjectPath};

type Object = (String, OwnedObjectPath);
const ACCESSIBLE: &str = "org.a11y.atspi.Accessible";
const ROOT: &str = "/org/a11y/atspi/accessible/root";
#[derive(Default)]
struct ObjectGenerations {
    next: AtomicU64,
    values: StdMutex<BTreeMap<String, u64>>,
}
impl ObjectGenerations {
    fn allocate(&self) -> u64 {
        self.next.fetch_add(1, Ordering::SeqCst).saturating_add(1)
    }
    fn get_or_assign(&self, identity: &str) -> Result<u64> {
        let mut values = self
            .values
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "AT-SPI generation state poisoned"))?;
        Ok(*values
            .entry(identity.to_owned())
            .or_insert_with(|| self.allocate()))
    }
    fn current(&self, identity: &str) -> Result<Option<u64>> {
        let values = self
            .values
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "AT-SPI generation state poisoned"))?;
        Ok(values.get(identity).copied())
    }
    fn invalidate(&self, identity: &str) -> Result<u64> {
        let generation = self.allocate();
        let mut values = self
            .values
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "AT-SPI generation state poisoned"))?;
        values.insert(identity.to_owned(), generation);
        Ok(generation)
    }
    fn clear(&self) {
        if let Ok(mut values) = self.values.lock() {
            values.clear();
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SnapshotKey {
    app: Option<String>,
    max_nodes: usize,
    max_depth: usize,
    actionable: bool,
}
#[derive(Clone)]
struct CachedSnapshot {
    revision: u64,
    nodes: BTreeMap<String, Value>,
}
pub struct Atspi {
    connection: Arc<AsyncMutex<Option<Connection>>>,
    connect_guard: AsyncMutex<()>,
    connection_generation: Arc<AtomicU64>,
    revision: Arc<AtomicU64>,
    barrier_revision: Arc<AtomicU64>,
    events_live: Arc<AtomicBool>,
    object_generations: Arc<ObjectGenerations>,
    snapshots: StdMutex<BTreeMap<SnapshotKey, CachedSnapshot>>,
}
impl Default for Atspi {
    fn default() -> Self {
        Self {
            connection: Arc::new(AsyncMutex::new(None)),
            connect_guard: AsyncMutex::new(()),
            connection_generation: Arc::new(AtomicU64::new(0)),
            revision: Arc::new(AtomicU64::new(1)),
            barrier_revision: Arc::new(AtomicU64::new(1)),
            events_live: Arc::new(AtomicBool::new(false)),
            object_generations: Arc::new(ObjectGenerations::default()),
            snapshots: StdMutex::new(BTreeMap::new()),
        }
    }
}
fn dbus<T>(r: std::result::Result<T, impl std::fmt::Debug>) -> Result<T> {
    r.map_err(|_| {
        Error::new(
            ErrorCode::BackendFailed,
            "Accessibility bus operation failed; application may have disappeared",
        )
    })
}
async fn bounded<T>(f: impl std::future::Future<Output = zbus::Result<T>>) -> Result<T> {
    tokio::time::timeout(Duration::from_millis(1200), f)
        .await
        .map_err(|_| Error::new(ErrorCode::Timeout, "Accessibility object did not respond"))?
        .map_err(|_| {
            Error::new(
                ErrorCode::BackendFailed,
                "Accessibility object/interface unavailable",
            )
        })
}
pub fn normalize_role(role: &str) -> String {
    match role {
        "push button" => "button".into(),
        "text" => "text".into(),
        "password text" => "password-entry".into(),
        _ => role.to_lowercase().replace(' ', "-"),
    }
}
pub fn decode_states(bits: &[u32]) -> Vec<&'static str> {
    let states = [
        (1, "active"),
        (3, "busy"),
        (4, "checked"),
        (5, "collapsed"),
        (6, "defunct"),
        (7, "editable"),
        (8, "enabled"),
        (9, "expandable"),
        (10, "expanded"),
        (11, "focusable"),
        (12, "focused"),
        (16, "modal"),
        (20, "pressed"),
        (22, "selectable"),
        (23, "selected"),
        (24, "sensitive"),
        (25, "showing"),
        (27, "stale"),
        (30, "visible"),
        (41, "checkable"),
        (43, "read-only"),
    ];
    states
        .into_iter()
        .filter(|(bit, _)| {
            bits.get(bit / 32)
                .is_some_and(|word| word & (1u32 << (bit % 32)) != 0)
        })
        .map(|(_, name)| name)
        .collect()
}
fn object_id(o: &Object) -> String {
    format!("{}|{}", o.0, o.1)
}
fn stable_node_id(identity: &str) -> String {
    format!("ui-node:{:x}", Sha256::digest(identity.as_bytes()))
}
fn delta_from_cache(
    previous: &CachedSnapshot,
    since_revision: u64,
    current: &BTreeMap<String, Value>,
) -> Option<(Vec<Value>, Vec<String>)> {
    if previous.revision != since_revision {
        return None;
    }
    let changed = current
        .iter()
        .filter(|(identity, node)| previous.nodes.get(*identity) != Some(*node))
        .map(|(_, node)| node.clone())
        .collect();
    let removed = previous
        .nodes
        .keys()
        .filter(|identity| !current.contains_key(*identity))
        .map(|identity| stable_node_id(identity))
        .collect();
    Some((changed, removed))
}
fn object_from_id(id: &str) -> Result<Object> {
    let (bus, path) = id
        .split_once('|')
        .ok_or_else(|| Error::invalid("Invalid accessibility identity"))?;
    if !bus.starts_with(':') {
        return Err(Error::invalid(
            "Accessibility references must retain a unique bus owner",
        ));
    }
    Ok((
        bus.into(),
        OwnedObjectPath::try_from(path.to_owned())
            .map_err(|_| Error::invalid("Invalid accessible object path"))?,
    ))
}
impl Atspi {
    async fn connect(&self) -> Result<Connection> {
        if self.events_live.load(Ordering::SeqCst)
            && let Some(connection) = self.connection.lock().await.clone()
        {
            return Ok(connection);
        }
        let _guard = self.connect_guard.lock().await;
        if self.events_live.load(Ordering::SeqCst)
            && let Some(connection) = self.connection.lock().await.clone()
        {
            return Ok(connection);
        }
        let session = dbus(Connection::session().await)?;
        let bus =
            dbus(Proxy::new(&session, "org.a11y.Bus", "/org/a11y/bus", "org.a11y.Bus").await)?;
        let address: String = bounded(bus.call("GetAddress", &())).await?;
        // Linux host detail: only the explicitly returned local accessibility bus; never TCP.
        if !address.starts_with("unix:") {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "Accessibility bus must be local Unix transport",
            ));
        }
        let connection = dbus(
            dbus(zbus::connection::Builder::address(address.as_str()))?
                .build()
                .await,
        )?;
        let generation = self
            .connection_generation
            .fetch_add(1, Ordering::SeqCst)
            .saturating_add(1);
        *self.connection.lock().await = Some(connection.clone());
        if let Err(error) = self.start_events(&connection, generation).await {
            *self.connection.lock().await = None;
            return Err(error);
        }
        Ok(connection)
    }
    fn invalidate_all(
        revision: &AtomicU64,
        barrier_revision: &AtomicU64,
        generations: &ObjectGenerations,
    ) -> u64 {
        let current = revision.fetch_add(1, Ordering::SeqCst).saturating_add(1);
        barrier_revision.store(current, Ordering::SeqCst);
        generations.clear();
        current
    }
    async fn start_events(&self, connection: &Connection, generation: u64) -> Result<()> {
        let registry = dbus(
            Proxy::new(
                connection,
                "org.a11y.atspi.Registry",
                "/org/a11y/atspi/registry",
                "org.a11y.atspi.Registry",
            )
            .await,
        )?;
        let mut streams = vec![];
        for interface in [
            "org.a11y.atspi.Event.Object",
            "org.a11y.atspi.Event.Window",
            "org.freedesktop.DBus",
        ] {
            let rule = dbus(
                zbus::MatchRule::builder()
                    .msg_type(zbus::message::Type::Signal)
                    .interface(interface),
            )?
            .build();
            streams.push(dbus(
                zbus::MessageStream::for_match_rule(rule, connection, Some(1024)).await,
            )?);
        }
        for category in ["object:", "window:"] {
            let _: () =
                bounded(registry.call("RegisterEvent", &(category, Vec::<String>::new(), "")))
                    .await?;
        }
        self.events_live.store(true, Ordering::SeqCst);
        for mut stream in streams {
            let revision = self.revision.clone();
            let barrier_revision = self.barrier_revision.clone();
            let live = self.events_live.clone();
            let generations = self.object_generations.clone();
            let next_connection_generation = self.connection_generation.clone();
            let connection_slot = self.connection.clone();
            tokio::spawn(async move {
                while let Some(message) = stream.next().await {
                    let Ok(message) = message else {
                        break;
                    };
                    let header = message.header();
                    let interface = header.interface().map(|value| value.as_str().to_owned());
                    let member = header.member().map(|value| value.as_str().to_owned());
                    let current = revision.fetch_add(1, Ordering::SeqCst).saturating_add(1);
                    let structural = matches!(
                        interface.as_deref(),
                        Some("org.a11y.atspi.Event.Window" | "org.freedesktop.DBus")
                    ) || member
                        .as_deref()
                        .is_some_and(|name| name.contains("ChildrenChanged"));
                    if structural {
                        barrier_revision.store(current, Ordering::SeqCst);
                        generations.clear();
                        continue;
                    }
                    if let (Some(sender), Some(path)) = (header.sender(), header.path()) {
                        let identity = format!("{}|{}", sender.as_str(), path.as_str());
                        let _ = generations.invalidate(&identity);
                    } else {
                        barrier_revision.store(current, Ordering::SeqCst);
                        generations.clear();
                    }
                }
                if next_connection_generation.load(Ordering::SeqCst) == generation
                    && live.swap(false, Ordering::SeqCst)
                {
                    Self::invalidate_all(&revision, &barrier_revision, &generations);
                    *connection_slot.lock().await = None;
                }
            });
        }
        Ok(())
    }
    async fn proxy<'a>(
        &self,
        c: &'a Connection,
        o: &'a Object,
        interface: &'a str,
    ) -> Result<Proxy<'a>> {
        let builder = dbus(zbus::proxy::Builder::<Proxy<'a>>::new(c).destination(o.0.as_str()))?;
        let builder = dbus(builder.path(o.1.as_str()))?;
        let builder = dbus(builder.interface(interface))?;
        dbus(
            builder
                .cache_properties(zbus::proxy::CacheProperties::No)
                .build()
                .await,
        )
    }
    async fn apps(&self, c: &Connection) -> Result<Vec<Object>> {
        let root = dbus(Proxy::new(c, "org.a11y.atspi.Registry", ROOT, ACCESSIBLE).await)?;
        let objects: Vec<Object> = bounded(root.call("GetChildren", &())).await?;
        if objects.len() > 512 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Accessibility app count exceeds budget",
            ));
        }
        Ok(objects)
    }
    async fn identity(&self, c: &Connection, o: &Object) -> Result<(String, String, String)> {
        let p = self.proxy(c, o, ACCESSIBLE).await?;
        let role: String = bounded(p.call("GetRoleName", &())).await?;
        let name: String = bounded(p.get_property("Name")).await?;
        let role = normalize_role(&role);
        let fingerprint = format!("{role}:{name}");
        Ok((role, name, fingerprint))
    }
    async fn app_name(&self, c: &Connection, o: &Object) -> Result<String> {
        let p = self.proxy(c, o, ACCESSIBLE).await?;
        let attrs: std::collections::HashMap<String, String> =
            bounded(p.call("GetAttributes", &()))
                .await
                .unwrap_or_default();
        if let Some(id) = attrs
            .get("desktop-entry")
            .or_else(|| attrs.get("application-id"))
        {
            return Ok(id.clone());
        }
        bounded(p.get_property::<String>("Name")).await
    }
    async fn actions(&self, c: &Connection, o: &Object) -> Vec<String> {
        let Ok(proxy) = self.proxy(c, o, "org.a11y.atspi.Action").await else {
            return vec![];
        };
        let Ok(count) = bounded(proxy.get_property::<i32>("NActions")).await else {
            return vec![];
        };
        let mut names = vec![];
        // GetActions returns localized labels. GetName is the machine action name.
        for index in 0..count.clamp(0, 32) {
            if let Ok(name) = bounded(proxy.call::<_, _, String>("GetName", &(index,))).await {
                names.push(name);
            }
        }
        names
    }
    async fn snapshot(&self, ctx: &Context, args: &Value) -> Result<Value> {
        let c = self.connect().await?;
        let revision = self.revision.load(Ordering::SeqCst);
        let since_revision = args.get("since_revision").and_then(Value::as_u64);
        if since_revision.is_some_and(|since| since > revision) {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Snapshot base revision is newer than the accessibility state",
            ));
        }
        let budget = args["max_nodes"].as_u64().unwrap_or(200).min(2000) as usize;
        let max_depth = args["max_depth"].as_u64().unwrap_or(5).min(32) as usize;
        let actionable = args["actionable"].as_bool().unwrap_or(false);
        let key = SnapshotKey {
            app: args["app"].as_str().map(str::to_owned),
            max_nodes: budget,
            max_depth,
            actionable,
        };
        let mut queue = VecDeque::new();
        let mut known: BTreeMap<String, NativeTarget> = BTreeMap::new();
        for app in self.apps(&c).await? {
            let app_name = match self.app_name(&c, &app).await {
                Ok(name) => name,
                Err(_) => continue,
            };
            if args["app"]
                .as_str()
                .is_some_and(|wanted| wanted != app_name)
            {
                continue;
            }
            queue.push_back((app, 0usize, app_name, None::<String>));
        }
        let mut seen = BTreeSet::new();
        let mut nodes = vec![];
        let mut nodes_by_identity = BTreeMap::new();
        let mut partial = false;
        let mut visited = 0;
        while let Some((object, depth, app, parent)) = queue.pop_front() {
            ctx.check_cancelled()?;
            if nodes.len() >= budget || visited >= budget.saturating_mul(4) {
                partial = true;
                break;
            }
            visited += 1;
            let identity = object_id(&object);
            if !seen.insert(identity.clone()) {
                partial = true;
                continue;
            }
            let (role, mut name, fingerprint) = match self.identity(&c, &object).await {
                Ok(info) => info,
                Err(_) => {
                    partial = true;
                    continue;
                }
            };
            let proxy = self.proxy(&c, &object, ACCESSIBLE).await?;
            let state_bits: Vec<u32> = bounded(proxy.call("GetState", &()))
                .await
                .unwrap_or_default();
            let states = decode_states(&state_bits);
            if states.contains(&"defunct") || states.contains(&"stale") {
                partial = true;
                continue;
            }
            let actions = self.actions(&c, &object).await;
            let target = NativeTarget {
                kind: "ui".into(),
                identity: identity.clone(),
                revision: self.object_generations.get_or_assign(&identity)?,
                fingerprint,
                app: app.clone(),
            };
            known.insert(identity.clone(), target.clone());
            let children: Vec<Object> = bounded(proxy.call("GetChildren", &()))
                .await
                .unwrap_or_else(|_| {
                    partial = true;
                    vec![]
                });
            let count = children.len();
            if count > 2000 {
                partial = true;
            }
            if depth < max_depth {
                for child in children.into_iter().take(2000) {
                    queue.push_back((child, depth + 1, app.clone(), Some(identity.clone())));
                }
            } else if count > 0 {
                partial = true;
            }
            if actionable && actions.is_empty() && !states.contains(&"editable") {
                continue;
            }
            let mut description: String = bounded(proxy.get_property("Description"))
                .await
                .unwrap_or_default();
            if role == "password-entry" {
                name = "[protected control]".into();
                description.clear();
            }
            name = name.chars().take(1024).collect();
            description = description.chars().take(1024).collect();
            let bounds = match self.proxy(&c, &object, "org.a11y.atspi.Component").await {
                Ok(component) => {
                    bounded(component.call::<_, _, (i32, i32, i32, i32)>("GetExtents", &(0u32,)))
                        .await
                        .ok()
                        .map(|(x, y, width, height)| {
                            json!({"x":x,"y":y,"width":width,"height":height,
                        "coordinate_space":"atspi_screen_reported"})
                        })
                }
                Err(_) => None,
            };
            let row = json!({
                "node_id": stable_node_id(&identity),
                "ref": target_marker(target),
                "role": role,
                "name": name,
                "description": description,
                "states": states,
                "actions": actions,
                "app": app,
                "parent_ref": parent
                    .and_then(|parent| known.get(&parent).cloned())
                    .map(target_marker),
                "bounds": bounds,
                "children_count": count
            });
            nodes_by_identity.insert(identity, row.clone());
            nodes.push(row);
        }
        let ending = self.revision.load(Ordering::SeqCst);
        if ending != revision {
            partial = true;
        }
        let events_live = self.events_live.load(Ordering::SeqCst);
        let complete = !partial && ending == revision && events_live;
        let barrier_revision = self.barrier_revision.load(Ordering::SeqCst);
        let mut mode = "full";
        let mut resync_required = since_revision.is_some();
        let mut removed_node_ids = Vec::new();
        let mut returned_nodes = nodes;
        if let Some(since) = since_revision
            && complete
            && since >= barrier_revision
        {
            let snapshots = self
                .snapshots
                .lock()
                .map_err(|_| Error::new(ErrorCode::Internal, "AT-SPI snapshot cache poisoned"))?;
            if let Some(previous) = snapshots.get(&key)
                && let Some((changed, removed)) =
                    delta_from_cache(previous, since, &nodes_by_identity)
            {
                returned_nodes = changed;
                removed_node_ids = removed;
                mode = "delta";
                resync_required = false;
            }
        }
        if complete {
            let mut snapshots = self
                .snapshots
                .lock()
                .map_err(|_| Error::new(ErrorCode::Internal, "AT-SPI snapshot cache poisoned"))?;
            snapshots.insert(
                key,
                CachedSnapshot {
                    revision,
                    nodes: nodes_by_identity,
                },
            );
            while snapshots.len() > 16 {
                let Some(oldest) = snapshots.keys().next().cloned() else {
                    break;
                };
                snapshots.remove(&oldest);
            }
        }
        Ok(json!({
            "nodes": returned_nodes,
            "revision": revision,
            "partial": partial,
            "mode": mode,
            "base_revision": since_revision,
            "removed_node_ids": removed_node_ids,
            "resync_required": resync_required,
            "changed_during_snapshot": ending != revision,
            "semantic_coverage": if partial {"partial"} else {"reported_tree"},
            "event_invalidation": events_live,
            "visited": visited
        }))
    }
}
#[async_trait]
impl Backend for Atspi {
    fn name(&self) -> &'static str {
        "atspi"
    }
    fn supports(&self, c: &str) -> bool {
        matches!(
            c,
            "app.list"
                | "ui.snapshot"
                | "ui.invoke"
                | "ui.set_text"
                | "ui.read_text"
                | "ui.get_value"
                | "ui.set_value"
                | "ui.toggle"
                | "ui.select"
                | "ui.expand"
        )
    }
    fn operation_feature(&self, command: &str) -> Option<String> {
        self.supports(command).then(|| "ui.observe".to_owned())
    }
    async fn probe(&self) -> Vec<Feature> {
        let ready = tokio::time::timeout(Duration::from_secs(3), self.connect())
            .await
            .is_ok_and(|r| r.is_ok());
        vec![feature(
            self.name(),
            "ui.observe",
            ready,
            "AT-SPI bus, registry and object-event subscription",
            "Enable accessibility and run within the application user's D-Bus session",
        )]
    }
    async fn execute(&self, ctx: &Context, command: &str, args: &Value) -> Result<Value> {
        ctx.check_cancelled()?;
        if command == "ui.snapshot" {
            return self.snapshot(ctx, args).await;
        }
        let c = self.connect().await?;
        if command == "app.list" {
            let mut apps = vec![];
            for o in self.apps(&c).await? {
                if let Ok((_, name, fingerprint)) = self.identity(&c, &o).await {
                    let app = self.app_name(&c, &o).await.unwrap_or_else(|_| name.clone());
                    let identity = object_id(&o);
                    apps.push(json!({"ref":target_marker(NativeTarget{kind:"app".into(),identity:identity.clone(),revision:self.object_generations.get_or_assign(&identity)?,fingerprint,app:app.clone()}),"name":name,"app":app}));
                }
            }
            return Ok(json!({"apps":apps}));
        }
        let target = native_target(args)?;
        self.validate(&target).await?;
        let object = object_from_id(&target.identity)?;
        let (role, _, _) = self.identity(&c, &object).await?;
        let result = match command {
            "ui.read_text" => {
                if role == "password-entry" {
                    return Err(Error::new(
                        ErrorCode::PolicyDenied,
                        "Reading protected/password fields is never exposed",
                    ));
                }
                let p = self.proxy(&c, &object, "org.a11y.atspi.Text").await?;
                let count: i32 = bounded(p.get_property("CharacterCount")).await?;
                let max = args["max_chars"].as_u64().unwrap_or(4096).min(65536) as i32;
                let text: String = bounded(p.call("GetText", &(0i32, count.clamp(0, max)))).await?;
                json!({"text":text,"truncated":count>max})
            }
            "ui.set_text" => {
                let p = self
                    .proxy(&c, &object, "org.a11y.atspi.EditableText")
                    .await?;
                ctx.check_cancelled()?;
                let success: bool = bounded(p.call("SetTextContents", &(arg_str(args, "text")?,)))
                    .await
                    .map_err(Error::uncertain)?;
                if !success {
                    return Err(Error::new(
                        ErrorCode::BackendFailed,
                        "Application rejected editable text",
                    )
                    .uncertain());
                }
                json!({"changed":true})
            }
            "ui.get_value" | "ui.set_value" => {
                let p = self.proxy(&c, &object, "org.a11y.atspi.Value").await?;
                let minimum: f64 = bounded(p.get_property("MinimumValue")).await?;
                let maximum: f64 = bounded(p.get_property("MaximumValue")).await?;
                if command == "ui.set_value" {
                    let value = args["value"]
                        .as_f64()
                        .ok_or_else(|| Error::invalid("value required"))?;
                    if !value.is_finite() || value < minimum || value > maximum {
                        return Err(Error::invalid("Value is outside the accessible range"));
                    }
                    ctx.check_cancelled()?;
                    dbus(p.set_property("CurrentValue", value).await).map_err(Error::uncertain)?;
                }
                let value: f64 = bounded(p.get_property("CurrentValue")).await?;
                json!({"value":value,"minimum":minimum,"maximum":maximum})
            }
            "ui.select" => {
                let p = self.proxy(&c, &object, "org.a11y.atspi.Selection").await?;
                let index = args["index"]
                    .as_i64()
                    .ok_or_else(|| Error::invalid("index required"))?
                    as i32;
                let success: bool = bounded(p.call("SelectChild", &(index,)))
                    .await
                    .map_err(Error::uncertain)?;
                if !success {
                    return Err(
                        Error::new(ErrorCode::BackendFailed, "Selection rejected").uncertain()
                    );
                }
                json!({"selected":true})
            }
            "ui.invoke" | "ui.toggle" | "ui.expand" => {
                let names = self.actions(&c, &object).await;
                let requested = args["action"].as_str().or(match command {
                    "ui.toggle" => Some("toggle"),
                    "ui.expand" => Some("expand"),
                    _ => None,
                });
                let index = if let Some(action) = requested {
                    names.iter().position(|n| n == action)
                } else if names.len() == 1 {
                    Some(0)
                } else {
                    None
                };
                let index = index.ok_or_else(|| {
                    Error::new(
                        if names.len() > 1 && requested.is_none() {
                            ErrorCode::AmbiguousTarget
                        } else {
                            ErrorCode::Unsupported
                        },
                        "Choose an exact advertised action; no coordinate fallback is attempted",
                    )
                })? as i32;
                let p = self.proxy(&c, &object, "org.a11y.atspi.Action").await?;
                self.validate(&target).await?;
                ctx.check_cancelled()?;
                let accepted: bool = bounded(p.call("DoAction", &(index,)))
                    .await
                    .map_err(Error::uncertain)?;
                if !accepted {
                    return Err(Error::new(
                        ErrorCode::BackendFailed,
                        "Application rejected the action",
                    )
                    .uncertain());
                }
                json!({"invoked":true})
            }
            _ => {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "Unknown semantic operation",
                ));
            }
        };
        if !matches!(command, "ui.read_text" | "ui.get_value") {
            self.object_generations.invalidate(&target.identity)?;
            self.revision.fetch_add(1, Ordering::SeqCst);
        }
        Ok(result)
    }
    async fn validate(&self, target: &NativeTarget) -> Result<()> {
        let c = self.connect().await?;
        let current_generation = self
            .object_generations
            .current(&target.identity)?
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::StaleReference,
                    "Accessibility generation was reset; take a fresh snapshot",
                )
            })?;
        if !self.events_live.load(Ordering::SeqCst) || target.revision != current_generation {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Accessibility object changed or event stream was lost; take a fresh snapshot",
            ));
        }
        let object = object_from_id(&target.identity)?;
        let (_, _, fingerprint) = self.identity(&c, &object).await.map_err(|_| {
            Error::new(
                ErrorCode::StaleReference,
                "Accessibility object disappeared",
            )
        })?;
        if fingerprint != target.fingerprint {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Accessibility object identity changed",
            ));
        }
        let p = self.proxy(&c, &object, ACCESSIBLE).await?;
        let bits: Vec<u32> = bounded(p.call("GetState", &())).await?;
        if decode_states(&bits)
            .iter()
            .any(|s| matches!(*s, "defunct" | "stale"))
        {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Accessible object is defunct/stale",
            ));
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn state_bits_cross_words() {
        assert_eq!(
            decode_states(&[1 << 8, 1 << (43 - 32)]),
            vec!["enabled", "read-only"]
        );
    }
    #[test]
    fn password_role_is_distinct() {
        assert_eq!(normalize_role("password text"), "password-entry");
    }
    #[test]
    fn machine_button_role() {
        assert_eq!(normalize_role("push button"), "button");
    }
    #[test]
    fn unique_owner_required() {
        assert!(object_from_id("org.app|/object").is_err());
        assert!(object_from_id(":1.5|/object").is_ok());
    }
    #[test]
    fn object_generations_are_targeted_and_reset_conservatively() {
        let generations = ObjectGenerations::default();
        let first = generations.get_or_assign(":1.5|/first").unwrap();
        let second = generations.get_or_assign(":1.5|/second").unwrap();
        assert_eq!(generations.get_or_assign(":1.5|/first").unwrap(), first);
        let changed = generations.invalidate(":1.5|/first").unwrap();
        assert_ne!(changed, first);
        assert_eq!(generations.current(":1.5|/second").unwrap(), Some(second));
        generations.clear();
        assert_eq!(generations.current(":1.5|/first").unwrap(), None);
        assert_eq!(generations.current(":1.5|/second").unwrap(), None);
        assert_ne!(generations.get_or_assign(":1.5|/first").unwrap(), changed);
    }
    #[test]
    fn delta_cache_reports_changed_and_removed_semantic_ids() {
        let previous = CachedSnapshot {
            revision: 7,
            nodes: BTreeMap::from([
                (
                    "a".into(),
                    json!({"node_id":stable_node_id("a"),"name":"A"}),
                ),
                (
                    "b".into(),
                    json!({"node_id":stable_node_id("b"),"name":"B"}),
                ),
            ]),
        };
        let current = BTreeMap::from([
            (
                "a".into(),
                json!({"node_id":stable_node_id("a"),"name":"A2"}),
            ),
            (
                "c".into(),
                json!({"node_id":stable_node_id("c"),"name":"C"}),
            ),
        ]);
        assert!(delta_from_cache(&previous, 6, &current).is_none());
        let (changed, removed) = delta_from_cache(&previous, 7, &current).unwrap();
        assert_eq!(changed.len(), 2);
        assert_eq!(removed, vec![stable_node_id("b")]);
        assert!(stable_node_id(":1.5|/object").starts_with("ui-node:"));
        assert!(!stable_node_id(":1.5|/object").contains("/object"));
    }
}
