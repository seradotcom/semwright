//! Independent reachability oracle; no production SCC/traversal helper is reused.
mod common;
use common::*;
use proptest::prelude::*;
use semwright_project_graph::*;
use std::collections::BTreeSet;
use std::sync::atomic::AtomicBool;
struct Catalog;
impl RebuildCatalog for Catalog {
    fn lookup(&self, p: &OperationIdentity) -> Result<Option<RebuildBinding>> {
        Ok(Some(RebuildBinding {
            capability: p.capability.clone(),
            descriptor: p.descriptor.clone(),
            runtime: p.runtime.clone(),
            prepare_capability: "fixture.prepare".into(),
            prepare_descriptor: digest("prepare"),
        }))
    }
}
fn reachable(edges: &[Vec<bool>], start: usize, target: usize) -> bool {
    let mut pending = vec![start];
    let mut visited = BTreeSet::new();
    while let Some(node) = pending.pop() {
        if node == target {
            return true;
        }
        if !visited.insert(node) {
            continue;
        }
        for (next, edge) in edges[node].iter().enumerate() {
            if *edge {
                pending.push(next);
            }
        }
    }
    false
}
proptest! {
    #![proptest_config(ProptestConfig { cases: 48, max_shrink_iters: 768, .. ProptestConfig::default() })]
    #[test]
    fn production_sccs_match_independent_pairwise_reachability(
        count in 1usize..9,
        masks in proptest::collection::vec(any::<u16>(), 8)
    ) {
        let edges: Vec<Vec<bool>> = (0..count)
            .map(|i| (0..count).map(|j| masks[i] & (1 << j) != 0).collect())
            .collect();
        let (mut graph, access) = setup();
        let source = asset(&mut graph, &access, "unproduced-source");
        let source_record = observe(&mut graph, &access, &source, "source", 1);
        let ids: Vec<_> = (0..count).map(|i| asset(&mut graph, &access, &format!("node-{i}"))).collect();
        let records: Vec<_> = ids.iter().enumerate()
            .map(|(i, id)| observe(&mut graph, &access, id, &format!("bytes-{i}"), i as u64 + 2))
            .collect();
        let mut activities = Vec::new();
        for (i, incoming) in edges.iter().enumerate() {
            let mut inputs: Vec<_> = incoming.iter().enumerate()
                .filter(|(_, edge)| **edge).map(|(j, _)| records[j].clone()).collect();
            if inputs.is_empty() { inputs.push(source_record.clone()); }
            let receipt = receipt(graph.project_id().clone(), &inputs, &records[i], i as u64 + 100);
            activities.push(receipt.derivation.clone());
            graph.accept_receipt(&access, admit(receipt)).unwrap();
        }
        // Force traversal of every produced asset without pretending a cache hit.
        graph.restart_observation_epoch();
        let plan = graph.propose_rebuild(&access, &ids, &Catalog, budget(), &AtomicBool::new(false)).unwrap();
        prop_assert!(!plan.truncated && !plan.cancelled);
        let mut expected = Vec::new();
        let mut grouped = BTreeSet::new();
        for (i, activity) in activities.iter().enumerate() {
            if grouped.contains(activity) { continue; }
            let mut component: Vec<_> = activities.iter().enumerate()
                .filter(|(j, _)| reachable(&edges, i, *j) && reachable(&edges, *j, i))
                .map(|(_, id)| id.clone()).collect();
            grouped.extend(component.iter().cloned());
            if component.len() > 1 || edges[i][i] {
                component.sort(); expected.push(component);
            }
        }
        expected.sort();
        prop_assert_eq!(&plan.cycles, &expected);
        if expected.is_empty() {
            prop_assert_eq!(plan.order.len(), count);
            for node in &plan.nodes {
                for dependency in &node.depends_on {
                    prop_assert!(plan.order.iter().position(|d| d == dependency)
                        < plan.order.iter().position(|d| d == &node.derivation));
                }
            }
        } else {
            prop_assert!(plan.order.is_empty());
        }
        let wire = composition::canonical_bytes(&plan).unwrap();
        let decoded: RebuildProposal = composition::strict_decode(&wire).unwrap();
        prop_assert_eq!(decoded.cycles, plan.cycles);
    }
}
