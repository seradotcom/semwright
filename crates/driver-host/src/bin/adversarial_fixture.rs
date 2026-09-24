use async_trait::async_trait;
use semwright_driver_sdk::{Capability, Driver, descriptor_digest, serve};
use semwright_types::{CommandDescriptor, Error, ErrorCode, Idempotency, Result, Risk};
use serde_json::{Value, json};
use std::{
    net::{SocketAddr, TcpStream},
    process::Command,
    time::Duration,
};

const ID: &str = "adversarial";

fn descriptor(name: &str, description: &str) -> CommandDescriptor {
    CommandDescriptor {
        name: name.into(),
        version: "1".into(),
        description: description.into(),
        input_schema: if name.ends_with(".probe") {
            json!({
                "type":"object",
                "properties":{
                    "host_pid":{"type":"integer","minimum":1},
                    "host_port":{"type":"integer","minimum":1,"maximum":65535},
                    "host_secret":{"type":"string","minLength":1,"maxLength":4096}
                },
                "required":["host_pid","host_port","host_secret"],
                "additionalProperties":false
            })
        } else {
            json!({"type":"object","additionalProperties":false})
        },
        output_schema: json!({"type":"object","additionalProperties":true}),
        requires: vec!["driver:adversarial".into()],
        risk: Risk::Mutating,
        idempotency: Idempotency::NonIdempotent,
        timeout_ms: 2_000,
        dry_run: false,
        interactive_consent: false,
        backends: vec!["driver:adversarial".into()],
    }
}

fn capabilities() -> Vec<Capability> {
    vec![
        Capability {
            descriptor: descriptor(
                "driver.adversarial.probe",
                "Test-only hostile driver sandbox probe",
            ),
            aliases: vec![],
            tags: vec!["test".into(), "sandbox".into()],
            object_types: vec![],
        },
        Capability {
            descriptor: descriptor(
                "driver.adversarial.spawn_descendant",
                "Test-only descendant cleanup probe",
            ),
            aliases: vec![],
            tags: vec!["test".into(), "sandbox".into()],
            object_types: vec![],
        },
    ]
}

fn required_u64(args: &Value, key: &str) -> Result<u64> {
    args.get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| Error::invalid(format!("missing {key}")))
}

fn required_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::invalid(format!("missing {key}")))
}

struct Adversarial;

#[async_trait]
impl Driver for Adversarial {
    fn id(&self) -> &str {
        ID
    }

    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    async fn capabilities(&mut self) -> Result<Vec<Capability>> {
        Ok(capabilities())
    }

    async fn execute(&mut self, command: &str, pinned_digest: &str, args: Value) -> Result<Value> {
        let capability = capabilities()
            .into_iter()
            .find(|capability| capability.descriptor.name == command)
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "unknown adversarial command"))?;
        if descriptor_digest(&capability.descriptor)? != pinned_digest {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "adversarial descriptor digest mismatch",
            ));
        }

        match command {
            "driver.adversarial.probe" => {
                let host_pid = required_u64(&args, "host_pid")?;
                let port = u16::try_from(required_u64(&args, "host_port")?)
                    .map_err(|_| Error::invalid("invalid port"))?;
                let host_secret = required_str(&args, "host_secret")?;

                let allowed_read = std::fs::read_to_string("/workspace/ro/allowed.txt")
                    .is_ok_and(|text| text == "allowed");
                let allowed_write = std::fs::write("/workspace/rw/allowed.txt", b"allowed").is_ok();
                let readonly_write =
                    std::fs::write("/workspace/ro/blocked.txt", b"blocked").is_ok();
                let readonly_execute = Command::new("/workspace/ro/tool").status().is_ok();
                let outside_home_write = std::fs::write("/home/breakout", b"blocked").is_ok();
                let outside_etc_write = std::fs::write("/etc/breakout", b"blocked").is_ok();
                let host_secret_visible = std::fs::read(host_secret).is_ok();
                let host_pid_visible = std::path::Path::new(&format!("/proc/{host_pid}")).exists();
                let address = SocketAddr::from(([127, 0, 0, 1], port));
                let host_loopback_connected =
                    TcpStream::connect_timeout(&address, Duration::from_millis(150)).is_ok();

                let mut environment = std::env::vars().map(|(key, _)| key).collect::<Vec<_>>();
                environment.sort();

                let mut nofile = libc::rlimit {
                    rlim_cur: 0,
                    rlim_max: 0,
                };
                // SAFETY: getrlimit synchronously initializes a valid rlimit pointer.
                let nofile_ok = unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut nofile) } == 0;

                Ok(json!({
                    "allowed_read":allowed_read,
                    "allowed_write":allowed_write,
                    "readonly_write":readonly_write,
                    "readonly_execute":readonly_execute,
                    "outside_home_write":outside_home_write,
                    "outside_etc_write":outside_etc_write,
                    "host_secret_visible":host_secret_visible,
                    "host_pid_visible":host_pid_visible,
                    "host_loopback_connected":host_loopback_connected,
                    "environment":environment,
                    "nofile": if nofile_ok { Some(nofile.rlim_cur) } else { None },
                }))
            }
            "driver.adversarial.spawn_descendant" => {
                Command::new("/usr/bin/sh")
                    .args([
                        "-c",
                        "sleep 1; printf escaped > /workspace/rw/driver-descendant.txt",
                    ])
                    .spawn()
                    .map_err(|_| Error::new(ErrorCode::BackendFailed, "descendant spawn failed"))?;
                Ok(json!({"spawned":true}))
            }
            _ => Err(Error::new(
                ErrorCode::NotFound,
                "unknown adversarial command",
            )),
        }
    }
}

#[tokio::main]
async fn main() {
    if let Err(error) = serve(Adversarial).await {
        eprintln!("{error}");
        std::process::exit(error.exit_code());
    }
}
