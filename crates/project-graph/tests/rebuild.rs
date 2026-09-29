mod common;
use common::*;
use semwright_project_graph::*;
use std::sync::atomic::AtomicBool;
struct Catalog;
impl RebuildCatalog for Catalog {
    fn lookup(&self, production: &OperationIdentity) -> Result<Option<RebuildBinding>> {
        Ok(Some(RebuildBinding {
            capability: production.capability.clone(),
            descriptor: production.descriptor.clone(),
            runtime: production.runtime.clone(),
            prepare_capability: "fixture.prepare".into(),
            prepare_descriptor: digest("prepare"),
        }))
    }
}
fn pipeline() -> (ProjectGraph, ProjectAccess, Vec<RevisionRecord>) {
    let (mut g, a) = setup();
    let ids: Vec<_> = (0..3)
        .map(|i| asset(&mut g, &a, &format!("asset-{i}")))
        .collect();
    let records: Vec<_> = ids
        .iter()
        .enumerate()
        .map(|(i, id)| observe(&mut g, &a, id, &format!("bytes-{i}"), i as u64 + 1))
        .collect();
    let mut determinants = Vec::new();
    for i in 1..3 {
        let r = receipt(
            g.project_id().clone(),
            &records[i - 1..i],
            &records[i],
            i as u64 + 10,
        );
        determinants.extend(r.required_determinants());
        g.accept_receipt(&a, admit(r)).unwrap();
    }
    g.observe_determinants(&a, determinants).unwrap();
    (g, a, records)
}
#[test]
fn stale_chain_proposes_preparation_in_dependency_order_not_saved_commands() {
    let (mut g, a, records) = pipeline();
    observe(&mut g, &a, &records[0].pin.asset, "changed", 20);
    let p = g
        .propose_rebuild(
            &a,
            &[records[2].pin.asset.clone()],
            &Catalog,
            budget(),
            &AtomicBool::new(false),
        )
        .unwrap();
    assert_eq!(p.nodes.len(), 2);
    assert_eq!(p.order.len(), 2);
    assert!(p.ready_for_preparation());
    for node in &p.nodes {
        for dependency in &node.depends_on {
            assert!(
                p.order.iter().position(|d| d == dependency)
                    < p.order.iter().position(|d| d == &node.derivation)
            );
        }
    }
    let wire = serde_json::to_value(&p).unwrap();
    assert!(wire.get("commands").is_none());
    assert!(wire.get("args").is_none());
}
#[test]
fn complete_current_outputs_are_reused_but_unknowns_are_not() {
    let (mut g, a, records) = pipeline();
    let target = records[2].pin.asset.clone();
    let current = g
        .propose_rebuild(
            &a,
            std::slice::from_ref(&target),
            &Catalog,
            budget(),
            &AtomicBool::new(false),
        )
        .unwrap();
    assert_eq!(current.reusable_outputs.len(), 1);
    assert!(current.nodes.is_empty());
    g.restart_observation_epoch();
    let unknown = g
        .propose_rebuild(&a, &[target], &Catalog, budget(), &AtomicBool::new(false))
        .unwrap();
    assert!(unknown.reusable_outputs.is_empty());
    assert!(!unknown.ready_for_preparation());
    assert!(unknown.unknown_frontier);
}
#[test]
fn production_cycle_returns_scc_and_never_a_topological_order() {
    let (mut g, a) = setup();
    let x = asset(&mut g, &a, "x");
    let y = asset(&mut g, &a, "y");
    let xr = observe(&mut g, &a, &x, "x", 1);
    let yr = observe(&mut g, &a, &y, "y", 2);
    let rx = receipt(g.project_id().clone(), std::slice::from_ref(&yr), &xr, 3);
    let ry = receipt(g.project_id().clone(), std::slice::from_ref(&xr), &yr, 4);
    g.observe_determinants(
        &a,
        rx.required_determinants()
            .into_iter()
            .chain(ry.required_determinants())
            .collect(),
    )
    .unwrap();
    g.accept_receipt(&a, admit(rx)).unwrap();
    g.accept_receipt(&a, admit(ry)).unwrap();
    let p = g
        .propose_rebuild(&a, &[x], &Catalog, budget(), &AtomicBool::new(false))
        .unwrap();
    assert_eq!(p.cycles.len(), 1);
    assert_eq!(p.cycles[0].len(), 2);
    assert!(p.order.is_empty());
    assert!(!p.ready_for_preparation());
}
#[test]
fn subset_cannot_replace_global_determinants_or_see_hidden_rebuild_inputs() {
    let (mut g, _, records) = pipeline();
    let target = records[2].pin.asset.clone();
    let a = ProjectAccess::authorized(
        owner(),
        g.project_id().clone(),
        Some([target.clone()].into()),
        true,
        digest("subset"),
    )
    .unwrap();
    assert!(matches!(
        g.observe_determinants(&a, vec![]),
        Err(GraphError::Denied)
    ));
    let p = g
        .propose_rebuild(&a, &[target], &Catalog, budget(), &AtomicBool::new(false))
        .unwrap();
    assert!(p.nodes.is_empty());
    assert!(p.unknown_frontier);
    let json = serde_json::to_string(&p).unwrap();
    assert!(!json.contains(records[0].pin.asset.as_str()));
    assert!(!json.contains(records[1].pin.asset.as_str()));
}
#[test]
fn cancelled_and_bounded_plans_cannot_be_called_complete() {
    let (mut g, a, records) = pipeline();
    observe(&mut g, &a, &records[0].pin.asset, "changed", 20);
    let p = g
        .propose_rebuild(
            &a,
            &[records[2].pin.asset.clone()],
            &Catalog,
            budget(),
            &AtomicBool::new(true),
        )
        .unwrap();
    assert!(p.cancelled && p.truncated && p.order.is_empty());
    let mut small = budget();
    small.nodes = 1;
    let p = g
        .propose_rebuild(
            &a,
            &[records[2].pin.asset.clone()],
            &Catalog,
            small,
            &AtomicBool::new(false),
        )
        .unwrap();
    assert!(p.truncated && p.order.is_empty());
    assert!(!p.ready_for_preparation());
}
#[test]
fn unavailable_catalog_is_a_blocker_not_provider_substitution() {
    struct Missing;
    impl RebuildCatalog for Missing {
        fn lookup(&self, _: &OperationIdentity) -> Result<Option<RebuildBinding>> {
            Ok(None)
        }
    }
    let (mut g, a, records) = pipeline();
    observe(&mut g, &a, &records[0].pin.asset, "changed", 20);
    let p = g
        .propose_rebuild(
            &a,
            &[records[2].pin.asset.clone()],
            &Missing,
            budget(),
            &AtomicBool::new(false),
        )
        .unwrap();
    assert!(!p.ready_for_preparation());
    assert!(
        p.nodes
            .iter()
            .all(|n| n.blockers.contains(&RebuildBlock::CapabilityUnavailable))
    );
}
