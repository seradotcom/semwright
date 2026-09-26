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
            "properties":{"message":{"type":"string","maxLength":128}},
            "required":["message"],
            "additionalProperties":false
        }),
        output_schema: json!({
            "type":"object",
            "properties":{
                "message":{"type":"string","maxLength":128},
                "path_present":{"type":"boolean"},
                "local_app_data_present":{"type":"boolean"},
                "temp_present":{"type":"boolean"}
            },
            "required":["message","path_present","local_app_data_present","temp_present"],
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
    Ok(json!({
        "message": message,
        "path_present": std::env::var_os("PATH").is_some(),
        "local_app_data_present": std::env::var_os("LOCALAPPDATA").is_some(),
        "temp_present": std::env::var_os("TEMP").is_some(),
    }))
}
