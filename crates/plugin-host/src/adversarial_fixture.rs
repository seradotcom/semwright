use semwright_types::{CommandDescriptor, Error, ErrorCode, Idempotency, Result, Risk};
use serde_json::{Value, json};
use std::{
    net::{SocketAddr, TcpStream},
    process::Command,
    time::Duration,
};

pub const NAME: &str = "adversarial";
pub const VERSION: &str = "1.0.0";

fn descriptor(
    name: &str,
    description: &str,
    input_schema: Value,
    output_schema: Value,
    timeout_ms: u64,
) -> CommandDescriptor {
    CommandDescriptor {
        name: name.into(),
        version: "1.0".into(),
        description: description.into(),
        input_schema,
        output_schema,
        requires: vec!["plugin:adversarial".into()],
        risk: Risk::Mutating,
        idempotency: Idempotency::NonIdempotent,
        timeout_ms,
        dry_run: false,
        interactive_consent: false,
        backends: vec!["plugin".into()],
    }
}

pub fn commands() -> Vec<CommandDescriptor> {
    vec![
        descriptor(
            "plugin.adversarial.probe",
            "Test-only hostile sandbox probe.",
            json!({
                "type":"object",
                "properties":{
                    "host_pid":{"type":"integer","minimum":1},
                    "host_port":{"type":"integer","minimum":1,"maximum":65535},
                    "host_secret":{"type":"string","maxLength":4096}
                },
                "required":["host_pid","host_port","host_secret"],
                "additionalProperties":false
            }),
            json!({
                "type":"object",
                "properties":{
                    "allowed_read":{"type":"boolean"},
                    "allowed_write":{"type":"boolean"},
                    "readonly_write":{"type":"boolean"},
                    "outside_home_write":{"type":"boolean"},
                    "outside_etc_write":{"type":"boolean"},
                    "null_device_write":{"type":"boolean"},
                    "other_device_write":{"type":"boolean"},
                    "host_secret_visible":{"type":"boolean"},
                    "host_pid_visible":{"type":"boolean"},
                    "host_loopback_connected":{"type":"boolean"},
                    "environment":{"type":"array","items":{"type":"string","maxLength":128},"maxItems":32}
                },
                "required":[
                    "allowed_read","allowed_write","readonly_write","outside_home_write",
                    "outside_etc_write","null_device_write","other_device_write",
                    "host_secret_visible","host_pid_visible","host_loopback_connected","environment"
                ],
                "additionalProperties":false
            }),
            3000,
        ),
        descriptor(
            "plugin.adversarial.hang",
            "Test-only watchdog and descendant-cleanup probe.",
            json!({"type":"object","additionalProperties":false}),
            json!({"type":"object","additionalProperties":false}),
            250,
        ),
    ]
}

fn required_u64(args: &Value, key: &str) -> Result<u64> {
    args.get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| Error::invalid("Missing adversarial fixture integer"))
}

fn required_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::invalid("Missing adversarial fixture string"))
}

pub fn dispatch(command: &str, args: Value) -> Result<Value> {
    match command {
        "plugin.adversarial.probe" => {
            let host_pid = required_u64(&args, "host_pid")?;
            let host_port = required_u64(&args, "host_port")?;
            let host_secret = required_str(&args, "host_secret")?;
            let port = u16::try_from(host_port)
                .map_err(|_| Error::invalid("Invalid adversarial fixture port"))?;

            let allowed_read = std::fs::read_to_string("/workspace/ro/allowed.txt")
                .is_ok_and(|text| text == "allowed");
            let allowed_write = std::fs::write("/workspace/rw/allowed.txt", b"allowed").is_ok();
            let readonly_write = std::fs::write("/workspace/ro/blocked.txt", b"blocked").is_ok();
            let outside_home_write = std::fs::write("/home/breakout", b"blocked").is_ok();
            let outside_etc_write = std::fs::write("/etc/breakout", b"blocked").is_ok();
            let null_device_write = std::fs::write("/dev/null", b"discarded").is_ok();
            let other_device_write = std::fs::write("/dev/zero", b"blocked").is_ok();
            let host_secret_visible = std::fs::read(host_secret).is_ok();
            let host_pid_visible = std::path::Path::new(&format!("/proc/{host_pid}")).exists();
            let address = SocketAddr::from(([127, 0, 0, 1], port));
            let host_loopback_connected =
                TcpStream::connect_timeout(&address, Duration::from_millis(150)).is_ok();
            let mut environment = std::env::vars().map(|(key, _)| key).collect::<Vec<_>>();
            environment.sort();

            Ok(json!({
                "allowed_read":allowed_read,
                "allowed_write":allowed_write,
                "readonly_write":readonly_write,
                "outside_home_write":outside_home_write,
                "outside_etc_write":outside_etc_write,
                "null_device_write":null_device_write,
                "other_device_write":other_device_write,
                "host_secret_visible":host_secret_visible,
                "host_pid_visible":host_pid_visible,
                "host_loopback_connected":host_loopback_connected,
                "environment":environment
            }))
        }
        "plugin.adversarial.hang" => {
            Command::new("/usr/bin/sh")
                .args([
                    "-c",
                    "sleep 1; printf leaked > /workspace/rw/descendant.txt",
                ])
                .spawn()
                .map_err(|_| {
                    Error::new(
                        ErrorCode::BackendFailed,
                        "Adversarial descendant could not be spawned",
                    )
                })?;
            std::thread::sleep(Duration::from_secs(10));
            Ok(json!({}))
        }
        _ => Err(Error::new(
            ErrorCode::NotFound,
            "Adversarial fixture command not found",
        )),
    }
}
