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
    assert!(accept_observation(&mut g, &a, observation(id, "x", 7)).is_err());
    assert_eq!(g.revisions(&a, id, None, 256).unwrap().len(), 1);
}

#[test]
fn cursor_binds_actual_visibility_even_when_grant_digest_is_reused() {
    let (g, a, records) = chain();
    let mut cursors = QueryCursors::default();
    let page = cursors
        .page(&g, &a, &AssetQuery::default(), None, 1)
        .unwrap();
    let subset = ProjectAccess::authorized(
        owner(),
        g.project_id().clone(),
        Some(records.iter().map(|r| r.pin.asset.clone()).collect()),
        false,
        digest("grants"),
    )
    .unwrap();
    assert!(matches!(
        cursors.page(
            &g,
            &subset,
            &AssetQuery::default(),
            page.next_cursor.as_deref(),
            1
        ),
        Err(GraphError::Denied)
    ));
    assert!(
        g.revisions(&subset, &records[0].pin.asset, None, 1)
            .is_err()
    );
}
#[test]
fn synthetic_changed_observation_cannot_prove_native_staleness() {
    let (mut g, a, records) = chain();
    let mut fake = observation(&records[0].pin.asset, "synthetic change", 9);
    fake.observation.source = EvidenceSource::Fixture;
    accept_observation(&mut g, &a, fake).unwrap();
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
}

#[test]
fn receipt_cannot_substitute_source_base_while_retaining_valid_revision_pins() {
    let (mut g, a) = setup();
    let x = asset(&mut g, &a, "source");
    let y = asset(&mut g, &a, "output");
    let input = observe(&mut g, &a, &x, "x", 1);
    let output = observe(&mut g, &a, &y, "y", 2);
    let mut receipt = receipt(g.project_id().clone(), &[input], &output, 3);
    receipt.source_base.0[0].generation = "foreign-generation".into();
    assert!(g.accept_receipt(&a, admit(receipt)).is_err());
}
#[test]
fn report_pass_for_another_output_does_not_certify_this_output() {
    let (mut g, a) = setup();
    let x = asset(&mut g, &a, "source");
    let y = asset(&mut g, &a, "output");
    let input = observe(&mut g, &a, &x, "x", 1);
    let output = observe(&mut g, &a, &y, "y", 2);
    let mut r = receipt(g.project_id().clone(), &[input], &output, 3);
    r.verification.validation.checks[0].evidence[0].scope[0].logical_id = x.as_str().into();
    assert_eq!(r.verification.verdict().unwrap(), Verdict::Pass);
    g.observe_determinants(&a, r.required_determinants())
        .unwrap();
    g.accept_receipt(&a, admit(r)).unwrap();
    let state = g.inspect(&a, &y).unwrap().knowledge;
    assert_eq!(state.verification, Verdict::Unknown);
    assert!(!state.cache_safe());
}

#[test]
fn same_bytes_after_explicit_rebind_do_not_validate_old_derivation() {
    let (mut graph, access, records) = chain();
    let id = &records[0].pin.asset;
    graph
        .rebind(
            &access,
            id,
            1,
            DurableLocator::ScopedFile {
                root: "workspace".into(),
                relative_path: "replacement.bin".into(),
            },
            "explicit replacement selection".into(),
        )
        .unwrap();
    let mut new = observation(id, "x", 10);
    new.binding_generation = 2;
    accept_observation(&mut graph, &access, new).unwrap();
    assert_eq!(
        graph
            .inspect(&access, &records[2].pin.asset)
            .unwrap()
            .knowledge
            .freshness,
        Freshness::Unknown
    );
    assert!(
        !graph
            .inspect(&access, &records[2].pin.asset)
            .unwrap()
            .knowledge
            .cache_safe()
    );
}

#[test]
fn external_intent_requires_dispatch_and_matching_persisted_receipt() {
    let (mut g, a) = setup();
    let source = asset(&mut g, &a, "source");
    let output = asset(&mut g, &a, "output");
    let sr = observe(&mut g, &a, &source, "source-v1", 1);
    let or = observe(&mut g, &a, &output, "output-v1", 2);
    let intent = external_intent(&g, vec![output.clone()], 3);
    let intent_id = intent.id.clone();

    g.prepare_external_intent(&a, intent).unwrap();
    assert_eq!(
        g.external_intent(&a, &intent_id).unwrap().status,
        composition::ExecutionStatus::Prepared
    );
    assert!(
        g.resolve_external_intent(
            &a,
            &intent_id,
            composition::ExecutionStatus::Completed,
            None,
        )
        .is_err()
    );

    g.mark_external_intent_applying(&a, &intent_id).unwrap();
    assert!(g.mark_external_intent_applying(&a, &intent_id).is_err());

    let r = receipt(g.project_id().clone(), &[sr], &or, 3);
    let receipt_id = r.id.clone();
    g.accept_receipt(&a, admit(r)).unwrap();
    g.resolve_external_intent(
        &a,
        &intent_id,
        composition::ExecutionStatus::Completed,
        Some(receipt_id.clone()),
    )
    .unwrap();
    let resolved = g.external_intent(&a, &intent_id).unwrap();
    assert_eq!(resolved.status, composition::ExecutionStatus::Completed);
    assert_eq!(resolved.receipt, Some(receipt_id));
}

#[test]
fn restarted_applying_external_intent_is_unknown_and_never_redispatched() {
    let (mut g, a) = setup();
    let output = asset(&mut g, &a, "output");
    let intent = external_intent(&g, vec![output], 1);
    let intent_id = intent.id.clone();
    g.prepare_external_intent(&a, intent).unwrap();
    g.mark_external_intent_applying(&a, &intent_id).unwrap();

    g.restart_observation_epoch();
    assert_eq!(
        g.external_intent(&a, &intent_id).unwrap().status,
        composition::ExecutionStatus::Unknown
    );
    assert!(g.mark_external_intent_applying(&a, &intent_id).is_err());
    g.resolve_external_intent(&a, &intent_id, composition::ExecutionStatus::Unknown, None)
        .unwrap();
    assert_eq!(
        g.external_intent(&a, &intent_id).unwrap().status,
        composition::ExecutionStatus::Unknown
    );
}

#[test]
fn external_intent_visibility_does_not_leak_hidden_affected_resources() {
    let (mut g, full) = setup();
    let visible = asset(&mut g, &full, "visible");
    let hidden = asset(&mut g, &full, "hidden");
    let intent = external_intent(&g, vec![hidden.clone()], 1);
    let intent_id = intent.id.clone();
    g.prepare_external_intent(&full, intent).unwrap();

    let subset = ProjectAccess::authorized(
        owner(),
        g.project_id().clone(),
        Some([visible].into()),
        false,
        digest("grants"),
    )
    .unwrap();
    assert!(matches!(
        g.external_intent(&subset, &intent_id),
        Err(GraphError::Denied)
    ));
}

fn native_revision_candidate(
    graph: &ProjectGraph,
    asset: &LogicalAssetId,
    source: EvidenceSource,
    method: &str,
    coverage: Coverage,
) -> (composition::ResourceKey, RevisionCandidate) {
    let resource = composition::ResourceKey {
        provider: "godot".into(),
        resource: "managed:fixture-project".into(),
    };
    let fingerprint = digest("native-readback");
    (
        resource.clone(),
        RevisionCandidate {
            version: SCHEMA_VERSION,
            asset: asset.clone(),
            fingerprint: Fingerprint {
                bytes: Some(fingerprint.clone()),
                projection: None,
            },
            equivalence: Equivalence::ExactBytes,
            observed_unix_ms: 100,
            binding_generation: 1,
            observation: composition::ObservationRef {
                id: "godot-native-observation".into(),
                base: composition::BaseStateSet(vec![composition::BaseState {
                    key: resource.clone(),
                    document_id: graph.project_id().as_str().into(),
                    provider_session: "driver-session".into(),
                    generation: "godot-native-v1".into(),
                    revision: composition::Revision::Fingerprint(fingerprint.clone()),
                    concurrency: composition::Concurrency::BestEffortRevalidate,
                }]),
                source,
                method: method.into(),
                method_version: 1,
                scope: vec![composition::Address {
                    resource,
                    logical_id: "scene/main".into(),
                    property: "native_readback".into(),
                }],
                artifact: Some(fingerprint),
                exhaustive: true,
            },
            coverage,
        },
    )
}

#[test]
fn registered_revision_adapter_assigns_revision_without_creating_activity() {
    let (mut graph, access) = setup();
    let asset = asset(&mut graph, &access, "native-scene");
    let (resource, candidate) = native_revision_candidate(
        &graph,
        &asset,
        EvidenceSource::NativeApi,
        "godot_native_project_observation",
        Coverage::unknown(),
    );
    let wire = serde_json::to_value(&candidate).unwrap();
    assert!(wire.get("revision").is_none());

    let adapter = RevisionAdapter::registered(
        resource,
        EvidenceSource::NativeApi,
        "godot_native_project_observation".into(),
        1,
    )
    .unwrap();
    let admitted = adapter
        .admit(&owner(), graph.project_id(), &asset, 1, candidate)
        .unwrap();
    let revision = admitted.record().pin.revision.clone();
    graph.accept_revision(&access, admitted).unwrap();

    assert_eq!(
        graph.inspect(&access, &asset).unwrap().latest_revision,
        Some(revision)
    );
    let impact = graph
        .impact(&access, &asset, budget(), &AtomicBool::new(false))
        .unwrap();
    assert!(impact.known.is_empty());
    assert!(
        !graph
            .inspect(&access, &asset)
            .unwrap()
            .knowledge
            .cache_safe()
    );
}

#[test]
fn revision_adapter_rejects_method_project_generation_and_source_substitution() {
    let (mut graph, access) = setup();
    let asset = asset(&mut graph, &access, "native-scene");
    let (resource, candidate) = native_revision_candidate(
        &graph,
        &asset,
        EvidenceSource::NativeApi,
        "godot_native_project_observation",
        Coverage::unknown(),
    );
    let adapter = RevisionAdapter::registered(
        resource.clone(),
        EvidenceSource::NativeApi,
        "godot_native_project_observation".into(),
        1,
    )
    .unwrap();

    let mut wrong_version = candidate.clone();
    wrong_version.version = SCHEMA_VERSION + 1;
    assert!(
        adapter
            .admit(&owner(), graph.project_id(), &asset, 1, wrong_version)
            .is_err()
    );

    let mut wrong_method = candidate.clone();
    wrong_method.observation.method = "client_claimed_native".into();
    assert!(matches!(
        adapter.admit(&owner(), graph.project_id(), &asset, 1, wrong_method),
        Err(GraphError::Denied)
    ));

    assert!(matches!(
        adapter.admit(&owner(), &ProjectId::new(), &asset, 1, candidate.clone(),),
        Err(GraphError::Denied)
    ));
    assert!(matches!(
        adapter.admit(&owner(), graph.project_id(), &asset, 2, candidate.clone()),
        Err(GraphError::Denied)
    ));

    let other = RevisionAdapter::registered(
        resource,
        EvidenceSource::FileRead,
        "godot_native_project_observation".into(),
        1,
    )
    .unwrap();
    assert!(matches!(
        other.admit(&owner(), graph.project_id(), &asset, 1, candidate),
        Err(GraphError::Denied)
    ));
}

#[test]
fn admitted_partial_native_revision_remains_unknown_not_cache_safe() {
    let (mut graph, access) = setup();
    let asset = asset(&mut graph, &access, "native-scene");
    let mut unknown = std::collections::BTreeSet::new();
    unknown.insert(DependencyClass::ImportSettings);
    let (resource, candidate) = native_revision_candidate(
        &graph,
        &asset,
        EvidenceSource::NativeApi,
        "godot_native_project_observation",
        Coverage {
            complete: false,
            unknown_frontier: unknown,
        },
    );
    let adapter = RevisionAdapter::registered(
        resource,
        EvidenceSource::NativeApi,
        "godot_native_project_observation".into(),
        1,
    )
    .unwrap();
    let admitted = adapter
        .admit(&owner(), graph.project_id(), &asset, 1, candidate)
        .unwrap();
    graph.accept_revision(&access, admitted).unwrap();

    let knowledge = graph.inspect(&access, &asset).unwrap().knowledge;
    assert_eq!(knowledge.verification, Verdict::Unknown);
    assert!(!knowledge.coverage.cache_safe());
    assert!(!knowledge.cache_safe());
}

#[test]
fn admitted_revision_is_owner_bound_and_nonexhaustive_complete_claim_is_rejected() {
    let (mut graph, access) = setup();
    let asset = asset(&mut graph, &access, "native-scene");
    let (resource, mut candidate) = native_revision_candidate(
        &graph,
        &asset,
        EvidenceSource::NativeApi,
        "godot_native_project_observation",
        Coverage::complete(),
    );
    candidate.observation.exhaustive = false;
    assert!(candidate.validate().is_err());

    candidate.observation.exhaustive = true;
    let adapter = RevisionAdapter::registered(
        resource,
        EvidenceSource::NativeApi,
        "godot_native_project_observation".into(),
        1,
    )
    .unwrap();
    let admitted = adapter
        .admit(&owner(), graph.project_id(), &asset, 1, candidate)
        .unwrap();
    let mut other_owner = owner();
    other_owner.session = "other-session".into();
    let other = ProjectAccess::authorized(
        other_owner,
        graph.project_id().clone(),
        None,
        true,
        digest("grants"),
    )
    .unwrap();
    assert!(matches!(
        graph.accept_revision(&other, admitted),
        Err(GraphError::Denied)
    ));
}

#[test]
fn provenance_explains_active_dependencies_derivatives_and_stale_reason() {
    let (mut graph, access, records) = chain();
    let source = records[0].pin.asset.clone();
    let middle = records[1].pin.asset.clone();
    let end = records[2].pin.asset.clone();

    let source_view = graph.provenance(&access, &source, 256).unwrap();
    assert_eq!(source_view.known_derivatives, vec![middle.clone()]);
    assert!(source_view.producer.is_none());

    let end_view = graph.provenance(&access, &end, 256).unwrap();
    assert_eq!(
        end_view.producer.as_ref().unwrap().inputs[0].asset,
        middle.clone()
    );
    assert!(!end_view.unknown_frontier);

    observe(&mut graph, &access, &source, "changed", 20);
    let stale = graph.provenance(&access, &end, 256).unwrap();
    assert_eq!(stale.asset.knowledge.freshness, Freshness::Stale);
    assert!(
        stale
            .reasons
            .contains(&ProvenanceReason::InputStale { asset: middle })
    );
}

#[test]
fn provenance_partial_scope_hides_receipt_membership_and_hidden_asset_ids() {
    let (graph, _access, records) = chain();
    let source = records[0].pin.asset.clone();
    let middle = records[1].pin.asset.clone();
    let end = records[2].pin.asset.clone();
    let subset = ProjectAccess::authorized(
        owner(),
        graph.project_id().clone(),
        Some([end.clone()].into()),
        false,
        digest("subset-grants"),
    )
    .unwrap();

    let view = graph.provenance(&subset, &end, 256).unwrap();
    assert!(view.producer.is_none());
    assert!(view.unknown_frontier);
    assert!(!view.truncated);
    assert!(view.known_derivatives.is_empty());
    assert!(view.possible_derivatives.is_empty());
    let wire = serde_json::to_string(&view).unwrap();
    assert!(!wire.contains(source.as_str()));
    assert!(!wire.contains(middle.as_str()));
}

#[test]
fn provenance_reports_changed_and_unknown_determinants_without_relabeling_history() {
    let (mut graph, access, records) = chain();
    let end = records[2].pin.asset.clone();
    let before = graph.provenance(&access, &end, 256).unwrap();
    let producer = before.producer.unwrap();
    let receipt = graph.receipt(&access, &producer.receipt).unwrap();
    let mut determinants = receipt.required_determinants();
    assert!(!determinants.is_empty());
    let changed_class = determinants[0].class;
    let changed_key = determinants[0].key.clone();
    determinants[0].digest = digest("changed-determinant");
    determinants.truncate(1);
    graph.observe_determinants(&access, determinants).unwrap();

    let view = graph.provenance(&access, &end, 256).unwrap();
    assert!(
        view.reasons
            .contains(&ProvenanceReason::DeterminantChanged {
                class: changed_class,
                key: changed_key,
            })
    );
    assert!(
        view.reasons
            .iter()
            .any(|reason| matches!(reason, ProvenanceReason::DeterminantUnknown { .. }))
    );
    assert_eq!(view.asset.knowledge.freshness, Freshness::Stale);
}

#[test]
fn declared_derivatives_remain_possible_and_provenance_is_bounded() {
    let (mut graph, access) = setup();
    let source = asset(&mut graph, &access, "source");
    let first = asset(&mut graph, &access, "declared-one");
    let second = asset(&mut graph, &access, "declared-two");

    for derived in [first.clone(), second.clone()] {
        graph
            .declare(
                &access,
                Edge {
                    from: Vertex::Asset(derived),
                    to: Vertex::Asset(source.clone()),
                    relation: Relation::DerivedFrom,
                    evidence: EdgeEvidence::Declared {
                        declaration: ReceiptId::new(),
                    },
                },
            )
            .unwrap();
    }

    assert!(graph.provenance(&access, &source, 0).is_err());
    assert!(graph.provenance(&access, &source, 257).is_err());

    let bounded = graph.provenance(&access, &source, 1).unwrap();
    assert!(bounded.known_derivatives.is_empty());
    assert_eq!(bounded.possible_derivatives.len(), 1);
    assert!(bounded.truncated);

    let complete = graph.provenance(&access, &source, 256).unwrap();
    assert!(complete.known_derivatives.is_empty());
    assert_eq!(
        complete.possible_derivatives,
        [first.clone(), second.clone()]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
    );
    assert!(!complete.truncated);
}
