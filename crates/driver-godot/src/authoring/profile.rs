use super::model::GodotAuthoringSpec;
use schemars::{JsonSchema, schema_for};
use semwright_project_graph::ExecutionReceipt;
use semwright_semantic_composition::{
    BaseStateSet, Digest, ExecutionStatus, ObservationRef, State, ValidationReport,
    VerificationReport,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const RULE_MANAGED_CURRENT: &str = "godot.managed_sources_current.v1";
pub const RULE_INTENT_MATCH: &str = "godot.intent_digest_matches.v1";

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum GodotOperation {
    PublishManagedProject {
        project: String,
        target_revision: u64,
        base_fingerprint: Digest,
        manifest_digest: Digest,
        writes: Vec<String>,
        repair: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileObservationView {
    pub path: String,
    pub expected: Option<Digest>,
    pub actual: Option<Digest>,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InspectRequest {
    pub project: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlanRequest {
    pub spec: GodotAuthoringSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlanRefRequest {
    pub plan_id: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RepairMode {
    MissingManagedSource,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RepairPlanRequest {
    pub parent_plan_id: String,
    pub mode: RepairMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SnapshotView {
    pub project: String,
    pub project_id: Option<String>,
    pub exists: bool,
    pub status: String,
    pub revision: Option<u64>,
    pub fingerprint: Digest,
    pub intent_digest: Option<Digest>,
    pub bindings: BTreeMap<String, String>,
    pub files: Vec<FileObservationView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlanResult {
    pub plan_id: String,
    pub root_plan_id: String,
    pub plan_digest: Digest,
    pub effect_contract_digest: Digest,
    pub base: BaseStateSet,
    pub intent_digest: Digest,
    pub target_revision: u64,
    pub writes: Vec<String>,
    pub repair: bool,
    pub no_op: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApplyResult {
    pub plan_id: String,
    pub plan_digest: Digest,
    pub execution_status: ExecutionStatus,
    pub project: String,
    pub revision: u64,
    pub written: Vec<String>,
    pub unchanged: Vec<String>,
    pub post_fingerprint: Digest,
    pub source_state: String,
    pub controller_state: State,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeasureResult {
    pub plan_id: String,
    pub observation: ObservationRef,
    pub snapshot: SnapshotView,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ValidateResult {
    pub plan_id: String,
    pub report: ValidationReport,
    pub progress: Vec<u64>,
    pub controller_state: State,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VerifyResult {
    pub plan_id: String,
    pub report: VerificationReport,
    pub receipt: ExecutionReceipt,
    pub controller_state: State,
}

pub fn generated<T: JsonSchema>() -> Value {
    serde_json::to_value(schema_for!(T)).expect("Godot authoring schema serializes")
}

pub fn inspect_in() -> Value {
    generated::<InspectRequest>()
}
pub fn inspect_out() -> Value {
    generated::<SnapshotView>()
}
pub fn plan_in() -> Value {
    generated::<PlanRequest>()
}
pub fn plan_out() -> Value {
    generated::<PlanResult>()
}
pub fn apply_in() -> Value {
    generated::<PlanRefRequest>()
}
pub fn apply_out() -> Value {
    generated::<ApplyResult>()
}
pub fn measure_in() -> Value {
    generated::<PlanRefRequest>()
}
pub fn measure_out() -> Value {
    generated::<MeasureResult>()
}
pub fn validate_in() -> Value {
    generated::<PlanRefRequest>()
}
pub fn validate_out() -> Value {
    generated::<ValidateResult>()
}
pub fn repair_plan_in() -> Value {
    generated::<RepairPlanRequest>()
}
pub fn repair_plan_out() -> Value {
    generated::<PlanResult>()
}
pub fn repair_apply_in() -> Value {
    generated::<PlanRefRequest>()
}
pub fn repair_apply_out() -> Value {
    generated::<ApplyResult>()
}
pub fn verify_in() -> Value {
    generated::<PlanRefRequest>()
}
pub fn verify_out() -> Value {
    generated::<VerifyResult>()
}
