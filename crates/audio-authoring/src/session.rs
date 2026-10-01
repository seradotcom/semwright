//! Session-bound orchestration over the shared C0 vault and controller.
//! This component grants no native authority. Its caller remains the authorized driver.
use crate::{
    AudioIntent, AudioValidation, GainRepair, MeasuredAudio, PlannedAudio, address, domain,
    gain_repair, plan, replay, validate,
};
use semwright_audio_domain::{
    edit::{self, Edit},
    model::AudioProject,
};
use semwright_semantic_composition::*;
use std::{collections::BTreeMap, time::Instant};
struct Entry {
    planned: PlannedAudio,
    controller: Option<Controller>,
    started: Instant,
    repair: bool,
    measurements: BTreeMap<String, MeasuredAudio>,
    validation: Option<AudioValidation>,
}
pub struct AudioSession {
    profile: ProfileDescriptor,
    vault: PlanVault,
    entries: BTreeMap<(Owner, String), Entry>,
}
pub struct ApplyPermit {
    native: BeginPermit,
    key: (Owner, String),
    effects: Vec<String>,
}
pub struct ApplyCandidate {
    pub model: AudioProject,
    pub permit: ApplyPermit,
}
impl AudioSession {
    pub fn new(profile: ProfileDescriptor) -> Result<Self> {
        profile.validate()?;
        Ok(Self {
            profile,
            vault: PlanVault::bounded(128, 64, 4096),
            entries: BTreeMap::new(),
        })
    }
    pub fn profile(&self) -> &ProfileDescriptor {
        &self.profile
    }
    pub fn prepare(
        &mut self,
        owner: Owner,
        base: BaseStateSet,
        project: &AudioProject,
        intent: AudioIntent,
    ) -> Result<PlannedAudio> {
        self.reap();
        ensure(self.entries.len() < 128, "audio session plan capacity")?;
        let result = plan(project, base, owner.clone(), intent, &self.profile)?;
        let id = result.plan.digest.as_str().to_owned();
        let budget = result.plan.body.budget.clone();
        if let Some(entry) = self.entries.get(&(owner.clone(), id.clone())) {
            return Ok(entry.planned.clone());
        }
        self.vault.issue(
            &owner,
            &id,
            &result,
            budget.clone(),
            result.plan.body.changes.operations.len() as u32,
            None,
            false,
        )?;
        let controller = Controller::new(
            result.plan.digest.clone(),
            budget,
            self.profile.required_rules.clone(),
        )?;
        self.entries.insert(
            (owner, id),
            Entry {
                planned: result.clone(),
                controller: Some(controller),
                started: Instant::now(),
                repair: false,
                measurements: BTreeMap::new(),
                validation: None,
            },
        );
        Ok(result)
    }
    pub fn begin_apply(
        &mut self,
        owner: &Owner,
        submitted: &PlannedAudio,
        project: &AudioProject,
        fresh: &BaseStateSet,
        request: &str,
    ) -> Result<ApplyCandidate> {
        let id = submitted.plan.digest.as_str();
        self.vault.matches(owner, id, submitted)?;
        submitted
            .plan
            .body
            .base
            .check_fresh(fresh, submitted.plan.body.require_compare_and_swap)?;
        ensure(
            submitted.plan.body.owner == *owner,
            "audio plan belongs to a different owner",
        )?;
        let model = replay(project, submitted, &self.profile)?;
        let key = (owner.clone(), id.to_owned());
        let entry = self
            .entries
            .get_mut(&key)
            .ok_or_else(|| ContractError::Denied("unknown audio plan".into()))?;
        let controller = entry
            .controller
            .as_mut()
            .ok_or_else(|| ContractError::Denied("plan lifecycle moved to its repair".into()))?;
        let native = self.vault.begin(owner, id, submitted, request)?;
        if let Err(error) = controller.applying(entry.repair) {
            self.vault.finish(native, ExecutionStatus::Failed, vec![])?;
            return Err(error);
        }
        Ok(ApplyCandidate {
            model,
            permit: ApplyPermit {
                native,
                key,
                effects: submitted.affected.iter().cloned().collect(),
            },
        })
    }
    pub fn finish_apply(&mut self, permit: ApplyPermit, status: ExecutionStatus) -> Result<()> {
        let effects = if status == ExecutionStatus::Completed {
            permit.effects
        } else {
            vec![]
        };
        self.vault.finish(permit.native, status, effects)?;
        let entry = self
            .entries
            .get_mut(&permit.key)
            .ok_or_else(|| ContractError::Unknown("lost audio lifecycle after effect".into()))?;
        entry
            .controller
            .as_mut()
            .ok_or_else(|| ContractError::Unknown("lost audio controller".into()))?
            .executed(status)?;
        Ok(())
    }
    /// Accept only the result of the driver's bounded native decoder.
    pub fn record_measurement(
        &mut self,
        owner: &Owner,
        plan_id: &str,
        measured: MeasuredAudio,
    ) -> Result<String> {
        let key = (owner.clone(), plan_id.to_owned());
        let entry = self.entries.get_mut(&key).ok_or_else(|| {
            ContractError::Denied("measurement plan is not owned by this session".into())
        })?;
        self.vault.matches(owner, plan_id, &entry.planned)?;
        ensure(entry.measurements.len() < 8, "audio measurement capacity")?;
        let expected = &entry.planned.plan.body.base.0[0];
        ensure(measured.base.0.len() == 1, "measurement base membership")?;
        let actual = &measured.base.0[0];
        ensure(
            actual.key == expected.key
                && actual.document_id == expected.document_id
                && actual.provider_session == expected.provider_session
                && actual.generation == expected.generation,
            "measurement provider/session generation changed",
        )?;
        let id = canonical_digest(&measured)?.as_str().to_owned();
        entry.measurements.insert(id.clone(), measured);
        Ok(id)
    }
    pub fn validate_measurement(
        &mut self,
        owner: &Owner,
        plan_id: &str,
        measurement_id: &str,
        project: &AudioProject,
    ) -> Result<AudioValidation> {
        let key = (owner.clone(), plan_id.to_owned());
        let entry = self
            .entries
            .get_mut(&key)
            .ok_or_else(|| ContractError::Denied("unknown audio plan".into()))?;
        self.vault.matches(owner, plan_id, &entry.planned)?;
        let measured = entry.measurements.get(measurement_id).ok_or_else(|| {
            ContractError::Denied("measurement was not issued by this session".into())
        })?;
        let validation = validate(&entry.planned, project, measured)?;
        self.vault
            .record_observation(owner, plan_id, validation.findings.len() as u32)?;
        let count = usize::from(
            gain_repair(&entry.planned, project, measured)
                .ok()
                .flatten()
                .is_some(),
        );
        let controller = entry
            .controller
            .as_mut()
            .ok_or_else(|| ContractError::Denied("plan lifecycle moved to its repair".into()))?;
        if controller.state == State::Observing {
            let failures = validation
                .report
                .checks
                .iter()
                .filter(|c| c.verdict == Verdict::Fail)
                .count() as u64;
            let elapsed = entry
                .started
                .elapsed()
                .as_millis()
                .min(u128::from(u64::MAX)) as u64;
            controller.observed(&validation.report, vec![failures], count, elapsed)?;
        }
        entry.validation = Some(validation.clone());
        Ok(validation)
    }
    pub fn prepare_gain_repair(
        &mut self,
        owner: &Owner,
        parent_id: &str,
        measurement_id: &str,
        project: &AudioProject,
        fresh: BaseStateSet,
    ) -> Result<(PlannedAudio, GainRepair)> {
        ensure(self.entries.len() < 128, "audio session plan capacity")?;
        let parent_key = (owner.clone(), parent_id.to_owned());
        let entry = self
            .entries
            .get(&parent_key)
            .ok_or_else(|| ContractError::Denied("unknown parent audio plan".into()))?;
        let measured = entry
            .measurements
            .get(measurement_id)
            .ok_or_else(|| ContractError::Denied("unknown audio measurement".into()))?;
        measured.base.check_fresh(&fresh, false)?;
        ensure(
            entry
                .controller
                .as_ref()
                .is_some_and(|c| c.state == State::RepairPlanned),
            "repair requires a failed, measured, uniquely repairable parent",
        )?;
        let repair = gain_repair(&entry.planned, project, measured)?
            .ok_or_else(|| ContractError::Invalid("audio already satisfies constraints".into()))?;
        let mut body = entry.planned.plan.body.clone();
        body.base = fresh;
        let location = address(&body.base, &repair.master_bus, "gain")?;
        let operation = Edit::BusGainSet {
            bus: repair.master_bus.clone(),
            gain: repair.next_gain,
        };
        let outcome = domain(edit::apply(
            project,
            &domain(project.semantic_digest())?,
            operation.clone(),
            "audio-repair-gain",
        ))?;
        body.observation_scope = vec![location.clone()];
        body.changes = ChangeSet {
            atomicity: Atomicity::InMemoryTransaction,
            operations: vec![TypedOperation {
                id: "audio-repair-gain".into(),
                payload: operation,
                reads: vec![location.clone()],
                writes: vec![location],
                effects: [EffectClass::UpdateOwnedObject].into(),
                depends_on: vec![],
                postconditions: body.required_rules.clone(),
            }],
        };
        let planned = PlannedAudio {
            plan: PreparedPlan::prepare(body, &self.profile)?,
            logical_bindings: entry.planned.logical_bindings.clone(),
            quantization_errors: entry.planned.quantization_errors.clone(),
            resulting_model_digest: Digest::parse(domain(outcome.result.semantic_digest())?)?,
            affected: [repair.master_bus.clone()].into(),
        };
        let started = entry.started;
        let id = planned.plan.digest.as_str().to_owned();
        self.vault.issue(
            owner,
            &id,
            &planned,
            planned.plan.body.budget.clone(),
            1,
            Some(parent_id),
            true,
        )?;
        let mut controller = self
            .entries
            .get_mut(&parent_key)
            .and_then(|e| e.controller.take())
            .ok_or_else(|| ContractError::Denied("repair lifecycle already consumed".into()))?;
        controller.bind_repair(planned.plan.digest.clone())?;
        self.entries.insert(
            (owner.clone(), id),
            Entry {
                planned: planned.clone(),
                controller: Some(controller),
                started,
                repair: true,
                measurements: BTreeMap::new(),
                validation: None,
            },
        );
        Ok((planned, repair))
    }
    pub fn get(&self, owner: &Owner, id: &str) -> Result<&PlannedAudio> {
        let entry = self
            .entries
            .get(&(owner.clone(), id.to_owned()))
            .ok_or_else(|| ContractError::Denied("unknown audio plan".into()))?;
        self.vault.matches(owner, id, &entry.planned)?;
        Ok(&entry.planned)
    }
    pub fn verify(
        &mut self,
        owner: &Owner,
        id: &str,
        measurement_id: &str,
        project: &AudioProject,
    ) -> Result<VerificationReport> {
        let validation = self.validate_measurement(owner, id, measurement_id, project)?;
        let entry = self
            .entries
            .get(&(owner.clone(), id.to_owned()))
            .ok_or_else(|| ContractError::Denied("unknown audio plan".into()))?;
        let status = self
            .vault
            .ledger(owner, id)?
            .last()
            .map(|attempt| attempt.status)
            .unwrap_or(ExecutionStatus::Prepared);
        let observed = status == ExecutionStatus::Completed
            && validation
                .report
                .checks
                .iter()
                .any(|c| c.rule == "audio.model" && c.verdict == Verdict::Pass);
        let scope = entry.planned.plan.body.observation_scope.clone();
        Ok(VerificationReport {
            execution_status: status,
            validation: validation.report,
            support_level: SupportLevel::Composed,
            effects_observed: if observed { scope.clone() } else { vec![] },
            effects_unobservable: if observed { vec![] } else { scope },
        })
    }
    pub fn status(&self, owner: &Owner, id: &str) -> Result<(State, Option<StopReason>)> {
        self.get(owner, id)?;
        let entry = self
            .entries
            .get(&(owner.clone(), id.to_owned()))
            .ok_or_else(|| ContractError::Denied("unknown audio plan".into()))?;
        let controller = entry.controller.as_ref().ok_or_else(|| {
            ContractError::Denied("lifecycle belongs to a newer repair plan".into())
        })?;
        Ok((controller.state, controller.stop))
    }
    pub fn revoke(&mut self, owner: &Owner) {
        self.vault.revoke(owner);
        self.entries
            .retain(|(entry_owner, _), _| entry_owner != owner);
    }
    fn reap(&mut self) {
        self.entries.retain(|_, entry| {
            entry.started.elapsed().as_millis()
                <= u128::from(entry.planned.plan.body.budget.max_elapsed_ms)
        });
    }
}
