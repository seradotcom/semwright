//! Exact project/source-media time. Rounding occurs only at declared boundaries.
//! Musical and device clocks belong to their domain adapters, not this crate.
mod cue;
pub use cue::*;
use schemars::JsonSchema;
use semwright_semantic_composition::{ContractError as Error, Result, ensure};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq, JsonSchema)]
pub struct Rational {
    #[schemars(with = "String")]
    pub num: i64,
    #[schemars(with = "String")]
    pub den: i64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    num: String,
    den: String,
}
fn integer(s: &str) -> bool {
    let t = s.strip_prefix('-').unwrap_or(s);
    !t.is_empty()
        && t.len() <= 19
        && t.bytes().all(|b| b.is_ascii_digit())
        && (t == "0" || !t.starts_with('0'))
        && s != "-0"
}
impl Serialize for Rational {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        self.validate().map_err(serde::ser::Error::custom)?;
        Wire {
            num: self.num.to_string(),
            den: self.den.to_string(),
        }
        .serialize(s)
    }
}
impl<'de> Deserialize<'de> for Rational {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        use serde::de::Error as _;
        let w = Wire::deserialize(d)?;
        if !integer(&w.num) || !integer(&w.den) {
            return Err(D::Error::custom("noncanonical rational integer"));
        }
        let q = Self {
            num: w.num.parse().map_err(D::Error::custom)?,
            den: w.den.parse().map_err(D::Error::custom)?,
        };
        q.validate().map_err(D::Error::custom)?;
        Ok(q)
    }
}
fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        let r = a % b;
        a = b;
        b = r;
    }
    a
}
impl Rational {
    pub const ZERO: Self = Self { num: 0, den: 1 };
    pub const ONE: Self = Self { num: 1, den: 1 };
    pub fn new(num: i64, den: i64) -> Result<Self> {
        Self::wide(i128::from(num), i128::from(den))
    }
    fn wide(mut n: i128, mut d: i128) -> Result<Self> {
        match d.cmp(&0) {
            Ordering::Less => {
                n = -n;
                d = -d;
            }
            Ordering::Equal => {
                return Err(Error::Invalid("zero rational denominator".into()));
            }
            Ordering::Greater => {}
        }
        let g = gcd(n.unsigned_abs(), d as u128) as i128;
        let num =
            i64::try_from(n / g).map_err(|_| Error::Limit("rational numerator overflow".into()))?;
        let den = i64::try_from(d / g)
            .map_err(|_| Error::Limit("rational denominator overflow".into()))?;
        Ok(Self { num, den })
    }
    pub fn validate(self) -> Result<()> {
        ensure(
            self.den > 0,
            "rational must be reduced with positive denominator",
        )?;
        ensure(
            gcd(self.num.unsigned_abs().into(), self.den as u128) == 1,
            "rational must be reduced with positive denominator",
        )
    }
    pub fn checked_add(self, other: Self) -> Result<Self> {
        self.validate()?;
        other.validate()?;
        let a = i128::from(self.num) * i128::from(other.den);
        let b = i128::from(other.num) * i128::from(self.den);
        let n = a
            .checked_add(b)
            .ok_or_else(|| Error::Limit("rational addition overflow".into()))?;
        Self::wide(n, i128::from(self.den) * i128::from(other.den))
    }
    pub fn checked_sub(self, other: Self) -> Result<Self> {
        self.validate()?;
        other.validate()?;
        let a = i128::from(self.num) * i128::from(other.den);
        let b = i128::from(other.num) * i128::from(self.den);
        let n = a
            .checked_sub(b)
            .ok_or_else(|| Error::Limit("rational subtraction overflow".into()))?;
        Self::wide(n, i128::from(self.den) * i128::from(other.den))
    }
    pub fn checked_mul(self, other: Self) -> Result<Self> {
        self.validate()?;
        other.validate()?;
        Self::wide(
            i128::from(self.num) * i128::from(other.num),
            i128::from(self.den) * i128::from(other.den),
        )
    }
    pub fn checked_div(self, other: Self) -> Result<Self> {
        self.validate()?;
        other.validate()?;
        Self::wide(
            i128::from(self.num) * i128::from(other.den),
            i128::from(self.den) * i128::from(other.num),
        )
    }
    pub fn mul_i64(self, n: i64) -> Result<Self> {
        self.checked_mul(Self { num: n, den: 1 })
    }
    pub fn round(self, mode: Round) -> Result<i64> {
        self.validate()?;
        let n = i128::from(self.num);
        let d = i128::from(self.den);
        let q = n / d;
        let r = n % d;
        let x = match mode {
            Round::Floor => q - i128::from(r < 0),
            Round::Ceil => q + i128::from(r > 0),
            Round::TowardZero => q,
            Round::NearestAway => q + if r.abs() * 2 >= d { n.signum() } else { 0 },
        };
        i64::try_from(x).map_err(|_| Error::Limit("quantization overflow".into()))
    }
}
impl Ord for Rational {
    fn cmp(&self, o: &Self) -> Ordering {
        (i128::from(self.num) * i128::from(o.den)).cmp(&(i128::from(o.num) * i128::from(self.den)))
    }
}
impl PartialOrd for Rational {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Round {
    Floor,
    Ceil,
    TowardZero,
    NearestAway,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Rate {
    pub num: u32,
    pub den: u32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Quantized {
    pub index: i64,
    pub error: Rational,
}
impl Rate {
    pub fn new(num: u32, den: u32) -> Result<Self> {
        ensure(
            num > 0 && den > 0 && num <= 1_000_000 && den <= 100_000,
            "media rate bounds",
        )?;
        let g = gcd(num.into(), den.into()) as u32;
        Ok(Self {
            num: num / g,
            den: den / g,
        })
    }
    pub fn validate(self) -> Result<()> {
        ensure(
            Self::new(self.num, self.den)? == self,
            "rate must be reduced",
        )
    }
    pub fn at(self, index: i64) -> Result<Rational> {
        self.validate()?;
        Rational::wide(
            i128::from(index) * i128::from(self.den),
            i128::from(self.num),
        )
    }
    pub fn quantize(self, time: Rational, round: Round) -> Result<Quantized> {
        self.validate()?;
        let ticks = time.checked_mul(Rational::new(self.num.into(), self.den.into())?)?;
        let index = ticks.round(round)?;
        Ok(Quantized {
            index,
            error: self.at(index)?.checked_sub(time)?,
        })
    }
}
impl TryFrom<semwright_video_domain::time::FrameRate> for Rate {
    type Error = Error;
    fn try_from(f: semwright_video_domain::time::FrameRate) -> Result<Self> {
        f.validate().map_err(|e| Error::Invalid(e.to_string()))?;
        Self::new(f.num, f.den)
    }
}
impl TryFrom<Rate> for semwright_video_domain::time::FrameRate {
    type Error = Error;
    fn try_from(r: Rate) -> Result<Self> {
        r.validate()?;
        Self::new(r.num, r.den).map_err(|e| Error::Invalid(e.to_string()))
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Interval {
    pub start: Rational,
    pub end: Rational,
}
impl Interval {
    pub fn new(start: Rational, end: Rational) -> Result<Self> {
        let x = Self { start, end };
        x.validate()?;
        Ok(x)
    }
    pub fn validate(self) -> Result<()> {
        self.start.validate()?;
        self.end.validate()?;
        ensure(
            self.start < self.end,
            "half-open interval must be nonempty and ordered",
        )
    }
    pub fn duration(self) -> Result<Rational> {
        self.validate()?;
        self.end.checked_sub(self.start)
    }
    pub fn nonnegative(self) -> Result<Self> {
        self.validate()?;
        ensure(self.start >= Rational::ZERO, "negative project interval")?;
        Ok(self)
    }
    pub fn contains(self, t: Rational) -> bool {
        self.start <= t && t < self.end
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MapSegment {
    pub source: Interval,
    pub target: Interval,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TimeMap {
    pub version: u32,
    pub segments: Vec<MapSegment>,
}
impl TimeMap {
    pub fn validate(&self) -> Result<()> {
        ensure(
            self.version == 1 && !self.segments.is_empty() && self.segments.len() <= 1024,
            "time map version/size",
        )?;
        for s in &self.segments {
            s.source.validate()?;
            s.target.validate()?;
        }
        for p in self.segments.windows(2) {
            ensure(
                p[0].source.end <= p[1].source.start && p[0].target.end <= p[1].target.start,
                "overlapping or reversed time-map segments",
            )?;
        }
        Ok(())
    }
    pub fn map(&self, t: Rational) -> Result<Rational> {
        self.validate()?;
        t.validate()?;
        let s = self
            .segments
            .iter()
            .find(|s| s.source.contains(t))
            .ok_or_else(|| Error::Unknown("time outside mapped range; no extrapolation".into()))?;
        s.target.start.checked_add(
            t.checked_sub(s.source.start)?
                .checked_mul(s.target.duration()?)?
                .checked_div(s.source.duration()?)?,
        )
    }
}

pub mod artifact;
pub use artifact::*;
