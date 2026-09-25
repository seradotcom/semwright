//! Deterministic fake desktop. Instantiated only by explicit fake-mode daemon/tests.
use async_trait::async_trait;
use semwright_backend_api::{Backend, Context, feature};
use semwright_types::*;
use serde_json::{Value, json};
use std::sync::Mutex;

#[derive(Clone)]
pub struct FakeNode {
    pub id: String,
    pub role: String,
    pub name: String,
    pub actions: Vec<String>,
    pub parent: Option<String>,
    pub text: String,
    pub value: f64,
}
struct State {
    revision: u64,
    nodes: Vec<FakeNode>,
    focus: bool,
    clipboard: String,
    invocations: u64,
    fail_next: bool,
    consent: bool,
}
pub struct FakeDesktop {
    state: Mutex<State>,
}
impl Default for FakeDesktop {
    fn default() -> Self {
        Self::new()
    }
}
impl FakeDesktop {
    pub fn new() -> Self {
        let node =
            |id: &str, role: &str, name: &str, actions: Vec<&str>, parent: Option<&str>| FakeNode {
                id: id.into(),
                role: role.into(),
                name: name.into(),
                actions: actions.into_iter().map(String::from).collect(),
                parent: parent.map(String::from),
                text: String::new(),
                value: 0.0,
            };
        Self {
            state: Mutex::new(State {
                revision: 1,
                focus: true,
                clipboard: String::new(),
                invocations: 0,
                fail_next: false,
                consent: false,
                nodes: vec![
                    node("root", "frame", "Fixture Editor", vec![], None),
                    node("filename", "entry", "Filename", vec![], Some("root")),
                    node("export", "button", "Export", vec!["click"], Some("root")),
                    node("save1", "button", "Save", vec!["click"], Some("root")),
                    node("save2", "button", "Save", vec!["click"], Some("root")),
                    node(
                        "toggle",
                        "check-box",
                        "Enabled",
                        vec!["toggle"],
                        Some("root"),
                    ),
                ],
            }),
        }
    }
    pub fn disappear(&self, id: &str) {
        if let Ok(mut s) = self.state.lock() {
            s.nodes.retain(|n| n.id != id);
            s.revision += 1;
        }
    }
    pub fn set_focus(&self, value: bool) {
        if let Ok(mut s) = self.state.lock() {
            s.focus = value;
        }
    }
    pub fn fail_next(&self) {
        if let Ok(mut s) = self.state.lock() {
            s.fail_next = true;
        }
    }
    pub fn invocations(&self) -> u64 {
        self.state.lock().map(|s| s.invocations).unwrap_or(0)
    }
    fn target(kind: &str, id: &str, revision: u64, fingerprint: &str) -> NativeTarget {
        NativeTarget {
            kind: kind.into(),
            identity: id.into(),
            revision,
            fingerprint: fingerprint.into(),
            app: "org.semwright.Fixture".into(),
        }
    }
}
#[async_trait]
impl Backend for FakeDesktop {
    fn name(&self) -> &'static str {
        "fake"
    }
    fn supports(&self, command: &str) -> bool {
        matches!(
            command,
            "app.list"
                | "window.list"
                | "window.focus"
                | "window.move"
                | "window.resize"
                | "window.close"
                | "app.close"
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
                | "clipboard.read"
                | "clipboard.write"
                | "input.key"
                | "input.type"
                | "pointer.move"
                | "pointer.click"
                | "pointer.scroll"
                | "portal.start"
                | "portal.status"
                | "portal.stop"
        )
    }
    fn operation_feature(&self, command: &str) -> Option<String> {
        self.supports(command).then(|| "desktop.fixture".to_owned())
    }
    async fn probe(&self) -> Vec<Feature> {
        vec![feature(
            "fake",
            "desktop.fixture",
            true,
            "Explicit deterministic fixture; not a real desktop",
            "Never use fake results as evidence of compositor support",
        )]
    }
    async fn execute(&self, ctx: &Context, command: &str, args: &Value) -> Result<Value> {
        ctx.check_cancelled()?;
        let mut s = self
            .state
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "Fake state poisoned"))?;
        if s.fail_next {
            s.fail_next = false;
            return Err(Error::new(
                ErrorCode::BackendFailed,
                "Injected fixture failure",
            ));
        }
        match command {
            "app.list" => Ok(
                json!({"apps":[{"ref":target_marker(Self::target("app","app",s.revision,"fixture")),"name":"Fixture Editor","app":"org.semwright.Fixture"}]}),
            ),
            "window.list" => Ok(
                json!({"windows":[{"ref":target_marker(Self::target("win","window",1,"fixture-window")),"title":"Fixture Editor","app":"org.semwright.Fixture","focused":s.focus}]}),
            ),
            "ui.snapshot" => {
                let limit = args["max_nodes"].as_u64().unwrap_or(200).min(2000) as usize;
                let depth = args["max_depth"].as_u64().unwrap_or(5);
                let actionable = args["actionable"].as_bool().unwrap_or(false);
                let since_revision = args.get("since_revision").and_then(Value::as_u64);
                if since_revision.is_some_and(|since| since > s.revision) {
                    return Err(Error::new(
                        ErrorCode::Conflict,
                        "Snapshot base revision is newer than fixture state",
                    ));
                }
                let nodes:Vec<Value>=s.nodes.iter().filter(|n|depth>0||n.parent.is_none()).filter(|n|!actionable||!n.actions.is_empty()||n.role=="entry").take(limit).map(|n|json!({
                    "node_id":format!("fixture:{}",n.id),
                    "ref":target_marker(Self::target("ui",&n.id,s.revision,&format!("{}:{}",n.role,n.name))),
                    "role":n.role,"name":n.name,"description":"Deterministic test fixture","states":["enabled","visible"],"actions":n.actions,
                    "app":"org.semwright.Fixture","parent_ref":n.parent.as_ref().map(|p|target_marker(Self::target("ui",p,s.revision,"frame:Fixture Editor"))),
                    "bounds":{"x":0,"y":0,"width":80,"height":24,"coordinate_space":"fixture_logical"},"children_count":s.nodes.iter().filter(|c|c.parent.as_deref()==Some(n.id.as_str())).count()
                })).collect();
                let delta = since_revision == Some(s.revision);
                Ok(json!({
                    "revision":s.revision,
                    "partial":nodes.len()<s.nodes.len(),
                    "nodes":if delta {vec![]} else {nodes},
                    "mode":if delta {"delta"} else {"full"},
                    "base_revision":since_revision,
                    "removed_node_ids":Vec::<String>::new(),
                    "resync_required":since_revision.is_some()&&!delta,
                    "semantic_coverage":"fixture",
                    "budget":limit
                }))
            }
            "ui.inspect" => {
                let target = native_target(args)?;
                if target.revision != s.revision {
                    return Err(Error::new(
                        ErrorCode::StaleReference,
                        "Fixture revision changed",
                    ));
                }
                let n = s
                    .nodes
                    .iter()
                    .find(|n| n.id == target.identity)
                    .ok_or_else(|| {
                        Error::new(ErrorCode::StaleReference, "Fixture target disappeared")
                    })?;
                Ok(json!({
                    "node": {
                        "node_id": format!("fixture:{}", n.id),
                        "ref": target_marker(Self::target(
                            "ui",
                            &n.id,
                            s.revision,
                            &format!("{}:{}", n.role, n.name),
                        )),
                        "role": n.role,
                        "name": n.name,
                        "description": "Deterministic test fixture",
                        "help": "",
                        "accessibility_id": format!("fixture-{}", n.id),
                        "framework": "fixture",
                        "attributes": {},
                        "relations": [],
                        "facets": {},
                        "states": ["enabled", "visible"],
                        "actions": n.actions,
                        "app": "org.semwright.Fixture",
                        "parent_ref": n.parent.as_ref().map(|parent| target_marker(Self::target(
                            "ui",
                            parent,
                            s.revision,
                            "fixture-parent",
                        ))),
                        "bounds": {
                            "x": 0,
                            "y": 0,
                            "width": 80,
                            "height": 24,
                            "coordinate_space": "fixture_logical"
                        },
                        "children_count": s.nodes
                            .iter()
                            .filter(|c| c.parent.as_deref() == Some(n.id.as_str()))
                            .count()
                    },
                    "semantic_coverage": "exact_ref"
                }))
            }
            "ui.hit_test" => {
                let x = args["x"]
                    .as_f64()
                    .ok_or_else(|| Error::invalid("x required"))?;
                let y = args["y"]
                    .as_f64()
                    .ok_or_else(|| Error::invalid("y required"))?;
                if !(0.0..80.0).contains(&x) || !(0.0..24.0).contains(&y) {
                    return Err(Error::new(
                        ErrorCode::NotFound,
                        "Fixture has no semantic node at that point",
                    ));
                }
                let n = s.nodes.first().ok_or_else(|| {
                    Error::new(ErrorCode::NotFound, "Fixture has no semantic nodes")
                })?;
                Ok(json!({
                    "node": {
                        "node_id": format!("fixture:{}", n.id),
                        "ref": target_marker(Self::target(
                            "ui",
                            &n.id,
                            s.revision,
                            &format!("{}:{}", n.role, n.name),
                        )),
                        "role": n.role,
                        "name": n.name,
                        "description": "Deterministic test fixture",
                        "help": "",
                        "accessibility_id": format!("fixture-{}", n.id),
                        "framework": "fixture",
                        "attributes": {},
                        "relations": [],
                        "facets": {},
                        "states": ["enabled", "visible"],
                        "actions": n.actions,
                        "app": "org.semwright.Fixture",
                        "parent_ref": Value::Null,
                        "bounds": {
                            "x": 0,
                            "y": 0,
                            "width": 80,
                            "height": 24,
                            "coordinate_space": "fixture_logical"
                        },
                        "children_count": s.nodes
                            .iter()
                            .filter(|c| c.parent.as_deref() == Some(n.id.as_str()))
                            .count()
                    },
                    "point": {"x": x, "y": y, "coordinate_space": "fixture_logical"},
                    "semantic_coverage": "native_hit_test"
                }))
            }
            "window.focus" => {
                s.focus = true;
                Ok(json!({"focused":true}))
            }
            "window.move" | "window.resize" => {
                Ok(json!({"changed":true,"coordinate_space":"fixture_logical"}))
            }
            "window.close" | "app.close" => {
                s.nodes.clear();
                s.revision += 1;
                Ok(json!({"closed":true}))
            }
            "ui.invoke" | "ui.toggle" | "ui.expand" | "ui.select" | "ui.set_text"
            | "ui.read_text" | "ui.set_value" | "ui.get_value" => {
                let target = native_target(args)?;
                if target.revision != s.revision {
                    return Err(Error::new(
                        ErrorCode::StaleReference,
                        "Fixture revision changed",
                    ));
                }
                let n = s
                    .nodes
                    .iter_mut()
                    .find(|n| n.id == target.identity)
                    .ok_or_else(|| {
                        Error::new(ErrorCode::StaleReference, "Fixture target disappeared")
                    })?;
                if command == "ui.read_text" {
                    return Ok(json!({"text":n.text}));
                }
                if command == "ui.get_value" {
                    return Ok(json!({"value":n.value,"minimum":0,"maximum":100}));
                }
                if command == "ui.set_text" {
                    if n.role != "entry" {
                        return Err(Error::new(ErrorCode::Unsupported, "Target is not editable"));
                    }
                    n.text = arg_str(args, "text")?.into();
                } else if command == "ui.set_value" {
                    n.value = args["value"]
                        .as_f64()
                        .ok_or_else(|| Error::invalid("Value required"))?;
                } else {
                    let action = args["action"]
                        .as_str()
                        .unwrap_or(if command == "ui.toggle" {
                            "toggle"
                        } else {
                            "click"
                        });
                    if !n.actions.iter().any(|a| a == action) {
                        return Err(Error::new(
                            ErrorCode::Unsupported,
                            "Action is not advertised by this node",
                        ));
                    }
                    s.invocations += 1;
                }
                s.revision += 1;
                Ok(json!({"changed":true,"revision":s.revision}))
            }
            "clipboard.read" => Ok(json!({"text":s.clipboard})),
            "clipboard.write" => {
                s.clipboard = arg_str(args, "text")?.into();
                Ok(json!({"written":true}))
            }
            "portal.start" => {
                s.consent = true;
                Ok(json!({"consent":"granted","fixture":true}))
            }
            "portal.stop" => {
                s.consent = false;
                Ok(json!({"consent":"expired"}))
            }
            "portal.status" => Ok(json!({"consent":if s.consent{"granted"}else{"not_requested"}})),
            c if c.starts_with("input.") || c.starts_with("pointer.") => {
                if !s.focus {
                    return Err(Error::new(
                        ErrorCode::Conflict,
                        "Expected window lost focus",
                    ));
                }
                if !s.consent {
                    return Err(Error::new(
                        ErrorCode::ConsentRequired,
                        "Fixture input consent not granted",
                    ));
                }
                Ok(json!({"sent":true}))
            }
            _ => Err(Error::new(
                ErrorCode::Unsupported,
                "No fixture implementation for this command",
            )),
        }
    }
    async fn validate(&self, target: &NativeTarget) -> Result<()> {
        let s = self
            .state
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "Fake state poisoned"))?;
        if target.kind == "win"
            && target.identity == "window"
            && target.fingerprint == "fixture-window"
            && !s.nodes.is_empty()
        {
            return Ok(());
        }
        if target.kind == "app"
            && target.identity == "app"
            && target.fingerprint == "fixture"
            && target.revision == s.revision
            && !s.nodes.is_empty()
        {
            return Ok(());
        }
        if target.revision != s.revision
            || !s.nodes.iter().any(|n| {
                n.id == target.identity && format!("{}:{}", n.role, n.name) == target.fingerprint
            })
        {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Fixture identity or generation changed",
            ));
        }
        Ok(())
    }
    async fn is_focused(&self, target: &NativeTarget) -> Result<bool> {
        Ok(target.identity == "window"
            && self
                .state
                .lock()
                .map_err(|_| Error::new(ErrorCode::Internal, "Fake state poisoned"))?
                .focus)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> Context {
        Context {
            session: "fake-delta-test".into(),
            request_id: semwright_types::unique_id(),
            cancellation: tokio_util::sync::CancellationToken::new(),
        }
    }

    #[tokio::test]
    async fn snapshot_delta_contract_is_conservative() {
        let backend = FakeDesktop::new();
        let first = backend
            .execute(&context(), "ui.snapshot", &json!({}))
            .await
            .unwrap();
        let revision = first["revision"].as_u64().unwrap();
        assert_eq!(first["mode"], "full");
        assert_eq!(first["resync_required"], false);
        assert!(first["nodes"][0]["node_id"].as_str().is_some());

        let unchanged = backend
            .execute(
                &context(),
                "ui.snapshot",
                &json!({"since_revision":revision}),
            )
            .await
            .unwrap();
        assert_eq!(unchanged["mode"], "delta");
        assert_eq!(unchanged["nodes"].as_array().unwrap().len(), 0);
        assert_eq!(unchanged["resync_required"], false);

        backend.disappear("save1");
        let resync = backend
            .execute(
                &context(),
                "ui.snapshot",
                &json!({"since_revision":revision}),
            )
            .await
            .unwrap();
        assert_eq!(resync["mode"], "full");
        assert_eq!(resync["resync_required"], true);

        let future = backend
            .execute(
                &context(),
                "ui.snapshot",
                &json!({"since_revision":u64::MAX}),
            )
            .await
            .unwrap_err();
        assert_eq!(future.code, ErrorCode::Conflict);
    }
}
