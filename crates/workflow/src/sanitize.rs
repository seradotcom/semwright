use semwright_types::{Error, ErrorCode, Result};
use serde_json::{Map, Value};

const MAX_VALUE_BYTES: usize = 64 * 1024;

pub fn looks_like_ref(s: &str) -> bool {
    let Some((kind, id)) = s.split_once(':') else {
        return false;
    };
    !kind.is_empty()
        && kind.len() <= 64
        && kind.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_' | b'.')
        })
        && id.len() == 32
        && id.bytes().all(|b| b.is_ascii_hexdigit())
}

fn private_key_name(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    [
        "password",
        "passwd",
        "secret",
        "token",
        "authorization",
        "cookie",
        "credential",
        "api_key",
        "apikey",
        "private_key",
    ]
    .iter()
    .any(|needle| key.contains(needle))
}

pub fn sanitize(value: &Value, capture_values: bool) -> Result<(Value, bool)> {
    fn walk(value: &Value, capture: bool, depth: usize, redacted: &mut bool) -> Result<Value> {
        if depth > 32 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Workflow value nesting exceeds 32",
            ));
        }
        Ok(match value {
            Value::Object(map) => {
                if map.len() > 256 {
                    return Err(Error::new(
                        ErrorCode::ResourceExhausted,
                        "Workflow object exceeds property budget",
                    ));
                }
                let mut out = Map::new();
                for (key, child) in map {
                    if private_key_name(key) {
                        *redacted = true;
                        out.insert(key.clone(), Value::String("[REDACTED]".into()));
                    } else {
                        out.insert(key.clone(), walk(child, capture, depth + 1, redacted)?);
                    }
                }
                Value::Object(out)
            }
            Value::Array(values) => {
                if values.len() > 2048 {
                    return Err(Error::new(
                        ErrorCode::ResourceExhausted,
                        "Workflow array exceeds item budget",
                    ));
                }
                Value::Array(
                    values
                        .iter()
                        .map(|v| walk(v, capture, depth + 1, redacted))
                        .collect::<Result<Vec<_>>>()?,
                )
            }
            Value::String(s) => {
                if !capture && !looks_like_ref(s) {
                    *redacted = true;
                    Value::String("[VALUE REDACTED]".into())
                } else if s.len() > 8192 {
                    return Err(Error::new(
                        ErrorCode::ResourceExhausted,
                        "Workflow string exceeds 8 KiB",
                    ));
                } else {
                    value.clone()
                }
            }
            Value::Number(_) | Value::Bool(_) if !capture => {
                *redacted = true;
                Value::String("[VALUE REDACTED]".into())
            }
            _ => value.clone(),
        })
    }
    let mut redacted = false;
    let out = walk(value, capture_values, 0, &mut redacted)?;
    if serde_json::to_vec(&out)?.len() > MAX_VALUE_BYTES {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Workflow captured value exceeds 64 KiB",
        ));
    }
    Ok((out, redacted))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn metadata_capture_keeps_refs_but_redacts_values() {
        let (value, redacted) = sanitize(
            &json!({"name":"private title","ref":"ui:00000000000000000000000000000001","password":"x"}),
            false,
        ).unwrap();
        assert!(redacted);
        assert_eq!(value["name"], "[VALUE REDACTED]");
        assert_eq!(value["password"], "[REDACTED]");
        assert_eq!(value["ref"], "ui:00000000000000000000000000000001");
    }

    #[test]
    fn recognizes_driver_owned_opaque_reference_families() {
        assert!(looks_like_ref("video:00000000000000000000000000000001"));
        assert!(looks_like_ref("obs-scene:abcdefabcdefabcdefabcdefabcdefab"));
        assert!(looks_like_ref(
            "custom.ref_kind:0123456789abcdef0123456789abcdef"
        ));
        assert!(!looks_like_ref("https://example.invalid"));
        assert!(!looks_like_ref("sha256:0123456789abcdef"));
        assert!(!looks_like_ref("UPPER:0123456789abcdef0123456789abcdef"));
    }
}
