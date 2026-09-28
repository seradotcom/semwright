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
                "host_path":{"type":"string","maxLength":4096}
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
                "host_visible":{"type":"boolean"}
            },
            "required":["message","path_present","local_app_data_present","temp_present","network_reachable","host_visible"],
            "additionalProperties":false
        }),
        requires: vec!["plugin:windows-fixture".into()],
        risk: Risk::ReadOnly,
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
            std::net::TcpStream::connect_timeout(&address, std::time::Duration::from_millis(750))
                .is_ok()
        });
    let host_visible = args
        .get("host_path")
        .and_then(Value::as_str)
        .is_some_and(|path| std::fs::read(path).is_ok());
    Ok(json!({
        "message": message,
        "path_present": std::env::var_os("PATH").is_some(),
        "local_app_data_present": std::env::var_os("LOCALAPPDATA").is_some(),
        "temp_present": std::env::var_os("TEMP").is_some(),
        "network_reachable": network_reachable,
        "host_visible": host_visible,
    }))
}
