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

fn workflow_fixture() -> Fixture {
    let mut config = PolicyConfig {
        profile: Profile::Desktop,
        ..Default::default()
    };
    config.allow.extend(
        ["workflow.record", "workflow.manage", "clipboard.write"]
            .into_iter()
            .map(String::from),
    );
    let fixture = Fixture::with_policy(config);
    fixture
        .broker
        .configure_workflows(&fixture._dir.path().join("workflows"))
        .unwrap();
    fixture
}

async fn record_clipboard_trace(fixture: &Fixture, text: &str) -> String {
    let started = fixture
        .call(
            "workflow.record.start",
            json!({"name":"clipboard-demo","intent":"Write clipboard text","capture_values":true}),
        )
        .await;
    assert!(started.ok, "{started:?}");
    let written = fixture.call("clipboard.write", json!({"text":text})).await;
    assert!(written.ok, "{written:?}");
    let stopped = fixture
        .call("workflow.record.stop", json!({"successful":true}))
        .await;
    assert!(stopped.ok, "{stopped:?}");
    stopped.data.unwrap()["trace"]["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn workflow_recording_requires_explicit_scope() {
    let fixture = Fixture::new(Profile::Desktop);
    let result = fixture
        .call(
            "workflow.record.start",
            json!({"name":"denied","capture_values":false}),
        )
        .await;
    assert_eq!(result.error.unwrap().code, ErrorCode::PolicyDenied);
}

#[tokio::test]
async fn metadata_only_workflow_trace_redacts_values_and_cannot_compile() {
    let fixture = workflow_fixture();
    let secret = "do-not-store-this-value";
    let started = fixture
        .call(
            "workflow.record.start",
            json!({"name":"private","intent":"privacy test","capture_values":false}),
        )
        .await;
    assert!(started.ok, "{started:?}");
    assert!(
        fixture
            .call("clipboard.write", json!({"text":secret}))
            .await
            .ok
    );
    let stopped = fixture
        .call("workflow.record.stop", json!({"successful":true}))
        .await;
    assert!(stopped.ok, "{stopped:?}");
    let trace = stopped.data.unwrap()["trace"].clone();
    let serialized = serde_json::to_string(&trace).unwrap();
    assert!(!serialized.contains(secret));
    assert_eq!(trace["steps"][0]["redacted"], true);
    let trace_id = trace["id"].as_str().unwrap();
    let compiled = fixture
        .call(
            "workflow.compile",
            json!({"trace_ids":[trace_id],"name":"private-copy"}),
        )
        .await;
    assert_eq!(compiled.error.unwrap().code, ErrorCode::Conflict);
}

#[tokio::test]
async fn learned_workflow_infers_input_verifies_replays_promotes_and_executes() {
    let fixture = workflow_fixture();
    let first = record_clipboard_trace(&fixture, "alpha").await;
    let second = record_clipboard_trace(&fixture, "beta").await;
    let compiled = fixture
        .call(
            "workflow.compile",
            json!({
                "trace_ids":[first,second],
                "name":"clipboard-copy",
                "description":"Write caller-provided text to the clipboard."
            }),
        )
        .await;
    assert!(compiled.ok, "{compiled:?}");
    let candidate = compiled.data.unwrap()["candidate"].clone();
    let candidate_id = candidate["id"].as_str().unwrap().to_owned();
    let candidates = fixture.call("workflow.candidates.list", json!({})).await;
    assert!(candidates.ok, "{candidates:?}");
    let candidates = candidates.data.unwrap()["candidates"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0]["id"], candidate_id);
    assert_eq!(candidates[0]["name"], "clipboard-copy");
    assert_eq!(candidates[0]["source_trace_count"], 2);
    assert_eq!(candidates[0]["static_verified"], false);
    assert_eq!(candidates[0]["successful_replays"], 0);
    assert_eq!(candidates[0]["status"], "valid");
    assert_eq!(
        candidate["recipe"]["inputs"]["step1_text"]["kind"],
        "string"
    );
    assert_eq!(
        candidate["recipe"]["steps"][0]["args"]["text"],
        json!({"$var":"/inputs/step1_text"})
    );

    let premature = fixture
        .call(
            "workflow.replay",
            json!({"candidate_id":candidate_id,"inputs":{"step1_text":"gamma"}}),
        )
        .await;
    assert_eq!(premature.error.unwrap().code, ErrorCode::Conflict);

    let verified = fixture
        .call("workflow.verify", json!({"candidate_id":candidate_id}))
        .await;
    assert!(verified.ok, "{verified:?}");
    let replayed = fixture
        .call(
            "workflow.replay",
            json!({"candidate_id":candidate_id,"inputs":{"step1_text":"gamma"}}),
        )
        .await;
    assert!(replayed.ok, "{replayed:?}");
    assert_eq!(replayed.data.unwrap()["completed"], true);
    let candidates = fixture.call("workflow.candidates.list", json!({})).await;
    assert!(candidates.ok, "{candidates:?}");
    let candidates = candidates.data.unwrap()["candidates"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(candidates[0]["static_verified"], true);
    assert_eq!(candidates[0]["successful_replays"], 1);
    assert_eq!(candidates[0]["status"], "valid");

    let promoted = fixture
        .call(
            "workflow.promote",
            json!({"candidate_id":candidate_id,"slug":"clipboard-learned"}),
        )
        .await;
    assert!(promoted.ok, "{promoted:?}");
    assert_eq!(
        promoted.data.unwrap()["capability"]["name"],
        "recipe.clipboard-learned.run"
    );

    assert!(
        fixture
            .call(
                "workflow.record.start",
                json!({"name":"promoted-run","capture_values":true}),
            )
            .await
            .ok
    );
    let executed = fixture
        .call(
            "recipe.clipboard-learned.run",
            json!({"step1_text":"delta"}),
        )
        .await;
    assert!(executed.ok, "{executed:?}");
    let captured = fixture
        .call("workflow.record.stop", json!({"successful":true}))
        .await;
    assert!(captured.ok, "{captured:?}");
    let trace = &captured.data.unwrap()["trace"];
    assert_eq!(trace["steps"].as_array().unwrap().len(), 1);
    assert_eq!(trace["steps"][0]["command"], "clipboard.write");
    assert_eq!(trace["steps"][0]["args"]["text"], "delta");

    let demoted = fixture
        .call("workflow.demote", json!({"slug":"clipboard-learned"}))
        .await;
    assert!(demoted.ok, "{demoted:?}");
    let gone = fixture
        .call(
            "capabilities.describe",
            json!({"name":"recipe.clipboard-learned.run"}),
        )
        .await;
    assert_eq!(gone.error.unwrap().code, ErrorCode::NotFound);
}

#[tokio::test]
async fn repeated_workflows_surface_suggestions_compile_and_resurface_after_new_evidence() {
    let fixture = workflow_fixture();
    record_clipboard_trace(&fixture, "alpha").await;
    record_clipboard_trace(&fixture, "beta").await;

    let early = fixture.call("workflow.suggestions.list", json!({})).await;
    assert!(early.ok, "{early:?}");
    assert!(
        early.data.unwrap()["suggestions"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    record_clipboard_trace(&fixture, "gamma").await;
    let suggestions = fixture.call("workflow.suggestions.list", json!({})).await;
    assert!(suggestions.ok, "{suggestions:?}");
    let row = suggestions.data.unwrap()["suggestions"][0].clone();
    assert_eq!(row["occurrences"], 3);
    assert_eq!(row["compile_ready_count"], 3);
    assert_eq!(row["suggested_name"], "clipboard-demo");
    assert!(
        row["varying_arguments"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| {
                value["step"] == 0 && value["pointer"] == "/text" && value["kind"] == "string"
            })
    );
    let suggestion_id = row["suggestion_id"].as_str().unwrap().to_owned();

    let compiled = fixture
        .call(
            "workflow.suggestion.compile",
            json!({"suggestion_id":suggestion_id}),
        )
        .await;
    assert!(compiled.ok, "{compiled:?}");
    let candidate = compiled.data.unwrap()["candidate"].clone();
    assert_eq!(
        candidate["recipe"]["inputs"]["step1_text"]["kind"],
        "string"
    );
    assert_eq!(candidate["source_trace_ids"].as_array().unwrap().len(), 3);

    let dismissed = fixture
        .call(
            "workflow.suggestion.dismiss",
            json!({"suggestion_id":suggestion_id}),
        )
        .await;
    assert!(dismissed.ok, "{dismissed:?}");
    let hidden = fixture.call("workflow.suggestions.list", json!({})).await;
    assert!(hidden.ok, "{hidden:?}");
    assert!(
        hidden.data.unwrap()["suggestions"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    record_clipboard_trace(&fixture, "delta").await;
    let resurfaced = fixture.call("workflow.suggestions.list", json!({})).await;
    assert!(resurfaced.ok, "{resurfaced:?}");
    let row = resurfaced.data.unwrap()["suggestions"][0].clone();
    assert_eq!(row["occurrences"], 4);
    assert_eq!(row["resurfaced"], true);

    let dismissed = fixture
        .call(
            "workflow.suggestion.dismiss",
            json!({"suggestion_id":suggestion_id,"permanent":true}),
        )
        .await;
    assert!(dismissed.ok, "{dismissed:?}");
    record_clipboard_trace(&fixture, "epsilon").await;
    let still_hidden = fixture.call("workflow.suggestions.list", json!({})).await;
    assert!(still_hidden.ok, "{still_hidden:?}");
    assert!(
        still_hidden.data.unwrap()["suggestions"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let restored = fixture
        .call(
            "workflow.suggestion.restore",
            json!({"suggestion_id":suggestion_id}),
        )
        .await;
    assert!(restored.ok, "{restored:?}");
    let visible = fixture.call("workflow.suggestions.list", json!({})).await;
    assert!(visible.ok, "{visible:?}");
    assert_eq!(visible.data.unwrap()["suggestions"][0]["occurrences"], 5);
}

#[tokio::test]
async fn learned_workflow_reacquires_ephemeral_refs_before_mutation() {
    let fixture = workflow_fixture();
    assert!(
        fixture
            .call(
                "workflow.record.start",
                json!({"name":"export-click","capture_values":true}),
            )
            .await
            .ok
    );
    let found = fixture.find("Export").await;
    let reference = found["nodes"][0]["ref"].clone();
    assert!(fixture.call("ui.invoke", json!({"ref":reference})).await.ok);
    assert_eq!(fixture.desktop.invocations(), 1);
    let stopped = fixture
        .call("workflow.record.stop", json!({"successful":true}))
        .await;
    let trace_id = stopped.data.unwrap()["trace"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let compiled = fixture
        .call(
            "workflow.compile",
            json!({"trace_ids":[trace_id],"name":"export-click"}),
        )
        .await;
    assert!(compiled.ok, "{compiled:?}");
    let candidate = compiled.data.unwrap()["candidate"].clone();
    assert_eq!(
        candidate["recipe"]["steps"][1]["args"]["ref"],
        json!({"$var":"/steps/step-1/nodes/0/ref"})
    );
    let candidate_id = candidate["id"].as_str().unwrap();
    assert!(
        fixture
            .call("workflow.verify", json!({"candidate_id":candidate_id}))
            .await
            .ok
    );
    let replayed = fixture
        .call(
            "workflow.replay",
            json!({"candidate_id":candidate_id,"inputs":{}}),
        )
        .await;
    assert!(replayed.ok, "{replayed:?}");
    assert_eq!(fixture.desktop.invocations(), 2);
}

async fn promote_clipboard_workflow(fixture: &Fixture, slug: &str) -> String {
    let first = record_clipboard_trace(fixture, "alpha").await;
    let second = record_clipboard_trace(fixture, "beta").await;
    let compiled = fixture
        .call(
            "workflow.compile",
            json!({
                "trace_ids":[first,second],
                "name":"clipboard-restart",
                "description":"Write caller-provided text to the clipboard."
            }),
        )
        .await;
    assert!(compiled.ok, "{compiled:?}");
    let candidate_id = compiled.data.unwrap()["candidate"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        fixture
            .call("workflow.verify", json!({"candidate_id":candidate_id}))
            .await
            .ok
    );
    assert!(
        fixture
            .call(
                "workflow.replay",
                json!({"candidate_id":candidate_id,"inputs":{"step1_text":"gamma"}}),
            )
            .await
            .ok
    );
    let promoted = fixture
        .call(
            "workflow.promote",
            json!({"candidate_id":candidate_id,"slug":slug}),
        )
        .await;
    assert!(promoted.ok, "{promoted:?}");
    candidate_id
}

fn restarted_workflow_broker(root: &std::path::Path) -> (Arc<Broker>, Arc<FakeDesktop>, Value) {
    let mut config = PolicyConfig {
        profile: Profile::Desktop,
        ..Default::default()
    };
    config.allow.extend(
        ["workflow.record", "workflow.manage", "clipboard.write"]
            .into_iter()
            .map(String::from),
    );
    let audit = Audit::open(
        &root.join(format!("audit-restart-{}", unique_id())),
        65536,
        2,
    )
    .unwrap();
    let desktop = Arc::new(FakeDesktop::new());
    let broker = Broker::new(
        Policy::new(config).unwrap(),
        vec![desktop.clone()],
        audit,
        Arc::new(NoApprover),
        None,
        json!({"fixture":true,"restart":true}),
        true,
    )
    .unwrap();
    let restored = broker.configure_workflows(&root.join("workflows")).unwrap();
    (broker, desktop, restored)
}

#[tokio::test]
async fn learned_workflow_survives_broker_restart_and_executes() {
    let fixture = workflow_fixture();
    promote_clipboard_workflow(&fixture, "clipboard-restart").await;

    let (broker, _desktop, restored) = restarted_workflow_broker(fixture._dir.path());
    assert!(
        restored["restored"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "recipe.clipboard-restart.run")
    );
    assert!(restored["stale"].as_array().unwrap().is_empty());

    let executed = broker
        .execute(
            unique_id(),
            unique_id(),
            ExecuteRequest {
                command: "recipe.clipboard-restart.run".into(),
                args: json!({"step1_text":"after-restart"}),
                dry_run: false,
                backend: None,
            },
            CancellationToken::new(),
        )
        .await;
    assert!(executed.ok, "{executed:?}");
    assert_eq!(executed.data.unwrap()["completed"], true);
}

#[tokio::test]
async fn descriptor_drift_prevents_restoring_persisted_workflow() {
    let fixture = workflow_fixture();
    promote_clipboard_workflow(&fixture, "clipboard-drift").await;

    let store = fixture._dir.path().join("workflows/workflows.json");
    let mut persisted: Value = serde_json::from_slice(&std::fs::read(&store).unwrap()).unwrap();
    let promotion = &mut persisted["promotions"].as_array_mut().unwrap()[0];
    promotion["candidate"]["source_descriptor_sha256"]["clipboard.write"] =
        Value::String("0".repeat(64));
    std::fs::write(&store, serde_json::to_vec_pretty(&persisted).unwrap()).unwrap();

    let (broker, _desktop, restored) = restarted_workflow_broker(fixture._dir.path());
    assert!(restored["restored"].as_array().unwrap().is_empty());
    assert_eq!(restored["stale"].as_array().unwrap().len(), 1);

    let described = broker
        .execute(
            unique_id(),
            unique_id(),
            ExecuteRequest {
                command: "capabilities.describe".into(),
                args: json!({"name":"recipe.clipboard-drift.run"}),
                dry_run: false,
                backend: None,
            },
            CancellationToken::new(),
        )
        .await;
    assert_eq!(described.error.unwrap().code, ErrorCode::NotFound);
}
