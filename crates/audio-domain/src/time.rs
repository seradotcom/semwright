use crate::{Error, Result};
use serde::{Deserialize, Serialize};

pub const MAX_SAMPLE_FRAME: u64 = 384_000 * 60 * 60 * 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SampleRate(pub u32);

impl SampleRate {
    pub fn new(hz: u32) -> Result<Self> {
        if !(8_000..=384_000).contains(&hz) {
            return Err(Error::invalid(
                "Sample rate must be between 8 kHz and 384 kHz",
            ));
        }
        Ok(Self(hz))
    }
    pub const fn hz(self) -> u32 {
        self.0
    }
    pub fn validate(self) -> Result<()> {
        Self::new(self.0).map(|_| ())
    }
}

impl Default for SampleRate {
    fn default() -> Self {
        Self(48_000)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SampleFrame(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SampleRange {
    pub start: SampleFrame,
    pub end: SampleFrame,
}

impl SampleRange {
    pub fn new(start: u64, end: u64) -> Result<Self> {
        if start >= end || end > MAX_SAMPLE_FRAME {
            return Err(Error::invalid("Invalid half-open sample range"));
        }
        Ok(Self {
            start: SampleFrame(start),
            end: SampleFrame(end),
        })
    }
    pub fn duration(self) -> u64 {
        self.end.0 - self.start.0
    }
    pub fn contains(self, frame: SampleFrame) -> bool {
        frame.0 >= self.start.0 && frame.0 < self.end.0
    }
}

pub fn milliseconds_to_frames(ms: u64, rate: SampleRate) -> Result<u64> {
    let frames = ms
        .checked_mul(u64::from(rate.0))
        .and_then(|v| v.checked_add(999))
        .map(|v| v / 1000)
        .ok_or_else(|| Error::limit("Time conversion overflow"))?;
    if frames > MAX_SAMPLE_FRAME {
        return Err(Error::limit("Time exceeds audio domain budget"));
    }
    Ok(frames)
}
