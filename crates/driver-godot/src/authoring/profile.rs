use super::model::GodotAuthoringSpec;
use schemars::{JsonSchema, schema_for};
use semwright_project_graph::ExecutionReceipt;
use semwright_semantic_composition::{
    BaseStateSet, Digest, ExecutionStatus, ObservationRef, State, ValidationReport,
    VerificationReport,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
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
    json!({
        "type":"object",
        "additionalProperties":false,
        "required":["spec"],
        "properties":{
            "spec":{
                "type":"object",
                "additionalProperties":false,
                "required":[
                    "version","project","title","main_scene","settings",
                    "inputs","assets","scenes","limits"
                ],
                "properties":{
                    "version":{"type":"integer","const":1},
                    "project":{"type":"string","pattern":"^[a-z][a-z0-9_]{0,47}$"},
                    "title":{"type":"string","minLength":1,"maxLength":128},
                    "main_scene":{"type":"string","pattern":"^[a-z][a-z0-9_]{0,47}$"},
                    "settings":{"type":"object","maxProperties":8},
                    "inputs":{"type":"array","maxItems":32},
                    "assets":{"type":"array","maxItems":64},
                    "scenes":{"type":"array","minItems":1,"maxItems":16},
                    "limits":{"type":"object","maxProperties":8}
                }
            }
        }
    })
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
pub fn native_verify_in() -> Value {
    let identifier = json!({"type":"string","pattern":"^[a-z][a-z0-9_]{0,47}$"});
    let input = json!({
        "type":"object",
        "additionalProperties":false,
        "required":["tick","action","pressed"],
        "properties":{
            "tick":{"type":"integer","minimum":1,"maximum":3600},
            "action":identifier.clone(),
            "pressed":{"type":"boolean"}
        }
    });
    json!({
        "type":"object",
        "additionalProperties":false,
        "required":["plan_id","scene","verification"],
        "properties":{
            "plan_id":{"type":"string","minLength":1,"maxLength":256},
            "scene":identifier.clone(),
            "verification":{
                "oneOf":[
                    {
                        "type":"object",
                        "additionalProperties":false,
                        "required":["kind"],
                        "properties":{"kind":{"const":"inspect"}}
                    },
                    {
                        "type":"object",
                        "additionalProperties":false,
                        "required":["kind"],
                        "properties":{"kind":{"const":"persistence"}}
                    },
                    {
                        "type":"object",
                        "additionalProperties":false,
                        "required":["kind","ticks","inputs","checkpoints","variables","capture"],
                        "properties":{
                            "kind":{"const":"play"},
                            "ticks":{"type":"integer","minimum":1,"maximum":3600},
                            "inputs":{"type":"array","maxItems":256,"items":input},
                            "checkpoints":{
                                "type":"array","maxItems":32,
                                "items":{"type":"integer","minimum":1,"maximum":3600}
                            },
                            "variables":{"type":"array","maxItems":64,"items":identifier},
                            "capture":{"type":"boolean"}
                        }
                    }
                ]
            }
        }
    })
}
pub fn native_verify_out() -> Value {
    let digest = json!({"type":"string","pattern":"^[0-9a-f]{64}$"});
    let binding = json!({
        "type":"object",
        "additionalProperties":false,
        "required":[
            "owner","request_id","project","slug","plan_digest",
            "intent_digest","source_fingerprint"
        ],
        "properties":{
            "owner":{"type":"object"},
            "request_id":{"type":"string","minLength":1,"maxLength":256},
            "project":{"type":"string","pattern":"^prj_[0-9a-f]{32}$"},
            "slug":{"type":"string","pattern":"^[a-z][a-z0-9_]{0,47}$"},
            "plan_digest":digest.clone(),
            "intent_digest":digest.clone(),
            "source_fingerprint":digest
        }
    });
    json!({
        "type":"object",
        "additionalProperties":false,
        "required":["kind","binding"],
        "properties":{
            "kind":{"enum":["inspect","persistence","play"]},
            "binding":binding,
            "observation":{"type":"object"},
            "writer":{"type":"object"},
            "reader":{"type":"object"},
            "evidence":{"type":"object"}
        },
        "oneOf":[
            {"properties":{"kind":{"const":"inspect"}},"required":["observation"]},
            {"properties":{"kind":{"const":"persistence"}},"required":["writer","reader","evidence"]},
            {"properties":{"kind":{"const":"play"}},"required":["observation"]}
        ]
    })
}
