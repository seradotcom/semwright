use schemars::JsonSchema;
use semwright_semantic_composition::{Digest, Result, bounded_id, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ObservedValue {
    Bool { value: bool },
    Text { value: String },
    Number { value: f64, units: String },
    Members { values: BTreeSet<String> },
    Digest { value: Digest },
    Relation { target: String, binding: String },
    Preservation { before: Digest, after: Digest },
    Reopened {
        writer_process: String, reader_process: String,
        before_projection: Digest, after_projection: Digest,
        saved_digest: Digest, reopened_digest: Digest,
    },
    Artifact { digest: Digest, bytes: u64, media_type: String },
}
impl ObservedValue {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Number { value, units } => {
                ensure(value.is_finite(), "non-finite observed number")?;
                bounded_id(units)?;
            }
            Self::Text { value } => { bounded_id(value)?; }
            Self::Members { values } => {
                ensure(values.len() <= 4096, "membership budget")?;
                for v in values { bounded_id(v)?; }
            }
            Self::Relation { target, binding } => {
                bounded_id(target)?; bounded_id(binding)?;
            }
            Self::Reopened { writer_process, reader_process, .. } => {
                bounded_id(writer_process)?; bounded_id(reader_process)?;
            }
            Self::Artifact { media_type, .. } => { bounded_id(media_type)?; }
            Self::Bool { .. } | Self::Digest { .. } | Self::Preservation { .. } => {}
        }
        Ok(())
    }
}

/// Closed, versioned predicates. No JSON pointer, expression, path or user eval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Predicate {
    Equals { expected: ObservedValue },
    Within { expected: f64, tolerance: f64, units: String },
    Range { min: f64, max: f64, units: String },
    Exists { member: String },
    Absent { member: String },
    Membership { expected: BTreeSet<String> },
    Cardinality { min: u32, max: u32 },
    DigestEquals { expected: Digest },
    Relation { target: String, binding: String },
    Preserved,
    ProjectionEquals { expected: Digest },
    Reopened,
    Artifact { digest: Digest, bytes: u64, media_type: String },
}
impl Predicate {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Equals { expected } => expected.validate()?,
            Self::Within { expected, tolerance, units } => {
                ensure(expected.is_finite() && tolerance.is_finite() && *tolerance >= 0.0,
                       "invalid precommitted tolerance")?;
                bounded_id(units)?;
            }
            Self::Range { min, max, units } => {
                ensure(min.is_finite() && max.is_finite() && min <= max, "invalid range")?;
                bounded_id(units)?;
            }
            Self::Exists { member } | Self::Absent { member } => { bounded_id(member)?; }
            Self::Membership { expected } => {
                ObservedValue::Members { values: expected.clone() }.validate()?;
            }
            Self::Cardinality { min, max } => { ensure(min <= max && *max <= 4096, "cardinality budget")?; }
            Self::Relation { target, binding } => { bounded_id(target)?; bounded_id(binding)?; }
            Self::Artifact { media_type, .. } => { bounded_id(media_type)?; }
            Self::DigestEquals { .. } | Self::ProjectionEquals { .. } | Self::Preserved | Self::Reopened => {}
        }
        Ok(())
    }
    pub fn needs_complete_universe(&self) -> bool {
        matches!(self, Self::Absent { .. } | Self::Membership { .. } | Self::Cardinality { .. })
            || matches!(self, Self::Equals { expected: ObservedValue::Members { .. } })
    }
    /// None means a typed/method/units mismatch, not a false predicate.
    pub fn compare(&self, actual: &ObservedValue) -> Result<Option<bool>> {
        self.validate()?; actual.validate()?;
        Ok(match (self, actual) {
            (Self::Equals { expected }, actual) => {
                if std::mem::discriminant(expected) != std::mem::discriminant(actual) { None }
                else if let (ObservedValue::Number { units: a, .. }, ObservedValue::Number { units: b, .. }) = (expected, actual) {
                    if a != b { None } else { Some(expected == actual) }
                } else { Some(expected == actual) }
            }
            (Self::Within { expected, tolerance, units }, ObservedValue::Number { value, units: u }) if units == u =>
                Some((value - expected).abs() <= *tolerance),
            (Self::Range { min, max, units }, ObservedValue::Number { value, units: u }) if units == u =>
                Some(min <= value && value <= max),
            (Self::Exists { member }, ObservedValue::Members { values }) => Some(values.contains(member)),
            (Self::Absent { member }, ObservedValue::Members { values }) => Some(!values.contains(member)),
            (Self::Membership { expected }, ObservedValue::Members { values }) => Some(expected == values),
            (Self::Cardinality { min, max }, ObservedValue::Members { values }) => Some((*min as usize..=*max as usize).contains(&values.len())),
            (Self::DigestEquals { expected } | Self::ProjectionEquals { expected }, ObservedValue::Digest { value }) => Some(expected == value),
            (Self::Relation { target, binding }, ObservedValue::Relation { target: t, binding: b }) => Some(target == t && binding == b),
            (Self::Preserved, ObservedValue::Preservation { before, after }) => Some(before == after),
            (Self::Reopened, ObservedValue::Reopened { writer_process, reader_process, before_projection, after_projection, saved_digest, reopened_digest }) =>
                Some(writer_process != reader_process && before_projection == after_projection && saved_digest == reopened_digest),
            (Self::Artifact { digest, bytes, media_type }, ObservedValue::Artifact { digest: d, bytes: b, media_type: m }) =>
                Some(digest == d && bytes == b && media_type == m),
            _ => None,
        })
    }
}
