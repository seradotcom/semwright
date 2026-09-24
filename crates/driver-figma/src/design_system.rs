use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum DesignError {
    #[error("alias cycle")]
    AliasCycle,
    #[error("unknown alias")]
    UnknownAlias,
    #[error("limit")]
    Limit,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DesignSystem {
    pub schema_version: u32,
    pub collections: Vec<Collection>,
    pub components: Vec<ComponentSpec>,
    pub styles: Vec<StyleSpec>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Collection {
    pub name: String,
    pub modes: Vec<String>,
    pub variables: Vec<Variable>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Variable {
    pub name: String,
    pub resolved_type: String,
    pub values: BTreeMap<String, TokenValue>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum TokenValue {
    Boolean(bool),
    Float(f64),
    String(String),
    Color(String),
    Alias(String),
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ComponentSpec {
    pub name: String,
    pub key: Option<String>,
    pub variants: BTreeMap<String, Vec<String>>,
    pub properties: BTreeMap<String, Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StyleSpec {
    pub name: String,
    pub kind: String,
    pub value: Value,
}
impl DesignSystem {
    pub fn validate(&self) -> Result<(), DesignError> {
        if self.collections.len() > 128 || self.components.len() > 2048 {
            return Err(DesignError::Limit);
        }
        let vars: BTreeMap<_, _> = self
            .collections
            .iter()
            .flat_map(|c| c.variables.iter().map(|v| (v.name.as_str(), v)))
            .collect();
        for v in vars.values() {
            for val in v.values.values() {
                if let TokenValue::Alias(target) = val {
                    let mut seen = BTreeSet::new();
                    let mut cur = target.as_str();
                    loop {
                        if !seen.insert(cur) {
                            return Err(DesignError::AliasCycle);
                        }
                        let Some(next) = vars.get(cur) else {
                            return Err(DesignError::UnknownAlias);
                        };
                        let alias = next.values.values().find_map(|x| {
                            if let TokenValue::Alias(a) = x {
                                Some(a.as_str())
                            } else {
                                None
                            }
                        });
                        if let Some(a) = alias {
                            cur = a
                        } else {
                            break;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
pub fn parse_css_variables(css: &str) -> Result<BTreeMap<String, String>, DesignError> {
    if css.len() > 1_048_576 {
        return Err(DesignError::Limit);
    }
    let mut out = BTreeMap::new();
    for part in css.split(';') {
        let Some((k, v)) = part.split_once(':') else {
            continue;
        };
        let key = k.trim().trim_start_matches('{').trim();
        if key.starts_with("--") && !v.trim().is_empty() {
            out.insert(
                key[2..].to_string(),
                v.trim().trim_end_matches('}').trim().to_string(),
            );
            if out.len() > 4096 {
                return Err(DesignError::Limit);
            }
        }
    }
    Ok(out)
}
