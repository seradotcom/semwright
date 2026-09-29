//! Bounded audio-analysis contracts.
//!
//! PCM statistics and perceptual loudness are distinct. Integrated LUFS and
//! true peak only exist when a backend provides a declared, versioned meter.

use crate::{Error, Result, time::SampleRate};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AudioAnalysis {
    pub sample_rate: SampleRate,
    pub channels: u16,
    pub frames: u64,
    pub peak_millidbfs: i32,
    pub rms_millidbfs: i32,
    pub integrated_lufs_milli: Option<i32>,
}
impl AudioAnalysis {
    pub fn validate(&self) -> Result<()> {
        self.sample_rate.validate()?;
        if !(1..=64).contains(&self.channels)
            || self.frames == 0
            || !(-120_000..=12_000).contains(&self.peak_millidbfs)
            || !(-120_000..=12_000).contains(&self.rms_millidbfs)
            || self
                .integrated_lufs_milli
                .is_some_and(|v| !(-120_000..=12_000).contains(&v))
        {
            return Err(Error::invalid("Invalid bounded audio-analysis result"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LoudnessAnalysis {
    pub schema_version: u32,
    pub method: String,
    pub version: String,
    pub frames: u64,
    pub sample_rate: u32,
    pub channels: u16,
    pub layout: String,
    pub nonfinite_samples: u64,
    pub integrated_lufs_milli: Option<i32>,
    pub momentary_lufs_milli: Option<i32>,
    pub short_term_lufs_milli: Option<i32>,
    pub loudness_range_milli: Option<u32>,
    pub true_peak_millidbtp: Option<i32>,
    pub sample_peak_millidbfs: Option<i32>,
    pub true_peak_oversample: u16,
    pub momentary_window_ms: u32,
    pub short_term_window_ms: u32,
    pub unknown_reason: Option<String>,
}
impl LoudnessAnalysis {
    pub fn validate(&self) -> Result<()> {
        SampleRate::new(self.sample_rate)?;
        if self.schema_version != 1
            || self.method != "libebur128"
            || self.version != "1.2.6"
            || self.frames == 0
            || !matches!(self.channels, 1 | 2)
            || (self.channels == 1 && self.layout != "mono")
            || (self.channels == 2 && self.layout != "stereo")
            || !matches!(self.true_peak_oversample, 1 | 2 | 4)
            || self.momentary_window_ms != 400
            || self.short_term_window_ms != 3000
            || self
                .unknown_reason
                .as_ref()
                .is_some_and(|v| v.is_empty() || v.len() > 256 || v.chars().any(char::is_control))
        {
            return Err(Error::invalid("Invalid loudness analysis receipt"));
        }
        for value in [
            self.integrated_lufs_milli,
            self.momentary_lufs_milli,
            self.short_term_lufs_milli,
            self.true_peak_millidbtp,
            self.sample_peak_millidbfs,
        ]
        .into_iter()
        .flatten()
        {
            if !(-200_000..=24_000).contains(&value) {
                return Err(Error::invalid("Loudness level is outside bounded units"));
            }
        }
        if self
            .loudness_range_milli
            .is_some_and(|value| value > 200_000)
        {
            return Err(Error::invalid("Loudness range is outside bounded units"));
        }
        if self.nonfinite_samples > 0
            && (self.integrated_lufs_milli.is_some()
                || self.momentary_lufs_milli.is_some()
                || self.short_term_lufs_milli.is_some()
                || self.loudness_range_milli.is_some()
                || self.true_peak_millidbtp.is_some())
        {
            return Err(Error::invalid(
                "Nonfinite input cannot produce trusted loudness metrics",
            ));
        }
        Ok(())
    }
}

pub fn analyze_pcm_i16_interleaved(
    values: &[i16],
    channels: u16,
    sample_rate: SampleRate,
) -> Result<AudioAnalysis> {
    sample_rate.validate()?;
    if !(1..=64).contains(&channels)
        || values.is_empty()
        || !values.len().is_multiple_of(usize::from(channels))
    {
        return Err(Error::invalid("PCM buffer shape is invalid"));
    }
    let peak = values
        .iter()
        .map(|value| i32::from(*value).unsigned_abs())
        .max()
        .unwrap_or(0) as f64
        / 32768.0;
    let mean_square = values
        .iter()
        .map(|value| {
            let x = f64::from(*value) / 32768.0;
            x * x
        })
        .sum::<f64>()
        / values.len() as f64;
    let rms = mean_square.sqrt();
    let result = AudioAnalysis {
        sample_rate,
        channels,
        frames: values.len() as u64 / u64::from(channels),
        peak_millidbfs: amplitude_to_millidb(peak),
        rms_millidbfs: amplitude_to_millidb(rms),
        integrated_lufs_milli: None,
    };
    result.validate()?;
    Ok(result)
}

fn amplitude_to_millidb(value: f64) -> i32 {
    if value <= 0.0 {
        -120_000
    } else {
        (20_000.0 * value.log10())
            .round()
            .clamp(-120_000.0, 12_000.0) as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonfinite_receipt_cannot_claim_loudness() {
        let invalid = LoudnessAnalysis {
            schema_version: 1,
            method: "libebur128".into(),
            version: "1.2.6".into(),
            frames: 48_000,
            sample_rate: 48_000,
            channels: 1,
            layout: "mono".into(),
            nonfinite_samples: 1,
            integrated_lufs_milli: Some(-20_000),
            momentary_lufs_milli: None,
            short_term_lufs_milli: None,
            loudness_range_milli: None,
            true_peak_millidbtp: None,
            sample_peak_millidbfs: None,
            true_peak_oversample: 4,
            momentary_window_ms: 400,
            short_term_window_ms: 3000,
            unknown_reason: Some("nonfinite_input".into()),
        };
        assert!(invalid.validate().is_err());
    }
}
