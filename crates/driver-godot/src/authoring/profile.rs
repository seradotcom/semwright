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
    pub asset: Option<String>,
    pub revision: Option<String>,
    pub kind: Option<String>,
    pub logical_key: Option<String>,
    pub active: Option<bool>,
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
    ReconcilePartialPublication,
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
fn native_binding_schema() -> Value {
    let digest = json!({"type":"string","pattern":"^[0-9a-f]{64}$"});
    json!({
        "type":"object",
        "additionalProperties":false,
        "required":[
            "owner","request_id","project","slug","scene","plan_digest",
            "intent_digest","source_fingerprint"
        ],
        "properties":{
            "owner":{"type":"object","maxProperties":4},
            "request_id":{"type":"string","minLength":1,"maxLength":256},
            "project":{"type":"string","pattern":"^prj_[0-9a-f]{32}$"},
            "slug":{"type":"string","pattern":"^[a-z][a-z0-9_]{0,47}$"},
            "scene":{"type":"string","pattern":"^[a-z][a-z0-9_]{0,47}$"},
            "plan_digest":digest.clone(),
            "intent_digest":digest.clone(),
            "source_fingerprint":digest
        }
    })
}
fn native_effects_schema() -> Value {
    let digest = json!({"type":"string","pattern":"^[0-9a-f]{64}$"});
    json!({
        "type":"object",
        "additionalProperties":false,
        "required":["report","contract_digest","owner","request_id","coverage","vacuous"],
        "properties":{
            "report":{"type":"object","maxProperties":8},
            "contract_digest":digest,
            "owner":{"type":"object","maxProperties":4},
            "request_id":{"type":"string","minLength":1,"maxLength":256},
            "coverage":{"type":"array","maxItems":64,"items":{"type":"object","maxProperties":8}},
            "vacuous":{"type":"boolean"}
        }
    })
}
pub fn native_verify_out() -> Value {
    json!({
        "type":"object",
        "additionalProperties":false,
        "required":["kind","binding","effects"],
        "properties":{
            "kind":{"enum":["inspect","persistence","play"]},
            "binding":native_binding_schema(),
            "effects":native_effects_schema(),
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
pub fn native_query_in() -> Value {
    let property = json!({
        "type":"string",
        "minLength":1,
        "maxLength":96,
        "pattern":"^[a-z][a-z0-9_]{0,95}$"
    });
    json!({
        "type":"object",
        "additionalProperties":false,
        "required":["plan_id","scene","target"],
        "properties":{
            "plan_id":{"type":"string","minLength":1,"maxLength":256},
            "scene":{"type":"string","pattern":"^[a-z][a-z0-9_]{0,47}$"},
            "target":{
                "oneOf":[
                    {
                        "type":"object",
                        "additionalProperties":false,
                        "required":["kind","logical_key"],
                        "properties":{
                            "kind":{"const":"node"},
                            "logical_key":{"type":"string","minLength":1,"maxLength":256}
                        }
                    },
                    {
                        "type":"object",
                        "additionalProperties":false,
                        "required":["kind","path"],
                        "properties":{
                            "kind":{"const":"resource"},
                            "path":{"type":"string","minLength":7,"maxLength":1024,"pattern":"^res://"}
                        }
                    }
                ]
            },
            "properties":{"type":"array","maxItems":32,"uniqueItems":true,"items":property}
        }
    })
}

pub fn native_query_out() -> Value {
    let native_value = json!({"type":"object","maxProperties":2});
    let property_map = json!({
        "type":"object",
        "maxProperties":32,
        "additionalProperties":native_value
    });
    let node = json!({
        "type":"object",
        "additionalProperties":false,
        "required":[
            "path","class","instance_id","parent","owner","scene_file",
            "logical_id","logical_key","groups","properties"
        ],
        "properties":{
            "path":{"type":"string","maxLength":1024},
            "class":{"type":"string","maxLength":128},
            "instance_id":{"type":"string","maxLength":64},
            "parent":{"type":["string","null"],"maxLength":1024},
            "owner":{"type":["string","null"],"maxLength":1024},
            "scene_file":{"type":"string","maxLength":1024},
            "logical_id":{"type":["string","null"],"maxLength":256},
            "logical_key":{"type":["string","null"],"maxLength":256},
            "groups":{"type":"array","maxItems":64,"items":{"type":"string","maxLength":128}},
            "properties":property_map.clone()
        }
    });
    let resource_ref = json!({
        "type":"object",
        "additionalProperties":false,
        "required":["class","path","uid","instance_id","local_to_scene"],
        "properties":{
            "class":{"type":"string","maxLength":128},
            "path":{"type":"string","maxLength":1024},
            "uid":{"type":["string","null"],"maxLength":128},
            "instance_id":{"type":"string","maxLength":64},
            "local_to_scene":{"type":"boolean"}
        }
    });
    let resource = json!({
        "type":"object",
        "additionalProperties":false,
        "required":["resource","properties"],
        "properties":{
            "resource":resource_ref,
            "properties":property_map
        }
    });
    json!({
        "type":"object",
        "additionalProperties":false,
        "required":["binding","query","effects"],
        "properties":{
            "binding":native_binding_schema(),
            "effects":native_effects_schema(),
            "query":{
                "oneOf":[
                    {
                        "type":"object",
                        "additionalProperties":false,
                        "required":["kind","value"],
                        "properties":{"kind":{"const":"node"},"value":node}
                    },
                    {
                        "type":"object",
                        "additionalProperties":false,
                        "required":["kind","value"],
                        "properties":{"kind":{"const":"resource"},"value":resource}
                    }
                ]
            }
        }
    })
}

pub fn native_track_page_in() -> Value {
    json!({
        "type":"object",
        "additionalProperties":false,
        "required":["plan_id","scene","limit"],
        "properties":{
            "plan_id":{"type":"string","minLength":1,"maxLength":256},
            "scene":{"type":"string","pattern":"^[a-z][a-z0-9_]{0,47}$"},
            "cursor":{
                "type":["string","null"],
                "maxLength":100,
                "pattern":"^gtr1\\.[0-9a-f]{64}\\.[0-9]{1,10}$"
            },
            "limit":{"type":"integer","minimum":1,"maximum":64}
        }
    })
}
pub fn native_track_page_out() -> Value {
    let digest = json!({"type":"string","pattern":"^[0-9a-f]{64}$"});
    let track = json!({
        "type":"object",
        "additionalProperties":false,
        "required":["player","library","animation","root","index","track_type","path","enabled","key_count"],
        "properties":{
            "player":{"type":"string","maxLength":1024},
            "library":{"type":"string","maxLength":256},
            "animation":{"type":"string","maxLength":256},
            "root":{"type":"string","maxLength":1024},
            "index":{"type":"integer","minimum":0,"maximum":2047},
            "track_type":{"type":"integer","minimum":0,"maximum":64},
            "path":{"type":"string","maxLength":2048},
            "enabled":{"type":"boolean"},
            "key_count":{"type":"integer","minimum":0,"maximum":16384}
        }
    });
    json!({
        "type":"object",
        "additionalProperties":false,
        "required":["binding","page","effects"],
        "properties":{
            "binding":native_binding_schema(),
            "effects":native_effects_schema(),
            "page":{
                "type":"object",
                "additionalProperties":false,
                "required":["snapshot","source_fingerprint","total","tracks","next_cursor"],
                "properties":{
                    "snapshot":digest.clone(),
                    "source_fingerprint":digest,
                    "total":{"type":"integer","minimum":0,"maximum":2048},
                    "tracks":{"type":"array","maxItems":64,"items":track},
                    "next_cursor":{
                        "type":["string","null"],
                        "maxLength":100,
                        "pattern":"^gtr1\\.[0-9a-f]{64}\\.[0-9]{1,10}$"
                    }
                }
            }
        }
    })
}
pub fn native_key_page_in() -> Value {
    json!({
        "type":"object",
        "additionalProperties":false,
        "required":["plan_id","scene","player","library","animation","track_index","limit"],
        "properties":{
            "plan_id":{"type":"string","minLength":1,"maxLength":256},
            "scene":{"type":"string","pattern":"^[a-z][a-z0-9_]{0,47}$"},
            "player":{"type":"string","minLength":1,"maxLength":1024},
            "library":{"type":"string","maxLength":256},
            "animation":{"type":"string","minLength":1,"maxLength":256},
            "track_index":{"type":"integer","minimum":0,"maximum":2047},
            "cursor":{
                "type":["string","null"],
                "maxLength":100,
                "pattern":"^gky1\\.[0-9a-f]{64}\\.[0-9]{1,10}$"
            },
            "limit":{"type":"integer","minimum":1,"maximum":64}
        }
    })
}
pub fn native_key_page_out() -> Value {
    let digest = json!({"type":"string","pattern":"^[0-9a-f]{64}$"});
    let key = json!({
        "type":"object",
        "additionalProperties":false,
        "required":["index","time","transition","value"],
        "properties":{
            "index":{"type":"integer","minimum":0,"maximum":16383},
            "time":{"type":"number"},
            "transition":{"type":"number"},
            "value":{"type":"object","maxProperties":2}
        }
    });
    json!({
        "type":"object",
        "additionalProperties":false,
        "required":["binding","page","effects"],
        "properties":{
            "binding":native_binding_schema(),
            "effects":native_effects_schema(),
            "page":{
                "type":"object",
                "additionalProperties":false,
                "required":[
                    "snapshot","query_digest","source_fingerprint","player","library",
                    "animation","track_index","total","keys","next_cursor"
                ],
                "properties":{
                    "snapshot":digest.clone(),
                    "query_digest":digest.clone(),
                    "source_fingerprint":digest,
                    "player":{"type":"string","maxLength":1024},
                    "library":{"type":"string","maxLength":256},
                    "animation":{"type":"string","maxLength":256},
                    "track_index":{"type":"integer","minimum":0,"maximum":2047},
                    "total":{"type":"integer","minimum":0,"maximum":16384},
                    "keys":{"type":"array","maxItems":64,"items":key},
                    "next_cursor":{
                        "type":["string","null"],
                        "maxLength":100,
                        "pattern":"^gky1\\.[0-9a-f]{64}\\.[0-9]{1,10}$"
                    }
                }
            }
        }
    })
}
