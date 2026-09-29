use crate::{ContractError, MAX_PAYLOAD_BYTES, Result, ensure};
use schemars::JsonSchema;
use serde::{
    Deserialize, Serialize,
    de::{DeserializeOwned, Error as _, MapAccess, SeqAccess, Visitor},
};
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use std::fmt;

/// Versioned Semwright canonical JSON, not a claim of RFC 8785 conformance.
/// Keys are sorted by Unicode scalar ordering; array order and string bytes are
/// preserved. Numeric spelling is normalized by serde_json. No lossy Unicode NFC.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct Digest(String);
impl Digest {
    pub fn parse(value: String) -> Result<Self> {
        ensure(
            value.len() == 64
                && value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "expected lowercase SHA-256",
        )?;
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn of_bytes(bytes: &[u8]) -> Self {
        Self(hex::encode(Sha256::digest(bytes)))
    }
}
impl<'de> Deserialize<'de> for Digest {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        Self::parse(String::deserialize(d)?).map_err(D::Error::custom)
    }
}
fn ordered(value: Value, depth: usize) -> Result<Value> {
    ensure(depth <= 64, "canonical JSON nesting limit")?;
    match value {
        Value::Object(map) => {
            let mut pairs: Vec<_> = map.into_iter().collect();
            pairs.sort_by(|a, b| a.0.cmp(&b.0));
            let mut out = serde_json::Map::new();
            for (k, v) in pairs {
                out.insert(k, ordered(v, depth + 1)?);
            }
            Ok(Value::Object(out))
        }
        Value::Array(items) => Ok(Value::Array(
            items
                .into_iter()
                .map(|v| ordered(v, depth + 1))
                .collect::<Result<_>>()?,
        )),
        other => Ok(other),
    }
}
pub fn canonical_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let v = serde_json::to_value(value).map_err(|e| ContractError::Invalid(e.to_string()))?;
    let bytes =
        serde_json::to_vec(&ordered(v, 0)?).map_err(|e| ContractError::Invalid(e.to_string()))?;
    if bytes.len() > MAX_PAYLOAD_BYTES {
        return Err(ContractError::Limit("canonical payload".into()));
    }
    Ok(bytes)
}
pub fn canonical_digest<T: Serialize>(value: &T) -> Result<Digest> {
    Ok(Digest::of_bytes(&canonical_bytes(value)?))
}
pub fn schema_digest<T: JsonSchema>() -> Result<Digest> {
    canonical_digest(&schemars::schema_for!(T))
}

// Deserialize through a duplicate-detecting visitor before typed deserialization.
// serde_json's ordinary Value parser silently overwrites duplicate keys.
struct StrictValue(Value);
impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = StrictValue;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("bounded JSON with unique keys")
            }
            fn visit_bool<E: serde::de::Error>(
                self,
                v: bool,
            ) -> std::result::Result<Self::Value, E> {
                Ok(StrictValue(Value::Bool(v)))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> std::result::Result<Self::Value, E> {
                Ok(StrictValue(Value::from(v)))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> std::result::Result<Self::Value, E> {
                Ok(StrictValue(Value::from(v)))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> std::result::Result<Self::Value, E> {
                serde_json::Number::from_f64(v)
                    .map(|n| StrictValue(Value::Number(n)))
                    .ok_or_else(|| E::custom("nonfinite number"))
            }
            fn visit_str<E: serde::de::Error>(
                self,
                v: &str,
            ) -> std::result::Result<Self::Value, E> {
                Ok(StrictValue(Value::String(v.into())))
            }
            fn visit_string<E: serde::de::Error>(
                self,
                v: String,
            ) -> std::result::Result<Self::Value, E> {
                Ok(StrictValue(Value::String(v)))
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
                Ok(StrictValue(Value::Null))
            }
            fn visit_none<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
                Ok(StrictValue(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut a: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut out = Vec::new();
                while let Some(v) = a.next_element::<StrictValue>()? {
                    if out.len() >= crate::MAX_ENTRIES {
                        return Err(A::Error::custom("array limit"));
                    }
                    out.push(v.0);
                }
                Ok(StrictValue(Value::Array(out)))
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut a: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut out = serde_json::Map::new();
                while let Some(k) = a.next_key::<String>()? {
                    if out.len() >= crate::MAX_ENTRIES || out.contains_key(&k) {
                        return Err(A::Error::custom("duplicate key or map limit"));
                    }
                    out.insert(k, a.next_value::<StrictValue>()?.0);
                }
                Ok(StrictValue(Value::Object(out)))
            }
        }
        d.deserialize_any(V)
    }
}
pub fn strict_decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    if bytes.len() > MAX_PAYLOAD_BYTES {
        return Err(ContractError::Limit("wire payload".into()));
    }
    let value: StrictValue =
        serde_json::from_slice(bytes).map_err(|e| ContractError::Invalid(e.to_string()))?;
    serde_json::from_value(ordered(value.0, 0)?).map_err(|e| ContractError::Invalid(e.to_string()))
}
