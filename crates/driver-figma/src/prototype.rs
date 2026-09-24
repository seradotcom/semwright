use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub action: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PrototypeFinding {
    pub rule: String,
    pub node: String,
    pub message: String,
}

pub fn validate(
    nodes: &BTreeSet<String>,
    starts: &BTreeSet<String>,
    edges: &[Edge],
) -> Vec<PrototypeFinding> {
    let mut f = vec![];
    for e in edges {
        if !nodes.contains(&e.to) {
            f.push(PrototypeFinding {
                rule: "dangling_destination".into(),
                node: e.from.clone(),
                message: format!("destination {} is absent", e.to),
            });
        }
    }
    if starts.is_empty() {
        f.push(PrototypeFinding {
            rule: "missing_start_flow".into(),
            node: "document".into(),
            message: "prototype has no starting point".into(),
        });
    }
    let adj: BTreeMap<_, Vec<_>> = nodes
        .iter()
        .map(|n| {
            (
                n.clone(),
                edges
                    .iter()
                    .filter(|e| &e.from == n)
                    .map(|e| e.to.clone())
                    .collect(),
            )
        })
        .collect();
    let mut seen = BTreeSet::new();
    let mut stack: Vec<_> = starts.iter().cloned().collect();
    while let Some(n) = stack.pop() {
        if seen.insert(n.clone())
            && let Some(next) = adj.get(&n)
        {
            stack.extend(next.iter().cloned());
        }
    }
    for n in nodes {
        if !seen.contains(n) {
            f.push(PrototypeFinding {
                rule: "unreachable".into(),
                node: n.clone(),
                message: "node is unreachable from a flow start".into(),
            });
        }
    }
    f
}
