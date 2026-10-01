//! Backend-neutral audio render/export intent.

use crate::{
    Error, Result,
    hash::sha256,
    model::AudioProject,
    time::{SampleRange, SampleRate},
};
use serde::{Deserialize, Serialize};

pub const RENDER_CONTRACT_VERSION: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AudioFormat {
    Wav,
    Flac,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BitDepth {
    Pcm16,
    Pcm24,
    Pcm32,
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ResampleQuality {
    Draft,
    Medium,
    #[default]
    High,
    Mastering,
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DitherPolicy {
    #[default]
    None,
    Tpdf {
        seed: u64,
    },
    NoiseShaped {
        seed: u64,
        order: u8,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RenderSource {
    Project,
    Stem { stem: String },
    Synth { synth: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderIntent {
    pub contract_version: u32,
    pub source: RenderSource,
    pub range: Option<SampleRange>,
    pub format: AudioFormat,
    pub sample_rate: SampleRate,
    pub channels: u16,
    pub bit_depth: BitDepth,
    pub normalize_lufs_milli: Option<i32>,
    #[serde(default)]
    pub resample_quality: ResampleQuality,
    #[serde(default)]
    pub dither: DitherPolicy,
}

impl RenderIntent {
    pub fn validate_against(&self, project: &AudioProject) -> Result<u64> {
        if self.contract_version != RENDER_CONTRACT_VERSION {
            return Err(Error::unsupported(
                "Unsupported semantic audio render contract",
            ));
        }
        project.validate()?;
        self.sample_rate.validate()?;
        if !(1..=64).contains(&self.channels) {
            return Err(Error::invalid("Invalid render channel count"));
        }
        if let Some(target) = self.normalize_lufs_milli
            && !(-70_000..=0).contains(&target)
        {
            return Err(Error::invalid(
                "LUFS normalization target is outside bounds",
            ));
        }
        match &self.source {
            RenderSource::Project => {}
            RenderSource::Stem { stem } => {
                project.stem(stem)?;
            }
            RenderSource::Synth { synth } => {
                if !project.synths.contains_key(synth) {
                    return Err(Error::new("NotFound", "Render synth not found"));
                }
            }
        }

        if let DitherPolicy::NoiseShaped { order, .. } = self.dither
            && !(1..=8).contains(&order)
        {
            return Err(Error::invalid("Noise-shaped dither order must be 1..=8"));
        }
        let natural = match &self.source {
            RenderSource::Project => project.duration(),
            RenderSource::Stem { stem } => project.stem(stem)?.duration(),
            RenderSource::Synth { .. } => self
                .range
                .map(SampleRange::duration)
                .ok_or_else(|| Error::invalid("Synth render requires an explicit sample range"))?,
        };
        if natural == 0 {
            return Err(Error::invalid("Cannot render empty audio"));
        }
        let source_frames = match self.range {
            Some(range) => {
                SampleRange::new(range.start.0, range.end.0)?;
                if !matches!(self.source, RenderSource::Synth { .. }) && range.end.0 > natural {
                    return Err(Error::invalid("Audio render range exceeds source duration"));
                }
                range.duration()
            }
            None => natural,
        };
        let numerator = u128::from(source_frames) * u128::from(self.sample_rate.0);
        let denominator = u128::from(project.profile.sample_rate.0);
        let rounded = numerator
            .checked_add(denominator / 2)
            .ok_or_else(|| Error::limit("Resampled frame count overflow"))?
            / denominator;
        let frames = u64::try_from(rounded)
            .map_err(|_| Error::limit("Resampled frame count exceeds u64"))?;
        if frames == 0 || frames > crate::time::MAX_SAMPLE_FRAME {
            return Err(Error::limit("Resampled frame count exceeds audio budget"));
        }
        Ok(frames)
    }

    pub fn semantic_digest(&self, project: &AudioProject) -> Result<String> {
        let frames = self.validate_against(project)?;
        let bytes = serde_json::to_vec(&(self, project.semantic_digest()?, frames))
            .map_err(|_| Error::new("BackendFailed", "Could not encode audio render intent"))?;
        Ok(sha256(&bytes))
    }

    pub fn extension(&self) -> &'static str {
        match self.format {
            AudioFormat::Wav => "wav",
            AudioFormat::Flac => "flac",
        }
    }
}
