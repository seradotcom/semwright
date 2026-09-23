use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const BRIDGE_PROTOCOL_VERSION: u32 = 2;
pub const MAX_MESSAGE_BYTES: usize = 1_048_576;
pub const MAX_TREE_NODES: usize = 2_000;
pub const MAX_TREE_DEPTH: usize = 32;
pub const MAX_SEARCH_RESULTS: usize = 200;
pub const MAX_BULK_MUTATIONS: usize = 256;
pub const MAX_KEYFRAMES: usize = 1_024;
pub const MAX_BINARY_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EditorType {
    Figma,
    Figjam,
    Slides,
    Buzz,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DocumentRef {
    pub session_id: String,
    pub document_id: String,
    pub generation: u64,
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NodeRef {
    pub document: DocumentRef,
    pub node_id: String,
    pub node_type: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Geometry {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub rotation: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Paint {
    Solid { r: f64, g: f64, b: f64, a: f64 },
    Gradient { stops: Vec<GradientStop> },
    Image { hash: String, scale_mode: String },
    Unknown { raw: Value },
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct GradientStop {
    pub position: f64,
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    pub mode: String,
    pub primary_sizing: String,
    pub counter_sizing: String,
    pub padding: [f64; 4],
    pub gap: f64,
    pub primary_align: String,
    pub counter_align: String,
    pub wrap: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct NodeSummary {
    pub reference: NodeRef,
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    pub geometry: Option<Geometry>,
    pub layout: Option<Layout>,
    pub fills: Vec<Paint>,
    pub children: Vec<NodeSummary>,
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PrototypeReaction {
    pub trigger: Value,
    pub actions: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MotionKeyframe {
    pub timeline_position: f64,
    pub value: Value,
    pub easing: Option<Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MotionTrack {
    pub field: Value,
    pub base_value: Option<Value>,
    pub keyframes: Vec<MotionKeyframe>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MotionTimeline {
    pub id: String,
    pub duration: f64,
    pub tracks: Vec<MotionTrack>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    pub rule: String,
    pub severity: String,
    pub message: String,
    pub node: Option<NodeRef>,
}
