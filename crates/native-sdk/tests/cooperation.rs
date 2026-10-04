//! Public-contract consumers; no private module access and no file-backed model.
use semwright_native_sdk::cooperation::*;
use semwright_native_sdk::{
    CommandDescriptor, Error, ErrorCode, Idempotency, Result, Risk, Value, async_trait, json,
};
use std::sync::Arc;

fn version(token: &str) -> ResourceVersion {
    ResourceVersion {
        resource: "inventory".into(),
        generation: "app-instance".into(),
        revision: RevisionToken::new(token).unwrap(),
    }
}
fn query() -> Query {
    Query {
        resource: "inventory".into(),
        scope: "stock".into(),
        limit: 2,
        cursor: None,
    }
}
fn page() -> ObservationPage {
    ObservationPage {
        version: version("db:1844674407370955161600001"),
        scope: "stock".into(),
        items: vec![json!({"sku":"part-a","stock":7})],
        next: None,
        complete: true,
    }
}
fn identity() -> RequestIdentity {
    RequestIdentity {
        resource: "inventory".into(),
        epoch: 4,
        key: "request-a".into(),
        request_sha256: "a".repeat(64),
    }
}
fn contract() -> OperationContract {
    OperationContract {
        descriptor: CommandDescriptor {
            name: "driver.inventory.reserve".into(),
            version: "1.0.0".into(),
            description: "Reserve in the application's transaction".into(),
            input_schema: json!({"type":"object","properties":{"ref":{"type":"string"}},"required":["ref"],"additionalProperties":false}),
            output_schema: json!({"type":"object"}),
            requires: vec!["driver:inventory".into()],
            risk: Risk::Mutating,
            idempotency: Idempotency::NonIdempotent,
            timeout_ms: 5_000,
            dry_run: false,
            interactive_consent: false,
            backends: vec!["driver:inventory".into()],
        },
        target: TargetRequirement::ObservedResource,
        commit: CommitSemantics::ApplicationTransaction,
        retry: RetrySemantics::Never,
        undo: UndoSemantics::None,
        cancellation: CancellationSemantics::BeforeEffects,
        atomic_revision_cas: true,
    }
}
struct ReadOnlyInventory;
#[async_trait]
impl ObservationProvider for ReadOnlyInventory {
    async fn observe(&self, request: &Query, _: &CallContext) -> Result<ObservationPage> {
        request.validate()?;
        if request.resource != "inventory" || request.scope != "stock" {
            return Err(Error::new(ErrorCode::NotFound, "Unknown resource"));
        }
        Ok(page())
    }
}
struct RejectingHandler;
#[async_trait]
impl OperationHandler for RejectingHandler {
    async fn invoke(&self, _: Value, _: &CallContext) -> Completion {
        Completion::NotApplied(Error::new(
            ErrorCode::PermissionDenied,
            "Application denied operation",
        ))
    }
}

#[test]
fn opaque_revision_is_not_a_lossy_numeric_counter() {
    let a = version("1844674407370955161600001");
    let b = version("1844674407370955161600002");
    assert_ne!(a.revision, b.revision);
    assert_ne!(
        version_fingerprint(&a).unwrap(),
        version_fingerprint(&b).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&a.revision).unwrap(),
        json!("1844674407370955161600001")
    );
}
#[test]
fn revision_deserialization_preserves_validation() {
    for value in [
        json!(""),
        json!("x".repeat(513)),
        json!("line\nbreak"),
        json!(123),
    ] {
        assert!(serde_json::from_value::<RevisionToken>(value).is_err());
    }
}
#[test]
fn generation_changes_the_complete_observation_binding() {
    let a = version("same");
    let mut b = a.clone();
    b.generation = "reopened".into();
    assert_ne!(
        version_fingerprint(&a).unwrap(),
        version_fingerprint(&b).unwrap()
    );
}
#[test]
fn nonreversible_nonidempotent_operation_is_supported() {
    contract().validate("inventory").unwrap();
}
#[test]
fn reversible_claim_requires_an_explicit_undo_contract() {
    let mut value = contract();
    value.descriptor.risk = Risk::MutatingReversible;
    assert!(value.validate("inventory").is_err());
    value.undo = UndoSemantics::ApplicationOperation {
        command: "driver.inventory.release".into(),
    };
    value.validate("inventory").unwrap();
}
#[test]
fn idempotency_cannot_be_inferred_from_mutation_registration() {
    let mut value = contract();
    value.descriptor.idempotency = Idempotency::Idempotent;
    assert!(value.validate("inventory").is_err());
    value.retry = RetrySemantics::DurableRequestKey;
    value.validate("inventory").unwrap();
}
#[test]
fn atomic_cas_requires_the_actual_application_transaction() {
    let mut value = contract();
    value.commit = CommitSemantics::NonAtomic;
    assert!(value.validate("inventory").is_err());
    value.atomic_revision_cas = false;
    value.validate("inventory").unwrap();
}
#[test]
fn canonical_provider_scope_cannot_be_replaced() {
    let mut value = contract();
    value.descriptor.backends = vec!["driver:another-app".into()];
    assert!(value.validate("inventory").is_err());
}
#[test]
fn app_registration_is_optional_and_does_not_require_model_storage() {
    let app = Application::new("inventory", "1.0.0")
        .unwrap()
        .with_observer(Arc::new(ReadOnlyInventory));
    assert!(app.has_observer());
    assert!(!app.has_recovery());
    assert!(app.contracts().is_empty());
}
#[test]
fn duplicate_operation_registration_is_rejected() {
    let app = Application::new("inventory", "1.0.0")
        .unwrap()
        .register(contract(), Arc::new(RejectingHandler))
        .unwrap();
    assert!(app.handler("driver.inventory.reserve").is_some());
    assert!(
        app.register(contract(), Arc::new(RejectingHandler))
            .is_err()
    );
}
#[test]
fn revision_bound_page_rejects_a_manual_edit_between_pages() {
    let mut request = query();
    request.cursor = Some(PageCursor {
        version: version("old"),
        scope: "stock".into(),
        token: "next".into(),
    });
    assert_eq!(
        page().validate_for(&request).unwrap_err().code,
        ErrorCode::StaleReference
    );
}
#[test]
fn cursor_cannot_cross_resource_or_query_scope() {
    let mut request = query();
    request.cursor = Some(PageCursor {
        version: version("old"),
        scope: "other".into(),
        token: "next".into(),
    });
    assert_eq!(
        request.validate().unwrap_err().code,
        ErrorCode::StaleReference
    );
}
#[test]
fn complete_partial_and_nonprogressing_pages_are_distinct() {
    page().validate_for(&query()).unwrap();
    let mut partial = page();
    partial.complete = false;
    partial.validate_for(&query()).unwrap();
    let cursor = PageCursor {
        version: partial.version.clone(),
        scope: partial.scope.clone(),
        token: "same".into(),
    };
    partial.next = Some(cursor.clone());
    let mut request = query();
    request.cursor = Some(cursor);
    assert!(partial.validate_for(&request).is_err());
    partial.complete = true;
    assert!(partial.validate_for(&query()).is_err());
}
#[test]
fn shared_numeric_bound_rejects_inexact_javascript_integers() {
    validate_value(&json!(MAX_SAFE_INTEGER)).unwrap();
    validate_value(&json!(-9_007_199_254_740_991i64)).unwrap();
    assert!(validate_value(&json!(MAX_SAFE_INTEGER + 1)).is_err());
    assert!(validate_value(&json!(-9_007_199_254_740_992i64)).is_err());
    validate_value(&json!({"large_revision":"184467440737095516160001","fraction":0.25})).unwrap();
}
#[test]
fn structural_and_byte_budgets_are_enforced() {
    assert!(validate_value(&json!("x".repeat(MAX_VALUE_BYTES))).is_err());
    let mut deep = json!(null);
    for _ in 0..34 {
        deep = json!([deep]);
    }
    assert!(validate_value(&deep).is_err());
}
#[test]
fn missing_historical_receipt_stays_unknown() {
    RecoveryRecord::OutcomeUnknown {
        identity: identity(),
    }
    .validate_for(&identity())
    .unwrap();
    let mut changed = identity();
    changed.request_sha256 = "b".repeat(64);
    assert_eq!(
        RecoveryRecord::Pending { identity: changed }
            .validate_for(&identity())
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
}
#[test]
fn retention_requires_a_closed_epoch_not_implicit_key_reuse() {
    RecoveryRecord::RetentionExpired {
        identity: identity(),
        current_epoch: 5,
    }
    .validate_for(&identity())
    .unwrap();
    assert!(
        RecoveryRecord::RetentionExpired {
            identity: identity(),
            current_epoch: 4
        }
        .validate_for(&identity())
        .is_err()
    );
}
#[test]
fn uncertain_completion_never_becomes_known_no_effects() {
    let error = Completion::Uncertain(Error::new(ErrorCode::Timeout, "Reply lost"))
        .into_result()
        .unwrap_err();
    assert!(!error.outcome_known);
    let error = Completion::NotApplied(Error::new(ErrorCode::Conflict, "CAS rejected"))
        .into_result()
        .unwrap_err();
    assert!(error.outcome_known);
}
#[tokio::test]
async fn application_local_consumer_can_read_without_driver_runtime_or_document() {
    let provider = ReadOnlyInventory;
    let result = provider
        .observe(
            &query(),
            &CallContext::application_local("ui-read").unwrap(),
        )
        .await
        .unwrap();
    result.validate_for(&query()).unwrap();
    assert_eq!(result.items[0]["stock"], 7);
}
#[cfg(feature = "driver")]
#[tokio::test]
async fn real_driver_adapter_exposes_only_registered_read_only_interface() {
    use semwright_native_sdk::{Driver, NativeDriver};
    let app = Application::new("inventory", "1.0.0")
        .unwrap()
        .with_observer(Arc::new(ReadOnlyInventory));
    let mut driver = NativeDriver::new(app).unwrap();
    let capabilities = driver.capabilities().await.unwrap();
    assert_eq!(capabilities.len(), 1);
    assert_eq!(capabilities[0].descriptor.name, "driver.inventory.observe");
    assert_eq!(capabilities[0].descriptor.risk, Risk::ReadOnly);
    assert!(!driver.interfaces().events);
    assert!(!driver.interfaces().host_tools);

    let tool_app = Application::new("inventory", "1.0.0")
        .unwrap()
        .require_host_tools()
        .with_observer(Arc::new(ReadOnlyInventory));
    let tool_driver = NativeDriver::new(tool_app).unwrap();
    assert!(tool_driver.interfaces().host_tools);
}
#[cfg(feature = "driver")]
#[tokio::test]
async fn direct_execution_cannot_invent_a_driver_host_context() {
    use semwright_native_sdk::{Driver, NativeDriver};
    let app = Application::new("inventory", "1.0.0")
        .unwrap()
        .with_observer(Arc::new(ReadOnlyInventory));
    let mut driver = NativeDriver::new(app).unwrap();
    assert_eq!(
        driver
            .execute("driver.inventory.observe", "ignored", json!({}))
            .await
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );
    assert!(
        CallContext::application_local("caller")
            .unwrap()
            .driver_context()
            .is_err()
    );
}

#[test]
fn shared_rust_typescript_golden_vectors_match_exactly() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../contracts/native/cooperation-vectors.json");
    let vectors: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(vectors["schema_version"], 1);
    let resource: ResourceVersion =
        serde_json::from_value(vectors["resource_version"].clone()).unwrap();
    resource.validate().unwrap();
    assert_eq!(
        serde_json::to_value(&resource).unwrap(),
        vectors["resource_version"]
    );
    let digest = exact_request_digest(
        vectors["request_digest"]["domain"].as_str().unwrap(),
        &vectors["request_digest"]["value"],
    )
    .unwrap();
    assert_eq!(digest, vectors["request_digest"]["sha256"]);
    validate_value(&vectors["safe_integer"]).unwrap();
    assert!(
        validate_value(&json!(
            vectors["large_integer_string"]
                .as_str()
                .unwrap()
                .parse::<u64>()
                .unwrap()
        ))
        .is_err()
    );
    assert!(exact_request_digest("app/1", &json!({"value":0.25})).is_err());
}

#[test]
fn private_publication_binds_candidate_request_and_destination() {
    let candidate = PrivatePublication {
        candidate_sha256: "c".repeat(64),
        destination: version("destination-revision"),
        request: identity(),
    };
    candidate.validate().unwrap();

    let mut wrong_resource = candidate.clone();
    wrong_resource.request.resource = "another-resource".into();
    assert_eq!(
        wrong_resource.validate().unwrap_err().code,
        ErrorCode::InvalidArgument
    );

    let mut wrong_digest = candidate;
    wrong_digest.candidate_sha256 = "C".repeat(64);
    assert_eq!(
        wrong_digest.validate().unwrap_err().code,
        ErrorCode::InvalidArgument
    );
}
