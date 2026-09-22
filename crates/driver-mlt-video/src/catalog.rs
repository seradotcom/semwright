//! Static curated catalog, strict schemas and observed serde struct digest ordering.
use crate::{
    Error, Result,
    hash::sha256,
    json::{self, Value},
};
pub const PROVIDER: &str = "driver:mlt-video";
pub const PREFIX: &str = "driver.mlt-video.";
const DESCRIPTOR_FIELDS: [&str; 12] = [
    "name",
    "version",
    "description",
    "input_schema",
    "output_schema",
    "requires",
    "risk",
    "idempotency",
    "timeout_ms",
    "dry_run",
    "interactive_consent",
    "backends",
];
#[derive(Clone, Debug)]
pub struct Capability {
    pub name: String,
    pub descriptor: Value,
    pub digest: String,
    pub wire: String,
}
impl Capability {
    pub fn input(&self, args: &Value) -> Result<()> {
        validate(self.descriptor.get("input_schema")?, args, 0)
    }
    pub fn output(&self, value: &Value) -> Result<()> {
        validate(self.descriptor.get("output_schema")?, value, 0).map_err(|_| {
            Error::new(
                "BackendFailed",
                "Result does not match the pinned output schema",
            )
        })
    }
    pub fn mutates(&self) -> bool {
        self.descriptor.str("risk").is_ok_and(|s| s != "read_only")
    }
}
pub fn capabilities() -> Result<Vec<Capability>> {
    let source = json::parse(include_bytes!("catalog.json"))?;
    let mut out = vec![];
    for item in source.as_array()? {
        let d = item.get("descriptor")?.clone();
        let fields: Vec<_> = DESCRIPTOR_FIELDS
            .iter()
            .map(|key| d.get(key).map(|v| (*key, v.encode())))
            .collect::<Result<_>>()?;
        let descriptor = json::ordered(&fields);
        let name = d.str("name")?.to_string();
        if !name.starts_with(PREFIX)
            || d.get("backends")?.as_array() != Ok(&[Value::from(PROVIDER)][..])
        {
            return Err(Error::invalid("Catalog identity mismatch"));
        }
        let wire = json::ordered(&[
            ("descriptor", descriptor.clone()),
            ("aliases", item.get("aliases")?.encode()),
            ("tags", item.get("tags")?.encode()),
            ("object_types", item.get("object_types")?.encode()),
        ]);
        out.push(Capability {
            name,
            descriptor: d,
            digest: sha256(descriptor.as_bytes()),
            wire,
        });
    }
    Ok(out)
}
pub fn catalog_wire(capabilities: &[Capability]) -> String {
    format!(
        "[{}]",
        capabilities
            .iter()
            .map(|c| c.wire.as_str())
            .collect::<Vec<_>>()
            .join(",")
    )
}
pub fn digest(capabilities: &[Capability]) -> String {
    sha256(catalog_wire(capabilities).as_bytes())
}
/// Only the schema keywords used by this static catalog are implemented. No remote schema loader.
pub fn validate(schema: &Value, v: &Value, depth: usize) -> Result<()> {
    if depth > 32 {
        return Err(Error::limit("Schema validation depth exceeded"));
    }
    if let Some(t) = schema.opt("type") {
        let types: Vec<&str> = match t {
            Value::String(s) => vec![s],
            Value::Array(a) => a.iter().map(Value::string).collect::<Result<_>>()?,
            _ => return Err(Error::invalid("Invalid schema type")),
        };
        let actual = match v {
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Number(s) => {
                if !s.contains(['.', 'e', 'E']) {
                    "integer"
                } else {
                    "number"
                }
            }
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
        };
        if !types.contains(&actual) {
            return Err(Error::invalid("Field type violates schema"));
        }
    }
    if let Some(expected) = schema.opt("const") {
        if v != expected {
            return Err(Error::invalid("Constant field mismatch"));
        }
    }
    if let Some(values) = schema.opt("enum") {
        if !values.as_array()?.contains(v) {
            return Err(Error::invalid("Field outside enumeration"));
        }
    }
    match v {
        Value::String(s) => {
            if schema
                .opt("maxLength")
                .and_then(|n| n.u64().ok())
                .is_some_and(|max| s.chars().count() as u64 > max)
            {
                return Err(Error::limit("String field exceeds schema"));
            }
        }
        Value::Number(n) => {
            let number = n.parse::<i128>().map_err(|_| {
                Error::invalid("This catalog only accepts bounded integer arguments")
            })?;
            for (key, is_min) in [("minimum", true), ("maximum", false)] {
                if let Some(Value::Number(bound)) = schema.opt(key) {
                    let b = bound
                        .parse::<i128>()
                        .map_err(|_| Error::invalid("Invalid numeric schema bound"))?;
                    if (is_min && number < b) || (!is_min && number > b) {
                        return Err(Error::invalid("Number outside schema range"));
                    }
                }
            }
        }
        Value::Array(a) => {
            if schema
                .opt("maxItems")
                .and_then(|n| n.u64().ok())
                .is_some_and(|max| a.len() as u64 > max)
            {
                return Err(Error::limit("Array exceeds schema budget"));
            }
            if let Some(items) = schema.opt("items") {
                for value in a {
                    validate(items, value, depth + 1)?;
                }
            }
        }
        Value::Object(m) => {
            let properties = schema.get("properties")?.object()?;
            if let Some(required) = schema.opt("required") {
                for key in required.as_array()? {
                    if !m.contains_key(key.string()?) {
                        return Err(Error::invalid("Required argument missing"));
                    }
                }
            }
            if schema.opt("additionalProperties") == Some(&Value::Bool(false))
                && m.keys().any(|key| !properties.contains_key(key))
            {
                return Err(Error::invalid("Unknown argument"));
            }
            for (key, value) in m {
                if let Some(s) = properties.get(key) {
                    validate(s, value, depth + 1)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}
