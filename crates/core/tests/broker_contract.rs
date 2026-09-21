//! These are Rust integration test SOURCES; consult VERIFY.md for execution status.
use semwright_backends::fake::FakeDesktop;
use semwright_core::{Broker, NoApprover, audit::Audit};
use semwright_policy::{Policy, PolicyConfig, Profile};
use semwright_types::{Envelope, ErrorCode, ExecuteRequest, unique_id};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

struct Fixture {
    broker: Arc<Broker>,
    desktop: Arc<FakeDesktop>,
    _dir: tempfile::TempDir,
    session: String,
}
impl Fixture {
    fn with_policy(config: PolicyConfig) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let audit = Audit::open(&dir.path().join("audit"), 65536, 2).unwrap();
        let desktop = Arc::new(FakeDesktop::new());
        let broker = Broker::new(
            Policy::new(config).unwrap(),
            vec![desktop.clone()],
            audit,
            Arc::new(NoApprover),
            None,
            json!({"fixture":true}),
            true,
        )
        .unwrap();
        Self {
            broker,
            desktop,
            _dir: dir,
            session: unique_id(),
        }
    }
    fn new(profile: Profile) -> Self {
        Self::with_policy(PolicyConfig {
            profile,
            ..Default::default()
        })
    }
    async fn call(&self, command: &str, args: Value) -> Envelope {
        self.broker
            .clone()
            .execute(
                self.session.clone(),
                unique_id(),
                ExecuteRequest {
                    command: command.into(),
                    args,
                    dry_run: false,
                    backend: None,
                },
                CancellationToken::new(),
            )
            .await
    }
    async fn find(&self, name: &str) -> Value {
        let result = self
            .call(
                "ui.find",
                json!({"selector":{"name":{"op":"exact","value":name}}}),
            )
            .await;
        assert!(result.ok, "{result:?}");
        result.data.unwrap()
    }
}
#[tokio::test]
async fn doctor_labels_fixture_not_live_desktop() {
    let f = Fixture::new(Profile::Observe);
    let r = f.call("doctor", json!({})).await;
    assert_eq!(r.data.unwrap()["fake"], true);
}
#[tokio::test]
async fn observe_can_inspect_but_cannot_invoke() {
    let f = Fixture::new(Profile::Observe);
    let n = f.find("Export").await;
    let r = f
        .call("ui.invoke", json!({"ref":n["nodes"][0]["ref"]}))
        .await;
    assert_eq!(r.error.unwrap().code, ErrorCode::PolicyDenied);
    assert_eq!(f.desktop.invocations(), 0);
}
#[tokio::test]
async fn duplicate_labels_remain_discovery_candidates() {
    let f = Fixture::new(Profile::Desktop);
    let n = f.find("Save").await;
    assert_eq!(n["count"], 2);
    assert_eq!(f.desktop.invocations(), 0);
}
#[tokio::test]
async fn mutation_requires_reference_not_selector() {
    let f = Fixture::new(Profile::Desktop);
    let r = f
        .call("ui.invoke", json!({"selector":{"name":"Save"}}))
        .await;
    assert_eq!(r.error.unwrap().code, ErrorCode::InvalidArgument);
}
#[tokio::test]
async fn exact_ref_invokes_and_then_becomes_stale() {
    let f = Fixture::new(Profile::Desktop);
    let n = f.find("Export").await;
    let args = json!({"ref":n["nodes"][0]["ref"],"action":"click"});
    assert!(f.call("ui.invoke", args.clone()).await.ok);
    assert_eq!(
        f.call("ui.invoke", args).await.error.unwrap().code,
        ErrorCode::StaleReference
    );
    assert_eq!(f.desktop.invocations(), 1);
}
#[tokio::test]
async fn reference_does_not_cross_sessions() {
    let f = Fixture::new(Profile::Desktop);
    let n = f.find("Export").await;
    let r = f
        .broker
        .clone()
        .execute(
            unique_id(),
            unique_id(),
            ExecuteRequest {
                command: "ui.invoke".into(),
                args: json!({"ref":n["nodes"][0]["ref"]}),
                dry_run: false,
                backend: None,
            },
            CancellationToken::new(),
        )
        .await;
    assert_eq!(r.error.unwrap().code, ErrorCode::StaleReference);
    assert_eq!(f.desktop.invocations(), 0);
}
#[tokio::test]
async fn vanished_node_cannot_target_a_replacement() {
    let f = Fixture::new(Profile::Desktop);
    let n = f.find("Export").await;
    f.desktop.disappear("export");
    assert_eq!(
        f.call("ui.invoke", json!({"ref":n["nodes"][0]["ref"]}))
            .await
            .error
            .unwrap()
            .code,
        ErrorCode::StaleReference
    );
}
#[tokio::test]
async fn dry_run_never_invokes() {
    let f = Fixture::new(Profile::Desktop);
    let n = f.find("Export").await;
    let r = f
        .broker
        .clone()
        .execute(
            f.session.clone(),
            unique_id(),
            ExecuteRequest {
                command: "ui.invoke".into(),
                args: json!({"ref":n["nodes"][0]["ref"]}),
                dry_run: true,
                backend: None,
            },
            CancellationToken::new(),
        )
        .await;
    assert!(r.ok);
    assert_eq!(r.data.unwrap()["side_effects"], false);
    assert_eq!(f.desktop.invocations(), 0);
}
#[tokio::test]
async fn injected_failure_is_not_retried() {
    let f = Fixture::new(Profile::Desktop);
    let n = f.find("Export").await;
    f.desktop.fail_next();
    let r = f
        .call("ui.invoke", json!({"ref":n["nodes"][0]["ref"]}))
        .await;
    assert!(!r.ok);
    assert!(!r.error.unwrap().outcome_known);
    assert!(r.execution.fallbacks_attempted.is_empty());
    assert_eq!(f.desktop.invocations(), 0);
}
#[tokio::test]
async fn cancelled_before_dispatch_has_no_side_effect() {
    let f = Fixture::new(Profile::Desktop);
    let n = f.find("Export").await;
    let token = CancellationToken::new();
    token.cancel();
    let r = f
        .broker
        .clone()
        .execute(
            f.session.clone(),
            unique_id(),
            ExecuteRequest {
                command: "ui.invoke".into(),
                args: json!({"ref":n["nodes"][0]["ref"]}),
                dry_run: false,
                backend: None,
            },
            token,
        )
        .await;
    assert_eq!(r.error.unwrap().code, ErrorCode::Cancelled);
    assert_eq!(f.desktop.invocations(), 0);
}
#[tokio::test]
async fn shell_is_not_registered() {
    let f = Fixture::new(Profile::Desktop);
    assert!(!f.call("shell.exec", json!({"command":"anything"})).await.ok);
}
#[tokio::test]
async fn extra_confirmation_flag_is_not_authority() {
    let f = Fixture::new(Profile::Desktop);
    let n = f.find("Export").await;
    let r = f
        .call(
            "ui.invoke",
            json!({"ref":n["nodes"][0]["ref"],"confirmed":true}),
        )
        .await;
    assert_eq!(r.error.unwrap().code, ErrorCode::InvalidArgument);
    assert_eq!(f.desktop.invocations(), 0);
}
#[tokio::test]
async fn configured_confirmation_cannot_be_self_approved() {
    let f = Fixture::with_policy(PolicyConfig {
        profile: Profile::Desktop,
        confirm_mutations: true,
        ..Default::default()
    });
    let n = f.find("Export").await;
    assert_eq!(
        f.call("ui.invoke", json!({"ref":n["nodes"][0]["ref"]}))
            .await
            .error
            .unwrap()
            .code,
        ErrorCode::ConsentRequired
    );
    assert_eq!(f.desktop.invocations(), 0);
}
#[tokio::test]
async fn app_scope_filters_discovery() {
    let mut config = PolicyConfig {
        profile: Profile::Desktop,
        ..Default::default()
    };
    config.apps.insert("different.application".into());
    let f = Fixture::with_policy(config);
    let n = f.find("Export").await;
    assert_eq!(n["count"], 0);
}
#[tokio::test]
async fn denied_audit_is_not_labelled_allow() {
    let f = Fixture::new(Profile::Observe);
    let n = f.find("Export").await;
    let r = f
        .call("ui.invoke", json!({"ref":n["nodes"][0]["ref"]}))
        .await;
    assert_eq!(r.execution.policy_decision, "deny");
}
#[tokio::test]
async fn audit_never_contains_typed_payload() {
    let f = Fixture::new(Profile::Desktop);
    let n = f.find("Filename").await;
    let secret = "never-in-metadata-5f32";
    assert!(
        f.call(
            "ui.set_text",
            json!({"ref":n["nodes"][0]["ref"],"text":secret})
        )
        .await
        .ok
    );
    let r = f.call("audit.tail", json!({"limit":100})).await;
    assert!(!serde_json::to_string(&r).unwrap().contains(secret));
}
#[tokio::test]
async fn full_fake_export_recipe() {
    let f = Fixture::new(Profile::Desktop);
    let recipe =
        semwright_recipes::parse(include_str!("../../../recipes/fake-export.yaml")).unwrap();
    let r = f
        .call("recipe.run", json!({"recipe":recipe,"inputs":{}}))
        .await;
    assert!(r.ok, "{r:?}");
    assert_eq!(r.data.unwrap()["outputs"]["changed"], true);
    assert_eq!(f.desktop.invocations(), 1);
}
#[tokio::test]
async fn observe_recipe_cannot_gain_mutation_permission() {
    let f = Fixture::new(Profile::Observe);
    let recipe =
        semwright_recipes::parse(include_str!("../../../recipes/fake-export.yaml")).unwrap();
    let r = f.call("recipe.run", json!({"recipe":recipe})).await;
    assert!(!r.ok);
    assert_eq!(r.error.unwrap().code, ErrorCode::PolicyDenied);
    assert_eq!(f.desktop.invocations(), 0);
}
#[tokio::test]
async fn recipe_dry_run_does_not_execute() {
    let f = Fixture::new(Profile::Desktop);
    let recipe =
        semwright_recipes::parse(include_str!("../../../recipes/fake-export.yaml")).unwrap();
    let r = f
        .broker
        .clone()
        .execute(
            f.session.clone(),
            unique_id(),
            ExecuteRequest {
                command: "recipe.run".into(),
                args: json!({"recipe":recipe}),
                dry_run: true,
                backend: None,
            },
            CancellationToken::new(),
        )
        .await;
    assert!(r.ok);
    assert_eq!(f.desktop.invocations(), 0);
}
#[tokio::test]
async fn session_revocation_invalidates_references() {
    let f = Fixture::new(Profile::Desktop);
    let n = f.find("Export").await;
    f.broker.revoke_session(&f.session);
    assert_eq!(
        f.call("ui.invoke", json!({"ref":n["nodes"][0]["ref"]}))
            .await
            .error
            .unwrap()
            .code,
        ErrorCode::StaleReference
    );
}
#[tokio::test]
async fn no_ref_migration_to_another_backend() {
    let f = Fixture::new(Profile::Desktop);
    let n = f.find("Export").await;
    let r = f
        .broker
        .clone()
        .execute(
            f.session.clone(),
            unique_id(),
            ExecuteRequest {
                command: "ui.invoke".into(),
                args: json!({"ref":n["nodes"][0]["ref"]}),
                dry_run: false,
                backend: Some("atspi".into()),
            },
            CancellationToken::new(),
        )
        .await;
    assert_eq!(r.error.unwrap().code, ErrorCode::PolicyDenied);
}
