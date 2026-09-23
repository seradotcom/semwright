//! Useful no-filesystem example: deterministic Unicode text statistics.
use semwright_types::{CommandDescriptor, Error, Idempotency, Result, Risk, arg_str};
use serde_json::{Value, json};

const PLUGIN_NAME: &str = "textstats";
const PLUGIN_VERSION: &str = "1.0.0";

fn commands() -> Vec<CommandDescriptor> {
    vec![CommandDescriptor {
        name: "plugin.textstats.count".into(),
        version: "1.0".into(),
        description: "Count Unicode characters, words, lines and UTF-8 bytes.".into(),
        input_schema: json!({
            "type":"object",
            "properties":{"text":{"type":"string","maxLength":65536}},
            "required":["text"],
            "additionalProperties":false
        }),
        output_schema: json!({
            "type":"object",
            "properties":{
                "bytes":{"type":"integer","minimum":0},
                "characters":{"type":"integer","minimum":0},
                "words":{"type":"integer","minimum":0},
                "lines":{"type":"integer","minimum":0}
            },
            "required":["bytes","characters","words","lines"],
            "additionalProperties":false
        }),
        requires: vec!["plugin:textstats".into()],
        risk: Risk::ReadOnly,
        idempotency: Idempotency::ReadOnly,
        timeout_ms: 5000,
        dry_run: true,
        interactive_consent: false,
        backends: vec!["plugin".into()],
    }]
}

fn count(command: &str, args: Value) -> Result<Value> {
    if command != "plugin.textstats.count" {
        return Err(Error::invalid("Unknown textstats command"));
    }
    let text = arg_str(&args, "text")?;
    if text.len() > 65536 {
        return Err(Error::invalid("Text exceeds budget"));
    }
    Ok(
        json!({"bytes":text.len(),"characters":text.chars().count(),"words":text.split_whitespace().count(),"lines":text.lines().count()}),
    )
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let commands = commands();
    if semwright_plugin_sdk::serve(PLUGIN_NAME, PLUGIN_VERSION, &commands, count)
        .await
        .is_err()
    {
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_is_not_bytes() {
        let out = count("plugin.textstats.count", json!({"text":"hola México"})).unwrap();
        assert_eq!(out["words"], 2);
        assert_eq!(out["characters"], 11);
        assert_eq!(out["bytes"], 12);
    }

    #[test]
    fn embedded_descriptor_matches_manifest_template() {
        let manifest: semwright_plugin_sdk::Manifest =
            serde_json::from_str(include_str!("../manifest.template.json")).unwrap();
        assert_eq!(manifest.name, PLUGIN_NAME);
        assert_eq!(manifest.version, PLUGIN_VERSION);
        assert_eq!(
            semwright_plugin_sdk::commands_digest(&manifest.commands).unwrap(),
            semwright_plugin_sdk::commands_digest(&commands()).unwrap()
        );
    }
}
