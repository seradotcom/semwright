//! Protocol-independent provider identity and invocation provenance.
use crate::{Error, ErrorCode, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Builtin,
    Plugin,
    Driver,
    ExternalMcp,
    Recipe,
}
impl SourceKind {
    pub fn prefix(self) -> Option<&'static str> {
        match self {
            Self::Builtin => None,
            Self::Plugin => Some("plugin"),
            Self::Driver => Some("driver"),
            Self::ExternalMcp => Some("external-mcp"),
            Self::Recipe => Some("recipe"),
        }
    }
    pub fn namespace_prefix(self) -> Option<&'static str> {
        match self {
            Self::ExternalMcp => Some("external"),
            other => other.prefix(),
        }
    }
}
/// Assigned by the owner-loaded host, not taken from an upstream's self-description.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProviderIdentity {
    pub id: String,
    pub kind: SourceKind,
    pub version: String,
    pub namespace: String,
    pub application: Option<String>,
    pub origin: String,
}
pub fn canonical_slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 40
        && value.as_bytes()[0].is_ascii_lowercase()
        && value.as_bytes()[value.len() - 1].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}
impl ProviderIdentity {
    pub fn external(kind: SourceKind, slug: &str, version: &str) -> Result<Self> {
        let prefix = kind.prefix().ok_or_else(|| {
            Error::new(
                ErrorCode::PolicyDenied,
                "External providers cannot claim builtin authority",
            )
        })?;
        if !canonical_slug(slug) {
            return Err(Error::invalid("Provider slug is not canonical"));
        }
        let identity = Self {
            id: format!("{prefix}:{slug}"),
            kind,
            version: version.into(),
            namespace: format!("{}.{slug}.", kind.namespace_prefix().unwrap_or(prefix)),
            application: None,
            origin: "owner-configured".into(),
        };
        identity.validate_external()?;
        Ok(identity)
    }
    pub fn validate_external(&self) -> Result<()> {
        let prefix = self.kind.prefix().ok_or_else(|| {
            Error::new(
                ErrorCode::PolicyDenied,
                "Dynamic registration cannot claim builtin authority",
            )
        })?;
        let slug = self
            .id
            .strip_prefix(&format!("{prefix}:"))
            .filter(|s| canonical_slug(s))
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::PolicyDenied,
                    "Provider identity does not match its kind",
                )
            })?;
        if self.namespace != format!("{}.{slug}.", self.kind.namespace_prefix().unwrap_or(prefix)) {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Provider namespace does not belong to its identity",
            ));
        }
        if self.version.is_empty()
            || self.version.len() > 80
            || self.version.chars().any(char::is_control)
            || self.origin.is_empty()
            || self.origin.len() > 256
            || self.origin.chars().any(char::is_control)
            || self
                .application
                .as_ref()
                .is_some_and(|s| s.is_empty() || s.len() > 256 || s.chars().any(char::is_control))
        {
            return Err(Error::invalid("Provider identity metadata exceeds bounds"));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InvocationProvenance {
    pub provider: String,
    pub source: SourceKind,
    pub provider_version: String,
    pub capability_version: String,
    pub descriptor_sha256: String,
    pub untrusted_metadata: bool,
    pub catalog_revision: u64,
    pub execution_provider: Option<String>,
    pub provider_generation: Option<u64>,
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn provider_identity_is_canonical_and_cannot_claim_core() {
        for slug in [
            "",
            "../core",
            "semwright:core",
            "Core",
            "-bad",
            "bad-",
            "x.y",
            "a/b",
        ] {
            assert!(ProviderIdentity::external(SourceKind::Driver, slug, "1").is_err());
        }
        assert!(ProviderIdentity::external(SourceKind::Builtin, "core", "1").is_err());
        let mut id =
            ProviderIdentity::external(SourceKind::ExternalMcp, "playwright", "1").unwrap();
        assert_eq!(id.namespace, "external.playwright.");
        id.id = "semwright-core".into();
        assert!(id.validate_external().is_err());
    }
    #[test]
    fn identity_namespace_and_unknown_fields_are_rejected() {
        let mut id = ProviderIdentity::external(SourceKind::Driver, "fixture", "1").unwrap();
        id.namespace = "driver.other.".into();
        assert!(id.validate_external().is_err());
        let mut value = serde_json::to_value(id).unwrap();
        value["trusted"] = serde_json::json!(true);
        assert!(serde_json::from_value::<ProviderIdentity>(value).is_err());
    }
}
