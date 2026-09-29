//! Backend-neutral audio operation identities and support classification.
//!
//! Support is descriptive capability, never authority. Semwright policy and
//! consent remain above every concrete audio backend or asset provider.

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use std::{fmt, str::FromStr};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AudioOperation {
    ProjectProfileSet,
    ProjectMetadataSet,
    SampleImport,
    SampleRemove,
    SynthCreate,
    SynthRemove,
    SignalAdd,
    SignalRemove,
    SignalConnect,
    SignalDisconnect,
    StemCreate,
    StemRemove,
    StemRename,
    StemMute,
    StemSolo,
    StemGainSet,
    StemPanSet,
    StemSendSet,
    StemReorder,
    GroupSet,
    GroupRemove,
    ClipInsert,
    ClipMove,
    ClipTrim,
    ClipSlip,
    ClipSplit,
    ClipFadeSet,
    ClipRemove,
    ClipDuplicate,
    EffectAdd,
    EffectReplace,
    EffectRemove,
    EffectEnable,
    EffectDisable,
    AutomationSet,
    AutomationRemove,
    BusCreate,
    BusRemove,
    BusGainSet,
    BusPanSet,
    BusSendSet,
    BusReorder,
    StemRoute,
    MarkerSet,
    MarkerRemove,
    RangeSet,
    RangeRemove,
    TempoMapSet,
    MidiPhraseSet,
    MidiPhraseRemove,
    SfxPresetMaterialize,
    RenderPlan,
    RenderStart,
    AnalysisInspect,
}

impl AudioOperation {
    pub const ALL: &'static [Self] = &[
        Self::ProjectProfileSet,
        Self::ProjectMetadataSet,
        Self::SampleImport,
        Self::SampleRemove,
        Self::SynthCreate,
        Self::SynthRemove,
        Self::SignalAdd,
        Self::SignalRemove,
        Self::SignalConnect,
        Self::SignalDisconnect,
        Self::StemCreate,
        Self::StemRemove,
        Self::StemRename,
        Self::StemMute,
        Self::StemSolo,
        Self::StemGainSet,
        Self::StemPanSet,
        Self::StemSendSet,
        Self::StemReorder,
        Self::GroupSet,
        Self::GroupRemove,
        Self::ClipInsert,
        Self::ClipMove,
        Self::ClipTrim,
        Self::ClipSlip,
        Self::ClipSplit,
        Self::ClipFadeSet,
        Self::ClipRemove,
        Self::ClipDuplicate,
        Self::EffectAdd,
        Self::EffectReplace,
        Self::EffectRemove,
        Self::EffectEnable,
        Self::EffectDisable,
        Self::AutomationSet,
        Self::AutomationRemove,
        Self::BusCreate,
        Self::BusRemove,
        Self::BusGainSet,
        Self::BusPanSet,
        Self::BusSendSet,
        Self::BusReorder,
        Self::StemRoute,
        Self::MarkerSet,
        Self::MarkerRemove,
        Self::RangeSet,
        Self::RangeRemove,
        Self::TempoMapSet,
        Self::MidiPhraseSet,
        Self::MidiPhraseRemove,
        Self::SfxPresetMaterialize,
        Self::RenderPlan,
        Self::RenderStart,
        Self::AnalysisInspect,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProjectProfileSet => "project.profile.set",
            Self::ProjectMetadataSet => "project.metadata.set",
            Self::SampleImport => "sample.import",
            Self::SampleRemove => "sample.remove",
            Self::SynthCreate => "synth.create",
            Self::SynthRemove => "synth.remove",
            Self::SignalAdd => "signal.add",
            Self::SignalRemove => "signal.remove",
            Self::SignalConnect => "signal.connect",
            Self::SignalDisconnect => "signal.disconnect",
            Self::StemCreate => "stem.create",
            Self::StemRemove => "stem.remove",
            Self::StemRename => "stem.rename",
            Self::StemMute => "stem.mute",
            Self::StemSolo => "stem.solo",
            Self::StemGainSet => "stem.gain.set",
            Self::StemPanSet => "stem.pan.set",
            Self::StemSendSet => "stem.send.set",
            Self::StemReorder => "stem.reorder",
            Self::GroupSet => "group.set",
            Self::GroupRemove => "group.remove",
            Self::ClipInsert => "clip.insert",
            Self::ClipMove => "clip.move",
            Self::ClipTrim => "clip.trim",
            Self::ClipSlip => "clip.slip",
            Self::ClipSplit => "clip.split",
            Self::ClipFadeSet => "clip.fade.set",
            Self::ClipRemove => "clip.remove",
            Self::ClipDuplicate => "clip.duplicate",
            Self::EffectAdd => "effect.add",
            Self::EffectReplace => "effect.replace",
            Self::EffectRemove => "effect.remove",
            Self::EffectEnable => "effect.enable",
            Self::EffectDisable => "effect.disable",
            Self::AutomationSet => "automation.set",
            Self::AutomationRemove => "automation.remove",
            Self::BusCreate => "bus.create",
            Self::BusRemove => "bus.remove",
            Self::BusGainSet => "bus.gain.set",
            Self::BusPanSet => "bus.pan.set",
            Self::BusSendSet => "bus.send.set",
            Self::BusReorder => "bus.reorder",
            Self::StemRoute => "stem.route",
            Self::MarkerSet => "marker.set",
            Self::MarkerRemove => "marker.remove",
            Self::RangeSet => "range.set",
            Self::RangeRemove => "range.remove",
            Self::TempoMapSet => "tempo_map.set",
            Self::MidiPhraseSet => "midi_phrase.set",
            Self::MidiPhraseRemove => "midi_phrase.remove",
            Self::SfxPresetMaterialize => "sfx.preset.materialize",
            Self::RenderPlan => "render.plan",
            Self::RenderStart => "render.start",
            Self::AnalysisInspect => "analysis.inspect",
        }
    }
}

impl fmt::Display for AudioOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseAudioOperationError(String);
impl fmt::Display for ParseAudioOperationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown semantic audio operation: {}", self.0)
    }
}
impl std::error::Error for ParseAudioOperationError {}

impl FromStr for AudioOperation {
    type Err = ParseAudioOperationError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let op = Self::ALL
            .iter()
            .copied()
            .find(|op| op.as_str() == value)
            .ok_or_else(|| ParseAudioOperationError(value.to_owned()))?;
        Ok(op)
    }
}

impl Serialize for AudioOperation {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}
impl<'de> Deserialize<'de> for AudioOperation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(D::Error::custom)
    }
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum OperationSupport {
    SafeRoundtrip,
    MetadataRisk,
    RenderOnly,
    Unsupported,
}
impl OperationSupport {
    pub fn allows_mutation(self) -> bool {
        matches!(self, Self::SafeRoundtrip | Self::MetadataRisk)
    }
    pub fn requires_metadata_acknowledgement(self) -> bool {
        self == Self::MetadataRisk
    }
}

impl schemars::JsonSchema for AudioOperation {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "AudioOperation".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type":"string","enum":Self::ALL.iter().map(|op| op.as_str()).collect::<Vec<_>>()})
    }
}
