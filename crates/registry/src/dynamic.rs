//! Atomic provider-owned catalog replacement and immutable invocation snapshots.
use crate::{Metadata, Registry, SourceKind, bounds};
use semwright_types::{
    CommandDescriptor, Error, ErrorCode, InvocationProvenance, ProviderIdentity, Result,
};
use serde_json::Value;
use std::sync::Arc;

#[derive(Clone)]
pub struct CapabilitySnapshot {
    pub descriptor: CommandDescriptor,
    pub metadata: Metadata,
    pub revision: u64,
    input: Arc<jsonschema::Validator>,
    output: Arc<jsonschema::Validator>,
}
impl CapabilitySnapshot {
    pub fn validate_input(&self, args: &Value) -> Result<()> {
        bounds::value_budget(args)?;
        if self.input.is_valid(args) {
            Ok(())
        } else {
            Err(Error::invalid(
                "Arguments do not match the pinned capability schema",
            ))
        }
    }
    pub fn validate_output(&self, value: &Value) -> Result<()> {
        bounds::value_budget(value)?;
        if self.output.is_valid(value) {
            Ok(())
        } else {
            Err(Error::new(
                ErrorCode::BackendFailed,
                "Result violates the pinned capability schema",
            ))
        }
    }
    pub fn provenance(&self) -> InvocationProvenance {
        InvocationProvenance {
            provider: self.metadata.provider.clone(),
            source: self.metadata.source,
            provider_version: self.metadata.source_version.clone(),
            capability_version: self.descriptor.version.clone(),
            descriptor_sha256: self.metadata.descriptor_sha256.clone(),
            untrusted_metadata: self.metadata.untrusted_metadata,
            catalog_revision: self.revision,
            execution_provider: None,
            provider_generation: None,
        }
    }
}
impl Registry {
    pub fn snapshot(&self, name: &str) -> Result<CapabilitySnapshot> {
        let (input, output) = self
            .validators
            .get(name)
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "Capability is not registered"))?;
        Ok(CapabilitySnapshot {
            descriptor: self.describe(name)?.clone(),
            metadata: self.metadata(name)?.clone(),
            revision: self.revision,
            input: input.clone(),
            output: output.clone(),
        })
    }
    pub fn touch(&mut self, expected_revision: u64) -> Result<u64> {
        if self.revision != expected_revision {
            return Err(Error::new(ErrorCode::Conflict, "Catalog revision changed"));
        }
        self.revision = self.revision.checked_add(1).ok_or_else(|| {
            Error::new(ErrorCode::ResourceExhausted, "Catalog revision exhausted")
        })?;
        Ok(self.revision)
    }
    pub fn replace_provider_catalog(
        &mut self,
        identity: &ProviderIdentity,
        commands: Vec<(CommandDescriptor, Metadata)>,
        expected_revision: u64,
        replace: bool,
    ) -> Result<u64> {
        identity.validate_external()?;
        if self.revision != expected_revision {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Catalog revision changed before registration",
            ));
        }
        if commands.len() > 2048 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Provider has too many capabilities",
            ));
        }
        let owned = self
            .metadata
            .iter()
            .filter(|(_, m)| m.provider == identity.id)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        if !replace && !owned.is_empty() {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Provider catalog already exists",
            ));
        }
        let mut candidate = self.clone();
        for name in owned {
            candidate.remove_owned(&name)?;
        }
        for (command, metadata) in commands {
            if metadata.provider != identity.id
                || metadata.source != identity.kind
                || metadata.source_version != identity.version
                || metadata.app != identity.application
                || !metadata.untrusted_metadata
                || !command.name.starts_with(&identity.namespace)
                || command.name == identity.namespace
                || command.backends != [identity.id.as_str()]
                || !command.requires.contains(&identity.id)
            {
                return Err(Error::new(
                    ErrorCode::PolicyDenied,
                    "Capability provenance or route does not match its registering provider",
                ));
            }
            candidate.register_with_metadata(command, metadata)?;
        }
        candidate.revision = expected_revision.checked_add(1).ok_or_else(|| {
            Error::new(ErrorCode::ResourceExhausted, "Catalog revision exhausted")
        })?;
        *self = candidate;
        Ok(self.revision)
    }
    pub fn remove_provider_catalog(
        &mut self,
        identity: &ProviderIdentity,
        expected_revision: u64,
    ) -> Result<u64> {
        self.replace_provider_catalog(identity, vec![], expected_revision, true)
    }
    pub(super) fn remove_owned(&mut self, name: &str) -> Result<()> {
        if self.metadata(name)?.source == SourceKind::Builtin {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Builtin capability ownership is immutable",
            ));
        }
        let bytes = serde_json::to_vec(self.describe(name)?)?.len()
            + serde_json::to_vec(self.metadata(name)?)?.len();
        self.commands.remove(name);
        self.metadata.remove(name);
        self.validators.remove(name);
        self.descriptor_bytes = self.descriptor_bytes.saturating_sub(bytes);
        Ok(())
    }
}
