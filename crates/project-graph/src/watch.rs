//! Opt-in observation stream bookkeeping. Notifications never authorize filesystem reads.
use crate::*;
use composition::{Digest, Owner, canonical_digest};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum WatchHint {
    Changed(Vec<LogicalAssetId>),
    Overflow,
    ProviderOffline,
}
#[derive(Debug, Clone, Serialize)]
pub struct WatchUpdate {
    pub duplicate: bool,
    pub requires_rescan: bool,
    pub scope: Vec<LogicalAssetId>,
}
/// A host-only registration, not a serialized watch capability or background task.
pub struct ScopedObserver {
    owner: Owner,
    project: ProjectId,
    grants: Digest,
    scope: BTreeSet<LogicalAssetId>,
    last: Option<(u64, Digest)>,
    rescan: bool,
}
impl ScopedObserver {
    pub fn register(
        graph: &ProjectGraph,
        access: &ProjectAccess,
        scope: BTreeSet<LogicalAssetId>,
    ) -> Result<Self> {
        graph.access(access, true)?;
        ensure(
            !scope.is_empty() && scope.len() <= 1024,
            "watch scope budget",
        )?;
        for id in &scope {
            graph.visible(access, id)?;
        }
        Ok(Self {
            owner: access.owner.clone(),
            project: access.project.clone(),
            grants: access.grants.clone(),
            scope,
            last: None,
            rescan: true,
        })
    }
    fn check(&self, graph: &ProjectGraph, access: &ProjectAccess) -> Result<()> {
        graph.access(access, true)?;
        if access.owner != self.owner
            || access.project != self.project
            || access.grants != self.grants
        {
            return Err(GraphError::Denied);
        }
        for id in &self.scope {
            graph.visible(access, id)?;
        }
        Ok(())
    }
    pub fn event(
        &mut self,
        graph: &mut ProjectGraph,
        access: &ProjectAccess,
        sequence: u64,
        hint: WatchHint,
    ) -> Result<WatchUpdate> {
        self.check(graph, access)?;
        ensure(sequence > 0, "watch sequence")?;
        let digest = canonical_digest(&hint)?;
        if self
            .last
            .as_ref()
            .is_some_and(|(n, hash)| *n == sequence && *hash == digest)
        {
            return Ok(WatchUpdate {
                duplicate: true,
                requires_rescan: self.rescan,
                scope: Vec::new(),
            });
        }
        let lost = self
            .last
            .as_ref()
            .is_none_or(|(n, _)| n.checked_add(1) != Some(sequence));
        let affected: Vec<_> = match &hint {
            WatchHint::Changed(ids) if !lost => {
                ensure(
                    ids.len() <= 1024 && ids.iter().all(|id| self.scope.contains(id)),
                    "watch hint outside registered scope",
                )?;
                ids.clone()
            }
            _ => self.scope.iter().cloned().collect(),
        };
        graph.invalidate_scope(access, affected.clone())?;
        self.rescan = true;
        self.last = Some((sequence, digest));
        Ok(WatchUpdate {
            duplicate: false,
            requires_rescan: true,
            scope: affected,
        })
    }
    /// The rescan marker can clear only after each member has actual new trusted
    /// observations/probes in the graph; passing a list of claimed IDs is insufficient.
    pub fn finish_rescan(&mut self, graph: &ProjectGraph, access: &ProjectAccess) -> Result<()> {
        self.check(graph, access)?;
        if self
            .scope
            .iter()
            .any(|id| !graph.seen.contains(id) || graph.assets.get(id).is_none_or(|s| s.gap))
        {
            return Err(GraphError::Conflict);
        }
        self.rescan = false;
        Ok(())
    }
}
