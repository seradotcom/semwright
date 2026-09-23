//! Bounded scene traversal utility. Parents may share children; cycles are never recursively expanded.
use crate::{Fault, FaultKind, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub uuid: String,
    pub children: Vec<String>,
}
#[derive(Debug, Serialize)]
pub struct Walk {
    pub nodes: Vec<String>,
    pub cycles_or_shared: usize,
    pub truncated: bool,
}
pub fn walk(
    graph: &BTreeMap<String, Node>,
    root: &str,
    max_depth: usize,
    max_nodes: usize,
) -> Result<Walk> {
    if max_depth > 16 || max_nodes == 0 || max_nodes > 1024 || graph.len() > 2048 {
        return Err(Fault::new(FaultKind::ResourceLimit));
    }
    if !graph.contains_key(root) {
        return Err(Fault::new(FaultKind::Precondition));
    }
    let mut pending = VecDeque::from([(root.to_owned(), 0)]);
    let mut seen = BTreeSet::new();
    let mut out = Walk {
        nodes: vec![],
        cycles_or_shared: 0,
        truncated: false,
    };
    while let Some((id, depth)) = pending.pop_front() {
        if !seen.insert(id.clone()) {
            out.cycles_or_shared += 1;
            continue;
        }
        if out.nodes.len() >= max_nodes {
            out.truncated = true;
            break;
        }
        let node = graph
            .get(&id)
            .ok_or_else(|| Fault::new(FaultKind::Protocol))?;
        if node.children.len() > 1024 || node.uuid != id {
            return Err(Fault::new(FaultKind::Protocol));
        }
        out.nodes.push(id);
        if depth == max_depth {
            out.truncated |= !node.children.is_empty();
            continue;
        }
        for child in &node.children {
            if pending.len() >= 2048 {
                return Err(Fault::new(FaultKind::ResourceLimit));
            }
            pending.push_back((child.clone(), depth + 1));
        }
    }
    Ok(out)
}
