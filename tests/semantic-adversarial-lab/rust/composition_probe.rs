//! Independent G contract probe; synthetic values do not prove native acceptance.
//! Compiled as a declared example overlay in a disposable, exact-SHA build copy.
use semwright_semantic_composition::*;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

const SOURCE: &str = env!("G_LAB_COMPILED_SOURCE_SHA");
fn owner() -> Owner {
    Owner { session: "g-synthetic-session".into(), principal: PrincipalBinding::Named("g-principal".into()) }
}
fn budget() -> ConvergenceBudget {
    ConvergenceBudget { max_iterations: 3, max_operations: 6, max_findings: 4, max_observations: 3, max_elapsed_ms: 10_000 }
}
fn base() -> BaseStateSet {
    BaseStateSet(["a", "b"].into_iter().map(|suffix| BaseState {
        key: ResourceKey { provider: "driver:g-synthetic".into(), resource: format!("document-{suffix}") },
        document_id: format!("logical-document-{suffix}"), provider_session: format!("native-{suffix}"),
        generation: "generation-1".into(), revision: Revision::Counter(19),
        concurrency: Concurrency::BestEffortRevalidate,
    }).collect())
}
fn address() -> Address {
    Address { resource: base().0[0].key.clone(), logical_id: "synthetic-node-a".into(), property: "transform".into() }
}
fn rules() -> BTreeSet<String> {
    ["required-a", "required-b", "required-c"].into_iter().map(String::from).collect()
}
fn profile() -> ProfileDescriptor {
    let effects = BTreeSet::from([EffectClass::UpdateOwnedObject]);
    ProfileDescriptor {
        identity: ProfileIdentity { id: "g-synthetic-profile".into(), version: 1,
            intent_schema: Digest::of_bytes(b"g-intent-v1"), operation_schema: Digest::of_bytes(b"g-operation-v1") },
        capabilities: vec![CapabilityBinding { phase: Phase::Apply, command: "g.synthetic.apply".into(),
            descriptor: Digest::of_bytes(b"g-descriptor"), effects: effects.clone() }],
        required_rules: rules(), allowed_effects: effects,
    }
}
fn plan(b: ConvergenceBudget) -> Result<PreparedPlan<Value, Value>> {
    let p = profile();
    let intent = json!({"document":"a", "value":7});
    PreparedPlan::prepare(PlanBody {
        contract_version: CONTRACT_VERSION, profile: p.identity.clone(), owner: owner(), base: base(),
        intent_digest: canonical_digest(&intent)?, intent,
        dependencies: BTreeMap::from([("source".into(), Digest::of_bytes(b"synthetic-source"))]),
        changes: ChangeSet { atomicity: Atomicity::NonAtomicSequence, operations: vec![TypedOperation {
            id: "operation-a".into(), payload: json!({"set":7}), reads: vec![address()], writes: vec![address()],
            effects: BTreeSet::from([EffectClass::UpdateOwnedObject]), depends_on: vec![], postconditions: rules(),
        }] },
        required_rules: rules(), observation_scope: vec![address()], budget: b, require_compare_and_swap: false,
    }, &p)
}
fn rehash(p: &mut PreparedPlan<Value, Value>) -> Result<()> {
    p.body.intent_digest = canonical_digest(&p.body.intent)?;
    p.digest = canonical_digest(&p.body)?;
    Ok(())
}
fn vault() -> PlanVault { PlanVault::bounded(16, 8, 32) }
fn issued(ops: u32) -> Result<(PlanVault, PreparedPlan<Value, Value>)> {
    let p = plan(budget())?;
    let mut v = vault();
    v.issue(&owner(), "root", &p, budget(), ops, None, false)?;
    Ok((v, p))
}
fn complete(v: &mut PlanVault, p: &PreparedPlan<Value, Value>) -> Result<()> {
    let permit = v.begin(&owner(), "root", p, "request-a")?;
    v.finish(permit, ExecutionStatus::Completed, vec!["synthetic-write:a".into()])
}
fn report() -> ValidationReport {
    ValidationReport {
        plan_digest: Digest::of_bytes(b"g-synthetic-plan"), base: base(), required_rules: rules(),
        checks: rules().into_iter().map(|rule| RuleResult {
            rule: rule.clone(), version: 1, verdict: Verdict::Pass, evidence_class: EvidenceClass::Deterministic,
            evidence: vec![ObservationRef { id: format!("observation-{rule}"), base: base(),
                source: EvidenceSource::NativeApi, method: "synthetic-contract-label-not-native-evidence".into(),
                method_version: 1, scope: vec![address()], artifact: Some(Digest::of_bytes(b"synthetic-artifact")), exhaustive: true }],
            reason: None,
        }).collect(),
    }
}
fn controller(r: &ValidationReport) -> Result<Controller> {
    let mut c = Controller::new(r.plan_digest.clone(), budget(), r.required_rules.clone())?;
    c.applying(false)?;
    c.executed(ExecutionStatus::Completed)?;
    Ok(c)
}
fn kind<T>(r: Result<T>) -> &'static str {
    match r {
        Ok(_) => "OK", Err(ContractError::Unknown(_)) => "UNKNOWN", Err(ContractError::Stale(_)) => "STALE",
        Err(ContractError::Denied(_)) => "DENIED", Err(ContractError::Limit(_)) => "LIMIT", Err(ContractError::Invalid(_)) => "INVALID",
    }
}

fn probe(case: &str) -> Result<Value> {
    Ok(match case {
        "G-PLAN-001" => {
            let (mut v, p) = issued(2)?;
            let permit = v.begin(&owner(), "root", &p, "request-a")?;
            let before = v.ledger(&owner(), "root")?[0].status;
            v.finish(permit, ExecutionStatus::Completed, vec!["synthetic-write:a".into()])?;
            json!({"reserved_before_effect":before == ExecutionStatus::Applying,
                   "attempts":v.ledger(&owner(), "root")?.len(), "receipt":v.ledger(&owner(), "root")?[0].effects})
        }
        "G-PLAN-002" => {
            let (mut v, mut p) = issued(1)?;
            let mut foreign = address(); foreign.logical_id = "synthetic-node-outside-approved-write-set".into();
            p.body.changes.operations[0].writes.push(foreign); rehash(&mut p)?;
            json!({"rehashed_structural_plan_valid":p.verify(&profile()).is_ok(),
                   "vault_denied":v.begin(&owner(), "root", &p, "changed-write-set").is_err(),
                   "attempts":v.ledger(&owner(), "root")?.len()})
        }
        "G-PLAN-003" | "G-PLAN-004" => {
            let (mut v, p) = issued(1)?;
            let mut other = owner();
            if case.ends_with("003") { other.principal = PrincipalBinding::Named("foreign-principal".into()); }
            else { other.session = "foreign-session".into(); }
            json!({"begin_denied":v.begin(&other, "root", &p, "foreign-request").is_err(),
                   "ledger_denied":v.ledger(&other, "root").is_err(), "owner_unmodified":v.ledger(&owner(), "root")?.is_empty()})
        }
        "G-PLAN-005" => {
            let (mut v, p) = issued(1)?;
            json!({"unissued_denied":v.begin(&owner(), "unissued", &p, "r").is_err()})
        }
        "G-PLAN-006" => {
            let (v, p) = issued(1)?;
            let mut denied = 0;
            for index in 0..6 {
                let mut changed = p.clone();
                match index {
                    0 => changed.body.intent = json!({"document":"b", "value":8}),
                    1 => changed.body.changes.operations[0].payload = json!({"set":8}),
                    2 => changed.body.dependencies.insert("source".into(), Digest::of_bytes(b"changed" )).map(|_| ()).unwrap_or(()),
                    3 => changed.body.base.0[0].generation = "generation-2".into(),
                    4 => changed.body.observation_scope[0].logical_id = "other-node".into(),
                    _ => changed.body.changes.atomicity = Atomicity::InMemoryTransaction,
                }
                rehash(&mut changed)?;
                if v.matches(&owner(), "root", &changed).is_err() { denied += 1; }
            }
            json!({"bound_fields_denied":denied})
        }
        "G-PLAN-007" => {
            let mut p = plan(budget())?; p.body.required_rules.remove("required-c"); rehash(&mut p)?;
            json!({"required_suppression_rejected":p.verify(&profile()).is_err()})
        }
        "G-PLAN-008" => {
            let p = plan(budget())?; let mut rejected = 0;
            for n in 0..4 {
                let mut q = p.clone();
                match n { 0 => q.body.profile.version += 1, 1 => q.body.profile.intent_schema = Digest::of_bytes(b"wrong"),
                    2 => q.body.profile.operation_schema = Digest::of_bytes(b"wrong"), _ => q.body.contract_version += 1 }
                rehash(&mut q)?; if q.verify(&profile()).is_err() { rejected += 1; }
            }
            json!({"profile_substitutions_rejected":rejected})
        }
        "G-PLAN-009" => {
            let mut p = plan(budget())?; p.body.changes.operations[0].effects.insert(EffectClass::PublishArtifact); rehash(&mut p)?;
            json!({"effect_escalation_rejected":p.verify(&profile()).is_err()})
        }
        "G-PLAN-010" => {
            let (v, mut p) = issued(1)?;
            p.body.changes.operations[0].writes[0].resource = base().0[1].key.clone(); rehash(&mut p)?;
            json!({"structurally_valid":p.verify(&profile()).is_ok(),"transplant_denied":v.matches(&owner(), "root", &p).is_err()})
        }
        "G-PLAN-011" => {
            let (mut v, p) = issued(1)?; complete(&mut v, &p)?;
            v.issue(&owner(), "repair", &p, budget(), 1, Some("root"), true)?;
            let denied = v.begin(&owner(), "repair", &p, "request-a").is_err();
            let new = v.begin(&owner(), "repair", &p, "request-b").is_ok();
            json!({"root_request_replay_denied":denied,"fresh_request_still_possible":new,"attempts":v.ledger(&owner(), "root")?.len()})
        }
        "G-PLAN-012" => {
            let mut retries = 0; let mut children = 0;
            for status in [ExecutionStatus::Partial, ExecutionStatus::Unknown, ExecutionStatus::Failed, ExecutionStatus::Cancelled, ExecutionStatus::Denied] {
                let (mut v, p) = issued(1)?; let permit = v.begin(&owner(), "root", &p, "request-a")?;
                v.finish(permit, status, vec!["synthetic-uncertain-effect".into()])?;
                retries += usize::from(v.begin(&owner(), "root", &p, "retry").is_err());
                children += usize::from(v.issue(&owner(), "repair", &p, budget(), 1, Some("root"), true).is_err());
            }
            json!({"unsafe_retries_denied":retries,"unsafe_repairs_denied":children})
        }
        "G-PLAN-013" => {
            let (mut v, p) = issued(1)?; let permit = v.begin(&owner(), "root", &p, "lost-dispatch")?; drop(permit);
            json!({"still_applying":v.ledger(&owner(), "root")?[0].status == ExecutionStatus::Applying,
                   "retry_denied":v.begin(&owner(), "root", &p, "retry").is_err(),
                   "repair_denied":v.issue(&owner(), "repair", &p, budget(), 1, Some("root"), true).is_err()})
        }
        "G-PLAN-014" => {
            let (mut v, p) = issued(1)?; complete(&mut v, &p)?;
            v.issue(&owner(), "root", &p, budget(), 1, None, false)?;
            json!({"reissue_did_not_reset":v.begin(&owner(), "root", &p, "retry").is_err(),"attempts":v.ledger(&owner(), "root")?.len()})
        }
        "G-PLAN-015" => {
            let (mut v, p) = issued(5)?; complete(&mut v, &p)?;
            v.issue(&owner(), "repair", &p, budget(), 2, Some("root"), true)?;
            json!({"aggregate_limit":kind(v.begin(&owner(), "repair", &p, "request-b")),"attempts":v.ledger(&owner(), "root")?.len()})
        }
        "G-PLAN-016" => {
            let (mut v, p) = issued(1)?; complete(&mut v, &p)?; let mut rejected = 0;
            for n in 0..5 {
                let mut b = budget(); match n {0=>b.max_iterations+=1,1=>b.max_operations+=1,2=>b.max_findings+=1,3=>b.max_observations+=1,_=>b.max_elapsed_ms+=1};
                rejected += usize::from(v.issue(&owner(), &format!("repair-{n}"), &p, b, 1, Some("root"), true).is_err());
            }
            json!({"budget_dimensions_bound":rejected})
        }
        "G-PLAN-017" => {
            let (mut v, p) = issued(1)?; v.record_observation(&owner(), "root", 4)?; complete(&mut v, &p)?;
            v.issue(&owner(), "repair", &p, budget(), 1, Some("root"), true)?;
            let excess_findings = v.record_observation(&owner(), "repair", 5).is_err();
            v.record_observation(&owner(), "repair", 1)?; v.record_observation(&owner(), "root", 0)?;
            json!({"findings_limit":excess_findings,"shared_observation_limit":v.record_observation(&owner(), "repair", 0).is_err()})
        }
        "G-PLAN-018" => {
            let (mut v, p) = issued(6)?; complete(&mut v, &p)?;
            v.issue(&owner(), "independent-root", &p, budget(), 6, None, false)?;
            json!({"independent_root_allowed":v.begin(&owner(), "independent-root", &p, "independent-request").is_ok()})
        }
        "G-PLAN-019" => {
            let (mut v, p) = issued(1)?; let mut other = owner(); other.session = "other-session".into();
            v.issue(&other, "root", &p, budget(), 1, None, false)?; v.revoke(&owner());
            json!({"revoked_denied":v.begin(&owner(), "root", &p, "r").is_err(),"other_unchanged":v.matches(&other, "root", &p).is_ok()})
        }
        "G-PLAN-020" => {
            let (_, p) = issued(1)?;
            json!({"restart_loses_authority":vault().begin(&owner(), "root", &p, "r").is_err()})
        }
        "G-PLAN-021" => {
            let (mut v, p) = issued(1)?; let old = v.begin(&owner(), "root", &p, "old-request")?;
            v.revoke(&owner());
            json!({"completion_after_revoke_denied":v.finish(old, ExecutionStatus::Completed, vec![]).is_err()})
        }
        "G-PLAN-022" => {
            let (mut v, p) = issued(1)?; let old = v.begin(&owner(), "root", &p, "old-request")?;
            v.revoke(&owner()); v.issue(&owner(), "root", &p, budget(), 1, None, false)?;
            let _current = v.begin(&owner(), "root", &p, "new-request")?;
            let accepted = v.finish(old, ExecutionStatus::Completed, vec!["synthetic-old-receipt".into()]).is_ok();
            json!({"old_epoch_completion_accepted":accepted,"new_attempt_still_applying":v.ledger(&owner(), "root")?[0].status == ExecutionStatus::Applying})
        }
        "G-PLAN-023" => {
            let (mut first, p) = issued(1)?; let old = first.begin(&owner(), "root", &p, "first-request")?;
            let mut second = vault(); let mut changed = p.clone(); changed.body.intent = json!({"document":"b","value":91}); rehash(&mut changed)?;
            second.issue(&owner(), "root", &changed, budget(), 1, None, false)?;
            let _current = second.begin(&owner(), "root", &changed, "second-request")?;
            json!({"foreign_vault_completion_accepted":second.finish(old, ExecutionStatus::Completed, vec![]).is_ok()})
        }
        "G-PLAN-024" => {
            let mut b = budget(); b.max_elapsed_ms = 200; let p = plan(b.clone())?; let mut v = vault();
            v.issue(&owner(), "root", &p, b.clone(), 1, None, false)?;
            let old = v.begin(&owner(), "root", &p, "expired-request")?;
            std::thread::sleep(Duration::from_millis(450));
            v.issue(&owner(), "root", &p, b, 1, None, false)?;
            let _current = v.begin(&owner(), "root", &p, "current-request")?;
            json!({"expired_epoch_completion_accepted":v.finish(old, ExecutionStatus::Completed, vec![]).is_ok()})
        }
        "G-PLAN-025" => {
            let mut b = budget(); b.max_elapsed_ms = 200; let p = plan(b.clone())?; let mut v = vault();
            v.issue(&owner(), "root", &p, b, 1, None, false)?;
            std::thread::sleep(Duration::from_millis(450));
            json!({"expired_begin":kind(v.begin(&owner(), "root", &p, "expired-request"))})
        }
        "G-PLAN-026" => {
            let (mut v, p) = issued(1)?; let permit = v.begin(&owner(), "root", &p, "invalid-completion")?;
            let invalid = v.finish(permit, ExecutionStatus::Prepared, vec![]).is_err();
            json!({"nonterminal_finish_rejected":invalid,"still_applying":v.ledger(&owner(), "root")?[0].status == ExecutionStatus::Applying,
                   "reservation_not_refunded":v.begin(&owner(), "root", &p, "retry").is_err()})
        }
        "G-PLAN-027" => {
            let (mut v, p) = issued(1)?;
            json!({"unobserved_parent_denied":v.issue(&owner(), "repair-a", &p, budget(), 1, Some("root"), true).is_err(),
                   "unclassified_child_denied":v.issue(&owner(), "repair-b", &p, budget(), 1, Some("root"), false).is_err()})
        }
        "G-PLAN-028" => {
            let mut v = PlanVault::bounded(2, 1, 1); let p = plan(budget())?;
            v.issue(&owner(), "root", &p, budget(), 1, None, false)?;
            let root_limit = kind(v.issue(&owner(), "root-b", &p, budget(), 1, None, false));
            complete(&mut v, &p)?; v.issue(&owner(), "repair", &p, budget(), 1, Some("root"), true)?;
            json!({"root_capacity":root_limit,"attempt_capacity":kind(v.begin(&owner(), "repair", &p, "request-b")),
                   "plan_capacity":kind(v.issue(&owner(), "repair-b", &p, budget(), 1, Some("root"), true))})
        }
        "G-PLAN-029" => {
            let p = plan(budget())?; let mut rejected = 0;
            for n in 0..3 {
                let mut q = p.clone();
                match n {0=>q.body.changes.operations.clear(),1=>q.body.changes.operations[0].depends_on.push("operation-a".into()),
                    _=>q.body.changes.operations[0].writes[0].resource.resource="not-in-base".into()}
                rehash(&mut q)?; rejected += usize::from(q.verify(&profile()).is_err());
            }
            json!({"invalid_structures_rejected":rejected})
        }
        "G-PLAN-030" => {
            let mut r = report(); let mut c = controller(&r)?; r.plan_digest = Digest::of_bytes(b"old-candidate");
            json!({"old_candidate_denied":c.observed(&r, vec![0], 0, 2).is_err()})
        }
        "G-PLAN-031" => {
            let mut r = report(); let mut c = controller(&r)?;
            r.required_rules.remove("required-c"); r.checks.pop();
            json!({"scope_suppression_denied":c.observed(&r, vec![0], 0, 2).is_err()})
        }
        "G-PLAN-032" => {
            let r = report(); let mut c = controller(&r)?;
            json!({"positive_complete":matches!(c.observed(&r, vec![0], 0, 2)?, Decision::Stop(StopReason::Complete)),
                   "verified_state":c.state == State::Verified})
        }
        "G-PLAN-033" => {
            let mut r = report(); r.checks[0].verdict = Verdict::Fail; let mut c = controller(&r)?;
            let first = matches!(c.observed(&r, vec![8], 1, 2)?, Decision::PlanRepair);
            let next = Digest::of_bytes(b"repair-candidate"); c.bind_repair(next.clone())?;
            c.applying(true)?; c.executed(ExecutionStatus::Completed)?; r.plan_digest = next;
            json!({"repair_initially_allowed":first,"stopped_without_progress":matches!(c.observed(&r, vec![8], 1, 3)?, Decision::Stop(StopReason::NoProgress))})
        }
        "G-PLAN-034" => {
            let mut r = report(); r.checks[0].verdict = Verdict::Fail; let mut c = controller(&r)?;
            json!({"ambiguous_repair_stopped":matches!(c.observed(&r, vec![8], 2, 2)?, Decision::Stop(StopReason::Ambiguous))})
        }
        "G-PLAN-035" => {
            let r = report(); let mut c = Controller::new(r.plan_digest.clone(), budget(), r.required_rules.clone())?;
            c.applying(false)?; c.executed(ExecutionStatus::Partial)?;
            json!({"partial_state":c.state == State::PartiallyApplied,"unsafe_retry_denied":c.applying(false).is_err()})
        }
        "G-PLAN-036" => {
            let r = report(); let mut c = controller(&r)?; c.cancel();
            json!({"cancelled_state":c.state == State::Cancelled,"apply_after_cancel_denied":c.applying(false).is_err(),
                   "ready_after_cancel_denied":c.observed(&r, vec![0], 0, 2).is_err()})
        }
        "G-PLAN-037" => {
            let original = base(); let mut rejected = 0;
            for n in 0..3 { let mut observed = original.clone();
                match n {0=>observed.0[0].generation="replacement".into(),1=>observed.0[0].provider_session="reconnected".into(),_=>observed.0[0].document_id="same-bytes-other-object".into()}
                rejected += usize::from(original.check_fresh(&observed, false).is_err());
            }
            json!({"identity_changes_rejected":rejected})
        }
        "G-PLAN-038" => {
            let original = base(); let mut observed = original.clone(); observed.0.reverse();
            json!({"same_resource_set_fresh":original.check_fresh(&observed, false).is_ok()})
        }
        "G-PLAN-039" => {
            let mut unknown = base(); unknown.0[0].revision = Revision::Unknown;
            let mut missing = base(); missing.0.pop();
            json!({"unobservable":kind(unknown.check_fresh(&unknown, false)),"missing_membership":kind(base().check_fresh(&missing, false))})
        }
        "G-PLAN-040" => {
            let b = base(); let mut cas = base(); for item in &mut cas.0 {item.concurrency=Concurrency::CompareAndSwap;}
            json!({"best_effort_is_not_cas":kind(b.check_fresh(&b,true)),"declared_cas_contract_accepted":cas.check_fresh(&cas,true).is_ok()})
        }
        "G-VERIFY-001" => json!({"verdict":report().verdict()?}),
        "G-VERIFY-002" => {
            let mut table = vec![];
            for a in [Verdict::Pass, Verdict::Fail, Verdict::Unknown] {
                for b in [Verdict::Pass, Verdict::Fail, Verdict::Unknown] {
                    for c in [Verdict::Pass, Verdict::Fail, Verdict::Unknown] {
                        let mut r=report(); for (check,v) in r.checks.iter_mut().zip([a,b,c]) {check.verdict=v;}
                        table.push(r.verdict()?);
                    }
                }
            }
            json!({"truth_table":table})
        }
        "G-VERIFY-003" => { let mut r=report(); r.checks.pop(); json!({"verdict":r.verdict()?}) }
        "G-VERIFY-004" => { let mut r=report(); r.checks.push(r.checks[2].clone()); json!({"duplicate_rejected":r.verdict().is_err()}) }
        "G-VERIFY-005" => {
            let mut output=vec![]; for index in 0..3 {let mut r=report();r.checks[index].evidence[0].exhaustive=false;output.push(r.verdict()?);}
            json!({"verdicts":output})
        }
        "G-VERIFY-006" => {
            let mut output=vec![]; for source in [EvidenceSource::Fixture,EvidenceSource::Simulation] {let mut r=report();r.checks[1].evidence[0].source=source;output.push(r.verdict()?);}
            json!({"verdicts":output})
        }
        "G-VERIFY-007" => {
            let mut output=vec![]; for class in [EvidenceClass::Heuristic,EvidenceClass::AestheticAssist] {let mut r=report();r.checks[1].evidence_class=class;output.push(r.verdict()?);}
            json!({"verdicts":output})
        }
        "G-VERIFY-008" => { let mut r=report();r.checks[2].evidence[0].base.0[1].revision=Revision::Counter(18);json!({"verdict":r.verdict()?}) }
        "G-VERIFY-009" => { let mut r=report();r.checks[1].evidence[0].method_version=0;json!({"invalid_method_can_pass":r.verdict().is_ok_and(|v|v==Verdict::Pass)}) }
        "G-VERIFY-010" => { let mut r=report();r.checks[1].evidence[0].scope[0].resource.resource="outside-observed-base".into();json!({"foreign_scope_can_pass":r.verdict().is_ok_and(|v|v==Verdict::Pass)}) }
        "G-VERIFY-011" => {
            let r=VerificationReport{execution_status:ExecutionStatus::Completed,validation:report(),support_level:SupportLevel::Native,
                effects_observed:vec![],effects_unobservable:vec![address()]};
            json!({"unobservable_required_effect_can_pass":r.verdict().is_ok_and(|v|v==Verdict::Pass)})
        }
        "G-VERIFY-012" => {
            let mut positive=vec![];let mut negative=vec![];
            for status in [ExecutionStatus::Prepared,ExecutionStatus::Applying,ExecutionStatus::Completed,ExecutionStatus::Partial,ExecutionStatus::Denied,ExecutionStatus::Cancelled,ExecutionStatus::Failed,ExecutionStatus::Unknown] {
                let mut r=VerificationReport{execution_status:status,validation:report(),support_level:SupportLevel::Native,effects_observed:vec![address()],effects_unobservable:vec![]};
                positive.push(r.verdict()?);r.validation.checks[0].verdict=Verdict::Fail;negative.push(r.verdict()?);
            }
            json!({"positive_validation":positive,"failed_validation":negative})
        }
        "G-VERIFY-013" => {
            let mut r=report();let mut optional=r.checks[0].clone();optional.rule="explicitly-optional".into();optional.verdict=Verdict::Fail;r.checks.push(optional);
            json!({"required_scope_verdict":r.verdict()?})
        }
        "G-VERIFY-014" => { let mut r=report();r.checks.pop();r.checks[0].verdict=Verdict::Fail;json!({"verdict":r.verdict()?}) }
        "G-VERIFY-015" => { let mut r=report();r.checks[2].evidence.clear();json!({"verdict":r.verdict()?}) }
        "G-VERIFY-016" => { let mut r=report();r.checks[2].version=0;json!({"invalid_rule_version_rejected":r.verdict().is_err()}) }
        "G-VERIFY-017" => { let mut r=report();r.base.0.push(r.base.0[0].clone());json!({"ambiguous_base_rejected":r.verdict().is_err()}) }
        "G-VERIFY-018" => { let mut r=report();r.checks=vec![r.checks[0].clone();257];json!({"check_budget_rejected":r.verdict().is_err()}) }
        "G-VERIFY-019" => { let mut r=report();r.required_rules.clear();r.checks.clear();json!({"vacuous_pass_rejected":r.verdict().is_err()}) }
        "G-VERIFY-020" => { let mut r=report();r.checks[0].verdict=Verdict::Unknown;r.checks[1].reason=Some("optional warning must not hide required unknown".into());json!({"verdict":r.verdict()?}) }
        "G-CODEC-001" => json!({"escaped_duplicate_rejected":strict_decode::<Value>(br#"{"outer":{"a":1,"\u0061":2}}"#).is_err()}),
        "G-CODEC-002" => {
            let input = serde_json::to_vec(&vec![0; 4097]).expect("synthetic JSON");
            json!({"entry_limit_rejected":strict_decode::<Value>(&input).is_err()})
        }
        "G-CODEC-003" => {
            let input = format!("{}0{}", "[".repeat(65), "]".repeat(65));
            json!({"depth_limit_rejected":strict_decode::<Value>(input.as_bytes()).is_err()})
        }
        "G-CODEC-004" => json!({"trailing_data_rejected":strict_decode::<Value>(b"{} []").is_err()}),
        "G-CODEC-005" => json!({"invalid_utf8_rejected":strict_decode::<Value>(&[b'"',255,b'"']).is_err()}),
        "G-CODEC-006" => json!({"nonfinite_number_rejected":strict_decode::<Value>(b"1e400").is_err()}),
        "G-CODEC-007" => {
            let input = json!({"unicode":"á𝄞שלום", "missing":null, "ordered":[1,2]});
            json!({"roundtrip":strict_decode::<Value>(&canonical_bytes(&input)?)? == input,
                "array_order_preserved":canonical_digest(&json!([1,2]))? != canonical_digest(&json!([2,1]))?})
        }
        "G-CODEC-008" => json!({"key_order_canonical":canonical_digest(&json!({"z":2,"a":1}))? == canonical_digest(&json!({"a":1,"z":2}))?,
                "unicode_forms_not_silently_normalized":canonical_digest(&json!("é"))? != canonical_digest(&json!("e\u{301}"))?}),
        "G-CODEC-009" => {
            let mut seed=0x715c_u64; let mut success=0;
            for _ in 0..64 {
                seed=seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                let input=json!({"n":(seed >> 40) as u32,"nil":null,"s":format!("fixture-{}",seed%193),"list":[true,false]});
                success += usize::from(strict_decode::<Value>(&canonical_bytes(&input)?)? == input);
            }
            json!({"seeded_roundtrips":success})
        }
        "G-CODEC-010" => {
            let input=br#"{"session":"g-synthetic","principal":"host_session","grant":true}"#;
            json!({"unknown_authority_field_rejected":strict_decode::<Owner>(input).is_err()})
        }
        _ => return Err(ContractError::Invalid("unknown independent case selector".into())),
    })
}
fn cases() -> Vec<String> {
    [("PLAN",40),("VERIFY",20),("CODEC",10)].into_iter()
        .flat_map(|(family,count)| (1..=count).map(move |n|format!("G-{family}-{n:03}"))).collect()
}
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 1 || std::env::var("G_LAB_TARGET_SHA").as_deref() != Ok(SOURCE) {
        eprintln!("one registered selector and matching immutable source context required");
        std::process::exit(2);
    }
    if args[0] == "--list" {
        println!("{}",json!({"schema_version":1,"source_sha":SOURCE,"cases":cases()}));
        return;
    }
    if !cases().contains(&args[0]) { std::process::exit(2); }
    match probe(&args[0]) {
        Ok(observed) => println!("{}",json!({"schema_version":1,"case_id":args[0],"source_sha":SOURCE,"observed":observed})),
        Err(error) => {
            eprintln!("independent probe setup/contract error: {error:?}");
            std::process::exit(1);
        }
    }
}
