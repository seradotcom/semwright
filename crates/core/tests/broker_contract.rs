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
async fn semantic_hit_test_materializes_a_ref_without_input_side_effects() {
    let f = Fixture::new(Profile::Observe);
    let result = f.call("ui.hit_test", json!({"x":10,"y":10})).await;
    assert!(result.ok, "{result:?}");
    let data = result.data.unwrap();
    let reference = data["node"]["ref"].as_str().unwrap();
    assert!(reference.starts_with("ui:"));
    assert_eq!(data["semantic_coverage"], "native_hit_test");
    assert_eq!(f.desktop.invocations(), 0);

    let miss = f.call("ui.hit_test", json!({"x":1000,"y":1000})).await;
    assert_eq!(miss.error.unwrap().code, ErrorCode::NotFound);
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
async fn app_scope_blocks_semantic_hit_test_observation() {
    let mut config = PolicyConfig {
        profile: Profile::Observe,
        ..Default::default()
    };
    config.apps.insert("different.application".into());
    let f = Fixture::with_policy(config);
    let result = f.call("ui.hit_test", json!({"x":10,"y":10})).await;
    assert_eq!(result.error.unwrap().code, ErrorCode::PolicyDenied);
    assert_eq!(f.desktop.invocations(), 0);
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

#[tokio::test]
async fn catalog_is_compact_provenanced_and_uses_actual_operation_state() {
    let fixture = Fixture::new(Profile::Observe);
    let result = fixture
        .call("capabilities.search", json!({"query":"ui.find","limit":1}))
        .await;
    assert!(result.ok, "{result:?}");
    let catalog = result.data.unwrap();
    let row = &catalog["capabilities"][0];
    assert_eq!(row["id"], "ui.find");
    assert_eq!(row["available"], true);
    assert_eq!(row["routes"][0]["provider"], "fake");
    assert!(row.get("input_schema").is_none());
    assert_eq!(
        row["provenance"]["descriptor_sha256"]
            .as_str()
            .unwrap()
            .len(),
        64
    );
    assert_eq!(catalog["availability_is_authorization"], false);
    let description = fixture
        .call("capabilities.describe", json!({"name":"ui.invoke"}))
        .await;
    assert!(description.data.unwrap()["capability"]["input_schema"].is_object());
    assert_eq!(
        fixture
            .call("capabilities.search", json!({"revision":0}))
            .await
            .error
            .unwrap()
            .code,
        ErrorCode::Conflict
    );
}

#[tokio::test]
async fn missing_native_provider_is_not_reported_available() {
    let fixture = Fixture::new(Profile::Observe);
    let result = fixture
        .call(
            "capabilities.search",
            json!({"provider":"blender-native","available":true}),
        )
        .await;
    assert!(result.ok);
    assert_eq!(result.data.unwrap()["total"], 0);
}

struct MixedPortal;
#[async_trait::async_trait]
impl semwright_backend_api::Backend for MixedPortal {
    fn name(&self) -> &'static str {
        "portal"
    }
    fn supports(&self, command: &str) -> bool {
        matches!(command, "screen.capture" | "portal.start")
    }
    fn operation_feature(&self, command: &str) -> Option<String> {
        Some(
            if command == "screen.capture" {
                "screen.capture"
            } else {
                "input.consented"
            }
            .into(),
        )
    }
    async fn probe(&self) -> Vec<semwright_types::Feature> {
        vec![semwright_backend_api::feature(
            "portal",
            "screen.capture",
            true,
            "Fixture screenshot-only service",
            "",
        )]
    }
    async fn execute(
        &self,
        _: &semwright_backend_api::Context,
        _: &str,
        _: &Value,
    ) -> semwright_types::Result<Value> {
        panic!("Catalog lookup must not execute a provider")
    }
}
#[tokio::test]
async fn one_working_probe_does_not_enable_unrelated_operations() {
    let directory = tempfile::tempdir().unwrap();
    let broker = Broker::new(
        Policy::new(PolicyConfig::default()).unwrap(),
        vec![Arc::new(MixedPortal)],
        Audit::open(&directory.path().join("audit"), 65536, 2).unwrap(),
        Arc::new(NoApprover),
        None,
        json!({}),
        false,
    )
    .unwrap();
    let capture = broker.catalog_describe("screen.capture").await.unwrap();
    let input = broker.catalog_describe("portal.start").await.unwrap();
    assert_eq!(capture["routes"][0]["available"], true);
    assert_eq!(input["routes"][0]["available"], false);
}

async fn wait_job(fixture: &Fixture, id: &str) -> Value {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let result = fixture.call("jobs.get", json!({"job_id":id})).await;
            assert!(result.ok, "{result:?}");
            let job = result.data.unwrap()["job"].clone();
            if matches!(
                job["state"].as_str(),
                Some("succeeded" | "failed" | "cancelled")
            ) {
                break job;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn read_only_job_executes_through_the_normal_broker_and_retains_result() {
    let fixture = Fixture::new(Profile::Observe);
    let started = fixture
        .call(
            "jobs.start",
            json!({"request":{"command":"app.list","args":{}}}),
        )
        .await;
    assert!(started.ok, "{started:?}");
    let id = started.data.unwrap()["job"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let job = wait_job(&fixture, &id).await;
    assert_eq!(job["state"], "succeeded");
    assert_eq!(job["command"], "app.list");
    assert_eq!(job["result"]["ok"], true);
    assert_eq!(job["result"]["command"], "app.list");
}

#[tokio::test]
async fn job_wrapper_cannot_turn_observe_into_mutation_authority() {
    let fixture = Fixture::new(Profile::Observe);
    let node = fixture.find("Export").await;
    let reference = node["nodes"][0]["ref"].clone();
    let started = fixture
        .call(
            "jobs.start",
            json!({"request":{"command":"ui.invoke","args":{"ref":reference,"action":"click"}}}),
        )
        .await;
    assert!(started.ok, "{started:?}");
    let id = started.data.unwrap()["job"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let job = wait_job(&fixture, &id).await;
    assert_eq!(job["state"], "failed");
    assert_eq!(job["result"]["error"]["code"], "PolicyDenied");
    assert_eq!(fixture.desktop.invocations(), 0);
}

#[tokio::test]
async fn jobs_are_not_visible_across_sessions() {
    let fixture = Fixture::new(Profile::Observe);
    let started = fixture
        .call(
            "jobs.start",
            json!({"request":{"command":"app.list","args":{}}}),
        )
        .await;
    let id = started.data.unwrap()["job"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let other_session = fixture
        .broker
        .clone()
        .execute(
            unique_id(),
            unique_id(),
            ExecuteRequest {
                command: "jobs.get".into(),
                args: json!({"job_id":id}),
                dry_run: false,
                backend: None,
            },
            CancellationToken::new(),
        )
        .await;
    assert_eq!(other_session.error.unwrap().code, ErrorCode::NotFound);
}

#[tokio::test]
async fn jobs_list_is_session_scoped_and_newest_first() {
    let fixture = Fixture::new(Profile::Observe);
    let first = fixture
        .call(
            "jobs.start",
            json!({"request":{"command":"app.list","args":{}}}),
        )
        .await;
    let first_id = first.data.unwrap()["job"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let second = fixture
        .call(
            "jobs.start",
            json!({"request":{"command":"window.list","args":{}}}),
        )
        .await;
    let second_id = second.data.unwrap()["job"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let listed = fixture.call("jobs.list", json!({})).await;
    assert!(listed.ok, "{listed:?}");
    let jobs = listed.data.unwrap()["jobs"].as_array().unwrap().clone();
    assert_eq!(jobs.len(), 2);
    assert_eq!(jobs[0]["id"].as_str(), Some(second_id.as_str()));
    assert_eq!(jobs[1]["id"].as_str(), Some(first_id.as_str()));

    let other = fixture
        .broker
        .clone()
        .execute(
            unique_id(),
            unique_id(),
            ExecuteRequest {
                command: "jobs.list".into(),
                args: json!({}),
                dry_run: false,
                backend: None,
            },
            CancellationToken::new(),
        )
        .await;
    assert!(other.ok, "{other:?}");
    assert_eq!(other.data.unwrap()["jobs"], json!([]));
}

#[tokio::test]
async fn job_events_are_visible_only_to_the_owning_session() {
    let fixture = Fixture::new(Profile::Observe);
    let started = fixture
        .call(
            "jobs.start",
            json!({"request":{"command":"app.list","args":{}}}),
        )
        .await;
    let id = started.data.unwrap()["job"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let _ = wait_job(&fixture, &id).await;

    let owner_events = fixture.broker.replay_for(&fixture.session, 0).unwrap();
    assert!(owner_events.iter().any(|event| {
        event.event.kind == "job.succeeded"
            && event.event.attributes.get("job_id") == Some(&json!(id))
    }));

    let stranger = unique_id();
    let stranger_events = fixture.broker.replay_for(&stranger, 0).unwrap();
    assert!(
        !stranger_events
            .iter()
            .any(|event| event.event.kind.starts_with("job."))
    );
    assert!(
        stranger_events
            .iter()
            .any(|event| event.event.kind == "command_start")
    );
}
