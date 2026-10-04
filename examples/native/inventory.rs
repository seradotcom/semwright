//! App-owned SQLite inventory reached through the public Native SDK TypeScript bridge.
//! The application schema/transactions live in examples/native-inventory/application.ts.
use semwright_native_sdk::cooperation::{
    Application, CancellationSemantics, CommitSemantics, OperationContract, RetrySemantics,
    TargetRequirement, UndoSemantics,
};
use semwright_native_sdk::process_bridge::{NodeBridge, NodeBridgeConfig};
use semwright_native_sdk::{
    CommandDescriptor, Idempotency, NativeDriver, Result, Risk, json, serve,
};
use std::time::Duration;

const APP_ID: &str = "native-inventory";
const DRIVER_VERSION: &str = env!("CARGO_PKG_VERSION");
const BUNDLE_SHA256: Option<&str> = option_env!("SEMWRIGHT_NATIVE_INVENTORY_BUNDLE_SHA256");

fn request_schema() -> semwright_native_sdk::Value {
    json!({
        "type":"object",
        "properties":{
            "resource":{"type":"string","minLength":1,"maxLength":512},
            "epoch":{"type":"integer","minimum":0,"maximum":9007199254740991u64},
            "key":{"type":"string","minLength":1,"maxLength":128},
            "request_sha256":{"type":"string","pattern":"^[0-9a-f]{64}$"}
        },
        "required":["resource","epoch","key","request_sha256"],
        "additionalProperties":false
    })
}

fn mutation_input(parameters: semwright_native_sdk::Value) -> semwright_native_sdk::Value {
    json!({
        "type":"object",
        "properties":{
            "ref":{"type":"string"},
            "request":request_schema(),
            "parameters":parameters
        },
        "required":["ref","request","parameters"],
        "additionalProperties":false
    })
}

fn contract(
    suffix: &str,
    description: &str,
    risk: Risk,
    parameters: semwright_native_sdk::Value,
) -> OperationContract {
    OperationContract {
        descriptor: CommandDescriptor {
            name: format!("driver.{APP_ID}.{suffix}"),
            version: DRIVER_VERSION.into(),
            description: description.into(),
            input_schema: mutation_input(parameters),
            output_schema: json!({"type":"object"}),
            requires: vec![format!("driver:{APP_ID}")],
            risk,
            idempotency: Idempotency::Idempotent,
            timeout_ms: 10_000,
            dry_run: false,
            interactive_consent: false,
            backends: vec![format!("driver:{APP_ID}")],
        },
        target: TargetRequirement::ObservedResource,
        commit: CommitSemantics::ApplicationTransaction,
        retry: RetrySemantics::DurableRequestKey,
        undo: UndoSemantics::None,
        cancellation: CancellationSemantics::BeforeEffects,
        atomic_revision_cas: true,
    }
}

fn events_contract() -> OperationContract {
    OperationContract {
        descriptor: CommandDescriptor {
            name: format!("driver.{APP_ID}.events"),
            version: DRIVER_VERSION.into(),
            description:
                "Poll bounded application event hints; resynchronize when history is incomplete"
                    .into(),
            input_schema: json!({
                "type":"object",
                "properties":{
                    "ref":{"type":"string"},
                    "resource":{"type":"string","minLength":1,"maxLength":512},
                    "cursor":{"type":["string","null"],"maxLength":1024},
                    "limit":{"type":"integer","minimum":1,"maximum":256}
                },
                "required":["ref","resource","limit"],
                "additionalProperties":false
            }),
            output_schema: json!({
                "type":"object",
                "properties":{
                    "generation":{"type":"string"},
                    "next_cursor":{"type":"string"},
                    "resync_required":{"type":"boolean"},
                    "historical_hints_only":{"const":true},
                    "events":{"type":"array","maxItems":256}
                },
                "required":["generation","next_cursor","resync_required","historical_hints_only","events"],
                "additionalProperties":false
            }),
            requires: vec![format!("driver:{APP_ID}")],
            risk: Risk::ReadOnly,
            idempotency: Idempotency::ReadOnly,
            timeout_ms: 10_000,
            dry_run: false,
            interactive_consent: false,
            backends: vec![format!("driver:{APP_ID}")],
        },
        target: TargetRequirement::ObservedResource,
        commit: CommitSemantics::ReadOnly,
        retry: RetrySemantics::StateIdempotent,
        undo: UndoSemantics::None,
        cancellation: CancellationSemantics::BeforeEffects,
        atomic_revision_cas: false,
    }
}

fn object(
    properties: semwright_native_sdk::Value,
    required: &[&str],
) -> semwright_native_sdk::Value {
    json!({
        "type":"object",
        "properties":properties,
        "required":required,
        "additionalProperties":false
    })
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let bundle_sha256 = BUNDLE_SHA256.ok_or_else(|| {
        semwright_native_sdk::Error::invalid(
            "Native inventory bundle SHA-256 must be pinned when building the runnable bridge",
        )
    })?;
    let bridge = NodeBridge::new(NodeBridgeConfig {
        tool: "node".into(),
        bundle_mount: "native-runtime".into(),
        bundle_file: "inventory.cjs".into(),
        bundle_sha256: bundle_sha256.into(),
        data_mount: "inventory-data".into(),
        output_mount: None,
        timeout: Duration::from_secs(8),
    })?;

    let mut app = Application::new(APP_ID, DRIVER_VERSION)?
        .require_host_tools()
        .with_observer(bridge.clone())
        .with_recovery(bridge.clone());

    let specs = [
        contract(
            "reserve",
            "Reserve application-owned stock using the exact observed revision and durable request key",
            Risk::Mutating,
            object(
                json!({
                    "sku":{"type":"string","minLength":1,"maxLength":128},
                    "quantity":{"type":"integer","minimum":1,"maximum":10000}
                }),
                &["sku", "quantity"],
            ),
        ),
        contract(
            "release",
            "Release an existing reservation in the application transaction",
            Risk::Mutating,
            object(
                json!({
                    "sku":{"type":"string","minLength":1,"maxLength":128},
                    "quantity":{"type":"integer","minimum":1,"maximum":10000}
                }),
                &["sku", "quantity"],
            ),
        ),
        contract(
            "snapshot",
            "Capture immutable application content without transferring model authority to the SDK",
            Risk::Mutating,
            object(json!({}), &[]),
        ),
        contract(
            "fork",
            "Create a fresh-identity application workspace from an immutable snapshot",
            Risk::Mutating,
            object(
                json!({"snapshot_id":{"type":"string","pattern":"^[0-9a-f]{64}$"}}),
                &["snapshot_id"],
            ),
        ),
        contract(
            "restore",
            "Replace one workspace from a snapshot and rotate its application generation",
            Risk::Destructive,
            object(
                json!({"snapshot_id":{"type":"string","pattern":"^[0-9a-f]{64}$"}}),
                &["snapshot_id"],
            ),
        ),
    ];
    for operation in specs {
        let command = operation.descriptor.name.clone();
        app = app.register(operation, bridge.operation(command))?;
    }
    let publication = contract(
        "publish-private",
        "Publish reviewed immutable content only to the configured private application destination",
        Risk::Mutating,
        object(
            json!({
                "snapshot_id":{"type":"string","pattern":"^[0-9a-f]{64}$"},
                "candidate_sha256":{"type":"string","pattern":"^[0-9a-f]{64}$"}
            }),
            &["snapshot_id", "candidate_sha256"],
        ),
    );
    app = app.register(publication, bridge.publication_operation())?;
    let events = events_contract();
    let command = events.descriptor.name.clone();
    app = app.register(events, bridge.operation(command))?;

    serve(NativeDriver::new(app)?).await
}
