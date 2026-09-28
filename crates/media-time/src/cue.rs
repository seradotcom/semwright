use crate::*;
use semwright_semantic_composition::{Digest, bounded_id, canonical_digest};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Anchor {
    Absolute { time: Rational },
    After { cue: String, offset: Rational },
    Unknown { reason: String },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Cue {
    pub id: String,
    pub anchor: Anchor,
    pub duration: Rational,
    pub source: Digest,
    pub method: String,
    pub version: u32,
    pub confidence: Option<u16>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResolvedCue {
    Resolved { start: Rational, end: Rational },
    Unknown { reason: String },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CueGraph {
    pub version: u32,
    pub cues: Vec<Cue>,
}
impl CueGraph {
    pub fn resolve(&self) -> Result<BTreeMap<String, ResolvedCue>> {
        ensure(
            self.version == 1 && self.cues.len() <= 512,
            "cue graph version/size",
        )?;
        let mut ids = BTreeSet::new();
        for c in &self.cues {
            bounded_id(&c.id)?;
            bounded_id(&c.method)?;
            c.duration.validate()?;
            ensure(
                c.duration >= Rational::ZERO
                    && c.version > 0
                    && c.confidence.is_none_or(|v| v <= 10000)
                    && ids.insert(c.id.clone()),
                "invalid or duplicate cue",
            )?;
        }
        let mut out = BTreeMap::new();
        for _ in 0..=self.cues.len() {
            let before = out.len();
            for c in &self.cues {
                if out.contains_key(&c.id) {
                    continue;
                }
                let start = match &c.anchor {
                    Anchor::Absolute { time } => {
                        time.validate()?;
                        Ok(*time)
                    }
                    Anchor::Unknown { reason } => {
                        bounded_id(reason)?;
                        Err(reason.clone())
                    }
                    Anchor::After { cue, offset } => {
                        offset.validate()?;
                        ensure(ids.contains(cue), "unknown cue reference")?;
                        match out.get(cue) {
                            Some(ResolvedCue::Resolved { end, .. }) => {
                                Ok(end.checked_add(*offset)?)
                            }
                            Some(ResolvedCue::Unknown { reason }) => Err(reason.clone()),
                            None => continue,
                        }
                    }
                };
                out.insert(
                    c.id.clone(),
                    match start {
                        Ok(start) => ResolvedCue::Resolved {
                            start,
                            end: start.checked_add(c.duration)?,
                        },
                        Err(reason) => ResolvedCue::Unknown { reason },
                    },
                );
            }
            if out.len() == self.cues.len() {
                return Ok(out);
            }
            if out.len() == before {
                return Err(Error::Invalid("cue dependency cycle".into()));
            }
        }
        Err(Error::Limit("cue resolution budget".into()))
    }
    pub fn digest(&self) -> Result<Digest> {
        self.resolve()?;
        canonical_digest(self)
    }
}

impl CueGraph {
    pub fn digest(
        &self,
    ) -> semwright_semantic_composition::Result<semwright_semantic_composition::Digest> {
        self.resolve()?;
        semwright_semantic_composition::canonical_digest(self)
    }
}
