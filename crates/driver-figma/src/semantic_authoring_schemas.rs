use crate::semantic_authoring::{FigmaCompositionSpecV1, FigmaPlanV1};
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(JsonSchema, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct InspectInput {
    session_id: Option<String>,
    expected_revision: Option<u64>,
    root_node_id: Option<String>,
    include_design_system: Option<bool>,
    max_nodes: Option<u32>,
}

#[derive(JsonSchema, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanInput {
    session_id: Option<String>,
    expected_revision: Option<u64>,
    spec: FigmaCompositionSpecV1,
}

#[derive(JsonSchema, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplyInput {
    session_id: Option<String>,
    expected_revision: Option<u64>,
    plan: FigmaPlanV1,
}

#[derive(JsonSchema, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MeasureInput {
    session_id: Option<String>,
    expected_revision: Option<u64>,
    root_node_id: String,
    max_nodes: Option<u32>,
}

#[derive(JsonSchema, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ValidateInput {
    session_id: Option<String>,
    expected_revision: Option<u64>,
    root_node_id: String,
    spec: Option<FigmaCompositionSpecV1>,
    max_findings: Option<u32>,
}
#[derive(JsonSchema, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RepairFindingInput {
    severity: String,
    category: String,
    confidence_class: String,
    subject_node_id: Option<String>,
    subject_logical_id: Option<String>,
    expected: Option<Value>,
    actual: Option<Value>,
    evidence: Option<Value>,
}

#[derive(JsonSchema, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RepairPlanInput {
    session_id: Option<String>,
    expected_revision: u64,
    plan: FigmaPlanV1,
    findings: Vec<RepairFindingInput>,
}

#[derive(JsonSchema, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct VerifyInput {
    session_id: Option<String>,
    expected_revision: Option<u64>,
    root_node_id: String,
    spec: Option<FigmaCompositionSpecV1>,
    scale: Option<f64>,
    name: Option<String>,
    max_findings: Option<u32>,
}

fn generated<T: JsonSchema>() -> Value {
    serde_json::to_value(schema_for!(T)).expect("authoring schema serializes")
}

fn s(max: usize) -> Value {
    json!({"type":"string","minLength":1,"maxLength":max})
}
fn u(max: u64) -> Value {
    json!({"type":"integer","minimum":0,"maximum":max})
}
fn arr(max: usize, item: Value) -> Value {
    json!({"type":"array","maxItems":max,"items":item})
}
fn loose(max: usize) -> Value {
    json!({"type":"object","maxProperties":max})
}

pub fn input_schema(name: &str) -> Option<Value> {
    Some(match name {
        "composition.inspect" => generated::<InspectInput>(),
        "composition.plan" => generated::<PlanInput>(),
        "composition.apply" | "composition.repair.apply" => generated::<ApplyInput>(),
        "composition.measure" => generated::<MeasureInput>(),
        "composition.validate" => generated::<ValidateInput>(),
        "composition.repair.plan" => generated::<RepairPlanInput>(),
        "composition.verify" => generated::<VerifyInput>(),
        _ => return None,
    })
}
pub fn output_schema(name: &str) -> Option<Value> {
    Some(match name {
        "composition.plan" | "composition.repair.plan" => generated::<FigmaPlanV1>(),
        "composition.inspect" => json!({
            "type":"object",
            "properties":{
                "editorType":s(32),
                "root":loose(128),
                "designSystem":loose(16),
                "limits":loose(16)
            },
            "required":["editorType","root","designSystem","limits"],
            "additionalProperties":false
        }),
        "composition.apply" => json!({
            "type":"object",
            "properties":{
                "applied":{"type":"boolean"},
                "rootNodeIds":arr(32,s(256)),
                "created":arr(512,loose(16)),
                "logicalToNode":loose(512),
                "observedRevision":u(u64::MAX),
                "effects":arr(1024,s(512))
            },
            "required":["applied","rootNodeIds","created","logicalToNode","observedRevision","effects"],
            "additionalProperties":false
        }),
        "composition.repair.apply" => json!({
            "type":"object",
            "properties":{
                "applied":{"type":"boolean"},
                "modified":arr(64,loose(16)),
                "observedRevision":u(u64::MAX)
            },
            "required":["applied","modified","observedRevision"],
            "additionalProperties":false
        }),
        "composition.measure" => json!({
            "type":"object",
            "properties":{
                "rootNodeId":s(256),
                "nodes":arr(2000,loose(48)),
                "truncated":{"type":"boolean"},
                "observedRevision":u(u64::MAX)
            },
            "required":["rootNodeId","nodes","truncated","observedRevision"],
            "additionalProperties":false
        }),
        "composition.validate" => json!({
            "type":"object",
            "properties":{
                "status":{"type":"string","enum":["PASS","FAIL","UNKNOWN"]},
                "findings":arr(1000,loose(32)),
                "summary":loose(16),
                "observedRevision":u(u64::MAX)
            },
            "required":["status","findings","summary","observedRevision"],
            "additionalProperties":false
        }),
        "composition.verify" => json!({
            "type":"object",
            "properties":{
                "token":s(128),
                "bytes":u(16_777_216),
                "mediaType":s(128),
                "name":s(256),
                "nodeId":s(256),
                "scale":{"type":"number","minimum":0.1,"maximum":4.0},
                "measurement":loose(16),
                "validation":loose(16),
                "observedRevision":u(u64::MAX)
            },
            "required":[
                "token","bytes","mediaType","name","nodeId","scale",
                "measurement","validation","observedRevision"
            ],
            "additionalProperties":false
        }),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authoring_inputs_reject_unknown_top_level_fields() {
        for name in [
            "composition.inspect",
            "composition.plan",
            "composition.apply",
            "composition.measure",
            "composition.validate",
            "composition.repair.plan",
            "composition.repair.apply",
            "composition.verify",
        ] {
            let schema = input_schema(name).unwrap();
            assert_eq!(schema["type"], "object", "{name}");
            assert_eq!(schema["additionalProperties"], false, "{name}");
        }
    }
}
