//! Sync is measured on decoded presentation timestamps, not source-file offsets.
use crate::Result;
use schemars::JsonSchema;
use semwright_media_time::Rational as Q;
use semwright_semantic_composition::{Digest, EvidenceSource, Verdict, bounded_id, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SyncCue {
    pub id: String,
    pub expected_time: Q,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SyncSpec {
    pub cues: Vec<SyncCue>,
    pub max_offset: Q,
    pub max_drift: Q,
    pub max_cue_error: Q,
    pub confidence_floor: u16,
    pub require_full_scan: bool,
}
impl SyncSpec {
    pub fn validate(&self) -> Result<()> {
        ensure(
            !self.cues.is_empty() && self.cues.len() <= 16 && self.confidence_floor <= 10000,
            "sync cue/confidence budget",
        )?;
        for v in [self.max_offset, self.max_drift, self.max_cue_error] {
            v.validate()?;
            ensure(
                v >= Q::ZERO && v <= Q::ONE,
                "sync tolerance must be explicitly bounded within one second",
            )?;
        }
        let mut seen = BTreeSet::new();
        let mut previous = None;
        for cue in &self.cues {
            bounded_id(&cue.id)?;
            cue.expected_time.validate()?;
            ensure(seen.insert(&cue.id), "duplicate sync cue")?;
            ensure(cue.expected_time >= Q::ZERO, "negative sync cue time")?;
            if let Some(last) = previous {
                ensure(cue.expected_time > last, "sync cues must be time ordered")?;
            }
            previous = Some(cue.expected_time);
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Detection {
    pub cue_id: String,
    pub presentation_time: Q,
    pub uncertainty: Q,
    pub confidence: u16,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DecodedSyncProbe {
    pub version: u32,
    pub artifact_digest: Digest,
    pub decoder_method: String,
    pub decoder_digest: Digest,
    pub source: EvidenceSource,
    pub exhaustive_video: bool,
    pub exhaustive_audio: bool,
    pub flashes: Vec<Detection>,
    pub impulses: Vec<Detection>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SyncMeasurement {
    pub cue_id: String,
    pub video_time: Q,
    pub audio_time: Q,
    pub offset: Q,
    pub uncertainty: Q,
    pub verdict: Verdict,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SyncReport {
    pub version: u32,
    pub artifact_digest: Digest,
    pub decoder_digest: Digest,
    pub verdict: Verdict,
    pub observations: Vec<SyncMeasurement>,
    pub missing: Vec<String>,
    pub observed_drift: Option<Q>,
    pub exhaustive: bool,
    pub limitations: Vec<String>,
}
fn abs(value: Q) -> Result<Q> {
    if value < Q::ZERO {
        Q::ZERO.checked_sub(value)
    } else {
        Ok(value)
    }
}
fn detections(values: &[Detection]) -> Result<BTreeMap<&str, &Detection>> {
    ensure(values.len() <= 512, "decoder detection count")?;
    let mut out = BTreeMap::new();
    for value in values {
        bounded_id(&value.cue_id)?;
        value.presentation_time.validate()?;
        value.uncertainty.validate()?;
        ensure(
            value.uncertainty >= Q::ZERO
                && value.confidence <= 10000
                && out.insert(value.cue_id.as_str(), value).is_none(),
            "invalid/ambiguous decoder detection",
        )?;
    }
    Ok(out)
}
pub fn verify_sync(spec: &SyncSpec, probe: &DecodedSyncProbe) -> Result<SyncReport> {
    spec.validate()?;
    bounded_id(&probe.decoder_method)?;
    ensure(probe.version == 1, "decoder probe version")?;
    let flashes = detections(&probe.flashes)?;
    let impulses = detections(&probe.impulses)?;
    let exhaustive = probe.exhaustive_video && probe.exhaustive_audio;
    let mut unknown = probe.source != EvidenceSource::DecodedMedia;
    if spec.require_full_scan {
        if !exhaustive {
            unknown = true;
        }
    }
    let mut failed = false;
    let mut observations = vec![];
    let mut missing = vec![];
    for cue in &spec.cues {
        let (Some(video), Some(audio)) =
            (flashes.get(cue.id.as_str()), impulses.get(cue.id.as_str()))
        else {
            missing.push(cue.id.clone());
            unknown = true;
            continue;
        };
        let offset = audio
            .presentation_time
            .checked_sub(video.presentation_time)?;
        let uncertainty = video.uncertainty.checked_add(audio.uncertainty)?;
        let absolute = abs(offset)?;
        let video_error = abs(video.presentation_time.checked_sub(cue.expected_time)?)?;
        let audio_error = abs(audio.presentation_time.checked_sub(cue.expected_time)?)?;
        let lower_offset = absolute.checked_sub(uncertainty)?;
        let upper_offset = absolute.checked_add(uncertainty)?;
        let failure_checks = [
            lower_offset > spec.max_offset,
            video_error.checked_sub(video.uncertainty)? > spec.max_cue_error,
            audio_error.checked_sub(audio.uncertainty)? > spec.max_cue_error,
        ];
        let pass_checks = [
            upper_offset <= spec.max_offset,
            video_error.checked_add(video.uncertainty)? <= spec.max_cue_error,
            audio_error.checked_add(audio.uncertainty)? <= spec.max_cue_error,
        ];
        let verdict = if failure_checks.into_iter().any(|failed_check| failed_check) {
            failed = true;
            Verdict::Fail
        } else if !pass_checks.into_iter().all(|passed_check| passed_check) {
            unknown = true;
            Verdict::Unknown
        } else if video.confidence < spec.confidence_floor {
            unknown = true;
            Verdict::Unknown
        } else if audio.confidence < spec.confidence_floor {
            unknown = true;
            Verdict::Unknown
        } else {
            Verdict::Pass
        };
        observations.push(SyncMeasurement {
            cue_id: cue.id.clone(),
            video_time: video.presentation_time,
            audio_time: audio.presentation_time,
            offset,
            uncertainty,
            verdict,
        });
    }
    let observed_drift = if observations.len() > 1 {
        let low = observations
            .iter()
            .min_by_key(|v| v.offset)
            .expect("observations");
        let high = observations
            .iter()
            .max_by_key(|v| v.offset)
            .expect("observations");
        let drift = high.offset.checked_sub(low.offset)?;
        let error = high.uncertainty.checked_add(low.uncertainty)?;
        if drift.checked_sub(error)? > spec.max_drift {
            failed = true;
        } else if drift.checked_add(error)? > spec.max_drift {
            unknown = true;
        }
        Some(drift)
    } else {
        if spec.cues.len() > 1 {
            unknown = true;
        }
        None
    };
    let mut limitations = vec![];
    if !exhaustive {
        limitations.push("Decoder sampled media; no claim about unobserved frames/samples".into());
    }
    if probe.source != EvidenceSource::DecodedMedia {
        limitations
            .push("Non-native decoder evidence cannot certify final-media synchronization".into());
    }
    Ok(SyncReport {
        version: 1,
        artifact_digest: probe.artifact_digest.clone(),
        decoder_digest: probe.decoder_digest.clone(),
        verdict: if failed {
            Verdict::Fail
        } else if unknown {
            Verdict::Unknown
        } else {
            Verdict::Pass
        },
        observations,
        missing,
        observed_drift,
        exhaustive,
        limitations,
    })
}
