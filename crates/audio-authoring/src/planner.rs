use crate::{AudioIntent, ClipIntent, DuckingIntent, Material, Placement, domain};
use schemars::JsonSchema;
use semwright_audio_domain::{
    edit::{self, AutomationOwner, Edit},
    model::{
        AudioClip, AudioProject, Automation, AutomationCurve, AutomationPoint, AutomationTarget,
        ClipSource, Stem,
    },
    presets,
    time::{MAX_SAMPLE_FRAME, SampleFrame, SampleRange},
    units::{MilliDb, Permille},
};
use semwright_media_time::{Rate, Rational, ResolvedCue, Round};
use semwright_semantic_composition::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
pub type AudioPlan = PreparedPlan<AudioIntent, Edit>;
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlannedAudio {
    pub plan: AudioPlan,
    pub logical_bindings: BTreeMap<String, String>,
    pub quantization_errors: BTreeMap<String, Rational>,
    pub resulting_model_digest: Digest,
    pub affected: BTreeSet<String>,
}
pub fn required_rules() -> BTreeSet<String> {
    [
        "audio.model",
        "audio.frames",
        "audio.finite",
        "audio.sample_peak",
        "audio.silence",
        "audio.loudness",
        "audio.true_peak",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}
pub fn profile(capabilities: Vec<CapabilityBinding>) -> Result<ProfileDescriptor> {
    let result = ProfileDescriptor {
        identity: ProfileIdentity {
            id: "semwright.audio".into(),
            version: 1,
            intent_schema: schema_digest::<AudioIntent>()?,
            operation_schema: schema_digest::<Edit>()?,
        },
        capabilities,
        required_rules: required_rules(),
        allowed_effects: [
            EffectClass::Inspect,
            EffectClass::CreateOwnedObject,
            EffectClass::UpdateOwnedObject,
            EffectClass::RenderPrivateArtifact,
            EffectClass::PublishArtifact,
        ]
        .into(),
    };
    result.validate()?;
    Ok(result)
}
pub fn address(base: &BaseStateSet, id: &str, property: &str) -> Result<Address> {
    ensure(base.0.len() == 1, "audio profile uses one project resource")?;
    Ok(Address {
        resource: base.0[0].key.clone(),
        logical_id: id.into(),
        property: property.into(),
    })
}
pub fn base_for(
    project: &AudioProject,
    provider: &str,
    owner: &Owner,
    generation: &str,
    concurrency: Concurrency,
) -> Result<BaseStateSet> {
    let base = BaseStateSet(vec![BaseState {
        key: ResourceKey {
            provider: provider.into(),
            resource: project.id.clone(),
        },
        document_id: project.id.clone(),
        provider_session: owner.session.clone(),
        generation: generation.into(),
        revision: Revision::Fingerprint(Digest::parse(domain(project.semantic_digest())?)?),
        concurrency,
    }]);
    base.validate()?;
    Ok(base)
}
fn quantize(time: Rational, rate: Rate) -> Result<(u64, Rational)> {
    ensure(
        time >= Rational::ZERO,
        "negative audio placement requires explicit pre-roll mapping",
    )?;
    let value = rate.quantize(time, Round::NearestAway)?;
    let frame = u64::try_from(value.index)
        .map_err(|_| ContractError::Invalid("negative sample position".into()))?;
    ensure(
        frame <= MAX_SAMPLE_FRAME,
        "sample placement exceeds domain budget",
    )?;
    Ok((frame, value.error))
}
fn clip_time(
    clip: &ClipIntent,
    cues: &BTreeMap<String, ResolvedCue>,
    rate: Rate,
) -> Result<(u64, u64, Rational)> {
    let (start, duration) = match &clip.placement {
        Placement::Absolute { start, duration } => (*start, *duration),
        Placement::Cue {
            cue,
            offset,
            duration,
        } => match cues.get(cue) {
            Some(ResolvedCue::Resolved { start, .. }) => (start.checked_add(*offset)?, *duration),
            _ => {
                return Err(ContractError::Unknown(
                    "required audio cue is unresolved".into(),
                ));
            }
        },
    };
    ensure(duration > Rational::ZERO, "clip duration must be positive")?;
    let (first, error) = quantize(start, rate)?;
    let (last, _) = quantize(start.checked_add(duration)?, rate)?;
    ensure(last > first, "clip duration quantizes to no samples")?;
    Ok((first, last - first, error))
}
struct Builder<'a> {
    project: AudioProject,
    operations: Vec<TypedOperation<Edit>>,
    affected: BTreeSet<String>,
    base: &'a BaseStateSet,
}
impl Builder<'_> {
    fn add(
        &mut self,
        edit: Edit,
        logical: &str,
        property: &str,
        create: bool,
    ) -> Result<Vec<String>> {
        ensure(self.operations.len() < 4096, "authoring operation budget")?;
        let id = format!("audio-op-{}", self.operations.len());
        let outcome = domain(edit::apply(
            &self.project,
            &domain(self.project.semantic_digest())?,
            edit.clone(),
            &id,
        ))?;
        let location = address(self.base, logical, property)?;
        self.operations.push(TypedOperation {
            id,
            payload: edit,
            reads: vec![location.clone()],
            writes: vec![location],
            effects: [if create {
                EffectClass::CreateOwnedObject
            } else {
                EffectClass::UpdateOwnedObject
            }]
            .into(),
            depends_on: self
                .operations
                .last()
                .map(|op| vec![op.id.clone()])
                .unwrap_or_default(),
            postconditions: ["audio.model".into()].into(),
        });
        self.affected.extend(outcome.affected);
        self.affected.extend(outcome.created.iter().cloned());
        self.project = outcome.result;
        Ok(outcome.created)
    }
}
pub fn plan(
    project: &AudioProject,
    base: BaseStateSet,
    owner: Owner,
    intent: AudioIntent,
    profile: &ProfileDescriptor,
) -> Result<PlannedAudio> {
    intent.validate(project)?;
    owner.validate()?;
    base.validate()?;
    ensure(
        base.0.len() == 1
            && base.0[0].document_id == project.id
            && base.0[0].provider_session == owner.session,
        "audio base/owner/document mismatch",
    )?;
    ensure(
        base.0[0].revision
            == Revision::Fingerprint(Digest::parse(domain(project.semantic_digest())?)?),
        "audio base revision does not describe the planning model",
    )?;
    let cues = intent.cues.resolve()?;
    let rate = Rate::new(project.profile.sample_rate.0, 1)?;
    let mut builder = Builder {
        project: project.clone(),
        operations: vec![],
        affected: BTreeSet::new(),
        base: &base,
    };
    let mut bindings = BTreeMap::new();
    let mut errors = BTreeMap::new();
    for track in &intent.tracks {
        let stem_id = if let Some(id) = &track.existing_stem {
            builder.add(
                Edit::StemGainSet {
                    stem: id.clone(),
                    gain: track.gain,
                },
                id,
                "gain",
                false,
            )?;
            builder.add(
                Edit::StemPanSet {
                    stem: id.clone(),
                    pan_milli: track.pan_milli,
                },
                id,
                "pan",
                false,
            )?;
            builder.add(
                Edit::StemRoute {
                    stem: id.clone(),
                    bus: track.output_bus.clone(),
                },
                id,
                "output_bus",
                false,
            )?;
            id.clone()
        } else {
            builder.add(
                Edit::StemCreate {
                    stem: Stem {
                        id: track.id.clone(),
                        name: track.name.clone(),
                        channels: track.channels,
                        muted: false,
                        soloed: false,
                        gain: track.gain,
                        pan_milli: track.pan_milli,
                        output_bus: track.output_bus.clone(),
                        sends: vec![],
                        clips: vec![],
                        effects: track.effects.clone(),
                        automations: vec![],
                    },
                },
                &track.id,
                "track",
                true,
            )?[0]
                .clone()
        };
        bindings.insert(track.id.clone(), stem_id.clone());
        for send in &track.sends {
            builder.add(
                Edit::StemSendSet {
                    stem: stem_id.clone(),
                    send: send.clone(),
                },
                &stem_id,
                "sends",
                false,
            )?;
        }
        for clip in &track.clips {
            let (start, frames, error) = clip_time(clip, &cues, rate)?;
            errors.insert(clip.id.clone(), error);
            let source = match &clip.material {
                Material::Sample { sample, .. } => ClipSource::Sample {
                    sample: sample.clone(),
                },
                Material::Synth {
                    synth,
                    midi_note,
                    velocity_permille,
                } => ClipSource::Synth {
                    synth: synth.clone(),
                    midi_note: *midi_note,
                    velocity: Permille(*velocity_permille),
                },
                Material::SoundEffect { preset, seed } => {
                    ensure(
                        clip.source_offset_frames == 0,
                        "SFX source offsets require explicit re-render/pre-roll",
                    )?;
                    let mut synth = domain(presets::synth_for(
                        *preset,
                        "authoring",
                        project.profile.sample_rate,
                        frames,
                        *seed,
                    ))?;
                    let unique = format!(
                        "s{}",
                        &canonical_digest(&(intent.id.clone(), clip.id.clone()))?.as_str()[..16]
                    );
                    let names: BTreeMap<_, _> = synth
                        .signals
                        .iter()
                        .enumerate()
                        .map(|(index, signal)| (signal.id.clone(), format!("{unique}_{index}")))
                        .collect();
                    for signal in &mut synth.signals {
                        signal.id = names[&signal.id].clone();
                        for input in &mut signal.inputs {
                            *input = names[input].clone();
                        }
                    }
                    synth.output = names[&synth.output].clone();
                    let id = builder.add(Edit::SynthCreate { synth }, &clip.id, "synth", true)?[0]
                        .clone();
                    ClipSource::Synth {
                        synth: id,
                        midi_note: 69,
                        velocity: Permille(1000),
                    }
                }
            };
            let end = clip
                .source_offset_frames
                .checked_add(frames)
                .ok_or_else(|| ContractError::Limit("source-range overflow".into()))?;
            let new = AudioClip {
                id: clip.id.clone(),
                name: clip.id.clone(),
                source,
                start: SampleFrame(start),
                source_range: domain(SampleRange::new(clip.source_offset_frames, end))?,
                gain: clip.gain,
                fade_in_frames: clip.fade_in_frames,
                fade_out_frames: clip.fade_out_frames,
            };
            let id = builder.add(
                Edit::ClipInsert {
                    stem: stem_id.clone(),
                    clip: new,
                },
                &clip.id,
                "clip",
                true,
            )?[0]
                .clone();
            bindings.insert(clip.id.clone(), id);
        }
    }
    for duck in &intent.ducking {
        let stem_id = bindings
            .get(&duck.track)
            .ok_or_else(|| ContractError::Invalid("ducking target missing".into()))?
            .clone();
        let stem = domain(builder.project.stem(&stem_id))?;
        let existing: Vec<String> = stem
            .automations
            .iter()
            .filter(|a| a.target == AutomationTarget::StemGain)
            .map(|a| a.id.clone())
            .collect();
        ensure(
            existing.is_empty() || duck.replace_existing_gain_automation,
            "ducking would replace existing gain automation without permission",
        )?;
        let automation = ducking_curve(duck, stem.gain, rate)?;
        for id in existing {
            builder.add(
                Edit::AutomationRemove {
                    owner: AutomationOwner::Stem {
                        stem: stem_id.clone(),
                    },
                    automation: id,
                },
                &stem_id,
                "gain_automation",
                false,
            )?;
        }
        builder.add(
            Edit::AutomationSet {
                owner: AutomationOwner::Stem {
                    stem: stem_id.clone(),
                },
                automation,
            },
            &stem_id,
            "gain_automation",
            false,
        )?;
    }
    let result_digest = Digest::parse(domain(builder.project.semantic_digest())?)?;
    ensure(
        !intent.dependencies.contains_key("effects.contract"),
        "client may not select the effect verification contract",
    )?;
    let contract = crate::effects::contract(
        &base,
        &builder.project.master_bus,
        &intent.delivery,
        &result_digest,
        &builder.operations,
    )?;
    let mut dependencies = intent.dependencies.clone();
    dependencies.insert("effects.contract".into(), contract.digest()?);
    let mut scope: Vec<Address> = builder
        .operations
        .iter()
        .flat_map(|op| op.writes.clone())
        .collect();
    scope.push(address(
        &base,
        &builder.project.master_bus,
        "decoded_master",
    )?);
    let prepared = PreparedPlan::prepare(
        PlanBody {
            contract_version: CONTRACT_VERSION,
            profile: profile.identity.clone(),
            owner,
            base: base.clone(),
            intent_digest: canonical_digest(&intent)?,
            dependencies,
            required_rules: profile.required_rules.clone(),
            budget: intent.budget.clone(),
            intent,
            changes: ChangeSet {
                operations: builder.operations,
                atomicity: Atomicity::InMemoryTransaction,
            },
            observation_scope: scope,
            require_compare_and_swap: false,
        },
        profile,
    )?;
    Ok(PlannedAudio {
        plan: prepared,
        logical_bindings: bindings,
        quantization_errors: errors,
        resulting_model_digest: result_digest,
        affected: builder.affected,
    })
}
pub fn replay(
    project: &AudioProject,
    planned: &PlannedAudio,
    profile: &ProfileDescriptor,
) -> Result<AudioProject> {
    planned.plan.verify(profile)?;
    let mut current = project.clone();
    for operation in &planned.plan.body.changes.operations {
        ensure(
            matches!(
                operation.payload,
                Edit::StemCreate { .. }
                    | Edit::StemGainSet { .. }
                    | Edit::StemPanSet { .. }
                    | Edit::StemSendSet { .. }
                    | Edit::StemRoute { .. }
                    | Edit::SynthCreate { .. }
                    | Edit::ClipInsert { .. }
                    | Edit::AutomationSet { .. }
                    | Edit::AutomationRemove { .. }
                    | Edit::BusGainSet { .. }
            ),
            "operation is outside the audio authoring allowlist",
        )?;
        current = domain(edit::apply(
            &current,
            &domain(current.semantic_digest())?,
            operation.payload.clone(),
            &operation.id,
        ))?
        .result;
    }
    ensure(
        Digest::parse(domain(current.semantic_digest())?)? == planned.resulting_model_digest,
        "audio plan derived-state mismatch",
    )?;
    Ok(current)
}
pub fn ducking_curve(duck: &DuckingIntent, base_gain: MilliDb, rate: Rate) -> Result<Automation> {
    let ducked = base_gain
        .0
        .checked_add(duck.attenuation.0)
        .ok_or_else(|| ContractError::Limit("duck gain overflow".into()))?;
    domain(MilliDb::new(ducked))?;
    let mut ranges = duck
        .foreground
        .iter()
        .map(|range| Ok((quantize(range.start, rate)?.0, quantize(range.end, rate)?.0)))
        .collect::<Result<Vec<_>>>()?;
    ranges.sort_unstable();
    let mut merged: Vec<(u64, u64)> = vec![];
    for (start, end) in ranges {
        ensure(start < end, "duck interval quantizes to zero samples")?;
        if let Some(last) = merged.last_mut()
            && start <= last.1.saturating_add(duck.merge_gap_frames)
        {
            last.1 = last.1.max(end);
        } else {
            merged.push((start, end));
        }
    }
    let mut points = BTreeMap::new();
    points.insert(0, (base_gain.0, AutomationCurve::Hold));
    for (index, &(start, end)) in merged.iter().enumerate() {
        let attack = start.saturating_sub(duck.attack_frames);
        if let Some(&(_, last_end)) = index.checked_sub(1).and_then(|i| merged.get(i)) {
            ensure(
                last_end.saturating_add(duck.release_frames) <= attack,
                "duck ramps overlap; increase explicit merge gap or shorten ramps",
            )?;
        }
        if attack < start {
            points.insert(attack, (base_gain.0, AutomationCurve::Linear));
        }
        points.insert(start, (ducked, AutomationCurve::Hold));
        let release_end = end
            .checked_add(duck.release_frames)
            .ok_or_else(|| ContractError::Limit("duck release overflow".into()))?;
        ensure(
            release_end <= MAX_SAMPLE_FRAME,
            "duck release exceeds project budget",
        )?;
        if release_end > end {
            points.insert(end, (ducked, AutomationCurve::Linear));
        }
        points.insert(release_end, (base_gain.0, AutomationCurve::Hold));
    }
    Ok(Automation {
        id: duck.id.clone(),
        target: AutomationTarget::StemGain,
        points: points
            .into_iter()
            .map(|(frame, (value, curve))| AutomationPoint {
                frame: SampleFrame(frame),
                value_milli: i64::from(value),
                curve,
            })
            .collect(),
    })
}
