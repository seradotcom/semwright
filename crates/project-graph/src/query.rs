use crate::*;
use composition::{Digest, Owner, canonical_digest};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TraversalBudget {
    pub nodes: usize,
    pub edges: usize,
    pub depth: usize,
    pub results: usize,
}
impl TraversalBudget {
    pub fn validate(&self) -> Result<()> {
        ensure(
            (1..=MAX_RESOURCES).contains(&self.nodes)
                && (1..=MAX_EDGES).contains(&self.edges)
                && (1..=256).contains(&self.depth)
                && (1..=10_000).contains(&self.results),
            "traversal budget",
        )
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImpactHit {
    pub asset: LogicalAssetId,
    pub via: LogicalAssetId,
    pub depth: usize,
    pub receipt: Option<ReceiptId>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImpactReport {
    pub snapshot: u64,
    pub known: Vec<ImpactHit>,
    pub possible: Vec<ImpactHit>,
    pub unknown_frontier: bool,
    pub truncated: bool,
    pub cancelled: bool,
    pub visited_nodes: usize,
    pub visited_edges: usize,
}
/// Live cancellation probe, without spawning another scheduler or watcher.
pub trait CancellationCheck {
    fn cancelled(&self) -> bool;
}
impl CancellationCheck for AtomicBool {
    fn cancelled(&self) -> bool {
        self.load(Ordering::Relaxed)
    }
}
impl ProjectGraph {
    pub fn impact<C: CancellationCheck + ?Sized>(
        &self,
        access: &ProjectAccess,
        source: &LogicalAssetId,
        budget: TraversalBudget,
        cancellation: &C,
    ) -> Result<ImpactReport> {
        self.visible(access, source)?;
        budget.validate()?;
        let mut report = ImpactReport {
            snapshot: self.sequence,
            known: Vec::new(),
            possible: Vec::new(),
            unknown_frontier: access.visible.is_some(),
            truncated: false,
            cancelled: false,
            visited_nodes: 0,
            visited_edges: 0,
        };
        let mut queue = VecDeque::from([(source.clone(), 0usize, true)]);
        let mut visited = BTreeSet::new();
        let mut scheduled = BTreeSet::from([(source.clone(), true)]);
        while let Some((id, depth, definite)) = queue.pop_front() {
            if cancellation.cancelled() {
                report.cancelled = true;
                report.truncated = true;
                break;
            }
            if !visited.insert((id.clone(), definite)) {
                continue;
            }
            if report.visited_nodes >= budget.nodes {
                report.truncated = true;
                break;
            }
            report.visited_nodes += 1;
            let state = self.visible(access, &id)?;
            if state
                .latest
                .as_ref()
                .and_then(|r| self.revisions.get(r))
                .is_none_or(|r| !r.coverage.cache_safe())
            {
                report.unknown_frontier = true;
            }
            let mut targets = BTreeMap::<LogicalAssetId, Option<ReceiptId>>::new();
            if let Some(items) = self.reverse.get(&id) {
                for target in items {
                    if !access.sees(target) {
                        continue;
                    }
                    let r = self
                        .assets
                        .get(target)
                        .and_then(|s| s.producer.as_ref())
                        .and_then(|r| self.receipts.get(r));
                    if let Some(r) = r {
                        if r.inputs.iter().any(|p| p.asset == id) {
                            targets.insert(target.clone(), Some(r.id.clone()));
                        }
                    }
                }
            }
            if let Some(items) = self.possible_reverse.get(&id) {
                for target in items {
                    if access.sees(target) {
                        targets.entry(target.clone()).or_insert(None);
                    }
                }
            }
            if depth >= budget.depth {
                report.truncated |= !targets.is_empty();
                continue;
            }
            for (target, receipt) in targets {
                if cancellation.cancelled() {
                    report.cancelled = true;
                    report.truncated = true;
                    break;
                }
                if report.visited_edges >= budget.edges {
                    report.truncated = true;
                    break;
                }
                report.visited_edges += 1;
                let known = definite && receipt.is_some();
                if scheduled.insert((target.clone(), known)) {
                    if target != *source
                        && report.known.len() + report.possible.len() >= budget.results
                    {
                        report.truncated = true;
                        break;
                    }
                    let hit = ImpactHit {
                        asset: target.clone(),
                        via: id.clone(),
                        depth: depth + 1,
                        receipt,
                    };
                    if target != *source {
                        if known {
                            report.known.push(hit);
                        } else {
                            report.possible.push(hit);
                        }
                    }
                    queue.push_back((target, depth + 1, known));
                }
            }
            if report.cancelled
                || (report.truncated
                    && (report.visited_edges >= budget.edges
                        || report.known.len() + report.possible.len() >= budget.results))
            {
                break;
            }
        }
        let mut known_ids = BTreeSet::new();
        report
            .known
            .retain(|h| h.asset != *source && known_ids.insert(h.asset.clone()));
        let mut possible_ids = BTreeSet::new();
        report.possible.retain(|h| {
            h.asset != *source
                && !known_ids.contains(&h.asset)
                && possible_ids.insert(h.asset.clone())
        });
        report.unknown_frontier |= report.truncated;
        Ok(report)
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct AssetQuery {
    pub resource_type: Option<String>,
    pub status: Option<String>,
    pub include_tombstones: bool,
}
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct QueryPage {
    pub snapshot: u64,
    pub items: Vec<AssetView>,
    pub next_cursor: Option<String>,
    pub scope_partial: bool,
    pub truncated: bool,
}
struct Cursor {
    owner: Owner,
    project: ProjectId,
    epoch: String,
    sequence: u64,
    query: Digest,
    grants: Digest,
    visibility: Digest,
    after: LogicalAssetId,
    created: std::time::Instant,
}
#[derive(Default)]
pub struct QueryCursors {
    entries: BTreeMap<String, Cursor>,
}
impl QueryCursors {
    pub fn revoke_session(&mut self, session: &str) {
        self.entries.retain(|_, c| c.owner.session != session);
    }
    pub fn page(
        &mut self,
        graph: &ProjectGraph,
        access: &ProjectAccess,
        query: &AssetQuery,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<QueryPage> {
        graph.access(access, false)?;
        ensure((1..=256).contains(&limit), "query page size")?;
        if let Some(kind) = &query.resource_type {
            name(kind)?;
        }
        if let Some(status) = &query.status {
            ensure(
                matches!(
                    status.as_str(),
                    "CURRENT" | "STALE" | "UNKNOWN" | "MISSING" | "DIVERGED"
                ),
                "query status",
            )?;
        }
        self.entries
            .retain(|_, c| c.created.elapsed().as_secs() <= 300);
        let digest = canonical_digest(query)?;
        let after = if let Some(token) = cursor {
            let c = self.entries.get(token).ok_or(GraphError::Denied)?;
            if c.owner != access.owner
                || c.project != access.project
                || c.grants != access.grants
                || c.visibility != access.visibility
                || c.query != digest
            {
                return Err(GraphError::Denied);
            }
            if c.epoch != graph.epoch || c.sequence != graph.sequence {
                return Err(GraphError::Conflict);
            }
            Some(c.after.clone())
        } else {
            None
        };
        let mut items = Vec::new();
        let mut last = None;
        let mut more = false;
        let mut scanned = 0;
        for (id, state) in &graph.assets {
            if !access.sees(id) || after.as_ref().is_some_and(|a| id <= a) {
                continue;
            }
            if items.len() >= limit || scanned >= 2048 {
                more = true;
                break;
            }
            scanned += 1;
            last = Some(id.clone());
            if (!query.include_tombstones && state.tombstoned)
                || query
                    .resource_type
                    .as_ref()
                    .is_some_and(|k| k != &state.asset.resource_type)
            {
                continue;
            }
            let view = graph.inspect(access, id)?;
            if query
                .status
                .as_ref()
                .is_none_or(|s| s == view.knowledge.label())
            {
                items.push(view);
            }
        }
        let next_cursor = if more {
            ensure(
                self.entries.len() < 512,
                "cursor capacity; consume or revoke existing queries",
            )?;
            let token = uuid::Uuid::new_v4().to_string();
            self.entries.insert(
                token.clone(),
                Cursor {
                    owner: access.owner.clone(),
                    project: access.project.clone(),
                    epoch: graph.epoch.clone(),
                    sequence: graph.sequence,
                    query: digest,
                    grants: access.grants.clone(),
                    visibility: access.visibility.clone(),
                    after: last.ok_or(GraphError::Corrupt)?,
                    created: std::time::Instant::now(),
                },
            );
            Some(token)
        } else {
            None
        };
        Ok(QueryPage {
            snapshot: graph.sequence,
            items,
            next_cursor,
            scope_partial: access.visible.is_some(),
            truncated: more && scanned >= 2048,
        })
    }
}
