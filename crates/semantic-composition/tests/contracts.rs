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
