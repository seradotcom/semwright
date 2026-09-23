use crate::{Fault, FaultKind, Result};
use serde::de::{DeserializeSeed, Error as DeError, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};
use std::{cell::Cell, fmt};

pub const MAX_FRAME: usize = 262_144;
pub const MAX_EVENT: usize = 16_384;
pub const MAX_DEPTH: usize = 32;
pub const MAX_NODES: usize = 8192;
pub const MAX_ARRAY: usize = 2048;
pub const MAX_MAP: usize = 128;
pub const MAX_STRING: usize = 4096;
pub const MAX_NAME: usize = 512;

struct Seed<'a> {
    depth: usize,
    count: &'a Cell<usize>,
}
struct BoundedVisitor<'a>(Seed<'a>);

impl<'de> DeserializeSeed<'de> for Seed<'_> {
    type Value = Value;
    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<Value, D::Error> {
        let count = self.count.get().saturating_add(1);
        self.count.set(count);
        if count > MAX_NODES || self.depth > MAX_DEPTH {
            return Err(D::Error::custom("JSON budget"));
        }
        deserializer.deserialize_any(BoundedVisitor(self))
    }
}
impl BoundedVisitor<'_> {
    fn child(&self) -> Seed<'_> {
        Seed {
            depth: self.0.depth + 1,
            count: self.0.count,
        }
    }
}
impl<'de> Visitor<'de> for BoundedVisitor<'_> {
    type Value = Value;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("bounded JSON")
    }
    fn visit_bool<E: DeError>(self, x: bool) -> std::result::Result<Value, E> {
        Ok(Value::Bool(x))
    }
    fn visit_i64<E: DeError>(self, x: i64) -> std::result::Result<Value, E> {
        Ok(Value::Number(x.into()))
    }
    fn visit_u64<E: DeError>(self, x: u64) -> std::result::Result<Value, E> {
        Ok(Value::Number(x.into()))
    }
    fn visit_f64<E: DeError>(self, x: f64) -> std::result::Result<Value, E> {
        Number::from_f64(x)
            .map(Value::Number)
            .ok_or_else(|| E::custom("finite number required"))
    }
    fn visit_str<E: DeError>(self, x: &str) -> std::result::Result<Value, E> {
        if x.len() > MAX_STRING {
            return Err(E::custom("string budget"));
        }
        Ok(Value::String(x.to_owned()))
    }
    fn visit_string<E: DeError>(self, x: String) -> std::result::Result<Value, E> {
        if x.len() > MAX_STRING {
            return Err(E::custom("string budget"));
        }
        Ok(Value::String(x))
    }
    fn visit_none<E: DeError>(self) -> std::result::Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_unit<E: DeError>(self) -> std::result::Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<Value, A::Error> {
        let mut out = Vec::new();
        while let Some(value) = seq.next_element_seed(self.child())? {
            if out.len() >= MAX_ARRAY {
                return Err(A::Error::custom("array budget"));
            }
            out.push(value);
        }
        Ok(Value::Array(out))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<Value, A::Error> {
        let mut out = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if key.len() > MAX_NAME || out.len() >= MAX_MAP || out.contains_key(&key) {
                return Err(A::Error::custom("map budget or duplicate key"));
            }
            let value = map.next_value_seed(self.child())?;
            out.insert(key, value);
        }
        Ok(Value::Object(out))
    }
}

pub fn parse(bytes: &[u8], max: usize) -> Result<Value> {
    if bytes.is_empty() || bytes.len() > max.min(MAX_FRAME) {
        return Err(Fault::new(FaultKind::ResourceLimit));
    }
    let count = Cell::new(0);
    let mut de = serde_json::Deserializer::from_slice(bytes);
    let value = Seed {
        depth: 0,
        count: &count,
    }
    .deserialize(&mut de)
    .map_err(|_| Fault::new(FaultKind::Protocol))?;
    de.end().map_err(|_| Fault::new(FaultKind::Protocol))?;
    Ok(value)
}

pub fn check(value: &Value, max: usize) -> Result<()> {
    let mut pending = vec![(value, 0usize)];
    let mut nodes = 0usize;
    let mut minimum_bytes = 0usize;
    while let Some((value, depth)) = pending.pop() {
        nodes += 1;
        if nodes > MAX_NODES || depth > MAX_DEPTH {
            return Err(Fault::new(FaultKind::ResourceLimit));
        }
        match value {
            Value::Object(map) => {
                if map.len() > MAX_MAP || map.keys().any(|key| key.len() > MAX_NAME) {
                    return Err(Fault::new(FaultKind::ResourceLimit));
                }
                minimum_bytes =
                    minimum_bytes.saturating_add(map.keys().map(String::len).sum::<usize>());
                pending.extend(map.values().map(|child| (child, depth + 1)));
            }
            Value::Array(array) => {
                if array.len() > MAX_ARRAY {
                    return Err(Fault::new(FaultKind::ResourceLimit));
                }
                pending.extend(array.iter().map(|child| (child, depth + 1)));
            }
            Value::String(text) => {
                if text.len() > MAX_STRING {
                    return Err(Fault::new(FaultKind::ResourceLimit));
                }
                minimum_bytes = minimum_bytes.saturating_add(text.len());
            }
            _ => {}
        }
        if pending.len() > MAX_NODES || minimum_bytes > max.min(MAX_FRAME) {
            return Err(Fault::new(FaultKind::ResourceLimit));
        }
    }
    let bytes = serde_json::to_vec(value)?;
    if bytes.is_empty() || bytes.len() > max.min(MAX_FRAME) {
        return Err(Fault::new(FaultKind::ResourceLimit));
    }
    Ok(())
}

pub fn name(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > MAX_NAME
        || value.chars().any(|c| c.is_control() || is_bidi(c))
    {
        Err(Fault::new(FaultKind::Configuration))
    } else {
        Ok(())
    }
}
fn is_bidi(c: char) -> bool {
    matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}
pub fn display(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_control() || is_bidi(c) {
            out.push('�');
        } else {
            out.push(c);
        }
        if out.len() >= MAX_NAME {
            break;
        }
    }
    out
}
pub fn scrub(value: &Value) -> Value {
    match value {
        Value::String(s) => Value::String(display(s)),
        Value::Array(a) => Value::Array(a.iter().map(scrub).collect()),
        Value::Object(o) => Value::Object(
            o.iter()
                .filter(|(k, _)| {
                    let k = k.to_ascii_lowercase();
                    k != "$ref"
                        && ![
                            "password",
                            "secret",
                            "token",
                            "authorization",
                            "streamkey",
                            "authentication",
                            "salt",
                            "challenge",
                        ]
                        .iter()
                        .any(|term| k.contains(term))
                })
                .map(|(k, v)| (display(k), scrub(v)))
                .collect(),
        ),
        other => other.clone(),
    }
}
