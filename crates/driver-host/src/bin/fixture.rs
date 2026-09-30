use async_trait::async_trait;
use semwright_driver_sdk::{
    Capability, Driver, DriverExecutionContext, DriverInterfaces, RuntimeToolArg, RuntimeToolCwd,
    RuntimeToolJob, RuntimeToolJobStatus, RuntimeToolSession, descriptor_digest, secret_mount,
    serve, system_config_mount, tool_path, workspace_mount,
};
use semwright_types::{
    CommandDescriptor, Error, ErrorCode, Idempotency, JobArtifact, JobProgress, Result, Risk,
};
use serde_json::{Value, json};
#[cfg(windows)]
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::windows::named_pipe::ClientOptions,
};

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
            input_schema: json!({
                "type":"object",
                "properties":{
                    "cwd_mount":{"type":"string","minLength":1,"maxLength":64},
                    "cwd_relative":{"type":"string","maxLength":1024},
                    "path_mount":{"type":"string","minLength":1,"maxLength":64},
                    "path_relative":{"type":"string","maxLength":0},
                    "dependency":{"type":"string","minLength":1,"maxLength":64}
                },
                "additionalProperties":false
            }),
            output_schema: json!({
                "type":"object",
                "properties":{
                    "stdout":{"type":"string"},
                    "read_ok":{"type":"boolean"},
                    "execute_open_ok":{"type":"boolean"},
                    "self_spawn_ok":{"type":"boolean"},
                    "self_spawn_errno":{"type":"integer"},
                    "null_spawn_ok":{"type":"boolean"},
                    "null_spawn_errno":{"type":"integer"},
                    "write_ok":{"type":"boolean"},
                    "spawn_error_kind":{"type":"string"},
                    "spawn_errno":{"type":"integer"},
                    "exit_code":{"type":"integer"}
                },
                "required":["stdout","read_ok","execute_open_ok","self_spawn_ok","self_spawn_errno","null_spawn_ok","null_spawn_errno","write_ok","spawn_error_kind","spawn_errno","exit_code"],
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

fn tool_job_capability() -> Capability {
    Capability {
        descriptor: CommandDescriptor {
            name: "driver.fixture.tool_job".into(),
            version: "1".into(),
            description: "Exercise detached Host-owned runtime-tool job lifecycle".into(),
            input_schema: json!({
                "type":"object",
                "properties":{
                    "action":{"enum":["start","status","cancel"]},
                    "job":{"type":"string","minLength":1,"maxLength":128},
                    "sleep_ms":{"type":"integer","minimum":1,"maximum":5000},
                    "cwd_mount":{"type":"string","minLength":1,"maxLength":64},
                    "lifecycle_marker":{"type":"boolean"}
                },
                "required":["action"],
                "additionalProperties":false
            }),
            output_schema: json!({
                "type":"object",
                "properties":{
                    "job":{"type":"string"},
                    "state":{"enum":["running","cancelling","succeeded","failed","cancelled"]},
                    "exit_code":{"type":"integer"},
                    "stdout":{"type":"string"},
                    "error_code":{"type":"string"}
                },
                "required":["job","state"],
                "additionalProperties":false
            }),
            requires: vec!["driver:fixture".into()],
            risk: Risk::ReadOnly,
            idempotency: Idempotency::NonIdempotent,
            timeout_ms: 5_000,
            dry_run: false,
            interactive_consent: false,
            backends: vec!["driver:fixture".into()],
        },
        aliases: vec!["tool_job".into()],
        tags: vec!["fixture".into(), "conformance".into(), "tool-job".into()],
        object_types: vec![],
    }
}

fn tool_session_capability() -> Capability {
    Capability {
        descriptor: CommandDescriptor {
            name: "driver.fixture.tool_session".into(),
            version: "1".into(),
            description: "Exercise provider-scoped persistent Host runtime-tool sessions".into(),
            input_schema: json!({
                "type":"object",
                "properties":{
                    "action":{"enum":["start","request","close"]},
                    "session":{"type":"string","minLength":1,"maxLength":128},
                    "payload":{"type":"string","maxLength":1024},
                    "cwd_mount":{"type":"string","minLength":1,"maxLength":64},
                    "lifecycle_marker":{"type":"boolean"}
                },
                "required":["action"],
                "additionalProperties":false
            }),
            output_schema: json!({
                "type":"object",
                "properties":{
                    "session":{"type":"string"},
                    "state":{"enum":["open","frame","closed"]},
                    "payload":{"type":"string"}
                },
                "required":["session","state"],
                "additionalProperties":false
            }),
            requires: vec!["driver:fixture".into()],
            risk: Risk::ReadOnly,
            idempotency: Idempotency::NonIdempotent,
            timeout_ms: 5_000,
            dry_run: false,
            interactive_consent: false,
            backends: vec!["driver:fixture".into()],
        },
        aliases: vec!["tool_session".into()],
        tags: vec![
            "fixture".into(),
            "conformance".into(),
            "tool-session".into(),
        ],
        object_types: vec![],
    }
}

fn tool_job_result(job: &RuntimeToolJob, status: RuntimeToolJobStatus) -> Result<Value> {
    let mut value = serde_json::Map::new();
    value.insert("job".into(), job.id.clone().into());
    match status {
        RuntimeToolJobStatus::Running => {
            value.insert("state".into(), "running".into());
        }
        RuntimeToolJobStatus::Cancelling => {
            value.insert("state".into(), "cancelling".into());
        }
        RuntimeToolJobStatus::Succeeded { output } => {
            let stdout = String::from_utf8(output.stdout).map_err(|_| {
                Error::new(
                    ErrorCode::BackendFailed,
                    "fixture detached tool output was not UTF-8",
                )
            })?;
            value.insert("state".into(), "succeeded".into());
            value.insert("exit_code".into(), output.exit_code.into());
            value.insert("stdout".into(), stdout.into());
        }
        RuntimeToolJobStatus::Failed { error } => {
            value.insert("state".into(), "failed".into());
            value.insert("error_code".into(), format!("{:?}", error.code).into());
        }
        RuntimeToolJobStatus::Cancelled => {
            value.insert("state".into(), "cancelled".into());
        }
    }
    Ok(Value::Object(value))
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

fn network_capability() -> Capability {
    Capability {
        descriptor: CommandDescriptor {
            name: "driver.fixture.network_probe".into(),
            version: "1".into(),
            description:
                "Probe owner-granted ambient network without inheriting filesystem authority".into(),
            input_schema: json!({
                "type":"object",
                "properties":{
                    "address":{"type":"string","maxLength":128},
                    "host_path":{"type":"string","maxLength":4096}
                },
                "required":["address","host_path"],
                "additionalProperties":false
            }),
            output_schema: json!({
                "type":"object",
                "properties":{
                    "reachable":{"type":"boolean"},
                    "host_visible":{"type":"boolean"},
                    "error_code":{"type":"integer"}
                },
                "required":["reachable","host_visible","error_code"],
                "additionalProperties":false
            }),
            requires: vec!["driver:fixture".into()],
            risk: Risk::ReadOnly,
            idempotency: Idempotency::ReadOnly,
            timeout_ms: 3_000,
            dry_run: true,
            interactive_consent: false,
            backends: vec!["driver:fixture".into()],
        },
        aliases: vec!["network_probe".into()],
        tags: vec!["fixture".into(), "conformance".into(), "network".into()],
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
            host_tools: std::env::var_os("SEMWRIGHT_DRIVER_HOST_TOOLS").is_some(),
        }
    }
    async fn capabilities(&mut self) -> Result<Vec<Capability>> {
        Ok(vec![
            capability(),
            mount_capability(),
            tool_capability(),
            tool_job_capability(),
            tool_session_capability(),
            config_capability(),
            secret_capability(),
            network_capability(),
            long_capability(),
            disconnect_capability(),
        ])
    }
    async fn execute(&mut self, command: &str, pinned_digest: &str, args: Value) -> Result<Value> {
        let capability = match command {
            "driver.fixture.ping" => capability(),
            "driver.fixture.mount_probe" => mount_capability(),
            "driver.fixture.tool_probe" => tool_capability(),
            "driver.fixture.tool_job" => tool_job_capability(),
            "driver.fixture.tool_session" => tool_session_capability(),
            "driver.fixture.config_probe" => config_capability(),
            "driver.fixture.secret_probe" => secret_capability(),
            "driver.fixture.network_probe" => network_capability(),
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
        if matches!(
            command,
            "driver.fixture.tool_job" | "driver.fixture.tool_session"
        ) {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "fixture runtime-tool lifecycle requires DriverExecutionContext",
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
            let read_ok = std::fs::File::open(&tool).is_ok();
            #[cfg(windows)]
            let execute_open_ok = {
                use std::os::windows::fs::OpenOptionsExt;
                std::fs::OpenOptions::new()
                    .access_mode(0x0012_00A0)
                    .share_mode(0x7)
                    .open(&tool)
                    .is_ok()
            };
            #[cfg(not(windows))]
            let execute_open_ok = read_ok;
            #[cfg(windows)]
            let (self_spawn_ok, self_spawn_errno) = match std::env::current_exe() {
                Ok(executable) => match std::process::Command::new(executable)
                    .env("SEMWRIGHT_FIXTURE_SELF_PROBE", "1")
                    .output()
                {
                    Ok(output) => (
                        output.status.success() && output.stdout.starts_with(b"self-ok"),
                        -1,
                    ),
                    Err(error) => (false, error.raw_os_error().unwrap_or(-1)),
                },
                Err(error) => (false, error.raw_os_error().unwrap_or(-1)),
            };
            #[cfg(not(windows))]
            let (self_spawn_ok, self_spawn_errno) = (true, -1);
            #[cfg(windows)]
            let (null_spawn_ok, null_spawn_errno) = {
                use std::process::Stdio;
                match std::process::Command::new(&tool)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                {
                    Ok(status) => (status.success(), -1),
                    Err(error) => (false, error.raw_os_error().unwrap_or(-1)),
                }
            };
            #[cfg(not(windows))]
            let (null_spawn_ok, null_spawn_errno) = (true, -1);
            let write_ok = std::fs::OpenOptions::new().write(true).open(&tool).is_ok();
            let output = match std::process::Command::new(&tool).output() {
                Ok(output) => output,
                Err(error) => {
                    return Ok(json!({
                        "stdout":"",
                        "read_ok":read_ok,
                        "execute_open_ok":execute_open_ok,
                        "self_spawn_ok":self_spawn_ok,
                        "self_spawn_errno":self_spawn_errno,
                        "null_spawn_ok":null_spawn_ok,
                        "null_spawn_errno":null_spawn_errno,
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
                    "read_ok":read_ok,
                    "execute_open_ok":execute_open_ok,
                    "self_spawn_ok":self_spawn_ok,
                    "self_spawn_errno":self_spawn_errno,
                    "null_spawn_ok":null_spawn_ok,
                    "null_spawn_errno":null_spawn_errno,
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
                "read_ok":read_ok,
                "execute_open_ok":execute_open_ok,
                "self_spawn_ok":self_spawn_ok,
                "self_spawn_errno":self_spawn_errno,
                "null_spawn_ok":null_spawn_ok,
                "null_spawn_errno":null_spawn_errno,
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
        if command == "driver.fixture.network_probe" {
            let args = args
                .as_object()
                .ok_or_else(|| Error::invalid("fixture network probe accepts an object"))?;
            if args
                .keys()
                .any(|key| key != "address" && key != "host_path")
            {
                return Err(Error::invalid(
                    "fixture network probe received an unknown argument",
                ));
            }
            let address = args
                .get("address")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::invalid("fixture network probe address is required"))?
                .parse::<std::net::SocketAddr>()
                .map_err(|_| Error::invalid("fixture network probe address is invalid"))?;
            let host_path = args
                .get("host_path")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::invalid("fixture network probe host_path is required"))?;
            // Windows network isolation can leave a connect operation pending longer than the
            // requested socket deadline. Keep that kernel wait off the Driver protocol runtime and
            // bound the fixture response independently so a denied probe cannot wedge the session.
            let (sender, receiver) = tokio::sync::oneshot::channel();
            let _network_probe = std::thread::spawn(move || {
                let outcome = match std::net::TcpStream::connect_timeout(
                    &address,
                    std::time::Duration::from_millis(750),
                ) {
                    Ok(_) => (true, 0),
                    Err(error) => (false, error.raw_os_error().unwrap_or(-1)),
                };
                let _ = sender.send(outcome);
            });
            let (reachable, error_code) =
                tokio::time::timeout(std::time::Duration::from_millis(1_000), receiver)
                    .await
                    .ok()
                    .and_then(|result| result.ok())
                    .unwrap_or((false, -2));
            let host_visible = std::fs::read(host_path).is_ok();
            return Ok(json!({
                "reachable":reachable,
                "host_visible":host_visible,
                "error_code":error_code
            }));
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
        if command == "driver.fixture.tool_session"
            && std::env::var_os("SEMWRIGHT_DRIVER_HOST_TOOLS").is_some()
        {
            let capability = tool_session_capability();
            if descriptor_digest(&capability.descriptor)? != pinned_digest {
                return Err(Error::new(
                    ErrorCode::StaleReference,
                    "Driver descriptor is not the pinned capability",
                ));
            }
            let args = args
                .as_object()
                .ok_or_else(|| Error::invalid("fixture tool session accepts an object"))?;
            if args.keys().any(|key| {
                !matches!(
                    key.as_str(),
                    "action" | "session" | "payload" | "cwd_mount" | "lifecycle_marker"
                )
            }) {
                return Err(Error::invalid(
                    "fixture tool session received an unknown argument",
                ));
            }
            let action = args
                .get("action")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::invalid("fixture tool session action is required"))?;
            return match action {
                "start" => {
                    if args.get("session").is_some() || args.get("payload").is_some() {
                        return Err(Error::invalid(
                            "fixture tool session start does not accept session or payload",
                        ));
                    }
                    let mut tool_args = vec![RuntimeToolArg::Literal {
                        value: "--session".into(),
                    }];
                    if args
                        .get("lifecycle_marker")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                    {
                        tool_args.push(RuntimeToolArg::Literal {
                            value: "--lifecycle-marker".into(),
                        });
                    }
                    let cwd =
                        args.get("cwd_mount")
                            .and_then(Value::as_str)
                            .map(|mount| RuntimeToolCwd {
                                mount: mount.to_owned(),
                                relative: String::new(),
                            });
                    let session = context
                        .start_runtime_tool_session_args(
                            "probe",
                            tool_args,
                            std::time::Duration::from_secs(60),
                            cwd,
                        )
                        .await?;
                    Ok(json!({"session":session.id,"state":"open"}))
                }
                "request" => {
                    if args.get("cwd_mount").is_some() || args.get("lifecycle_marker").is_some() {
                        return Err(Error::invalid(
                            "fixture tool session request accepts only session and payload",
                        ));
                    }
                    let session = RuntimeToolSession {
                        id: args
                            .get("session")
                            .and_then(Value::as_str)
                            .ok_or_else(|| Error::invalid("fixture session id is required"))?
                            .to_owned(),
                    };
                    let payload = args
                        .get("payload")
                        .and_then(Value::as_str)
                        .filter(|payload| payload.len() <= 1024)
                        .ok_or_else(|| Error::invalid("fixture session payload is invalid"))?;
                    let response = context
                        .runtime_tool_session_request(
                            &session,
                            payload.as_bytes().to_vec(),
                            std::time::Duration::from_secs(2),
                        )
                        .await?;
                    let response = String::from_utf8(response).map_err(|_| {
                        Error::new(
                            ErrorCode::BackendFailed,
                            "fixture session response is not UTF-8",
                        )
                    })?;
                    Ok(json!({"session":session.id,"state":"frame","payload":response}))
                }
                "close" => {
                    if args.get("payload").is_some()
                        || args.get("cwd_mount").is_some()
                        || args.get("lifecycle_marker").is_some()
                    {
                        return Err(Error::invalid(
                            "fixture tool session close does not accept payload",
                        ));
                    }
                    let session = RuntimeToolSession {
                        id: args
                            .get("session")
                            .and_then(Value::as_str)
                            .ok_or_else(|| Error::invalid("fixture session id is required"))?
                            .to_owned(),
                    };
                    context.close_runtime_tool_session(&session).await?;
                    Ok(json!({"session":session.id,"state":"closed"}))
                }
                _ => Err(Error::invalid("fixture tool session action is invalid")),
            };
        }

        if command == "driver.fixture.tool_job"
            && std::env::var_os("SEMWRIGHT_DRIVER_HOST_TOOLS").is_some()
        {
            let capability = tool_job_capability();
            if descriptor_digest(&capability.descriptor)? != pinned_digest {
                return Err(Error::new(
                    ErrorCode::StaleReference,
                    "Driver descriptor is not the pinned capability",
                ));
            }
            let args = args
                .as_object()
                .ok_or_else(|| Error::invalid("fixture tool job accepts an object"))?;
            if args.keys().any(|key| {
                !matches!(
                    key.as_str(),
                    "action" | "job" | "sleep_ms" | "cwd_mount" | "lifecycle_marker"
                )
            }) {
                return Err(Error::invalid(
                    "fixture tool job received an unknown argument",
                ));
            }
            let action = args
                .get("action")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::invalid("fixture tool job action is required"))?;
            return match action {
                "start" => {
                    if args.get("job").is_some() {
                        return Err(Error::invalid(
                            "fixture tool job start does not accept a job id",
                        ));
                    }
                    let sleep_ms = args.get("sleep_ms").and_then(Value::as_u64).unwrap_or(500);
                    if !(1..=5_000).contains(&sleep_ms) {
                        return Err(Error::invalid(
                            "fixture tool job sleep_ms exceeds test bounds",
                        ));
                    }
                    let mut tool_args = vec!["--sleep-ms".into(), sleep_ms.to_string()];
                    if args
                        .get("lifecycle_marker")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                    {
                        tool_args.push("--lifecycle-marker".into());
                    }
                    let cwd =
                        args.get("cwd_mount")
                            .and_then(Value::as_str)
                            .map(|mount| RuntimeToolCwd {
                                mount: mount.to_owned(),
                                relative: String::new(),
                            });
                    let job = context
                        .start_runtime_tool_job(
                            "probe",
                            tool_args,
                            Vec::new(),
                            std::time::Duration::from_secs(10),
                            cwd,
                        )
                        .await?;
                    Ok(json!({"job":job.id,"state":"running"}))
                }
                "status" | "cancel" => {
                    if args.get("sleep_ms").is_some()
                        || args.get("cwd_mount").is_some()
                        || args.get("lifecycle_marker").is_some()
                    {
                        return Err(Error::invalid(
                            "fixture tool job status/cancel accepts only action and job",
                        ));
                    }
                    let job = RuntimeToolJob {
                        id: args
                            .get("job")
                            .and_then(Value::as_str)
                            .ok_or_else(|| Error::invalid("fixture tool job id is required"))?
                            .to_owned(),
                    };
                    let status = if action == "status" {
                        context.runtime_tool_job_status(&job).await?
                    } else {
                        context.cancel_runtime_tool_job(&job).await?
                    };
                    tool_job_result(&job, status)
                }
                _ => Err(Error::invalid("fixture tool job action is invalid")),
            };
        }

        if command == "driver.fixture.tool_probe"
            && std::env::var_os("SEMWRIGHT_DRIVER_HOST_TOOLS").is_some()
        {
            let capability = tool_capability();
            if descriptor_digest(&capability.descriptor)? != pinned_digest {
                return Err(Error::new(
                    ErrorCode::StaleReference,
                    "Driver descriptor is not the pinned capability",
                ));
            }
            let args = args
                .as_object()
                .ok_or_else(|| Error::invalid("fixture tool probe accepts an object"))?;
            if args.keys().any(|key| {
                !matches!(
                    key.as_str(),
                    "cwd_mount" | "cwd_relative" | "path_mount" | "path_relative" | "dependency"
                )
            }) {
                return Err(Error::invalid(
                    "fixture tool probe received an unknown argument",
                ));
            }
            let cwd = match (
                args.get("cwd_mount").and_then(Value::as_str),
                args.get("cwd_relative").and_then(Value::as_str),
            ) {
                (None, None) => None,
                (Some(mount), Some(relative)) => Some(RuntimeToolCwd {
                    mount: mount.into(),
                    relative: relative.into(),
                }),
                _ => {
                    return Err(Error::invalid(
                        "fixture tool probe requires cwd_mount and cwd_relative together",
                    ));
                }
            };

            let path_ref = match (
                args.get("path_mount").and_then(Value::as_str),
                args.get("path_relative").and_then(Value::as_str),
            ) {
                (None, None) => None,
                (Some(mount), Some(relative)) => Some(RuntimeToolArg::MountPath {
                    mount: mount.to_owned(),
                    relative: relative.to_owned(),
                }),
                _ => {
                    return Err(Error::invalid(
                        "fixture tool probe requires path_mount and path_relative together",
                    ));
                }
            };
            let dependency = args.get("dependency").and_then(Value::as_str).map(|tool| {
                RuntimeToolArg::ToolPath {
                    tool: tool.to_owned(),
                }
            });
            let typed = path_ref.is_some() || dependency.is_some();

            let direct_path_visible = tool_path("probe").is_ok();
            #[cfg(windows)]
            let (self_spawn_ok, self_spawn_errno) = match std::env::current_exe() {
                Ok(executable) => match std::process::Command::new(executable)
                    .env("SEMWRIGHT_FIXTURE_SELF_PROBE", "1")
                    .output()
                {
                    Ok(output) => (
                        output.status.success() && output.stdout.starts_with(b"self-ok"),
                        -1,
                    ),
                    Err(error) => (false, error.raw_os_error().unwrap_or(-1)),
                },
                Err(error) => (false, error.raw_os_error().unwrap_or(-1)),
            };
            #[cfg(not(windows))]
            let (self_spawn_ok, self_spawn_errno) = (false, -1);

            let output = if typed {
                let mut runtime_args = Vec::new();
                if let Some(path_ref) = path_ref {
                    runtime_args.push(RuntimeToolArg::Literal {
                        value: "--probe-path".into(),
                    });
                    runtime_args.push(path_ref);
                }
                if let Some(dependency) = dependency {
                    runtime_args.push(RuntimeToolArg::Literal {
                        value: if cfg!(windows) {
                            "--probe-dependency-path".into()
                        } else {
                            "--run-dependency".into()
                        },
                    });
                    runtime_args.push(dependency);
                }
                if cwd.is_some() {
                    runtime_args.push(RuntimeToolArg::Literal {
                        value: "--print-cwd".into(),
                    });
                }
                context
                    .execute_runtime_tool_args(
                        "probe",
                        runtime_args,
                        Vec::new(),
                        std::time::Duration::from_millis(1_500),
                        cwd,
                    )
                    .await?
            } else {
                match cwd {
                    Some(cwd) => {
                        context
                            .execute_runtime_tool_with_cwd(
                                "probe",
                                vec!["--print-cwd".into()],
                                Vec::new(),
                                std::time::Duration::from_millis(1_500),
                                cwd,
                            )
                            .await?
                    }
                    None => {
                        context
                            .execute_tool(
                                "probe",
                                Vec::new(),
                                Vec::new(),
                                std::time::Duration::from_millis(1_500),
                            )
                            .await?
                    }
                }
            };
            let stdout = String::from_utf8(output.stdout).map_err(|_| {
                Error::new(
                    ErrorCode::BackendFailed,
                    "fixture Host-tool output was not UTF-8",
                )
            })?;
            return Ok(json!({
                "stdout":stdout,
                "read_ok":direct_path_visible,
                "execute_open_ok":false,
                "self_spawn_ok":self_spawn_ok,
                "self_spawn_errno":self_spawn_errno,
                "null_spawn_ok":false,
                "null_spawn_errno":-1,
                "write_ok":false,
                "spawn_error_kind":"",
                "spawn_errno":-1,
                "exit_code":output.exit_code
            }));
        }

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

#[cfg(windows)]
async fn loopback_echo_connection(
    mut pipe: tokio::net::windows::named_pipe::NamedPipeClient,
) -> std::io::Result<()> {
    let mut buffer = [0u8; 4096];
    loop {
        let read = pipe.read(&mut buffer).await?;
        if read == 0 {
            return Ok(());
        }
        pipe.write_all(&buffer[..read]).await?;
        pipe.flush().await?;
    }
}

#[cfg(windows)]
async fn loopback_echo_task(path: String) {
    if !path.starts_with(r"\\.\pipe\semwright-loopback-") || path.len() > 256 {
        return;
    }
    loop {
        match ClientOptions::new().open(&path) {
            Ok(pipe) => {
                tokio::spawn(async move {
                    let _ = loopback_echo_connection(pipe).await;
                });
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound
                        | std::io::ErrorKind::PermissionDenied
                        | std::io::ErrorKind::WouldBlock
                ) || error.raw_os_error() == Some(231) =>
            {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
            Err(_) => {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if std::env::var_os("SEMWRIGHT_FIXTURE_SELF_PROBE").is_some() {
        println!("self-ok");
        return;
    }
    #[cfg(windows)]
    if let Ok(path) = std::env::var("SEMWRIGHT_DRIVER_LOOPBACK_PIPE") {
        tokio::spawn(loopback_echo_task(path));
    }
    if let Err(error) = serve(Fixture).await {
        eprintln!("{error}");
        std::process::exit(error.exit_code());
    }
}
