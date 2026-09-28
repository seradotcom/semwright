//! Bounded audio-analysis values and a deterministic PCM reference analyzer.
//!
//! Integrated LUFS is represented in the domain but is intentionally optional:
//! a backend must use a declared BS.1770/EBU-compatible measurement path rather
//! than fabricating it from RMS.

use crate::{Error, Result, time::SampleRate};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
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
