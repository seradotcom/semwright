//! Bounded reconstruction proposals. These are data, never execution grants.
use crate::*;
use composition::Digest;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};

/// Supplied by a trusted catalog adapter. No serialized program or arbitrary arguments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RebuildBinding {
    pub capability: String,
    pub descriptor: Digest,
    pub runtime: Digest,
    pub prepare_capability: String,
    pub prepare_descriptor: Digest,
}
impl RebuildBinding {
    pub fn validate(&self) -> Result<()> {
        name(&self.capability)?;
        name(&self.prepare_capability)
    }
}
/// Lookup observes the current registered catalog, not a client's claimed descriptors.
pub trait RebuildCatalog {
    fn lookup(&self, production: &OperationIdentity) -> Result<Option<RebuildBinding>>;
}
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum RebuildBlock {
    NoProductionReceipt,
    ObservationRequired,
    IncompleteDependencies,
    HiddenDependencies,
    CapabilityUnavailable,
    InPlaceProduction,
    ProductionCycle,
    BudgetExhausted,
    Cancelled,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RebuildNode {
    pub derivation: DerivationId,
    pub receipt: ReceiptId,
    pub historical_operation: OperationIdentity,
    pub current_binding: Option<RebuildBinding>,
    pub input_pins: Vec<RevisionPin>,
    pub output_pins: Vec<RevisionPin>,
    pub depends_on: BTreeSet<DerivationId>,
    pub blockers: BTreeSet<RebuildBlock>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RebuildProposal {
    pub version: u32,
    pub project: ProjectId,
    pub snapshot: u64,
    pub observation_epoch: String,
    pub targets: Vec<LogicalAssetId>,
    pub nodes: Vec<RebuildNode>,
    /// Empty when any production cycle or incomplete traversal prevents a valid order.
    pub order: Vec<DerivationId>,
    pub cycles: Vec<Vec<DerivationId>>,
    pub reusable_outputs: Vec<RevisionPin>,
    pub unresolved: BTreeMap<LogicalAssetId, BTreeSet<RebuildBlock>>,
    pub unknown_frontier: bool,
    pub truncated: bool,
    pub cancelled: bool,
    pub visited_nodes: usize,
    pub visited_edges: usize,
}
impl RebuildProposal {
    /// Even true means only that native planning may begin through Broker policy.
    /// It does not authorize execution of a previous operation or reuse old refs.
    pub fn ready_for_preparation(&self) -> bool {
        !self.truncated
            && !self.cancelled
            && !self.unknown_frontier
            && self.cycles.is_empty()
            && self.unresolved.is_empty()
            && self.nodes.iter().all(|node| node.blockers.is_empty())
    }
}
impl ProjectGraph {
    pub fn propose_rebuild(
        &self,
        access: &ProjectAccess,
        targets: &[LogicalAssetId],
        catalog: &impl RebuildCatalog,
        budget: TraversalBudget,
        cancellation: &AtomicBool,
    ) -> Result<RebuildProposal> {
        self.access(access, false)?;
        budget.validate()?;
        ensure(
            !targets.is_empty() && targets.len() <= 256,
            "rebuild target budget",
        )?;
        let unique: BTreeSet<_> = targets.iter().cloned().collect();
        ensure(unique.len() == targets.len(), "duplicate rebuild target")?;
        for id in targets {
            self.visible(access, id)?;
        }
        let mut report = RebuildProposal {
            version: 1,
            project: self.project.clone(),
            snapshot: self.sequence,
            observation_epoch: self.epoch.clone(),
            targets: targets.to_vec(),
            nodes: Vec::new(),
            order: Vec::new(),
            cycles: Vec::new(),
            reusable_outputs: Vec::new(),
            unresolved: BTreeMap::new(),
            unknown_frontier: access.visible.is_some(),
            truncated: false,
            cancelled: false,
            visited_nodes: 0,
            visited_edges: 0,
        };
        let mut pending: VecDeque<_> = targets.iter().cloned().map(|id| (id, 0usize)).collect();
        let mut visited = BTreeSet::new();
        let mut nodes = BTreeMap::<DerivationId, RebuildNode>::new();
        while let Some((id, depth)) = pending.pop_front() {
            if cancellation.load(Ordering::Relaxed) {
                report.cancelled = true;
                report.truncated = true;
                break;
            }
            if !visited.insert(id.clone()) {
                continue;
            }
            if report.visited_nodes >= budget.nodes || depth > budget.depth {
                report.truncated = true;
                report
                    .unresolved
                    .entry(id)
                    .or_default()
                    .insert(RebuildBlock::BudgetExhausted);
                continue;
            }
            report.visited_nodes += 1;
            let state = self.visible(access, &id)?;
            let view = self.inspect(access, &id)?;
            if view.knowledge.cache_safe() {
                let pin = state
                    .latest
                    .as_ref()
                    .and_then(|r| self.revisions.get(r))
                    .ok_or(GraphError::Corrupt)?
                    .pin
                    .clone();
                report.reusable_outputs.push(pin);
                continue;
            }
            let Some(receipt) = state.producer.as_ref().and_then(|r| self.receipts.get(r)) else {
                // An observed source can feed a native replan, but is never an output cache hit.
                if unique.contains(&id)
                    || view.knowledge.existence != Existence::Present
                    || view.knowledge.freshness != Freshness::Current
                    || view.knowledge.requires_reconcile
                    || !view.knowledge.coverage.cache_safe()
                {
                    report
                        .unresolved
                        .entry(id)
                        .or_default()
                        .insert(RebuildBlock::NoProductionReceipt);
                    report.unknown_frontier = true;
                }
                continue;
            };
            // Omit the ENTIRE receipt when any part is not granted. Do not leak its count/IDs.
            if receipt
                .inputs
                .iter()
                .chain(&receipt.outputs)
                .any(|p| !access.sees(&p.asset))
            {
                report
                    .unresolved
                    .entry(id)
                    .or_default()
                    .insert(RebuildBlock::HiddenDependencies);
                report.unknown_frontier = true;
                continue;
            }
            if nodes.contains_key(&receipt.derivation) {
                continue;
            }
            if nodes.len() >= budget.results {
                report.truncated = true;
                break;
            }
            let mut blockers = BTreeSet::new();
            if !receipt.coverage.cache_safe() || !view.knowledge.coverage.cache_safe() {
                blockers.insert(RebuildBlock::IncompleteDependencies);
                report.unknown_frontier = true;
            }
            if view.knowledge.requires_reconcile || view.knowledge.freshness == Freshness::Unknown {
                blockers.insert(RebuildBlock::ObservationRequired);
                report.unknown_frontier = true;
            }
            if receipt
                .inputs
                .iter()
                .any(|input| receipt.outputs.iter().any(|o| o.asset == input.asset))
            {
                blockers.insert(RebuildBlock::InPlaceProduction);
            }
            let current_binding = catalog.lookup(&receipt.operation)?;
            if let Some(binding) = &current_binding {
                binding.validate()?;
                ensure(
                    binding.capability == receipt.operation.capability,
                    "catalog capability substitution",
                )?;
            } else {
                blockers.insert(RebuildBlock::CapabilityUnavailable);
            }
            for input in &receipt.inputs {
                if report.visited_edges >= budget.edges {
                    report.truncated = true;
                    blockers.insert(RebuildBlock::BudgetExhausted);
                    break;
                }
                report.visited_edges += 1;
                pending.push_back((input.asset.clone(), depth + 1));
            }
            nodes.insert(
                receipt.derivation.clone(),
                RebuildNode {
                    derivation: receipt.derivation.clone(),
                    receipt: receipt.id.clone(),
                    historical_operation: receipt.operation.clone(),
                    current_binding,
                    input_pins: receipt.inputs.clone(),
                    output_pins: receipt.outputs.clone(),
                    depends_on: BTreeSet::new(),
                    blockers,
                },
            );
        }
        let node_ids: BTreeSet<_> = nodes.keys().cloned().collect();
        for node in nodes.values_mut() {
            for input in &node.input_pins {
                if let Some(upstream) = self
                    .assets
                    .get(&input.asset)
                    .and_then(|s| s.producer.as_ref())
                    .and_then(|r| self.receipts.get(r))
                {
                    if node_ids.contains(&upstream.derivation) {
                        node.depends_on.insert(upstream.derivation.clone());
                    }
                }
            }
        }
        let adjacency: BTreeMap<_, _> = nodes
            .iter()
            .map(|(id, n)| (id.clone(), n.depends_on.clone()))
            .collect();
        report.cycles = production_cycles(&adjacency);
        let cyclic: BTreeSet<_> = report.cycles.iter().flatten().cloned().collect();
        for id in &cyclic {
            nodes
                .get_mut(id)
                .ok_or(GraphError::Corrupt)?
                .blockers
                .insert(RebuildBlock::ProductionCycle);
        }
        if report.cycles.is_empty() && !report.truncated && !report.cancelled {
            report.order = dependency_order(&adjacency)?;
        }
        report.nodes = nodes.into_values().collect();
        report.unknown_frontier |= report.truncated || report.cancelled;
        // This also enforces Composition's canonical wire-size bound for a proposal.
        composition::canonical_bytes(&report)?;
        Ok(report)
    }
}

/// Iterative Kosaraju: no recursive DFS or fabricated ordering through a cycle.
fn production_cycles(
    adjacency: &BTreeMap<DerivationId, BTreeSet<DerivationId>>,
) -> Vec<Vec<DerivationId>> {
    let mut visited = BTreeSet::new();
    let mut finish = Vec::new();
    for start in adjacency.keys() {
        let mut stack = vec![(start.clone(), false)];
        while let Some((id, done)) = stack.pop() {
            if done {
                finish.push(id);
                continue;
            }
            if !visited.insert(id.clone()) {
                continue;
            }
            stack.push((id.clone(), true));
            if let Some(next) = adjacency.get(&id) {
                stack.extend(next.iter().rev().cloned().map(|n| (n, false)));
            }
        }
    }
    let mut reverse = BTreeMap::<DerivationId, BTreeSet<DerivationId>>::new();
    for (id, dependencies) in adjacency {
        reverse.entry(id.clone()).or_default();
        for dependency in dependencies {
            reverse
                .entry(dependency.clone())
                .or_default()
                .insert(id.clone());
        }
    }
    visited.clear();
    let mut result = Vec::new();
    for start in finish.into_iter().rev() {
        if visited.contains(&start) {
            continue;
        }
        let mut component = Vec::new();
        let mut stack = vec![start];
        while let Some(id) = stack.pop() {
            if !visited.insert(id.clone()) {
                continue;
            }
            if let Some(next) = reverse.get(&id) {
                stack.extend(next.iter().cloned());
            }
            component.push(id);
        }
        component.sort();
        if component.len() > 1
            || component
                .first()
                .is_some_and(|id| adjacency.get(id).is_some_and(|v| v.contains(id)))
        {
            result.push(component);
        }
    }
    result.sort();
    result
}
fn dependency_order(
    adjacency: &BTreeMap<DerivationId, BTreeSet<DerivationId>>,
) -> Result<Vec<DerivationId>> {
    let mut counts: BTreeMap<_, _> = adjacency
        .iter()
        .map(|(id, deps)| (id.clone(), deps.len()))
        .collect();
    let mut reverse = BTreeMap::<DerivationId, BTreeSet<DerivationId>>::new();
    for (id, dependencies) in adjacency {
        for dependency in dependencies {
            reverse
                .entry(dependency.clone())
                .or_default()
                .insert(id.clone());
        }
    }
    let mut ready: BTreeSet<_> = counts
        .iter()
        .filter(|(_, n)| **n == 0)
        .map(|(id, _)| id.clone())
        .collect();
    let mut result = Vec::new();
    while let Some(id) = ready.pop_first() {
        result.push(id.clone());
        if let Some(children) = reverse.get(&id) {
            for child in children {
                let count = counts.get_mut(child).ok_or(GraphError::Corrupt)?;
                *count = count.checked_sub(1).ok_or(GraphError::Corrupt)?;
                if *count == 0 {
                    ready.insert(child.clone());
                }
            }
        }
    }
    ensure(
        result.len() == adjacency.len(),
        "cyclic production dependencies",
    )?;
    Ok(result)
}
