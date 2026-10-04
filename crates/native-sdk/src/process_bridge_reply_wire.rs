//! Strict decoding for the existing process_bridge protocol, not a new SDK.
//! The wire schema and canonical Error/Value authorities remain unchanged.
use super::{BRIDGE_SCHEMA, BridgeReply, MAX_REPLY};
use crate::cooperation::validate_value;
use crate::{Error, ErrorCode, Result, Value};
use serde::Deserialize;
use std::fmt;

pub(super) fn parse(bytes: &[u8]) -> Result<BridgeReply> {
    let value = strict_json(bytes, MAX_REPLY)?;
    let mut fields = match value {
        Value::Object(fields) => fields,
        _ => return Err(Error::invalid("Application reply must be an object")),
    };
    let schema_version = take_string(&mut fields, "schema_version")?;
    let id = take_string(&mut fields, "id")?;
    let ok = match fields.remove("ok") {
        Some(Value::Bool(value)) => value,
        _ => {
            return Err(Error::invalid(
                "Application reply requires a boolean status",
            ));
        }
    };
    if schema_version != BRIDGE_SCHEMA {
        return Err(Error::new(
            ErrorCode::ProtocolMismatch,
            "Application bridge schema differs",
        ));
    }

    // Presence and JSON null are different. A successful null is valid JSON
    // and must not be collapsed by Option<Value>'s ordinary deserializer.
    let (data, error) = if ok {
        let value = fields
            .remove("data")
            .ok_or_else(|| Error::invalid("Successful reply has no data field"))?;
        (Some(value), None)
    } else {
        let value = fields
            .remove("error")
            .ok_or_else(|| Error::invalid("Failed reply has no error field"))?;
        (None, Some(serde_json::from_value::<Error>(value)?))
    };
    if !fields.is_empty() {
        return Err(Error::invalid(
            "Unexpected or conflicting application reply fields",
        ));
    }
    Ok(BridgeReply {
        schema_version,
        id,
        ok,
        data,
        error,
    })
}

fn take_string(fields: &mut serde_json::Map<String, Value>, name: &str) -> Result<String> {
    match fields.remove(name) {
        Some(Value::String(value)) => Ok(value),
        _ => Err(Error::invalid(
            "Application reply is missing a typed identity field",
        )),
    }
}

fn strict_json(bytes: &[u8], limit: usize) -> Result<Value> {
    if bytes.len() > limit {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Bridge frame exceeds byte budget",
        ));
    }

    struct Strict(Value);
    impl<'de> Deserialize<'de> for Strict {
        fn deserialize<D: serde::Deserializer<'de>>(de: D) -> std::result::Result<Self, D::Error> {
            struct Visitor;
            impl<'de> serde::de::Visitor<'de> for Visitor {
                type Value = Strict;

                fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.write_str("bounded JSON without duplicate keys")
                }
                fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Strict, E> {
                    Ok(Strict(Value::Null))
                }
                fn visit_bool<E: serde::de::Error>(
                    self,
                    v: bool,
                ) -> std::result::Result<Strict, E> {
                    Ok(Strict(Value::Bool(v)))
                }
                fn visit_i64<E: serde::de::Error>(self, v: i64) -> std::result::Result<Strict, E> {
                    Ok(Strict(Value::from(v)))
                }
                fn visit_u64<E: serde::de::Error>(self, v: u64) -> std::result::Result<Strict, E> {
                    Ok(Strict(Value::from(v)))
                }
                fn visit_f64<E: serde::de::Error>(self, v: f64) -> std::result::Result<Strict, E> {
                    serde_json::Number::from_f64(v)
                        .map(|number| Strict(Value::Number(number)))
                        .ok_or_else(|| E::custom("nonfinite JSON number"))
                }
                fn visit_str<E: serde::de::Error>(self, v: &str) -> std::result::Result<Strict, E> {
                    Ok(Strict(Value::String(v.into())))
                }
                fn visit_string<E: serde::de::Error>(
                    self,
                    v: String,
                ) -> std::result::Result<Strict, E> {
                    Ok(Strict(Value::String(v)))
                }
                fn visit_seq<A: serde::de::SeqAccess<'de>>(
                    self,
                    mut seq: A,
                ) -> std::result::Result<Strict, A::Error> {
                    let mut values = Vec::new();
                    while let Some(Strict(value)) = seq.next_element()? {
                        values.push(value);
                    }
                    Ok(Strict(Value::Array(values)))
                }
                fn visit_map<A: serde::de::MapAccess<'de>>(
                    self,
                    mut map: A,
                ) -> std::result::Result<Strict, A::Error> {
                    let mut values = serde_json::Map::new();
                    while let Some(key) = map.next_key::<String>()? {
                        if values.contains_key(&key) {
                            return Err(serde::de::Error::custom("duplicate JSON key"));
                        }
                        let Strict(value) = map.next_value()?;
                        values.insert(key, value);
                    }
                    Ok(Strict(Value::Object(values)))
                }
            }
            de.deserialize_any(Visitor)
        }
    }

    let mut de = serde_json::Deserializer::from_slice(bytes);
    let Strict(value) = Strict::deserialize(&mut de)?;
    de.end()?;
    validate_value(&value)?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json;

    fn wire(value: Value) -> Vec<u8> {
        serde_json::to_vec(&value).unwrap()
    }
    fn success(value: Value) -> Value {
        json!({"schema_version":BRIDGE_SCHEMA,"id":"owned-request","ok":true,"data":value})
    }

    #[test]
    fn success_null_is_present_data_not_an_absent_field() {
        let reply = parse(&wire(success(Value::Null))).unwrap();
        assert!(reply.ok);
        assert_eq!(reply.data, Some(Value::Null));
        assert!(reply.error.is_none());
    }

    #[test]
    fn ordinary_results_and_large_opaque_tokens_remain_supported() {
        for value in [
            json!({}),
            json!([true, null, "opaque:90071992547409930000001"]),
            json!(1.5),
        ] {
            assert_eq!(
                parse(&wire(success(value.clone()))).unwrap().data,
                Some(value)
            );
        }
    }

    #[test]
    fn malformed_or_conflicting_discriminants_are_not_accepted() {
        for value in [
            json!({"schema_version":BRIDGE_SCHEMA,"id":"r","ok":true}),
            json!({"schema_version":BRIDGE_SCHEMA,"id":"r","ok":false,"error":null}),
            json!({"schema_version":BRIDGE_SCHEMA,"id":"r","ok":true,"data":null,"error":null}),
            json!({"schema_version":BRIDGE_SCHEMA,"id":"r","ok":true,"data":{},"approved":true}),
            json!({"schema_version":"other","id":"r","ok":true,"data":{}}),
        ] {
            assert!(parse(&wire(value)).is_err());
        }
    }

    #[test]
    fn uncertain_canonical_error_is_not_upgraded_to_no_effect() {
        let reply = parse(&wire(json!({"schema_version":BRIDGE_SCHEMA,"id":"r","ok":false,"error":{"code":"BackendFailed","message":"Synthetic confirmation loss","outcome_known":false}}))).unwrap();
        assert!(!reply.ok);
        assert!(reply.data.is_none());
        assert!(!reply.error.unwrap().outcome_known);
    }

    #[test]
    fn known_canonical_rejection_remains_known() {
        let reply = parse(&wire(json!({"schema_version":BRIDGE_SCHEMA,"id":"r","ok":false,"error":{"code":"Conflict","message":"Synthetic stale revision","outcome_known":true}}))).unwrap();
        let error = reply.error.unwrap();
        assert!(error.outcome_known);
        assert_eq!(error.code, ErrorCode::Conflict);
    }

    #[test]
    fn duplicate_fields_at_any_data_depth_are_rejected() {
        for data in [
            r#"{"x":1,"x":2}"#,
            r#"{"x":1,"\u0078":2}"#,
            r#"[{"x":1,"x":2}]"#,
        ] {
            let text = format!(
                r#"{{"schema_version":"{BRIDGE_SCHEMA}","id":"r","ok":true,"data":{data}}}"#
            );
            assert!(parse(text.as_bytes()).is_err());
        }
    }

    #[test]
    fn framing_unicode_and_shared_numeric_bounds_are_enforced() {
        for data in [
            b"{}{}".as_slice(),
            b"\xff",
            b"\xef\xbb\xbf{}",
            br#""\ud800""#,
            b"9007199254740992",
        ] {
            assert!(strict_json(data, MAX_REPLY).is_err());
        }
        assert!(parse(&vec![b' '; MAX_REPLY + 1]).is_err());
        let depth = format!("{}0{}", "[".repeat(33), "]".repeat(33));
        assert!(strict_json(depth.as_bytes(), MAX_REPLY).is_err());
    }
}
