use crate::roles::semantic_role;
use semwright_platform_windows_sys::window::process_creation_time;
use semwright_types::{
    Error, ErrorCode, NativeTarget, Result, UiFacets, UiScrollFacet, UiSelectionFacet,
    UiTableFacet, UiTextFacet, UiTransformFacet, UiValueFacet, UiWindowFacet,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use uiautomation::patterns::{
    UIExpandCollapsePattern, UIGridItemPattern, UIGridPattern, UIInvokePattern,
    UIRangeValuePattern, UIScrollPattern, UISelectionItemPattern, UISelectionPattern,
    UITablePattern, UITextPattern, UITogglePattern, UITransformPattern, UIValuePattern,
    UIWindowPattern,
};
use uiautomation::{UIAutomation, UIElement, UITreeWalker, types::Point};

const MAX_NODES: usize = 2_000;
const MAX_DEPTH: usize = 32;
const MAX_CHILDREN: usize = 256;
const MAX_TEXT: usize = 4_096;
const SNAPSHOT_BUDGET: Duration = Duration::from_millis(1_500);

#[derive(Clone, Debug, PartialEq, Eq)]
struct Stamp {
    pid: u32,
    process_start: u64,
    runtime_id: Vec<i32>,
    automation_id: String,
    control_type: String,
    framework: String,
}

struct Entry {
    element: UIElement,
    stamp: Stamp,
    revision: u64,
}

struct State {
    automation: UIAutomation,
    walker: UITreeWalker,
    entries: BTreeMap<String, Entry>,
    next_revision: u64,
}

#[derive(Clone)]
pub struct UiaActor {
    tx: mpsc::Sender<Call>,
}

enum Call {
    Invoke(NativeTarget, mpsc::Sender<Result<Value>>),
    SetText(NativeTarget, String, mpsc::Sender<Result<Value>>),
    ReadText(NativeTarget, mpsc::Sender<Result<Value>>),
    SetValue(NativeTarget, f64, mpsc::Sender<Result<Value>>),
    GetValue(NativeTarget, mpsc::Sender<Result<Value>>),
    Toggle(NativeTarget, mpsc::Sender<Result<Value>>),
    Select(NativeTarget, usize, mpsc::Sender<Result<Value>>),
    Expand(NativeTarget, bool, mpsc::Sender<Result<Value>>),
    Snapshot(Option<NativeTarget>, mpsc::Sender<Result<Value>>),
    HitTest(i32, i32, mpsc::Sender<Result<Value>>),
    Validate(NativeTarget, mpsc::Sender<Result<Value>>),
    Focused(NativeTarget, mpsc::Sender<Result<Value>>),
    Shutdown(mpsc::Sender<Result<Value>>),
}

fn bounded(s: String) -> String {
    if s.len() <= MAX_TEXT {
        s
    } else {
        s.chars().take(MAX_TEXT).collect()
    }
}

fn digest_stamp(stamp: &Stamp) -> String {
    let mut h = Sha256::new();
    h.update(stamp.pid.to_le_bytes());
    h.update(stamp.process_start.to_le_bytes());
    for id in &stamp.runtime_id {
        h.update(id.to_le_bytes());
    }
    h.update(stamp.automation_id.as_bytes());
    h.update(stamp.control_type.as_bytes());
    h.update(stamp.framework.as_bytes());
    let d = h.finalize();
    let mut out = String::with_capacity(64);
    for b in d {
        use std::fmt::Write;
        let _ = write!(out, "{b:02x}");
    }
    out
}

fn uia_error(_: uiautomation::Error, message: &'static str) -> Error {
    Error::new(ErrorCode::BackendFailed, message)
}

fn stamp(element: &UIElement) -> Result<Stamp> {
    let pid = element
        .get_process_id()
        .map_err(|e| uia_error(e, "UIA process identity unavailable"))?;
    Ok(Stamp {
        pid,
        process_start: process_creation_time(pid)?,
        runtime_id: element
            .get_runtime_id()
            .map_err(|e| uia_error(e, "UIA RuntimeId unavailable"))?,
        automation_id: bounded(element.get_automation_id().unwrap_or_default()),
        control_type: format!(
            "{:?}",
            element
                .get_control_type()
                .map_err(|e| uia_error(e, "UIA control type unavailable"))?
        ),
        framework: bounded(element.get_framework_id().unwrap_or_default()),
    })
}

impl State {
    fn remember(&mut self, element: UIElement, kind: &str) -> Result<NativeTarget> {
        let stamp = stamp(&element)?;
        let identity = format!("uia:{}", digest_stamp(&stamp));
        if let Some(entry) = self.entries.get_mut(&identity)
            && entry.stamp == stamp
        {
            entry.element = element;
            return Ok(NativeTarget {
                kind: kind.into(),
                identity,
                revision: entry.revision,
                fingerprint: digest_stamp(&stamp),
                app: format!("pid:{}", stamp.pid),
            });
        }
        self.next_revision = self.next_revision.saturating_add(1);
        let revision = self.next_revision;
        self.entries.insert(
            identity.clone(),
            Entry {
                element,
                stamp: stamp.clone(),
                revision,
            },
        );
        Ok(NativeTarget {
            kind: kind.into(),
            identity,
            revision,
            fingerprint: digest_stamp(&stamp),
            app: format!("pid:{}", stamp.pid),
        })
    }

    fn resolve(&mut self, target: &NativeTarget) -> Result<UIElement> {
        let (element, expected, revision) = {
            let entry = self.entries.get(&target.identity).ok_or_else(|| {
                Error::new(ErrorCode::StaleReference, "Unknown Windows UIA reference")
            })?;
            (entry.element.clone(), entry.stamp.clone(), entry.revision)
        };
        if revision != target.revision || digest_stamp(&expected) != target.fingerprint {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Windows UIA reference generation mismatch",
            ));
        }
        let fresh = stamp(&element).map_err(|_| {
            Error::new(ErrorCode::StaleReference, "Windows UIA element disappeared")
        })?;
        if fresh != expected {
            self.entries.remove(&target.identity);
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Windows UIA object identity changed",
            ));
        }
        Ok(element)
    }

    fn semantic_facets(&self, element: &UIElement, password: bool) -> UiFacets {
        let mut facets = UiFacets::default();

        if let Ok(pattern) = element.get_pattern::<UITextPattern>() {
            facets.text = Some(UiTextFacet {
                character_count: None,
                caret_offset: None,
                selection_count: pattern.get_selection().ok().map(|rows| rows.len()),
                editable: element
                    .get_pattern::<UIValuePattern>()
                    .ok()
                    .is_some_and(|value| !value.is_readonly().unwrap_or(true)),
                password,
            });
        } else if element.get_pattern::<UIValuePattern>().is_ok() {
            facets.text = Some(UiTextFacet {
                editable: element
                    .get_pattern::<UIValuePattern>()
                    .ok()
                    .is_some_and(|value| !value.is_readonly().unwrap_or(true)),
                password,
                ..UiTextFacet::default()
            });
        }

        if let Ok(pattern) = element.get_pattern::<UIRangeValuePattern>() {
            facets.value = Some(UiValueFacet {
                current: pattern.get_value().ok(),
                minimum: pattern.get_minimum().ok(),
                maximum: pattern.get_maximum().ok(),
                increment: pattern.get_small_change().ok(),
                text: None,
            });
        } else if let Ok(pattern) = element.get_pattern::<UIValuePattern>() {
            facets.value = Some(UiValueFacet {
                text: (!password)
                    .then(|| pattern.get_value().ok().map(bounded))
                    .flatten(),
                ..UiValueFacet::default()
            });
        }

        let selected = element
            .get_pattern::<UISelectionItemPattern>()
            .ok()
            .and_then(|pattern| pattern.is_selected().ok());
        if let Ok(pattern) = element.get_pattern::<UISelectionPattern>() {
            facets.selection = Some(UiSelectionFacet {
                selected,
                selected_count: pattern.get_selection().ok().map(|rows| rows.len()),
                child_count: None,
                multi_select: pattern.can_select_multiple().ok(),
            });
        } else if selected.is_some() {
            facets.selection = Some(UiSelectionFacet {
                selected,
                ..UiSelectionFacet::default()
            });
        }

        let mut table = UiTableFacet::default();
        let mut has_table = false;
        if let Ok(pattern) = element.get_pattern::<UIGridPattern>() {
            table.rows = pattern
                .get_row_count()
                .ok()
                .and_then(|value| usize::try_from(value.max(0)).ok());
            table.columns = pattern
                .get_column_count()
                .ok()
                .and_then(|value| usize::try_from(value.max(0)).ok());
            has_table = true;
        }
        if let Ok(pattern) = element.get_pattern::<UIGridItemPattern>() {
            table.row = pattern
                .get_row()
                .ok()
                .and_then(|value| usize::try_from(value.max(0)).ok());
            table.column = pattern
                .get_column()
                .ok()
                .and_then(|value| usize::try_from(value.max(0)).ok());
            table.row_span = pattern
                .get_row_span()
                .ok()
                .and_then(|value| usize::try_from(value.max(0)).ok());
            table.column_span = pattern
                .get_column_span()
                .ok()
                .and_then(|value| usize::try_from(value.max(0)).ok());
            has_table = true;
        }
        if let Ok(pattern) = element.get_pattern::<UITablePattern>() {
            table.row_headers = pattern
                .get_row_headers()
                .unwrap_or_default()
                .into_iter()
                .take(64)
                .filter_map(|element| element.get_name().ok())
                .map(bounded)
                .collect();
            table.column_headers = pattern
                .get_column_headers()
                .unwrap_or_default()
                .into_iter()
                .take(64)
                .filter_map(|element| element.get_name().ok())
                .map(bounded)
                .collect();
            has_table = true;
        }
        if has_table {
            facets.table = Some(table);
        }

        if let Ok(pattern) = element.get_pattern::<UIScrollPattern>() {
            facets.scroll = Some(UiScrollFacet {
                horizontal_percent: pattern.get_horizontal_scroll_percent().ok(),
                vertical_percent: pattern.get_vertical_scroll_percent().ok(),
                horizontal_view_size: pattern.get_horizontal_view_size().ok(),
                vertical_view_size: pattern.get_vertical_view_size().ok(),
            });
        }

        if let Ok(pattern) = element.get_pattern::<UIWindowPattern>() {
            facets.window = Some(UiWindowFacet {
                modal: pattern.is_modal().ok(),
                minimized: pattern.is_minimized().ok(),
                maximized: pattern.is_maximized().ok(),
                can_minimize: pattern.can_minimize().ok(),
                can_maximize: pattern.can_maximize().ok(),
            });
        }

        if let Ok(pattern) = element.get_pattern::<UITransformPattern>() {
            facets.transform = Some(UiTransformFacet {
                can_move: pattern.can_move().ok(),
                can_resize: pattern.can_resize().ok(),
                can_rotate: pattern.can_rotate().ok(),
            });
        }

        facets
    }

    fn node(
        &mut self,
        element: UIElement,
        depth: usize,
        count: &mut usize,
        deadline: Instant,
    ) -> Result<Value> {
        if depth > MAX_DEPTH || *count >= MAX_NODES || Instant::now() >= deadline {
            return Ok(json!({"truncated": true}));
        }
        *count += 1;
        let control = element
            .get_control_type()
            .map_err(|e| uia_error(e, "UIA control type unavailable"))?;
        let password = element.is_password().unwrap_or(false);
        let target = self.remember(
            element.clone(),
            if semantic_role(control) == "window" {
                "win"
            } else {
                "ui"
            },
        )?;
        let rect = element.get_bounding_rectangle().ok();
        let mut children = Vec::new();
        if depth < MAX_DEPTH
            && *count < MAX_NODES
            && Instant::now() < deadline
            && let Ok(mut child) = self.walker.get_first_child(&element)
        {
            for _ in 0..MAX_CHILDREN {
                children.push(self.node(child.clone(), depth + 1, count, deadline)?);
                if *count >= MAX_NODES || Instant::now() >= deadline {
                    break;
                }
                match self.walker.get_next_sibling(&child) {
                    Ok(next) => child = next,
                    Err(_) => break,
                }
            }
        }
        let name = if password {
            String::new()
        } else {
            bounded(element.get_name().unwrap_or_default())
        };
        let enabled = element.is_enabled().unwrap_or(false);
        let focused = element.has_keyboard_focus().unwrap_or(false);
        let mut states = Vec::new();
        if enabled {
            states.push("enabled");
        }
        if focused {
            states.push("focused");
        }
        if password {
            states.push("password");
        }
        let mut actions = Vec::new();
        if element.get_pattern::<UIInvokePattern>().is_ok() {
            actions.push("click");
        }
        if element.get_pattern::<UIValuePattern>().is_ok() {
            actions.push("set_text");
        }
        if element.get_pattern::<UIRangeValuePattern>().is_ok() {
            actions.push("set_value");
        }
        if element.get_pattern::<UITogglePattern>().is_ok() {
            actions.push("toggle");
        }
        if element.get_pattern::<UISelectionItemPattern>().is_ok() {
            actions.push("select");
        }
        if element.get_pattern::<UIExpandCollapsePattern>().is_ok() {
            actions.push("expand");
        }
        let help = if password {
            String::new()
        } else {
            bounded(element.get_help_text().unwrap_or_default())
        };
        let automation_id = bounded(element.get_automation_id().unwrap_or_default());
        let framework = bounded(element.get_framework_id().unwrap_or_default());
        let class_name = bounded(element.get_classname().unwrap_or_default());
        let mut attributes = BTreeMap::new();
        if !class_name.is_empty() {
            attributes.insert("class".to_owned(), class_name.clone());
        }
        if let Ok(item_status) = element.get_item_status()
            && !item_status.is_empty()
        {
            attributes.insert("item_status".to_owned(), bounded(item_status));
        }
        let facets = self.semantic_facets(&element, password);
        let mut relations = Vec::new();
        if let Ok(label) = element.get_labeled_by()
            && let Ok(label_target) = self.remember(label, "ui")
        {
            relations.push(json!({
                "kind": "labelled_by",
                "targets": [{"$ref": label_target}]
            }));
        }
        Ok(json!({
            "$ref": target,
            "role": semantic_role(control),
            "name": name,
            "description": "",
            "help": help,
            "accessibility_id": automation_id,
            "framework": framework,
            "attributes": attributes,
            "relations": relations,
            "facets": facets,
            "states": states,
            "actions": actions,
            "app": format!("pid:{}", element.get_process_id().unwrap_or_default()),
            "class": class_name,
            "enabled": enabled,
            "focused": focused,
            "password": password,
            "bounds": rect.map(|r| json!({"x":r.get_left(),"y":r.get_top(),"width":r.get_width(),"height":r.get_height()})),
            "children": children,
        }))
    }

    fn flatten_snapshot_node(
        value: Value,
        parent: Option<Value>,
        nodes: &mut Vec<Value>,
        partial: &mut bool,
    ) -> Result<()> {
        let Value::Object(mut object) = value else {
            return Err(Error::new(
                ErrorCode::BackendFailed,
                "UIA snapshot node was not an object",
            ));
        };
        if object
            .get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            *partial = true;
            return Ok(());
        }
        let target = object.remove("$ref").ok_or_else(|| {
            Error::new(
                ErrorCode::BackendFailed,
                "UIA snapshot node omitted its native reference",
            )
        })?;
        let children = object
            .remove("children")
            .and_then(|value| value.as_array().cloned())
            .unwrap_or_default();
        let reference_marker = json!({"$ref": target.clone()});
        object.insert("ref".into(), reference_marker.clone());
        object.insert("parent_ref".into(), parent.unwrap_or(Value::Null));
        object.insert("children_count".into(), json!(children.len()));
        nodes.push(Value::Object(object));
        for child in children {
            Self::flatten_snapshot_node(child, Some(reference_marker.clone()), nodes, partial)?;
        }
        Ok(())
    }

    fn snapshot(&mut self, target: Option<NativeTarget>) -> Result<Value> {
        let root = match target {
            Some(t) => self.resolve(&t)?,
            None => self
                .automation
                .get_root_element()
                .map_err(|e| uia_error(e, "UIA desktop root unavailable"))?,
        };
        let mut count = 0usize;
        let deadline = Instant::now() + SNAPSHOT_BUDGET;
        let tree = self.node(root, 0, &mut count, deadline)?;
        let mut nodes = Vec::with_capacity(count.min(MAX_NODES));
        let mut partial = count >= MAX_NODES || Instant::now() >= deadline;
        Self::flatten_snapshot_node(tree, None, &mut nodes, &mut partial)?;
        Ok(json!({
            "nodes": nodes,
            "partial": partial,
            "revision": self.next_revision,
            "view": "control",
            "node_count": count,
            "max_nodes": MAX_NODES,
            "max_depth": MAX_DEPTH,
            "time_budget_ms": 1500
        }))
    }

    fn hit_test(&mut self, x: i32, y: i32) -> Result<Value> {
        let element = self
            .automation
            .element_from_point(Point::new(x, y))
            .map_err(|e| uia_error(e, "UIA element-from-point failed"))?;
        let mut count = 0usize;
        let tree = self.node(
            element,
            MAX_DEPTH,
            &mut count,
            Instant::now() + Duration::from_millis(250),
        )?;
        let mut nodes = Vec::with_capacity(1);
        let mut partial = false;
        Self::flatten_snapshot_node(tree, None, &mut nodes, &mut partial)?;
        let node = nodes.into_iter().next().ok_or_else(|| {
            Error::new(
                ErrorCode::NotFound,
                "UIA element-from-point did not produce a semantic node",
            )
        })?;
        Ok(json!({
            "node": node,
            "point": {"x": x, "y": y, "coordinate_space": "windows_virtual_desktop"},
            "semantic_coverage": "native_hit_test"
        }))
    }

    fn invoke(&mut self, target: &NativeTarget) -> Result<Value> {
        let e = self.resolve(target)?;
        e.get_pattern::<UIInvokePattern>()
            .map_err(|_| {
                Error::new(
                    ErrorCode::Unsupported,
                    "UIA element does not support InvokePattern",
                )
            })?
            .invoke()
            .map_err(|_| Error::new(ErrorCode::BackendFailed, "UIA InvokePattern failed"))?;
        Ok(json!({"invoked":true}))
    }

    fn set_text(&mut self, target: &NativeTarget, value: &str) -> Result<Value> {
        let e = self.resolve(target)?;
        if e.is_password().unwrap_or(false) {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Credential/password UI is not writable through generic UIA",
            ));
        }
        let p = e.get_pattern::<UIValuePattern>().map_err(|_| {
            Error::new(
                ErrorCode::Unsupported,
                "UIA element does not support ValuePattern",
            )
        })?;
        if p.is_readonly().unwrap_or(true) {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "UIA ValuePattern is read-only",
            ));
        }
        p.set_value(value)
            .map_err(|_| Error::new(ErrorCode::BackendFailed, "UIA ValuePattern set failed"))?;
        Ok(json!({"changed":true}))
    }

    fn read_text(&mut self, target: &NativeTarget) -> Result<Value> {
        let e = self.resolve(target)?;
        if e.is_password().unwrap_or(false) {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Password text is intentionally redacted",
            ));
        }
        if let Ok(p) = e.get_pattern::<UIValuePattern>() {
            return Ok(json!({"text":bounded(p.get_value().unwrap_or_default())}));
        }
        Ok(json!({"text":bounded(e.get_name().unwrap_or_default())}))
    }

    fn set_value(&mut self, target: &NativeTarget, value: f64) -> Result<Value> {
        if !value.is_finite() {
            return Err(Error::invalid("UIA range value must be finite"));
        }
        let e = self.resolve(target)?;
        e.get_pattern::<UIRangeValuePattern>()
            .map_err(|_| {
                Error::new(
                    ErrorCode::Unsupported,
                    "UIA element does not support RangeValuePattern",
                )
            })?
            .set_value(value)
            .map_err(|_| {
                Error::new(ErrorCode::BackendFailed, "UIA RangeValuePattern set failed")
            })?;
        Ok(json!({"applied":true}))
    }

    fn get_value(&mut self, target: &NativeTarget) -> Result<Value> {
        let e = self.resolve(target)?;
        let p = e.get_pattern::<UIRangeValuePattern>().map_err(|_| {
            Error::new(
                ErrorCode::Unsupported,
                "UIA element does not expose a numeric RangeValuePattern",
            )
        })?;
        let value = p.get_value().map_err(|_| {
            Error::new(
                ErrorCode::BackendFailed,
                "UIA RangeValuePattern read failed",
            )
        })?;
        let minimum = p.get_minimum().map_err(|_| {
            Error::new(
                ErrorCode::BackendFailed,
                "UIA RangeValuePattern minimum unavailable",
            )
        })?;
        let maximum = p.get_maximum().map_err(|_| {
            Error::new(
                ErrorCode::BackendFailed,
                "UIA RangeValuePattern maximum unavailable",
            )
        })?;
        Ok(json!({"value":value,"minimum":minimum,"maximum":maximum}))
    }

    fn toggle(&mut self, target: &NativeTarget) -> Result<Value> {
        let e = self.resolve(target)?;
        let p = e.get_pattern::<UITogglePattern>().map_err(|_| {
            Error::new(
                ErrorCode::Unsupported,
                "UIA element does not support TogglePattern",
            )
        })?;
        p.toggle()
            .map_err(|_| Error::new(ErrorCode::BackendFailed, "UIA TogglePattern failed"))?;
        Ok(json!({"invoked":true}))
    }

    fn select(&mut self, target: &NativeTarget, index: usize) -> Result<Value> {
        if index > 2_000 {
            return Err(Error::invalid("UIA selection index exceeds budget"));
        }
        let container = self.resolve(target)?;
        let mut child = self.walker.get_first_child(&container).map_err(|_| {
            Error::new(
                ErrorCode::Unsupported,
                "UIA selection container has no selectable children",
            )
        })?;
        for _ in 0..index {
            child = self.walker.get_next_sibling(&child).map_err(|_| {
                Error::new(
                    ErrorCode::InvalidArgument,
                    "UIA selection index is outside the container",
                )
            })?;
        }
        child
            .get_pattern::<UISelectionItemPattern>()
            .map_err(|_| {
                Error::new(
                    ErrorCode::Unsupported,
                    "UIA child does not support SelectionItemPattern",
                )
            })?
            .select()
            .map_err(|_| Error::new(ErrorCode::BackendFailed, "UIA SelectionItemPattern failed"))?;
        Ok(json!({"selected":true}))
    }

    fn expand(&mut self, target: &NativeTarget, expand: bool) -> Result<Value> {
        let e = self.resolve(target)?;
        let p = e.get_pattern::<UIExpandCollapsePattern>().map_err(|_| {
            Error::new(
                ErrorCode::Unsupported,
                "UIA element does not support ExpandCollapsePattern",
            )
        })?;
        if expand { p.expand() } else { p.collapse() }.map_err(|_| {
            Error::new(ErrorCode::BackendFailed, "UIA ExpandCollapsePattern failed")
        })?;
        Ok(json!({"applied":true}))
    }
}

impl UiaActor {
    pub fn start() -> Result<Self> {
        let (tx, rx) = mpsc::channel::<Call>();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name("semwright-uia".into())
            .spawn(move || {
                let setup = UIAutomation::new().and_then(|automation| {
                    automation
                        .get_control_view_walker()
                        .map(|walker| (automation, walker))
                });
                match setup {
                    Ok((automation, walker)) => {
                        let _ = ready_tx.send(Ok(()));
                        let mut state = State {
                            automation,
                            walker,
                            entries: BTreeMap::new(),
                            next_revision: 0,
                        };
                        while let Ok(call) = rx.recv() {
                            let stop = matches!(call, Call::Shutdown(_));
                            dispatch(&mut state, call);
                            if stop {
                                break;
                            }
                        }
                    }
                    Err(_) => {
                        let _ = ready_tx.send(Err(Error::unavailable(
                            "Microsoft UI Automation initialization failed",
                        )));
                    }
                }
            })
            .map_err(|_| Error::unavailable("UIA actor thread could not start"))?;
        ready_rx
            .recv()
            .map_err(|_| Error::unavailable("UIA actor terminated during initialization"))??;
        Ok(Self { tx })
    }

    fn request(&self, make: impl FnOnce(mpsc::Sender<Result<Value>>) -> Call) -> Result<Value> {
        let (tx, rx) = mpsc::channel();
        self.tx
            .send(make(tx))
            .map_err(|_| Error::unavailable("UIA actor is unavailable"))?;
        rx.recv_timeout(Duration::from_secs(5))
            .map_err(|_| Error::new(ErrorCode::Timeout, "UIA actor request timed out"))?
    }

    pub fn invoke(&self, t: NativeTarget) -> Result<Value> {
        self.request(|r| Call::Invoke(t, r))
    }
    pub fn set_text(&self, t: NativeTarget, v: String) -> Result<Value> {
        self.request(|r| Call::SetText(t, v, r))
    }
    pub fn read_text(&self, t: NativeTarget) -> Result<Value> {
        self.request(|r| Call::ReadText(t, r))
    }
    pub fn set_value(&self, t: NativeTarget, v: f64) -> Result<Value> {
        self.request(|r| Call::SetValue(t, v, r))
    }
    pub fn get_value(&self, t: NativeTarget) -> Result<Value> {
        self.request(|r| Call::GetValue(t, r))
    }
    pub fn toggle(&self, t: NativeTarget) -> Result<Value> {
        self.request(|r| Call::Toggle(t, r))
    }
    pub fn select(&self, t: NativeTarget, index: usize) -> Result<Value> {
        self.request(|r| Call::Select(t, index, r))
    }
    pub fn expand(&self, t: NativeTarget, v: bool) -> Result<Value> {
        self.request(|r| Call::Expand(t, v, r))
    }
    pub fn snapshot(&self, t: Option<NativeTarget>) -> Result<Value> {
        self.request(|r| Call::Snapshot(t, r))
    }
    pub fn hit_test(&self, x: i32, y: i32) -> Result<Value> {
        self.request(|r| Call::HitTest(x, y, r))
    }
    pub fn validate(&self, t: NativeTarget) -> Result<()> {
        self.request(|r| Call::Validate(t, r)).map(|_| ())
    }
    pub fn focused(&self, t: NativeTarget) -> Result<bool> {
        self.request(|r| Call::Focused(t, r))
            .map(|v| v["focused"].as_bool() == Some(true))
    }
    pub fn shutdown(&self) -> Result<()> {
        self.request(Call::Shutdown).map(|_| ())
    }
}

fn dispatch(state: &mut State, call: Call) {
    match call {
        Call::Invoke(t, r) => {
            let _ = r.send(state.invoke(&t));
        }
        Call::SetText(t, v, r) => {
            let _ = r.send(state.set_text(&t, &v));
        }
        Call::ReadText(t, r) => {
            let _ = r.send(state.read_text(&t));
        }
        Call::SetValue(t, v, r) => {
            let _ = r.send(state.set_value(&t, v));
        }
        Call::GetValue(t, r) => {
            let _ = r.send(state.get_value(&t));
        }
        Call::Toggle(t, r) => {
            let _ = r.send(state.toggle(&t));
        }
        Call::Select(t, index, r) => {
            let _ = r.send(state.select(&t, index));
        }
        Call::Expand(t, v, r) => {
            let _ = r.send(state.expand(&t, v));
        }
        Call::Snapshot(t, r) => {
            let _ = r.send(state.snapshot(t));
        }
        Call::HitTest(x, y, r) => {
            let _ = r.send(state.hit_test(x, y));
        }
        Call::Validate(t, r) => {
            let _ = r.send(state.resolve(&t).map(|_| json!({"valid":true})));
        }
        Call::Focused(t, r) => {
            let _ = r.send(
                state
                    .resolve(&t)
                    .map(|e| json!({"focused":e.has_keyboard_focus().unwrap_or(false)})),
            );
        }
        Call::Shutdown(r) => {
            let _ = r.send(Ok(json!({"shutdown":true})));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(identity: &str) -> NativeTarget {
        NativeTarget {
            kind: "ui".into(),
            identity: identity.into(),
            revision: 7,
            fingerprint: format!("fp:{identity}"),
            app: "pid:42".into(),
        }
    }

    #[test]
    fn flatten_snapshot_uses_portable_ref_markers() {
        let tree = json!({
            "$ref": target("uia:root"),
            "role": "window",
            "name": "Root",
            "children": [{
                "$ref": target("uia:child"),
                "role": "button",
                "name": "Save",
                "children": []
            }]
        });
        let mut nodes = Vec::new();
        let mut partial = false;
        State::flatten_snapshot_node(tree, None, &mut nodes, &mut partial).unwrap();
        assert!(!partial);
        assert_eq!(nodes.len(), 2);
        assert!(nodes[0].get("$ref").is_none());
        assert_eq!(nodes[0]["ref"]["$ref"]["identity"], "uia:root");
        assert_eq!(nodes[0]["parent_ref"], Value::Null);
        assert_eq!(nodes[1]["parent_ref"]["$ref"]["identity"], "uia:root");
        assert_eq!(nodes[1]["ref"]["$ref"]["identity"], "uia:child");
    }
}
