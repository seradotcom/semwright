//! Backend-neutral render/export intent.
//!
//! Drivers map these semantic presets to native encoders and keep executable,
//! filesystem, queue and application-specific settings outside this crate.

use crate::{Error, Result, hash::sha256, model::Project, time::FrameRange};
use serde::{Deserialize, Serialize};

pub const RENDER_CONTRACT_VERSION: u32 = 1;
pub const MAX_RENDER_DIMENSION: u32 = 16_384;
const MAX_RENDER_TOKEN: usize = 64;

fn validate_token(label: &str, value: &str, max: usize) -> Result<()> {
    let first = value.as_bytes().first().copied();
    let last = value.as_bytes().last().copied();
    if value.is_empty()
        || value.len() > max
        || first.is_none_or(|b| !b.is_ascii_lowercase() && !b.is_ascii_digit())
        || last.is_none_or(|b| !b.is_ascii_lowercase() && !b.is_ascii_digit())
        || value.contains("..")
        || !value.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
        })
    {
        return Err(Error::invalid(format!("Invalid {label} identifier")));
    }
    Ok(())
}

fn valid_reason(value: &str) -> bool {
    !value.is_empty() && value.len() <= 4096 && !value.chars().any(char::is_control)
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderPreset {
    pub id: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// Semantic codec family (for example h264 or ffv1), not an encoder binary.
    pub video_codec: Option<String>,
    /// Semantic codec family (for example aac or pcm_s16le).
    pub audio_codec: Option<String>,
    /// Semantic container identifier (for example mp4, matroska or wav).
    pub container: String,
    /// File extension without a leading dot.
    pub extension: String,
}

impl RenderPreset {
    pub fn validate(&self) -> Result<()> {
        validate_token("render preset", &self.id, MAX_RENDER_TOKEN)?;
        validate_token("container", &self.container, MAX_RENDER_TOKEN)?;
        validate_token("extension", &self.extension, 16)?;

        if self.video_codec.is_none() && self.audio_codec.is_none() {
            return Err(Error::invalid(
                "Render preset must contain video, audio or both",
            ));
        }
        if self.width.is_some() != self.height.is_some() {
            return Err(Error::invalid(
                "Render width and height must be specified together",
            ));
        }
        if let (Some(width), Some(height)) = (self.width, self.height)
            && (self.video_codec.is_none()
                || width < 2
                || height < 2
                || width > MAX_RENDER_DIMENSION
                || height > MAX_RENDER_DIMENSION)
        {
            return Err(Error::invalid("Invalid render dimensions"));
        }
        if let Some(codec) = &self.video_codec {
            validate_token("video codec", codec, MAX_RENDER_TOKEN)?;
        }
        if let Some(codec) = &self.audio_codec {
            validate_token("audio codec", codec, MAX_RENDER_TOKEN)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "frames", rename_all = "snake_case")]
pub enum RenderRange {
    FullSequence,
    Frames(FrameRange),
}
impl RenderRange {
    pub fn resolve(self, sequence_duration: u64) -> Result<FrameRange> {
        if sequence_duration == 0 {
            return Err(Error::invalid("Cannot render an empty sequence"));
        }
        match self {
            Self::FullSequence => FrameRange::new(0, sequence_duration),
            Self::Frames(range) => {
                FrameRange::new(range.start.0, range.end.0)?;
                if range.end.0 > sequence_duration {
                    return Err(Error::invalid("Render range exceeds sequence duration"));
                }
                Ok(range)
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderIntent {
    pub contract_version: u32,
    pub sequence: String,
    pub range: RenderRange,
    pub preset: RenderPreset,
}

impl RenderIntent {
    pub fn full(sequence: impl Into<String>, preset: RenderPreset) -> Self {
        Self {
            contract_version: RENDER_CONTRACT_VERSION,
            sequence: sequence.into(),
            range: RenderRange::FullSequence,
            preset,
        }
    }
    pub fn validate_against(&self, project: &Project) -> Result<FrameRange> {
        if self.contract_version != RENDER_CONTRACT_VERSION {
            return Err(Error::unsupported(
                "Unsupported semantic render contract version",
            ));
        }
        if self.sequence.is_empty() || self.sequence.len() > 256 {
            return Err(Error::invalid("Invalid render sequence identity"));
        }
        self.preset.validate()?;
        project.validate()?;
        let sequence = project.sequence(&self.sequence)?;
        self.range.resolve(sequence.duration())
    }

    pub fn expected_frames(&self, project: &Project) -> Result<u64> {
        Ok(self.validate_against(project)?.duration())
    }

    pub fn semantic_digest(&self, project: &Project) -> Result<String> {
        let range = self.validate_against(project)?;
        let bytes = serde_json::to_vec(&(self, project.semantic_digest()?, range))
            .map_err(|_| Error::new("BackendFailed", "Could not encode render intent"))?;
        Ok(sha256(&bytes))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RenderSupport {
    Available,
    Unavailable { reasons: Vec<String> },
    Unsupported { reason: String },
}
impl RenderSupport {
    pub fn allows_render(&self) -> bool {
        matches!(self, Self::Available)
    }

    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Available => Ok(()),
            Self::Unavailable { reasons } => {
                if reasons.is_empty()
                    || reasons.len() > 64
                    || reasons.iter().any(|reason| !valid_reason(reason))
                {
                    return Err(Error::invalid("Invalid render unavailability reasons"));
                }
                Ok(())
            }
            Self::Unsupported { reason } => {
                if !valid_reason(reason) {
                    return Err(Error::invalid("Invalid render unsupported reason"));
                }
                Ok(())
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderCapability {
    pub preset: RenderPreset,
    pub support: RenderSupport,
}

impl RenderCapability {
    pub fn validate(&self) -> Result<()> {
        self.preset.validate()?;
        self.support.validate()
    }
}
