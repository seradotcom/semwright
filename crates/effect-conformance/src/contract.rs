use crate::Predicate;
use schemars::JsonSchema;
use semwright_semantic_composition::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const EFFECT_CONTRACT_VERSION: u32 = 1;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Obligation { Required, Forbidden, Preference }
impl Obligation {
    pub fn required(self) -> bool { self != Self::Preference }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ObservationMethod {
    pub name: String,
    pub version: u32,
    pub source: EvidenceSource,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EffectRule {
    pub id: String,
    pub version: u32,
    pub obligation: Obligation,
    pub operation_id: String,
    pub address: Address,
    pub predicate: Predicate,
    pub method: ObservationMethod,
    pub universe: Option<String>,
    pub artifact: Option<Digest>,
    pub require_causal_attribution: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EffectLimit {
    pub operation_id: String,
    pub effects: BTreeSet<EffectClass>,
    pub writes: BTreeSet<Address>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EffectContract {
    pub version: u32,
    pub profile: String,
    pub allowed: Vec<EffectLimit>,
    pub rules: Vec<EffectRule>,
}
impl EffectContract {
    pub fn validate(&self) -> Result<()> {
        ensure(self.version == EFFECT_CONTRACT_VERSION, "effect contract version")?;
        bounded_id(&self.profile)?;
        ensure(self.rules.len() <= 256 && self.allowed.len() <= 256, "effect contract budget")?;
        let mut seen = BTreeSet::new();
        for rule in &self.rules {
            bounded_id(&rule.id)?; bounded_id(&rule.operation_id)?;
            bounded_id(&rule.address.resource.provider)?; bounded_id(&rule.address.resource.resource)?;
            bounded_id(&rule.address.logical_id)?; bounded_id(&rule.address.property)?;
            bounded_id(&rule.method.name)?;
            ensure(rule.version > 0 && rule.method.version > 0, "missing rule/method version")?;
            ensure(seen.insert(&rule.id), "duplicate effect rule")?;
            rule.predicate.validate()?;
            if let Some(universe) = &rule.universe { bounded_id(universe)?; }
            ensure(!rule.predicate.needs_complete_universe() || rule.universe.is_some(), "global predicate requires declared universe")?;
        }
        let mut operations = BTreeSet::new();
        for bound in &self.allowed {
            bounded_id(&bound.operation_id)?;
            ensure(operations.insert(&bound.operation_id), "duplicate effect limit")?;
            ensure(bound.writes.len() <= 4096, "effect write budget")?;
            for address in &bound.writes {
                bounded_id(&address.resource.provider)?; bounded_id(&address.resource.resource)?;
                bounded_id(&address.logical_id)?; bounded_id(&address.property)?;
            }
        }
        Ok(())
    }
    pub fn digest(&self) -> Result<Digest> { self.validate()?; canonical_digest(self) }
    pub fn required_rules(&self) -> BTreeSet<String> {
        self.rules.iter().filter(|r| r.obligation.required()).map(|r| r.id.clone()).collect()
    }
}
/// Pure containment check; a successful result is NOT a Broker grant or permit.
/// Both trusted bounds must come from implementation and current Broker policy,
/// never from fields supplied in the client effect contract.
pub fn check_effect_bounds<O>(changes: &ChangeSet<O>, client: &[EffectLimit], implementation: &[EffectLimit], broker: &[EffectLimit]) -> Result<()> {
    ensure(!changes.operations.is_empty() && changes.operations.len() <= 4096, "change budget")?;
    let mut seen = BTreeSet::new();
    for op in &changes.operations {
        ensure(seen.insert(&op.id), "duplicate operation")?;
        ensure(!op.effects.is_empty(), "missing effect classification")?;
        for limits in [client, implementation, broker] {
            let matches: Vec<_> = limits.iter().filter(|l| l.operation_id == op.id).collect();
            ensure(matches.len() == 1, "missing or ambiguous trusted effect bound")?;
            let limit = matches[0];
            ensure(op.effects.is_subset(&limit.effects), "effect class outside intersected bounds")?;
            ensure(op.writes.iter().all(|a| limit.writes.contains(a)), "write outside intersected bounds")?;
        }
    }
    Ok(())
}
