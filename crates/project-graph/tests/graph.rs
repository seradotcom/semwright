mod common;
use common::*;
use composition::{EvidenceSource, PrincipalBinding, Verdict};
use semwright_project_graph::*;
use std::sync::atomic::AtomicBool;
fn chain() -> (ProjectGraph, ProjectAccess, Vec<RevisionRecord>) {
    let (mut g, a) = setup();
    let source = asset(&mut g, &a, "source");
    let middle = asset(&mut g, &a, "middle");
    let end = asset(&mut g, &a, "end");
    let x = observe(&mut g, &a, &source, "x", 1);
    let y = observe(&mut g, &a, &middle, "y", 2);
    let z = observe(&mut g, &a, &end, "z", 3);
    let first = receipt(g.project_id().clone(), std::slice::from_ref(&x), &y, 4);
    let second = receipt(g.project_id().clone(), std::slice::from_ref(&y), &z, 5);
    let determinants = first
        .required_determinants()
        .into_iter()
        .chain(second.required_determinants())
        .collect();
    g.accept_receipt(&a, admit(first)).unwrap();
    g.accept_receipt(&a, admit(second)).unwrap();
    g.observe_determinants(&a, determinants).unwrap();
    (g, a, vec![x, y, z])
}
#[test]
fn transitive_stale_preserves_unchanged_output_and_exact_revert() {
    let (mut g, a, records) = chain();
    let end = &records[2].pin.asset;
    assert!(g.inspect(&a, end).unwrap().knowledge.cache_safe());
    observe(&mut g, &a, &records[0].pin.asset, "changed", 6);
    let view = g.inspect(&a, end).unwrap();
    assert_eq!(view.knowledge.freshness, Freshness::Stale);
    assert_eq!(view.knowledge.divergence, Divergence::Clean);
    observe(&mut g, &a, &records[0].pin.asset, "x", 7);
    assert!(g.inspect(&a, end).unwrap().knowledge.cache_safe());
}
#[test]
fn external_output_change_is_diverged_and_downstream_stale() {
    let (mut g, a, records) = chain();
    observe(&mut g, &a, &records[1].pin.asset, "external edit", 6);
    assert_eq!(
        g.inspect(&a, &records[1].pin.asset)
            .unwrap()
            .knowledge
            .divergence,
        Divergence::Diverged
    );
    assert_eq!(
        g.inspect(&a, &records[2].pin.asset)
            .unwrap()
            .knowledge
            .freshness,
        Freshness::Stale
    );
}
#[test]
fn restart_and_watcher_gap_require_new_observation() {
    let (mut g, a, records) = chain();
    g.invalidate_scope(&a, vec![records[0].pin.asset.clone()])
        .unwrap();
    assert_eq!(
        g.inspect(&a, &records[2].pin.asset)
            .unwrap()
            .knowledge
            .freshness,
        Freshness::Unknown
    );
    g.restart_observation_epoch();
    assert_eq!(
        g.inspect(&a, &records[2].pin.asset)
            .unwrap()
            .knowledge
            .label(),
        "UNKNOWN"
    );
}
#[test]
fn denied_and_missing_remain_distinct_and_required_dependency_is_not_current() {
    let (mut g, a, records) = chain();
    g.record_probe(&a, &records[0].pin.asset, ProbeOutcome::Denied)
        .unwrap();
    assert_eq!(
        g.inspect(&a, &records[0].pin.asset)
            .unwrap()
            .knowledge
            .existence,
        Existence::Unknown
    );
    assert_eq!(
        g.inspect(&a, &records[2].pin.asset)
            .unwrap()
            .knowledge
            .freshness,
        Freshness::Unknown
    );
    g.record_probe(&a, &records[0].pin.asset, ProbeOutcome::ConclusiveNotFound)
        .unwrap();
    assert_eq!(
        g.inspect(&a, &records[0].pin.asset)
            .unwrap()
            .knowledge
            .existence,
        Existence::Missing
    );
    assert_eq!(
        g.inspect(&a, &records[2].pin.asset)
            .unwrap()
            .knowledge
            .freshness,
        Freshness::Stale
    );
}
#[test]
fn declared_verified_edges_are_rejected_and_reference_cycles_are_legal() {
    let (mut g, a) = setup();
    let x = asset(&mut g, &a, "x");
    let y = asset(&mut g, &a, "y");
    g.declare(&a, declared(&x, &y, Relation::References))
        .unwrap();
    g.declare(&a, declared(&y, &x, Relation::References))
        .unwrap();
    assert!(
        g.declare(&a, declared(&x, &y, Relation::VerifiedBy))
            .is_err()
    );
    let report = g.impact(&a, &x, budget(), &AtomicBool::new(false)).unwrap();
    assert!(report.known.is_empty());
    assert_eq!(report.possible.len(), 1);
    assert!(report.unknown_frontier);
    g.declare(&a, declared(&x, &y, Relation::Contains)).unwrap();
    assert!(g.declare(&a, declared(&y, &x, Relation::Contains)).is_err());
}
#[test]
fn snapshot_cursor_rejects_changed_graph_query_grants_and_owner() {
    let (mut g, a, _) = chain();
    let mut cursors = QueryCursors::default();
    let query = AssetQuery::default();
    let page = cursors.page(&g, &a, &query, None, 1).unwrap();
    let cursor = page.next_cursor.unwrap();
    let mut o = owner();
    o.session = "another-session".into();
    let other = ProjectAccess::authorized(o, g.project_id().clone(), None, false, digest("grants"))
        .unwrap();
    assert!(cursors.page(&g, &other, &query, Some(&cursor), 1).is_err());
    let changed = ProjectAccess::authorized(
        owner(),
        g.project_id().clone(),
        None,
        false,
        digest("new-grants"),
    )
    .unwrap();
    assert!(
        cursors
            .page(&g, &changed, &query, Some(&cursor), 1)
            .is_err()
    );
    asset(&mut g, &a, "new");
    assert!(matches!(
        cursors.page(&g, &a, &query, Some(&cursor), 1),
        Err(GraphError::Conflict)
    ));
}
#[test]
fn scope_filter_never_reveals_hidden_names_or_counts() {
    let (g, _, records) = chain();
    let only = records[0].pin.asset.clone();
    let a = ProjectAccess::authorized(
        owner(),
        g.project_id().clone(),
        Some([only.clone()].into()),
        false,
        digest("limited"),
    )
    .unwrap();
    assert!(g.inspect(&a, &records[1].pin.asset).is_err());
    let p = QueryCursors::default()
        .page(&g, &a, &AssetQuery::default(), None, 256)
        .unwrap();
    assert_eq!(p.items.len(), 1);
    assert!(p.scope_partial);
    assert!(p.next_cursor.is_none());
    let r = g
        .impact(&a, &only, budget(), &AtomicBool::new(false))
        .unwrap();
    assert!(r.known.is_empty());
    assert!(r.possible.is_empty());
    assert!(r.unknown_frontier);
}
#[test]
fn bounded_impact_reports_truncation_and_cancelled_scope() {
    let (g, a, records) = chain();
    let mut small = budget();
    small.nodes = 1;
    let r = g
        .impact(&a, &records[0].pin.asset, small, &AtomicBool::new(false))
        .unwrap();
    assert!(r.truncated);
    assert!(r.unknown_frontier);
    let r = g
        .impact(&a, &records[0].pin.asset, budget(), &AtomicBool::new(true))
        .unwrap();
    assert!(r.cancelled && r.truncated);
    assert_eq!(r.visited_nodes, 0);
}
#[test]
fn receipt_idempotency_rejects_content_and_owner_substitution() {
    let (mut g, a) = setup();
    let x = asset(&mut g, &a, "x");
    let y = asset(&mut g, &a, "y");
    let xr = observe(&mut g, &a, &x, "x", 1);
    let yr = observe(&mut g, &a, &y, "y", 2);
    let r = receipt(g.project_id().clone(), &[xr], &yr, 3);
    g.accept_receipt(&a, admit(r.clone())).unwrap();
    let seq = g.snapshot_revision();
    g.accept_receipt(&a, admit(r.clone())).unwrap();
    assert_eq!(seq, g.snapshot_revision());
    let mut changed = r.clone();
    changed.operation.parameters = digest("substitution");
    assert!(g.accept_receipt(&a, admit(changed)).is_err());
    let mut foreign = r.clone();
    foreign.owner.principal = PrincipalBinding::Named("foreign".into());
    assert!(g.accept_receipt(&a, admit(foreign)).is_err());
    let adapter = ReceiptAdapter::registered(
        r.operation.capability.clone(),
        r.operation.descriptor.clone(),
        digest("wrong runtime"),
    )
    .unwrap();
    assert!(adapter.admit(&owner(), &r.request_id, r.clone()).is_err());
}
#[test]
fn fixture_report_remains_unknown_and_runtime_drift_stales() {
    let (mut g, a) = setup();
    let x = asset(&mut g, &a, "x");
    let y = asset(&mut g, &a, "y");
    let xr = observe(&mut g, &a, &x, "x", 1);
    let yr = observe(&mut g, &a, &y, "y", 2);
    let mut r = receipt(g.project_id().clone(), &[xr], &yr, 3);
    r.verification.validation.checks[0].evidence[0].source = EvidenceSource::Fixture;
    let mut determinants = r.required_determinants();
    g.observe_determinants(&a, determinants.clone()).unwrap();
    g.accept_receipt(&a, admit(r)).unwrap();
    assert_eq!(
        g.inspect(&a, &y).unwrap().knowledge.verification,
        Verdict::Unknown
    );
    assert!(!g.inspect(&a, &y).unwrap().knowledge.cache_safe());
    determinants[0].digest = digest("runtime changed");
    g.observe_determinants(&a, determinants).unwrap();
    assert_eq!(
        g.inspect(&a, &y).unwrap().knowledge.freshness,
        Freshness::Stale
    );
}
#[test]
fn rebind_is_audited_generation_and_old_observation_cannot_resolve_it() {
    let (mut g, a, records) = chain();
    let id = &records[0].pin.asset;
    g.rebind(
        &a,
        id,
        1,
        DurableLocator::ScopedFile {
            root: "new-root".into(),
            relative_path: "source.bin".into(),
        },
        "explicit owner selection".into(),
    )
    .unwrap();
    assert_eq!(g.inspect(&a, id).unwrap().binding_generation, 2);
    assert!(g.observe(&a, observation(id, "x", 7)).is_err());
    assert_eq!(g.revisions(&a, id, None, 256).unwrap().len(), 1);
}
