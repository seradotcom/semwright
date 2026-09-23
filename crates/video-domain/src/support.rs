//! Backend-neutral operation identity and mutation support.
//!
//! These types describe semantic video capability, never authority. Policy,
//! consent, native revision checks and backend preconditions remain outside
//! this crate.

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use std::{fmt, str::FromStr};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VideoOperation {
    ProjectProfileSet,
    SequenceCreate,
    AssetImport,
    AssetRelink,
    TrackCreate,
    TrackRemove,
    TrackRename,
    TrackMute,
    TrackHide,
    TrackReorder,
    ClipInsert,
    ClipMove,
    ClipTrim,
    ClipSplit,
    ClipRemove,
    ClipDuplicate,
    TransitionAdd,
    TransitionPatch,
    TransitionRemove,
    EffectAdd,
    EffectPatch,
    EffectRemove,
    EffectEnable,
    EffectDisable,
    KeyframeSet,
    KeyframeRemove,
    MarkerAdd,
    MarkerPatch,
    MarkerRemove,
    AudioVolumeSet,
    AudioFadeIn,
    AudioFadeOut,
}

impl VideoOperation {
    pub const ALL: &'static [Self] = &[
        Self::ProjectProfileSet,
        Self::SequenceCreate,
        Self::AssetImport,
        Self::AssetRelink,
        Self::TrackCreate,
        Self::TrackRemove,
        Self::TrackRename,
        Self::TrackMute,
        Self::TrackHide,
        Self::TrackReorder,
        Self::ClipInsert,
        Self::ClipMove,
        Self::ClipTrim,
        Self::ClipSplit,
        Self::ClipRemove,
        Self::ClipDuplicate,
        Self::TransitionAdd,
        Self::TransitionPatch,
        Self::TransitionRemove,
        Self::EffectAdd,
        Self::EffectPatch,
        Self::EffectRemove,
        Self::EffectEnable,
        Self::EffectDisable,
        Self::KeyframeSet,
        Self::KeyframeRemove,
        Self::MarkerAdd,
        Self::MarkerPatch,
        Self::MarkerRemove,
        Self::AudioVolumeSet,
        Self::AudioFadeIn,
        Self::AudioFadeOut,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProjectProfileSet => "project.profile.set",
            Self::SequenceCreate => "sequence.create",
            Self::AssetImport => "asset.import",
            Self::AssetRelink => "asset.relink",
            Self::TrackCreate => "track.create",
            Self::TrackRemove => "track.remove",
            Self::TrackRename => "track.rename",
            Self::TrackMute => "track.mute",
            Self::TrackHide => "track.hide",
            Self::TrackReorder => "track.reorder",
            Self::ClipInsert => "clip.insert",
            Self::ClipMove => "clip.move",
            Self::ClipTrim => "clip.trim",
            Self::ClipSplit => "clip.split",
            Self::ClipRemove => "clip.remove",
            Self::ClipDuplicate => "clip.duplicate",
            Self::TransitionAdd => "transition.add",
            Self::TransitionPatch => "transition.patch",
            Self::TransitionRemove => "transition.remove",
            Self::EffectAdd => "effect.add",
            Self::EffectPatch => "effect.patch",
            Self::EffectRemove => "effect.remove",
            Self::EffectEnable => "effect.enable",
            Self::EffectDisable => "effect.disable",
            Self::KeyframeSet => "keyframe.set",
            Self::KeyframeRemove => "keyframe.remove",
            Self::MarkerAdd => "marker.add",
            Self::MarkerPatch => "marker.patch",
            Self::MarkerRemove => "marker.remove",
            Self::AudioVolumeSet => "audio.volume.set",
            Self::AudioFadeIn => "audio.fade_in",
            Self::AudioFadeOut => "audio.fade_out",
        }
    }
}

impl fmt::Display for VideoOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseVideoOperationError(String);

impl fmt::Display for ParseVideoOperationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown semantic video operation: {}", self.0)
    }
}

impl std::error::Error for ParseVideoOperationError {}

impl FromStr for VideoOperation {
    type Err = ParseVideoOperationError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let operation = match value {
            "project.profile.set" => Self::ProjectProfileSet,
            "sequence.create" => Self::SequenceCreate,
            "asset.import" => Self::AssetImport,
            "asset.relink" => Self::AssetRelink,
            "track.create" => Self::TrackCreate,
            "track.remove" => Self::TrackRemove,
            "track.rename" => Self::TrackRename,
            "track.mute" => Self::TrackMute,
            "track.hide" => Self::TrackHide,
            "track.reorder" => Self::TrackReorder,
            "clip.insert" => Self::ClipInsert,
            "clip.move" => Self::ClipMove,
            "clip.trim" => Self::ClipTrim,
            "clip.split" => Self::ClipSplit,
            "clip.remove" => Self::ClipRemove,
            "clip.duplicate" => Self::ClipDuplicate,
            "transition.add" => Self::TransitionAdd,
            "transition.patch" => Self::TransitionPatch,
            "transition.remove" => Self::TransitionRemove,
            "effect.add" => Self::EffectAdd,
            "effect.patch" => Self::EffectPatch,
            "effect.remove" => Self::EffectRemove,
            "effect.enable" => Self::EffectEnable,
            "effect.disable" => Self::EffectDisable,
            "keyframe.set" => Self::KeyframeSet,
            "keyframe.remove" => Self::KeyframeRemove,
            "marker.add" => Self::MarkerAdd,
            "marker.patch" => Self::MarkerPatch,
            "marker.remove" => Self::MarkerRemove,
            "audio.volume.set" => Self::AudioVolumeSet,
            "audio.fade_in" => Self::AudioFadeIn,
            "audio.fade_out" => Self::AudioFadeOut,
            _ => return Err(ParseVideoOperationError(value.to_owned())),
        };
        Ok(operation)
    }
}

impl Serialize for VideoOperation {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for VideoOperation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(D::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationSupport {
    /// The backend can preserve its native project representation safely.
    SafeRoundtrip,
    /// Semantics are supported but native application metadata may change.
    MetadataRisk,
    /// The representation can be rendered/observed but not safely mutated.
    RenderOnly,
    /// The backend cannot perform this semantic mutation.
    Unsupported,
}

impl MutationSupport {
    pub fn allows_mutation(self) -> bool {
        matches!(self, Self::SafeRoundtrip | Self::MetadataRisk)
    }

    pub fn requires_metadata_acknowledgement(self) -> bool {
        self == Self::MetadataRisk
    }
}
