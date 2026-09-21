//! One command registry for CLI, MCP, policy, recipes, docs and plugins.
use semwright_types::{CommandDescriptor, Error, ErrorCode, Result};
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};
pub mod catalog;
pub use catalog::{CatalogQuery, Metadata, SourceKind};

pub const BUILTIN_COMMANDS: &str = include_str!("../../../schemas/commands.json");
#[derive(Clone)]
pub struct Registry {
    commands: BTreeMap<String, CommandDescriptor>,
    metadata: BTreeMap<String, Metadata>,
    validators: BTreeMap<String, (Arc<jsonschema::Validator>, Arc<jsonschema::Validator>)>,
    revision: u64,
}
impl Registry {
    pub fn builtin() -> Result<Self> {
        let descriptors: Vec<CommandDescriptor> = serde_json::from_str(BUILTIN_COMMANDS)?;
        let mut registry = Self::empty();
        for descriptor in descriptors {
            registry.register(descriptor)?;
        }
        Ok(registry)
    }
    pub fn empty() -> Self {
        Self {
            commands: BTreeMap::new(),
            metadata: BTreeMap::new(),
            validators: BTreeMap::new(),
            revision: 0,
        }
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn metadata(&self, command: &str) -> Result<&Metadata> {
        self.metadata.get(command).ok_or_else(|| {
            Error::new(
                ErrorCode::NotFound,
                "Capability provenance is not registered",
            )
        })
    }
    pub fn register(&mut self, descriptor: CommandDescriptor) -> Result<()> {
        let metadata = Metadata::builtin(&descriptor);
        self.register_with_metadata(descriptor, metadata)
    }
    /// Registration is a trusted host operation, never an authority granted by imported text.
    pub fn register_with_metadata(
        &mut self,
        descriptor: CommandDescriptor,
        mut metadata: Metadata,
    ) -> Result<()> {
        if descriptor.name.is_empty()
            || descriptor.name.len() > 128
            || !descriptor.name.bytes().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'.' | b'_' | b'-')
            })
        {
            return Err(Error::invalid(
                "Command names must be lowercase dotted identifiers",
            ));
        }
        if descriptor.requires.is_empty()
            || descriptor.timeout_ms == 0
            || descriptor.timeout_ms > 300_000
        {
            return Err(Error::invalid(
                "Commands must declare capabilities and a bounded timeout",
            ));
        }
        if serde_json::to_vec(&descriptor)?.len() > 262_144 || descriptor.description.len() > 16_384
        {
            return Err(Error::invalid(
                "Capability descriptor exceeds its byte budget",
            ));
        }
        for schema in [&descriptor.input_schema, &descriptor.output_schema] {
            reject_remote_refs(schema)?;
        }
        let input = Arc::new(
            jsonschema::validator_for(&descriptor.input_schema)
                .map_err(|_| Error::invalid("Invalid command input JSON Schema"))?,
        );
        let output = Arc::new(
            jsonschema::validator_for(&descriptor.output_schema)
                .map_err(|_| Error::invalid("Invalid command output JSON Schema"))?,
        );
        metadata.validate_for(&descriptor)?;
        metadata.descriptor_sha256 = catalog::descriptor_digest(&descriptor)?;

        if self.commands.contains_key(&descriptor.name) {
            return Err(Error::new(
                ErrorCode::Conflict,
                "A registered command cannot be overwritten",
            ));
        }
        self.validators
            .insert(descriptor.name.clone(), (input, output));
        self.metadata.insert(descriptor.name.clone(), metadata);
        self.commands.insert(descriptor.name.clone(), descriptor);
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }
    pub fn remove_plugin_command(&mut self, name: &str) -> Result<()> {
        if !name.starts_with("plugin.") {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Built-in commands cannot be removed",
            ));
        }
        self.commands
            .remove(name)
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "Plugin command not registered"))?;
        self.metadata.remove(name);
        self.validators.remove(name);
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }
    pub fn describe(&self, name: &str) -> Result<&CommandDescriptor> {
        self.commands
            .get(name)
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "Command is not registered"))
    }
    pub fn all(&self) -> impl Iterator<Item = &CommandDescriptor> {
        self.commands.values()
    }
    pub fn search(&self, query: &str, limit: usize) -> Vec<&CommandDescriptor> {
        let query = query.to_lowercase();
        let words: Vec<_> = query.split_whitespace().collect();
        self.commands
            .values()
            .filter(|d| {
                let text = format!("{} {}", d.name, d.description).to_lowercase();
                words.iter().all(|w| text.contains(w))
            })
            .take(limit.min(100))
            .collect()
    }
    pub fn validate_input(&self, command: &str, args: &Value) -> Result<()> {
        let validator = &self
            .validators
            .get(command)
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "Command is not registered"))?
            .0;
        if validator.is_valid(args) {
            Ok(())
        } else {
            Err(Error::invalid(
                "Command arguments do not match the published schema",
            ))
        }
    }
    pub fn validate_output(&self, command: &str, value: &Value) -> Result<()> {
        let validator = &self
            .validators
            .get(command)
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "Command is not registered"))?
            .1;
        if validator.is_valid(value) {
            Ok(())
        } else {
            Err(Error::new(
                ErrorCode::BackendFailed,
                "Backend output violates its published contract",
            ))
        }
    }
}
fn reject_remote_refs(value: &Value) -> Result<()> {
    match value {
        Value::Object(map) => {
            for (key, v) in map {
                if matches!(key.as_str(), "$ref" | "$dynamicRef" | "$recursiveRef")
                    && !v.as_str().is_some_and(|s| s.starts_with('#'))
                {
                    return Err(Error::invalid("Remote schema references are disabled"));
                }
                reject_remote_refs(v)?;
            }
        }
        Value::Array(values) => {
            for v in values {
                reject_remote_refs(v)?;
            }
        }
        _ => (),
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_schema_builds() {
        let r = Registry::builtin().unwrap();
        assert!(r.all().count() > 70);
    }
    #[test]
    fn no_client_backend_internals() {
        let r = Registry::builtin().unwrap();
        assert!(
            r.validate_input(
                "ui.invoke",
                &serde_json::json!({"_target":{},"ref":"ui:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"})
            )
            .is_err()
        );
    }
    #[test]
    fn schemas_reject_unbounded_snapshots() {
        let r = Registry::builtin().unwrap();
        assert!(
            r.validate_input("ui.snapshot", &serde_json::json!({"max_nodes":1000000}))
                .is_err()
        );
    }
    #[test]
    fn no_duplicate_registration() {
        let mut r = Registry::builtin().unwrap();
        let c = r.describe("doctor").unwrap().clone();
        assert!(r.register(c).is_err());
    }
    #[test]
    fn remote_schema_is_forbidden() {
        assert!(
            reject_remote_refs(&serde_json::json!({"$ref":"https://untrusted.example/schema"}))
                .is_err()
        );
    }
    #[test]
    fn discovery_is_bounded() {
        let r = Registry::builtin().unwrap();
        assert_eq!(r.search("", 2).len(), 2);
        assert!(!r.search("blender material", 100).is_empty());
    }
}
