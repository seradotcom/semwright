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

fn request(target: LogicalAssetId) -> RebuildRequest {
    RebuildRequest {
        targets: vec![target],
        traversal: budget(),
        convergence: composition::ConvergenceBudget {
            max_iterations: 8,
            max_operations: 16,
            max_findings: 16,
            max_observations: 16,
            max_elapsed_ms: 60_000,
        },
    }
}
#[test]
fn altered_reservation_cannot_be_promoted_even_with_a_recomputed_digest() {
    let (mut g, a, records) = pipeline();
    observe(&mut g, &a, &records[0].pin.asset, "changed", 20);
    let mut vault = composition::PlanVault::bounded(16, 16, 32);
    let reservation = g
        .reserve_rebuild(
            &a,
            &request(records[2].pin.asset.clone()),
            &Catalog,
            &mut vault,
            &AtomicBool::new(false),
        )
        .unwrap();
    let mut altered = reservation.clone();
    altered.proposal.nodes[0].historical_operation.parameters = digest("injected");
    // Computing a new public digest cannot change A's server-held canonical bytes.
    let _digest = composition::canonical_digest(&altered).unwrap();
    assert!(
        g.begin_rebuild_preparation(
            &a,
            &altered,
            &Catalog,
            &mut vault,
            "prepare-1",
            &AtomicBool::new(false)
        )
        .is_err()
    );
    let permit = g
        .begin_rebuild_preparation(
            &a,
            &reservation,
            &Catalog,
            &mut vault,
            "prepare-1",
            &AtomicBool::new(false),
        )
        .unwrap();
    vault
        .finish(permit, composition::ExecutionStatus::Completed, vec![])
        .unwrap();
    assert!(
        g.begin_rebuild_preparation(
            &a,
            &reservation,
            &Catalog,
            &mut vault,
            "prepare-2",
            &AtomicBool::new(false)
        )
        .is_err()
    );
}
#[test]
fn reservation_binds_session_grants_snapshot_and_restart_epoch() {
    let (mut g, a, records) = pipeline();
    observe(&mut g, &a, &records[0].pin.asset, "changed", 20);
    let mut vault = composition::PlanVault::bounded(16, 16, 32);
    let reservation = g
        .reserve_rebuild(
            &a,
            &request(records[2].pin.asset.clone()),
            &Catalog,
            &mut vault,
            &AtomicBool::new(false),
        )
        .unwrap();
    let mut other_owner = owner();
    other_owner.session = "new-session".into();
    let session = ProjectAccess::authorized(
        other_owner,
        g.project_id().clone(),
        None,
        true,
        digest("grants"),
    )
    .unwrap();
    assert!(
        g.begin_rebuild_preparation(
            &session,
            &reservation,
            &Catalog,
            &mut vault,
            "p",
            &AtomicBool::new(false)
        )
        .is_err()
    );
    let grants = ProjectAccess::authorized(
        owner(),
        g.project_id().clone(),
        None,
        true,
        digest("changed-grants"),
    )
    .unwrap();
    assert!(matches!(
        g.begin_rebuild_preparation(
            &grants,
            &reservation,
            &Catalog,
            &mut vault,
            "p",
            &AtomicBool::new(false)
        ),
        Err(GraphError::Denied)
    ));
    let mut restarted = g.clone();
    restarted.restart_observation_epoch();
    assert!(matches!(
        restarted.begin_rebuild_preparation(
            &a,
            &reservation,
            &Catalog,
            &mut vault,
            "p",
            &AtomicBool::new(false)
        ),
        Err(GraphError::Conflict)
    ));
    g.rename(&a, &records[0].pin.asset, "renamed source".into())
        .unwrap();
    assert!(matches!(
        g.begin_rebuild_preparation(
            &a,
            &reservation,
            &Catalog,
            &mut vault,
            "p",
            &AtomicBool::new(false)
        ),
        Err(GraphError::Conflict)
    ));
}
#[test]
fn changed_runtime_or_cancelled_attempt_does_not_enter_the_controller() {
    struct Changed;
    impl RebuildCatalog for Changed {
        fn lookup(&self, p: &OperationIdentity) -> Result<Option<RebuildBinding>> {
            let mut binding = Catalog.lookup(p)?.unwrap();
            binding.runtime = digest("new-runtime");
            Ok(Some(binding))
        }
    }
    let (mut g, a, records) = pipeline();
    observe(&mut g, &a, &records[0].pin.asset, "changed", 20);
    let mut vault = composition::PlanVault::bounded(16, 16, 32);
    let reservation = g
        .reserve_rebuild(
            &a,
            &request(records[2].pin.asset.clone()),
            &Catalog,
            &mut vault,
            &AtomicBool::new(false),
        )
        .unwrap();
    assert!(matches!(
        g.begin_rebuild_preparation(
            &a,
            &reservation,
            &Changed,
            &mut vault,
            "p",
            &AtomicBool::new(false)
        ),
        Err(GraphError::Conflict)
    ));
    assert!(matches!(
        g.begin_rebuild_preparation(
            &a,
            &reservation,
            &Catalog,
            &mut vault,
            "p",
            &AtomicBool::new(true)
        ),
        Err(GraphError::Cancelled)
    ));
    assert!(
        vault
            .ledger(&owner(), &reservation.plan_ref)
            .unwrap()
            .is_empty()
    );
}
#[test]
fn diverged_output_is_not_silently_overwritten_by_rebuild() {
    let (mut g, a, records) = pipeline();
    observe(&mut g, &a, &records[2].pin.asset, "external edit", 20);
    let p = g
        .propose_rebuild(
            &a,
            &[records[2].pin.asset.clone()],
            &Catalog,
            budget(),
            &AtomicBool::new(false),
        )
        .unwrap();
    assert!(!p.ready_for_preparation());
    assert!(
        p.nodes
            .iter()
            .any(|n| n.blockers.contains(&RebuildBlock::OutputDiverged))
    );
}
#[test]
fn exceeded_edge_budget_does_not_build_a_second_unbounded_ordering() {
    let (mut g, a, records) = pipeline();
    observe(&mut g, &a, &records[0].pin.asset, "changed", 20);
    let mut limits = budget();
    limits.edges = 1;
    let p = g
        .propose_rebuild(
            &a,
            &[records[2].pin.asset.clone()],
            &Catalog,
            limits,
            &AtomicBool::new(false),
        )
        .unwrap();
    assert!(p.truncated && p.unknown_frontier && p.order.is_empty());
    assert_eq!(p.visited_edges, 1);
    assert!(!p.ready_for_preparation());
}
