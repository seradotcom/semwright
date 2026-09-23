use crate::{Fault, FaultKind, Result, bounds};
use semwright_driver_sdk::{Capability, descriptor_digest};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub command: String,
    pub request: String,
    pub target: Option<String>,
    pub mode: String,
    pub fields: BTreeMap<String, String>,
    pub fixed: Map<String, Value>,
    pub response: BTreeMap<String, String>,
    pub mutation: bool,
}

pub struct Entry {
    pub capability: Capability,
    pub plan: Plan,
    pub digest: String,
    input: jsonschema::Validator,
    output: jsonschema::Validator,
}

pub struct Catalog {
    entries: BTreeMap<String, Entry>,
}
impl Catalog {
    pub fn load() -> Result<Self> {
        let caps: Vec<Capability> = serde_json::from_str(include_str!("capabilities.json"))?;
        let plans: Vec<Plan> = serde_json::from_str(include_str!("plans.json"))?;
        if caps.len() != plans.len() {
            return Err(Fault::new(FaultKind::Configuration));
        }
        let mut entries = BTreeMap::new();
        for (capability, plan) in caps.into_iter().zip(plans) {
            if capability.descriptor.name != format!("driver.obs.{}", plan.command) {
                return Err(Fault::new(FaultKind::Configuration));
            }
            let digest = descriptor_digest(&capability.descriptor)
                .map_err(|_| Fault::new(FaultKind::Configuration))?;
            let input = jsonschema::validator_for(&capability.descriptor.input_schema)
                .map_err(|_| Fault::new(FaultKind::Configuration))?;
            let output = jsonschema::validator_for(&capability.descriptor.output_schema)
                .map_err(|_| Fault::new(FaultKind::Configuration))?;
            let name = capability.descriptor.name.clone();
            if entries
                .insert(
                    name,
                    Entry {
                        capability,
                        plan,
                        digest,
                        input,
                        output,
                    },
                )
                .is_some()
            {
                return Err(Fault::new(FaultKind::Configuration));
            }
        }
        Ok(Self { entries })
    }
    pub fn capabilities(&self) -> Vec<Capability> {
        self.entries
            .values()
            .map(|entry| entry.capability.clone())
            .collect()
    }
    pub fn get(&self, name: &str) -> Result<&Entry> {
        self.entries
            .get(name)
            .ok_or_else(|| Fault::new(FaultKind::Unsupported))
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
impl Entry {
    pub fn input(&self, value: &Value) -> Result<()> {
        bounds::check(value, 65536)?;
        if !self.input.is_valid(value) {
            return Err(Fault::new(FaultKind::Configuration));
        }
        if let Some(patch) = value.get("patch") {
            bounds::check(patch, 8192)?;
        }
        for key in ["name", "new_name"] {
            if let Some(name) = value[key].as_str() {
                bounds::name(name)?;
            }
        }
        Ok(())
    }
    pub fn output(&self, value: &Value) -> Result<()> {
        bounds::check(value, bounds::MAX_FRAME)?;
        if !self.output.is_valid(value) {
            return Err(Fault::new(FaultKind::Protocol));
        }
        Ok(())
    }
}

pub fn map_arguments(plan: &Plan, args: &Value) -> Result<Value> {
    let mut data = plan.fixed.clone();
    for (from, to) in &plan.fields {
        let value = args
            .get(from)
            .ok_or_else(|| Fault::new(FaultKind::Configuration))?;
        data.insert(to.clone(), value.clone());
    }
    let data = Value::Object(data);
    bounds::check(&data, 65536)?;
    Ok(data)
}
pub fn map_output(plan: &Plan, data: &Value) -> Result<Value> {
    let mut out = Map::new();
    for (to, from) in &plan.response {
        let value = data
            .get(from)
            .ok_or_else(|| Fault::new(FaultKind::Protocol))?;
        out.insert(to.clone(), bounds::scrub(value));
    }
    Ok(Value::Object(out))
}
