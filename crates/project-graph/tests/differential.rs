mod common;
use common::*;
use proptest::prelude::*;
use semwright_project_graph::*;
use std::collections::BTreeSet;
use std::sync::atomic::AtomicBool;
proptest! {
    #![proptest_config(ProptestConfig::with_cases(40))]
    #[test]
    fn incremental_receipt_index_matches_independent_full_fixed_point(raw in prop::collection::vec((0usize..25,0usize..25),0..160)) {
        let links: BTreeSet<_> = raw.into_iter().filter(|(a,b)| a != b).collect();
        let (mut graph, access) = setup(); let mut records = Vec::new();
        for i in 0..25 { let id = asset(&mut graph, &access, &format!("node-{i}")); records.push(observe(&mut graph, &access, &id, &format!("bytes-{i}"), i as u64 + 1)); }
        for output in 0..25 { let inputs: Vec<_> = links.iter().filter(|(_,to)| *to == output).map(|(from,_)| records[*from].clone()).collect(); if !inputs.is_empty() { let r = receipt(graph.project_id().clone(), &inputs, &records[output], 100 + output as u64); graph.accept_receipt(&access, admit(r)).unwrap(); } }
        let mut reachable = BTreeSet::from([0usize]);
        loop { let prior = reachable.len(); for (input, output) in &links { if reachable.contains(input) { reachable.insert(*output); } } if prior == reachable.len() { break; } }
        reachable.remove(&0);
        let expected: BTreeSet<_> = reachable.into_iter().map(|n| records[n].pin.asset.clone()).collect();
        let impact = graph.impact(&access, &records[0].pin.asset, budget(), &AtomicBool::new(false)).unwrap();
        prop_assert!(!impact.truncated); prop_assert!(impact.possible.is_empty());
        let actual: BTreeSet<_> = impact.known.into_iter().map(|h| h.asset).collect(); prop_assert_eq!(actual, expected);
    }
}
