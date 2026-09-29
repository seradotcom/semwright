//! Deterministic transactional edit engine for backend-neutral audio.

use crate::{
    Error, Result,
    hash::sha256,
    model::{
        AudioClip, AudioProfile, AudioProject, Automation, Bus, BusSend, EffectInstance, Marker,
        MidiPhrase, NamedRange, ProjectMetadata, Sample, Signal, Stem, StemGroup, Synth,
        TempoChange,
    },
    presets::{self, SfxPreset},
    support::AudioOperation,
    time::SampleRange,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChainOwner {
    Stem { stem: String },
    Bus { bus: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AutomationOwner {
    Stem { stem: String },
    Bus { bus: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Edit {
    ProjectProfileSet {
        profile: AudioProfile,
    },
    ProjectMetadataSet {
        metadata: ProjectMetadata,
    },
    SampleImport {
        sample: Sample,
    },
    SampleRemove {
        sample: String,
    },
    SynthCreate {
        synth: Synth,
    },
    SynthRemove {
        synth: String,
    },
    SignalAdd {
        synth: String,
        signal: Signal,
    },
    SignalRemove {
        synth: String,
        signal: String,
    },
    SignalConnect {
        synth: String,
        source: String,
        target: String,
    },
    SignalDisconnect {
        synth: String,
        source: String,
        target: String,
    },
    StemCreate {
        stem: Stem,
    },
    StemRemove {
        stem: String,
    },
    StemRename {
        stem: String,
        name: String,
    },
    StemMute {
        stem: String,
        value: bool,
    },
    StemSolo {
        stem: String,
        value: bool,
    },
    StemGainSet {
        stem: String,
        gain: crate::units::MilliDb,
    },
    StemPanSet {
        stem: String,
        pan_milli: i16,
    },
    StemSendSet {
        stem: String,
        send: BusSend,
    },
    StemReorder {
        stem: String,
        before: Option<String>,
    },
    GroupSet {
        group: StemGroup,
    },
    GroupRemove {
        group: String,
    },
    ClipInsert {
        stem: String,
        clip: AudioClip,
    },
    ClipMove {
        stem: String,
        clip: String,
        start: u64,
    },
    ClipTrim {
        stem: String,
        clip: String,
        source: SampleRange,
    },
    ClipSlip {
        stem: String,
        clip: String,
        source_start: u64,
    },
    ClipSplit {
        stem: String,
        clip: String,
        at: u64,
    },
    ClipFadeSet {
        stem: String,
        clip: String,
        fade_in_frames: u64,
        fade_out_frames: u64,
    },
    ClipRemove {
        stem: String,
        clip: String,
    },
    ClipDuplicate {
        stem: String,
        clip: String,
        start: u64,
    },
    EffectAdd {
        owner: ChainOwner,
        effect: EffectInstance,
    },
    EffectReplace {
        owner: ChainOwner,
        effect: String,
        replacement: crate::model::Effect,
    },
    EffectRemove {
        owner: ChainOwner,
        effect: String,
    },
    EffectEnable {
        owner: ChainOwner,
        effect: String,
        value: bool,
    },
    AutomationSet {
        owner: AutomationOwner,
        automation: Automation,
    },
    AutomationRemove {
        owner: AutomationOwner,
        automation: String,
    },
    BusCreate {
        bus: Bus,
    },
    BusRemove {
        bus: String,
    },
    BusGainSet {
        bus: String,
        gain: crate::units::MilliDb,
    },
    BusPanSet {
        bus: String,
        pan_milli: i16,
    },
    BusSendSet {
        bus: String,
        send: BusSend,
    },
    BusReorder {
        bus: String,
        before: Option<String>,
    },
    StemRoute {
        stem: String,
        bus: String,
    },
    MarkerSet {
        marker: Marker,
    },
    MarkerRemove {
        marker: String,
    },
    RangeSet {
        range: NamedRange,
    },
    RangeRemove {
        range: String,
    },
    TempoMapSet {
        changes: Vec<TempoChange>,
    },
    MidiPhraseSet {
        phrase: MidiPhrase,
    },
    MidiPhraseRemove {
        phrase: String,
    },
    SfxPresetMaterialize {
        preset: SfxPreset,
        duration_frames: u64,
        seed: u64,
    },
}

impl Edit {
    pub const fn operation(&self) -> AudioOperation {
        match self {
            Self::ProjectProfileSet { .. } => AudioOperation::ProjectProfileSet,
            Self::ProjectMetadataSet { .. } => AudioOperation::ProjectMetadataSet,
            Self::SampleImport { .. } => AudioOperation::SampleImport,
            Self::SampleRemove { .. } => AudioOperation::SampleRemove,
            Self::SynthCreate { .. } => AudioOperation::SynthCreate,
            Self::SynthRemove { .. } => AudioOperation::SynthRemove,
            Self::SignalAdd { .. } => AudioOperation::SignalAdd,
            Self::SignalRemove { .. } => AudioOperation::SignalRemove,
            Self::SignalConnect { .. } => AudioOperation::SignalConnect,
            Self::SignalDisconnect { .. } => AudioOperation::SignalDisconnect,
            Self::StemCreate { .. } => AudioOperation::StemCreate,
            Self::StemRemove { .. } => AudioOperation::StemRemove,
            Self::StemRename { .. } => AudioOperation::StemRename,
            Self::StemMute { .. } => AudioOperation::StemMute,
            Self::StemSolo { .. } => AudioOperation::StemSolo,
            Self::StemGainSet { .. } => AudioOperation::StemGainSet,
            Self::StemPanSet { .. } => AudioOperation::StemPanSet,
            Self::StemSendSet { .. } => AudioOperation::StemSendSet,
            Self::StemReorder { .. } => AudioOperation::StemReorder,
            Self::GroupSet { .. } => AudioOperation::GroupSet,
            Self::GroupRemove { .. } => AudioOperation::GroupRemove,
            Self::ClipInsert { .. } => AudioOperation::ClipInsert,
            Self::ClipMove { .. } => AudioOperation::ClipMove,
            Self::ClipTrim { .. } => AudioOperation::ClipTrim,
            Self::ClipSlip { .. } => AudioOperation::ClipSlip,
            Self::ClipSplit { .. } => AudioOperation::ClipSplit,
            Self::ClipFadeSet { .. } => AudioOperation::ClipFadeSet,
            Self::ClipRemove { .. } => AudioOperation::ClipRemove,
            Self::ClipDuplicate { .. } => AudioOperation::ClipDuplicate,
            Self::EffectAdd { .. } => AudioOperation::EffectAdd,
            Self::EffectReplace { .. } => AudioOperation::EffectReplace,
            Self::EffectRemove { .. } => AudioOperation::EffectRemove,
            Self::EffectEnable { value: true, .. } => AudioOperation::EffectEnable,
            Self::EffectEnable { value: false, .. } => AudioOperation::EffectDisable,
            Self::AutomationSet { .. } => AudioOperation::AutomationSet,
            Self::AutomationRemove { .. } => AudioOperation::AutomationRemove,
            Self::BusCreate { .. } => AudioOperation::BusCreate,
            Self::BusRemove { .. } => AudioOperation::BusRemove,
            Self::BusGainSet { .. } => AudioOperation::BusGainSet,
            Self::BusPanSet { .. } => AudioOperation::BusPanSet,
            Self::BusSendSet { .. } => AudioOperation::BusSendSet,
            Self::BusReorder { .. } => AudioOperation::BusReorder,
            Self::StemRoute { .. } => AudioOperation::StemRoute,
            Self::MarkerSet { .. } => AudioOperation::MarkerSet,
            Self::MarkerRemove { .. } => AudioOperation::MarkerRemove,
            Self::RangeSet { .. } => AudioOperation::RangeSet,
            Self::RangeRemove { .. } => AudioOperation::RangeRemove,
            Self::TempoMapSet { .. } => AudioOperation::TempoMapSet,
            Self::MidiPhraseSet { .. } => AudioOperation::MidiPhraseSet,
            Self::MidiPhraseRemove { .. } => AudioOperation::MidiPhraseRemove,
            Self::SfxPresetMaterialize { .. } => AudioOperation::SfxPresetMaterialize,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditOutcome {
    pub operation: AudioOperation,
    pub result: AudioProject,
    pub affected: Vec<String>,
    pub created: Vec<String>,
    pub before_frames: u64,
    pub after_frames: u64,
}

pub fn revision(project: &AudioProject) -> Result<String> {
    project.semantic_digest()
}

pub fn apply(
    project: &AudioProject,
    expected_revision: &str,
    edit: Edit,
    seed: &str,
) -> Result<EditOutcome> {
    let actual = revision(project)?;
    if actual != expected_revision {
        return Err(Error::stale());
    }
    apply_with_identity_base(project, edit, seed, &actual)
}

pub fn apply_with_identity_base(
    project: &AudioProject,
    edit: Edit,
    seed: &str,
    identity_base: &str,
) -> Result<EditOutcome> {
    project.validate()?;
    if seed.is_empty() || seed.len() > 256 || seed.chars().any(char::is_control) {
        return Err(Error::invalid("Invalid audio edit identity seed"));
    }
    let operation = edit.operation();
    let before_frames = project.duration();
    let mut result = project.clone();
    let mut affected = Vec::new();
    let mut created = Vec::new();
    let new_id = |prefix: &str| {
        format!(
            "{prefix}_{}",
            &sha256(format!("{identity_base}:{seed}:{prefix}").as_bytes())[..24]
        )
    };

    match edit {
        Edit::ProjectProfileSet { profile } => {
            profile.validate()?;
            result.profile = profile;
            affected.push("profile".into());
        }
        Edit::ProjectMetadataSet { metadata } => {
            result.metadata = metadata;
            affected.push("metadata".into());
        }
        Edit::SampleImport { mut sample } => {
            sample.id = new_id("sample");
            created.push(sample.id.clone());
            result.samples.insert(sample.id.clone(), sample);
        }
        Edit::SampleRemove { sample } => {
            if result.samples.remove(&sample).is_none() {
                return Err(not_found("sample"));
            }
            affected.push(sample);
        }
        Edit::SynthCreate { mut synth } => {
            synth.id = new_id("synth");
            created.push(synth.id.clone());
            result.synths.insert(synth.id.clone(), synth);
        }
        Edit::SynthRemove { synth } => {
            if result.synths.remove(&synth).is_none() {
                return Err(not_found("synth"));
            }
            affected.push(synth);
        }
        Edit::SignalAdd { synth, mut signal } => {
            let synth_ref = result
                .synths
                .get_mut(&synth)
                .ok_or_else(|| not_found("synth"))?;
            signal.id = new_id("signal");
            created.push(signal.id.clone());
            synth_ref.signals.push(signal);
            affected.push(synth);
        }
        Edit::SignalRemove { synth, signal } => {
            let synth_ref = result
                .synths
                .get_mut(&synth)
                .ok_or_else(|| not_found("synth"))?;
            let old = synth_ref.signals.len();
            synth_ref.signals.retain(|value| value.id != signal);
            if old == synth_ref.signals.len() {
                return Err(not_found("signal"));
            }
            affected.extend([synth, signal]);
        }
        Edit::SignalConnect {
            synth,
            source,
            target,
        } => {
            let synth_ref = result
                .synths
                .get_mut(&synth)
                .ok_or_else(|| not_found("synth"))?;
            if !synth_ref.signals.iter().any(|value| value.id == source) {
                return Err(not_found("source signal"));
            }
            let target_ref = synth_ref
                .signals
                .iter_mut()
                .find(|value| value.id == target)
                .ok_or_else(|| not_found("target signal"))?;
            if !target_ref.inputs.contains(&source) {
                target_ref.inputs.push(source.clone());
            }
            affected.extend([synth, source, target]);
        }
        Edit::SignalDisconnect {
            synth,
            source,
            target,
        } => {
            let synth_ref = result
                .synths
                .get_mut(&synth)
                .ok_or_else(|| not_found("synth"))?;
            let target_ref = synth_ref
                .signals
                .iter_mut()
                .find(|value| value.id == target)
                .ok_or_else(|| not_found("target signal"))?;
            let old = target_ref.inputs.len();
            target_ref.inputs.retain(|value| value != &source);
            if old == target_ref.inputs.len() {
                return Err(Error::invalid("Signal edge does not exist"));
            }
            affected.extend([synth, source, target]);
        }
        Edit::StemCreate { mut stem } => {
            stem.id = new_id("stem");
            for (index, clip) in stem.clips.iter_mut().enumerate() {
                clip.id = format!("{}_clip_{index}", stem.id);
            }
            created.push(stem.id.clone());
            result.stems.push(stem);
        }
        Edit::StemRemove { stem } => {
            if result
                .groups
                .iter()
                .any(|group| group.stems.iter().any(|member| member == &stem))
            {
                return Err(Error::new(
                    "Conflict",
                    "Audio stem is still referenced by a group",
                ));
            }
            let old = result.stems.len();
            result.stems.retain(|value| value.id != stem);
            if old == result.stems.len() {
                return Err(not_found("stem"));
            }
            affected.push(stem);
        }
        Edit::StemRename { stem, name } => {
            validate_name(&name)?;
            result.stem_mut(&stem)?.name = name;
            affected.push(stem);
        }
        Edit::StemMute { stem, value } => {
            result.stem_mut(&stem)?.muted = value;
            affected.push(stem);
        }
        Edit::StemSolo { stem, value } => {
            result.stem_mut(&stem)?.soloed = value;
            affected.push(stem);
        }
        Edit::StemGainSet { stem, gain } => {
            crate::units::MilliDb::new(gain.0)?;
            result.stem_mut(&stem)?.gain = gain;
            affected.push(stem);
        }
        Edit::StemPanSet { stem, pan_milli } => {
            if !(-1000..=1000).contains(&pan_milli) {
                return Err(Error::invalid("Stem pan is outside semantic bounds"));
            }
            result.stem_mut(&stem)?.pan_milli = pan_milli;
            affected.push(stem);
        }
        Edit::StemSendSet { stem, send } => {
            let stem_ref = result.stem_mut(&stem)?;
            if let Some(existing) = stem_ref
                .sends
                .iter_mut()
                .find(|value| value.target_bus == send.target_bus)
            {
                *existing = send;
            } else {
                stem_ref.sends.push(send);
            }
            affected.push(stem);
        }
        Edit::StemReorder { stem, before } => {
            reorder_stem(&mut result.stems, &stem, before.as_deref())?;
            affected.push(stem);
        }
        Edit::GroupSet { mut group } => {
            if group.id.is_empty() {
                group.id = new_id("group");
                created.push(group.id.clone());
                result.groups.push(group);
            } else if let Some(existing) = result.groups.iter_mut().find(|item| item.id == group.id)
            {
                let id = existing.id.clone();
                *existing = group;
                existing.id = id;
            } else {
                created.push(group.id.clone());
                result.groups.push(group);
            }
            affected.push("groups".into());
        }
        Edit::GroupRemove { group } => {
            let old = result.groups.len();
            result.groups.retain(|value| value.id != group);
            if old == result.groups.len() {
                return Err(not_found("group"));
            }
            affected.push(group);
        }
        Edit::ClipInsert { stem, mut clip } => {
            clip.id = new_id("clip");
            created.push(clip.id.clone());
            result.stem_mut(&stem)?.clips.push(clip);
            affected.push(stem);
        }
        Edit::ClipMove { stem, clip, start } => {
            let target = result
                .stem_mut(&stem)?
                .clips
                .iter_mut()
                .find(|value| value.id == clip)
                .ok_or_else(|| not_found("clip"))?;
            target.start.0 = start;
            affected.extend([stem, clip]);
        }
        Edit::ClipTrim { stem, clip, source } => {
            SampleRange::new(source.start.0, source.end.0)?;
            let target = result
                .stem_mut(&stem)?
                .clips
                .iter_mut()
                .find(|value| value.id == clip)
                .ok_or_else(|| not_found("clip"))?;
            target.source_range = source;
            if target.fade_in_frames + target.fade_out_frames > target.duration() {
                target.fade_in_frames = 0;
                target.fade_out_frames = 0;
            }
            affected.extend([stem, clip]);
        }
        Edit::ClipSlip {
            stem,
            clip,
            source_start,
        } => {
            let target = result
                .stem_mut(&stem)?
                .clips
                .iter_mut()
                .find(|value| value.id == clip)
                .ok_or_else(|| not_found("clip"))?;
            if !matches!(target.source, crate::model::ClipSource::Sample { .. }) {
                return Err(Error::unsupported(
                    "Slip editing is defined only for sample-backed clips",
                ));
            }
            let end = source_start
                .checked_add(target.duration())
                .ok_or_else(|| Error::limit("Clip slip source range overflows"))?;
            target.source_range = SampleRange::new(source_start, end)?;
            affected.extend([stem, clip]);
        }
        Edit::ClipSplit { stem, clip, at } => {
            let stem_ref = result.stem_mut(&stem)?;
            let index = stem_ref
                .clips
                .iter()
                .position(|value| value.id == clip)
                .ok_or_else(|| not_found("clip"))?;
            let original = stem_ref.clips[index].clone();
            let end = original.end()?;
            if at <= original.start.0 || at >= end {
                return Err(Error::invalid(
                    "Clip split must be strictly inside the clip",
                ));
            }
            let offset = at - original.start.0;
            let source_split = original
                .source_range
                .start
                .0
                .checked_add(offset)
                .ok_or_else(|| Error::limit("Clip split source position overflows"))?;
            let mut left = original.clone();
            left.source_range = SampleRange::new(original.source_range.start.0, source_split)?;
            left.fade_out_frames = 0;
            let mut right = original;
            right.id = new_id("clip");
            right.start.0 = at;
            right.source_range = SampleRange::new(source_split, right.source_range.end.0)?;
            right.fade_in_frames = 0;
            stem_ref.clips[index] = left;
            stem_ref.clips.insert(index + 1, right.clone());
            created.push(right.id);
            affected.extend([stem, clip]);
        }
        Edit::ClipFadeSet {
            stem,
            clip,
            fade_in_frames,
            fade_out_frames,
        } => {
            let target = result
                .stem_mut(&stem)?
                .clips
                .iter_mut()
                .find(|value| value.id == clip)
                .ok_or_else(|| not_found("clip"))?;
            if fade_in_frames
                .checked_add(fade_out_frames)
                .is_none_or(|sum| sum > target.duration())
            {
                return Err(Error::invalid("Clip fades exceed clip duration"));
            }
            target.fade_in_frames = fade_in_frames;
            target.fade_out_frames = fade_out_frames;
            affected.extend([stem, clip]);
        }
        Edit::ClipRemove { stem, clip } => {
            let stem_ref = result.stem_mut(&stem)?;
            let old = stem_ref.clips.len();
            stem_ref.clips.retain(|value| value.id != clip);
            if old == stem_ref.clips.len() {
                return Err(not_found("clip"));
            }
            affected.extend([stem, clip]);
        }
        Edit::ClipDuplicate { stem, clip, start } => {
            let mut duplicate = result
                .stem(&stem)?
                .clips
                .iter()
                .find(|value| value.id == clip)
                .cloned()
                .ok_or_else(|| not_found("clip"))?;
            duplicate.id = new_id("clip");
            duplicate.start.0 = start;
            created.push(duplicate.id.clone());
            result.stem_mut(&stem)?.clips.push(duplicate);
            affected.push(stem);
        }
        Edit::EffectAdd { owner, mut effect } => {
            effect.id = new_id("effect");
            created.push(effect.id.clone());
            chain_mut(&mut result, &owner)?.effects.push(effect);
            affected.push(owner_id(&owner).to_owned());
        }
        Edit::EffectReplace {
            owner,
            effect,
            replacement,
        } => {
            let target = chain_mut(&mut result, &owner)?
                .effects
                .iter_mut()
                .find(|value| value.id == effect)
                .ok_or_else(|| not_found("effect"))?;
            target.effect = replacement;
            affected.extend([owner_id(&owner).to_owned(), effect]);
        }
        Edit::EffectRemove { owner, effect } => {
            let chain = chain_mut(&mut result, &owner)?;
            let old = chain.effects.len();
            chain.effects.retain(|value| value.id != effect);
            if old == chain.effects.len() {
                return Err(not_found("effect"));
            }
            affected.extend([owner_id(&owner).to_owned(), effect]);
        }
        Edit::EffectEnable {
            owner,
            effect,
            value,
        } => {
            let target = chain_mut(&mut result, &owner)?
                .effects
                .iter_mut()
                .find(|item| item.id == effect)
                .ok_or_else(|| not_found("effect"))?;
            target.enabled = value;
            affected.extend([owner_id(&owner).to_owned(), effect]);
        }
        Edit::AutomationSet {
            owner,
            mut automation,
        } => {
            let values = automations_mut(&mut result, &owner)?;
            if automation.id.is_empty() {
                automation.id = new_id("automation");
                created.push(automation.id.clone());
                values.push(automation);
            } else if let Some(target) = values.iter_mut().find(|item| item.id == automation.id) {
                let id = target.id.clone();
                *target = automation;
                target.id = id;
            } else {
                created.push(automation.id.clone());
                values.push(automation);
            }
            affected.push(automation_owner_id(&owner).to_owned());
        }
        Edit::AutomationRemove { owner, automation } => {
            let values = automations_mut(&mut result, &owner)?;
            let old = values.len();
            values.retain(|value| value.id != automation);
            if old == values.len() {
                return Err(not_found("automation"));
            }
            affected.extend([automation_owner_id(&owner).to_owned(), automation]);
        }
        Edit::BusCreate { mut bus } => {
            bus.id = new_id("bus");
            created.push(bus.id.clone());
            result.buses.push(bus);
        }
        Edit::BusRemove { bus } => {
            if bus == result.master_bus {
                return Err(Error::invalid("Master bus cannot be removed"));
            }
            if result.stems.iter().any(|stem| {
                stem.output_bus == bus || stem.sends.iter().any(|send| send.target_bus == bus)
            }) || result
                .buses
                .iter()
                .any(|candidate| candidate.sends.iter().any(|send| send.target_bus == bus))
            {
                return Err(Error::new(
                    "Conflict",
                    "Audio bus is still referenced by a stem or send",
                ));
            }
            let old = result.buses.len();
            result.buses.retain(|value| value.id != bus);
            if old == result.buses.len() {
                return Err(not_found("bus"));
            }
            affected.push(bus);
        }
        Edit::BusGainSet { bus, gain } => {
            crate::units::MilliDb::new(gain.0)?;
            let target = result
                .buses
                .iter_mut()
                .find(|value| value.id == bus)
                .ok_or_else(|| not_found("bus"))?;
            target.gain = gain;
            affected.push(bus);
        }
        Edit::BusPanSet { bus, pan_milli } => {
            if !(-1000..=1000).contains(&pan_milli) {
                return Err(Error::invalid("Bus pan is outside semantic bounds"));
            }
            let target = result
                .buses
                .iter_mut()
                .find(|value| value.id == bus)
                .ok_or_else(|| not_found("bus"))?;
            target.pan_milli = pan_milli;
            affected.push(bus);
        }
        Edit::BusSendSet { bus, send } => {
            let bus_ref = result
                .buses
                .iter_mut()
                .find(|value| value.id == bus)
                .ok_or_else(|| not_found("bus"))?;
            if let Some(existing) = bus_ref
                .sends
                .iter_mut()
                .find(|value| value.target_bus == send.target_bus)
            {
                *existing = send;
            } else {
                bus_ref.sends.push(send);
            }
            affected.push(bus);
        }
        Edit::BusReorder { bus, before } => {
            reorder_bus(&mut result.buses, &bus, before.as_deref())?;
            affected.push(bus);
        }
        Edit::StemRoute { stem, bus } => {
            result.stem_mut(&stem)?.output_bus = bus;
            affected.push(stem);
        }
        Edit::MarkerSet { mut marker } => {
            if marker.id.is_empty() {
                marker.id = new_id("marker");
                created.push(marker.id.clone());
                result.markers.push(marker);
            } else if let Some(existing) =
                result.markers.iter_mut().find(|item| item.id == marker.id)
            {
                let id = existing.id.clone();
                *existing = marker;
                existing.id = id;
            } else {
                created.push(marker.id.clone());
                result.markers.push(marker);
            }
            affected.push("markers".into());
        }
        Edit::MarkerRemove { marker } => {
            let old = result.markers.len();
            result.markers.retain(|value| value.id != marker);
            if old == result.markers.len() {
                return Err(not_found("marker"));
            }
            affected.push(marker);
        }
        Edit::RangeSet { mut range } => {
            if range.id.is_empty() {
                range.id = new_id("range");
                created.push(range.id.clone());
                result.ranges.push(range);
            } else if let Some(existing) = result.ranges.iter_mut().find(|item| item.id == range.id)
            {
                let id = existing.id.clone();
                *existing = range;
                existing.id = id;
            } else {
                created.push(range.id.clone());
                result.ranges.push(range);
            }
            affected.push("ranges".into());
        }
        Edit::RangeRemove { range } => {
            let old = result.ranges.len();
            result.ranges.retain(|value| value.id != range);
            if old == result.ranges.len() {
                return Err(not_found("range"));
            }
            affected.push(range);
        }
        Edit::TempoMapSet { changes } => {
            result.tempo_changes = changes;
            affected.push("tempo_map".into());
        }
        Edit::MidiPhraseSet { mut phrase } => {
            if phrase.id.is_empty() {
                phrase.id = new_id("midi_phrase");
                created.push(phrase.id.clone());
                result.midi_phrases.push(phrase);
            } else if let Some(existing) = result
                .midi_phrases
                .iter_mut()
                .find(|item| item.id == phrase.id)
            {
                let id = existing.id.clone();
                *existing = phrase;
                existing.id = id;
            } else {
                created.push(phrase.id.clone());
                result.midi_phrases.push(phrase);
            }
            affected.push("midi_phrases".into());
        }
        Edit::MidiPhraseRemove { phrase } => {
            let old = result.midi_phrases.len();
            result.midi_phrases.retain(|value| value.id != phrase);
            if old == result.midi_phrases.len() {
                return Err(not_found("MIDI phrase"));
            }
            affected.push(phrase);
        }
        Edit::SfxPresetMaterialize {
            preset,
            duration_frames,
            seed: preset_seed,
        } => {
            if duration_frames == 0 {
                return Err(Error::invalid("SFX preset duration must be non-zero"));
            }
            let id = new_id("synth");
            let synth = presets::synth_for(
                preset,
                id.clone(),
                result.profile.sample_rate,
                duration_frames,
                preset_seed,
            )?;
            result.synths.insert(id.clone(), synth);
            created.push(id);
        }
    }

    result.validate()?;
    Ok(EditOutcome {
        operation,
        before_frames,
        after_frames: result.duration(),
        result,
        affected,
        created,
    })
}

fn reorder_stem(values: &mut Vec<Stem>, id: &str, before: Option<&str>) -> Result<()> {
    let index = values
        .iter()
        .position(|value| value.id == id)
        .ok_or_else(|| not_found("stem"))?;
    if before == Some(id) {
        return Err(Error::invalid("Stem cannot be ordered before itself"));
    }
    let value = values.remove(index);
    let target = match before {
        Some(before) => values
            .iter()
            .position(|candidate| candidate.id == before)
            .ok_or_else(|| not_found("stem order target"))?,
        None => values.len(),
    };
    values.insert(target, value);
    Ok(())
}

fn reorder_bus(values: &mut Vec<Bus>, id: &str, before: Option<&str>) -> Result<()> {
    let index = values
        .iter()
        .position(|value| value.id == id)
        .ok_or_else(|| not_found("bus"))?;
    if before == Some(id) {
        return Err(Error::invalid("Bus cannot be ordered before itself"));
    }
    let value = values.remove(index);
    let target = match before {
        Some(before) => values
            .iter()
            .position(|candidate| candidate.id == before)
            .ok_or_else(|| not_found("bus order target"))?,
        None => values.len(),
    };
    values.insert(target, value);
    Ok(())
}

fn chain_mut<'a>(
    project: &'a mut AudioProject,
    owner: &ChainOwner,
) -> Result<&'a mut crate::model::EffectChain> {
    match owner {
        ChainOwner::Stem { stem } => Ok(&mut project.stem_mut(stem)?.effects),
        ChainOwner::Bus { bus } => project
            .buses
            .iter_mut()
            .find(|value| value.id == *bus)
            .map(|value| &mut value.effects)
            .ok_or_else(|| not_found("bus")),
    }
}
fn automations_mut<'a>(
    project: &'a mut AudioProject,
    owner: &AutomationOwner,
) -> Result<&'a mut Vec<Automation>> {
    match owner {
        AutomationOwner::Stem { stem } => Ok(&mut project.stem_mut(stem)?.automations),
        AutomationOwner::Bus { bus } => project
            .buses
            .iter_mut()
            .find(|value| value.id == *bus)
            .map(|value| &mut value.automations)
            .ok_or_else(|| not_found("bus")),
    }
}
fn owner_id(owner: &ChainOwner) -> &str {
    match owner {
        ChainOwner::Stem { stem } => stem,
        ChainOwner::Bus { bus } => bus,
    }
}
fn automation_owner_id(owner: &AutomationOwner) -> &str {
    match owner {
        AutomationOwner::Stem { stem } => stem,
        AutomationOwner::Bus { bus } => bus,
    }
}
fn not_found(kind: &str) -> Error {
    Error::new("NotFound", format!("Audio {kind} not found"))
}
fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > 4096 || name.chars().any(char::is_control) {
        return Err(Error::invalid("Invalid audio object name"));
    }
    Ok(())
}
