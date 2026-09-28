use semwright_plugin_sdk::workspace_mount;
use semwright_types::{CommandDescriptor, Error, Idempotency, Result, Risk};
use serde_json::{Value, json};

pub const NAME: &str = "windows-fixture";
pub const VERSION: &str = "1.0.0";
pub const COMMAND: &str = "plugin.windows-fixture.ping";

pub fn commands() -> Vec<CommandDescriptor> {
    vec![CommandDescriptor {
        name: COMMAND.into(),
        version: "1.0".into(),
        description: "Test-only Windows secure Plugin Host round-trip.".into(),
        input_schema: json!({
            "type":"object",
            "properties":{
                "message":{"type":"string","maxLength":128},
                "address":{"type":"string","maxLength":128},
                "host_path":{"type":"string","maxLength":4096},
                "mount_name":{"type":"string","maxLength":64}
            },
            "required":["message"],
            "additionalProperties":false
        }),
        output_schema: json!({
            "type":"object",
            "properties":{
                "message":{"type":"string","maxLength":128},
                "path_present":{"type":"boolean"},
                "local_app_data_present":{"type":"boolean"},
                "temp_present":{"type":"boolean"},
                "network_reachable":{"type":"boolean"},
                "host_visible":{"type":"boolean"},
                "mount_read":{"type":["string","null"]},
                "mount_write_ok":{"type":"boolean"}
            },
            "required":["message","path_present","local_app_data_present","temp_present","network_reachable","host_visible","mount_read","mount_write_ok"],
            "additionalProperties":false
        }),
        requires: vec!["plugin:windows-fixture".into()],
        risk: Risk::MutatingReversible,
        idempotency: Idempotency::Idempotent,
        timeout_ms: 5_000,
        dry_run: false,
        interactive_consent: false,
        backends: vec!["plugin".into()],
    }]
}

pub fn dispatch(command: &str, args: Value) -> Result<Value> {
    if command != COMMAND {
        return Err(Error::invalid("Unknown Windows plugin fixture command"));
    }
    let message = args
        .get("message")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::invalid("Fixture message is required"))?;
    let network_reachable = args
        .get("address")
        .and_then(Value::as_str)
        .and_then(|address| address.parse::<std::net::SocketAddr>().ok())
        .is_some_and(|address| {
            // A denied LPAC connect can remain pending longer than the socket deadline.
            // Keep the kernel wait off the plugin protocol thread and bound the fixture probe.
            let (sender, receiver) = std::sync::mpsc::sync_channel(1);
            let _probe = std::thread::spawn(move || {
                let reachable = std::net::TcpStream::connect_timeout(
                    &address,
                    std::time::Duration::from_millis(750),
                )
                .is_ok();
                let _ = sender.send(reachable);
            });
            receiver
                .recv_timeout(std::time::Duration::from_millis(1_000))
                .unwrap_or(false)
        });
    let host_visible = args
        .get("host_path")
        .and_then(Value::as_str)
        .is_some_and(|path| std::fs::read(path).is_ok());
    let (mount_read, mount_write_ok) = match args.get("mount_name").and_then(Value::as_str) {
        Some(name) => {
            let root = workspace_mount(name)?;
            let read = std::fs::read_to_string(root.join("input.txt"))?;
            let write_ok = std::fs::write(root.join("child.txt"), b"plugin-written").is_ok();
            (Some(read), write_ok)
        }
        None => (None, false),
    };
    Ok(json!({
        "message": message,
        "path_present": std::env::var_os("PATH").is_some(),
        "local_app_data_present": std::env::var_os("LOCALAPPDATA").is_some(),
        "temp_present": std::env::var_os("TEMP").is_some(),
        "network_reachable": network_reachable,
        "host_visible": host_visible,
        "mount_read": mount_read,
        "mount_write_ok": mount_write_ok,
    }))
}
