//! Provider-owned media receipts. A reference token is never a filesystem path.
use crate::{Rate, Rational};
use schemars::JsonSchema;
use semwright_semantic_composition::{BaseStateSet, Digest, Owner, Result, bounded_id, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VideoMetadata {
    pub width: u32,
    pub height: u32,
    pub frame_rate: Rate,
    pub frames: u64,
    pub alpha: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AudioMetadata {
    pub sample_rate: u32,
    pub channels: u16,
    pub channel_layout: String,
    pub sample_frames: u64,
    pub priming_samples: Option<u64>,
    pub padding_samples: Option<u64>,
    pub latency_samples: Option<u64>,
    pub tail_samples: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MediaMetadata {
    pub duration: Rational,
    pub encoded_duration: Option<Rational>,
    pub video: Option<VideoMetadata>,
    pub audio: Option<AudioMetadata>,
}
impl MediaMetadata {
    pub fn validate(&self) -> Result<()> {
        self.duration.validate()?;
        ensure(self.duration >= Rational::ZERO, "negative media duration")?;
        if let Some(d) = self.encoded_duration {
            d.validate()?;
            ensure(d >= Rational::ZERO, "negative encoded duration")?;
        }
        ensure(
            self.video.is_some() || self.audio.is_some(),
            "media metadata needs an observed stream",
        )?;
        if let Some(v) = &self.video {
            ensure(
                (1..=16384).contains(&v.width)
                    && (1..=16384).contains(&v.height)
                    && v.frames <= 100_000_000,
                "video metadata limits",
            )?;
            v.frame_rate.validate()?;
        }
        if let Some(a) = &self.audio {
            ensure(
                (8000..=384000).contains(&a.sample_rate)
                    && (1..=64).contains(&a.channels)
                    && a.sample_frames <= 384000 * 86400,
                "audio metadata limits",
            )?;
            bounded_id(&a.channel_layout)?;
            if let (Some(priming), Some(padding)) = (a.priming_samples, a.padding_samples) {
                ensure(
                    priming
                        .checked_add(padding)
                        .is_some_and(|n| n <= a.sample_frames),
                    "priming/padding exceed encoded samples",
                )?;
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Retention {
    PrivateCandidate,
    ExplicitlyPublished,
    OwnerManaged,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MediaArtifact {
    pub reference: String,
    pub owner: Owner,
    pub sha256: Digest,
    pub bytes: u64,
    pub media_type: String,
    pub source_plan: Digest,
    pub source_state: BaseStateSet,
    pub metadata: MediaMetadata,
    pub dependencies: BTreeMap<String, Digest>,
    pub provenance: Option<String>,
    pub license: Option<String>,
    pub retention: Retention,
}
impl MediaArtifact {
    pub fn validate(&self) -> Result<()> {
        bounded_id(&self.reference)?;
        self.owner.validate()?;
        self.source_state.validate()?;
        self.metadata.validate()?;
        ensure(
            self.reference.starts_with("artifact:")
                && !self.reference.contains("..")
                && !self.reference.contains('/')
                && !self.reference.contains('\\'),
            "provider artifact reference must not encode a filesystem path",
        )?;
        ensure(
            self.bytes > 0
                && self.bytes <= 1_073_741_824
                && self.media_type.len() <= 128
                && self.media_type.contains('/'),
            "artifact byte/media-type bounds",
        )?;
        ensure(self.dependencies.len() <= 256, "artifact dependency budget")?;
        for key in self.dependencies.keys() {
            bounded_id(key)?;
        }
        for value in [&self.provenance, &self.license].into_iter().flatten() {
            ensure(
                value.len() <= 2048 && !value.chars().any(char::is_control),
                "artifact provenance bounds",
            )?;
        }
        Ok(())
    }
}
