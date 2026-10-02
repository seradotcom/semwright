use crate::*;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Instant,
};

fn expired(elapsed_ms: u128, max_elapsed_ms: u64) -> bool {
    elapsed_ms > u128::from(max_elapsed_ms)
}

#[derive(Debug, Clone, Serialize)]
pub struct Attempt {
    pub request_id: String,
    pub plan_digest: String,
    pub status: ExecutionStatus,
    pub effects: Vec<String>,
}
#[derive(Debug)]
struct VaultIdentity;
#[derive(Debug)]
struct RootIdentity;

struct Root {
    identity: Arc<RootIdentity>,
    budget: ConvergenceBudget,
    started: Instant,
    operations: u32,
    iterations: u32,
    observations: u32,
    attempts: Vec<Attempt>,
    reconciliation_requests: BTreeSet<String>,
    reconciliations: Vec<ReconciliationRecord>,
}
struct Entry {
    root: String,
    canonical: Vec<u8>,
    operations: u32,
    consumed: bool,
}
/// Evidence of an explicitly observed uncertain attempt. Its original status and
/// effects remain in the attempt ledger; this record never promotes them to PASS.
#[derive(Debug, Clone, Serialize)]
pub struct ReconciliationRecord {
    pub attempt_index: usize,
    pub request_id: String,
    pub observation: ObservationRef,
    pub child_plan_id: Option<String>,
}

/// Read-only reconciliation ticket, bound to one vault/root/attempt incarnation.
/// Only a trusted profile may use it with newly collected native evidence.
/// It grants no Broker permission and is never deserializable from request JSON.
pub struct ReconciliationPermit {
    vault_identity: Arc<VaultIdentity>,
    root_identity: Arc<RootIdentity>,
    owner: Owner,
    root: String,
    index: usize,
    plan_id: String,
    request_id: String,
    observation_sequence: u32,
    expected_base: BaseStateSet,
    scope: BTreeSet<Address>,
}

/// Private, non-serializable permit. Only begin() creates one, and finish consumes it.
/// This prevents replay, not policy bypass: caller must still use the Broker.
pub struct BeginPermit {
    vault_identity: Arc<VaultIdentity>,
    root_identity: Arc<RootIdentity>,
    owner: Owner,
    root: String,
    index: usize,
    digest: String,
}
pub struct PlanVault {
    identity: Arc<VaultIdentity>,
    entries: BTreeMap<(Owner, String), Entry>,
    roots: BTreeMap<(Owner, String), Root>,
    max_plans: usize,
    max_roots: usize,
    max_attempts: usize,
}
impl PlanVault {
    pub fn bounded(max_plans: usize, max_roots: usize, max_attempts: usize) -> Self {
        Self {
            identity: Arc::new(VaultIdentity),
            entries: BTreeMap::new(),
            roots: BTreeMap::new(),
            max_plans: max_plans.clamp(1, 256),
            max_roots: max_roots.clamp(1, 64),
            max_attempts: max_attempts.clamp(1, 4096),
        }
    }
    fn reap(&mut self) {
        self.roots
            .retain(|_, r| !expired(r.started.elapsed().as_millis(), r.budget.max_elapsed_ms));
        self.entries
            .retain(|(o, _), e| self.roots.contains_key(&(o.clone(), e.root.clone())));
    }
    /// Only call after the trusted planner has validated the domain payload.
    /// `id` is an index, not an authentication token. Full canonical bytes are bound.
    #[allow(clippy::too_many_arguments)]
    pub fn issue<T: Serialize>(
        &mut self,
        owner: &Owner,
        id: &str,
        plan: &T,
        budget: ConvergenceBudget,
        operations: u32,
        parent: Option<&str>,
        repair: bool,
    ) -> Result<()> {
        self.reap();
        owner.validate()?;
        bounded_id(id)?;
        budget.validate()?;
        ensure(
            operations <= budget.max_operations,
            "plan exceeds operation budget",
        )?;
        let canonical = canonical_bytes(plan)?;
        let key = (owner.clone(), id.to_owned());
        if let Some(e) = self.entries.get(&key) {
            return ensure(e.canonical == canonical, "plan ID collision");
        }
        if self.entries.len() >= self.max_plans {
            return Err(ContractError::Limit("server plan capacity".into()));
        }
        let root = if let Some(parent) = parent {
            ensure(repair, "child plan must be an explicit repair")?;
            let e = self
                .entries
                .get(&(owner.clone(), parent.into()))
                .ok_or_else(|| ContractError::Denied("unknown parent plan".into()))?;
            let r = self
                .roots
                .get_mut(&(owner.clone(), e.root.clone()))
                .ok_or_else(|| ContractError::Stale("expired root".into()))?;
            ensure(r.budget == budget, "repair cannot reset or enlarge budget")?;
            ensure(
                r.attempts.last().is_some_and(|a| a.plan_digest == parent),
                "repair parent is not the latest attempted plan",
            )?;
            if r.attempts
                .last()
                .is_none_or(|a| a.status != ExecutionStatus::Completed)
            {
                let index = r.attempts.len() - 1;
                let reconciled = r.reconciliations.last_mut().ok_or_else(|| {
                    ContractError::Unknown("repair needs explicit outcome reconciliation".into())
                })?;
                ensure(
                    reconciled.attempt_index == index && reconciled.child_plan_id.is_none(),
                    "reconciliation already consumed or belongs to another attempt",
                )?;
                reconciled.child_plan_id = Some(id.to_owned());
            }
            e.root.clone()
        } else {
            ensure(!repair, "repair needs a root")?;
            if self.roots.len() >= self.max_roots {
                return Err(ContractError::Limit("server root capacity".into()));
            }
            self.roots.insert(
                key.clone(),
                Root {
                    identity: Arc::new(RootIdentity),
                    budget,
                    started: Instant::now(),
                    operations: 0,
                    iterations: 0,
                    observations: 0,
                    attempts: vec![],
                    reconciliation_requests: BTreeSet::new(),
                    reconciliations: vec![],
                },
            );
            id.into()
        };
        self.entries.insert(
            key,
            Entry {
                root,
                canonical,
                operations,
                consumed: false,
            },
        );
        Ok(())
    }
    pub fn matches<T: Serialize>(&self, owner: &Owner, id: &str, plan: &T) -> Result<()> {
        owner.validate()?;
        let e = self
            .entries
            .get(&(owner.clone(), id.into()))
            .ok_or_else(|| ContractError::Denied("plan not issued to this host session".into()))?;
        let r = self
            .roots
            .get(&(owner.clone(), e.root.clone()))
            .ok_or_else(|| ContractError::Stale("missing plan root".into()))?;
        if expired(r.started.elapsed().as_millis(), r.budget.max_elapsed_ms) {
            return Err(ContractError::Stale("plan expired".into()));
        }
        if e.canonical != canonical_bytes(plan)? {
            return Err(ContractError::Denied(
                "plan bytes changed, even if a client recomputed the digest".into(),
            ));
        }
        Ok(())
    }
    pub fn root_budget(&self, owner: &Owner, id: &str) -> Result<ConvergenceBudget> {
        let e = self
            .entries
            .get(&(owner.clone(), id.into()))
            .ok_or_else(|| ContractError::Denied("unknown plan".into()))?;
        self.roots
            .get(&(owner.clone(), e.root.clone()))
            .map(|r| r.budget.clone())
            .ok_or_else(|| ContractError::Stale("expired root".into()))
    }
    pub fn begin<T: Serialize>(
        &mut self,
        owner: &Owner,
        id: &str,
        plan: &T,
        request_id: &str,
    ) -> Result<BeginPermit> {
        self.matches(owner, id, plan)?;
        bounded_id(request_id)?;
        let e = self
            .entries
            .get_mut(&(owner.clone(), id.into()))
            .ok_or_else(|| ContractError::Denied("unknown plan".into()))?;
        if e.consumed {
            return Err(ContractError::Denied(
                "plan already attempted; observe instead of retry".into(),
            ));
        }
        let r = self
            .roots
            .get_mut(&(owner.clone(), e.root.clone()))
            .ok_or_else(|| ContractError::Stale("missing root".into()))?;
        if r.attempts.iter().any(|a| a.request_id == request_id)
            || r.reconciliation_requests.contains(request_id)
        {
            return Err(ContractError::Denied("request replay".into()));
        }
        if r.attempts
            .last()
            .is_some_and(|a| a.status != ExecutionStatus::Completed)
            && r.reconciliations.last().is_none_or(|record| {
                record.attempt_index != r.attempts.len() - 1
                    || record.child_plan_id.as_deref() != Some(id)
            })
        {
            return Err(ContractError::Unknown(
                "previous outcome needs reconciliation".into(),
            ));
        }
        let operations = r
            .operations
            .checked_add(e.operations)
            .ok_or_else(|| ContractError::Limit("operation overflow".into()))?;
        if operations > r.budget.max_operations
            || r.iterations >= r.budget.max_iterations
            || r.attempts.len() >= self.max_attempts
        {
            return Err(ContractError::Limit(
                "aggregate convergence budget exhausted".into(),
            ));
        }
        // Reserve before the first side effect. Failure never refunds the budget.
        e.consumed = true;
        r.operations = operations;
        r.iterations += 1;
        let index = r.attempts.len();
        r.attempts.push(Attempt {
            request_id: request_id.into(),
            plan_digest: id.into(),
            status: ExecutionStatus::Applying,
            effects: vec![],
        });
        Ok(BeginPermit {
            vault_identity: self.identity.clone(),
            root_identity: r.identity.clone(),
            owner: owner.clone(),
            root: e.root.clone(),
            index,
            digest: id.into(),
        })
    }
    pub fn finish(
        &mut self,
        permit: BeginPermit,
        status: ExecutionStatus,
        effects: Vec<String>,
    ) -> Result<()> {
        ensure(
            !matches!(
                status,
                ExecutionStatus::Prepared | ExecutionStatus::Applying
            ),
            "finish requires terminal attempt status",
        )?;
        ensure(
            Arc::ptr_eq(&self.identity, &permit.vault_identity),
            "attempt permit belongs to another plan vault incarnation",
        )?;
        ensure(effects.len() <= 4096, "effect receipt count")?;
        for effect in &effects {
            ensure(
                effect.len() <= 512 && !effect.chars().any(char::is_control),
                "invalid effect receipt",
            )?;
        }
        let r = self
            .roots
            .get_mut(&(permit.owner, permit.root))
            .ok_or_else(|| ContractError::Unknown("attempt root lost".into()))?;
        ensure(
            Arc::ptr_eq(&r.identity, &permit.root_identity),
            "attempt permit belongs to a stale root incarnation",
        )?;
        let a = r
            .attempts
            .get_mut(permit.index)
            .ok_or_else(|| ContractError::Unknown("attempt receipt lost".into()))?;
        ensure(
            a.plan_digest == permit.digest && a.status == ExecutionStatus::Applying,
            "invalid attempt permit",
        )?;
        a.status = status;
        a.effects = effects;
        Ok(())
    }
    /// Reserve observation budget before the profile performs any reconciliation
    /// read. expected_base and scope must come from trusted current provider state,
    /// not from client input. A failed read never refunds this reservation.
    pub fn begin_reconciliation<T: Serialize>(
        &mut self,
        owner: &Owner,
        id: &str,
        plan: &T,
        request_id: &str,
        expected_base: BaseStateSet,
        scope: Vec<Address>,
    ) -> Result<ReconciliationPermit> {
        self.matches(owner, id, plan)?;
        bounded_id(request_id)?;
        expected_base.validate()?;
        ensure(
            !scope.is_empty() && scope.len() <= MAX_ENTRIES,
            "reconciliation scope",
        )?;
        let resources: BTreeSet<_> = expected_base.0.iter().map(|base| &base.key).collect();
        for address in &scope {
            bounded_id(&address.logical_id)?;
            bounded_id(&address.property)?;
            ensure(
                resources.contains(&address.resource),
                "reconciliation resource scope",
            )?;
        }
        let entry = self
            .entries
            .get(&(owner.clone(), id.into()))
            .expect("matches checked");
        let root = self
            .roots
            .get_mut(&(owner.clone(), entry.root.clone()))
            .expect("matches checked");
        let attempt = root
            .attempts
            .last()
            .ok_or_else(|| ContractError::Denied("no attempted parent".into()))?;
        ensure(
            attempt.plan_digest == id
                && matches!(
                    attempt.status,
                    ExecutionStatus::Partial | ExecutionStatus::Unknown
                ),
            "reconciliation requires the latest Partial/Unknown attempt",
        )?;
        ensure(
            !root
                .attempts
                .iter()
                .any(|attempt| attempt.request_id == request_id)
                && !root.reconciliation_requests.contains(request_id),
            "reconciliation request replay",
        )?;
        ensure(
            root.reconciliations
                .last()
                .is_none_or(|record| record.attempt_index != root.attempts.len() - 1),
            "attempt already reconciled",
        )?;
        if root.observations >= root.budget.max_observations {
            return Err(ContractError::Limit("aggregate observation budget".into()));
        }
        root.observations += 1;
        root.reconciliation_requests.insert(request_id.into());
        Ok(ReconciliationPermit {
            vault_identity: self.identity.clone(),
            root_identity: root.identity.clone(),
            owner: owner.clone(),
            root: entry.root.clone(),
            index: root.attempts.len() - 1,
            plan_id: id.into(),
            request_id: request_id.into(),
            observation_sequence: root.observations,
            expected_base,
            scope: scope.into_iter().collect(),
        })
    }

    /// The trusted profile must freshly observe the pending effect and prove one
    /// deterministic repair before calling this method. Serialized evidence alone
    /// is not authority. The old attempt/effects/budget are preserved unchanged.
    pub fn finish_reconciliation(
        &mut self,
        permit: ReconciliationPermit,
        observed: ObservationRef,
    ) -> Result<()> {
        ensure(
            Arc::ptr_eq(&self.identity, &permit.vault_identity),
            "foreign reconciliation vault",
        )?;
        canonical_bytes(&observed)?;
        bounded_id(&observed.id)?;
        bounded_id(&observed.method)?;
        ensure(
            observed.exhaustive
                && observed.method_version > 0
                && observed.artifact.is_some()
                && matches!(
                    observed.source,
                    EvidenceSource::NativeApi
                        | EvidenceSource::RendererState
                        | EvidenceSource::DecodedMedia
                        | EvidenceSource::FileRead
                ),
            "reconciliation needs exhaustive native evidence",
        )?;
        permit.expected_base.check_fresh(&observed.base, false)?;
        let scope: BTreeSet<_> = observed.scope.iter().cloned().collect();
        ensure(
            scope == permit.scope && observed.scope.len() == scope.len(),
            "reconciliation observation scope mismatch",
        )?;
        let root = self
            .roots
            .get_mut(&(permit.owner, permit.root))
            .ok_or_else(|| ContractError::Stale("reconciliation root lost".into()))?;
        ensure(
            Arc::ptr_eq(&root.identity, &permit.root_identity),
            "stale reconciliation root incarnation",
        )?;
        if expired(
            root.started.elapsed().as_millis(),
            root.budget.max_elapsed_ms,
        ) {
            return Err(ContractError::Stale("reconciliation root expired".into()));
        }
        ensure(
            root.attempts.len() == permit.index + 1
                && root.observations == permit.observation_sequence,
            "stale reconciliation observation",
        )?;
        let attempt = &root.attempts[permit.index];
        ensure(
            attempt.plan_digest == permit.plan_id
                && matches!(
                    attempt.status,
                    ExecutionStatus::Partial | ExecutionStatus::Unknown
                )
                && root
                    .reconciliations
                    .last()
                    .is_none_or(|record| record.attempt_index != permit.index),
            "reconciliation attempt changed or replayed",
        )?;
        root.reconciliations.push(ReconciliationRecord {
            attempt_index: permit.index,
            request_id: permit.request_id,
            observation: observed,
            child_plan_id: None,
        });
        Ok(())
    }

    pub fn reconciliations(&self, owner: &Owner, id: &str) -> Result<&[ReconciliationRecord]> {
        let entry = self
            .entries
            .get(&(owner.clone(), id.into()))
            .ok_or_else(|| ContractError::Denied("unknown plan".into()))?;
        self.roots
            .get(&(owner.clone(), entry.root.clone()))
            .map(|root| root.reconciliations.as_slice())
            .ok_or_else(|| ContractError::Stale("missing root".into()))
    }

    pub fn record_observation(&mut self, owner: &Owner, id: &str, findings: u32) -> Result<()> {
        let e = self
            .entries
            .get(&(owner.clone(), id.into()))
            .ok_or_else(|| ContractError::Denied("unknown plan".into()))?;
        let r = self
            .roots
            .get_mut(&(owner.clone(), e.root.clone()))
            .ok_or_else(|| ContractError::Stale("missing root".into()))?;
        if findings > r.budget.max_findings || r.observations >= r.budget.max_observations {
            return Err(ContractError::Limit("aggregate observation budget".into()));
        }
        r.observations += 1;
        Ok(())
    }
    pub fn ledger(&self, owner: &Owner, id: &str) -> Result<&[Attempt]> {
        let e = self
            .entries
            .get(&(owner.clone(), id.into()))
            .ok_or_else(|| ContractError::Denied("unknown plan".into()))?;
        self.roots
            .get(&(owner.clone(), e.root.clone()))
            .map(|r| r.attempts.as_slice())
            .ok_or_else(|| ContractError::Stale("missing root".into()))
    }
    pub fn revoke(&mut self, owner: &Owner) {
        self.entries.retain(|(o, _), _| o != owner);
        self.roots.retain(|(o, _), _| o != owner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn test_owner() -> Owner {
        Owner {
            session: "vault-unit-session".into(),
            principal: PrincipalBinding::HostSession,
        }
    }

    fn test_budget() -> ConvergenceBudget {
        ConvergenceBudget {
            max_iterations: 4,
            max_operations: 8,
            max_findings: 8,
            max_observations: 8,
            max_elapsed_ms: 100,
        }
    }

    #[test]
    fn expiry_boundary_is_strict_and_deterministic() {
        assert!(!expired(0, 0));
        assert!(!expired(100, 100));
        assert!(expired(101, 100));
        assert!(expired(u128::from(u64::MAX) + 1, u64::MAX));
    }

    #[test]
    fn finish_requires_both_matching_digest_and_applying_state() {
        let owner = test_owner();
        let plan = json!({"kind":"unit"});
        let mut vault = PlanVault::bounded(8, 4, 8);
        vault
            .issue(&owner, "root", &plan, test_budget(), 1, None, false)
            .unwrap();
        let permit = vault.begin(&owner, "root", &plan, "request").unwrap();

        let bad_digest = BeginPermit {
            vault_identity: permit.vault_identity.clone(),
            root_identity: permit.root_identity.clone(),
            owner: permit.owner.clone(),
            root: permit.root.clone(),
            index: permit.index,
            digest: "different-plan".into(),
        };
        assert!(
            vault
                .finish(bad_digest, ExecutionStatus::Completed, vec![])
                .is_err()
        );

        vault
            .roots
            .get_mut(&(permit.owner.clone(), permit.root.clone()))
            .unwrap()
            .attempts[permit.index]
            .status = ExecutionStatus::Completed;
        assert!(
            vault
                .finish(permit, ExecutionStatus::Completed, vec![])
                .is_err()
        );
    }

    #[test]
    fn stale_permit_cannot_cross_revoke_and_reissue() {
        let owner = test_owner();
        let plan = json!({"kind":"unit"});
        let mut vault = PlanVault::bounded(8, 4, 8);
        vault
            .issue(&owner, "root", &plan, test_budget(), 1, None, false)
            .unwrap();
        let stale = vault.begin(&owner, "root", &plan, "request-old").unwrap();

        vault.revoke(&owner);
        vault
            .issue(&owner, "root", &plan, test_budget(), 1, None, false)
            .unwrap();
        let current = vault.begin(&owner, "root", &plan, "request-new").unwrap();

        assert!(
            vault
                .finish(stale, ExecutionStatus::Completed, vec![])
                .is_err()
        );
        vault
            .finish(current, ExecutionStatus::Completed, vec![])
            .unwrap();
    }

    #[test]
    fn permit_cannot_cross_independent_vaults() {
        let owner = test_owner();
        let plan = json!({"kind":"unit"});
        let mut first = PlanVault::bounded(8, 4, 8);
        let mut second = PlanVault::bounded(8, 4, 8);
        first
            .issue(&owner, "root", &plan, test_budget(), 1, None, false)
            .unwrap();
        second
            .issue(&owner, "root", &plan, test_budget(), 1, None, false)
            .unwrap();

        let foreign = first.begin(&owner, "root", &plan, "request").unwrap();
        let current = second.begin(&owner, "root", &plan, "request").unwrap();
        assert!(
            second
                .finish(foreign, ExecutionStatus::Completed, vec![])
                .is_err()
        );
        second
            .finish(current, ExecutionStatus::Completed, vec![])
            .unwrap();
    }

    #[test]
    fn stale_permit_cannot_cross_expiry_and_reissue() {
        let owner = test_owner();
        let plan = json!({"kind":"unit"});
        let mut expiring = test_budget();
        expiring.max_elapsed_ms = 5;
        let mut vault = PlanVault::bounded(8, 4, 8);
        vault
            .issue(&owner, "root", &plan, expiring, 1, None, false)
            .unwrap();
        let stale = vault.begin(&owner, "root", &plan, "request-old").unwrap();

        std::thread::sleep(std::time::Duration::from_millis(10));
        vault
            .issue(&owner, "root", &plan, test_budget(), 1, None, false)
            .unwrap();
        let current = vault.begin(&owner, "root", &plan, "request-new").unwrap();

        assert!(
            vault
                .finish(stale, ExecutionStatus::Completed, vec![])
                .is_err()
        );
        vault
            .finish(current, ExecutionStatus::Completed, vec![])
            .unwrap();
    }
}
