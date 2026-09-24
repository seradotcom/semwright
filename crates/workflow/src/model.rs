use semwright_recipes::Recipe;
use semwright_types::{ErrorCode, Idempotency, Risk};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const TRACE_VERSION: u32 = 1;
pub const CANDIDATE_VERSION: u32 = 1;
pub const MAX_TRACE_STEPS: usize = 64;
pub const MAX_TRACES: usize = 256;
pub const MAX_PROMOTIONS: usize = 256;
pub const PATTERN_VERSION: u32 = 1;
pub const DISMISSAL_VERSION: u32 = 1;
pub const DEFAULT_MIN_OCCURRENCES: usize = 3;
pub const MAX_MIN_OCCURRENCES: usize = 32;
pub const MAX_DISMISSALS: usize = 256;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TraceStep {
    pub index: usize,
    pub command: String,
    pub args: Value,
    #[serde(default)]
    pub result: Option<Value>,
    pub ok: bool,
    #[serde(default)]
    pub error_code: Option<ErrorCode>,
    pub outcome_known: bool,
    pub risk: Risk,
    pub idempotency: Idempotency,
    pub capability_version: String,
    pub descriptor_sha256: String,
    pub backend: String,
    #[serde(default)]
    pub provider: Option<String>,
    pub duration_ms: u64,
    #[serde(default)]
    pub redacted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkflowTrace {
    pub version: u32,
    pub id: String,
    pub name: String,
    pub intent: String,
    pub started_unix_ms: u64,
    pub ended_unix_ms: u64,
    pub capture_values: bool,
    pub successful: bool,
    pub steps: Vec<TraceStep>,
}

#[derive(Debug, Clone)]
pub struct ActiveTrace {
    pub id: String,
    pub name: String,
    pub intent: String,
    pub started_unix_ms: u64,
    pub capture_values: bool,
    pub steps: Vec<TraceStep>,
    pub invalid_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ParameterHint {
    pub name: String,
    pub step: usize,
    /// JSON Pointer within the step arguments. Empty means the whole args value.
    pub pointer: String,
    #[serde(default)]
    pub secret: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub version: u32,
    pub id: String,
    pub recipe: Recipe,
    pub source_trace_ids: Vec<String>,
    pub source_descriptor_sha256: BTreeMap<String, String>,
    pub compiled_unix_ms: u64,
    pub fingerprint: String,
    #[serde(default)]
    pub static_verified: bool,
    #[serde(default)]
    pub successful_replays: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Promotion {
    pub version: u32,
    pub slug: String,
    pub capability: String,
    pub candidate: Candidate,
    pub promoted_unix_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PatternVariation {
    pub step: usize,
    pub pointer: String,
    pub kind: String,
    pub observations: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkflowPattern {
    pub version: u32,
    pub id: String,
    pub suggestion_id: String,
    pub fingerprint: String,
    pub commands: Vec<String>,
    pub occurrences: usize,
    pub compile_ready_count: usize,
    pub trace_ids: Vec<String>,
    pub compile_trace_ids: Vec<String>,
    pub first_seen_unix_ms: u64,
    pub last_seen_unix_ms: u64,
    pub suggested_name: String,
    pub varying_arguments: Vec<PatternVariation>,
    pub dismissed: bool,
    pub resurfaced: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PatternDismissal {
    pub version: u32,
    pub pattern_id: String,
    pub fingerprint: String,
    pub dismissed_unix_ms: u64,
    pub dismissed_through_occurrences: usize,
    #[serde(default)]
    pub permanent: bool,
}
