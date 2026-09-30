use schemars::JsonSchema;
use semwright_semantic_composition::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;
fn owner() -> Owner {
    Owner {
        session: "session-a".into(),
        principal: PrincipalBinding::HostSession,
    }
}
fn budget() -> ConvergenceBudget {
    ConvergenceBudget {
        max_iterations: 3,
        max_operations: 5,
        max_findings: 10,
        max_observations: 3,
        max_elapsed_ms: 30_000,
    }
}
fn base() -> BaseStateSet {
    BaseStateSet(vec![BaseState {
        key: ResourceKey {
            provider: "driver:fixture".into(),
            resource: "document".into(),
        },
        document_id: "document".into(),
        provider_session: "native-session".into(),
        generation: "1".into(),
        revision: Revision::Counter(1),
        concurrency: Concurrency::BestEffortRevalidate,
    }])
}
fn report() -> ValidationReport {
    let b = base();
    ValidationReport {
        plan_digest: Digest::of_bytes(b"plan"),
        base: b.clone(),
        required_rules: BTreeSet::from(["rule".into()]),
        checks: vec![RuleResult {
            rule: "rule".into(),
            version: 1,
            verdict: Verdict::Pass,
            evidence_class: EvidenceClass::Deterministic,
            evidence: vec![ObservationRef {
                id: "observation".into(),
                base: b,
                source: EvidenceSource::NativeApi,
                method: "native-read".into(),
                method_version: 1,
                scope: vec![],
                artifact: None,
                exhaustive: true,
            }],
            reason: None,
        }],
    }
}
#[test]
fn key_order_is_canonical() {
    assert_eq!(
        canonical_digest(&json!({"b":2,"a":1})).unwrap(),
        canonical_digest(&json!({"a":1,"b":2})).unwrap()
    );
}
#[test]
fn duplicate_json_keys_are_rejected() {
    assert!(strict_decode::<Value>(br#"{"a":1,"a":2}"#).is_err());
}
#[test]
fn nested_duplicate_keys_are_rejected() {
    assert!(strict_decode::<Value>(br#"{"b":[{"a":1,"a":2}]}"#).is_err());
}
#[test]
fn unicode_null_and_arrays_are_preserved() {
    let v = json!({"text":"á𝄞שלום","mixed":null,"items":[1,2]});
    assert_eq!(
        strict_decode::<Value>(&canonical_bytes(&v).unwrap()).unwrap(),
        v
    );
    assert_ne!(
        canonical_digest(&json!([1, 2])).unwrap(),
        canonical_digest(&json!([2, 1])).unwrap()
    );
}
#[test]
fn malformed_digest_rejected() {
    assert!(strict_decode::<Digest>(br#""abc""#).is_err());
}
#[test]
fn oversize_rejected() {
    assert!(strict_decode::<Value>(&vec![b' '; MAX_PAYLOAD_BYTES + 1]).is_err());
}
#[test]
fn depth_limit_rejected() {
    let x = format!("{}0{}", "[".repeat(65), "]".repeat(65));
    assert!(strict_decode::<Value>(x.as_bytes()).is_err());
}
#[test]
fn unknown_fields_rejected() {
    assert!(
        strict_decode::<Owner>(br#"{"session":"a","principal":"host_session","execute":"shell"}"#)
            .is_err()
    );
}
#[test]
fn duplicate_resources_rejected() {
    let mut b = base();
    b.0.push(b.0[0].clone());
    assert!(b.validate().is_err());
}
#[test]
fn stale_generation_is_not_global_revision() {
    let a = base();
    let mut b = a.clone();
    b.0[0].generation = "2".into();
    assert!(a.check_fresh(&b, false).is_err());
    assert!(a.check_fresh(&a, true).is_err());
    assert!(a.check_fresh(&a, false).is_ok());
}
#[test]
fn complete_native_report_passes() {
    assert_eq!(report().verdict().unwrap(), Verdict::Pass);
}
#[test]
fn missing_rule_is_unknown() {
    let mut r = report();
    r.required_rules.insert("unperformed".into());
    assert_eq!(r.verdict().unwrap(), Verdict::Unknown);
}
#[test]
fn missing_evidence_is_unknown() {
    let mut r = report();
    r.checks[0].evidence.clear();
    assert_eq!(r.verdict().unwrap(), Verdict::Unknown);
}
#[test]
fn heuristic_cannot_certify_required_check() {
    let mut r = report();
    r.checks[0].evidence_class = EvidenceClass::Heuristic;
    assert_eq!(r.verdict().unwrap(), Verdict::Unknown);
}
#[test]
fn samples_and_fixtures_do_not_certify_native_range() {
    for mode in 0..2 {
        let mut r = report();
        if mode == 0 {
            r.checks[0].evidence[0].exhaustive = false;
        } else {
            r.checks[0].evidence[0].source = EvidenceSource::Fixture;
        }
        assert_eq!(r.verdict().unwrap(), Verdict::Unknown);
    }
}
#[test]
fn fail_cannot_hide_behind_missing_check() {
    let mut r = report();
    r.required_rules.insert("unperformed".into());
    r.checks[0].verdict = Verdict::Fail;
    assert_eq!(r.verdict().unwrap(), Verdict::Fail);
}
#[test]
fn duplicate_rules_rejected() {
    let mut r = report();
    r.checks.push(r.checks[0].clone());
    assert!(r.verdict().is_err());
}
#[test]
fn plan_rehash_is_not_authority() {
    let mut v = PlanVault::bounded(10, 5, 10);
    let p = json!({"target":"owned"});
    v.issue(&owner(), "p", &p, budget(), 1, None, false)
        .unwrap();
    assert!(
        v.begin(&owner(), "p", &json!({"target":"foreign"}), "r")
            .is_err()
    );
}
#[test]
fn plan_session_substitution_denied() {
    let mut v = PlanVault::bounded(10, 5, 10);
    let p = json!({});
    v.issue(&owner(), "p", &p, budget(), 1, None, false)
        .unwrap();
    let mut other = owner();
    other.session = "other".into();
    assert!(v.begin(&other, "p", &p, "r").is_err());
}
#[test]
fn unknown_mutation_cannot_retry() {
    let mut v = PlanVault::bounded(10, 5, 10);
    let p = json!({});
    v.issue(&owner(), "p", &p, budget(), 1, None, false)
        .unwrap();
    let permit = v.begin(&owner(), "p", &p, "r").unwrap();
    v.finish(permit, ExecutionStatus::Unknown, vec!["created:a".into()])
        .unwrap();
    assert!(v.begin(&owner(), "p", &p, "r2").is_err());
    assert_eq!(v.ledger(&owner(), "p").unwrap()[0].effects, ["created:a"]);
}
#[test]
fn repair_budget_is_cumulative() {
    let mut v = PlanVault::bounded(10, 5, 10);
    let p = json!({});
    v.issue(&owner(), "p", &p, budget(), 4, None, false)
        .unwrap();
    let permit = v.begin(&owner(), "p", &p, "r").unwrap();
    v.finish(permit, ExecutionStatus::Completed, vec![])
        .unwrap();
    v.issue(&owner(), "repair", &p, budget(), 2, Some("p"), true)
        .unwrap();
    assert!(v.begin(&owner(), "repair", &p, "r2").is_err());
}
#[test]
fn repair_cannot_enlarge_budget() {
    let mut v = PlanVault::bounded(10, 5, 10);
    let p = json!({});
    v.issue(&owner(), "p", &p, budget(), 1, None, false)
        .unwrap();
    let mut b = budget();
    b.max_operations += 1;
    assert!(
        v.issue(&owner(), "repair", &p, b, 1, Some("p"), true)
            .is_err()
    );
}
#[test]
fn repeat_issue_does_not_reset_attempt() {
    let mut v = PlanVault::bounded(10, 5, 10);
    let p = json!({});
    v.issue(&owner(), "p", &p, budget(), 1, None, false)
        .unwrap();
    let permit = v.begin(&owner(), "p", &p, "r").unwrap();
    v.finish(permit, ExecutionStatus::Completed, vec![])
        .unwrap();
    v.issue(&owner(), "p", &p, budget(), 1, None, false)
        .unwrap();
    assert!(v.begin(&owner(), "p", &p, "r2").is_err());
}
#[test]
fn revoke_invalidates_server_plan() {
    let mut v = PlanVault::bounded(10, 5, 10);
    let p = json!({});
    v.issue(&owner(), "p", &p, budget(), 1, None, false)
        .unwrap();
    v.revoke(&owner());
    assert!(v.begin(&owner(), "p", &p, "r").is_err());
}
#[test]
fn controller_needs_complete_scoped_evidence() {
    let r = report();
    let mut c = Controller::new(r.plan_digest.clone(), budget(), r.required_rules.clone()).unwrap();
    assert!(c.executed(ExecutionStatus::Completed).is_err());
    c.applying(false).unwrap();
    assert_eq!(
        c.executed(ExecutionStatus::Completed).unwrap(),
        Decision::Observe
    );
    assert_eq!(
        c.observed(&r, vec![0], 0, 10).unwrap(),
        Decision::Stop(StopReason::Complete)
    );
    assert_eq!(c.state, State::Verified);
}
#[test]
fn controller_preserves_partial_outcome() {
    let r = report();
    let mut c = Controller::new(r.plan_digest, budget(), r.required_rules).unwrap();
    c.applying(false).unwrap();
    c.executed(ExecutionStatus::Partial).unwrap();
    assert_eq!(c.state, State::PartiallyApplied);
    assert!(c.applying(false).is_err());
}
#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct FigmaIntent {
    nodes: u32,
}
#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct MotionIntent {
    frames: u32,
}
#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AudioIntent {
    samples: u64,
}
#[test]
fn domains_remain_distinct_payloads() {
    let f = schema_digest::<FigmaIntent>().unwrap();
    let m = schema_digest::<MotionIntent>().unwrap();
    let a = schema_digest::<AudioIntent>().unwrap();
    assert_ne!(f, m);
    assert_ne!(m, a);
    assert!(strict_decode::<FigmaIntent>(br#"{"samples":48000}"#).is_err());
}

#[test]
fn controller_terminal_receipts_map_to_distinct_fail_closed_states() {
    let cases = [
        (
            ExecutionStatus::Partial,
            State::PartiallyApplied,
            StopReason::UnknownOutcome,
        ),
        (
            ExecutionStatus::Unknown,
            State::Unknown,
            StopReason::UnknownOutcome,
        ),
        (ExecutionStatus::Denied, State::Denied, StopReason::Denied),
        (
            ExecutionStatus::Cancelled,
            State::Cancelled,
            StopReason::Cancelled,
        ),
        (
            ExecutionStatus::Failed,
            State::Failed,
            StopReason::UnknownOutcome,
        ),
    ];
    for (status, state, reason) in cases {
        let r = report();
        let mut controller = Controller::new(r.plan_digest, budget(), r.required_rules).unwrap();
        controller.applying(false).unwrap();
        assert_eq!(
            controller.executed(status).unwrap(),
            Decision::Stop(reason),
            "{status:?}"
        );
        assert_eq!(controller.state, state, "{status:?}");
        assert_eq!(controller.stop, Some(reason), "{status:?}");
        assert!(controller.applying(false).is_err(), "{status:?}");
    }
}

fn failed_report() -> ValidationReport {
    let mut value = report();
    value.checks[0].verdict = Verdict::Fail;
    value
}

#[test]
fn controller_observation_budget_boundary_is_inclusive_then_exhausts() {
    let report = failed_report();
    let mut b = budget();
    b.max_iterations = 1;
    b.max_elapsed_ms = 10;
    let mut controller =
        Controller::new(report.plan_digest.clone(), b, report.required_rules.clone()).unwrap();
    controller.applying(false).unwrap();
    assert_eq!(
        controller.executed(ExecutionStatus::Completed).unwrap(),
        Decision::Observe
    );
    assert_eq!(
        controller.observed(&report, vec![10], 1, 10).unwrap(),
        Decision::PlanRepair
    );
    assert_eq!(controller.state, State::RepairPlanned);

    controller.bind_repair(report.plan_digest.clone()).unwrap();
    controller.applying(true).unwrap();
    controller.executed(ExecutionStatus::Completed).unwrap();
    assert_eq!(
        controller.observed(&report, vec![9], 1, 10).unwrap(),
        Decision::Stop(StopReason::BudgetExhausted)
    );
    assert_eq!(controller.state, State::Exhausted);
}

#[test]
fn controller_elapsed_budget_exhausts_only_after_the_boundary() {
    let report = failed_report();
    let mut b = budget();
    b.max_elapsed_ms = 10;
    let mut controller =
        Controller::new(report.plan_digest.clone(), b, report.required_rules.clone()).unwrap();
    controller.applying(false).unwrap();
    controller.executed(ExecutionStatus::Completed).unwrap();
    assert_eq!(
        controller.observed(&report, vec![10], 1, 11).unwrap(),
        Decision::Stop(StopReason::BudgetExhausted)
    );
    assert_eq!(controller.state, State::Exhausted);
}

#[test]
fn controller_rejects_scope_progress_and_candidate_substitution() {
    let report = failed_report();

    let mut wrong_plan = report.clone();
    wrong_plan.plan_digest = Digest::of_bytes(b"other-plan");
    let mut controller = Controller::new(
        report.plan_digest.clone(),
        budget(),
        report.required_rules.clone(),
    )
    .unwrap();
    controller.applying(false).unwrap();
    controller.executed(ExecutionStatus::Completed).unwrap();
    assert!(controller.observed(&wrong_plan, vec![1], 1, 1).is_err());

    let mut wrong_rules = report.clone();
    wrong_rules.required_rules.insert("extra".into());
    let mut controller = Controller::new(
        report.plan_digest.clone(),
        budget(),
        report.required_rules.clone(),
    )
    .unwrap();
    controller.applying(false).unwrap();
    controller.executed(ExecutionStatus::Completed).unwrap();
    assert!(controller.observed(&wrong_rules, vec![1], 1, 1).is_err());

    for progress in [vec![], vec![1; 33]] {
        let mut controller = Controller::new(
            report.plan_digest.clone(),
            budget(),
            report.required_rules.clone(),
        )
        .unwrap();
        controller.applying(false).unwrap();
        controller.executed(ExecutionStatus::Completed).unwrap();
        assert!(controller.observed(&report, progress, 1, 1).is_err());
    }

    for candidates in [0, 2] {
        let mut controller = Controller::new(
            report.plan_digest.clone(),
            budget(),
            report.required_rules.clone(),
        )
        .unwrap();
        controller.applying(false).unwrap();
        controller.executed(ExecutionStatus::Completed).unwrap();
        assert_eq!(
            controller
                .observed(&report, vec![10], candidates, 1)
                .unwrap(),
            Decision::Stop(StopReason::Ambiguous)
        );
        assert_eq!(controller.state, State::Conflicted);
    }
}

#[test]
fn controller_detects_cycle_and_non_improving_progress() {
    let report = failed_report();

    let mut cycle = Controller::new(
        report.plan_digest.clone(),
        budget(),
        report.required_rules.clone(),
    )
    .unwrap();
    cycle.applying(false).unwrap();
    cycle.executed(ExecutionStatus::Completed).unwrap();
    assert_eq!(
        cycle.observed(&report, vec![10], 1, 1).unwrap(),
        Decision::PlanRepair
    );
    cycle.bind_repair(report.plan_digest.clone()).unwrap();
    cycle.applying(true).unwrap();
    cycle.executed(ExecutionStatus::Completed).unwrap();
    assert_eq!(
        cycle.observed(&report, vec![10], 1, 2).unwrap(),
        Decision::Stop(StopReason::Cycle)
    );

    let mut no_progress = Controller::new(
        report.plan_digest.clone(),
        budget(),
        report.required_rules.clone(),
    )
    .unwrap();
    no_progress.applying(false).unwrap();
    no_progress.executed(ExecutionStatus::Completed).unwrap();
    no_progress.observed(&report, vec![10], 1, 1).unwrap();
    no_progress.bind_repair(report.plan_digest.clone()).unwrap();
    no_progress.applying(true).unwrap();
    no_progress.executed(ExecutionStatus::Completed).unwrap();
    assert_eq!(
        no_progress.observed(&report, vec![11], 1, 2).unwrap(),
        Decision::Stop(StopReason::NoProgress)
    );
}

#[test]
fn vault_exact_operation_and_iteration_boundaries_are_enforced() {
    let owner = owner();
    let p = json!({"intent":"root"});
    let mut b = budget();
    b.max_operations = 2;
    b.max_iterations = 1;
    let mut vault = PlanVault::bounded(10, 5, 10);
    vault
        .issue(&owner, "root", &p, b.clone(), 2, None, false)
        .unwrap();
    let permit = vault.begin(&owner, "root", &p, "request-one").unwrap();
    vault
        .finish(permit, ExecutionStatus::Completed, vec![])
        .unwrap();

    vault
        .issue(
            &owner,
            "repair",
            &json!({"intent":"repair"}),
            b,
            0,
            Some("root"),
            true,
        )
        .unwrap();
    assert!(
        vault
            .begin(&owner, "repair", &json!({"intent":"repair"}), "request-two")
            .is_err(),
        "second iteration must not cross max_iterations"
    );
}

#[test]
fn vault_observation_budget_counts_exactly_and_never_refunds() {
    let owner = owner();
    let p = json!({});
    let mut b = budget();
    b.max_findings = 1;
    b.max_observations = 2;
    let mut vault = PlanVault::bounded(10, 5, 10);
    vault.issue(&owner, "root", &p, b, 1, None, false).unwrap();

    vault.record_observation(&owner, "root", 1).unwrap();
    vault.record_observation(&owner, "root", 0).unwrap();
    assert!(vault.record_observation(&owner, "root", 0).is_err());
    assert!(vault.record_observation(&owner, "root", 2).is_err());
}

#[test]
fn vault_finish_rejects_nonterminal_status_and_invalid_effect_receipts() {
    for status in [ExecutionStatus::Prepared, ExecutionStatus::Applying] {
        let owner = owner();
        let p = json!({});
        let mut vault = PlanVault::bounded(10, 5, 10);
        vault
            .issue(&owner, "root", &p, budget(), 1, None, false)
            .unwrap();
        let permit = vault.begin(&owner, "root", &p, "request").unwrap();
        assert!(vault.finish(permit, status, vec![]).is_err());
    }

    let owner = owner();
    let p = json!({});
    let mut vault = PlanVault::bounded(10, 5, 10);
    vault
        .issue(&owner, "root", &p, budget(), 1, None, false)
        .unwrap();
    let permit = vault.begin(&owner, "root", &p, "request").unwrap();
    assert!(
        vault
            .finish(
                permit,
                ExecutionStatus::Completed,
                vec![
                    "bad
receipt"
                        .into()
                ]
            )
            .is_err()
    );
}

#[test]
fn vault_revoke_is_owner_scoped() {
    let first = owner();
    let mut second = owner();
    second.session = "session-b".into();
    let p = json!({});
    let mut vault = PlanVault::bounded(10, 5, 10);
    vault
        .issue(&first, "first", &p, budget(), 1, None, false)
        .unwrap();
    vault
        .issue(&second, "second", &p, budget(), 1, None, false)
        .unwrap();

    vault.revoke(&first);
    assert!(vault.begin(&first, "first", &p, "request-a").is_err());
    let permit = vault
        .begin(&second, "second", &p, "request-b")
        .expect("revoking first owner must not remove second");
    vault
        .finish(permit, ExecutionStatus::Completed, vec![])
        .unwrap();
}

#[test]
fn controller_progress_dimension_change_is_not_treated_as_improvement() {
    let report = failed_report();
    let mut controller = Controller::new(
        report.plan_digest.clone(),
        budget(),
        report.required_rules.clone(),
    )
    .unwrap();
    controller.applying(false).unwrap();
    controller.executed(ExecutionStatus::Completed).unwrap();
    assert_eq!(
        controller.observed(&report, vec![10, 10], 1, 1).unwrap(),
        Decision::PlanRepair
    );
    controller.bind_repair(report.plan_digest.clone()).unwrap();
    controller.applying(true).unwrap();
    controller.executed(ExecutionStatus::Completed).unwrap();
    assert_eq!(
        controller.observed(&report, vec![9], 1, 2).unwrap(),
        Decision::Stop(StopReason::NoProgress)
    );
    assert_eq!(controller.state, State::Conflicted);
}

#[test]
fn controller_bind_repair_requires_an_actual_pending_repair() {
    let report = failed_report();
    let mut controller = Controller::new(
        report.plan_digest.clone(),
        budget(),
        report.required_rules.clone(),
    )
    .unwrap();
    assert!(
        controller
            .bind_repair(Digest::of_bytes(b"foreign"))
            .is_err()
    );
    assert_eq!(controller.state, State::Prepared);
}

#[test]
fn vault_request_ids_cannot_replay_across_repair_entries_of_one_root() {
    let owner = owner();
    let root = json!({"intent":"root"});
    let repair = json!({"intent":"repair"});
    let b = budget();
    let mut vault = PlanVault::bounded(10, 5, 10);
    vault
        .issue(&owner, "root", &root, b.clone(), 1, None, false)
        .unwrap();
    let permit = vault
        .begin(&owner, "root", &root, "request-shared")
        .unwrap();
    vault
        .finish(permit, ExecutionStatus::Completed, vec![])
        .unwrap();
    vault
        .issue(&owner, "repair", &repair, b, 1, Some("root"), true)
        .unwrap();

    let error = vault
        .begin(&owner, "repair", &repair, "request-shared")
        .unwrap_err();
    assert!(matches!(error, ContractError::Denied(_)));
}

#[test]
fn vault_unknown_attempt_blocks_a_preissued_sibling_until_reconciled() {
    let owner = owner();
    let root = json!({"intent":"root"});
    let first = json!({"intent":"repair-a"});
    let second = json!({"intent":"repair-b"});
    let b = budget();
    let mut vault = PlanVault::bounded(10, 5, 10);
    vault
        .issue(&owner, "root", &root, b.clone(), 1, None, false)
        .unwrap();
    let permit = vault.begin(&owner, "root", &root, "root-request").unwrap();
    vault
        .finish(permit, ExecutionStatus::Completed, vec![])
        .unwrap();

    vault
        .issue(&owner, "repair-a", &first, b.clone(), 1, Some("root"), true)
        .unwrap();
    vault
        .issue(&owner, "repair-b", &second, b, 1, Some("root"), true)
        .unwrap();

    let permit = vault
        .begin(&owner, "repair-a", &first, "repair-a-request")
        .unwrap();
    vault
        .finish(
            permit,
            ExecutionStatus::Unknown,
            vec!["maybe-created".into()],
        )
        .unwrap();

    let error = vault
        .begin(&owner, "repair-b", &second, "repair-b-request")
        .unwrap_err();
    assert!(matches!(error, ContractError::Unknown(_)));
}
