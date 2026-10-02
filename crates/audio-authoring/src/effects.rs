//! Compiled verification consumer over measurements admitted by AudioSession.
//! This adapter reads no filesystem and grants no native execution authority.
use crate::{AudioPlan, DeliveryProfile, MeasuredAudio, address, required_rules};
use semwright_audio_domain::{edit::Edit, model::AudioProject};
use semwright_effect_conformance as effect;
use semwright_semantic_composition::*;

const METHOD: &str = "audio-decoded-constraints";

fn boolean() -> effect::Predicate {
    effect::Predicate::Equals {
        expected: effect::ObservedValue::Bool { value: true },
    }
}

/// The planner supplies the expected model and operations. Request JSON never
/// supplies or replaces this specification. Artifact bytes are not known before
/// rendering; the original decoder receipt is preserved separately below.
pub fn contract(
    base: &BaseStateSet,
    master: &str,
    delivery: &DeliveryProfile,
    expected_model: &Digest,
    operations: &[TypedOperation<Edit>],
) -> Result<effect::EffectContract> {
    let operation = operations
        .last()
        .ok_or_else(|| ContractError::Invalid("empty audio effect plan".into()))?;
    let location = address(base, master, "decoded_master")?;
    let rules = required_rules()
        .into_iter()
        .map(|id| {
            let predicate = match id.as_str() {
                "audio.model" => effect::Predicate::DigestEquals {
                    expected: expected_model.clone(),
                },
                "audio.loudness" => delivery
                    .integrated_lufs_milli
                    .map_or_else(boolean, |target| effect::Predicate::Within {
                        expected: f64::from(target),
                        tolerance: f64::from(delivery.loudness_tolerance_milli),
                        units: "LUFS_milli".into(),
                    }),
                "audio.true_peak" => {
                    delivery
                        .true_peak_ceiling_millidbtp
                        .map_or_else(boolean, |ceiling| effect::Predicate::Range {
                            min: f64::from(i32::MIN),
                            max: f64::from(ceiling),
                            units: "dBTP_milli".into(),
                        })
                }
                _ => boolean(),
            };
            effect::EffectRule {
                id,
                version: 1,
                obligation: effect::Obligation::Required,
                operation_id: operation.id.clone(),
                address: location.clone(),
                predicate,
                method: effect::ObservationMethod {
                    name: METHOD.into(),
                    version: 1,
                    source: EvidenceSource::DecodedMedia,
                },
                universe: None,
                artifact: None,
                require_causal_attribution: false,
            }
        })
        .collect();
    let result = effect::EffectContract {
        version: effect::EFFECT_CONTRACT_VERSION,
        profile: "semwright.audio".into(),
        rules,
        allowed: operations
            .iter()
            .map(|op| effect::EffectLimit {
                operation_id: op.id.clone(),
                effects: op.effects.clone(),
                writes: op.writes.iter().cloned().collect(),
            })
            .collect(),
    };
    result.validate()?;
    Ok(result)
}

struct AudioEvidenceAdapter<'a> {
    owner: &'a Owner,
    project: &'a AudioProject,
    measured: &'a MeasuredAudio,
    delivery: &'a DeliveryProfile,
}

impl effect::EvidenceAdapter for AudioEvidenceAdapter<'_> {
    fn identity(&self, resource: &ResourceKey) -> Option<effect::AdapterIdentity> {
        self.measured
            .base
            .0
            .iter()
            .find(|base| &base.key == resource)
            .map(|base| effect::AdapterIdentity {
                owner: self.owner.clone(),
                provider: base.key.provider.clone(),
                provider_session: base.provider_session.clone(),
                generation: base.generation.clone(),
            })
    }

    fn observe(
        &mut self,
        context: &effect::EvaluationContext,
        rule: &effect::EffectRule,
    ) -> Result<effect::AdapterObservation> {
        let pcm = &self.measured.statistics;
        let bool_value = |value| Some(effect::ObservedValue::Bool { value });
        let value = match rule.id.as_str() {
            "audio.model" => Some(effect::ObservedValue::Digest {
                value: self.measured.source_model.clone(),
            }),
            "audio.frames" => bool_value(
                pcm.frames == self.project.duration()
                    && pcm.sample_rate == self.project.profile.sample_rate
                    && pcm.channels.len() == usize::from(self.project.profile.channels),
            ),
            "audio.finite" => bool_value(pcm.nonfinite_samples == 0),
            "audio.silence" => bool_value(self.delivery.allow_silence || !pcm.entirely_silent),
            "audio.sample_peak" => bool_value(
                pcm.out_of_range_samples == 0
                    && pcm.peak_millidbfs.map_or(
                        pcm.entirely_silent && self.delivery.allow_silence,
                        |value| value <= self.delivery.peak_ceiling_millidbfs,
                    ),
            ),
            "audio.loudness" => match self.delivery.integrated_lufs_milli {
                None => bool_value(true),
                Some(_) => self
                    .measured
                    .loudness
                    .as_ref()
                    .and_then(|m| m.integrated_lufs_milli)
                    .map(|value| effect::ObservedValue::Number {
                        value: f64::from(value),
                        units: "LUFS_milli".into(),
                    }),
            },
            "audio.true_peak" => match self.delivery.true_peak_ceiling_millidbtp {
                None => bool_value(true),
                Some(_) => self
                    .measured
                    .loudness
                    .as_ref()
                    .and_then(|m| m.true_peak_millidbtp)
                    .map(|value| effect::ObservedValue::Number {
                        value: f64::from(value),
                        units: "dBTP_milli".into(),
                    }),
            },
            _ => {
                return Err(ContractError::Denied(
                    "unsupported compiled audio rule".into(),
                ));
            }
        };
        Ok(effect::AdapterObservation {
            binding: effect::EvidenceBinding {
                owner: context.owner.clone(),
                request_id: context.request_id.clone(),
                operation_id: rule.operation_id.clone(),
                plan_digest: context.plan_digest.clone(),
                contract_digest: context.contract_digest.clone(),
            },
            observation: ObservationRef {
                id: canonical_digest(self.measured)?.as_str().to_owned(),
                base: self.measured.base.clone(),
                source: EvidenceSource::DecodedMedia,
                method: METHOD.into(),
                method_version: 1,
                scope: vec![rule.address.clone()],
                artifact: None,
                exhaustive: self.measured.exhaustive,
            },
            readback: if value.is_some() {
                effect::ReadbackState::Observed
            } else {
                effect::ReadbackState::Unavailable
            },
            value,
            coverage: effect::ObservationCoverage {
                consistent: true,
                missing: vec![],
                attribution: effect::Attribution::Ordered,
                enumeration: None,
            },
        })
    }
}

pub(crate) fn evaluate_measurement(
    planned: &crate::PlannedAudio,
    profile: &ProfileDescriptor,
    project: &AudioProject,
    measured: &MeasuredAudio,
    attempt: &Attempt,
) -> Result<effect::EffectEvaluation> {
    let plan: &AudioPlan = &planned.plan;
    let contract = contract(
        &plan.body.base,
        &project.master_bus,
        &plan.body.intent.delivery,
        &planned.resulting_model_digest,
        &plan.body.changes.operations,
    )?;
    let context = effect::EvaluationContext {
        owner: plan.body.owner.clone(),
        request_id: attempt.request_id.clone(),
        plan_digest: plan.digest.clone(),
        contract_digest: contract.digest()?,
        before: plan.body.base.clone(),
        after: measured.base.clone(),
        operations: plan
            .body
            .changes
            .operations
            .iter()
            .map(|op| op.id.clone())
            .collect(),
        observation_scope: plan.body.observation_scope.iter().cloned().collect(),
        execution_status: attempt.status,
        support_level: SupportLevel::Composed,
        budget: plan.body.budget.clone(),
    };
    context.validate_plan(plan, profile, &contract)?;
    let mut adapter = AudioEvidenceAdapter {
        owner: &context.owner,
        project,
        measured,
        delivery: &plan.body.intent.delivery,
    };
    let batch = effect::collect(&contract, &context, &mut adapter)?;
    let mut result = effect::evaluate(&contract, &context, &batch)?;
    // Preserve actual decoder/version/artifact provenance without changing F's
    // verdicts or pretending that a future render digest was pinned in the plan.
    let decoded = ObservationRef {
        id: format!("pcm-{}", &measured.artifact.as_str()[..16]),
        base: measured.base.clone(),
        source: EvidenceSource::DecodedMedia,
        method: measured.decoder.clone(),
        method_version: measured.decoder_version,
        scope: vec![address(
            &measured.base,
            &project.master_bus,
            "decoded_master",
        )?],
        artifact: Some(measured.artifact.clone()),
        exhaustive: measured.exhaustive,
    };
    for check in &mut result.report.validation.checks {
        check.evidence.push(decoded.clone());
    }
    Ok(result)
}
