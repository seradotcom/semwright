use schemars::JsonSchema;
use semwright_semantic_composition::Digest;
use serde::{Deserialize, Serialize};

pub const AUTHORING_VERSION: u32 = 1;
pub const GENERATOR_VERSION: &str = "semwright-godot-ir-v1";
pub const MAX_SPEC_BYTES: usize = 196_608;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GodotAuthoringSpec {
    pub version: u32,
    pub project: String,
    pub title: String,
    pub main_scene: String,
    pub settings: ProjectSettings,
    pub inputs: Vec<InputAction>,
    pub assets: Vec<Asset>,
    pub scenes: Vec<Scene>,
    pub limits: RuntimeLimits,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectSettings {
    pub width: u32,
    pub height: u32,
    pub physics_ticks: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuntimeLimits {
    pub actions_per_event: u32,
    pub actions_per_tick: u32,
    pub events_per_tick: u32,
    pub entities: u32,
    pub spawns_per_tick: u32,
}
impl Default for RuntimeLimits {
    fn default() -> Self {
        Self {
            actions_per_event: 128,
            actions_per_tick: 2048,
            events_per_tick: 128,
            entities: 1024,
            spawns_per_tick: 8,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InputAction {
    pub id: String,
    pub key: Key,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Key {
    Left,
    Right,
    Up,
    Down,
    Space,
    Enter,
    Escape,
    A,
    D,
    W,
    S,
    R,
}
impl Key {
    pub fn code(self) -> u32 {
        match self {
            Self::Left => 4194319,
            Self::Right => 4194321,
            Self::Up => 4194320,
            Self::Down => 4194322,
            Self::Space => 32,
            Self::Enter => 4194309,
            Self::Escape => 4194305,
            Self::A => 65,
            Self::D => 68,
            Self::W => 87,
            Self::S => 83,
            Self::R => 82,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    Glb,
    Texture,
    Audio,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    pub id: String,
    pub kind: AssetKind,
    pub file: String,
    pub sha256: Digest,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Dimension {
    Two,
    Three,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Scene {
    pub id: String,
    pub dimension: Dimension,
    pub entities: Vec<Entity>,
    #[serde(default)]
    pub materials: Vec<Material3d>,
    pub animations: Vec<Clip>,
    #[serde(default)]
    pub animation_graphs: Vec<AnimationGraph>,
    pub behavior: Behavior,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Material3d {
    pub id: String,
    pub color: [f64; 4],
    pub roughness: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MaterialSharing {
    Shared,
    LocalToScene,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MaterialBinding {
    pub material: String,
    pub sharing: MaterialSharing,
    #[serde(default)]
    pub color_override: Option<[f64; 4]>,
    #[serde(default)]
    pub roughness_override: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Entity {
    pub id: String,
    pub parent: Option<String>,
    pub position: [f64; 3],
    pub rotation: [f64; 3],
    pub scale: [f64; 3],
    pub groups: Vec<String>,
    pub node: NativeNode,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeNode {
    Node2d,
    Node3d,
    Body2d {
        shape: Shape2d,
        layer: u32,
        mask: u32,
    },
    Body3d {
        shape: Shape3d,
        layer: u32,
        mask: u32,
    },
    Area2d {
        shape: Shape2d,
        layer: u32,
        mask: u32,
    },
    Area3d {
        shape: Shape3d,
        layer: u32,
        mask: u32,
    },
    Visual2d {
        size: [f64; 2],
        color: [f64; 4],
    },
    Mesh3d {
        shape: Shape3d,
        color: [f64; 4],
    },
    Mesh3dMaterial {
        shape: Shape3d,
        material: MaterialBinding,
    },
    Camera2d {
        follow: String,
    },
    Camera3d {
        follow: String,
        offset: [f64; 3],
        fov: f64,
    },
    Label {
        text: String,
        size: u32,
    },
    Audio {
        asset: String,
    },
    Animator,
    Instance {
        asset: String,
    },
    Light {
        energy: f64,
        color: [f64; 4],
    },
    Sprite {
        asset: String,
    },
}
impl NativeNode {
    pub fn class(&self) -> &'static str {
        match self {
            Self::Node2d => "Node2D",
            Self::Node3d => "Node3D",
            Self::Body2d { .. } => "CharacterBody2D",
            Self::Body3d { .. } => "CharacterBody3D",
            Self::Area2d { .. } => "Area2D",
            Self::Area3d { .. } => "Area3D",
            Self::Visual2d { .. } => "Polygon2D",
            Self::Mesh3d { .. } | Self::Mesh3dMaterial { .. } => "MeshInstance3D",
            Self::Camera2d { .. } => "Camera2D",
            Self::Camera3d { .. } => "Camera3D",
            Self::Label { .. } => "Label",
            Self::Audio { .. } => "AudioStreamPlayer",
            Self::Animator => "AnimationPlayer",
            Self::Instance { .. } => "Node3D",
            Self::Light { .. } => "DirectionalLight3D",
            Self::Sprite { .. } => "Sprite2D",
        }
    }
    pub fn dimension(&self) -> Option<Dimension> {
        match self {
            Self::Node2d
            | Self::Body2d { .. }
            | Self::Area2d { .. }
            | Self::Visual2d { .. }
            | Self::Camera2d { .. }
            | Self::Sprite { .. } => Some(Dimension::Two),
            Self::Node3d
            | Self::Body3d { .. }
            | Self::Area3d { .. }
            | Self::Mesh3d { .. }
            | Self::Mesh3dMaterial { .. }
            | Self::Camera3d { .. }
            | Self::Instance { .. }
            | Self::Light { .. } => Some(Dimension::Three),
            _ => None,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Shape2d {
    Rectangle { size: [f64; 2] },
    Circle { radius: f64 },
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Shape3d {
    Box { size: [f64; 3] },
    Sphere { radius: f64 },
    Capsule { radius: f64, height: f64 },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ValueType {
    Bool,
    Int,
    Scalar,
    Vector2,
    Vector3,
    Color,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Literal {
    Bool(bool),
    Int(i32),
    Scalar(f64),
    Vector2([f64; 2]),
    Vector3([f64; 3]),
    Color([f64; 4]),
}
impl Literal {
    pub fn value_type(&self) -> ValueType {
        match self {
            Self::Bool(_) => ValueType::Bool,
            Self::Int(_) => ValueType::Int,
            Self::Scalar(_) => ValueType::Scalar,
            Self::Vector2(_) => ValueType::Vector2,
            Self::Vector3(_) => ValueType::Vector3,
            Self::Color(_) => ValueType::Color,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Variable {
    pub id: String,
    pub initial: Literal,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Less,
    Equal,
    And,
    Or,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Expression {
    Literal { value: Literal },
    Read { variable: String },
    Axis { negative: String, positive: String },
    Binary { op: BinaryOp, left: u16, right: u16 },
    Not { operand: u16 },
    Clamp { value: u16, min: u16, max: u16 },
    Vector2 { x: u16, y: u16 },
    Vector3 { x: u16, y: u16, z: u16 },
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Behavior {
    pub states: Vec<String>,
    pub initial_state: String,
    pub variables: Vec<Variable>,
    pub expressions: Vec<Expression>,
    pub handlers: Vec<Handler>,
    pub timers: Vec<Timer>,
    pub signals: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Timer {
    pub id: String,
    pub ticks: u32,
    pub repeat: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Event {
    Ready,
    PhysicsTick,
    Input { action: String },
    AreaEntered { area: String, body: String },
    Timer { timer: String },
    Signal { signal: String },
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Handler {
    pub id: String,
    pub event: Event,
    pub state: Option<String>,
    pub condition: Option<u16>,
    pub repeat: u16,
    pub actions: Vec<Action>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Set {
        variable: String,
        value: u16,
    },
    Transition {
        state: String,
    },
    Move2d {
        entity: String,
        velocity: u16,
        max_speed: f64,
    },
    Move3d {
        entity: String,
        velocity: u16,
        max_speed: f64,
    },
    Label {
        entity: String,
        prefix: String,
        variable: String,
    },
    Visible {
        entity: String,
        visible: bool,
    },
    Position {
        entity: String,
        value: u16,
    },
    Animate {
        entity: String,
        clip: String,
    },
    AnimationState {
        graph: String,
        state: String,
    },
    AnimationBlend {
        graph: String,
        value: u16,
    },
    PlayAudio {
        entity: String,
    },
    StartTimer {
        timer: String,
    },
    CancelTimer {
        timer: String,
    },
    Emit {
        signal: String,
    },
    Spawn {
        scene: String,
        count: u16,
    },
    Despawn {
        entity: String,
    },
    Restart,
    ChangeScene {
        scene: String,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AnimatedProperty {
    Position,
    Rotation,
    Scale,
    Visible,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Clip {
    pub id: String,
    pub animator: String,
    pub length: f64,
    pub looping: bool,
    pub tracks: Vec<Track>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Track {
    pub entity: String,
    pub property: AnimatedProperty,
    pub keys: Vec<Keyframe>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Keyframe {
    pub time: f64,
    pub value: Literal,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnimationGraph {
    pub id: String,
    pub animator: String,
    pub active: bool,
    pub root: AnimationGraphRoot,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AnimationGraphRoot {
    StateMachine {
        initial: String,
        states: Vec<AnimationState>,
        transitions: Vec<AnimationTransition>,
    },
    BlendSpace1d {
        min: f64,
        max: f64,
        initial: f64,
        sync_mode: AnimationBlendSyncMode,
        #[serde(default)]
        cyclic_length: Option<f64>,
        points: Vec<AnimationBlendPoint>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnimationState {
    pub id: String,
    pub clip: String,
    pub position: [f64; 2],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AnimationTransitionSwitch {
    Immediate,
    Sync,
    AtEnd,
}
impl AnimationTransitionSwitch {
    pub fn code(self) -> u8 {
        match self {
            Self::Immediate => 0,
            Self::Sync => 1,
            Self::AtEnd => 2,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnimationTransition {
    pub from: String,
    pub to: String,
    pub xfade_time: f64,
    pub reset: bool,
    pub switch_mode: AnimationTransitionSwitch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AnimationBlendSyncMode {
    None,
    Independent,
    CyclicMutable,
    CyclicConstant,
}
impl AnimationBlendSyncMode {
    pub fn code(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Independent => 1,
            Self::CyclicMutable => 2,
            Self::CyclicConstant => 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnimationBlendPoint {
    pub id: String,
    pub clip: String,
    pub position: f64,
}
