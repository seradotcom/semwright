//! Deterministic, bounded capability discovery. Metadata never changes authorization.
use crate::Registry;
use semwright_types::{CommandDescriptor, Error, ErrorCode, Result, Risk};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Builtin,
    Plugin,
    Driver,
    ExternalMcp,
    Recipe,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub source: SourceKind,
    pub provider: String,
    pub source_version: String,
    pub app: Option<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub object_types: Vec<String>,
    pub untrusted_metadata: bool,
    #[serde(default)]
    pub descriptor_sha256: String,
}
impl Metadata {
    pub fn builtin(command: &CommandDescriptor) -> Self {
        let parts: Vec<_> = command.name.split('.').collect();
        let source = if parts[0] == "plugin" {
            SourceKind::Plugin
        } else {
            SourceKind::Builtin
        };
        let provider = if source == SourceKind::Plugin {
            format!("plugin:{}", parts.get(1).unwrap_or(&"invalid"))
        } else {
            match parts[0] {
                "blender" => "blender-native".into(),
                "browser" => "browser-cdp".into(),
                _ => "semwright-core".into(),
            }
        };
        Self {
            source,
            provider,
            source_version: command.version.clone(),
            app: match parts[0] {
                "blender" => Some("org.blender.Blender".into()),
                "browser" => Some("org.semwright.Chromium".into()),
                _ => None,
            },
            aliases: vec![],
            tags: parts.iter().take(2).map(|s| (*s).to_owned()).collect(),
            object_types: parts
                .get(1)
                .filter(|s| {
                    matches!(
                        **s,
                        "object" | "window" | "node" | "material" | "scene" | "tab" | "collection"
                    )
                })
                .map(|s| vec![(*s).into()])
                .unwrap_or_default(),
            untrusted_metadata: source != SourceKind::Builtin,
            descriptor_sha256: String::new(),
        }
    }
    pub fn validate_for(&self, command: &CommandDescriptor) -> Result<()> {
        if self.provider.is_empty()
            || self.provider.len() > 128
            || self.source_version.len() > 128
            || self.app.as_ref().is_some_and(|s| s.len() > 256)
            || [&self.aliases, &self.tags, &self.object_types]
                .iter()
                .any(|values| {
                    values.len() > 32
                        || values.iter().any(|s| {
                            s.is_empty() || s.len() > 128 || s.chars().any(char::is_control)
                        })
                })
        {
            return Err(Error::invalid("Capability metadata exceeds its bounds"));
        }
        if self.source != SourceKind::Builtin && !self.untrusted_metadata {
            return Err(Error::invalid(
                "Third-party metadata must be labelled untrusted",
            ));
        }
        let namespace = match self.source {
            SourceKind::ExternalMcp => Some("external."),
            SourceKind::Plugin => Some("plugin."),
            SourceKind::Driver => Some("driver."),
            SourceKind::Recipe => Some("recipe."),
            SourceKind::Builtin => None,
        };
        if namespace.is_some_and(|prefix| !command.name.starts_with(prefix)) {
            return Err(Error::invalid(
                "Capability source cannot claim another source namespace",
            ));
        }
        Ok(())
    }
}
pub fn descriptor_digest(command: &CommandDescriptor) -> Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(command)?)
    ))
}
fn default_limit() -> usize {
    20
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogQuery {
    #[serde(default)]
    pub query: String,
    pub provider: Option<String>,
    pub app: Option<String>,
    pub category: Option<String>,
    pub risk: Option<Risk>,
    pub available: Option<bool>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub object_types: Vec<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
    pub revision: Option<u64>,
}
impl Default for CatalogQuery {
    fn default() -> Self {
        Self {
            query: String::new(),
            provider: None,
            app: None,
            category: None,
            risk: None,
            available: None,
            tags: vec![],
            object_types: vec![],
            limit: 20,
            offset: 0,
            revision: None,
        }
    }
}
impl CatalogQuery {
    pub fn validate(&self) -> Result<Vec<String>> {
        if self.query.len() > 256
            || self.limit == 0
            || self.limit > 100
            || self.offset > 8192
            || self.tags.len() > 16
            || self.object_types.len() > 16
            || self
                .tags
                .iter()
                .chain(&self.object_types)
                .any(|s| s.len() > 128)
            || [&self.provider, &self.app, &self.category]
                .iter()
                .any(|s| s.as_ref().is_some_and(|s| s.len() > 256))
        {
            return Err(Error::invalid("Capability search exceeds its bounds"));
        }
        let mut phrases = vec![];
        let mut current = String::new();
        let mut quoted = false;
        for ch in self.query.to_lowercase().chars() {
            if ch == '"' {
                quoted = !quoted;
            } else if ch.is_whitespace() && !quoted {
                if !current.is_empty() {
                    phrases.push(std::mem::take(&mut current));
                }
            } else {
                current.push(ch);
            }
        }
        if quoted {
            return Err(Error::invalid("Search phrase has an unmatched quote"));
        }
        if !current.is_empty() {
            phrases.push(current);
        }
        Ok(phrases)
    }
}
impl Registry {
    /// Returns a stable ranking before availability filtering/pagination in the broker.
    pub fn ranked(
        &self,
        query: &CatalogQuery,
    ) -> Result<Vec<(&CommandDescriptor, &Metadata, u32)>> {
        let phrases = query.validate()?;
        if query
            .revision
            .is_some_and(|revision| revision != self.revision())
        {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Capability catalog changed; restart pagination",
            ));
        }
        let exact = query.query.trim().to_lowercase();
        let mut results = vec![];
        for descriptor in self.all() {
            let metadata = self.metadata(&descriptor.name)?;
            if query
                .provider
                .as_ref()
                .is_some_and(|p| p != &metadata.provider && !descriptor.backends.contains(p))
                || query
                    .app
                    .as_ref()
                    .is_some_and(|p| metadata.app.as_ref() != Some(p))
                || query
                    .category
                    .as_ref()
                    .is_some_and(|p| descriptor.name.split('.').next() != Some(p.as_str()))
                || query.risk.is_some_and(|risk| risk != descriptor.risk)
                || !query.tags.iter().all(|tag| metadata.tags.contains(tag))
                || !query
                    .object_types
                    .iter()
                    .all(|kind| metadata.object_types.contains(kind))
            {
                continue;
            }
            let name = descriptor.name.to_lowercase();
            let aliases = metadata.aliases.join(" ").to_lowercase();
            let tags = metadata.tags.join(" ").to_lowercase();
            let description = descriptor.description.to_lowercase();
            let mut score = 0;
            let mut matches = true;
            for phrase in &phrases {
                let weight = if name == *phrase {
                    1000
                } else if metadata
                    .aliases
                    .iter()
                    .any(|alias| alias.eq_ignore_ascii_case(phrase))
                {
                    600
                } else if name.split(['.', '_', '-']).any(|token| token == phrase) {
                    120
                } else if name.contains(phrase) {
                    80
                } else if aliases.contains(phrase) {
                    40
                } else if tags.contains(phrase) {
                    20
                } else if description.contains(phrase) {
                    5
                } else {
                    0
                };
                if weight == 0 {
                    matches = false;
                    break;
                }
                score += weight;
            }
            if matches {
                if name == exact && !exact.is_empty() {
                    score += 10_000;
                }
                results.push((descriptor, metadata, score));
            }
        }
        results.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.name.cmp(&b.0.name)));
        Ok(results)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_id_wins_with_stable_digest() {
        let registry = Registry::builtin().unwrap();
        let query = CatalogQuery {
            query: "blender.object.list".into(),
            ..Default::default()
        };
        let rows = registry.ranked(&query).unwrap();
        assert_eq!(rows[0].0.name, "blender.object.list");
        assert_eq!(
            rows[0].1.descriptor_sha256,
            descriptor_digest(rows[0].0).unwrap()
        );
        assert_eq!(rows[0].1.provider, "blender-native");
    }
    #[test]
    fn filters_and_pagination_revision_do_not_change_authority() {
        let registry = Registry::builtin().unwrap();
        let query = CatalogQuery {
            query: "material".into(),
            provider: Some("blender-native".into()),
            risk: Some(Risk::ReadOnly),
            ..Default::default()
        };
        let rows = registry.ranked(&query).unwrap();
        assert!(!rows.is_empty());
        assert!(
            rows.iter()
                .all(|(command, _, _)| command.risk == Risk::ReadOnly)
        );
        assert!(
            registry
                .ranked(&CatalogQuery {
                    revision: Some(0),
                    ..query
                })
                .is_err()
        );
    }
    #[test]
    fn quotes_are_phrases_and_order_is_deterministic() {
        let registry = Registry::builtin().unwrap();
        let query = CatalogQuery {
            query: "\"Blender\" material".into(),
            ..Default::default()
        };
        let a: Vec<_> = registry
            .ranked(&query)
            .unwrap()
            .iter()
            .map(|r| r.0.name.clone())
            .collect();
        let b: Vec<_> = registry
            .ranked(&query)
            .unwrap()
            .iter()
            .map(|r| r.0.name.clone())
            .collect();
        assert_eq!(a, b);
        assert!(!a.is_empty());
        assert!(
            CatalogQuery {
                query: "\"unclosed".into(),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }
    #[test]
    fn source_cannot_claim_core_or_trusted_metadata() {
        let registry = Registry::builtin().unwrap();
        let command = registry.describe("doctor").unwrap();
        let mut metadata = Metadata::builtin(command);
        metadata.source = SourceKind::ExternalMcp;
        assert!(metadata.validate_for(command).is_err());
        metadata.untrusted_metadata = true;
        assert!(metadata.validate_for(command).is_err());
    }
    #[test]
    fn empty_identifier_is_rejected() {
        let mut registry = Registry::builtin().unwrap();
        let mut command = registry.describe("doctor").unwrap().clone();
        command.name.clear();
        assert!(registry.register(command).is_err());
    }

    #[test]
    fn checked_in_ranking_vectors() {
        let rows: Vec<serde_json::Value> =
            serde_json::from_str(include_str!("../../../tests/golden/catalog-ranking.json"))
                .unwrap();
        let registry = Registry::builtin().unwrap();
        for row in rows {
            let query = CatalogQuery {
                query: row["query"].as_str().unwrap().into(),
                provider: row["provider"].as_str().map(String::from),
                ..Default::default()
            };
            assert_eq!(
                registry.ranked(&query).unwrap()[0].0.name,
                row["first"].as_str().unwrap()
            );
        }
    }
}
