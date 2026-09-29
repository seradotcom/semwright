use crate::*;
use composition::{Digest, Owner, PrincipalBinding, canonical_digest};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
/// Constructed by trusted Broker code after grant filtering, never deserialized.
#[derive(Clone)]
pub struct ProjectAccess {
    pub(crate) owner: Owner,
    pub(crate) project: ProjectId,
    pub(crate) visible: Option<BTreeSet<LogicalAssetId>>,
    pub(crate) writable: bool,
    pub(crate) grants: Digest,
    pub(crate) visibility: Digest,
}
impl ProjectAccess {
    pub fn authorized(
        owner: Owner,
        project: ProjectId,
        visible: Option<BTreeSet<LogicalAssetId>>,
        writable: bool,
        grants: Digest,
    ) -> Result<Self> {
        owner.validate()?;
        ensure(
            matches!(owner.principal, PrincipalBinding::Named(_)),
            "durable principal required",
        )?;
        ensure(
            visible.as_ref().is_none_or(|v| v.len() <= MAX_RESOURCES),
            "visibility budget",
        )?;
        let mut scope = Vec::from(b"project-graph-visibility-v1:".as_slice());
        match &visible {
            None => scope.extend_from_slice(b"all"),
            Some(ids) => {
                scope.extend_from_slice(b"subset:");
                for id in ids {
                    scope.extend_from_slice(id.as_str().as_bytes());
                    scope.push(0);
                }
            }
        }
        let visibility = Digest::of_bytes(&scope);
        Ok(Self {
            owner,
            project,
            visibility,
            visible,
            writable,
            grants,
        })
    }
    pub(crate) fn sees(&self, id: &LogicalAssetId) -> bool {
        self.visible.as_ref().is_none_or(|v| v.contains(id))
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AssetView {
    pub asset: Asset,
    pub binding_generation: u64,
    pub latest_revision: Option<AssetRevision>,
    pub tombstoned: bool,
    pub knowledge: Knowledge,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AssetState {
    pub asset: Asset,
    pub binding_generation: u64,
    pub latest: Option<AssetRevision>,
    pub tombstoned: bool,
    pub probe: ProbeOutcome,
    pub producer: Option<ReceiptId>,
    pub gap: bool,
}
/// Canonical private log events are not public ingestion requests.
#[derive(Clone, Serialize, Deserialize)]
#[serde(
    tag = "event",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(crate) enum GraphEvent {
    Register(Asset),
    Rename {
        id: LogicalAssetId,
        label: String,
    },
    Rebind {
        id: LogicalAssetId,
        expected_generation: u64,
        locator: DurableLocator,
        reason: String,
    },
    Observe(RevisionRecord),
    Probe {
        id: LogicalAssetId,
        outcome: ProbeOutcome,
    },
    Receipt(Box<ExecutionReceipt>),
    Declare(Edge),
    Determinants(Vec<Determinant>),
    Gap(Vec<LogicalAssetId>),
    Tombstone(LogicalAssetId),
}
#[derive(Clone)]
pub struct ProjectGraph {
    pub(crate) project: ProjectId,
    pub(crate) principal: PrincipalBinding,
    pub(crate) sequence: u64,
    pub(crate) epoch: String,
    pub(crate) assets: BTreeMap<LogicalAssetId, AssetState>,
    pub(crate) revisions: BTreeMap<AssetRevision, RevisionRecord>,
    pub(crate) receipts: BTreeMap<ReceiptId, ExecutionReceipt>,
    pub(crate) edges: BTreeMap<Digest, Edge>,
    pub(crate) determinants: BTreeMap<(DependencyClass, String), Digest>,
    pub(crate) reverse: BTreeMap<LogicalAssetId, BTreeSet<LogicalAssetId>>,
    pub(crate) possible_reverse: BTreeMap<LogicalAssetId, BTreeSet<LogicalAssetId>>,
    pub(crate) seen: BTreeSet<LogicalAssetId>,
    pub(crate) determinants_seen: bool,
    pub(crate) pending: Vec<GraphEvent>,
}
impl ProjectGraph {
    pub fn new(project: ProjectId, principal: PrincipalBinding) -> Result<Self> {
        ensure(
            matches!(&principal, PrincipalBinding::Named(p) if !p.is_empty()),
            "persistent named owner required",
        )?;
        Ok(Self {
            project,
            principal,
            sequence: 0,
            epoch: uuid::Uuid::new_v4().to_string(),
            assets: BTreeMap::new(),
            revisions: BTreeMap::new(),
            receipts: BTreeMap::new(),
            edges: BTreeMap::new(),
            determinants: BTreeMap::new(),
            reverse: BTreeMap::new(),
            possible_reverse: BTreeMap::new(),
            seen: BTreeSet::new(),
            determinants_seen: false,
            pending: Vec::new(),
        })
    }
    pub fn project_id(&self) -> &ProjectId {
        &self.project
    }
    pub fn snapshot_revision(&self) -> u64 {
        self.sequence
    }
    pub fn restart_observation_epoch(&mut self) {
        self.epoch = uuid::Uuid::new_v4().to_string();
        self.seen.clear();
        self.determinants_seen = false;
    }
    pub(crate) fn access(&self, access: &ProjectAccess, write: bool) -> Result<()> {
        if access.project != self.project
            || access.owner.principal != self.principal
            || (write && !access.writable)
        {
            return Err(GraphError::Denied);
        }
        Ok(())
    }
    pub(crate) fn visible(
        &self,
        access: &ProjectAccess,
        id: &LogicalAssetId,
    ) -> Result<&AssetState> {
        self.access(access, false)?;
        if !access.sees(id) {
            return Err(GraphError::Denied);
        }
        self.assets.get(id).ok_or(GraphError::Denied)
    }
    pub fn register(&mut self, access: &ProjectAccess, asset: Asset) -> Result<()> {
        self.access(access, true)?;
        if !access.sees(&asset.id) {
            return Err(GraphError::Denied);
        }
        self.apply(GraphEvent::Register(asset), true)
    }
    pub fn rename(
        &mut self,
        access: &ProjectAccess,
        id: &LogicalAssetId,
        label: String,
    ) -> Result<()> {
        self.visible(access, id)?;
        self.access(access, true)?;
        self.apply(
            GraphEvent::Rename {
                id: id.clone(),
                label,
            },
            true,
        )
    }
    pub fn rebind(
        &mut self,
        access: &ProjectAccess,
        id: &LogicalAssetId,
        expected_generation: u64,
        locator: DurableLocator,
        reason: String,
    ) -> Result<()> {
        self.visible(access, id)?;
        self.access(access, true)?;
        self.apply(
            GraphEvent::Rebind {
                id: id.clone(),
                expected_generation,
                locator,
                reason,
            },
            true,
        )
    }
    /// Host-only observation ingress: callers cannot expose this as an arbitrary JSON route.
    pub fn observe(&mut self, access: &ProjectAccess, record: RevisionRecord) -> Result<()> {
        self.visible(access, &record.pin.asset)?;
        self.access(access, true)?;
        self.apply(GraphEvent::Observe(record), true)
    }
    pub fn record_probe(
        &mut self,
        access: &ProjectAccess,
        id: &LogicalAssetId,
        outcome: ProbeOutcome,
    ) -> Result<()> {
        self.visible(access, id)?;
        self.access(access, true)?;
        self.apply(
            GraphEvent::Probe {
                id: id.clone(),
                outcome,
            },
            true,
        )
    }
    pub fn accept_receipt(
        &mut self,
        access: &ProjectAccess,
        receipt: AdmittedReceipt,
    ) -> Result<()> {
        self.access(access, true)?;
        if receipt.0.owner != access.owner {
            return Err(GraphError::Denied);
        }
        for p in receipt.0.inputs.iter().chain(&receipt.0.outputs) {
            self.visible(access, &p.asset)?;
        }
        self.apply(GraphEvent::Receipt(Box::new(receipt.0)), true)
    }
    pub fn declare(&mut self, access: &ProjectAccess, edge: Edge) -> Result<()> {
        self.access(access, true)?;
        for v in [&edge.from, &edge.to] {
            match v {
                Vertex::Asset(id) => {
                    self.visible(access, id)?;
                }
                _ => {
                    return Err(GraphError::Invalid(
                        "declared edge endpoints must be assets",
                    ));
                }
            }
        }
        ensure(
            matches!(edge.evidence, EdgeEvidence::Declared { .. })
                && !matches!(
                    edge.relation,
                    Relation::VerifiedBy | Relation::ProducedBy | Relation::ConsumedBy
                ),
            "declaration cannot certify execution",
        )?;
        self.apply(GraphEvent::Declare(edge), true)
    }
    pub fn observe_determinants(
        &mut self,
        access: &ProjectAccess,
        determinants: Vec<Determinant>,
    ) -> Result<()> {
        self.access(access, true)?;
        // This operation replaces a project-wide observed set. A subset grant
        // cannot overwrite determining inputs of assets it cannot inspect.
        if access.visible.is_some() {
            return Err(GraphError::Denied);
        }
        self.apply(GraphEvent::Determinants(determinants), true)
    }
    pub fn invalidate_scope(
        &mut self,
        access: &ProjectAccess,
        ids: Vec<LogicalAssetId>,
    ) -> Result<()> {
        self.access(access, true)?;
        for id in &ids {
            self.visible(access, id)?;
        }
        self.apply(GraphEvent::Gap(ids), true)
    }
    pub fn tombstone(&mut self, access: &ProjectAccess, id: &LogicalAssetId) -> Result<()> {
        self.visible(access, id)?;
        self.access(access, true)?;
        self.apply(GraphEvent::Tombstone(id.clone()), true)
    }
    pub(crate) fn apply(&mut self, event: GraphEvent, live: bool) -> Result<()> {
        composition::canonical_bytes(&event)?;
        let next = self
            .sequence
            .checked_add(1)
            .ok_or(GraphError::Limit("graph revision"))?;
        match &event {
            GraphEvent::Register(asset) => {
                asset.validate()?;
                ensure(
                    !self.assets.contains_key(&asset.id),
                    "asset identity already registered",
                )?;
                ensure(self.assets.len() < MAX_RESOURCES, "asset limit")?;
                self.assets.insert(
                    asset.id.clone(),
                    AssetState {
                        asset: asset.clone(),
                        binding_generation: 1,
                        latest: None,
                        tombstoned: false,
                        probe: ProbeOutcome::Ambiguous,
                        producer: None,
                        gap: true,
                    },
                );
            }
            GraphEvent::Rename { id, label } => {
                name(label)?;
                self.assets
                    .get_mut(id)
                    .ok_or(GraphError::Denied)?
                    .asset
                    .label = label.clone();
            }
            GraphEvent::Rebind {
                id,
                expected_generation,
                locator,
                reason,
            } => {
                locator.validate()?;
                name(reason)?;
                let s = self.assets.get_mut(id).ok_or(GraphError::Denied)?;
                if s.binding_generation != *expected_generation {
                    return Err(GraphError::Conflict);
                }
                let generation = s
                    .binding_generation
                    .checked_add(1)
                    .ok_or(GraphError::Limit("binding generation"))?;
                s.asset.locator = Some(locator.clone());
                s.binding_generation = generation;
                s.latest = None;
                s.producer = None;
                s.probe = ProbeOutcome::Ambiguous;
                s.gap = true;
                self.seen.remove(id);
            }
            GraphEvent::Observe(record) => {
                record.validate()?;
                let s = self
                    .assets
                    .get(&record.pin.asset)
                    .ok_or(GraphError::Denied)?;
                if s.binding_generation != record.binding_generation || s.tombstoned {
                    return Err(GraphError::Conflict);
                }
                if let Some(old) = self.revisions.get(&record.pin.revision) {
                    if canonical_digest(old)? == canonical_digest(record)? {
                        return Ok(());
                    }
                    return Err(GraphError::Conflict);
                }
                ensure(self.revisions.len() < 100_000, "revision history limit")?;
                if let Some(old) = s.latest.as_ref().and_then(|r| self.revisions.get(r)) {
                    if record.observed_unix_ms < old.observed_unix_ms {
                        return Err(GraphError::Conflict);
                    }
                }
                let id = record.pin.asset.clone();
                self.revisions
                    .insert(record.pin.revision.clone(), record.clone());
                let s = self.assets.get_mut(&id).ok_or(GraphError::Denied)?;
                s.latest = Some(record.pin.revision.clone());
                s.probe = ProbeOutcome::Present;
                s.gap = false;
                if live {
                    self.seen.insert(id);
                }
            }
            GraphEvent::Probe { id, outcome } => {
                let s = self.assets.get_mut(id).ok_or(GraphError::Denied)?;
                ensure(
                    *outcome != ProbeOutcome::Present,
                    "present needs an actual observation",
                )?;
                s.probe = *outcome;
                s.gap = !matches!(outcome, ProbeOutcome::ConclusiveNotFound);
                if live {
                    self.seen.insert(id.clone());
                }
            }
            GraphEvent::Receipt(receipt) => {
                if !self.insert_receipt(receipt)? {
                    return Ok(());
                }
            }
            GraphEvent::Declare(edge) => {
                ensure(
                    matches!(edge.evidence, EdgeEvidence::Declared { .. })
                        && !matches!(
                            edge.relation,
                            Relation::VerifiedBy | Relation::ProducedBy | Relation::ConsumedBy
                        ),
                    "declaration cannot certify execution",
                )?;
                let (Vertex::Asset(from), Vertex::Asset(to)) = (&edge.from, &edge.to) else {
                    return Err(GraphError::Invalid("declaration endpoints"));
                };
                ensure(
                    self.assets.contains_key(from) && self.assets.contains_key(to),
                    "unknown edge endpoint",
                )?;
                if edge.relation == Relation::Contains
                    && (from == to || self.contains_path(to, from))
                {
                    return Err(GraphError::Conflict);
                }
                let digest = canonical_digest(edge)?;
                if self.edges.contains_key(&digest) {
                    return Ok(());
                }
                ensure(self.edges.len() < MAX_EDGES, "edge limit")?;
                self.edges.insert(digest, edge.clone());
                if matches!(
                    edge.relation,
                    Relation::References
                        | Relation::DerivedFrom
                        | Relation::Realizes
                        | Relation::PublishedAs
                ) {
                    self.possible_reverse
                        .entry(to.clone())
                        .or_default()
                        .insert(from.clone());
                }
            }
            GraphEvent::Determinants(values) => {
                ensure(values.len() <= MAX_DEPENDENCIES, "determinant budget")?;
                let mut keys = BTreeSet::new();
                for v in values {
                    name(&v.key)?;
                    ensure(
                        keys.insert((v.class, v.key.clone())),
                        "duplicate determinant",
                    )?;
                }
                self.determinants = values
                    .iter()
                    .map(|d| ((d.class, d.key.clone()), d.digest.clone()))
                    .collect();
                self.determinants_seen = live;
            }
            GraphEvent::Gap(ids) => {
                ensure(ids.len() <= MAX_RESOURCES, "gap scope limit")?;
                ensure(
                    ids.iter().all(|id| self.assets.contains_key(id)),
                    "gap endpoint",
                )?;
                for id in ids {
                    self.assets.get_mut(id).ok_or(GraphError::Denied)?.gap = true;
                    self.seen.remove(id);
                }
            }
            GraphEvent::Tombstone(id) => {
                let s = self.assets.get_mut(id).ok_or(GraphError::Denied)?;
                s.tombstoned = true;
                s.gap = true;
                self.seen.remove(id);
            }
        }
        self.sequence = next;
        self.pending.push(event);
        Ok(())
    }
    fn contains_path(&self, start: &LogicalAssetId, target: &LogicalAssetId) -> bool {
        let mut pending = vec![start.clone()];
        let mut visited = BTreeSet::new();
        while let Some(id) = pending.pop() {
            if id == *target {
                return true;
            }
            if !visited.insert(id.clone()) {
                continue;
            }
            for e in self.edges.values() {
                if e.relation == Relation::Contains && e.from == Vertex::Asset(id.clone()) {
                    if let Vertex::Asset(next) = &e.to {
                        pending.push(next.clone());
                    }
                }
            }
        }
        false
    }
    fn insert_receipt(&mut self, r: &ExecutionReceipt) -> Result<bool> {
        r.validate()?;
        if r.project != self.project || r.owner.principal != self.principal {
            return Err(GraphError::Denied);
        }
        if let Some(old) = self.receipts.get(&r.id) {
            if canonical_digest(old)? == canonical_digest(r)? {
                return Ok(false);
            }
            return Err(GraphError::Conflict);
        }
        ensure(self.receipts.len() < 20_000, "receipt history limit")?;
        ensure(
            !self.receipts.values().any(|old| {
                old.derivation == r.derivation
                    || (old.owner == r.owner && old.request_id == r.request_id)
            }),
            "execution identity collision",
        )?;
        for pin in r.inputs.iter().chain(&r.outputs) {
            let stored = self
                .revisions
                .get(&pin.revision)
                .ok_or(GraphError::Conflict)?;
            ensure(
                &stored.pin == pin,
                "receipt substitutes revision or fingerprint",
            )?;
        }
        let completed = r.verification.execution_status == composition::ExecutionStatus::Completed;
        let needed = r
            .inputs
            .len()
            .checked_mul(r.outputs.len())
            .and_then(|n| n.checked_add(r.inputs.len() + r.outputs.len()))
            .ok_or(GraphError::Limit("receipt edge count"))?;
        ensure(
            !completed || needed + self.edges.len() <= MAX_EDGES,
            "receipt edge budget",
        )?;
        let mut edges = Vec::new();
        if completed {
            for input in &r.inputs {
                edges.push(Edge {
                    from: Vertex::Asset(input.asset.clone()),
                    to: Vertex::Activity(r.derivation.clone()),
                    relation: Relation::ConsumedBy,
                    evidence: EdgeEvidence::Executed {
                        receipt: r.id.clone(),
                    },
                });
            }
            for output in &r.outputs {
                edges.push(Edge {
                    from: Vertex::Asset(output.asset.clone()),
                    to: Vertex::Activity(r.derivation.clone()),
                    relation: Relation::ProducedBy,
                    evidence: EdgeEvidence::Executed {
                        receipt: r.id.clone(),
                    },
                });
                for input in &r.inputs {
                    edges.push(Edge {
                        from: Vertex::Asset(output.asset.clone()),
                        to: Vertex::Asset(input.asset.clone()),
                        relation: Relation::DerivedFrom,
                        evidence: EdgeEvidence::Executed {
                            receipt: r.id.clone(),
                        },
                    });
                }
            }
        }
        ensure(
            edges
                .len()
                .checked_add(self.edges.len())
                .is_some_and(|n| n <= MAX_EDGES),
            "receipt edge budget",
        )?;
        let edges = edges
            .into_iter()
            .map(|edge| Ok((canonical_digest(&edge)?, edge)))
            .collect::<Result<Vec<_>>>()?;
        for (digest, edge) in edges {
            self.edges.insert(digest, edge);
        }
        if completed {
            for output in &r.outputs {
                if let Some(s) = self.assets.get_mut(&output.asset) {
                    if s.latest.as_ref() == Some(&output.revision) {
                        s.producer = Some(r.id.clone());
                    }
                }
                for input in &r.inputs {
                    self.reverse
                        .entry(input.asset.clone())
                        .or_default()
                        .insert(output.asset.clone());
                }
            }
        }
        self.receipts.insert(r.id.clone(), r.clone());
        Ok(true)
    }
    pub fn inspect(&self, access: &ProjectAccess, id: &LogicalAssetId) -> Result<AssetView> {
        let s = self.visible(access, id)?;
        let mut seen = BTreeSet::new();
        let mut remaining = 10_000;
        Ok(AssetView {
            asset: s.asset.clone(),
            binding_generation: s.binding_generation,
            latest_revision: s.latest.clone(),
            tombstoned: s.tombstoned,
            knowledge: self.knowledge(access, id, &mut seen, &mut remaining, 0)?,
        })
    }
    fn knowledge(
        &self,
        access: &ProjectAccess,
        id: &LogicalAssetId,
        visiting: &mut BTreeSet<LogicalAssetId>,
        remaining: &mut usize,
        depth: usize,
    ) -> Result<Knowledge> {
        let s = self.visible(access, id)?;
        if *remaining == 0 || depth >= 64 || !visiting.insert(id.clone()) {
            return Ok(Knowledge::unknown());
        }
        *remaining -= 1;
        let mut k = Knowledge::unknown();
        k.existence = s.probe.existence();
        if s.tombstoned || s.gap || !self.seen.contains(id) {
            k.existence = Existence::Unknown;
            visiting.remove(id);
            return Ok(k);
        }
        k.requires_reconcile = false;
        let Some(observed) = s.latest.as_ref().and_then(|r| self.revisions.get(r)) else {
            visiting.remove(id);
            return Ok(k);
        };
        k.coverage = observed.coverage.clone();
        k.observed_unix_ms = Some(observed.observed_unix_ms);
        let real = !matches!(
            observed.observation.source,
            composition::EvidenceSource::Fixture | composition::EvidenceSource::Simulation
        );
        if k.existence != Existence::Present || !real {
            if !real {
                k.existence = Existence::Unknown;
                k.requires_reconcile = true;
            }
            visiting.remove(id);
            return Ok(k);
        }
        k.freshness = if observed.observation.exhaustive
            && observed
                .pin
                .fingerprint
                .equivalent(&observed.pin.fingerprint, observed.pin.equivalence)
                == Some(true)
        {
            Freshness::Current
        } else {
            Freshness::Unknown
        };
        if let Some(receipt) = s.producer.as_ref().and_then(|r| self.receipts.get(r)) {
            k.verification = receipt.verification.verdict()?;
            k.coverage.complete &= receipt.coverage.complete;
            k.coverage
                .unknown_frontier
                .extend(receipt.coverage.unknown_frontier.iter());
            let expected_output = receipt
                .outputs
                .iter()
                .find(|p| p.asset == *id)
                .ok_or(GraphError::Corrupt)?;
            k.divergence = match expected_output
                .fingerprint
                .equivalent(&observed.pin.fingerprint, expected_output.equivalence)
            {
                Some(true) => Divergence::Clean,
                Some(false) => Divergence::Diverged,
                None => Divergence::Unknown,
            };
            let mut freshness = if k.coverage.cache_safe() && self.determinants_seen {
                Freshness::Current
            } else {
                Freshness::Unknown
            };
            for d in &receipt.required_determinants() {
                let observed = self
                    .determinants_seen
                    .then(|| self.determinants.get(&(d.class, d.key.clone())))
                    .flatten();
                match observed {
                    Some(now) if now != &d.digest => freshness = Freshness::Stale,
                    None => {
                        if freshness != Freshness::Stale {
                            freshness = Freshness::Unknown;
                        }
                        k.coverage.complete = false;
                        k.coverage.unknown_frontier.insert(d.class);
                    }
                    _ => (),
                }
            }
            for input in &receipt.inputs {
                if !access.sees(&input.asset) {
                    if freshness != Freshness::Stale {
                        freshness = Freshness::Unknown;
                    }
                    k.coverage.complete = false;
                    k.coverage
                        .unknown_frontier
                        .insert(DependencyClass::External);
                    continue;
                }
                let state = self.knowledge(access, &input.asset, visiting, remaining, depth + 1)?;
                let current = self
                    .assets
                    .get(&input.asset)
                    .and_then(|a| a.latest.as_ref())
                    .and_then(|r| self.revisions.get(r));
                let comparison = current.and_then(|r| {
                    input
                        .fingerprint
                        .equivalent(&r.pin.fingerprint, input.equivalence)
                });
                if state.freshness == Freshness::Stale
                    || (state.existence == Existence::Missing && !state.requires_reconcile)
                    || (comparison == Some(false) && !state.requires_reconcile)
                {
                    freshness = Freshness::Stale;
                } else if (state.freshness != Freshness::Current
                    || state.existence != Existence::Present
                    || state.divergence == Divergence::Diverged
                    || state.requires_reconcile
                    || comparison != Some(true))
                    && freshness != Freshness::Stale
                {
                    freshness = Freshness::Unknown;
                }
                k.coverage.complete &= state.coverage.complete;
                k.coverage
                    .unknown_frontier
                    .extend(state.coverage.unknown_frontier);
            }
            if !k.coverage.cache_safe() && freshness == Freshness::Current {
                freshness = Freshness::Unknown;
            }
            k.freshness = freshness;
        }
        visiting.remove(id);
        Ok(k)
    }
    pub fn revisions(
        &self,
        access: &ProjectAccess,
        id: &LogicalAssetId,
        after: Option<&AssetRevision>,
        limit: usize,
    ) -> Result<Vec<RevisionRecord>> {
        self.visible(access, id)?;
        if access.visible.is_some() {
            return Err(GraphError::Denied);
        }
        ensure((1..=256).contains(&limit), "revision page limit")?;
        Ok(self
            .revisions
            .values()
            .filter(|r| r.pin.asset == *id && after.is_none_or(|a| r.pin.revision > *a))
            .take(limit)
            .cloned()
            .collect())
    }
    pub fn receipt(&self, access: &ProjectAccess, id: &ReceiptId) -> Result<ExecutionReceipt> {
        self.access(access, false)?;
        if access.visible.is_some() {
            return Err(GraphError::Denied);
        }
        let r = self.receipts.get(id).ok_or(GraphError::Denied)?;
        for pin in r.inputs.iter().chain(&r.outputs) {
            self.visible(access, &pin.asset)?;
        }
        Ok(r.clone())
    }
}
