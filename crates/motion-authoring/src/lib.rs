//! Authoring intent above video-domain and managed Motion Canvas, never an NLE replacement.
mod grammar;
mod measurement;
mod model;
mod temporal;
mod validate;
pub use grammar::*;
pub use measurement::*;
pub use model::*;
use schemars::JsonSchema;
use semwright_media_time::Interval;
use semwright_semantic_composition::{Result, canonical_digest, ensure};
use serde::{Deserialize, Serialize};
pub use temporal::*;

pub const AUTHORING_VERSION: u32 = 1;
pub fn shot_root(id: &str) -> String {
    format!("sw-shot-{id}")
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RealizedScene {
    pub id: String,
    pub interval: Interval,
    pub shots: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Realization {
    pub version: u32,
    pub scenes: Vec<RealizedScene>,
    pub schedule: Schedule,
    pub instructions: Vec<Instruction>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ManagedBinding {
    pub schema_version: u32,
    pub intent: Film,
    pub intent_digest: semwright_semantic_composition::Digest,
    pub realization: Realization,
    pub realization_digest: semwright_semantic_composition::Digest,
    pub projection_digest: semwright_semantic_composition::Digest,
}
impl ManagedBinding {
    pub fn validate(&self) -> Result<()> {
        ensure(self.schema_version == 1, "authoring binding version")?;
        ensure(
            self.intent_digest == canonical_digest(&self.intent)?
                && self.realization_digest == canonical_digest(&self.realization)?,
            "authoring intent/realization digest drift",
        )?;
        ensure(
            self.realization == realize(&self.intent)?,
            "authoring realization is not the deterministic compiler output",
        )
    }
}
pub fn realize(film: &Film) -> Result<Realization> {
    film.validate()?;
    let schedule = film.timing.solve(&film.cues)?;
    let mut scenes = vec![];
    let mut instructions = vec![];
    for sequence in &film.sequences {
        let interval = schedule.interval(&sequence.span_id)?;
        let mut shots = vec![];
        for shot in sequence.beats.iter().flat_map(|b| &b.shots) {
            let range = schedule.interval(&shot.span_id)?;
            ensure(
                range.start >= interval.start && range.end <= interval.end,
                "shot lies outside sequence",
            )?;
            let values = compile_grammar(&sequence.id, shot, &schedule)?;
            ensure(
                values.len() + instructions.len() <= 4096,
                "instruction budget",
            )?;
            instructions.extend(values);
            shots.push(shot.id.clone());
        }
        scenes.push(RealizedScene {
            id: sequence.id.clone(),
            interval,
            shots,
        });
    }
    scenes.sort_by(|a, b| {
        a.interval
            .start
            .cmp(&b.interval.start)
            .then(a.id.cmp(&b.id))
    });
    let mut boundary = semwright_media_time::Rational::ZERO;
    for scene in &scenes {
        ensure(
            scene.interval.start == boundary,
            "sequences must tile the delivery timeline without silent gaps/overlap",
        )?;
        boundary = scene.interval.end;
    }
    ensure(
        boundary == film.timing.duration,
        "sequence coverage does not match delivery duration",
    )?;
    instructions.sort_by(|a, b| a.start.cmp(&b.start).then(a.id.cmp(&b.id)));
    validate_channels(&instructions)?;
    Ok(Realization {
        version: 1,
        scenes,
        schedule,
        instructions,
    })
}
