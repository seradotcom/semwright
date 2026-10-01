//! Streaming signal statistics, separate from perception and BS.1770 metering.
use crate::{
    Error, Result,
    time::{MAX_SAMPLE_FRAME, SampleRate},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const METHOD: &str = "semwright-pcm-statistics-v2";
const MAX_SILENCE_RANGES: usize = 4096;
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChannelStatistics {
    /// None is mathematically undefined (silence or no finite samples), not -120 dBFS.
    pub peak_millidbfs: Option<i32>,
    pub rms_millidbfs: Option<i32>,
    pub dc_offset: Option<f64>,
    pub finite_samples: u64,
    pub nonfinite_samples: u64,
    /// Samples outside [-1, 1], before any integer quantization or saturation.
    pub out_of_range_samples: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SilenceRange {
    pub start_frame: u64,
    pub end_frame: u64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SignalStatistics {
    pub schema_version: u32,
    pub method: String,
    pub sample_rate: SampleRate,
    pub frames: u64,
    pub channels: Vec<ChannelStatistics>,
    pub peak_millidbfs: Option<i32>,
    pub rms_millidbfs: Option<i32>,
    pub silence_threshold_millidbfs: i32,
    pub silence_ranges: Vec<SilenceRange>,
    pub silence_ranges_exhaustive: bool,
    pub entirely_silent: bool,
    pub nonfinite_samples: u64,
    pub out_of_range_samples: u64,
}
#[derive(Debug, Default, Clone)]
struct Sum {
    value: f64,
    compensation: f64,
}
impl Sum {
    fn add(&mut self, value: f64) {
        let corrected = value - self.compensation;
        let next = self.value + corrected;
        self.compensation = (next - self.value) - corrected;
        self.value = next;
    }
}
#[derive(Debug, Default, Clone)]
struct Channel {
    peak: f64,
    sum: Sum,
    squares: Sum,
    finite: u64,
    nonfinite: u64,
    out_of_range: u64,
}
#[derive(Debug)]
pub struct PcmAnalyzer {
    rate: SampleRate,
    channels: Vec<Channel>,
    frames: u64,
    frame_budget: u64,
    threshold_db: i32,
    threshold: f64,
    minimum_silence_frames: u64,
    silence_start: Option<u64>,
    silence_ranges: Vec<SilenceRange>,
    silence_exhaustive: bool,
}
impl PcmAnalyzer {
    pub fn new(
        rate: SampleRate,
        channels: u16,
        frame_budget: u64,
        threshold_db: i32,
        minimum_silence_frames: u64,
    ) -> Result<Self> {
        rate.validate()?;
        if !(1..=64).contains(&channels)
            || frame_budget == 0
            || frame_budget > MAX_SAMPLE_FRAME
            || !(-180_000..=0).contains(&threshold_db)
            || minimum_silence_frames == 0
        {
            return Err(Error::invalid("Invalid PCM analysis configuration"));
        }
        Ok(Self {
            rate,
            channels: vec![Channel::default(); channels as usize],
            frames: 0,
            frame_budget,
            threshold_db,
            threshold: 10_f64.powf(f64::from(threshold_db) / 20000.0),
            minimum_silence_frames,
            silence_start: None,
            silence_ranges: vec![],
            silence_exhaustive: true,
        })
    }
    pub fn push_interleaved(&mut self, values: &[f64]) -> Result<()> {
        if !values.len().is_multiple_of(self.channels.len()) {
            return Err(Error::invalid("PCM block ends inside a frame"));
        }
        let count = (values.len() / self.channels.len()) as u64;
        if self
            .frames
            .checked_add(count)
            .is_none_or(|n| n > self.frame_budget)
        {
            return Err(Error::limit("PCM frame budget exceeded"));
        }
        // Check the whole block before changing state, so rejection does not partially consume it.
        if values
            .iter()
            .any(|v| v.is_finite() && v.abs() > f64::from(f32::MAX))
        {
            return Err(Error::invalid(
                "Finite PCM exceeds supported float32 magnitude",
            ));
        }
        for frame in values.chunks_exact(self.channels.len()) {
            let mut quiet = true;
            for (channel, &sample) in self.channels.iter_mut().zip(frame) {
                if !sample.is_finite() {
                    channel.nonfinite += 1;
                    quiet = false;
                    continue;
                }
                channel.finite += 1;
                channel.peak = channel.peak.max(sample.abs());
                channel.sum.add(sample);
                channel.squares.add(sample * sample);
                if sample.abs() > 1.0 {
                    channel.out_of_range += 1;
                }
                quiet &= sample.abs() <= self.threshold;
            }
            if quiet {
                self.silence_start.get_or_insert(self.frames);
            } else {
                self.close_silence();
            }
            self.frames += 1;
        }
        Ok(())
    }
    fn close_silence(&mut self) {
        if let Some(start) = self.silence_start.take()
            && self.frames - start >= self.minimum_silence_frames
        {
            if self.silence_ranges.len() < MAX_SILENCE_RANGES {
                self.silence_ranges.push(SilenceRange {
                    start_frame: start,
                    end_frame: self.frames,
                });
            } else {
                self.silence_exhaustive = false;
            }
        }
    }
    pub fn finish(mut self) -> Result<SignalStatistics> {
        if self.frames == 0 {
            return Err(Error::invalid("Cannot analyze zero PCM frames"));
        }
        self.close_silence();
        let finite = self.channels.iter().map(|c| c.finite).sum::<u64>();
        let peak = self.channels.iter().map(|c| c.peak).fold(0.0, f64::max);
        let square_sum = self.channels.iter().map(|c| c.squares.value).sum::<f64>();
        let nonfinite = self.channels.iter().map(|c| c.nonfinite).sum::<u64>();
        let out_of_range = self.channels.iter().map(|c| c.out_of_range).sum::<u64>();
        let channels = self
            .channels
            .iter()
            .map(|c| ChannelStatistics {
                peak_millidbfs: level(c.peak),
                rms_millidbfs: if c.finite > 0 {
                    level((c.squares.value / c.finite as f64).sqrt())
                } else {
                    None
                },
                dc_offset: if c.finite > 0 {
                    Some(c.sum.value / c.finite as f64)
                } else {
                    None
                },
                finite_samples: c.finite,
                nonfinite_samples: c.nonfinite,
                out_of_range_samples: c.out_of_range,
            })
            .collect();
        Ok(SignalStatistics {
            schema_version: 2,
            method: METHOD.into(),
            sample_rate: self.rate,
            frames: self.frames,
            channels,
            peak_millidbfs: level(peak),
            rms_millidbfs: if finite > 0 {
                level((square_sum / finite as f64).sqrt())
            } else {
                None
            },
            silence_threshold_millidbfs: self.threshold_db,
            silence_ranges: self.silence_ranges,
            silence_ranges_exhaustive: self.silence_exhaustive,
            entirely_silent: peak == 0.0 && nonfinite == 0,
            nonfinite_samples: nonfinite,
            out_of_range_samples: out_of_range,
        })
    }
}
fn level(amplitude: f64) -> Option<i32> {
    if amplitude > 0.0 && amplitude.is_finite() {
        Some((20000.0 * amplitude.log10()).round() as i32)
    } else {
        None
    }
}
