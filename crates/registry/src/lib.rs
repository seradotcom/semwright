//! One command registry for CLI, MCP, policy, recipes, docs and plugins.
use semwright_types::{CommandDescriptor, Error, ErrorCode, Result};
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};
pub mod bounds;
pub mod catalog;
mod dynamic;
pub use catalog::{CatalogQuery, Metadata, SourceKind};
pub use dynamic::CapabilitySnapshot;

pub const BUILTIN_COMMANDS: &str = include_str!("../../../schemas/commands.json");
#[derive(Clone)]
pub struct Registry {
    commands: BTreeMap<String, CommandDescriptor>,
    metadata: BTreeMap<String, Metadata>,
    validators: BTreeMap<String, (Arc<jsonschema::Validator>, Arc<jsonschema::Validator>)>,
    revision: u64,
    descriptor_bytes: usize,
}
impl Registry {
    pub fn builtin() -> Result<Self> {
        let descriptors: Vec<CommandDescriptor> = serde_json::from_str(BUILTIN_COMMANDS)?;
        let mut provenance: BTreeMap<String, Metadata> =
            serde_json::from_str(include_str!("../../../schemas/builtin-provenance.json"))?;
        let mut registry = Self::empty();
        for descriptor in descriptors {
            let metadata = provenance
                .remove(&descriptor.name)
                .ok_or_else(|| Error::invalid("Builtin descriptor has no explicit provenance"))?;
            registry.register_with_metadata(descriptor, metadata)?;
        }
        if !provenance.is_empty() {
            return Err(Error::invalid(
                "Builtin provenance contains an unregistered capability",
            ));
        }
        Ok(registry)
    }
    pub fn empty() -> Self {
        Self {
            commands: BTreeMap::new(),
            metadata: BTreeMap::new(),
            validators: BTreeMap::new(),
            revision: 0,
            descriptor_bytes: 0,
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
        let metadata = Metadata::builtin(&descriptor)?;
        self.register_with_metadata(descriptor, metadata)
    }
    /// Registration is a trusted host operation, never an authority granted by imported text.
    pub fn register_with_metadata(
        &mut self,
        descriptor: CommandDescriptor,
        mut metadata: Metadata,
    ) -> Result<()> {
        if self.commands.len() >= 8192 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Capability catalog is full",
            ));
        }
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
            || descriptor.requires.len() > 32
            || descriptor.requires.iter().any(|s| {
                s.is_empty()
                    || s.len() > 128
                    || !s.bytes().all(|b| {
                        b.is_ascii_lowercase()
                            || b.is_ascii_digit()
                            || matches!(b, b'.' | b':' | b'_' | b'-')
                    })
            })
            || descriptor.backends.is_empty()
            || descriptor.backends.len() > 16
            || descriptor
                .backends
                .iter()
                .any(|s| s.is_empty() || s.len() > 128 || s.chars().any(char::is_control))
            || descriptor.version.is_empty()
            || descriptor.version.len() > 80
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
        metadata.validate_for(&descriptor)?;
        for schema in [&descriptor.input_schema, &descriptor.output_schema] {
            bounds::schema_budget(schema, metadata.untrusted_metadata)?;
        }
        let input = Arc::new(
            jsonschema::validator_for(&descriptor.input_schema)
                .map_err(|_| Error::invalid("Invalid command input JSON Schema"))?,
        );
        let output = Arc::new(
            jsonschema::validator_for(&descriptor.output_schema)
                .map_err(|_| Error::invalid("Invalid command output JSON Schema"))?,
        );
        metadata.descriptor_sha256 = catalog::descriptor_digest(&descriptor)?;

        if self.commands.contains_key(&descriptor.name) {
            return Err(Error::new(
                ErrorCode::Conflict,
                "A registered command cannot be overwritten",
            ));
        }
        let bytes = serde_json::to_vec(&descriptor)?.len() + serde_json::to_vec(&metadata)?.len();
        if self.descriptor_bytes.saturating_add(bytes) > 16 * 1024 * 1024 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Capability catalog exceeds its aggregate byte budget",
            ));
        }
        let next_revision = self.revision.checked_add(1).ok_or_else(|| {
            Error::new(ErrorCode::ResourceExhausted, "Catalog revision exhausted")
        })?;
        self.validators
            .insert(descriptor.name.clone(), (input, output));
        self.metadata.insert(descriptor.name.clone(), metadata);
        self.commands.insert(descriptor.name.clone(), descriptor);
        self.revision = next_revision;
        self.descriptor_bytes += bytes;
        Ok(())
    }
    pub fn remove_plugin_command(&mut self, name: &str) -> Result<()> {
        if self.metadata(name)?.source != SourceKind::Plugin {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Only registered plugin-owned commands can be removed",
            ));
        }
        let next = self.revision.checked_add(1).ok_or_else(|| {
            Error::new(ErrorCode::ResourceExhausted, "Catalog revision exhausted")
        })?;
        self.remove_owned(name)?;
        self.revision = next;
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
        bounds::value_budget(args)?;
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
        bounds::value_budget(value)?;
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
            bounds::schema_budget(
                &serde_json::json!({"$ref":"https://untrusted.example/schema"}),
                true
            )
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
