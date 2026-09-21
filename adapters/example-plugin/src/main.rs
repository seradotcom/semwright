//! Useful no-filesystem example: deterministic Unicode text statistics.
use semwright_types::{Error, Result, arg_str};
use serde_json::{Value, json};
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
    if semwright_plugin_sdk::serve("textstats", count)
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
}
