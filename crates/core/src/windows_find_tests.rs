//! Source-level Windows ui.find scope and reference-custody tests.
//! All UIA elements are synthetic; no Windows desktop or user window is read.
use super::*;
use semwright_policy::{PolicyConfig, Profile};
use std::sync::atomic::{AtomicUsize, Ordering};

struct BoundedWindows {
    snapshots: AtomicUsize,
    validated: AtomicUsize,
    last_target: StdMutex<Option<NativeTarget>>,
    wrong_root: bool,
}
impl BoundedWindows {
    fn new(wrong_root: bool) -> Self {
        Self {
            snapshots: AtomicUsize::new(0),
            validated: AtomicUsize::new(0),
            last_target: StdMutex::new(None),
            wrong_root,
        }
    }

    fn target(kind: &str, identity: &str, app: &str) -> NativeTarget {
        NativeTarget {
            kind: kind.into(),
            identity: identity.into(),
            revision: 1,
            fingerprint: "test-owned-generation".into(),
            app: app.into(),
        }
    }
}

#[async_trait::async_trait]
impl Backend for BoundedWindows {
    fn name(&self) -> &'static str {
        "windows"
    }

    fn supports(&self, command: &str) -> bool {
        command == "ui.snapshot"
    }

    fn operation_feature(&self, command: &str) -> Option<String> {
        self.supports(command).then(|| "uia.scoped".into())
    }

    async fn probe(&self) -> Vec<Feature> {
        vec![semwright_backend_api::feature(
            "windows",
            "uia.scoped",
            true,
            "Only synthetic exact-root snapshots are supported",
            "",
        )]
    }

    async fn validate(&self, target: &NativeTarget) -> Result<()> {
        self.validated.fetch_add(1, Ordering::SeqCst);
        if matches!(target.identity.as_str(), "win:owned-window" | "uia:root") {
            Ok(())
        } else {
            Err(Error::new(
                ErrorCode::StaleReference,
                "Synthetic Windows target generation is not live",
            ))
        }
    }

    async fn execute(&self, ctx: &Context, command: &str, args: &Value) -> Result<Value> {
        ctx.check_cancelled()?;
        if command != "ui.snapshot" {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "Synthetic Windows backend only supports UI snapshots",
            ));
        }
        self.snapshots.fetch_add(1, Ordering::SeqCst);
        let raw = args.get("_target").ok_or_else(|| {
            Error::new(
                ErrorCode::PolicyDenied,
                "An unscoped Windows UIA snapshot must never reach the backend",
            )
        })?;
        let target: NativeTarget = serde_json::from_value(raw.clone())?;
        *self.last_target.lock().unwrap() = Some(target.clone());
        let root_app = if self.wrong_root {
            "pid:99"
        } else {
            target.app.as_str()
        };
        let root = if target.identity.starts_with("win:") {
            Self::target("win", "uia:root", root_app)
        } else {
            Self::target(&target.kind, &target.identity, root_app)
        };
        let button = Self::target("ui", "uia:owned-button", &target.app);
        Ok(json!({
            "nodes": [
                {
                    "ref": target_marker(root.clone()),
                    "role": "window",
                    "name": "Owned Win32 fixture",
                    "app": root_app,
                    "parent_ref": null,
                    "children_count": 1
                },
                {
                    "ref": target_marker(button),
                    "role": "button",
                    "name": "Invoke me",
                    "app": target.app,
                    "parent_ref": target_marker(root),
                    "actions": ["click"]
                }
            ],
            "partial": false,
            "revision": 1
        }))
    }
}

struct WindowsFindFixture {
    broker: Arc<Broker>,
    backend: Arc<BoundedWindows>,
    session: String,
    _dir: tempfile::TempDir,
}
impl WindowsFindFixture {
    fn new(wrong_root: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let audit = audit::Audit::open(&dir.path().join("audit"), 65_536, 2).unwrap();
        let backend = Arc::new(BoundedWindows::new(wrong_root));
        let broker = Broker::new(
            Policy::new(PolicyConfig {
                profile: Profile::Observe,
                apps: vec!["pid:42".into()],
                ..Default::default()
            })
            .unwrap(),
            vec![backend.clone()],
            audit,
            Arc::new(NoApprover),
            None,
            json!({"fixture":"windows-find-only"}),
            false,
        )
        .unwrap();
        Self {
            broker,
            backend,
            session: unique_id(),
            _dir: dir,
        }
    }

    fn reference(&self, kind: &str, identity: &str, app: &str) -> String {
        self.broker
            .references
            .lock()
            .unwrap()
            .insert(
                &self.session,
                "windows",
                BoundedWindows::target(kind, identity, app),
            )
            .unwrap()
    }

    async fn execute(&self, session: &str, ancestor: Option<&str>, app: Option<&str>) -> Envelope {
        let mut selector = json!({
            "name": {"op":"exact","value":"Invoke me"},
            "role": "button"
        });
        if let Some(ancestor) = ancestor {
            selector["ancestor"] = json!(ancestor);
        }
        if let Some(app) = app {
            selector["app"] = json!(app);
        }
        self.broker
            .clone()
            .execute(
                session.to_owned(),
                unique_id(),
                ExecuteRequest {
                    command: "ui.find".into(),
                    args: json!({"selector":selector,"max_nodes":50,"max_depth":5}),
                    dry_run: false,
                    backend: Some("windows".into()),
                },
                CancellationToken::new(),
            )
            .await
    }
}

#[tokio::test]
async fn windows_ui_find_window_ancestor_routes_and_rebinds_only_its_native_root() {
    let fixture = WindowsFindFixture::new(false);
    let reference = fixture.reference("win", "win:owned-window", "pid:42");
    let result = fixture
        .execute(&fixture.session, Some(&reference), None)
        .await;
    assert!(result.ok, "{result:?}");
    let output = result.data.unwrap();
    assert_eq!(output["count"], 1);
    assert_eq!(output["nodes"][0]["name"], "Invoke me");
    assert_eq!(output["nodes"][0]["app"], "pid:42");
    assert_eq!(fixture.backend.snapshots.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.backend.validated.load(Ordering::SeqCst), 1);
    assert_eq!(
        fixture
            .backend
            .last_target
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .identity,
        "win:owned-window"
    );
}

#[tokio::test]
async fn windows_ui_find_ui_element_ancestor_preserves_exact_identity() {
    let fixture = WindowsFindFixture::new(false);
    let reference = fixture.reference("ui", "uia:root", "pid:42");
    let result = fixture
        .execute(&fixture.session, Some(&reference), Some("pid:42"))
        .await;
    assert!(result.ok, "{result:?}");
    assert_eq!(result.data.unwrap()["count"], 1);
    assert_eq!(fixture.backend.snapshots.load(Ordering::SeqCst), 1);
    assert_eq!(
        fixture
            .backend
            .last_target
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .identity,
        "uia:root"
    );
}

#[tokio::test]
async fn windows_ui_find_without_ancestor_never_snapshots_the_desktop() {
    let fixture = WindowsFindFixture::new(false);
    for app in [None, Some("pid:42")] {
        let result = fixture.execute(&fixture.session, None, app).await;
        assert!(!result.ok);
        assert_eq!(result.error.unwrap().code, ErrorCode::InvalidArgument);
    }
    assert_eq!(fixture.backend.snapshots.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn windows_ui_find_rejects_cross_session_and_unknown_ancestor_without_snapshot() {
    let fixture = WindowsFindFixture::new(false);
    let reference = fixture.reference("win", "win:owned-window", "pid:42");
    for (session, ancestor) in [
        ("unrelated-session", reference.as_str()),
        (
            fixture.session.as_str(),
            "win:00000000000000000000000000000000",
        ),
    ] {
        let result = fixture.execute(session, Some(ancestor), None).await;
        assert!(!result.ok);
        assert_eq!(result.error.unwrap().code, ErrorCode::StaleReference);
    }
    assert_eq!(fixture.backend.snapshots.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn windows_ui_find_does_not_accept_other_app_or_reference_kind() {
    let fixture = WindowsFindFixture::new(false);
    let reference = fixture.reference("win", "win:owned-window", "pid:42");
    let result = fixture
        .execute(&fixture.session, Some(&reference), Some("pid:99"))
        .await;
    assert!(!result.ok);
    assert_eq!(result.error.unwrap().code, ErrorCode::InvalidArgument);

    let other_app = fixture.reference("win", "win:owned-window", "pid:99");
    let result = fixture
        .execute(&fixture.session, Some(&other_app), None)
        .await;
    assert!(!result.ok);
    assert_eq!(result.error.unwrap().code, ErrorCode::PolicyDenied);

    let wrong_kind = fixture.reference("app", "win:owned-window", "pid:42");
    let result = fixture
        .execute(&fixture.session, Some(&wrong_kind), None)
        .await;
    assert!(!result.ok);
    assert_eq!(result.error.unwrap().code, ErrorCode::InvalidArgument);
    assert_eq!(fixture.backend.snapshots.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn windows_ui_find_rejects_mismatched_root_after_bounded_backend_call() {
    let fixture = WindowsFindFixture::new(true);
    let reference = fixture.reference("win", "win:owned-window", "pid:42");
    let result = fixture
        .execute(&fixture.session, Some(&reference), None)
        .await;
    assert!(!result.ok);
    assert_eq!(result.error.unwrap().code, ErrorCode::PolicyDenied);
    assert_eq!(fixture.backend.snapshots.load(Ordering::SeqCst), 1);
}
