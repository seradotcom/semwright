//! AT-SPI2 over its dedicated accessibility bus. Public API types never leak zbus.
//! Invalidates all UI generations conservatively on object/window events or bus loss.
use async_trait::async_trait;
use futures_util::StreamExt;
use semwright_backend_api::{Backend, Context, ProviderSignal, feature};
use semwright_types::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::sync::{
    Arc, Mutex as StdMutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::Duration;
use tokio::sync::{Mutex as AsyncMutex, broadcast};
use zbus::{Connection, Proxy, zvariant::OwnedObjectPath};

type Object = (String, OwnedObjectPath);
const ACCESSIBLE: &str = "org.a11y.atspi.Accessible";
const APPLICATION: &str = "org.a11y.atspi.Application";
const COLLECTION: &str = "org.a11y.atspi.Collection";
const ROOT: &str = "/org/a11y/atspi/accessible/root";
// AtspiRole is a stable protocol enum; PASSWORD_TEXT has value 40.
// Some toolkits (notably Qt) specialize GetRole without specializing GetRoleName.
const ATSPI_ROLE_PASSWORD_TEXT: u32 = 40;
const MAX_CHILDREN_PER_NODE: usize = 2_000;
const SNAPSHOT_OPTIONAL_BUDGET: Duration = Duration::from_millis(250);
const COLLECTION_MATCH_ALL: i32 = 1;
const COLLECTION_SORT_CANONICAL: u32 = 1;
const COLLECTION_CANDIDATE_BUDGET: Duration = Duration::from_millis(1_000);
type CollectionRule = (
    Vec<i32>,
    i32,
    HashMap<String, String>,
    i32,
    Vec<i32>,
    i32,
    Vec<String>,
    i32,
    bool,
);
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
    signals: broadcast::Sender<ProviderSignal>,
}
impl Default for Atspi {
    fn default() -> Self {
        let (signals, _) = broadcast::channel(512);
        Self {
            connection: Arc::new(AsyncMutex::new(None)),
            connect_guard: AsyncMutex::new(()),
            connection_generation: Arc::new(AtomicU64::new(0)),
            revision: Arc::new(AtomicU64::new(1)),
            barrier_revision: Arc::new(AtomicU64::new(1)),
            events_live: Arc::new(AtomicBool::new(false)),
            object_generations: Arc::new(ObjectGenerations::default()),
            snapshots: StdMutex::new(BTreeMap::new()),
            signals,
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
fn snapshot_optional_budget(deadline: tokio::time::Instant) -> Option<Duration> {
    let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
    (!remaining.is_zero()).then_some(remaining.min(SNAPSHOT_OPTIONAL_BUDGET))
}
async fn snapshot_optional_until<T>(
    deadline: tokio::time::Instant,
    f: impl std::future::Future<Output = zbus::Result<T>>,
) -> Option<T> {
    let budget = snapshot_optional_budget(deadline)?;
    match tokio::time::timeout(budget, f).await {
        Ok(Ok(value)) => Some(value),
        _ => None,
    }
}
fn bounded_child_count(reported: Option<i32>, observed: usize) -> (usize, bool) {
    let reported = reported.and_then(|count| usize::try_from(count).ok());
    let actual = reported.unwrap_or(0).max(observed);
    (
        actual.min(MAX_CHILDREN_PER_NODE),
        actual > MAX_CHILDREN_PER_NODE,
    )
}
fn normalize_protocol_role(role_id: u32, role_name: &str) -> String {
    if role_id == ATSPI_ROLE_PASSWORD_TEXT {
        "password-entry".to_owned()
    } else {
        normalize_role(role_name)
    }
}

pub fn normalize_role(role: &str) -> String {
    match role {
        "application" => "application",
        "frame" | "dialog" | "window" => "window",
        "push button" | "button" => "button",
        "toggle button" => "toggle_button",
        "check box" => "check_box",
        "radio button" => "radio_button",
        "entry" | "text" => "text",
        "password text" => "password-entry",
        "static" | "label" | "accelerator label" => "label",
        "link" => "link",
        "menu" | "menu bar" | "popup menu" => "menu",
        "menu item" | "check menu item" | "radio menu item" => "menu_item",
        "list" => "list",
        "list item" => "list_item",
        "combo box" => "combo_box",
        "tree" => "tree",
        "tree item" => "tree_item",
        "table" => "table",
        "table cell" => "table_cell",
        "table row header" | "row header" => "table_row_header",
        "table column header" | "column header" => "table_column_header",
        "slider" => "slider",
        "spin button" => "spinner",
        "page tab list" => "tab_list",
        "page tab" => "tab",
        "tool bar" => "tool_bar",
        "scroll bar" => "scroll_bar",
        "scroll pane" => "scroll_pane",
        "panel" | "root pane" | "option pane" => "pane",
        "progress bar" => "progress_bar",
        "image" | "icon" => "image",
        "grouping" | "section" => "group",
        other => return other.to_lowercase().replace(' ', "-"),
    }
    .into()
}
const SEMANTIC_STATES: &[(usize, &str)] = &[
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

pub fn decode_states(bits: &[u32]) -> Vec<&'static str> {
    SEMANTIC_STATES
        .iter()
        .copied()
        .filter(|(bit, _)| {
            bits.get(bit / 32)
                .is_some_and(|word| word & (1u32 << (bit % 32)) != 0)
        })
        .map(|(_, name)| name)
        .collect()
}

fn collection_state_bits(states: &[String]) -> Option<Vec<i32>> {
    if states.is_empty() {
        return Some(Vec::new());
    }
    let mut highest = 0usize;
    let mut selected = Vec::with_capacity(states.len());
    for state in states {
        let bit = SEMANTIC_STATES
            .iter()
            .find_map(|(bit, name)| (*name == state).then_some(*bit))?;
        highest = highest.max(bit);
        selected.push(bit);
    }
    let mut words = vec![0u32; highest / 32 + 1];
    for bit in selected {
        words[bit / 32] |= 1u32 << (bit % 32);
    }
    Some(words.into_iter().map(|word| word as i32).collect())
}

fn collection_rule(selector: &Selector) -> Option<CollectionRule> {
    if selector.role.is_some()
        || selector.name.is_some()
        || selector.help.is_some()
        || selector.framework.is_some()
        || selector.action.is_some()
        || selector.relation.is_some()
        || selector.facet.is_some()
        || selector.text.is_some()
        || selector.value.is_some()
        || selector.selection.is_some()
        || selector.table.is_some()
        || selector.ancestor.is_some()
        || selector.nth.is_some()
        || selector.query.is_some()
        || (selector.states.is_empty() && selector.attributes.is_empty())
    {
        return None;
    }
    let states = collection_state_bits(&selector.states)?;
    // The ATK bridge parses ':' and '\\' inside attribute values as its own
    // multi-value escape syntax. Refuse those values here rather than risk a
    // native false negative; the caller will use the portable bounded path.
    if selector
        .attributes
        .values()
        .any(|value| value.contains(':') || value.contains('\\'))
    {
        return None;
    }
    let attributes = selector
        .attributes
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    // MATCH_ALL with an empty criterion is the neutral form implemented by the
    // current GNOME bridge. MATCH_EMPTY is not neutral: by specification it
    // requires the object's corresponding set to be empty.
    Some((
        states,
        COLLECTION_MATCH_ALL,
        attributes,
        COLLECTION_MATCH_ALL,
        Vec::new(),
        COLLECTION_MATCH_ALL,
        Vec::new(),
        COLLECTION_MATCH_ALL,
        false,
    ))
}
fn protect_collection_candidate(
    role: &str,
    states: &[&str],
    name: &mut String,
    attributes: &mut BTreeMap<String, String>,
) -> UiFacets {
    if role != "password-entry" {
        return UiFacets::default();
    }
    name.clear();
    attributes.clear();
    UiFacets {
        text: Some(UiTextFacet {
            editable: states.contains(&"editable"),
            password: true,
            ..UiTextFacet::default()
        }),
        ..UiFacets::default()
    }
}

fn object_id(o: &Object) -> String {
    format!("{}|{}", o.0, o.1)
}
fn relation_name(code: u32) -> String {
    match code {
        0 => "null",
        1 => "label_for",
        2 => "labelled_by",
        3 => "controller_for",
        4 => "controlled_by",
        5 => "member_of",
        6 => "tooltip_for",
        7 => "node_child_of",
        8 => "node_parent_of",
        9 => "extended",
        10 => "flows_to",
        11 => "flows_from",
        12 => "subwindow_of",
        13 => "embeds",
        14 => "embedded_by",
        15 => "popup_for",
        16 => "parent_window_of",
        17 => "description_for",
        18 => "described_by",
        19 => "details",
        20 => "details_for",
        21 => "error_message",
        22 => "error_for",
        _ => return format!("relation_{code}"),
    }
    .into()
}
fn supports_interface(interfaces: &[String], name: &str) -> bool {
    interfaces
        .iter()
        .any(|value| value == name || value.rsplit('.').next() == Some(name))
}

fn semantic_event_kind(interface: Option<&str>, member: Option<&str>) -> &'static str {
    if interface == Some("org.freedesktop.DBus") {
        return semantic_ui_event::BACKEND_INVALIDATED;
    }
    if interface == Some("org.a11y.atspi.Event.Window") {
        return if member.is_some_and(|name| {
            name.contains("Activate") || name.contains("Deactivate") || name.contains("Focus")
        }) {
            semantic_ui_event::FOCUS_CHANGED
        } else {
            semantic_ui_event::WINDOW_CHANGED
        };
    }
    match member.unwrap_or_default() {
        name if name.contains("ChildrenChanged") => semantic_ui_event::STRUCTURE_CHANGED,
        name if name.contains("TextSelectionChanged") || name.contains("SelectionChanged") => {
            semantic_ui_event::SELECTION_CHANGED
        }
        name if name.contains("TextChanged") || name.contains("TextAttributesChanged") => {
            semantic_ui_event::TEXT_CHANGED
        }
        name if name.contains("StateChanged") && name.to_ascii_lowercase().contains("focus") => {
            semantic_ui_event::FOCUS_CHANGED
        }
        name if name.contains("StateChanged") => semantic_ui_event::STATE_CHANGED,
        name if name.contains("PropertyChange") || name.contains("PropertyChanged") => {
            semantic_ui_event::PROPERTY_CHANGED
        }
        name if name.contains("BoundsChanged") || name.contains("VisibleDataChanged") => {
            semantic_ui_event::GEOMETRY_CHANGED
        }
        _ => semantic_ui_event::OBJECT_CHANGED,
    }
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
            let signals = self.signals.clone();
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
                    let identity = match (header.sender(), header.path()) {
                        (Some(sender), Some(path)) => {
                            Some(format!("{}|{}", sender.as_str(), path.as_str()))
                        }
                        _ => None,
                    };
                    let kind = semantic_event_kind(interface.as_deref(), member.as_deref());
                    let _ = signals.send(ProviderSignal::Event {
                        kind: kind.into(),
                        payload: json!({
                            "node_id": identity.as_deref().map(stable_node_id),
                            "native_interface": interface,
                            "native_member": member,
                            "semantic_revision": current,
                            "structural": structural
                        }),
                    });
                    if structural {
                        barrier_revision.store(current, Ordering::SeqCst);
                        generations.clear();
                        continue;
                    }
                    if let Some(identity) = identity {
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
        // These properties are independent D-Bus reads. Keeping them concurrent prevents
        // rich semantic snapshots from multiplying per-node bus latency.
        let (role_name, role_id, name) = tokio::join!(
            bounded(p.call::<_, _, String>("GetRoleName", &())),
            bounded(p.call::<_, _, u32>("GetRole", &())),
            bounded(p.get_property::<String>("Name"))
        );
        let role = normalize_protocol_role(role_id?, &role_name?);
        let name = name?;
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
    async fn app_framework(&self, c: &Connection, app: &Object) -> String {
        let Ok(proxy) = self.proxy(c, app, APPLICATION).await else {
            return String::new();
        };
        bounded(proxy.get_property::<String>("ToolkitName"))
            .await
            .unwrap_or_default()
            .chars()
            .take(128)
            .collect()
    }

    async fn relations(&self, c: &Connection, object: &Object, app: &str) -> Vec<Value> {
        let Ok(proxy) = self.proxy(c, object, ACCESSIBLE).await else {
            return vec![];
        };
        let Ok(relations) =
            bounded(proxy.call::<_, _, Vec<(u32, Vec<Object>)>>("GetRelationSet", &())).await
        else {
            return vec![];
        };
        let mut out = Vec::new();
        let mut total_targets = 0usize;
        for (kind, targets) in relations.into_iter().take(32) {
            let mut refs = Vec::new();
            for target in targets.into_iter().take(32) {
                if total_targets >= 128 {
                    break;
                }
                total_targets += 1;
                let identity = object_id(&target);
                let Ok((_, _, fingerprint)) = self.identity(c, &target).await else {
                    continue;
                };
                let target_app = self
                    .app_name(c, &target)
                    .await
                    .unwrap_or_else(|_| app.to_owned());
                let Ok(revision) = self.object_generations.get_or_assign(&identity) else {
                    continue;
                };
                refs.push(target_marker(NativeTarget {
                    kind: "ui".into(),
                    identity,
                    revision,
                    fingerprint,
                    app: target_app,
                }));
            }
            if !refs.is_empty() {
                out.push(json!({"kind":relation_name(kind),"targets":refs}));
            }
        }
        out
    }

    async fn facets(
        &self,
        c: &Connection,
        object: &Object,
        interfaces: &[String],
        states: &[&str],
        role: &str,
        child_count: usize,
    ) -> UiFacets {
        let mut facets = UiFacets::default();

        if supports_interface(interfaces, "Text")
            && let Ok(proxy) = self.proxy(c, object, "org.a11y.atspi.Text").await
        {
            let password = role == "password-entry";
            let (character_count, caret_offset, selection_count) = if password {
                (None, None, None)
            } else {
                let (character_count, caret_offset, selection_count) = tokio::join!(
                    bounded(proxy.get_property::<i32>("CharacterCount")),
                    bounded(proxy.get_property::<i32>("CaretOffset")),
                    bounded(proxy.call::<_, _, i32>("GetNSelections", &()))
                );
                (
                    character_count
                        .ok()
                        .and_then(|value| usize::try_from(value.max(0)).ok()),
                    caret_offset.ok().map(i64::from),
                    selection_count
                        .ok()
                        .and_then(|value| usize::try_from(value.max(0)).ok()),
                )
            };
            let mut selections = Vec::new();
            if !password {
                for index in 0..selection_count.unwrap_or(0).min(8) {
                    if let Ok((start, end)) =
                        bounded(proxy.call::<_, _, (i32, i32)>("GetSelection", &(index as i32,)))
                            .await
                        && start >= 0
                        && end >= start
                    {
                        selections.push(UiTextRange {
                            start: i64::from(start),
                            end: i64::from(end),
                        });
                    }
                }
            }
            let (caret_attributes, caret_attribute_range) = if !password
                && let Some(caret) = caret_offset.and_then(|value| i32::try_from(value).ok())
            {
                match bounded(
                    proxy.call::<_, _, (std::collections::HashMap<String, String>, i32, i32)>(
                        "GetAttributeRun",
                        &(caret, true),
                    ),
                )
                .await
                {
                    Ok((attributes, start, end)) if start >= 0 && end >= start => {
                        let attributes = attributes
                            .into_iter()
                            .take(32)
                            .map(|(key, value)| {
                                (
                                    key.chars().take(128).collect(),
                                    value.chars().take(512).collect(),
                                )
                            })
                            .collect();
                        (
                            attributes,
                            Some(UiTextRange {
                                start: i64::from(start),
                                end: i64::from(end),
                            }),
                        )
                    }
                    _ => (BTreeMap::new(), None),
                }
            } else {
                (BTreeMap::new(), None)
            };
            facets.text = Some(UiTextFacet {
                character_count,
                caret_offset,
                selection_count,
                selections,
                caret_attributes,
                caret_attribute_range,
                editable: supports_interface(interfaces, "EditableText")
                    || states.contains(&"editable"),
                password,
            });
            if password {
                return facets;
            }
        }

        if supports_interface(interfaces, "Value")
            && let Ok(proxy) = self.proxy(c, object, "org.a11y.atspi.Value").await
        {
            let (current, minimum, maximum, increment, text) = tokio::join!(
                bounded(proxy.get_property::<f64>("CurrentValue")),
                bounded(proxy.get_property::<f64>("MinimumValue")),
                bounded(proxy.get_property::<f64>("MaximumValue")),
                bounded(proxy.get_property::<f64>("MinimumIncrement")),
                bounded(proxy.get_property::<String>("Text"))
            );
            let current = current.ok();
            let minimum = minimum.ok();
            let maximum = maximum.ok();
            let increment = increment.ok();
            let text = text
                .ok()
                .filter(|value| !value.is_empty())
                .map(|value| value.chars().take(1024).collect());
            facets.value = Some(UiValueFacet {
                current,
                minimum,
                maximum,
                increment,
                text,
            });
        }

        if supports_interface(interfaces, "Selection")
            && let Ok(proxy) = self.proxy(c, object, "org.a11y.atspi.Selection").await
        {
            let selected_count = bounded(proxy.get_property::<i32>("NSelectedChildren"))
                .await
                .ok()
                .and_then(|value| usize::try_from(value.max(0)).ok());
            facets.selection = Some(UiSelectionFacet {
                selected: states.contains(&"selected").then_some(true),
                selected_count,
                child_count: Some(child_count),
                multi_select: None,
            });
        }

        if supports_interface(interfaces, "Table")
            && let Ok(proxy) = self.proxy(c, object, "org.a11y.atspi.Table").await
        {
            let nonnegative = |value: Result<i32>| {
                value
                    .ok()
                    .and_then(|value| usize::try_from(value.max(0)).ok())
            };
            let (rows, columns, selected_rows, selected_columns) = tokio::join!(
                bounded(proxy.get_property::<i32>("NRows")),
                bounded(proxy.get_property::<i32>("NColumns")),
                bounded(proxy.get_property::<i32>("NSelectedRows")),
                bounded(proxy.get_property::<i32>("NSelectedColumns"))
            );
            facets.table = Some(UiTableFacet {
                rows: nonnegative(rows),
                columns: nonnegative(columns),
                row: None,
                column: None,
                row_span: None,
                column_span: None,
                selected_rows: nonnegative(selected_rows),
                selected_columns: nonnegative(selected_columns),
                row_headers: vec![],
                column_headers: vec![],
            });
        }

        if supports_interface(interfaces, "TableCell")
            && let Ok(proxy) = self.proxy(c, object, "org.a11y.atspi.TableCell").await
        {
            let mut table = facets.table.take().unwrap_or_default();
            if let Ok((row, column, row_span, column_span)) =
                bounded(proxy.call::<_, _, (i32, i32, i32, i32)>("GetRowColumnSpan", &())).await
            {
                table.row = usize::try_from(row.max(0)).ok();
                table.column = usize::try_from(column.max(0)).ok();
                table.row_span = usize::try_from(row_span.max(0)).ok();
                table.column_span = usize::try_from(column_span.max(0)).ok();
            }
            if let Ok(headers) =
                bounded(proxy.call::<_, _, Vec<Object>>("GetRowHeaderCells", &())).await
            {
                for header in headers.into_iter().take(32) {
                    if let Ok((_, name, _)) = self.identity(c, &header).await
                        && !name.is_empty()
                    {
                        table.row_headers.push(name.chars().take(256).collect());
                    }
                }
            }
            if let Ok(headers) =
                bounded(proxy.call::<_, _, Vec<Object>>("GetColumnHeaderCells", &())).await
            {
                for header in headers.into_iter().take(32) {
                    if let Ok((_, name, _)) = self.identity(c, &header).await
                        && !name.is_empty()
                    {
                        table.column_headers.push(name.chars().take(256).collect());
                    }
                }
            }
            facets.table = Some(table);
        }

        if supports_interface(interfaces, "Document")
            && let Ok(proxy) = self.proxy(c, object, "org.a11y.atspi.Document").await
        {
            let (locale, page_index, page_count) = tokio::join!(
                bounded(proxy.call::<_, _, String>("GetLocale", &())),
                bounded(proxy.get_property::<i32>("CurrentPageNumber")),
                bounded(proxy.get_property::<i32>("PageCount"))
            );
            let locale = locale.ok().filter(|value| !value.is_empty());
            let page_index = page_index.ok().map(i64::from);
            let page_count = page_count.ok().map(i64::from);
            facets.document = Some(UiDocumentFacet {
                locale,
                page_index,
                page_count,
                mime_type: None,
            });
        }

        if supports_interface(interfaces, "Hypertext")
            && let Ok(proxy) = self.proxy(c, object, "org.a11y.atspi.Hypertext").await
        {
            let link_count = bounded(proxy.call::<_, _, i32>("GetNLinks", &()))
                .await
                .ok()
                .and_then(|value| usize::try_from(value.max(0)).ok());
            facets.hypertext = Some(UiHypertextFacet { link_count });
        }

        if supports_interface(interfaces, "Image")
            && let Ok(proxy) = self.proxy(c, object, "org.a11y.atspi.Image").await
        {
            let (description, locale) = tokio::join!(
                bounded(proxy.get_property::<String>("ImageDescription")),
                bounded(proxy.get_property::<String>("ImageLocale"))
            );
            let description = description
                .ok()
                .filter(|value| !value.is_empty())
                .map(|value| value.chars().take(1024).collect());
            let locale = locale
                .ok()
                .filter(|value| !value.is_empty())
                .map(|value| value.chars().take(128).collect());
            facets.image = Some(UiImageFacet {
                description,
                locale,
            });
        }

        if matches!(role, "window" | "frame" | "dialog") {
            facets.window = Some(UiWindowFacet {
                modal: states.contains(&"modal").then_some(true),
                minimized: None,
                maximized: None,
                can_minimize: None,
                can_maximize: None,
            });
        }

        facets
    }

    async fn within_depth(
        &self,
        c: &Connection,
        object: &Object,
        root: &Object,
        max_depth: usize,
    ) -> Result<bool> {
        let root_id = object_id(root);
        let mut current = object.clone();
        for depth in 0..=max_depth {
            if object_id(&current) == root_id {
                return Ok(true);
            }
            if depth == max_depth {
                return Ok(false);
            }
            let proxy = self.proxy(c, &current, ACCESSIBLE).await?;
            let parent: Object = bounded(proxy.get_property("Parent")).await?;
            if parent.0.is_empty() || parent.1.as_str() == "/org/a11y/atspi/null" {
                return Err(Error::new(
                    ErrorCode::BackendFailed,
                    "AT-SPI Collection candidate escaped its application root",
                ));
            }
            current = parent;
        }
        Ok(false)
    }

    async fn collection_candidate_node(
        &self,
        c: &Connection,
        object: &Object,
        app: &str,
        framework: &str,
        selector: &Selector,
    ) -> Result<Option<Value>> {
        let (role, mut name, fingerprint) = self.identity(c, object).await?;
        let proxy = self.proxy(c, object, ACCESSIBLE).await?;
        let (state_bits, attributes) = tokio::join!(
            bounded(proxy.call::<_, _, Vec<u32>>("GetState", &())),
            bounded(proxy.call::<_, _, HashMap<String, String>>("GetAttributes", &()))
        );
        let state_bits = state_bits?;
        let states = decode_states(&state_bits);
        if states
            .iter()
            .any(|state| matches!(*state, "defunct" | "stale"))
        {
            return Ok(None);
        }
        let mut attributes = attributes?
            .into_iter()
            .take(128)
            .map(|(key, value)| {
                (
                    key.chars().take(128).collect::<String>(),
                    value.chars().take(1024).collect::<String>(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        if !selector
            .states
            .iter()
            .all(|wanted| states.iter().any(|actual| actual == wanted))
            || !selector
                .attributes
                .iter()
                .all(|(key, value)| attributes.get(key) == Some(value))
        {
            return Ok(None);
        }
        let facets = protect_collection_candidate(&role, &states, &mut name, &mut attributes);
        let identity = object_id(object);
        let target = NativeTarget {
            kind: "ui".into(),
            identity: identity.clone(),
            revision: self.object_generations.get_or_assign(&identity)?,
            fingerprint,
            app: app.to_owned(),
        };
        Ok(Some(json!({
            "node_id": stable_node_id(&identity),
            "ref": target_marker(target),
            "role": role,
            "name": name.chars().take(1024).collect::<String>(),
            "description": "",
            "help": "",
            "accessibility_id": "",
            "framework": framework,
            "attributes": attributes,
            "relations": [],
            "facets": facets,
            "states": states,
            "actions": [],
            "app": app,
            "parent_ref": Value::Null,
            "bounds": Value::Null,
            "children_count": 0
        })))
    }

    async fn collection_candidates(
        &self,
        ctx: &Context,
        selector: &Selector,
        max_nodes: usize,
        max_depth: usize,
    ) -> Result<Option<Value>> {
        let Some(rule) = collection_rule(selector) else {
            return Ok(None);
        };
        let c = self.connect().await?;
        let revision = self.revision.load(Ordering::SeqCst);
        let max_nodes = max_nodes.clamp(1, 2_000);
        let max_depth = max_depth.min(32);
        let mut nodes = Vec::new();
        let mut seen = BTreeSet::new();
        let mut partial = false;

        for app_object in self.apps(&c).await? {
            ctx.check_cancelled()?;
            let app = match self.app_name(&c, &app_object).await {
                Ok(app) => app,
                Err(_) => continue,
            };
            if selector.app.as_ref().is_some_and(|wanted| wanted != &app) {
                continue;
            }
            let framework = self.app_framework(&c, &app_object).await;
            let root_id = object_id(&app_object);
            if seen.insert(root_id) {
                match self
                    .collection_candidate_node(&c, &app_object, &app, &framework, selector)
                    .await
                {
                    Ok(Some(node)) => {
                        nodes.push(node);
                        if nodes.len() >= max_nodes {
                            partial = true;
                            break;
                        }
                    }
                    Ok(None) => {}
                    Err(_) => return Ok(None),
                }
            }

            let collection = match self.proxy(&c, &app_object, COLLECTION).await {
                Ok(collection) => collection,
                Err(_) => return Ok(None),
            };
            let remaining = max_nodes.saturating_sub(nodes.len());
            if remaining == 0 {
                partial = true;
                break;
            }
            let count = i32::try_from(remaining)
                .map_err(|_| Error::invalid("AT-SPI candidate budget out of range"))?;
            let matches: Vec<Object> = match bounded(collection.call(
                "GetMatches",
                &(rule.clone(), COLLECTION_SORT_CANONICAL, count, true),
            ))
            .await
            {
                Ok(matches) => matches,
                Err(_) => return Ok(None),
            };
            let saturated = matches.len() >= remaining;
            for object in matches {
                ctx.check_cancelled()?;
                let within_depth =
                    match self.within_depth(&c, &object, &app_object, max_depth).await {
                        Ok(within_depth) => within_depth,
                        Err(_) => return Ok(None),
                    };
                if !within_depth {
                    continue;
                }
                let identity = object_id(&object);
                if !seen.insert(identity) {
                    continue;
                }
                match self
                    .collection_candidate_node(&c, &object, &app, &framework, selector)
                    .await
                {
                    Ok(Some(node)) => {
                        nodes.push(node);
                        if nodes.len() >= max_nodes {
                            partial = true;
                            break;
                        }
                    }
                    Ok(None) => {}
                    Err(_) => return Ok(None),
                }
            }
            partial |= saturated;
            if nodes.len() >= max_nodes {
                break;
            }
        }

        if self.revision.load(Ordering::SeqCst) != revision {
            // Native pushdown is only an optimization. If accessibility state changed
            // while collecting candidates, discard it and let the portable path resync.
            return Ok(None);
        }
        Ok(Some(json!({
            "nodes": nodes,
            "revision": revision,
            "partial": partial,
            "mode": "native_candidates",
            "semantic_coverage": "atspi_collection_candidates"
        })))
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
        // Rich metadata is opportunistic in a full-tree snapshot. One absolute deadline
        // prevents per-node optional reads from multiplying into multi-second stalls.
        // Exact ui.inspect keeps the deeper per-object path when a caller needs it.
        let optional_deadline = tokio::time::Instant::now() + Duration::from_millis(750);
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
            let framework = self.app_framework(&c, &app).await;
            queue.push_back((app, 0usize, app_name, framework, None::<String>));
        }
        let mut seen = BTreeSet::new();
        let mut nodes = vec![];
        let mut nodes_by_identity = BTreeMap::new();
        let mut partial = false;
        let mut visited = 0;
        while let Some((object, depth, app, framework, parent)) = queue.pop_front() {
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
            let (state_bits, mut actions, reported_child_count) = tokio::join!(
                async {
                    bounded(proxy.call::<_, _, Vec<u32>>("GetState", &()))
                        .await
                        .unwrap_or_default()
                },
                self.actions(&c, &object),
                async { bounded(proxy.get_property::<i32>("ChildCount")).await.ok() }
            );
            let states = decode_states(&state_bits);
            if states.contains(&"defunct") || states.contains(&"stale") {
                partial = true;
                continue;
            }
            let target = NativeTarget {
                kind: "ui".into(),
                identity: identity.clone(),
                revision: self.object_generations.get_or_assign(&identity)?,
                fingerprint,
                app: app.clone(),
            };
            known.insert(identity.clone(), target.clone());
            let mut children: Vec<Object> = if reported_child_count
                .and_then(|count| usize::try_from(count).ok())
                .is_some_and(|count| count > MAX_CHILDREN_PER_NODE)
            {
                // Do not request a potentially enormous GetChildren payload. Large/virtualized
                // containers are explicitly partial and can be searched through bounded native
                // query paths instead of being materialized wholesale.
                partial = true;
                vec![]
            } else {
                bounded(proxy.call("GetChildren", &()))
                    .await
                    .unwrap_or_else(|_| {
                        partial = true;
                        vec![]
                    })
            };
            if children.len() > MAX_CHILDREN_PER_NODE {
                children.truncate(MAX_CHILDREN_PER_NODE);
                partial = true;
            }
            let (count, child_count_truncated) =
                bounded_child_count(reported_child_count, children.len());
            partial |= child_count_truncated;
            if depth < max_depth {
                for child in children.into_iter() {
                    queue.push_back((
                        child,
                        depth + 1,
                        app.clone(),
                        framework.clone(),
                        Some(identity.clone()),
                    ));
                }
            } else if count > 0 {
                partial = true;
            }
            if actionable && actions.is_empty() && !states.contains(&"editable") {
                continue;
            }

            // Rich metadata reads are independent. Execute them concurrently so one slow
            // optional interface does not multiply snapshot latency across every node.
            let attributes_read = async {
                snapshot_optional_until(
                    optional_deadline,
                    proxy.call::<_, _, std::collections::HashMap<String, String>>(
                        "GetAttributes",
                        &(),
                    ),
                )
                .await
                .unwrap_or_default()
                .into_iter()
                .take(128)
                .map(|(key, value)| {
                    (
                        key.chars().take(128).collect(),
                        value.chars().take(1024).collect(),
                    )
                })
                .collect::<BTreeMap<String, String>>()
            };
            let help_read = async {
                snapshot_optional_until(optional_deadline, proxy.get_property::<String>("HelpText"))
                    .await
                    .unwrap_or_default()
                    .chars()
                    .take(1024)
                    .collect::<String>()
            };
            let accessibility_id_read = async {
                snapshot_optional_until(
                    optional_deadline,
                    proxy.get_property::<String>("AccessibleId"),
                )
                .await
                .unwrap_or_default()
                .chars()
                .take(512)
                .collect::<String>()
            };
            let locale_read = async {
                snapshot_optional_until(optional_deadline, proxy.get_property::<String>("Locale"))
                    .await
                    .unwrap_or_default()
                    .chars()
                    .take(128)
                    .collect::<String>()
            };
            let interfaces: Vec<String> = snapshot_optional_until(
                optional_deadline,
                proxy.call::<_, _, Vec<String>>("GetInterfaces", &()),
            )
            .await
            .unwrap_or_default()
            .into_iter()
            .take(64)
            .map(|value| value.chars().take(128).collect())
            .collect();
            let facets_read = async {
                let Some(budget) = snapshot_optional_budget(optional_deadline) else {
                    return UiFacets::default();
                };
                tokio::time::timeout(
                    budget,
                    self.facets(&c, &object, &interfaces, &states, &role, count),
                )
                .await
                .unwrap_or_default()
            };
            let relations_read = async {
                let Some(budget) = snapshot_optional_budget(optional_deadline) else {
                    return Vec::new();
                };
                tokio::time::timeout(budget, self.relations(&c, &object, &app))
                    .await
                    .unwrap_or_default()
            };
            let description_read = async {
                snapshot_optional_until(
                    optional_deadline,
                    proxy.get_property::<String>("Description"),
                )
                .await
                .unwrap_or_default()
            };
            let bounds_read = async {
                match self.proxy(&c, &object, "org.a11y.atspi.Component").await {
                    Ok(component) => snapshot_optional_until(
                        optional_deadline,
                        component.call::<_, _, (i32, i32, i32, i32)>("GetExtents", &(0u32,)),
                    )
                    .await
                    .map(|(x, y, width, height)| {
                        json!({"x":x,"y":y,"width":width,"height":height,
                            "coordinate_space":"atspi_screen_reported"})
                    }),
                    Err(_) => None,
                }
            };
            let (
                mut attributes,
                mut help,
                mut accessibility_id,
                locale,
                relations,
                mut description,
                bounds,
                mut facets,
            ) = tokio::join!(
                attributes_read,
                help_read,
                accessibility_id_read,
                locale_read,
                relations_read,
                description_read,
                bounds_read,
                facets_read
            );
            if facets.document.is_none() && !locale.is_empty() {
                facets.document = Some(UiDocumentFacet {
                    locale: Some(locale.clone()),
                    ..UiDocumentFacet::default()
                });
            }
            if role == "password-entry" {
                // Protected text semantics are security-critical, not optional rich metadata.
                // Preserve this facet even after the snapshot-wide metadata deadline expires.
                if facets.text.is_none() {
                    facets.text = Some(UiTextFacet {
                        editable: states.contains(&"editable"),
                        password: true,
                        ..UiTextFacet::default()
                    });
                }
                facets.value = None;
                actions.clear();
                name.clear();
                description.clear();
                help.clear();
                accessibility_id.clear();
                attributes.clear();
            }
            name = name.chars().take(1024).collect();
            description = description.chars().take(1024).collect();
            let row = json!({
                "node_id": stable_node_id(&identity),
                "ref": target_marker(target),
                "role": role,
                "name": name,
                "description": description,
                "help": help,
                "accessibility_id": accessibility_id,
                "framework": framework,
                "attributes": attributes,
                "relations": relations,
                "facets": facets,
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

    async fn inspect_target(&self, c: &Connection, target: &NativeTarget) -> Result<Value> {
        self.validate(target).await?;
        let object = object_from_id(&target.identity)?;
        let (role, mut name, _) = self.identity(c, &object).await?;
        let proxy = self.proxy(c, &object, ACCESSIBLE).await?;
        let state_bits: Vec<u32> = bounded(proxy.call("GetState", &()))
            .await
            .unwrap_or_default();
        let states = decode_states(&state_bits);
        if states.contains(&"defunct") || states.contains(&"stale") {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Accessible object became stale during inspection",
            ));
        }
        let actions = self.actions(c, &object).await;
        let child_count: i32 = bounded(proxy.get_property("ChildCount")).await.unwrap_or(0);
        let (child_count, _) = bounded_child_count(Some(child_count), 0);
        let mut attributes: BTreeMap<String, String> = bounded(
            proxy.call::<_, _, std::collections::HashMap<String, String>>("GetAttributes", &()),
        )
        .await
        .unwrap_or_default()
        .into_iter()
        .take(128)
        .map(|(key, value)| {
            (
                key.chars().take(128).collect(),
                value.chars().take(1024).collect(),
            )
        })
        .collect();
        let mut help: String = bounded(proxy.get_property::<String>("HelpText"))
            .await
            .unwrap_or_default()
            .chars()
            .take(1024)
            .collect();
        let mut accessibility_id: String = bounded(proxy.get_property::<String>("AccessibleId"))
            .await
            .unwrap_or_default()
            .chars()
            .take(512)
            .collect();
        let locale: String = bounded(proxy.get_property::<String>("Locale"))
            .await
            .unwrap_or_default()
            .chars()
            .take(128)
            .collect();
        let interfaces: Vec<String> =
            bounded(proxy.call::<_, _, Vec<String>>("GetInterfaces", &()))
                .await
                .unwrap_or_default()
                .into_iter()
                .take(64)
                .map(|value| value.chars().take(128).collect())
                .collect();
        let relations = self.relations(c, &object, &target.app).await;
        let mut facets = self
            .facets(c, &object, &interfaces, &states, &role, child_count)
            .await;
        if facets.document.is_none() && !locale.is_empty() {
            facets.document = Some(UiDocumentFacet {
                locale: Some(locale),
                ..UiDocumentFacet::default()
            });
        }
        let mut description: String = bounded(proxy.get_property("Description"))
            .await
            .unwrap_or_default();
        if role == "password-entry" {
            name.clear();
            description.clear();
            help.clear();
            accessibility_id.clear();
            attributes.clear();
        }
        let bounds = match self.proxy(c, &object, "org.a11y.atspi.Component").await {
            Ok(component) => {
                bounded(component.call::<_, _, (i32, i32, i32, i32)>("GetExtents", &(0u32,)))
                    .await
                    .ok()
                    .map(|(x, y, width, height)| {
                        json!({
                            "x": x,
                            "y": y,
                            "width": width,
                            "height": height,
                            "coordinate_space": "atspi_screen_reported"
                        })
                    })
            }
            Err(_) => None,
        };
        let mut framework = String::new();
        for app_object in self.apps(c).await.unwrap_or_default() {
            if self
                .app_name(c, &app_object)
                .await
                .is_ok_and(|name| name == target.app)
            {
                framework = self.app_framework(c, &app_object).await;
                break;
            }
        }
        Ok(json!({
            "node": {
                "node_id": stable_node_id(&target.identity),
                "ref": target_marker(target.clone()),
                "role": role,
                "name": name.chars().take(1024).collect::<String>(),
                "description": description.chars().take(1024).collect::<String>(),
                "help": help,
                "accessibility_id": accessibility_id,
                "framework": framework,
                "attributes": attributes,
                "relations": relations,
                "facets": facets,
                "states": states,
                "actions": actions,
                "app": target.app,
                "parent_ref": Value::Null,
                "bounds": bounds,
                "children_count": child_count
            },
            "semantic_coverage": "exact_ref"
        }))
    }

    async fn hit_test(&self, ctx: &Context, args: &Value) -> Result<Value> {
        let x = args["x"]
            .as_i64()
            .ok_or_else(|| Error::invalid("x required"))?;
        let y = args["y"]
            .as_i64()
            .ok_or_else(|| Error::invalid("y required"))?;
        if !(-1_000_000..=1_000_000).contains(&x) || !(-1_000_000..=1_000_000).contains(&y) {
            return Err(Error::invalid("UI hit-test coordinates exceed budget"));
        }
        let x = i32::try_from(x).map_err(|_| Error::invalid("x out of range"))?;
        let y = i32::try_from(y).map_err(|_| Error::invalid("y out of range"))?;
        let c = self.connect().await?;
        for app_object in self.apps(&c).await? {
            ctx.check_cancelled()?;
            let app = match self.app_name(&c, &app_object).await {
                Ok(app) => app,
                Err(_) => continue,
            };
            let framework = self.app_framework(&c, &app_object).await;
            let Ok(component) = self
                .proxy(&c, &app_object, "org.a11y.atspi.Component")
                .await
            else {
                continue;
            };
            let hit: Object =
                match bounded(component.call("GetAccessibleAtPoint", &(x, y, 0u32))).await {
                    Ok(hit) => hit,
                    Err(_) => continue,
                };
            if hit.0.is_empty() || hit.1.as_str() == "/org/a11y/atspi/null" {
                continue;
            }
            let identity = object_id(&hit);
            let (role, mut name, fingerprint) = self.identity(&c, &hit).await?;
            let proxy = self.proxy(&c, &hit, ACCESSIBLE).await?;
            let state_bits: Vec<u32> = bounded(proxy.call("GetState", &()))
                .await
                .unwrap_or_default();
            let states = decode_states(&state_bits);
            if states.contains(&"defunct") || states.contains(&"stale") {
                continue;
            }
            let actions = self.actions(&c, &hit).await;
            let child_count: i32 = bounded(proxy.get_property("ChildCount")).await.unwrap_or(0);
            let (child_count, _) = bounded_child_count(Some(child_count), 0);
            let mut attributes: BTreeMap<String, String> = bounded(
                proxy.call::<_, _, std::collections::HashMap<String, String>>("GetAttributes", &()),
            )
            .await
            .unwrap_or_default()
            .into_iter()
            .take(128)
            .map(|(key, value)| {
                (
                    key.chars().take(128).collect(),
                    value.chars().take(1024).collect(),
                )
            })
            .collect();
            let mut help: String = bounded(proxy.get_property::<String>("HelpText"))
                .await
                .unwrap_or_default()
                .chars()
                .take(1024)
                .collect();
            let mut accessibility_id: String =
                bounded(proxy.get_property::<String>("AccessibleId"))
                    .await
                    .unwrap_or_default()
                    .chars()
                    .take(512)
                    .collect();
            let locale: String = bounded(proxy.get_property::<String>("Locale"))
                .await
                .unwrap_or_default()
                .chars()
                .take(128)
                .collect();
            let interfaces: Vec<String> =
                bounded(proxy.call::<_, _, Vec<String>>("GetInterfaces", &()))
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .take(64)
                    .map(|value| value.chars().take(128).collect())
                    .collect();
            let relations = self.relations(&c, &hit, &app).await;
            let mut facets = self
                .facets(&c, &hit, &interfaces, &states, &role, child_count)
                .await;
            if facets.document.is_none() && !locale.is_empty() {
                facets.document = Some(UiDocumentFacet {
                    locale: Some(locale),
                    ..UiDocumentFacet::default()
                });
            }
            let mut description: String = bounded(proxy.get_property("Description"))
                .await
                .unwrap_or_default();
            if role == "password-entry" {
                name.clear();
                description.clear();
                help.clear();
                accessibility_id.clear();
                attributes.clear();
            }
            let bounds = match self.proxy(&c, &hit, "org.a11y.atspi.Component").await {
                Ok(component) => {
                    bounded(component.call::<_, _, (i32, i32, i32, i32)>("GetExtents", &(0u32,)))
                        .await
                        .ok()
                        .map(|(x, y, width, height)| {
                            json!({
                                "x": x,
                                "y": y,
                                "width": width,
                                "height": height,
                                "coordinate_space": "atspi_screen_reported"
                            })
                        })
                }
                Err(_) => None,
            };
            let target = NativeTarget {
                kind: "ui".into(),
                identity: identity.clone(),
                revision: self.object_generations.get_or_assign(&identity)?,
                fingerprint,
                app: app.clone(),
            };
            return Ok(json!({
                "node": {
                    "node_id": stable_node_id(&identity),
                    "ref": target_marker(target),
                    "role": role,
                    "name": name.chars().take(1024).collect::<String>(),
                    "description": description.chars().take(1024).collect::<String>(),
                    "help": help,
                    "accessibility_id": accessibility_id,
                    "framework": framework,
                    "attributes": attributes,
                    "relations": relations,
                    "facets": facets,
                    "states": states,
                    "actions": actions,
                    "app": app,
                    "parent_ref": Value::Null,
                    "bounds": bounds,
                    "children_count": child_count
                },
                "point": {"x": x, "y": y, "coordinate_space": "screen"},
                "semantic_coverage": "native_hit_test"
            }));
        }
        Err(Error::new(
            ErrorCode::NotFound,
            "No accessible object was reported at the requested screen point",
        ))
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
                | "ui.hit_test"
                | "ui.inspect"
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
    fn events(&self) -> Option<broadcast::Receiver<ProviderSignal>> {
        Some(self.signals.subscribe())
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
    async fn find_ui_candidates(
        &self,
        ctx: &Context,
        selector: &Selector,
        max_nodes: usize,
        max_depth: usize,
    ) -> Result<Option<Value>> {
        match tokio::time::timeout(
            COLLECTION_CANDIDATE_BUDGET,
            self.collection_candidates(ctx, selector, max_nodes, max_depth),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => Ok(None),
        }
    }

    async fn execute(&self, ctx: &Context, command: &str, args: &Value) -> Result<Value> {
        ctx.check_cancelled()?;
        if command == "ui.snapshot" {
            return self.snapshot(ctx, args).await;
        }
        if command == "ui.hit_test" {
            return self.hit_test(ctx, args).await;
        }
        let c = self.connect().await?;
        if command == "ui.inspect" {
            let target = native_target(args)?;
            return self.inspect_target(&c, &target).await;
        }
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
                if role == "password-entry" {
                    return Err(Error::new(
                        ErrorCode::PolicyDenied,
                        "Credential/password UI is not writable through generic AT-SPI",
                    ));
                }
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
    fn selector(value: Value) -> Selector {
        serde_json::from_value(value).expect("valid selector")
    }

    #[test]
    fn collection_rule_encodes_only_proven_state_and_attribute_predicates() {
        let selector = selector(json!({
            "app": "org.example.App",
            "states": ["enabled", "read-only"],
            "attributes": {"automation": "stable"}
        }));
        let rule = collection_rule(&selector).expect("safe Collection rule");
        assert_eq!(rule.0, vec![1 << 8, 1 << (43 - 32)]);
        assert_eq!(rule.1, COLLECTION_MATCH_ALL);
        assert_eq!(rule.2.get("automation").map(String::as_str), Some("stable"));
        assert_eq!(rule.3, COLLECTION_MATCH_ALL);
        assert_eq!(rule.5, COLLECTION_MATCH_ALL);
        assert_eq!(rule.7, COLLECTION_MATCH_ALL);
        assert!(!rule.8);
    }

    #[test]
    fn collection_rule_falls_back_for_unrepresentable_selector_semantics() {
        for value in [
            json!({"role": "button", "states": ["enabled"]}),
            json!({"name": {"op": "exact", "value": "Export"}, "states": ["enabled"]}),
            json!({"query": "export", "states": ["enabled"]}),
            json!({"states": ["future-state"]}),
            json!({"app": "org.example.App"}),
        ] {
            assert!(
                collection_rule(&selector(value)).is_none(),
                "unsupported semantics must fall back to the portable snapshot"
            );
        }
    }

    #[test]
    fn collection_rule_uses_vacuous_all_for_unused_dimensions() {
        let selector = selector(json!({"attributes": {"kind": "primary"}}));
        let rule = collection_rule(&selector).expect("attribute-only pushdown");
        assert!(rule.0.is_empty());
        assert_eq!(rule.1, COLLECTION_MATCH_ALL);
        assert_eq!(rule.3, COLLECTION_MATCH_ALL);
        assert!(rule.4.is_empty());
        assert_eq!(rule.5, COLLECTION_MATCH_ALL);
        assert!(rule.6.is_empty());
        assert_eq!(rule.7, COLLECTION_MATCH_ALL);
    }

    #[test]
    fn collection_rule_falls_back_for_bridge_attribute_escape_syntax() {
        for value in [
            json!({"attributes": {"kind": "one:two"}}),
            json!({"attributes": {"kind": "one\\two"}}),
        ] {
            assert!(
                collection_rule(&selector(value)).is_none(),
                "special bridge attribute encodings must use portable filtering"
            );
        }
    }

    #[test]
    fn collection_password_candidates_are_structural_and_redacted() {
        let mut name = "Secret fixture input".to_owned();
        let mut attributes = BTreeMap::from([
            ("automation".to_owned(), "secret-id".to_owned()),
            ("placeholder".to_owned(), "sensitive metadata".to_owned()),
        ]);
        let facets = protect_collection_candidate(
            "password-entry",
            &["enabled", "editable"],
            &mut name,
            &mut attributes,
        );
        assert!(name.is_empty());
        assert!(attributes.is_empty());
        let text = facets.text.expect("password text facet");
        assert!(text.password);
        assert!(text.editable);
        assert!(facets.value.is_none());
    }

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
        assert_eq!(
            normalize_protocol_role(ATSPI_ROLE_PASSWORD_TEXT, "text"),
            "password-entry"
        );
        assert_eq!(normalize_protocol_role(42, "text"), "text");
    }
    #[test]
    fn machine_button_role() {
        assert_eq!(normalize_role("push button"), "button");
    }
    #[test]
    fn rich_roles_normalize_to_portable_semantics() {
        for (native, semantic) in [
            ("dialog", "window"),
            ("check box", "check_box"),
            ("radio button", "radio_button"),
            ("combo box", "combo_box"),
            ("table cell", "table_cell"),
            ("table column header", "table_column_header"),
            ("page tab list", "tab_list"),
            ("scroll pane", "scroll_pane"),
            ("progress bar", "progress_bar"),
            ("icon", "image"),
            ("section", "group"),
        ] {
            assert_eq!(normalize_role(native), semantic);
        }
        assert_eq!(normalize_role("custom thing"), "custom-thing");
    }
    #[test]
    fn unique_owner_required() {
        assert!(object_from_id("org.app|/object").is_err());
        assert!(object_from_id(":1.5|/object").is_ok());
    }
    #[test]
    fn atspi_events_normalize_to_portable_semantics() {
        assert_eq!(
            semantic_event_kind(Some("org.a11y.atspi.Event.Object"), Some("ChildrenChanged")),
            "semantic.structure.changed"
        );
        assert_eq!(
            semantic_event_kind(Some("org.a11y.atspi.Event.Object"), Some("TextChanged")),
            "semantic.text.changed"
        );
        assert_eq!(
            semantic_event_kind(
                Some("org.a11y.atspi.Event.Object"),
                Some("SelectionChanged")
            ),
            "semantic.selection.changed"
        );
        assert_eq!(
            semantic_event_kind(Some("org.a11y.atspi.Event.Window"), Some("Activate")),
            "semantic.focus.changed"
        );
        assert_eq!(
            semantic_event_kind(Some("org.freedesktop.DBus"), Some("NameOwnerChanged")),
            "semantic.backend.invalidated"
        );
    }
    #[test]
    fn child_count_is_bounded_without_trusting_large_accessible_payloads() {
        assert_eq!(bounded_child_count(Some(12), 12), (12, false));
        assert_eq!(bounded_child_count(Some(-1), 8), (8, false));
        assert_eq!(
            bounded_child_count(Some(50_000), 0),
            (MAX_CHILDREN_PER_NODE, true)
        );
        assert_eq!(
            bounded_child_count(Some(1), MAX_CHILDREN_PER_NODE + 7),
            (MAX_CHILDREN_PER_NODE, true)
        );
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
