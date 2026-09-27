use async_trait::async_trait;
use semwright_driver_sdk::{
    Capability, Driver, DriverExecutionContext, DriverInterfaces, descriptor_digest, serve,
    workspace_mount,
};
use semwright_types::{
    CommandDescriptor, Error, ErrorCode, Idempotency, JobArtifact, JobProgress, Result, Risk,
};
use serde_json::{Value, json};

fn capability() -> Capability {
    Capability {
        descriptor: CommandDescriptor {
            name: "driver.fixture.ping".into(),
            version: "1".into(),
            description: "Return a deterministic response from the driver conformance fixture"
                .into(),
            input_schema: json!({
                "type":"object",
                "additionalProperties":false
            }),
            output_schema: json!({
                "type":"object",
                "properties":{"ok":{"const":true}},
                "required":["ok"],
                "additionalProperties":false
            }),
            requires: vec!["driver:fixture".into()],
            risk: Risk::ReadOnly,
            idempotency: Idempotency::ReadOnly,
            timeout_ms: 2_000,
            dry_run: true,
            interactive_consent: false,
            backends: vec!["driver:fixture".into()],
        },
        aliases: vec!["ping".into()],
        tags: vec!["fixture".into(), "conformance".into()],
        object_types: vec![],
    }
}

fn mount_capability() -> Capability {
    Capability {
        descriptor: CommandDescriptor {
            name: "driver.fixture.mount_probe".into(),
            version: "1".into(),
            description: "Read the owner-granted fixture workspace and probe write authority"
                .into(),
            input_schema: json!({
                "type":"object",
                "additionalProperties":false
            }),
            output_schema: json!({
                "type":"object",
                "properties":{
                    "read":{"type":"string"},
                    "write_ok":{"type":"boolean"}
                },
                "required":["read","write_ok"],
                "additionalProperties":false
            }),
            requires: vec!["driver:fixture".into()],
            risk: Risk::MutatingReversible,
            idempotency: Idempotency::Idempotent,
            timeout_ms: 2_000,
            dry_run: false,
            interactive_consent: false,
            backends: vec!["driver:fixture".into()],
        },
        aliases: vec!["mount_probe".into()],
        tags: vec!["fixture".into(), "conformance".into(), "filesystem".into()],
        object_types: vec![],
    }
}

fn long_capability() -> Capability {
    Capability {
        descriptor: CommandDescriptor {
            name: "driver.fixture.long".into(),
            version: "1".into(),
            description: "Exercise protocol-v2 progress, events and cooperative cancellation"
                .into(),
            input_schema: json!({
                "type":"object",
                "additionalProperties":false
            }),
            output_schema: json!({
                "type":"object",
                "properties":{"done":{"const":true}},
                "required":["done"],
                "additionalProperties":false
            }),
            requires: vec!["driver:fixture".into()],
            risk: Risk::ReadOnly,
            idempotency: Idempotency::ReadOnly,
            timeout_ms: 5_000,
            dry_run: true,
            interactive_consent: false,
            backends: vec!["driver:fixture".into()],
        },
        aliases: vec!["long".into()],
        tags: vec!["fixture".into(), "protocol-v2".into()],
        object_types: vec![],
    }
}

struct Fixture;

#[async_trait]
impl Driver for Fixture {
    fn id(&self) -> &str {
        "fixture"
    }
    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }
    fn interfaces(&self) -> DriverInterfaces {
        DriverInterfaces {
            dynamic_capabilities: true,
            cooperative_cancellation: true,
            events: true,
            progress: true,
            artifacts: true,
            health: true,
            native_refs: false,
        }
    }
    async fn capabilities(&mut self) -> Result<Vec<Capability>> {
        Ok(vec![capability(), mount_capability(), long_capability()])
    }
    async fn execute(&mut self, command: &str, pinned_digest: &str, args: Value) -> Result<Value> {
        let capability = match command {
            "driver.fixture.ping" => capability(),
            "driver.fixture.mount_probe" => mount_capability(),
            _ => {
                return Err(Error::new(
                    ErrorCode::StaleReference,
                    "Driver descriptor is not the pinned capability",
                ));
            }
        };
        if descriptor_digest(&capability.descriptor)? != pinned_digest {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Driver descriptor is not the pinned capability",
            ));
        }
        if args.as_object().is_none_or(|args| !args.is_empty()) {
            return Err(Error::invalid("fixture command accepts an empty object"));
        }
        if command == "driver.fixture.mount_probe" {
            let root = workspace_mount("fixture-data")?;
            let read = std::fs::read_to_string(root.join("input.txt"))?;
            let write_ok = std::fs::write(root.join("child.txt"), b"written").is_ok();
            return Ok(json!({"read":read,"write_ok":write_ok}));
        }
        Ok(json!({"ok":true}))
    }
    async fn execute_with_context(
        &mut self,
        command: &str,
        pinned_digest: &str,
        args: Value,
        context: DriverExecutionContext,
    ) -> Result<Value> {
        if command != "driver.fixture.long" {
            return self.execute(command, pinned_digest, args).await;
        }
        let capability = long_capability();
        if descriptor_digest(&capability.descriptor)? != pinned_digest {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Driver descriptor is not the pinned capability",
            ));
        }
        if args.as_object().is_none_or(|args| !args.is_empty()) {
            return Err(Error::invalid("fixture long accepts an empty object"));
        }
        context.emit_event(
            "fixture.started",
            json!({"request_id":context.request_id()}),
        )?;
        context.capabilities_changed()?;
        let cancellation = context.cancellation();
        for completed in 1..=4 {
            tokio::select! {
                _ = cancellation.cancelled() => {
                    return Err(Error::new(ErrorCode::Cancelled, "Fixture observed cooperative cancellation"));
                }
                _ = tokio::time::sleep(std::time::Duration::from_millis(80)) => {}
            }
            let artifacts = if completed == 2 {
                vec![JobArtifact {
                    name: "preview".into(),
                    reference: "artifact:fixture-preview".into(),
                    media_type: Some("application/octet-stream".into()),
                    sha256: Some("b".repeat(64)),
                    bytes: Some(16),
                }]
            } else {
                vec![]
            };
            context.report_progress(
                JobProgress {
                    completed,
                    total: Some(4),
                    message: Some(format!("fixture step {completed}")),
                },
                artifacts,
            )?;
        }
        Ok(json!({"done":true}))
    }

    async fn health(&mut self) -> Result<Value> {
        Ok(json!({"healthy":true,"fixture":true}))
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = serve(Fixture).await {
        eprintln!("{error}");
        std::process::exit(error.exit_code());
    }
}
