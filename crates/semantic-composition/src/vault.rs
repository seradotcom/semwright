use crate::*;
use serde::Serialize;
use std::{collections::BTreeMap, time::Instant};

#[derive(Debug, Clone, Serialize)]
pub struct Attempt {
    pub request_id: String,
    pub plan_digest: String,
    pub status: ExecutionStatus,
    pub effects: Vec<String>,
}
struct Root {
    budget: ConvergenceBudget,
    started: Instant,
    operations: u32,
    iterations: u32,
    observations: u32,
    attempts: Vec<Attempt>,
}
struct Entry {
    root: String,
    canonical: Vec<u8>,
    operations: u32,
    consumed: bool,
}
/// Private, non-serializable permit. Only begin() creates one, and finish consumes it.
/// This prevents replay, not policy bypass: caller must still use the Broker.
pub struct BeginPermit {
    owner: Owner,
    root: String,
    index: usize,
    digest: String,
}
pub struct PlanVault {
    entries: BTreeMap<(Owner, String), Entry>,
    roots: BTreeMap<(Owner, String), Root>,
    max_plans: usize,
    max_roots: usize,
    max_attempts: usize,
}
impl PlanVault {
    pub fn bounded(max_plans: usize, max_roots: usize, max_attempts: usize) -> Self {
        Self {
            entries: BTreeMap::new(),
            roots: BTreeMap::new(),
            max_plans: max_plans.clamp(1, 256),
            max_roots: max_roots.clamp(1, 64),
            max_attempts: max_attempts.clamp(1, 4096),
        }
    }
    fn reap(&mut self) {
        self.roots
            .retain(|_, r| r.started.elapsed().as_millis() <= u128::from(r.budget.max_elapsed_ms));
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
                .get(&(owner.clone(), e.root.clone()))
                .ok_or_else(|| ContractError::Stale("expired root".into()))?;
            ensure(r.budget == budget, "repair cannot reset or enlarge budget")?;
            ensure(
                r.attempts
                    .last()
                    .is_some_and(|a| a.status == ExecutionStatus::Completed),
                "repair needs a completed observed parent; unknown requires reconciliation",
            )?;
            e.root.clone()
        } else {
            ensure(!repair, "repair needs a root")?;
            if self.roots.len() >= self.max_roots {
                return Err(ContractError::Limit("server root capacity".into()));
            }
            self.roots.insert(
                key.clone(),
                Root {
                    budget,
                    started: Instant::now(),
                    operations: 0,
                    iterations: 0,
                    observations: 0,
                    attempts: vec![],
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
        if r.started.elapsed().as_millis() > u128::from(r.budget.max_elapsed_ms) {
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
        if r.attempts.iter().any(|a| a.request_id == request_id) {
            return Err(ContractError::Denied("request replay".into()));
        }
        if r.attempts
            .last()
            .is_some_and(|a| a.status != ExecutionStatus::Completed)
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
