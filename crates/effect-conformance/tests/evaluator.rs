mod common;
use common::*;
use semwright_effect_conformance::composition::*;
use semwright_effect_conformance::*;

#[test]
fn model_native_boundary_passes_but_this_test_is_not_native_evidence() {
    assert_eq!(
        run(&mut ModelAdapter::default()).verdict().unwrap(),
        Verdict::Pass
    );
}
#[test]
fn imported_native_flags_cannot_mint_authenticated_evidence() {
    let (c, ctx) = fixture();
    let forged = c.rules.iter().map(|r| observation(&ctx, r)).collect();
    let batch = collect_untrusted(&c, &ctx, forged).unwrap();
    let out = evaluate(&c, &ctx, &batch).unwrap();
    assert_eq!(out.verdict().unwrap(), Verdict::Unknown);
    assert!(out.coverage.iter().all(|r| !r.sufficient));
}
#[test]
fn owner_request_operation_plan_contract_and_artifact_tampering_are_unknown() {
    let mutations: [fn(&EffectRule, &mut AdapterObservation); 6] = [
        |_, o| o.binding.owner.session = "attacker".into(),
        |_, o| o.binding.request_id = "old-request".into(),
        |_, o| o.binding.operation_id = "old-operation".into(),
        |_, o| o.binding.plan_digest = Digest::of_bytes(b"other-plan"),
        |_, o| o.binding.contract_digest = Digest::of_bytes(b"other-contract"),
        |_, o| o.observation.artifact = Some(Digest::of_bytes(b"other-artifact")),
    ];
    for mutate in mutations {
        assert_eq!(
            run(&mut ModelAdapter {
                mutate,
                ..Default::default()
            })
            .verdict()
            .unwrap(),
            Verdict::Unknown
        );
    }
}
#[test]
fn method_scope_generation_source_and_revision_substitution_are_unknown() {
    let mutations: [fn(&EffectRule, &mut AdapterObservation); 8] = [
        |_, o| o.observation.method_version = 0,
        |_, o| o.observation.method = "ack".into(),
        |_, o| o.observation.scope.clear(),
        |_, o| o.observation.base.0[0].generation = "next".into(),
        |_, o| o.observation.base.0[0].provider_session = "different".into(),
        |_, o| o.observation.base.0[0].revision = Revision::Counter(0),
        |_, o| o.observation.source = EvidenceSource::Simulation,
        |_, o| o.coverage.consistent = false,
    ];
    for mutate in mutations {
        assert_eq!(
            run(&mut ModelAdapter {
                mutate,
                ..Default::default()
            })
            .verdict()
            .unwrap(),
            Verdict::Unknown
        );
    }
}
#[test]
fn observer_is_not_invoked_outside_host_scope() {
    let (c, mut ctx) = fixture();
    ctx.observation_scope.clear();
    let mut adapter = ModelAdapter::default();
    let batch = collect(&c, &ctx, &mut adapter).unwrap();
    assert_eq!(adapter.calls, 0);
    assert_eq!(
        evaluate(&c, &ctx, &batch).unwrap().verdict().unwrap(),
        Verdict::Unknown
    );
}
#[test]
fn duplicate_or_versionless_rules_are_invalid_not_coverage() {
    let (mut c, _) = fixture();
    c.rules.push(c.rules[0].clone());
    assert!(c.validate().is_err());
    c.rules.pop();
    c.rules[0].version = 0;
    assert!(c.validate().is_err());
}
#[test]
fn removing_required_rule_or_replaying_context_cannot_silently_pass() {
    let (mut c, mut ctx) = fixture();
    let batch = collect(&c, &ctx, &mut ModelAdapter::default()).unwrap();
    ctx.request_id = "another".into();
    assert!(evaluate(&c, &ctx, &batch).is_err());
    ctx.request_id = "request-1".into();
    c.rules.pop();
    ctx.contract_digest = c.digest().unwrap();
    assert!(evaluate(&c, &ctx, &batch).is_err());
}
#[test]
fn vacuous_contract_is_explicitly_unknown_and_bare_a_report_is_invalid() {
    let (mut c, mut ctx) = fixture();
    c.rules.clear();
    ctx.contract_digest = c.digest().unwrap();
    let batch = collect(&c, &ctx, &mut ModelAdapter::default()).unwrap();
    let out = evaluate(&c, &ctx, &batch).unwrap();
    assert!(out.vacuous);
    assert_eq!(out.verdict().unwrap(), Verdict::Unknown);
    assert!(out.report.verdict().is_err());
}
#[test]
fn required_false_survives_unknown_and_optional_warning() {
    let mut adapter = ModelAdapter {
        mutate: |r, o| {
            if r.id == "position" {
                o.value = Some(ObservedValue::Number {
                    value: 9.0,
                    units: "metre".into(),
                });
            } else {
                o.readback = ReadbackState::Unavailable;
            }
        },
        ..Default::default()
    };
    let out = run(&mut adapter);
    assert_eq!(out.verdict().unwrap(), Verdict::Fail);
    assert_eq!(out.report.validation.checks[1].verdict, Verdict::Unknown);
}
#[test]
fn optional_failure_is_visible_without_erasing_required_pass() {
    let (mut c, mut ctx) = fixture();
    c.rules[1].obligation = Obligation::Preference;
    ctx.contract_digest = c.digest().unwrap();
    let mut adapter = ModelAdapter {
        mutate: |r, o| {
            if r.id == "external-material" {
                o.value = Some(ObservedValue::Preservation {
                    before: Digest::of_bytes(b"a"),
                    after: Digest::of_bytes(b"b"),
                });
            }
        },
        ..Default::default()
    };
    let batch = collect(&c, &ctx, &mut adapter).unwrap();
    let out = evaluate(&c, &ctx, &batch).unwrap();
    assert_eq!(out.verdict().unwrap(), Verdict::Pass);
    assert_eq!(out.report.validation.checks[1].verdict, Verdict::Fail);
    assert!(!out.coverage[1].reasons.is_empty());
}
#[test]
fn causal_ambiguity_is_not_hidden_by_equal_values() {
    let (mut c, mut ctx) = fixture();
    c.rules[0].require_causal_attribution = true;
    ctx.contract_digest = c.digest().unwrap();
    let mut adapter = ModelAdapter {
        mutate: |_, o| o.coverage.attribution = Attribution::Concurrent,
        ..Default::default()
    };
    let batch = collect(&c, &ctx, &mut adapter).unwrap();
    let out = evaluate(&c, &ctx, &batch).unwrap();
    assert_eq!(out.verdict().unwrap(), Verdict::Unknown);
    assert_eq!(out.coverage[0].attribution, Some(Attribution::Concurrent));
}
#[test]
fn truth_table_288_execution_and_required_combinations() {
    struct TruthAdapter {
        first: u8,
        second: u8,
    }
    impl EvidenceAdapter for TruthAdapter {
        fn identity(&self, r: &ResourceKey) -> Option<AdapterIdentity> {
            ModelAdapter::default().identity(r)
        }
        fn observe(
            &mut self,
            ctx: &EvaluationContext,
            rule: &EffectRule,
        ) -> Result<AdapterObservation> {
            let mut o = observation(ctx, rule);
            let mode = if rule.id == "position" {
                self.first
            } else {
                self.second
            };
            match mode {
                0 => {}
                1 => {
                    o.value = Some(if rule.id == "position" {
                        ObservedValue::Number {
                            value: 9.0,
                            units: "metre".into(),
                        }
                    } else {
                        ObservedValue::Preservation {
                            before: Digest::of_bytes(b"a"),
                            after: Digest::of_bytes(b"b"),
                        }
                    })
                }
                2 => o.value = None,
                3 => o.readback = ReadbackState::Error,
                4 => o.readback = ReadbackState::Unsupported,
                _ => o.readback = ReadbackState::Acknowledged,
            }
            Ok(o)
        }
    }
    let statuses = [
        ExecutionStatus::Prepared,
        ExecutionStatus::Applying,
        ExecutionStatus::Completed,
        ExecutionStatus::Partial,
        ExecutionStatus::Denied,
        ExecutionStatus::Cancelled,
        ExecutionStatus::Failed,
        ExecutionStatus::Unknown,
    ];
    let mut cases = 0;
    for status in statuses {
        for first in 0..6 {
            for second in 0..6 {
                let (c, mut ctx) = fixture();
                ctx.execution_status = status;
                let batch = collect(&c, &ctx, &mut TruthAdapter { first, second }).unwrap();
                let expected = if first == 1 || second == 1 {
                    Verdict::Fail
                } else if first == 0 && second == 0 && status == ExecutionStatus::Completed {
                    Verdict::Pass
                } else {
                    Verdict::Unknown
                };
                assert_eq!(
                    evaluate(&c, &ctx, &batch).unwrap().verdict().unwrap(),
                    expected,
                    "{status:?}/{first}/{second}"
                );
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 288);
}

#[test]
fn observation_budget_is_checked_before_native_io() {
    let (c, mut ctx) = fixture();
    ctx.budget.max_observations = 1;
    let mut adapter = ModelAdapter::default();
    assert!(collect(&c, &ctx, &mut adapter).is_err());
    assert_eq!(adapter.calls, 0);
}
