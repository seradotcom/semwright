use crate::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Prepared,
    Applying,
    Observing,
    Validating,
    RepairPlanned,
    Repairing,
    Verified,
    PartiallyApplied,
    Conflicted,
    Denied,
    Cancelled,
    Exhausted,
    Unknown,
    Failed,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    Complete,
    NoProgress,
    Cycle,
    Contradiction,
    Stale,
    Ambiguous,
    Denied,
    Cancelled,
    UnknownOutcome,
    BudgetExhausted,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Observe,
    PlanRepair,
    Stop(StopReason),
}
/// Pure lifecycle machine. Caller-supplied progress must come from a trusted
/// profile, not summed severities across unrelated domains. Smaller is better.
pub struct Controller {
    pub state: State,
    pub stop: Option<StopReason>,
    budget: ConvergenceBudget,
    required: BTreeSet<String>,
    plan: Digest,
    seen: BTreeSet<Digest>,
    last: Option<Vec<u64>>,
    rounds: u32,
}
impl Controller {
    pub fn new(
        plan: Digest,
        budget: ConvergenceBudget,
        required: BTreeSet<String>,
    ) -> Result<Self> {
        budget.validate()?;
        ensure(
            !required.is_empty() && required.len() <= 256,
            "required rules",
        )?;
        Ok(Self {
            state: State::Prepared,
            stop: None,
            budget,
            required,
            plan,
            seen: BTreeSet::new(),
            last: None,
            rounds: 0,
        })
    }
    pub fn applying(&mut self, repair: bool) -> Result<()> {
        ensure(
            self.state
                == if repair {
                    State::RepairPlanned
                } else {
                    State::Prepared
                },
            "illegal apply transition",
        )?;
        self.state = if repair {
            State::Repairing
        } else {
            State::Applying
        };
        Ok(())
    }
    fn terminal(&mut self, state: State, reason: StopReason) -> Decision {
        self.state = state;
        self.stop = Some(reason);
        Decision::Stop(reason)
    }
    pub fn executed(&mut self, status: ExecutionStatus) -> Result<Decision> {
        ensure(
            matches!(self.state, State::Applying | State::Repairing),
            "no active application",
        )?;
        Ok(match status {
            ExecutionStatus::Completed => {
                self.state = State::Observing;
                Decision::Observe
            }
            ExecutionStatus::Partial => {
                self.terminal(State::PartiallyApplied, StopReason::UnknownOutcome)
            }
            ExecutionStatus::Unknown => self.terminal(State::Unknown, StopReason::UnknownOutcome),
            ExecutionStatus::Denied => self.terminal(State::Denied, StopReason::Denied),
            ExecutionStatus::Cancelled => self.terminal(State::Cancelled, StopReason::Cancelled),
            ExecutionStatus::Failed => self.terminal(State::Failed, StopReason::UnknownOutcome),
            _ => {
                return Err(ContractError::Invalid(
                    "nonterminal execution receipt".into(),
                ));
            }
        })
    }
    pub fn observed(
        &mut self,
        report: &ValidationReport,
        progress: Vec<u64>,
        candidate_count: usize,
        elapsed_ms: u64,
    ) -> Result<Decision> {
        ensure(
            self.state == State::Observing,
            "observation out of sequence",
        )?;
        ensure(
            report.plan_digest == self.plan && report.required_rules == self.required,
            "validation scope/plan substitution",
        )?;
        ensure(
            !progress.is_empty() && progress.len() <= 32,
            "invalid profile progress vector",
        )?;
        self.state = State::Validating;
        if elapsed_ms > self.budget.max_elapsed_ms || self.rounds >= self.budget.max_iterations {
            return Ok(self.terminal(State::Exhausted, StopReason::BudgetExhausted));
        }
        self.rounds += 1;
        match report.verdict()? {
            Verdict::Pass => return Ok(self.terminal(State::Verified, StopReason::Complete)),
            Verdict::Unknown => {
                return Ok(self.terminal(State::Unknown, StopReason::UnknownOutcome));
            }
            Verdict::Fail => {}
        }
        let fingerprint = canonical_digest(&(report, &progress))?;
        if !self.seen.insert(fingerprint) {
            return Ok(self.terminal(State::Conflicted, StopReason::Cycle));
        }
        if self
            .last
            .as_ref()
            .is_some_and(|v| v.len() != progress.len() || progress >= *v)
        {
            return Ok(self.terminal(State::Conflicted, StopReason::NoProgress));
        }
        self.last = Some(progress);
        if candidate_count != 1 {
            return Ok(self.terminal(State::Conflicted, StopReason::Ambiguous));
        }
        self.state = State::RepairPlanned;
        Ok(Decision::PlanRepair)
    }
    /// Repair approval and freshness are enforced by the profile and Broker.
    /// This binding change itself grants no right to execute.
    pub fn bind_repair(&mut self, new_plan: Digest) -> Result<()> {
        ensure(self.state == State::RepairPlanned, "no repair pending")?;
        self.plan = new_plan;
        Ok(())
    }
    pub fn cancel(&mut self) -> Decision {
        self.terminal(State::Cancelled, StopReason::Cancelled)
    }
}
