//! Transport value limits shared by registry validation and pre-publication checks.
//! This is the unchanged registry value budget; schema limits remain in registry.
use crate::{Error, ErrorCode, MAX_FRAME, Result};
use serde_json::Value;

const MAX_VALUE_NODES: usize = 16384;

pub fn value_budget(value: &Value) -> Result<()> {
    let mut pending = vec![(value, 0)];
    let mut nodes = 0;
    let mut bytes = 0usize;
    while let Some((value, depth)) = pending.pop() {
        nodes += 1;
        if nodes > MAX_VALUE_NODES || depth > 64 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "JSON structural budget exceeded",
            ));
        }
        match value {
            Value::Object(map) => {
                bytes = bytes.saturating_add(map.keys().map(String::len).sum::<usize>());
                pending.extend(map.values().map(|v| (v, depth + 1)));
            }
            Value::Array(array) => pending.extend(array.iter().map(|v| (v, depth + 1))),
            Value::String(text) => bytes = bytes.saturating_add(text.len()),
            _ => (),
        }
        if pending.len() > MAX_VALUE_NODES || bytes > MAX_FRAME {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "JSON value budget exceeded",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn value_budget_preserves_node_and_depth_boundaries() {
        let exact = Value::Array(vec![Value::Null; MAX_VALUE_NODES - 1]);
        assert!(value_budget(&exact).is_ok());
        let over = Value::Array(vec![Value::Null; MAX_VALUE_NODES]);
        assert_eq!(
            value_budget(&over).unwrap_err().code,
            ErrorCode::ResourceExhausted
        );
        let pending = Value::Array(vec![Value::Null; MAX_VALUE_NODES + 1]);
        assert_eq!(
            value_budget(&pending).unwrap_err().code,
            ErrorCode::ResourceExhausted
        );
        let mut nested = Value::Null;
        for _ in 0..64 {
            nested = Value::Array(vec![nested]);
        }
        assert!(value_budget(&nested).is_ok());
        nested = Value::Array(vec![nested]);
        assert_eq!(
            value_budget(&nested).unwrap_err().code,
            ErrorCode::ResourceExhausted
        );
    }
    #[test]
    fn value_budget_counts_object_keys_and_strings_at_existing_byte_boundary() {
        let exact = serde_json::json!({"key": "x".repeat(MAX_FRAME - 3)});
        assert!(value_budget(&exact).is_ok());
        let over = serde_json::json!({"key": "x".repeat(MAX_FRAME - 2)});
        assert_eq!(
            value_budget(&over).unwrap_err().code,
            ErrorCode::ResourceExhausted
        );
    }
}
