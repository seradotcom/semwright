//! Frame-accurate timeline primitives. Intervals are [start, end); no float time math.
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
pub const MAX_FRAME: u64 = 100_000_000;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Frame(pub u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameRange {
    pub start: Frame,
    pub end: Frame,
}
impl FrameRange {
    pub fn new(start: u64, end: u64) -> Result<Self> {
        if start >= end || end > MAX_FRAME {
            return Err(Error::invalid(
                "Frame range must be nonempty, ordered and bounded",
            ));
        }
        Ok(Self {
            start: Frame(start),
            end: Frame(end),
        })
    }
    pub fn duration(self) -> u64 {
        self.end.0 - self.start.0
    }
    pub fn inclusive(self) -> (u64, u64) {
        (self.start.0, self.end.0 - 1)
    }
    pub fn from_inclusive(start: u64, out: u64) -> Result<Self> {
        Self::new(
            start,
            out.checked_add(1)
                .ok_or_else(|| Error::invalid("Inclusive range end overflow"))?,
        )
    }
    pub fn split(self, at: u64) -> Result<(Self, Self)> {
        if at <= self.start.0 || at >= self.end.0 {
            return Err(Error::invalid("Split must be strictly inside range"));
        }
        Ok((Self::new(self.start.0, at)?, Self::new(at, self.end.0)?))
    }
    pub fn overlaps(self, other: Self) -> bool {
        self.start < other.end && other.start < self.end
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameRate {
    pub num: u32,
    pub den: u32,
}
fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        let r = a % b;
        a = b;
        b = r;
    }
    a
}
impl FrameRate {
    pub fn new(num: u32, den: u32) -> Result<Self> {
        if num == 0
            || den == 0
            || num > 240_000
            || den > 100_000
            || u64::from(num) > 240 * u64::from(den)
        {
            return Err(Error::invalid("Unsupported rational frame rate"));
        }
        let g = gcd(num, den);
        Ok(Self {
            num: num / g,
            den: den / g,
        })
    }
    pub fn validate(self) -> Result<Self> {
        let v = Self::new(self.num, self.den)?;
        if v != self {
            return Err(Error::invalid("Frame rate must be reduced"));
        }
        Ok(v)
    }
    pub fn nominal(self) -> u64 {
        (u64::from(self.num) + u64::from(self.den) / 2) / u64::from(self.den)
    }
    pub fn milliseconds(self, frame: u64) -> Result<u64> {
        self.validate()?;
        if frame > MAX_FRAME {
            return Err(Error::limit("Frame exceeds limit"));
        }
        Ok(
            ((u128::from(frame) * u128::from(self.den) * 1000 + u128::from(self.num) / 2)
                / u128::from(self.num)) as u64,
        )
    }
    /// Millisecond timestamps are quantized: nearest frame, ties up, not truncation.
    pub fn from_milliseconds(self, ms: u64) -> Result<u64> {
        self.validate()?;
        let d = u128::from(self.den) * 1000;
        let f = (u128::from(ms) * u128::from(self.num) + d / 2) / d;
        if f > u128::from(MAX_FRAME) {
            return Err(Error::limit("Time exceeds frame limit"));
        }
        Ok(f as u64)
    }
    pub fn parse_clock_or_frame(self, text: &str) -> Result<u64> {
        if text.len() > 32 {
            return Err(Error::invalid("Time token exceeds limit"));
        }
        if text.bytes().all(|c| c.is_ascii_digit()) && !text.is_empty() {
            let f = text
                .parse::<u64>()
                .map_err(|_| Error::invalid("Frame overflow"))?;
            if f > MAX_FRAME {
                return Err(Error::limit("Frame limit"));
            }
            return Ok(f);
        }
        let parts: Vec<_> = text.split(':').collect();
        if parts.len() != 3 {
            return Err(Error::invalid("Expected frame number or hh:mm:ss.mmm"));
        }
        let h = parse_uint(parts[0])?;
        let m = parse_uint(parts[1])?;
        let (sec, frac) = parts[2]
            .split_once('.')
            .ok_or_else(|| Error::invalid("Milliseconds required"))?;
        if m >= 60 || frac.len() != 3 {
            return Err(Error::invalid("Invalid media clock"));
        }
        let s = parse_uint(sec)?;
        let ms = parse_uint(frac)?;
        if s >= 60 {
            return Err(Error::invalid("Seconds out of range"));
        }
        let millis = h
            .checked_mul(3_600_000)
            .and_then(|v| v.checked_add(m * 60_000 + s * 1000 + ms))
            .ok_or_else(|| Error::invalid("Time overflow"))?;
        self.from_milliseconds(millis)
    }
    pub fn parse_timecode(self, text: &str) -> Result<u64> {
        self.validate()?;
        if text.len() != 11 || !text.is_ascii() {
            return Err(Error::invalid("Expected hh:mm:ss:ff or hh:mm:ss;ff"));
        }
        let b = text.as_bytes();
        if b[2] != b':' || b[5] != b':' || !matches!(b[8], b':' | b';') {
            return Err(Error::invalid("Invalid timecode separators"));
        }
        let h = parse_uint(&text[0..2])?;
        let m = parse_uint(&text[3..5])?;
        let s = parse_uint(&text[6..8])?;
        let f = parse_uint(&text[9..11])?;
        let nominal = self.nominal();
        if h >= 24 || m >= 60 || s >= 60 || f >= nominal {
            return Err(Error::invalid("Timecode field out of range"));
        }
        let total_minutes = h * 60 + m;
        let mut frames = (h * 3600 + m * 60 + s) * nominal + f;
        if b[8] == b';' {
            let drop = match (self.num, self.den) {
                (30000, 1001) => 2,
                (60000, 1001) => 4,
                _ => {
                    return Err(Error::unsupported(
                        "Drop-frame only supports 30000/1001 and 60000/1001",
                    ));
                }
            };
            if m % 10 != 0 && s == 0 && f < drop {
                return Err(Error::invalid("Nonexistent drop-frame label"));
            }
            frames -= drop * (total_minutes - total_minutes / 10);
        }
        if frames > MAX_FRAME {
            return Err(Error::limit("Timecode exceeds limit"));
        }
        Ok(frames)
    }
    pub fn timecode(self, frames: u64, drop_frame: bool) -> Result<String> {
        self.validate()?;
        let n = self.nominal();
        if n == 0 || n > 99 {
            return Err(Error::unsupported("Timecode requires nominal fps 1..99"));
        }
        let mut labels = frames;
        if drop_frame {
            let d = match (self.num, self.den) {
                (30000, 1001) => 2,
                (60000, 1001) => 4,
                _ => return Err(Error::unsupported("Drop-frame rate unsupported")),
            };
            let ten = n * 600 - d * 9;
            let minute = n * 60 - d;
            let days = frames % (ten * 6 * 24);
            let blocks = days / ten;
            let remainder = days % ten;
            labels = days
                + d * 9 * blocks
                + if remainder >= d {
                    d * ((remainder - d) / minute)
                } else {
                    0
                };
        }
        labels %= n * 86400;
        let f = labels % n;
        let seconds = labels / n;
        Ok(format!(
            "{:02}:{:02}:{:02}{}{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60,
            if drop_frame { ';' } else { ':' },
            f
        ))
    }
}
fn parse_uint(t: &str) -> Result<u64> {
    if t.is_empty() || !t.bytes().all(|c| c.is_ascii_digit()) {
        return Err(Error::invalid("Unsigned decimal field required"));
    }
    t.parse().map_err(|_| Error::invalid("Integer overflow"))
}
