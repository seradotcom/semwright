use crate::{Error, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MilliDb(pub i32);
impl MilliDb {
    pub const SILENCE: Self = Self(-120_000);
    pub fn new(value: i32) -> Result<Self> {
        if !(-120_000..=24_000).contains(&value) {
            return Err(Error::invalid("Gain must be between -120 dB and +24 dB"));
        }
        Ok(Self(value))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MilliHz(pub u32);
impl MilliHz {
    pub fn new(value: u32) -> Result<Self> {
        if !(1..=192_000_000).contains(&value) {
            return Err(Error::invalid("Frequency is outside audio-domain bounds"));
        }
        Ok(Self(value))
    }
    pub fn as_hz_f64(self) -> f64 {
        f64::from(self.0) / 1000.0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Permille(pub u16);
impl Permille {
    pub fn new(value: u16) -> Result<Self> {
        if value > 1000 {
            return Err(Error::invalid("Permille value exceeds 1000"));
        }
        Ok(Self(value))
    }
}
