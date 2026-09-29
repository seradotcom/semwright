use crate::composition::Verdict;
use crate::{Coverage, Equivalence, Fingerprint};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Existence {
    Present,
    Missing,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Freshness {
    Current,
    Stale,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Divergence {
    Clean,
    Diverged,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProbeOutcome {
    Present,
    ConclusiveNotFound,
    Denied,
    Offline,
    Ambiguous,
    Failed,
}
impl ProbeOutcome {
    pub fn existence(self) -> Existence {
        match self {
            Self::Present => Existence::Present,
            Self::ConclusiveNotFound => Existence::Missing,
            _ => Existence::Unknown,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Knowledge {
    pub existence: Existence,
    pub freshness: Freshness,
    pub divergence: Divergence,
    pub verification: Verdict,
    pub coverage: Coverage,
    pub observed_unix_ms: Option<u64>,
    pub requires_reconcile: bool,
}
impl Knowledge {
    pub fn unknown() -> Self {
        Self {
            existence: Existence::Unknown,
            freshness: Freshness::Unknown,
            divergence: Divergence::Unknown,
            verification: Verdict::Unknown,
            coverage: Coverage::unknown(),
            observed_unix_ms: None,
            requires_reconcile: true,
        }
    }
    pub fn cache_safe(&self) -> bool {
        self.existence == Existence::Present
            && self.freshness == Freshness::Current
            && self.divergence == Divergence::Clean
            && self.verification == Verdict::Pass
            && self.coverage.cache_safe()
            && !self.requires_reconcile
    }
    /// Presentation only; callers must retain all underlying dimensions.
    pub fn label(&self) -> &'static str {
        if self.existence == Existence::Missing {
            "MISSING"
        } else if self.divergence == Divergence::Diverged {
            "DIVERGED"
        } else if self.freshness == Freshness::Stale {
            "STALE"
        } else if self.existence == Existence::Present
            && self.freshness == Freshness::Current
            && !self.requires_reconcile
        {
            "CURRENT"
        } else {
            "UNKNOWN"
        }
    }
}
/// Missing evidence never proves equality. A known changed dependency remains
/// stale even when another dependency is unobservable; coverage records that gap.
pub fn dependency_freshness<'a>(
    pins: impl IntoIterator<Item = (&'a Fingerprint, &'a Fingerprint, Equivalence)>,
    complete: bool,
) -> Freshness {
    let mut unknown = !complete;
    for (expected, observed, equivalence) in pins {
        match expected.equivalent(observed, equivalence) {
            Some(false) => return Freshness::Stale,
            None => unknown = true,
            Some(true) => (),
        }
    }
    if unknown {
        Freshness::Unknown
    } else {
        Freshness::Current
    }
}
