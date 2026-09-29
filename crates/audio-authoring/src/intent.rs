use crate::domain;
use schemars::JsonSchema;
use semwright_audio_domain::{
    model::{AudioProject, BusSend, EffectChain, validate_id},
    presets::SfxPreset,
    units::MilliDb,
};
use semwright_media_time::{CueGraph, Interval, Rational};
use semwright_semantic_composition::{ConvergenceBudget, Digest, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BusRole {
    Narration,
    MusicBed,
    Ambience,
    SoundEffect,
    Instrument,
    Stem,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Material {
    Sample {
        sample: String,
        sha256: Digest,
    },
    Synth {
        synth: String,
        midi_note: u8,
        velocity_permille: u16,
    },
    SoundEffect {
        preset: SfxPreset,
        seed: u64,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Placement {
    Absolute {
        start: Rational,
        duration: Rational,
    },
    Cue {
        cue: String,
        offset: Rational,
        duration: Rational,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClipIntent {
    pub id: String,
    pub material: Material,
    pub placement: Placement,
    pub source_offset_frames: u64,
    pub gain: MilliDb,
    pub fade_in_frames: u64,
    pub fade_out_frames: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TrackIntent {
    pub id: String,
    pub name: String,
    pub role: BusRole,
    pub existing_stem: Option<String>,
    pub output_bus: String,
    pub channels: u16,
    pub gain: MilliDb,
    pub pan_milli: i16,
    #[serde(default)]
    pub sends: Vec<BusSend>,
    pub effects: EffectChain,
    pub clips: Vec<ClipIntent>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DuckingIntent {
    pub id: String,
    pub track: String,
    pub foreground: Vec<Interval>,
    pub attenuation: MilliDb,
    pub attack_frames: u64,
    pub release_frames: u64,
    pub merge_gap_frames: u64,
    pub replace_existing_gain_automation: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeliveryProfile {
    pub id: String,
    pub peak_ceiling_millidbfs: i32,
    pub integrated_lufs_milli: Option<i32>,
    pub loudness_tolerance_milli: u32,
    pub true_peak_ceiling_millidbtp: Option<i32>,
    pub allow_silence: bool,
    pub minimum_master_gain: MilliDb,
    pub maximum_master_gain: MilliDb,
}
impl DeliveryProfile {
    pub fn validate(&self) -> Result<()> {
        domain(validate_id(&self.id))?;
        ensure(
            (-120_000..=0).contains(&self.peak_ceiling_millidbfs),
            "delivery sample-peak ceiling",
        )?;
        ensure(
            self.integrated_lufs_milli
                .is_none_or(|v| (-70_000..=0).contains(&v)),
            "delivery loudness target",
        )?;
        ensure(
            self.true_peak_ceiling_millidbtp
                .is_none_or(|v| (-120_000..=0).contains(&v)),
            "delivery true-peak ceiling",
        )?;
        ensure(
            self.loudness_tolerance_milli <= 6000,
            "delivery loudness tolerance",
        )?;
        domain(MilliDb::new(self.minimum_master_gain.0))?;
        domain(MilliDb::new(self.maximum_master_gain.0))?;
        ensure(
            self.minimum_master_gain.0 <= self.maximum_master_gain.0,
            "master gain bounds",
        )
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AudioIntent {
    pub version: u32,
    pub id: String,
    pub tracks: Vec<TrackIntent>,
    pub ducking: Vec<DuckingIntent>,
    pub cues: CueGraph,
    pub dependencies: BTreeMap<String, Digest>,
    pub delivery: DeliveryProfile,
    pub budget: ConvergenceBudget,
}
impl AudioIntent {
    pub fn validate(&self, project: &AudioProject) -> Result<()> {
        domain(project.validate())?;
        ensure(
            self.version == 1 && !self.tracks.is_empty() && self.tracks.len() <= 128,
            "audio intent version/track limit",
        )?;
        ensure(
            self.ducking.len() <= 128 && self.dependencies.len() <= 256,
            "audio intent dependency budget",
        )?;
        domain(validate_id(&self.id))?;
        self.budget.validate()?;
        self.delivery.validate()?;
        self.cues.resolve()?;
        let mut ids = BTreeSet::new();
        let mut existing = BTreeSet::new();
        let mut clips = 0;
        for track in &self.tracks {
            domain(validate_id(&track.id))?;
            ensure(ids.insert(track.id.clone()), "duplicate track logical ID")?;
            ensure(
                !track.name.is_empty()
                    && track.name.len() <= 4096
                    && !track.name.chars().any(char::is_control),
                "track name",
            )?;
            domain(project.bus(&track.output_bus))?;
            domain(MilliDb::new(track.gain.0))?;
            ensure(
                (1..=64).contains(&track.channels) && (-1000..=1000).contains(&track.pan_milli),
                "track channel/pan bounds",
            )?;
            ensure(track.sends.len() <= 256, "track send budget")?;
            let mut send_targets = BTreeSet::new();
            for send in &track.sends {
                domain(project.bus(&send.target_bus))?;
                domain(MilliDb::new(send.gain.0))?;
                ensure(
                    send_targets.insert(&send.target_bus),
                    "duplicate track send target",
                )?;
            }
            if let Some(id) = &track.existing_stem {
                domain(project.stem(id))?;
                ensure(
                    existing.insert(id),
                    "two intents target the same existing stem",
                )?;
                ensure(
                    track.effects.effects.is_empty(),
                    "existing effect chains require explicit low-level edits",
                )?;
            }
            for clip in &track.clips {
                clips += 1;
                domain(validate_id(&clip.id))?;
                ensure(ids.insert(clip.id.clone()), "duplicate clip logical ID")?;
                domain(MilliDb::new(clip.gain.0))?;
                if let Material::Sample { sample, sha256 } = &clip.material {
                    ensure(
                        project.samples.contains_key(sample),
                        "sample material is not imported",
                    )?;
                    ensure(
                        self.dependencies.get(sample) == Some(sha256),
                        "sample dependency digest is not bound",
                    )?;
                }
            }
        }
        ensure(clips <= 2048, "audio authoring clip budget")?;
        let mut ducked = BTreeSet::new();
        for duck in &self.ducking {
            domain(validate_id(&duck.id))?;
            ensure(ids.insert(duck.id.clone()), "duplicate ducking logical ID")?;
            ensure(
                self.tracks.iter().any(|t| t.id == duck.track) && ducked.insert(&duck.track),
                "unknown or duplicate ducking target",
            )?;
            ensure(
                !duck.foreground.is_empty() && duck.foreground.len() <= 128,
                "ducking foreground limit",
            )?;
            ensure(
                (-60_000..=-1).contains(&duck.attenuation.0),
                "ducking attenuation must be negative and bounded",
            )?;
            let max = u64::from(project.profile.sample_rate.0) * 30;
            ensure(
                duck.attack_frames <= max
                    && duck.release_frames <= max
                    && duck.merge_gap_frames <= max,
                "ducking time budget",
            )?;
            for range in &duck.foreground {
                range.nonnegative()?;
            }
        }
        for cue in &self.cues.cues {
            ensure(
                self.dependencies
                    .values()
                    .any(|digest| *digest == cue.source),
                "cue provenance is not bound to an input dependency",
            )?;
        }
        Ok(())
    }
}
