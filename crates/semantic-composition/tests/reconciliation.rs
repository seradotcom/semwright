use semwright_semantic_composition::*;
use serde_json::{Value, json};

fn owner(name: &str) -> Owner {
    Owner {
        session: name.into(),
        principal: PrincipalBinding::HostSession,
    }
}
fn budget(operations: u32) -> ConvergenceBudget {
    ConvergenceBudget {
        max_iterations: 4,
        max_operations: operations,
        max_findings: 8,
        max_observations: 8,
        max_elapsed_ms: 60_000,
    }
}
fn observed() -> ObservationRef {
    let resource = ResourceKey {
        provider: "test".into(),
        resource: "managed:scene".into(),
    };
    ObservationRef {
        id: "fresh-source".into(),
        base: BaseStateSet(vec![BaseState {
            key: resource.clone(),
            document_id: "document".into(),
            provider_session: "alice".into(),
            generation: "native-v1".into(),
            revision: Revision::Fingerprint(Digest::of_bytes(b"current partial state")),
            concurrency: Concurrency::BestEffortRevalidate,
        }]),
        source: EvidenceSource::FileRead,
        method: "native_source_hash".into(),
        method_version: 1,
        scope: vec![Address {
            resource,
            logical_id: "scene".into(),
            property: "managed_files".into(),
        }],
        artifact: Some(Digest::of_bytes(b"current partial state")),
        exhaustive: true,
    }
}
fn partial(vault: &mut PlanVault, owner: &Owner, plan: &Value, budget: ConvergenceBudget) {
    vault
        .issue(owner, "root", plan, budget, 1, None, false)
        .unwrap();
    let permit = vault.begin(owner, "root", plan, "write-original").unwrap();
    vault
        .finish(
            permit,
            ExecutionStatus::Unknown,
            vec!["known-first-write".into()],
        )
        .unwrap();
}
fn ticket(
    vault: &mut PlanVault,
    owner: &Owner,
    plan: &Value,
    request: &str,
) -> ReconciliationPermit {
    let evidence = observed();
    vault
        .begin_reconciliation(owner, "root", plan, request, evidence.base, evidence.scope)
        .unwrap()
}

#[test]
fn reconciliation_preserves_unknown_ledger_and_authorizes_only_one_fresh_child() {
    let alice = owner("alice");
    let plan = json!({"kind":"parent"});
    let mut vault = PlanVault::bounded(8, 4, 8);
    let original_budget = budget(8);
    partial(&mut vault, &alice, &plan, original_budget.clone());
    let child = json!({"kind":"repair"});
    assert!(
        vault
            .issue(
                &alice,
                "child",
                &child,
                original_budget.clone(),
                1,
                Some("root"),
                true
            )
            .is_err()
    );
    let permit = ticket(&mut vault, &alice, &plan, "observe-uncertain");
    vault.finish_reconciliation(permit, observed()).unwrap();
    assert_eq!(
        vault.ledger(&alice, "root").unwrap()[0].status,
        ExecutionStatus::Unknown
    );
    assert_eq!(
        vault.ledger(&alice, "root").unwrap()[0].effects,
        vec!["known-first-write"]
    );
    assert!(vault.begin(&alice, "root", &plan, "parent-retry").is_err());
    vault
        .issue(
            &alice,
            "child",
            &child,
            original_budget.clone(),
            1,
            Some("root"),
            true,
        )
        .unwrap();
    assert!(
        vault
            .issue(
                &alice,
                "sibling",
                &child,
                original_budget,
                1,
                Some("root"),
                true
            )
            .is_err()
    );
    assert!(
        vault
            .begin(&alice, "child", &child, "write-original")
            .is_err()
    );
    assert!(
        vault
            .begin(&alice, "child", &child, "observe-uncertain")
            .is_err()
    );
    let permit = vault
        .begin(&alice, "child", &child, "write-repair")
        .unwrap();
    vault
        .finish(permit, ExecutionStatus::Completed, vec![])
        .unwrap();
    assert_eq!(vault.ledger(&alice, "child").unwrap().len(), 2);
    assert_eq!(
        vault.ledger(&alice, "child").unwrap()[0].status,
        ExecutionStatus::Unknown
    );
    assert_eq!(
        vault.reconciliations(&alice, "child").unwrap()[0]
            .child_plan_id
            .as_deref(),
        Some("child")
    );
}

#[test]
fn reconciliation_never_refunds_operation_budget_or_accepts_foreign_owner() {
    let alice = owner("alice");
    let plan = json!({"kind":"parent"});
    let mut vault = PlanVault::bounded(8, 4, 8);
    partial(&mut vault, &alice, &plan, budget(1));
    let evidence = observed();
    assert!(
        vault
            .begin_reconciliation(
                &owner("bob"),
                "root",
                &plan,
                "foreign",
                evidence.base.clone(),
                evidence.scope.clone()
            )
            .is_err()
    );
    let permit = ticket(&mut vault, &alice, &plan, "fresh");
    vault.finish_reconciliation(permit, evidence).unwrap();
    let child = json!({"kind":"repair"});
    vault
        .issue(&alice, "child", &child, budget(1), 1, Some("root"), true)
        .unwrap();
    assert!(matches!(
        vault.begin(&alice, "child", &child, "new-request"),
        Err(ContractError::Limit(_))
    ));
    assert_eq!(vault.root_budget(&alice, "child").unwrap(), budget(1));
}

#[test]
fn reconciliation_rejects_foreign_vault_and_revoked_root_incarnation() {
    let alice = owner("alice");
    let plan = json!({"kind":"parent"});
    let mut first = PlanVault::bounded(8, 4, 8);
    let mut second = PlanVault::bounded(8, 4, 8);
    partial(&mut first, &alice, &plan, budget(8));
    partial(&mut second, &alice, &plan, budget(8));
    let foreign = ticket(&mut first, &alice, &plan, "foreign-vault");
    assert!(second.finish_reconciliation(foreign, observed()).is_err());
    let stale = ticket(&mut first, &alice, &plan, "old-root");
    first.revoke(&alice);
    partial(&mut first, &alice, &plan, budget(8));
    assert!(first.finish_reconciliation(stale, observed()).is_err());
    let fresh = ticket(&mut first, &alice, &plan, "new-root");
    first.finish_reconciliation(fresh, observed()).unwrap();
}

#[test]
fn reconciliation_rejects_changed_binding_fixture_and_nonexhaustive_evidence() {
    let alice = owner("alice");
    let plan = json!({"kind":"parent"});
    let mut vault = PlanVault::bounded(8, 4, 8);
    partial(&mut vault, &alice, &plan, budget(8));
    let permit = ticket(&mut vault, &alice, &plan, "stale-read");
    let mut changed = observed();
    changed.base.0[0].generation = "changed-generation".into();
    assert!(vault.finish_reconciliation(permit, changed).is_err());
    let permit = ticket(&mut vault, &alice, &plan, "fixture-read");
    let mut fake = observed();
    fake.source = EvidenceSource::Fixture;
    assert!(vault.finish_reconciliation(permit, fake).is_err());
    let permit = ticket(&mut vault, &alice, &plan, "partial-read");
    let mut incomplete = observed();
    incomplete.exhaustive = false;
    assert!(vault.finish_reconciliation(permit, incomplete).is_err());
    let permit = ticket(&mut vault, &alice, &plan, "valid-read");
    vault.finish_reconciliation(permit, observed()).unwrap();
    let evidence = observed();
    assert!(
        vault
            .begin_reconciliation(
                &alice,
                "root",
                &plan,
                "valid-read",
                evidence.base,
                evidence.scope
            )
            .is_err()
    );
}

#[test]
fn observation_failure_reserves_budget_and_old_ticket_cannot_cross_new_read() {
    let alice = owner("alice");
    let plan = json!({"kind":"parent"});
    let mut vault = PlanVault::bounded(8, 4, 8);
    let mut limits = budget(8);
    limits.max_observations = 2;
    partial(&mut vault, &alice, &plan, limits);
    let old = ticket(&mut vault, &alice, &plan, "first-read");
    let latest = ticket(&mut vault, &alice, &plan, "second-read");
    assert!(vault.finish_reconciliation(old, observed()).is_err());
    let evidence = observed();
    assert!(matches!(
        vault.begin_reconciliation(
            &alice,
            "root",
            &plan,
            "third-read",
            evidence.base,
            evidence.scope
        ),
        Err(ContractError::Limit(_))
    ));
    vault.finish_reconciliation(latest, observed()).unwrap();
}
