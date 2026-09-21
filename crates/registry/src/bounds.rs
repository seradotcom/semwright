//! Resource limits enforced before schema compilation and synchronous validation.
use semwright_types::{Error, ErrorCode, MAX_FRAME, Result};
use serde_json::Value;

#[derive(Clone, Copy)]
enum Position {
    Schema,
    Map,
    Dependencies,
    Array,
    Data,
}
const MAX_SCHEMA_NODES: usize = 4096;
const MAX_VALUE_NODES: usize = 16384;

/// External schemas support bounded, acyclic local definitions. Dynamic resolution is not enabled.
pub fn schema_budget(root: &Value, external: bool) -> Result<()> {
    struct Walk<'a> {
        root: &'a Value,
        external: bool,
        nodes: usize,
        references: Vec<&'a str>,
    }
    impl<'a> Walk<'a> {
        fn visit(&mut self, value: &'a Value, position: Position, depth: usize) -> Result<()> {
            self.nodes += 1;
            if depth > 32 || self.nodes > MAX_SCHEMA_NODES {
                return Err(Error::new(
                    ErrorCode::ResourceExhausted,
                    "Schema structural budget exceeded",
                ));
            }
            match value {
                Value::Object(map) => {
                    if matches!(position, Position::Schema) {
                        if let Some(reference) = map.get("$ref") {
                            let reference = reference
                                .as_str()
                                .ok_or_else(|| Error::invalid("Invalid schema reference"))?;
                            if !reference.starts_with('#') {
                                return Err(Error::invalid(
                                    "Remote schema references are disabled",
                                ));
                            }
                            if self.external {
                                if self.references.contains(&reference) {
                                    return Err(Error::invalid(
                                        "Recursive external schemas are unsupported",
                                    ));
                                }
                                let target = if reference == "#" {
                                    self.root
                                } else {
                                    self.root
                                        .pointer(reference.strip_prefix('#').unwrap_or(""))
                                        .ok_or_else(|| {
                                            Error::invalid("Unresolved local schema reference")
                                        })?
                                };
                                self.references.push(reference);
                                self.visit(target, Position::Schema, depth + 1)?;
                                self.references.pop();
                            }
                        }
                        for key in ["$dynamicRef", "$recursiveRef"] {
                            if let Some(reference) = map.get(key)
                                && (self.external
                                    || !reference.as_str().is_some_and(|s| s.starts_with('#')))
                            {
                                return Err(Error::invalid(
                                    "Dynamic schema references are disabled",
                                ));
                            }
                        }
                        if self.external {
                            if ["$id", "$anchor", "$dynamicAnchor"]
                                .iter()
                                .any(|key| map.contains_key(*key))
                            {
                                return Err(Error::invalid(
                                    "External schema identity overrides are disabled",
                                ));
                            }
                            if let Some(dialect) = map.get("$schema")
                                && !matches!(
                                    dialect.as_str(),
                                    Some(
                                        "https://json-schema.org/draft/2020-12/schema"
                                            | "http://json-schema.org/draft-07/schema#"
                                            | "https://json-schema.org/draft-07/schema#"
                                    )
                                )
                            {
                                return Err(Error::invalid(
                                    "External schema dialect is unsupported",
                                ));
                            }
                            if let Some(pattern) = map.get("pattern") {
                                safe_pattern(
                                    pattern
                                        .as_str()
                                        .ok_or_else(|| Error::invalid("Invalid schema pattern"))?,
                                )?;
                            }
                            if let Some(patterns) =
                                map.get("patternProperties").and_then(Value::as_object)
                            {
                                for pattern in patterns.keys() {
                                    safe_pattern(pattern)?;
                                }
                            }
                            for key in ["allOf", "oneOf", "anyOf"] {
                                if map
                                    .get(key)
                                    .and_then(Value::as_array)
                                    .is_some_and(|a| a.len() > 16)
                                {
                                    return Err(Error::invalid(
                                        "Schema has too many combinator branches",
                                    ));
                                }
                            }
                        }
                    }
                    for (key, child) in map {
                        let next = match position {
                            Position::Map => Position::Schema,
                            Position::Dependencies => {
                                if child.is_object() {
                                    Position::Schema
                                } else {
                                    Position::Data
                                }
                            }
                            Position::Schema => match key.as_str() {
                                "properties" | "patternProperties" | "$defs" | "definitions"
                                | "dependentSchemas" => Position::Map,
                                "dependencies" => Position::Dependencies,
                                "allOf" | "anyOf" | "oneOf" | "prefixItems" => Position::Array,
                                "items" if child.is_array() => Position::Array,
                                "items"
                                | "additionalItems"
                                | "additionalProperties"
                                | "if"
                                | "then"
                                | "else"
                                | "not"
                                | "contains"
                                | "unevaluatedItems"
                                | "unevaluatedProperties"
                                | "propertyNames" => Position::Schema,
                                _ => Position::Data,
                            },
                            _ => Position::Data,
                        };
                        self.visit(child, next, depth + 1)?;
                    }
                }
                Value::Array(values) => {
                    let next = if matches!(position, Position::Array) {
                        Position::Schema
                    } else {
                        Position::Data
                    };
                    for child in values {
                        self.visit(child, next, depth + 1)?;
                    }
                }
                _ => (),
            }
            Ok(())
        }
    }
    Walk {
        root,
        external,
        nodes: 0,
        references: vec![],
    }
    .visit(root, Position::Schema, 0)
}
fn safe_pattern(pattern: &str) -> Result<()> {
    if pattern.len() > 256
        || regex::RegexBuilder::new(pattern)
            .size_limit(65536)
            .build()
            .is_err()
    {
        return Err(Error::invalid(
            "External schema pattern is outside the bounded regular-expression subset",
        ));
    }
    Ok(())
}
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
