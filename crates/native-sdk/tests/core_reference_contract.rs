//! Actual pinned Core Broker and SDK models through an in-process test provider.
//! The provider replaces Driver Host. These tests do not open IPC, launch a
//! daemon, grant host permissions, or establish native Host acceptance.
#![cfg(unix)]

#[allow(dead_code)]
#[path = "../../../examples/native/scene.rs"]
mod scene;
#[allow(dead_code)]
#[path = "../../../examples/native/table.rs"]
mod table;

use semwright_backend_api::{Context, ProvidedCapability, Provider, ProviderInterfaces, feature};
use semwright_core::{Broker, NoApprover, audit::Audit};
use semwright_native_sdk::{
    CommandDescriptor, Driver, Error, ErrorCode, Model, NativeApp, NativeTarget, Result,
    SDK_VERSION, Value, async_trait, descriptor_digest, json, sha256,
};
use semwright_policy::{Policy, PolicyConfig};
use semwright_types::{Envelope, ExecuteRequest, Feature, ProviderIdentity, SourceKind, unique_id};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

struct OwnedProvider<M: Model> {
    identity: ProviderIdentity,
    app: Mutex<NativeApp<M>>,
    descriptors: Vec<CommandDescriptor>,
    executions: AtomicUsize,
    validations: AtomicUsize,
}

#[async_trait]
impl<M: Model> Provider for OwnedProvider<M> {
    fn identity(&self) -> &ProviderIdentity {
        &self.identity
    }
    fn supports(&self, name: &str) -> bool {
        self.descriptors
            .iter()
            .any(|descriptor| descriptor.name == name)
    }
    async fn capabilities(&self) -> Result<Vec<ProvidedCapability>> {
        Ok(self.descriptors.iter().cloned().map(Into::into).collect())
    }
    async fn probe(&self) -> Vec<Feature> {
        self.descriptors
            .iter()
            .map(|descriptor| {
                feature(
                    &self.identity.id,
                    &descriptor.name,
                    true,
                    "owned in-process contract",
                    "",
                )
            })
            .collect()
    }
    fn interfaces(&self) -> ProviderInterfaces {
        ProviderInterfaces {
            dynamic_capabilities: true,
            native_refs: true,
            health: true,
            ..ProviderInterfaces::default()
        }
    }
    async fn validate(&self, target: &NativeTarget) -> Result<()> {
        self.validations.fetch_add(1, Ordering::SeqCst);
        self.app.lock().await.validate_native_ref(target).await
    }
    async fn execute(
        &self,
        context: &Context,
        descriptor: &CommandDescriptor,
        args: &Value,
    ) -> Result<Value> {
        context.check_cancelled()?;
        let expected = self
            .descriptors
            .iter()
            .find(|row| row.name == descriptor.name)
            .ok_or_else(|| Error::new(ErrorCode::Unsupported, "Unknown owned descriptor"))?;
        let digest = descriptor_digest(descriptor)?;
        if descriptor_digest(expected)? != digest {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Owned descriptor changed",
            ));
        }
        self.executions.fetch_add(1, Ordering::SeqCst);
        let mut app = self.app.lock().await;
        let mut child_args = args.clone();
        let target = child_args
            .as_object_mut()
            .and_then(|row| row.remove("_target"));
        if child_args.get("ref").is_some() && target.is_none() {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Broker ref has no resolved target",
            ));
        }
        if let Some(target) = target {
            app.validate_execution_target(&child_args, &serde_json::from_value(target)?)?;
        }
        let operation = descriptor
            .name
            .strip_prefix(&self.identity.namespace)
            .ok_or_else(|| Error::new(ErrorCode::PermissionDenied, "Wrong provider namespace"))?;
        if matches!(
            operation,
            "inspect" | "interfaces" | "events" | "operation.get"
        ) {
            // Core already resolved the outer reference above. This fixture
            // cannot construct DriverExecutionContext. Framed tests check it.
            child_args.as_object_mut().unwrap().remove("ref");
            app.execute(&descriptor.name, &digest, child_args).await
        } else {
            // Keep the original mutation envelope, including its Broker ref,
            // in the SDK journal. This is the public owner-side model API.
            app.apply(operation, &child_args, || {
                context.cancellation.is_cancelled()
            })
        }
    }
}

struct Fixture<M: Model> {
    root: tempfile::TempDir,
    broker: Arc<Broker>,
    provider: Arc<OwnedProvider<M>>,
    session: String,
    audit: Arc<Audit>,
}

impl<M: Model> Fixture<M> {
    async fn new(model: M, granted: bool, confirm: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let app = NativeApp::create(root.path().join("document"), model.clone()).unwrap();
        let identity =
            ProviderIdentity::external(SourceKind::Driver, model.id(), SDK_VERSION).unwrap();
        let descriptors = app
            .capabilities_value()
            .into_iter()
            .map(|cap| cap.descriptor)
            .collect();
        let provider = Arc::new(OwnedProvider {
            identity,
            app: Mutex::new(app),
            descriptors,
            executions: AtomicUsize::new(0),
            validations: AtomicUsize::new(0),
        });
        let audit = Audit::open(&root.path().join("audit"), 65536, 2).unwrap();
        let policy = Policy::new(PolicyConfig {
            allow: if granted {
                [provider.identity.id.clone()].into()
            } else {
                Default::default()
            },
            confirm_mutations: confirm,
            ..PolicyConfig::default()
        })
        .unwrap();
        let broker = Broker::new(
            policy,
            vec![],
            audit.clone(),
            Arc::new(NoApprover),
            None,
            json!({}),
            false,
        )
        .unwrap();
        broker.mount_provider(provider.clone()).await.unwrap();
        Self {
            root,
            broker,
            provider,
            session: unique_id(),
            audit,
        }
    }
    async fn call_in(&self, session: &str, operation: &str, args: Value) -> Envelope {
        self.broker
            .clone()
            .execute(
                session.into(),
                unique_id(),
                ExecuteRequest {
                    command: format!("{}{operation}", self.provider.identity.namespace),
                    args,
                    dry_run: false,
                    backend: None,
                },
                CancellationToken::new(),
            )
            .await
    }
    async fn call(&self, operation: &str, args: Value) -> Envelope {
        self.call_in(&self.session, operation, args).await
    }
    async fn view(&self, workspace: Option<&str>) -> Value {
        let args = workspace.map_or_else(|| json!({}), |id| json!({"workspace_id": id}));
        data(self.call("inspect", args).await)
    }
}

fn data(result: Envelope) -> Value {
    assert!(result.ok, "{result:?}");
    assert!(!result.execution.dry_run);
    result
        .data
        .expect("Successful owned command has its actual result")
}
fn mutation(view: &Value, key: &str, parameters: Value, workspace: Option<&str>) -> Value {
    let mut args = json!({"ref": view["ref"], "expected_revision": view["revision"],
        "expected_generation": view["generation"], "operation_key": key, "parameters": parameters});
    if let Some(id) = workspace {
        args["workspace_id"] = json!(id);
    }
    args
}

async fn journey<M: Model>(model: M, operation: &str, parameters: Value, output_fragment: &str) {
    let fixture = Fixture::new(model, true, false).await;
    let original = std::fs::read(fixture.root.path().join("document/document.json")).unwrap();
    let root = fixture.view(None).await;
    assert!(root["ref"].as_str().unwrap().starts_with("native:"));
    assert!(root["native_target"].is_object());
    let before = fixture.provider.executions.load(Ordering::SeqCst);
    let foreign = fixture
        .call_in(&unique_id(), "inspect", json!({"ref": root["ref"]}))
        .await;
    assert!(!foreign.ok);
    assert_eq!(fixture.provider.executions.load(Ordering::SeqCst), before);

    data(
        fixture
            .call(
                "fork",
                mutation(&root, "copy_01", json!({"workspace_id":"isolated"}), None),
            )
            .await,
    );
    let child = fixture.view(Some("isolated")).await;
    assert_ne!(child["resource_id"], root["resource_id"]);
    assert_ne!(child["ref"], root["ref"]);

    let mut mismatched = mutation(&child, "wrong_target", parameters.clone(), Some("isolated"));
    mismatched["ref"] = root["ref"].clone();
    let refused = fixture.call(operation, mismatched).await;
    assert!(!refused.ok);
    assert_eq!(refused.error.unwrap().code, ErrorCode::StaleReference);
    assert_eq!(
        fixture.view(Some("isolated")).await["revision"],
        child["revision"]
    );

    let original_request = mutation(&child, "edit_01", parameters, Some("isolated"));
    let result = data(fixture.call(operation, original_request.clone()).await);
    // Treat the successful response as lost; recovery must use observation.
    let calls_after_edit = fixture.provider.executions.load(Ordering::SeqCst);
    let stale = fixture.call(operation, original_request.clone()).await;
    assert!(!stale.ok);
    assert_eq!(stale.error.unwrap().code, ErrorCode::StaleReference);
    assert_eq!(
        fixture.provider.executions.load(Ordering::SeqCst),
        calls_after_edit
    );
    let current = fixture.view(Some("isolated")).await;
    let recovered = data(
        fixture
            .call(
                "operation.get",
                json!({"ref": current["ref"],
        "workspace_id":"isolated", "operation":operation, "operation_key":"edit_01",
        "request":original_request}),
            )
            .await,
    );
    assert_eq!(recovered["state"], "RECORDED");
    assert_eq!(recovered["result"], result);
    assert_eq!(recovered["current_authority"], false);
    assert_eq!(recovered["replay_allowed"], false);

    let exported = data(
        fixture
            .call(
                "export",
                mutation(
                    &current,
                    "export_01",
                    json!({"output_namespace":"contract", "slot":"actual"}),
                    Some("isolated"),
                ),
            )
            .await,
    );
    let artifact = &exported["artifact"];
    let bytes = std::fs::read(
        fixture
            .root
            .path()
            .join("document/outputs")
            .join(artifact["path"].as_str().unwrap()),
    )
    .unwrap();
    assert_eq!(sha256(&bytes), artifact["sha256"]);
    assert_eq!(bytes.len().to_string(), artifact["bytes"]);
    assert!(String::from_utf8(bytes).unwrap().contains(output_fragment));
    assert_eq!(
        std::fs::read(fixture.root.path().join("document/document.json")).unwrap(),
        original
    );
    assert!(fixture.provider.validations.load(Ordering::SeqCst) >= 5);
    assert!(
        fixture.audit.tail(64).unwrap().iter().any(|row| row
            .provenance
            .as_ref()
            .is_some_and(|pin| pin.provider == fixture.provider.identity.id
                && pin.provider_generation.is_some()))
    );
    fixture.broker.shutdown().await;
}

#[tokio::test]
async fn scene_reference_copy_edit_recovery_and_export_use_actual_broker_contracts() {
    journey(
        scene::Scene,
        "set-object",
        json!({"object_id":"cube", "color":"#123456"}),
        "#123456",
    )
    .await;
}

#[tokio::test]
async fn table_reference_copy_edit_recovery_and_export_use_actual_broker_contracts() {
    journey(
        table::Table,
        "set-cell",
        json!({"cell":"A1", "value":50}),
        "A3,70",
    )
    .await;
}

#[tokio::test]
async fn table_formula_schema_and_cycle_refusal_pass_through_actual_broker() {
    let fixture = Fixture::new(table::Table, true, false).await;
    let initial = fixture.view(None).await;
    data(
        fixture
            .call(
                "set-cell",
                mutation(
                    &initial,
                    "sum_01",
                    json!({"cell":"A1", "value":{"sum":["A2"]}}),
                    None,
                ),
            )
            .await,
    );
    let current = fixture.view(None).await;
    let rows = current["projection"]["rows"].as_array().unwrap();
    assert_eq!(
        rows.iter().find(|row| row["cell"] == "A3").unwrap()["value"],
        40.0
    );
    let before = std::fs::read(fixture.root.path().join("document/document.json")).unwrap();
    let cycle = fixture
        .call(
            "set-cell",
            mutation(
                &current,
                "cycle_01",
                json!({"cell":"A2", "value":{"sum":["A3"]}}),
                None,
            ),
        )
        .await;
    assert!(!cycle.ok);
    assert_eq!(fixture.view(None).await["revision"], current["revision"]);
    assert_eq!(
        std::fs::read(fixture.root.path().join("document/document.json")).unwrap(),
        before
    );
    fixture.broker.shutdown().await;
}

#[tokio::test]
async fn actual_broker_denies_missing_capability_and_unapproved_mutation_before_sdk_work() {
    let denied = Fixture::new(scene::Scene, false, false).await;
    assert!(!denied.call("inspect", json!({})).await.ok);
    assert_eq!(denied.provider.executions.load(Ordering::SeqCst), 0);
    denied.broker.shutdown().await;
    let confirmation = Fixture::new(table::Table, true, true).await;
    let view = confirmation.view(None).await;
    let count = confirmation.provider.executions.load(Ordering::SeqCst);
    let result = confirmation
        .call(
            "set-cell",
            mutation(&view, "unapproved", json!({"cell":"A1","value":50}), None),
        )
        .await;
    assert!(!result.ok);
    assert_eq!(result.error.unwrap().code, ErrorCode::ConsentRequired);
    assert_eq!(
        confirmation.provider.executions.load(Ordering::SeqCst),
        count
    );
    confirmation.broker.shutdown().await;
}

#[tokio::test]
async fn owned_adapter_refuses_a_reference_without_a_core_resolved_target() {
    let fixture = Fixture::new(scene::Scene, true, false).await;
    let view = fixture.view(None).await;
    let request = mutation(
        &view,
        "unresolved",
        json!({"object_id":"cube", "color":"#123456"}),
        None,
    );
    let descriptor = fixture
        .provider
        .descriptors
        .iter()
        .find(|row| row.name.ends_with(".set-object"))
        .unwrap();
    let error = fixture
        .provider
        .execute(
            &Context {
                session: fixture.session.clone(),
                request_id: unique_id(),
                cancellation: CancellationToken::new(),
            },
            descriptor,
            &request,
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleReference);
    assert_eq!(fixture.view(None).await["revision"], view["revision"]);
    fixture.broker.shutdown().await;
}
