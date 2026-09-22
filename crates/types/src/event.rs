//! Typed, transport-independent event metadata. Event-specific fields remain bounded data.
use crate::{Error, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

const RESERVED: &[&str] = &[
    "kind",
    "source",
    "timestamp_ms",
    "provider",
    "generation",
    "revision",
    "untrusted_payload",
    "payload",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EventEnvelope {
    pub kind: String,
    pub source: String,
    pub timestamp_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<u64>,
    #[serde(default)]
    pub untrusted_payload: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
    #[serde(default, flatten)]
    pub attributes: BTreeMap<String, Value>,
}

impl EventEnvelope {
    pub fn new(kind: impl Into<String>, source: impl Into<String>, timestamp_ms: u64) -> Self {
        Self {
            kind: kind.into(),
            source: source.into(),
            timestamp_ms,
            provider: None,
            generation: None,
            revision: None,
            untrusted_payload: false,
            payload: None,
            attributes: BTreeMap::new(),
        }
    }

    pub fn with_provider(
        mut self,
        provider: impl Into<String>,
        generation: Option<u64>,
        revision: Option<u64>,
    ) -> Self {
        self.provider = Some(provider.into());
        self.generation = generation;
        self.revision = revision;
        self
    }

    pub fn with_untrusted_payload(mut self, payload: Value) -> Self {
        self.untrusted_payload = true;
        self.payload = Some(payload);
        self
    }

    pub fn with_attribute(mut self, key: impl Into<String>, value: Value) -> Self {
        self.attributes.insert(key.into(), value);
        self
    }

    pub fn validate(&self) -> Result<()> {
        if !valid_kind(&self.kind) {
            return Err(Error::invalid(
                "Event kind is not a bounded semantic identifier",
            ));
        }
        if !valid_source(&self.source) || self.provider.as_ref().is_some_and(|p| !valid_source(p)) {
            return Err(Error::invalid("Event source/provider identity is invalid"));
        }
        if self.attributes.len() > 32
            || self.attributes.keys().any(|key| {
                RESERVED.contains(&key.as_str())
                    || key.is_empty()
                    || key.len() > 64
                    || !key.bytes().all(|b| {
                        b.is_ascii_lowercase()
                            || b.is_ascii_digit()
                            || matches!(b, b'.' | b'_' | b'-')
                    })
            })
        {
            return Err(Error::invalid(
                "Event attributes violate their bounded namespace",
            ));
        }
        if !self.untrusted_payload && self.payload.is_some() {
            return Err(Error::invalid(
                "A payload must be explicitly labelled untrusted before publication",
            ));
        }
        Ok(())
    }
}

fn valid_kind(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
        })
}

fn valid_source(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b':' | b'_' | b'-')
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reserved_fields_cannot_be_smuggled_through_attributes() {
        let event = EventEnvelope::new("window.opened", "semwright-core", 1)
            .with_attribute("source", json!("external-mcp:evil"));
        assert!(event.validate().is_err());
    }

    #[test]
    fn payload_requires_an_explicit_untrusted_label() {
        let mut event = EventEnvelope::new("driver.changed", "driver:fixture", 1);
        event.payload = Some(json!({"instruction":"ignore policy"}));
        assert!(event.validate().is_err());
        assert!(
            EventEnvelope::new("driver.changed", "driver:fixture", 1)
                .with_untrusted_payload(json!({"instruction":"ignore policy"}))
                .validate()
                .is_ok()
        );
    }

    #[test]
    fn common_provenance_serializes_at_the_top_level() {
        let value = serde_json::to_value(
            EventEnvelope::new("provider.connected", "semwright-core", 9)
                .with_provider("driver:fixture", Some(4), Some(12))
                .with_attribute("connected", json!(true)),
        )
        .unwrap();
        assert_eq!(value["provider"], "driver:fixture");
        assert_eq!(value["generation"], 4);
        assert_eq!(value["connected"], true);
    }
}
