mod common;
use common::*;
use semwright_effect_conformance::composition::*;
use semwright_effect_conformance::*;
use std::collections::{BTreeMap, BTreeSet};

fn prepared() -> (
    EffectContract,
    EvaluationContext,
    PreparedPlan<(), ()>,
    ProfileDescriptor,
) {
    let (contract, mut ctx) = fixture();
    let effects = BTreeSet::from([EffectClass::UpdateOwnedObject]);
    let profile = ProfileDescriptor {
        identity: ProfileIdentity {
            id: contract.profile.clone(),
            version: 1,
            intent_schema: Digest::of_bytes(b"unit"),
            operation_schema: Digest::of_bytes(b"unit"),
        },
        capabilities: vec![CapabilityBinding {
            phase: Phase::Apply,
            command: "fixture.apply".into(),
            descriptor: Digest::of_bytes(b"fixture"),
            effects: effects.clone(),
        }],
        required_rules: contract.required_rules(),
        allowed_effects: effects.clone(),
    };
    let plan = PreparedPlan::prepare(
        PlanBody {
            contract_version: CONTRACT_VERSION,
            profile: profile.identity.clone(),
            owner: ctx.owner.clone(),
            base: ctx.before.clone(),
            intent: (),
            intent_digest: canonical_digest(&()).unwrap(),
            dependencies: BTreeMap::from([("effects.contract".into(), contract.digest().unwrap())]),
            changes: ChangeSet {
                atomicity: Atomicity::NonAtomicSequence,
                operations: vec![TypedOperation {
                    id: "save".into(),
                    payload: (),
                    reads: vec![],
                    writes: vec![],
                    effects,
                    depends_on: vec![],
                    postconditions: contract.required_rules(),
                }],
            },
            required_rules: contract.required_rules(),
            observation_scope: ctx.observation_scope.iter().cloned().collect(),
            budget: ctx.budget.clone(),
            require_compare_and_swap: false,
        },
        &profile,
    )
    .unwrap();
    ctx.plan_digest = plan.digest.clone();
    (contract, ctx, plan, profile)
}
#[test]
fn a_prepared_plan_pins_effect_contract_required_rules_scope_and_budget() {
    let (contract, mut ctx, plan, profile) = prepared();
    ctx.validate_plan(&plan, &profile, &contract).unwrap();
    let batch = collect(&contract, &ctx, &mut ModelAdapter::default()).unwrap();
    assert_eq!(
        evaluate(&contract, &ctx, &batch)
            .unwrap()
            .verdict()
            .unwrap(),
        Verdict::Pass
    );
    ctx.budget.max_observations += 1;
    assert!(ctx.validate_plan(&plan, &profile, &contract).is_err());
    ctx.budget = plan.body.budget.clone();
    ctx.observation_scope.clear();
    assert!(ctx.validate_plan(&plan, &profile, &contract).is_err());
}
#[test]
fn unpinned_effect_contract_is_rejected_even_if_a_plan_digest_is_valid() {
    let (contract, mut ctx, mut plan, profile) = prepared();
    plan.body.dependencies.clear();
    plan.digest = canonical_digest(&plan.body).unwrap();
    ctx.plan_digest = plan.digest.clone();
    plan.verify(&profile).unwrap();
    assert!(ctx.validate_plan(&plan, &profile, &contract).is_err());
}
#[test]
fn changing_observer_channel_during_readback_is_unknown() {
    struct Changing {
        calls: usize,
    }
    impl EvidenceAdapter for Changing {
        fn identity(&self, resource: &ResourceKey) -> Option<AdapterIdentity> {
            let mut id = ModelAdapter::default().identity(resource).unwrap();
            if self.calls > 0 {
                id.generation = "changed".into();
            }
            Some(id)
        }
        fn observe(
            &mut self,
            ctx: &EvaluationContext,
            rule: &EffectRule,
        ) -> Result<AdapterObservation> {
            self.calls += 1;
            Ok(observation(ctx, rule))
        }
    }
    let (contract, ctx) = fixture();
    let mut adapter = Changing { calls: 0 };
    let batch = collect(&contract, &ctx, &mut adapter).unwrap();
    assert_eq!(
        evaluate(&contract, &ctx, &batch)
            .unwrap()
            .verdict()
            .unwrap(),
        Verdict::Unknown
    );
}
