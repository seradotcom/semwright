#![no_main]
use libfuzzer_sys::fuzz_target;
use semwright_project_graph::*;
use std::collections::BTreeSet;
use std::sync::atomic::AtomicBool;
fuzz_target!(|data: &[u8]| {
    if data.len() > 512 {
        return;
    }
    let project = ProjectId::new();
    let principal = composition::PrincipalBinding::Named("traversal-fuzz".into());
    let owner = composition::Owner {
        session: "fuzz".into(),
        principal: principal.clone(),
    };
    let access = ProjectAccess::authorized(
        owner,
        project.clone(),
        None,
        true,
        composition::Digest::of_bytes(b"grants"),
    )
    .unwrap();
    let mut graph = ProjectGraph::new(project, principal).unwrap();
    let ids: Vec<_> = (0..16)
        .map(|n| {
            let id = LogicalAssetId::new();
            graph
                .register(
                    &access,
                    Asset {
                        id: id.clone(),
                        resource_type: "synthetic".into(),
                        label: format!("resource-{n}"),
                        locator: None,
                    },
                )
                .unwrap();
            id
        })
        .collect();
    let links: BTreeSet<_> = data
        .chunks_exact(2)
        .map(|pair| (usize::from(pair[0]) % 16, usize::from(pair[1]) % 16))
        .collect();
    for (from, to) in &links {
        graph
            .declare(
                &access,
                Edge {
                    from: Vertex::Asset(ids[*to].clone()),
                    to: Vertex::Asset(ids[*from].clone()),
                    relation: Relation::References,
                    evidence: EdgeEvidence::Declared {
                        declaration: ReceiptId::new(),
                    },
                },
            )
            .unwrap();
    }
    let mut reachable = BTreeSet::from([0]);
    loop {
        let old = reachable.len();
        for (from, to) in &links {
            if reachable.contains(from) {
                reachable.insert(*to);
            }
        }
        if reachable.len() == old {
            break;
        }
    }
    reachable.remove(&0);
    let budget = TraversalBudget {
        nodes: 64,
        edges: 1024,
        depth: 64,
        results: 64,
    };
    let report = graph
        .impact(&access, &ids[0], budget, &AtomicBool::new(false))
        .unwrap();
    assert!(!report.truncated);
    assert!(report.known.is_empty());
    assert_eq!(
        report
            .possible
            .into_iter()
            .map(|h| h.asset)
            .collect::<BTreeSet<_>>(),
        reachable.into_iter().map(|n| ids[n].clone()).collect()
    );
});
