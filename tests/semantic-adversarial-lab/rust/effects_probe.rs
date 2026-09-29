//! Independent G probes for F's effect/evidence contract. Contractual only; no native claim.
use semwright_effect_conformance::composition::*;
use semwright_effect_conformance::*;
use semwright_types::{CommandDescriptor, Idempotency, Risk};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

const SOURCE: &str = env!("G_LAB_COMPILED_SOURCE_SHA");
type ProbeResult<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn d(s: &str) -> Digest {
    Digest::of_bytes(s.as_bytes())
}
fn owner() -> Owner {
    Owner {
        session: "g-effect-session".into(),
        principal: PrincipalBinding::Named("g-principal".into()),
    }
}
fn resource() -> ResourceKey {
    ResourceKey {
        provider: "driver:g-effects".into(),
        resource: "document".into(),
    }
}
fn address() -> Address {
    Address {
        resource: resource(),
        logical_id: "node-a".into(),
        property: "visible".into(),
    }
}
fn other_address() -> Address {
    Address {
        resource: resource(),
        logical_id: "node-b".into(),
        property: "visible".into(),
    }
}
fn base() -> BaseStateSet {
    BaseStateSet(vec![BaseState {
        key: resource(),
        document_id: "g-document".into(),
        provider_session: "provider-session".into(),
        generation: "generation-1".into(),
        revision: Revision::Counter(7),
        concurrency: Concurrency::BestEffortRevalidate,
    }])
}
fn budget(observations: u32) -> ConvergenceBudget {
    ConvergenceBudget {
        max_iterations: 2,
        max_operations: 8,
        max_findings: 8,
        max_observations: observations,
        max_elapsed_ms: 10_000,
    }
}
fn rule_bool() -> EffectRule {
    EffectRule {
        id: "rule-a".into(),
        version: 1,
        obligation: Obligation::Required,
        operation_id: "op-a".into(),
        address: address(),
        predicate: Predicate::Equals {
            expected: ObservedValue::Bool { value: true },
        },
        method: ObservationMethod {
            name: "g-native-property".into(),
            version: 1,
            source: EvidenceSource::NativeApi,
        },
        universe: None,
        artifact: None,
        require_causal_attribution: false,
    }
}
fn contract_with(rules: Vec<EffectRule>) -> EffectContract {
    EffectContract {
        version: EFFECT_CONTRACT_VERSION,
        profile: "g-profile".into(),
        allowed: vec![EffectLimit {
            operation_id: "op-a".into(),
            effects: BTreeSet::from([EffectClass::UpdateOwnedObject]),
            writes: BTreeSet::from([address()]),
        }],
        rules,
    }
}
fn contract() -> EffectContract {
    contract_with(vec![rule_bool()])
}
fn context(c: &EffectContract) -> ProbeResult<EvaluationContext> {
    Ok(EvaluationContext {
        owner: owner(),
        request_id: "request-a".into(),
        plan_digest: d("plan"),
        contract_digest: c.digest()?,
        before: base(),
        after: base(),
        operations: BTreeSet::from(["op-a".into()]),
        observation_scope: BTreeSet::from([address(), other_address()]),
        execution_status: ExecutionStatus::Completed,
        support_level: SupportLevel::Native,
        budget: budget(16),
    })
}
fn observation(c: &EffectContract, ctx: &EvaluationContext) -> AdapterObservation {
    AdapterObservation {
        binding: EvidenceBinding {
            owner: ctx.owner.clone(),
            request_id: ctx.request_id.clone(),
            operation_id: "op-a".into(),
            plan_digest: ctx.plan_digest.clone(),
            contract_digest: c.digest().expect("validated contract"),
        },
        observation: ObservationRef {
            id: "g-observation".into(),
            base: ctx.after.clone(),
            source: EvidenceSource::NativeApi,
            method: "g-native-property".into(),
            method_version: 1,
            scope: vec![address()],
            artifact: None,
            exhaustive: true,
        },
        readback: ReadbackState::Observed,
        value: Some(ObservedValue::Bool { value: true }),
        coverage: ObservationCoverage {
            consistent: true,
            missing: vec![],
            attribution: Attribution::Isolated,
            enumeration: None,
        },
    }
}
fn identity(ctx: &EvaluationContext) -> AdapterIdentity {
    AdapterIdentity {
        owner: ctx.owner.clone(),
        provider: resource().provider,
        provider_session: "provider-session".into(),
        generation: "generation-1".into(),
    }
}
#[derive(Clone)]
struct Adapter {
    identity: AdapterIdentity,
    value: AdapterObservation,
    calls: usize,
    drift_after_observe: bool,
}
impl Adapter {
    fn good(c: &EffectContract, ctx: &EvaluationContext) -> Self {
        Self {
            identity: identity(ctx),
            value: observation(c, ctx),
            calls: 0,
            drift_after_observe: false,
        }
    }
}
impl EvidenceAdapter for Adapter {
    fn identity(&self, _: &ResourceKey) -> Option<AdapterIdentity> {
        let mut id = self.identity.clone();
        if self.drift_after_observe && self.calls > 0 {
            id.generation = "generation-reconnected".into();
        }
        Some(id)
    }
    fn observe(
        &mut self,
        _: &EvaluationContext,
        _: &EffectRule,
    ) -> composition::Result<AdapterObservation> {
        self.calls += 1;
        Ok(self.value.clone())
    }
}
fn evaluated(
    c: &EffectContract,
    ctx: &EvaluationContext,
    adapter: &mut Adapter,
) -> ProbeResult<EffectEvaluation> {
    let batch = collect(c, ctx, adapter)?;
    Ok(evaluate(c, ctx, &batch)?)
}
fn descriptor(name: &str, risk: Risk) -> CommandDescriptor {
    CommandDescriptor {
        name: name.into(),
        version: "1".into(),
        description: "g synthetic descriptor".into(),
        input_schema: json!({"type":"object"}),
        output_schema: json!({"type":"object"}),
        requires: vec![],
        risk,
        idempotency: if risk == Risk::ReadOnly {
            Idempotency::ReadOnly
        } else {
            Idempotency::NonIdempotent
        },
        timeout_ms: 1_000,
        dry_run: false,
        interactive_consent: false,
        backends: vec!["g".into()],
    }
}
fn workflow(
    c: &EffectContract,
    mappings: Vec<ReadbackMapping>,
    persistence: bool,
) -> ProbeResult<(Vec<CommandDescriptor>, ReadbackWorkflow)> {
    let mutation = descriptor("g.mutate", Risk::Mutating);
    let observers = vec![
        descriptor("g.read", Risk::ReadOnly),
        descriptor("g.reopen", Risk::CodeExecution),
    ];
    let mut descriptors = vec![mutation.clone()];
    descriptors.extend(observers);
    Ok((
        descriptors,
        ReadbackWorkflow {
            version: 1,
            workflow: "g-workflow".into(),
            mutation_command: mutation.name.clone(),
            mutation_descriptor: canonical_digest(&mutation)?,
            contract_digest: c.digest()?,
            persistence_required: persistence,
            mappings,
        },
    ))
}
fn required_mapping(
    route: ReadbackRoute,
    command: &CommandDescriptor,
    isolation: bool,
) -> ProbeResult<ReadbackMapping> {
    Ok(ReadbackMapping {
        rule: "rule-a".into(),
        route,
        observer_commands: BTreeMap::from([(command.name.clone(), canonical_digest(command)?)]),
        isolation_required: isolation,
        limitation: None,
    })
}
fn verdict(e: &EffectEvaluation) -> ProbeResult<Verdict> {
    Ok(e.verdict()?)
}

fn probe(case: &str) -> ProbeResult<Value> {
    Ok(match case {
        "G-EFFECT-001" => {
            let c = contract();
            let ctx = context(&c)?;
            let mut a = Adapter::good(&c, &ctx);
            let e = evaluated(&c, &ctx, &mut a)?;
            json!({"verdict":verdict(&e)?,"calls":a.calls,"observed":e.report.effects_observed.len(),"unobservable":e.report.effects_unobservable.len()})
        }
        "G-EFFECT-002" => {
            let c = contract();
            let ctx = context(&c)?;
            let raw = observation(&c, &ctx);
            let batch = collect_untrusted(&c, &ctx, vec![raw])?;
            let e = evaluate(&c, &ctx, &batch)?;
            json!({"verdict":e.verdict()?,"sufficient":e.coverage[0].sufficient})
        }
        "G-EFFECT-003" => {
            let c = contract();
            let ctx = context(&c)?;
            let mut a = Adapter::good(&c, &ctx);
            a.value.binding.owner.session = "foreign".into();
            let e = evaluated(&c, &ctx, &mut a)?;
            json!({"verdict":e.verdict()?,"sufficient":e.coverage[0].sufficient})
        }
        "G-EFFECT-004" => {
            let c = contract();
            let ctx = context(&c)?;
            let mut a = Adapter::good(&c, &ctx);
            a.value.binding.request_id = "old-request".into();
            let e = evaluated(&c, &ctx, &mut a)?;
            json!({"verdict":e.verdict()?})
        }
        "G-EFFECT-005" => {
            let c = contract();
            let ctx = context(&c)?;
            let mut a = Adapter::good(&c, &ctx);
            a.value.binding.plan_digest = d("old-plan");
            let e = evaluated(&c, &ctx, &mut a)?;
            json!({"verdict":e.verdict()?})
        }
        "G-EFFECT-006" => {
            let c = contract();
            let ctx = context(&c)?;
            let mut a = Adapter::good(&c, &ctx);
            a.value.observation.method_version = 2;
            let e = evaluated(&c, &ctx, &mut a)?;
            json!({"verdict":e.verdict()?})
        }
        "G-EFFECT-007" => {
            let mut r = rule_bool();
            r.method.source = EvidenceSource::Fixture;
            let c = contract_with(vec![r]);
            let ctx = context(&c)?;
            let mut a = Adapter::good(&c, &ctx);
            a.value.observation.source = EvidenceSource::Fixture;
            let e = evaluated(&c, &ctx, &mut a)?;
            json!({"verdict":e.verdict()?})
        }
        "G-EFFECT-008" => {
            let c = contract();
            let ctx = context(&c)?;
            let mut a = Adapter::good(&c, &ctx);
            a.value.observation.scope.clear();
            let e = evaluated(&c, &ctx, &mut a)?;
            json!({"verdict":e.verdict()?})
        }
        "G-EFFECT-009" => {
            let c = contract();
            let ctx = context(&c)?;
            let mut a = Adapter::good(&c, &ctx);
            a.value.observation.scope.push(address());
            let e = evaluated(&c, &ctx, &mut a)?;
            json!({"verdict":e.verdict()?})
        }
        "G-EFFECT-010" => {
            let c = contract();
            let mut ctx = context(&c)?;
            ctx.observation_scope.remove(&other_address());
            let mut a = Adapter::good(&c, &ctx);
            a.value.observation.scope.push(other_address());
            let e = evaluated(&c, &ctx, &mut a)?;
            json!({"verdict":e.verdict()?})
        }
        "G-EFFECT-011" => {
            let c = contract();
            let ctx = context(&c)?;
            let mut a = Adapter::good(&c, &ctx);
            a.value.observation.exhaustive = false;
            let e = evaluated(&c, &ctx, &mut a)?;
            json!({"verdict":e.verdict()?})
        }
        "G-EFFECT-012" => {
            let c = contract();
            let ctx = context(&c)?;
            let mut a = Adapter::good(&c, &ctx);
            a.value.coverage.missing.push(address());
            let e = evaluated(&c, &ctx, &mut a)?;
            json!({"verdict":e.verdict()?})
        }
        "G-EFFECT-013" => {
            let c = contract();
            let ctx = context(&c)?;
            let mut a = Adapter::good(&c, &ctx);
            a.value.readback = ReadbackState::RequestEcho;
            let e = evaluated(&c, &ctx, &mut a)?;
            json!({"verdict":e.verdict()?})
        }
        "G-EFFECT-014" => {
            let c = contract();
            let ctx = context(&c)?;
            let mut a = Adapter::good(&c, &ctx);
            a.value.value = None;
            let e = evaluated(&c, &ctx, &mut a)?;
            json!({"verdict":e.verdict()?})
        }
        "G-EFFECT-015" => {
            let mut r = rule_bool();
            r.require_causal_attribution = true;
            let c = contract_with(vec![r]);
            let ctx = context(&c)?;
            let mut a = Adapter::good(&c, &ctx);
            a.value.coverage.attribution = Attribution::Concurrent;
            let e = evaluated(&c, &ctx, &mut a)?;
            json!({"verdict":e.verdict()?})
        }
        "G-EFFECT-016" => {
            let c = contract();
            let ctx = context(&c)?;
            let mut a = Adapter::good(&c, &ctx);
            a.value.value = Some(ObservedValue::Bool { value: false });
            let e = evaluated(&c, &ctx, &mut a)?;
            json!({"verdict":e.verdict()?,"sufficient":e.coverage[0].sufficient})
        }
        "G-EFFECT-017" => {
            let mut r = rule_bool();
            r.predicate = Predicate::Within {
                expected: 1.0,
                tolerance: 0.1,
                units: "m".into(),
            };
            let c = contract_with(vec![r]);
            let ctx = context(&c)?;
            let mut a = Adapter::good(&c, &ctx);
            a.value.value = Some(ObservedValue::Number {
                value: 1.0,
                units: "s".into(),
            });
            let e = evaluated(&c, &ctx, &mut a)?;
            json!({"verdict":e.verdict()?})
        }
        "G-EFFECT-018" => {
            let mut second = rule_bool();
            second.id = "rule-b".into();
            let c = contract_with(vec![rule_bool(), second]);
            let mut ctx = context(&c)?;
            ctx.budget = budget(1);
            let mut a = Adapter::good(&c, &ctx);
            let denied = collect(&c, &ctx, &mut a).is_err();
            json!({"budget_denied_before_io":denied,"calls":a.calls})
        }
        "G-EFFECT-019" => {
            let c = contract();
            let ctx = context(&c)?;
            let mut a = Adapter::good(&c, &ctx);
            a.drift_after_observe = true;
            let e = evaluated(&c, &ctx, &mut a)?;
            json!({"verdict":e.verdict()?,"calls":a.calls})
        }
        "G-EFFECT-020" => {
            let mut r = rule_bool();
            r.predicate = Predicate::Membership {
                expected: BTreeSet::from(["a".into(), "b".into()]),
            };
            r.universe = Some("children".into());
            let c = contract_with(vec![r]);
            let ctx = context(&c)?;
            let binding = ctx.enumeration_binding(&c.rules[0])?;
            let mut a = Adapter::good(&c, &ctx);
            a.value.value = Some(ObservedValue::Members {
                values: BTreeSet::from(["a".into(), "b".into()]),
            });
            a.value.coverage.enumeration = Some(vec![EnumerationPage {
                binding,
                index: 0,
                cursor_in: None,
                cursor_out: None,
                items: vec!["a".into(), "b".into()],
                total: Some(2),
                final_page: true,
                truncated: false,
                consistency: EnumerationConsistency::Snapshot,
            }]);
            let e = evaluated(&c, &ctx, &mut a)?;
            json!({"verdict":e.verdict()?})
        }
        "G-EFFECT-021" => {
            let mut r = rule_bool();
            r.predicate = Predicate::Membership {
                expected: BTreeSet::from(["a".into()]),
            };
            r.universe = Some("children".into());
            let c = contract_with(vec![r]);
            let ctx = context(&c)?;
            let binding = ctx.enumeration_binding(&c.rules[0])?;
            let mut a = Adapter::good(&c, &ctx);
            a.value.value = Some(ObservedValue::Members {
                values: BTreeSet::from(["a".into()]),
            });
            a.value.coverage.enumeration = Some(vec![EnumerationPage {
                binding,
                index: 0,
                cursor_in: None,
                cursor_out: Some("next".into()),
                items: vec!["a".into()],
                total: Some(1),
                final_page: false,
                truncated: false,
                consistency: EnumerationConsistency::Snapshot,
            }]);
            let e = evaluated(&c, &ctx, &mut a)?;
            json!({"verdict":e.verdict()?})
        }
        "G-EFFECT-022" => {
            let mut r = rule_bool();
            r.predicate = Predicate::Cardinality { min: 1, max: 2 };
            r.universe = Some("children".into());
            let c = contract_with(vec![r]);
            let ctx = context(&c)?;
            let binding = ctx.enumeration_binding(&c.rules[0])?;
            let mut a = Adapter::good(&c, &ctx);
            a.value.value = Some(ObservedValue::Members {
                values: BTreeSet::from(["a".into()]),
            });
            a.value.coverage.enumeration = Some(vec![EnumerationPage {
                binding,
                index: 0,
                cursor_in: None,
                cursor_out: None,
                items: vec!["a".into()],
                total: Some(1),
                final_page: true,
                truncated: false,
                consistency: EnumerationConsistency::BestEffort,
            }]);
            let e = evaluated(&c, &ctx, &mut a)?;
            json!({"verdict":e.verdict()?})
        }
        "G-EFFECT-023" => {
            let mut r = rule_bool();
            r.predicate = Predicate::Membership {
                expected: BTreeSet::from(["a".into()]),
            };
            r.universe = Some("children".into());
            let c = contract_with(vec![r]);
            let ctx = context(&c)?;
            let binding = ctx.enumeration_binding(&c.rules[0])?;
            let mut a = Adapter::good(&c, &ctx);
            a.value.value = Some(ObservedValue::Members {
                values: BTreeSet::from(["a".into()]),
            });
            a.value.coverage.enumeration = Some(vec![EnumerationPage {
                binding,
                index: 0,
                cursor_in: None,
                cursor_out: None,
                items: vec!["a".into(), "a".into()],
                total: Some(2),
                final_page: true,
                truncated: false,
                consistency: EnumerationConsistency::Snapshot,
            }]);
            let e = evaluated(&c, &ctx, &mut a)?;
            json!({"verdict":e.verdict()?})
        }
        "G-EFFECT-024" => {
            let op = TypedOperation {
                id: "op-a".into(),
                payload: json!({}),
                reads: vec![],
                writes: vec![address()],
                effects: BTreeSet::from([EffectClass::UpdateOwnedObject]),
                depends_on: vec![],
                postconditions: BTreeSet::new(),
            };
            let changes = ChangeSet {
                atomicity: Atomicity::NonAtomicSequence,
                operations: vec![op],
            };
            let bound = EffectLimit {
                operation_id: "op-a".into(),
                effects: BTreeSet::from([EffectClass::UpdateOwnedObject]),
                writes: BTreeSet::from([address()]),
            };
            let allowed = check_effect_bounds(
                &changes,
                &[bound.clone()],
                &[bound.clone()],
                &[bound.clone()],
            )
            .is_ok();
            let denied = check_effect_bounds(
                &changes,
                &[bound.clone()],
                &[bound.clone()],
                &[EffectLimit {
                    operation_id: "op-a".into(),
                    effects: bound.effects.clone(),
                    writes: BTreeSet::new(),
                }],
            )
            .is_err();
            json!({"intersection_allows_exact":allowed,"broker_missing_write_denied":denied})
        }
        "G-EFFECT-025" => {
            let op = TypedOperation {
                id: "op-a".into(),
                payload: json!({}),
                reads: vec![],
                writes: vec![address()],
                effects: BTreeSet::from([EffectClass::PublishArtifact]),
                depends_on: vec![],
                postconditions: BTreeSet::new(),
            };
            let changes = ChangeSet {
                atomicity: Atomicity::NonAtomicSequence,
                operations: vec![op],
            };
            let narrow = EffectLimit {
                operation_id: "op-a".into(),
                effects: BTreeSet::from([EffectClass::UpdateOwnedObject]),
                writes: BTreeSet::from([address()]),
            };
            json!({"effect_escalation_denied":check_effect_bounds(&changes,&[narrow.clone()],&[narrow.clone()],&[narrow]).is_err()})
        }
        "G-EFFECT-026" => {
            let r = rule_bool();
            let c = contract_with(vec![r.clone(), r]);
            json!({"duplicate_rule_rejected":c.validate().is_err()})
        }
        "G-EFFECT-027" => {
            let mut r = rule_bool();
            r.predicate = Predicate::Absent {
                member: "ghost".into(),
            };
            r.universe = None;
            json!({"global_without_universe_rejected":contract_with(vec![r]).validate().is_err()})
        }
        "G-EFFECT-028" => {
            let mut r = rule_bool();
            r.predicate = Predicate::Within {
                expected: 1.0,
                tolerance: -0.1,
                units: "m".into(),
            };
            json!({"negative_tolerance_rejected":contract_with(vec![r]).validate().is_err()})
        }
        "G-EFFECT-029" => {
            let c = contract();
            let ctx = context(&c)?;
            let mut a = Adapter::good(&c, &ctx);
            let e = evaluated(&c, &ctx, &mut a)?;
            let identity = WorkflowIdentity {
                driver: "g".into(),
                driver_version: "1".into(),
                runtime: "synthetic".into(),
                os: "linux".into(),
                workflow: "g".into(),
                fixture: "g".into(),
                source_sha: SOURCE.into(),
                route: EvidenceRoute::Contractual,
            };
            let mappings = BTreeMap::from([(
                QualityDimension::NativeEvidence,
                BTreeSet::from(["rule-a".into()]),
            )]);
            let q = workflow_quality(
                identity,
                &mappings,
                &e,
                BTreeSet::from(["negative-a".into()]),
            )?;
            let v = q
                .dimensions
                .iter()
                .find(|d| d.dimension == QualityDimension::NativeEvidence)
                .expect("dimension")
                .verdict;
            json!({"native_evidence":v})
        }
        "G-EFFECT-030" => {
            let c = contract();
            let ctx = context(&c)?;
            let mut a = Adapter::good(&c, &ctx);
            let e = evaluated(&c, &ctx, &mut a)?;
            let identity = WorkflowIdentity {
                driver: "g".into(),
                driver_version: "1".into(),
                runtime: "synthetic".into(),
                os: "linux".into(),
                workflow: "g".into(),
                fixture: "g".into(),
                source_sha: SOURCE.into(),
                route: EvidenceRoute::NativeAdapter,
            };
            let mappings = BTreeMap::from([(
                QualityDimension::Conformance,
                BTreeSet::from(["rule-a".into()]),
            )]);
            let q = workflow_quality(identity, &mappings, &e, BTreeSet::new())?;
            let v = q
                .dimensions
                .iter()
                .find(|d| d.dimension == QualityDimension::Conformance)
                .expect("dimension")
                .verdict;
            json!({"conformance_without_negative_cases":v})
        }
        "G-EFFECT-031" => {
            let c = contract();
            let mutation = descriptor("g.mutate", Risk::ReadOnly);
            let w = ReadbackWorkflow {
                version: 1,
                workflow: "g".into(),
                mutation_command: mutation.name.clone(),
                mutation_descriptor: canonical_digest(&mutation)?,
                contract_digest: c.digest()?,
                persistence_required: false,
                mappings: vec![],
            };
            json!({"readonly_mutation_descriptor_rejected":lint_readback(&[mutation],&c,&w).is_err()})
        }
        "G-EFFECT-032" => {
            let c = contract();
            let mutation = descriptor("g.mutate", Risk::Mutating);
            let reader = descriptor("g.read", Risk::ReadOnly);
            let mapping = required_mapping(ReadbackRoute::FileReopen, &reader, true)?;
            let w = ReadbackWorkflow {
                version: 1,
                workflow: "g".into(),
                mutation_command: mutation.name.clone(),
                mutation_descriptor: canonical_digest(&mutation)?,
                contract_digest: c.digest()?,
                persistence_required: false,
                mappings: vec![mapping],
            };
            json!({"reopen_cannot_masquerade_readonly":lint_readback(&[mutation,reader],&c,&w).is_err()})
        }
        "G-EFFECT-033" => {
            let c = contract();
            let (descriptors, w) = workflow(&c, vec![], false)?;
            json!({"missing_required_mapping_rejected":lint_readback(&descriptors,&c,&w).is_err()})
        }
        "G-EFFECT-034" => {
            let c = contract();
            let unavailable = ReadbackMapping {
                rule: "rule-a".into(),
                route: ReadbackRoute::Unavailable,
                observer_commands: BTreeMap::new(),
                isolation_required: false,
                limitation: Some("synthetic unavailable".into()),
            };
            let (descriptors, w) = workflow(&c, vec![unavailable], false)?;
            let ctx = context(&c)?;
            let mut a = Adapter::good(&c, &ctx);
            let e = run_readback_conformance(&descriptors, &w, &c, &ctx, &mut a)?;
            json!({"verdict":e.verdict()?,"calls":a.calls})
        }
        "G-EFFECT-035" => {
            let c = contract();
            let ctx = context(&c)?;
            let mut changed = contract();
            changed.rules[0].version = 2;
            let mut a = Adapter::good(&c, &ctx);
            let denied = collect(&changed, &ctx, &mut a).is_err();
            json!({"changed_contract_denied_before_io":denied,"calls":a.calls})
        }
        "G-EFFECT-036" => {
            let mut r = rule_bool();
            r.method.source = EvidenceSource::HumanReview;
            let c = contract_with(vec![r]);
            let ctx = context(&c)?;
            let mut a = Adapter::good(&c, &ctx);
            a.value.observation.source = EvidenceSource::HumanReview;
            let e = evaluated(&c, &ctx, &mut a)?;
            json!({"verdict":e.verdict()?})
        }
        "G-EFFECT-037" => {
            let c = contract();
            let mut ctx = context(&c)?;
            ctx.support_level = SupportLevel::Unsupported;
            let mut a = Adapter::good(&c, &ctx);
            let e = evaluated(&c, &ctx, &mut a)?;
            json!({"verdict":e.verdict()?})
        }
        "G-EFFECT-038" => {
            let mut r = rule_bool();
            r.method.source = EvidenceSource::RendererState;
            let c = contract_with(vec![r]);
            let ctx = context(&c)?;
            let mut a = Adapter::good(&c, &ctx);
            a.value.observation.source = EvidenceSource::RendererState;
            let e = evaluated(&c, &ctx, &mut a)?;
            json!({"verdict":e.verdict()?})
        }
        "G-EFFECT-039" => {
            let predicate = Predicate::Reopened;
            let bad = ObservedValue::Reopened {
                writer_process: "same".into(),
                reader_process: "same".into(),
                before_projection: d("p"),
                after_projection: d("p"),
                saved_digest: d("s"),
                reopened_digest: d("s"),
            };
            let good = ObservedValue::Reopened {
                writer_process: "writer".into(),
                reader_process: "reader".into(),
                before_projection: d("p"),
                after_projection: d("p"),
                saved_digest: d("s"),
                reopened_digest: d("s"),
            };
            json!({"same_process_fails":predicate.compare(&bad)?==Some(false),"fresh_process_passes":predicate.compare(&good)?==Some(true)})
        }
        "G-EFFECT-040" => {
            let parsed: std::result::Result<Predicate, _> =
                serde_json::from_value(json!({"kind":"eval","code":"return true"}));
            json!({"arbitrary_expression_predicate_rejected":parsed.is_err()})
        }
        _ => {
            eprintln!("unregistered effects selector");
            std::process::exit(2)
        }
    })
}
fn cases() -> Vec<String> {
    (1..=40).map(|i| format!("G-EFFECT-{i:03}")).collect()
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 1 || std::env::var("G_LAB_TARGET_SHA").as_deref() != Ok(SOURCE) {
        std::process::exit(2)
    }
    if args[0] == "--list" {
        println!(
            "{}",
            json!({"schema_version":1,"source_sha":SOURCE,"cases":cases()})
        );
        return;
    }
    if !cases().contains(&args[0]) {
        std::process::exit(2)
    }
    match probe(&args[0]) {
        Ok(observed) => println!(
            "{}",
            json!({"schema_version":1,"source_sha":SOURCE,"case_id":args[0],"observed":observed})
        ),
        Err(error) => {
            eprintln!("effects probe contract/setup error: {error:?}");
            std::process::exit(1)
        }
    }
}
