#![allow(dead_code)]
use semwright_effect_conformance::*;
use semwright_effect_conformance::composition::*;
use std::collections::BTreeSet;

pub fn fixture() -> (EffectContract, EvaluationContext) {
    let contract: EffectContract = strict_decode(include_bytes!("../../fixtures/e0.json")).unwrap();
    let base = BaseStateSet(vec![BaseState { key: contract.rules[0].address.resource.clone(), document_id: "fixture-document".into(), provider_session: "native-session".into(), generation: "1".into(), revision: Revision::Counter(2), concurrency: Concurrency::BestEffortRevalidate }]);
    let context = EvaluationContext { owner: Owner { session: "host-session".into(), principal: PrincipalBinding::HostSession }, request_id: "request-1".into(), plan_digest: Digest::of_bytes(b"plan-1"), contract_digest: contract.digest().unwrap(), before: base.clone(), after: base, operations: BTreeSet::from(["save".into()]), observation_scope: contract.rules.iter().map(|r| r.address.clone()).collect(), execution_status: ExecutionStatus::Completed, support_level: SupportLevel::Native };
    (contract, context)
}
pub fn observation(ctx: &EvaluationContext, rule: &EffectRule) -> AdapterObservation {
    let value = match &rule.predicate {
        Predicate::Within { expected, units, .. } => ObservedValue::Number { value: *expected, units: units.clone() },
        Predicate::Preserved => ObservedValue::Preservation { before: Digest::of_bytes(b"sentinel"), after: Digest::of_bytes(b"sentinel") },
        Predicate::Membership { expected } => ObservedValue::Members { values: expected.clone() },
        _ => ObservedValue::Bool { value: true },
    };
    AdapterObservation { binding: EvidenceBinding { owner: ctx.owner.clone(), request_id: ctx.request_id.clone(), operation_id: rule.operation_id.clone(), plan_digest: ctx.plan_digest.clone(), contract_digest: ctx.contract_digest.clone() }, observation: ObservationRef { id: format!("obs:{}", rule.id), base: ctx.after.clone(), source: rule.method.source, method: rule.method.name.clone(), method_version: rule.method.version, scope: vec![rule.address.clone()], artifact: rule.artifact.clone(), exhaustive: true }, readback: ReadbackState::Observed, value: Some(value), coverage: ObservationCoverage { consistent: true, missing: vec![], attribution: Attribution::Isolated, enumeration: None } }
}
pub struct ModelAdapter { pub mutate: fn(&EffectRule, &mut AdapterObservation), pub trusted: bool, pub calls: usize }
impl Default for ModelAdapter {
    fn default() -> Self { Self { mutate: |_, _| {}, trusted: true, calls: 0 } }
}
impl EvidenceAdapter for ModelAdapter {
    fn identity(&self, resource: &ResourceKey) -> Option<AdapterIdentity> {
        self.trusted.then(|| AdapterIdentity { owner: Owner { session: "host-session".into(), principal: PrincipalBinding::HostSession }, provider: resource.provider.clone(), provider_session: "native-session".into(), generation: "1".into() })
    }
    fn observe(&mut self, ctx: &EvaluationContext, rule: &EffectRule) -> Result<AdapterObservation> {
        self.calls += 1;
        let mut o = observation(ctx, rule); (self.mutate)(rule, &mut o); Ok(o)
    }
}
pub fn run(adapter: &mut impl EvidenceAdapter) -> EffectEvaluation {
    let (c, ctx) = fixture(); let batch = collect(&c, &ctx, adapter).unwrap(); evaluate(&c, &ctx, &batch).unwrap()
}
pub fn binding() -> EnumerationBinding {
    let (mut c, ctx) = fixture(); c.rules[0].universe = Some("scene-nodes".into()); ctx.enumeration_binding(&c.rules[0]).unwrap()
}
pub fn pages() -> Vec<EnumerationPage> {
    (0..3).map(|i| EnumerationPage { binding: binding(), index: i,
        cursor_in: if i == 0 { None } else { Some(format!("cursor-{i}")) },
        cursor_out: if i == 2 { None } else { Some(format!("cursor-{}", i + 1)) },
        items: vec![format!("member-{i}")], total: Some(3), final_page: i == 2,
        truncated: false, consistency: EnumerationConsistency::Snapshot }).collect()
}
