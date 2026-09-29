//! Blender adapter for F's published E0 contract. Evaluation remains data-only;
//! native observation happens through the existing authenticated driver channel.
use super::*;
use semwright_effect_conformance as effects;
use semwright_semantic_composition::*;
use serde_json::Value;

pub const READBACK_RULE: &str = "blender.plan-native-readback.v1";
pub const PRESERVATION_RULE: &str = "blender.unmanaged-preservation.v1";

pub struct PreparedAuthoring {
    pub plan: PreparedPlan<AuthoringIntent, NativeOperation>,
    pub profile: ProfileDescriptor,
    pub contract: effects::EffectContract,
}

pub fn effect_contract(
    intent: &AuthoringIntent,
    changes: &ChangeSet<NativeOperation>,
) -> Result<effects::EffectContract> {
    ensure(!changes.operations.is_empty(), "effect contract requires operations")?;
    let allowed = changes
        .operations
        .iter()
        .map(|op| effects::EffectLimit {
            operation_id: op.id.clone(),
            effects: op.effects.clone(),
            writes: op.writes.iter().cloned().collect(),
        })
        .collect::<Vec<_>>();
    let last = changes.operations.last().expect("checked nonempty");
    let address = last
        .writes
        .first()
        .cloned()
        .ok_or_else(|| ContractError::Invalid("final operation has no observed write".into()))?;
    let mut rules = vec![effects::EffectRule {
        id: READBACK_RULE.into(),
        version: 1,
        obligation: effects::Obligation::Required,
        operation_id: last.id.clone(),
        address: address.clone(),
        predicate: effects::Predicate::Equals {
            expected: effects::ObservedValue::Bool { value: true },
        },
        method: effects::ObservationMethod {
            name: "blender-bounded-source-rna".into(),
            version: 1,
            source: EvidenceSource::NativeApi,
        },
        universe: None,
        artifact: None,
        require_causal_attribution: true,
    }];
    if matches!(intent, AuthoringIntent::Create { .. }) {
        rules.push(effects::EffectRule {
            id: PRESERVATION_RULE.into(),
            version: 1,
            obligation: effects::Obligation::Required,
            operation_id: last.id.clone(),
            address,
            predicate: effects::Predicate::Preserved,
            method: effects::ObservationMethod {
                name: "blender-unmanaged-source-projection".into(),
                version: 1,
                source: EvidenceSource::NativeApi,
            },
            universe: None,
            artifact: None,
            require_causal_attribution: true,
        });
    }
    let contract = effects::EffectContract {
        version: effects::EFFECT_CONTRACT_VERSION,
        profile: "blender-native-authoring".into(),
        allowed,
        rules,
    };
    contract.validate()?;
    Ok(contract)
}

fn target_island(plan: &PreparedPlan<AuthoringIntent, NativeOperation>) -> Result<&str> {
    plan.body
        .changes
        .operations
        .first()
        .map(|op| match &op.payload {
            NativeOperation::Collection { island, .. }
            | NativeOperation::Material { island, .. }
            | NativeOperation::Entity { island, .. }
            | NativeOperation::Relation { island, .. }
            | NativeOperation::Animation { island, .. }
            | NativeOperation::Transform { island, .. } => island.as_str(),
        })
        .ok_or_else(|| ContractError::Invalid("empty authoring plan".into()))
}

fn unmanaged_digest(snapshot: &NativeSnapshot, target: &str) -> Result<Digest> {
    ensure(
        snapshot.source_only && snapshot.exhaustive && snapshot.island.is_none(),
        "unmanaged preservation needs a complete whole-scene source snapshot",
    )?;
    let rows = snapshot
        .items
        .iter()
        .filter(|row| row.get("native_island").and_then(Value::as_str) != Some(target))
        .cloned()
        .collect::<Vec<_>>();
    canonical_digest(&rows)
}

struct BlenderEvidenceAdapter<'a> {
    plan: &'a PreparedPlan<AuthoringIntent, NativeOperation>,
    before: &'a NativeSnapshot,
    after: &'a NativeSnapshot,
    global_after: Option<&'a NativeSnapshot>,
    identity: effects::AdapterIdentity,
}

impl effects::EvidenceAdapter for BlenderEvidenceAdapter<'_> {
    fn identity(&self, resource: &ResourceKey) -> Option<effects::AdapterIdentity> {
        (resource.provider == "driver:blender" && resource.resource == "authoring-workspace")
            .then(|| self.identity.clone())
    }

    fn observe(
        &mut self,
        context: &effects::EvaluationContext,
        rule: &effects::EffectRule,
    ) -> Result<effects::AdapterObservation> {
        let value = match rule.id.as_str() {
            READBACK_RULE => effects::ObservedValue::Bool {
                value: !self.after.drift && native_matches(&self.plan.body.intent, self.after),
            },
            PRESERVATION_RULE => {
                let target = target_island(self.plan)?;
                let after = self.global_after.ok_or_else(|| {
                    ContractError::Unknown(
                        "whole-scene post-state was not observed for preservation".into(),
                    )
                })?;
                effects::ObservedValue::Preservation {
                    before: unmanaged_digest(self.before, target)?,
                    after: unmanaged_digest(after, target)?,
                }
            }
            _ => {
                return Err(ContractError::Invalid(
                    "effect rule is not owned by the Blender adapter".into(),
                ));
            }
        };
        Ok(effects::AdapterObservation {
            binding: effects::EvidenceBinding {
                owner: context.owner.clone(),
                request_id: context.request_id.clone(),
                operation_id: rule.operation_id.clone(),
                plan_digest: context.plan_digest.clone(),
                contract_digest: context.contract_digest.clone(),
            },
            observation: ObservationRef {
                id: format!("obs-{}", rule.id),
                base: context.after.clone(),
                source: rule.method.source,
                method: rule.method.name.clone(),
                method_version: rule.method.version,
                scope: vec![rule.address.clone()],
                artifact: rule.artifact.clone(),
                exhaustive: true,
            },
            readback: effects::ReadbackState::Observed,
            value: Some(value),
            coverage: effects::ObservationCoverage {
                consistent: true,
                missing: vec![],
                attribution: effects::Attribution::Isolated,
                enumeration: None,
            },
        })
    }
}

pub fn evaluate_native_effects(
    prepared: &PreparedAuthoring,
    request_id: &str,
    before: &NativeSnapshot,
    after: &NativeSnapshot,
    global_after: Option<&NativeSnapshot>,
    status: ExecutionStatus,
) -> Result<effects::EffectEvaluation> {
    let post = after.base(&prepared.plan.body.owner)?;
    let context = effects::EvaluationContext {
        owner: prepared.plan.body.owner.clone(),
        request_id: request_id.into(),
        plan_digest: prepared.plan.digest.clone(),
        contract_digest: prepared.contract.digest()?,
        before: prepared.plan.body.base.clone(),
        after: post.clone(),
        operations: prepared
            .plan
            .body
            .changes
            .operations
            .iter()
            .map(|op| op.id.clone())
            .collect(),
        observation_scope: prepared.plan.body.observation_scope.iter().cloned().collect(),
        execution_status: status,
        support_level: SupportLevel::Native,
        budget: prepared.plan.body.budget.clone(),
    };
    context.validate_plan(&prepared.plan, &prepared.profile, &prepared.contract)?;
    let state = &post.0[0];
    let mut adapter = BlenderEvidenceAdapter {
        plan: &prepared.plan,
        before,
        after,
        global_after,
        identity: effects::AdapterIdentity {
            owner: context.owner.clone(),
            provider: state.key.provider.clone(),
            provider_session: state.provider_session.clone(),
            generation: state.generation.clone(),
        },
    };
    let batch = effects::collect(&prepared.contract, &context, &mut adapter)?;
    effects::evaluate(&prepared.contract, &context, &batch)
}
