use crate::{EffectContract, EffectRule, EnumerationBinding, EnumerationPage, ObservedValue};
use schemars::JsonSchema;
use semwright_semantic_composition::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Host-owned invocation context. Deliberately NOT deserializable from client JSON.
/// Obtain owner, execution and state stamps through existing Broker/PlanVault/Host.
#[derive(Debug, Clone, Serialize)]
pub struct EvaluationContext {
    pub owner: Owner,
    pub request_id: String,
    pub plan_digest: Digest,
    pub contract_digest: Digest,
    pub before: BaseStateSet,
    pub after: BaseStateSet,
    pub operations: BTreeSet<String>,
    pub observation_scope: BTreeSet<Address>,
    pub execution_status: ExecutionStatus,
    pub support_level: SupportLevel,
    pub budget: ConvergenceBudget,
}
impl EvaluationContext {
    pub fn validate(&self) -> Result<()> {
        self.owner.validate()?; bounded_id(&self.request_id)?;
        self.before.validate()?; self.after.validate()?; self.budget.validate()?;
        ensure(!self.operations.is_empty() && self.operations.len() <= 4096 && self.observation_scope.len() <= 4096, "evaluation context budget")?;
        for op in &self.operations { bounded_id(op)?; }
        Ok(())
    }
    pub fn enumeration_binding(&self, rule: &EffectRule) -> Result<EnumerationBinding> {
        let state = self.after.0.iter().find(|s| s.key == rule.address.resource)
            .ok_or_else(|| ContractError::Invalid("missing enumeration resource base".into()))?;
        let universe = rule.universe.clone().ok_or_else(|| ContractError::Invalid("missing universe".into()))?;
        Ok(EnumerationBinding { owner: self.owner.clone(), resource: state.key.clone(),
            provider_session: state.provider_session.clone(), generation: state.generation.clone(),
            query_digest: canonical_digest(&(&rule.address, &universe))?, snapshot_digest: canonical_digest(state)?, universe })
    }
}
/// The identity comes from the observer's authenticated channel, not the payload.
#[derive(Debug, Clone)]
pub struct AdapterIdentity {
    pub owner: Owner,
    pub provider: String,
    pub provider_session: String,
    pub generation: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EvidenceBinding {
    pub owner: Owner,
    pub request_id: String,
    pub operation_id: String,
    pub plan_digest: Digest,
    pub contract_digest: Digest,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReadbackState { Observed, Acknowledged, RequestEcho, Unavailable, Error, Unsupported }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Attribution { Isolated, Ordered, Concurrent, Ambiguous }
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ObservationCoverage {
    pub consistent: bool,
    pub missing: Vec<Address>,
    pub attribution: Attribution,
    pub enumeration: Option<Vec<EnumerationPage>>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AdapterObservation {
    pub binding: EvidenceBinding,
    pub observation: ObservationRef,
    pub readback: ReadbackState,
    pub value: Option<ObservedValue>,
    pub coverage: ObservationCoverage,
}
/// Implement ONLY in trusted compiled provider/Host integration code. observe
/// must perform/read an authorized result, not reflect request fields. This trait
/// does not authorize render/open/read; adapters use existing Broker operations.
/// In-process malicious code is outside this data library's trust boundary.
pub trait EvidenceAdapter {
    fn identity(&self, resource: &ResourceKey) -> Option<AdapterIdentity>;
    fn observe(&mut self, context: &EvaluationContext, rule: &EffectRule) -> Result<AdapterObservation>;
}

pub(crate) struct CollectedObservation {
    pub rule_id: String,
    pub identity: Option<AdapterIdentity>,
    pub result: Result<AdapterObservation>,
}
/// No Deserialize and no public fields/constructor: a wire receipt cannot mint it.
pub struct EvidenceBatch {
    pub(crate) contract_digest: Digest,
    pub(crate) context_digest: Digest,
    pub(crate) observations: Vec<CollectedObservation>,
}
pub fn collect<A: EvidenceAdapter>(contract: &EffectContract, context: &EvaluationContext, adapter: &mut A) -> Result<EvidenceBatch> {
    contract.validate()?; context.validate()?;
    ensure(contract.rules.len() <= context.budget.max_observations as usize, "observation budget must be reserved before any observer I/O")?;
    ensure(contract.digest()? == context.contract_digest, "effect contract changed after plan was pinned")?;
    let mut observations = Vec::new();
    for rule in &contract.rules {
        let identity = adapter.identity(&rule.address.resource);
        // Refuse out-of-scope calls BEFORE invoking an observer with potential I/O.
        let result = if !context.operations.contains(&rule.operation_id) || !context.observation_scope.contains(&rule.address) {
            Err(ContractError::Denied("observer outside trusted invocation scope".into()))
        } else { adapter.observe(context, rule) };
        observations.push(CollectedObservation { rule_id: rule.id.clone(), identity, result });
    }
    Ok(EvidenceBatch { contract_digest: contract.digest()?, context_digest: canonical_digest(context)?, observations })
}
/// Explicit untrusted intake for transport tests/imported reports. It remains
/// untrusted even if all JSON provenance/exhaustive/source fields look native.
pub fn collect_untrusted(contract: &EffectContract, context: &EvaluationContext, observations: Vec<AdapterObservation>) -> Result<EvidenceBatch> {
    struct Untrusted { observations: Vec<AdapterObservation> }
    impl EvidenceAdapter for Untrusted {
        fn identity(&self, _: &ResourceKey) -> Option<AdapterIdentity> { None }
        fn observe(&mut self, _: &EvaluationContext, rule: &EffectRule) -> Result<AdapterObservation> {
            let matches: Vec<_> = self.observations.iter().filter(|o| o.binding.operation_id == rule.operation_id && o.observation.scope.contains(&rule.address)).collect();
            ensure(matches.len() == 1, "missing/duplicate imported observation")?;
            Ok(matches[0].clone())
        }
    }
    collect(contract, context, &mut Untrusted { observations })
}
