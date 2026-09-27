use async_trait::async_trait;
use semwright_driver_sdk::{
    Capability, Driver, DriverExecutionContext, DriverInterfaces, descriptor_digest, secret_mount,
    serve, system_config_mount, tool_path, workspace_mount,
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
                "properties":{
                    "cpu_ms":{"type":"integer","minimum":0,"maximum":5000}
                },
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
            timeout_ms: 5_000,
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

fn tool_capability() -> Capability {
    Capability {
        descriptor: CommandDescriptor {
            name: "driver.fixture.tool_probe".into(),
            version: "1".into(),
            description: "Execute one Host-sealed fixture tool and probe mutation authority".into(),
            input_schema: json!({"type":"object","additionalProperties":false}),
            output_schema: json!({
                "type":"object",
                "properties":{
                    "stdout":{"type":"string"},
                    "write_ok":{"type":"boolean"},
                    "spawn_error_kind":{"type":"string"},
                    "spawn_errno":{"type":"integer"},
                    "exit_code":{"type":"integer"}
                },
                "required":["stdout","write_ok","spawn_error_kind","spawn_errno","exit_code"],
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
        aliases: vec!["tool_probe".into()],
        tags: vec!["fixture".into(), "conformance".into(), "tool".into()],
        object_types: vec![],
    }
}

fn config_capability() -> Capability {
    Capability {
        descriptor: CommandDescriptor {
            name: "driver.fixture.config_probe".into(),
            version: "1".into(),
            description: "Read owner-granted system configuration and probe write authority".into(),
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
        aliases: vec!["config_probe".into()],
        tags: vec!["fixture".into(), "conformance".into(), "filesystem".into()],
        object_types: vec![],
    }
}

fn disconnect_capability() -> Capability {
    Capability {
        descriptor: CommandDescriptor {
            name: "driver.fixture.disconnect".into(),
            version: "1".into(),
            description: "Terminate the fixture child during a protocol-v2 request".into(),
            input_schema: json!({"type":"object","additionalProperties":false}),
            output_schema: json!({"type":"object","additionalProperties":false}),
            requires: vec!["driver:fixture".into()],
            risk: Risk::ReadOnly,
            idempotency: Idempotency::ReadOnly,
            timeout_ms: 5_000,
            dry_run: true,
            interactive_consent: false,
            backends: vec!["driver:fixture".into()],
        },
        aliases: vec![],
        tags: vec!["fixture".into(), "protocol-v2".into(), "continuity".into()],
        object_types: vec![],
    }
}

fn secret_capability() -> Capability {
    Capability {
        descriptor: CommandDescriptor {
            name: "driver.fixture.secret_probe".into(),
            version: "1".into(),
            description: "Read an owner-granted secret and probe mutation authority".into(),
            input_schema: json!({"type":"object","additionalProperties":false}),
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
        aliases: vec!["secret_probe".into()],
        tags: vec!["fixture".into(), "conformance".into(), "secret".into()],
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
        Ok(vec![
            capability(),
            mount_capability(),
            tool_capability(),
            config_capability(),
            secret_capability(),
            long_capability(),
            disconnect_capability(),
        ])
    }
    async fn execute(&mut self, command: &str, pinned_digest: &str, args: Value) -> Result<Value> {
        let capability = match command {
            "driver.fixture.ping" => capability(),
            "driver.fixture.mount_probe" => mount_capability(),
            "driver.fixture.tool_probe" => tool_capability(),
            "driver.fixture.config_probe" => config_capability(),
            "driver.fixture.secret_probe" => secret_capability(),
            "driver.fixture.disconnect" => disconnect_capability(),
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
        if command == "driver.fixture.mount_probe" {
            if args.as_object().is_none_or(|args| !args.is_empty()) {
                return Err(Error::invalid(
                    "fixture mount probe accepts an empty object",
                ));
            }
            let root = workspace_mount("fixture-data")?;
            let read = std::fs::read_to_string(root.join("input.txt"))?;
            let write_ok = std::fs::write(root.join("child.txt"), b"written").is_ok();
            return Ok(json!({"read":read,"write_ok":write_ok}));
        }
        if command == "driver.fixture.tool_probe" {
            if args.as_object().is_none_or(|args| !args.is_empty()) {
                return Err(Error::invalid("fixture tool probe accepts an empty object"));
            }
            let tool = tool_path("probe")?;
            let write_ok = std::fs::OpenOptions::new().write(true).open(&tool).is_ok();
            let output = match std::process::Command::new(&tool).output() {
                Ok(output) => output,
                Err(error) => {
                    return Ok(json!({
                        "stdout":"",
                        "write_ok":write_ok,
                        "spawn_error_kind":format!("{:?}", error.kind()),
                        "spawn_errno":error.raw_os_error().unwrap_or(-1),
                        "exit_code":-1
                    }));
                }
            };
            let exit_code = output.status.code().unwrap_or(-1);
            if !output.status.success() {
                return Ok(json!({
                    "stdout":"",
                    "write_ok":write_ok,
                    "spawn_error_kind":"",
                    "spawn_errno":-1,
                    "exit_code":exit_code
                }));
            }
            let stdout = String::from_utf8(output.stdout).map_err(|_| {
                Error::new(
                    ErrorCode::BackendFailed,
                    "fixture tool output was not UTF-8",
                )
            })?;
            return Ok(json!({
                "stdout":stdout,
                "write_ok":write_ok,
                "spawn_error_kind":"",
                "spawn_errno":-1,
                "exit_code":exit_code
            }));
        }
        if command == "driver.fixture.config_probe" {
            if args.as_object().is_none_or(|args| !args.is_empty()) {
                return Err(Error::invalid(
                    "fixture config probe accepts an empty object",
                ));
            }
            let root = system_config_mount("fixture-config")?;
            let read = std::fs::read_to_string(root.join("config.txt"))?;
            let write_ok = std::fs::write(root.join("child.txt"), b"written").is_ok();
            return Ok(json!({"read":read,"write_ok":write_ok}));
        }
        if command == "driver.fixture.secret_probe" {
            if args.as_object().is_none_or(|args| !args.is_empty()) {
                return Err(Error::invalid(
                    "fixture secret probe accepts an empty object",
                ));
            }
            let path = secret_mount("fixture-secret")?;
            let read = std::fs::read_to_string(&path)?;
            let write_ok = std::fs::write(&path, b"changed").is_ok();
            return Ok(json!({"read":read,"write_ok":write_ok}));
        }
        if command == "driver.fixture.disconnect" {
            if args.as_object().is_none_or(|args| !args.is_empty()) {
                return Err(Error::invalid("fixture disconnect accepts an empty object"));
            }
            std::process::exit(71);
        }

        let args = args
            .as_object()
            .ok_or_else(|| Error::invalid("fixture ping accepts an object"))?;
        if args.keys().any(|key| key != "cpu_ms") {
            return Err(Error::invalid("fixture ping received an unknown argument"));
        }
        let cpu_ms = args.get("cpu_ms").and_then(Value::as_u64).unwrap_or(0);
        if cpu_ms > 5_000 {
            return Err(Error::invalid(
                "fixture ping cpu_ms exceeds the test budget",
            ));
        }
        if cpu_ms != 0 {
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(cpu_ms);
            let mut state = 0x9e37_79b9_u64;
            while std::time::Instant::now() < deadline {
                for _ in 0..16_384 {
                    state = state
                        .wrapping_mul(6_364_136_223_846_793_005)
                        .wrapping_add(1);
                }
                std::hint::black_box(state);
            }
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
