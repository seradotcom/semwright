mod common;
use common::*;
use semwright_project_graph::*;
#[test]
fn watcher_loss_reorder_duplicate_and_rescan_are_explicit() {
    let (mut g, a) = setup();
    let id = asset(&mut g, &a, "source");
    observe(&mut g, &a, &id, "a", 1);
    let mut watcher = ScopedObserver::register(&mut g, &a, [id.clone()].into()).unwrap();
    let first = watcher
        .event(&mut g, &a, 10, WatchHint::Changed(vec![id.clone()]))
        .unwrap();
    assert!(first.requires_rescan);
    assert!(watcher.finish_rescan(&g, &a).is_err());
    let revision = g.snapshot_revision();
    let duplicate = watcher
        .event(&mut g, &a, 10, WatchHint::Changed(vec![id.clone()]))
        .unwrap();
    assert!(duplicate.duplicate);
    assert_eq!(g.snapshot_revision(), revision);
    observe(&mut g, &a, &id, "b", 2);
    watcher.finish_rescan(&g, &a).unwrap();
    let reordered = watcher
        .event(&mut g, &a, 9, WatchHint::Changed(vec![id.clone()]))
        .unwrap();
    assert!(reordered.requires_rescan);
    assert_eq!(g.inspect(&a, &id).unwrap().knowledge.label(), "UNKNOWN");
    observe(&mut g, &a, &id, "b", 3);
    watcher.finish_rescan(&g, &a).unwrap();
    watcher.event(&mut g, &a, 11, WatchHint::Overflow).unwrap();
    assert_eq!(g.inspect(&a, &id).unwrap().knowledge.label(), "UNKNOWN");
}
#[test]
fn imported_manifest_never_inherits_identity_locator_receipts_or_current() {
    let (mut source, a) = setup();
    let x = asset(&mut source, &a, "source");
    let y = asset(&mut source, &a, "output");
    observe(&mut source, &a, &x, "bytes", 1);
    source
        .declare(&a, declared(&y, &x, Relation::DerivedFrom))
        .unwrap();
    let manifest = source.export_manifest(&a, &[x.clone(), y.clone()]).unwrap();
    let bytes = composition::canonical_bytes(&manifest).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("locator"));
    let mut manifest = PortableManifest::decode(&bytes).unwrap();
    manifest.coverage_complete = true;
    let (mut destination, b) = setup();
    let mapping = destination.import_manifest(&b, &manifest).unwrap();
    assert_ne!(mapping[&x], x);
    for id in mapping.values() {
        let view = destination.inspect(&b, id).unwrap();
        assert!(view.asset.locator.is_none());
        assert_eq!(view.knowledge.label(), "UNKNOWN");
        assert!(!view.knowledge.cache_safe());
    }
}
#[test]
fn malformed_manifest_and_parent_cycle_leave_target_unchanged() {
    let (mut g, a) = setup();
    let id = LogicalAssetId::new();
    let m = PortableManifest {
        version: 1,
        source_project: ProjectId::new(),
        source_snapshot: 3,
        assets: vec![PortableAsset {
            source_id: id.clone(),
            label: "a".into(),
            resource_type: "file".into(),
        }],
        declarations: vec![PortableEdge {
            from: id.clone(),
            to: id,
            relation: Relation::Contains,
        }],
        coverage_complete: true,
    };
    assert!(g.import_manifest(&a, &m).is_err());
    assert_eq!(g.snapshot_revision(), 0);
    assert!(PortableManifest::decode(br#"{"version":1,"version":2}"#).is_err());
}

#[test]
fn watcher_registration_and_restart_never_reuse_old_rescan_evidence() {
    let (mut graph, access) = setup();
    let id = asset(&mut graph, &access, "file");
    observe(&mut graph, &access, &id, "initial", 1);
    let mut watcher = ScopedObserver::register(&mut graph, &access, [id.clone()].into()).unwrap();
    assert_eq!(
        graph.inspect(&access, &id).unwrap().knowledge.label(),
        "UNKNOWN"
    );
    assert!(watcher.finish_rescan(&graph, &access).is_err());
    observe(&mut graph, &access, &id, "current", 2);
    watcher.finish_rescan(&graph, &access).unwrap();
    graph.restart_observation_epoch();
    assert!(
        watcher
            .event(&mut graph, &access, 1, WatchHint::Changed(vec![id]))
            .is_err()
    );
}
