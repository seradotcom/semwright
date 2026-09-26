//! Version 1: declarative data, never executable application source.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SCHEMA_VERSION: u32 = 1;
pub const COMPONENT_VERSION: u32 = 1;
pub const MOTION_CANVAS_VERSION: &str = "3.17.2";
pub const NODE_VERSION: &str = "22.22.0";
pub const MAX_PROJECT_BYTES: usize = 524_288;
pub const MAX_SCENES: usize = 32;
pub const MAX_NODES: usize = 1024;
pub const MAX_ANIMATIONS: usize = 4096;
pub const MAX_CUES: usize = 512;
pub const MAX_ASSETS: usize = 128;
pub const MAX_TEXT: usize = 16_384;
pub const MAX_CODE: usize = 65_536;
pub const MAX_SVG: usize = 131_072;
pub const MAX_ASSET_BYTES: usize = 16_777_216;
pub const MAX_PROJECT_MS: u64 = 600_000;
pub const MAX_SCENE_MS: u64 = 120_000;
pub const MAX_FRAMES: u64 = 18_000;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub schema_version: u32,
    pub component_version: u32,
    pub id: String,
    pub generation: String,
    pub revision: u64,
    pub settings: Settings,
    pub theme: Theme,
    #[serde(default)]
    pub variables: BTreeMap<String, SemanticValue>,
    #[serde(default)]
    pub scenes: Vec<Scene>,
    #[serde(default)]
    pub assets: Vec<Asset>,
    #[serde(default)]
    pub audio: Vec<AudioTrack>,
}
impl Project {
    pub fn empty(id: String) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            component_version: COMPONENT_VERSION,
            id,
            generation: uuid::Uuid::new_v4().simple().to_string(),
            revision: 1,
            settings: Settings::default(),
            theme: Theme::default(),
            variables: BTreeMap::new(),
            scenes: Vec::new(),
            assets: Vec::new(),
            audio: Vec::new(),
        }
    }
    pub fn duration_ms(&self) -> u64 {
        self.scenes
            .iter()
            .fold(0u64, |total, s| total.saturating_add(s.duration_ms))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub background: Option<String>,
    #[serde(default)]
    pub color_space: ColorSpace,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            fps: 30,
            background: Some("#f7f5ee".into()),
            color_space: ColorSpace::Srgb,
        }
    }
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ColorSpace {
    #[default]
    Srgb,
    DisplayP3,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Theme {
    pub font_family: String,
    pub mono_family: String,
    pub font_size: f64,
    pub font_weight: u16,
    pub spacing: f64,
    pub line_width: f64,
    pub radius: f64,
    pub colors: BTreeMap<String, String>,
}
impl Default for Theme {
    fn default() -> Self {
        Self {
            font_family: "Instrument Sans Variable".into(),
            mono_family: "IBM Plex Mono".into(),
            font_size: 36.0,
            font_weight: 500,
            spacing: 16.0,
            line_width: 2.0,
            radius: 16.0,
            colors: [
                ("paper", "#f7f5ee"),
                ("surface", "#fdfcf8"),
                ("ink", "#202d42"),
                ("muted", "#5b6471"),
                ("accent", "#234ea2"),
                ("line", "#dadbd4"),
                ("mint", "#dce9dc"),
                ("peach", "#f2ddc8"),
            ]
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Scene {
    pub id: String,
    pub name: String,
    pub duration_ms: u64,
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub animations: Vec<Animation>,
    #[serde(default)]
    pub cues: Vec<Cue>,
    #[serde(default)]
    pub transition: Option<Transition>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: String,
    pub name: String,
    pub kind: NodeKind,
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub properties: Properties,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    Group,
    Layout,
    Rect,
    Circle,
    Line,
    Text,
    Code,
    Svg,
    Image,
    Video,
    Latex,
    Camera,
    Grid,
    Polygon,
    Path,
    CubicBezier,
    QuadBezier,
    Spline,
    Knot,
    Ray,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FilterKind {
    Invert,
    Sepia,
    Grayscale,
    Brightness,
    Contrast,
    Saturate,
    Hue,
    Blur,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FilterSpec {
    pub kind: FilterKind,
    pub value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(untagged)]
pub enum LengthValue {
    Number(f64),
    Percent(String),
}
impl From<f64> for LengthValue {
    fn from(value: f64) -> Self {
        Self::Number(value)
    }
}
impl LengthValue {
    pub fn pixels(&self) -> Option<f64> {
        if let Self::Number(v) = self {
            Some(*v)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(untagged)]
pub enum GapValue {
    Single(LengthValue),
    Pair([LengthValue; 2]),
}
impl Default for GapValue {
    fn default() -> Self {
        Self::Single(0.0.into())
    }
}
impl From<f64> for GapValue {
    fn from(value: f64) -> Self {
        Self::Single(value.into())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(untagged)]
pub enum FlexBasisValue {
    Number(f64),
    Text(String),
}
impl From<f64> for FlexBasisValue {
    fn from(value: f64) -> Self {
        Self::Number(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(untagged)]
pub enum RadiusValue {
    Number(f64),
    Two([f64; 2]),
    Three([f64; 3]),
    Four([f64; 4]),
}
impl From<f64> for RadiusValue {
    fn from(value: f64) -> Self {
        Self::Number(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(untagged)]
pub enum TextWrapValue {
    Bool(bool),
    Keyword(String),
}
impl From<bool> for TextWrapValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GradientKind {
    #[default]
    Linear,
    Conic,
    Radial,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GradientStopSpec {
    pub offset: f64,
    pub color: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GradientSpec {
    #[serde(default)]
    pub kind: GradientKind,
    #[serde(default)]
    pub from: [f64; 2],
    #[serde(default)]
    pub to: [f64; 2],
    #[serde(default)]
    pub angle: f64,
    #[serde(default)]
    pub from_radius: f64,
    #[serde(default)]
    pub to_radius: f64,
    pub stops: Vec<GradientStopSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(untagged)]
pub enum SemanticValue {
    Bool(bool),
    Number(f64),
    Text(String),
    Vec2([f64; 2]),
    Spacing([f64; 4]),
    NumberList(Vec<f64>),
    Gradient(GradientSpec),
}

/// Only these properties can appear in generated source. A node-kind validator
/// further narrows them. Null in a patch resets a property to the theme default.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Properties {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<[f64; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale: Option<[f64; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<LengthValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<LengthValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stroke: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stroke_width: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radius: Option<RadiusValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_weight: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_height: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub letter_spacing: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_align: Option<TextAlign>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wrap: Option<TextWrapValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<Language>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection: Option<CodeSelection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<[f64; 2]>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_arrow: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_arrow: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arrow_size: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dash: Option<Vec<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub svg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latex: Option<LatexValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_offset_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playback_rate: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loop_media: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout: Option<Layout>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clip: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge: Option<Edge>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zoom: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<FilterSpec>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub semantic: BTreeMap<String, SemanticValue>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TextAlign {
    Left,
    Center,
    Right,
    Start,
    End,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    Plain,
    Javascript,
    Typescript,
    Python,
    Rust,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(untagged)]
pub enum LatexValue {
    Text(String),
    Parts(Vec<String>),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CodeSelection {
    Lines { start: u32, end: u32 },
    Word { line: u32, start: u32, length: u32 },
    Ranges { ranges: Vec<[[u32; 2]; 2]> },
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LayoutMode {
    Inherit,
    #[default]
    Enabled,
    Disabled,
}
fn layout_mode_is_enabled(value: &LayoutMode) -> bool {
    *value == LayoutMode::Enabled
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    #[serde(default, skip_serializing_if = "layout_mode_is_enabled")]
    pub mode: LayoutMode,
    pub direction: LayoutDirection,
    #[serde(default)]
    pub gap: GapValue,
    #[serde(default)]
    pub padding: [f64; 4],
    #[serde(default)]
    pub align: Align,
    #[serde(default)]
    pub justify: Justify,
    #[serde(default)]
    pub grow: f64,
    #[serde(default)]
    pub basis: Option<FlexBasisValue>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LayoutDirection {
    Row,
    RowReverse,
    Column,
    ColumnReverse,
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    Start,
    #[default]
    Center,
    End,
    Stretch,
    Baseline,
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Justify {
    Start,
    #[default]
    Center,
    End,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Edge {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Animation {
    pub id: String,
    pub target: String,
    pub property: AnimatedProperty,
    #[serde(default)]
    pub from: Option<AnimatedValue>,
    pub to: AnimatedValue,
    #[serde(default)]
    pub at: TimeAnchor,
    pub duration_ms: u64,
    #[serde(default)]
    pub duration_cue: Option<String>,
    #[serde(default)]
    pub easing: Easing,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TimeAnchor {
    #[serde(default)]
    pub cue: Option<String>,
    #[serde(default)]
    pub offset_ms: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(untagged)]
pub enum AnimatedValue {
    Number(f64),
    Vector([f64; 2]),
    Text(String),
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum AnimatedProperty {
    Position,
    X,
    Y,
    Scale,
    Rotation,
    Opacity,
    Fill,
    Stroke,
    Width,
    Height,
    Radius,
    Text,
    Code,
    LineStart,
    LineEnd,
    FontSize,
    LetterSpacing,
    CameraZoom,
    CameraFocus,
    Counter,
    Semantic(String),
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Easing {
    Linear,
    Sin,
    Cos,
    EaseInSine,
    EaseOutSine,
    EaseInOutSine,
    EaseInQuad,
    EaseOutQuad,
    EaseInOutQuad,
    EaseInCubic,
    EaseOutCubic,
    #[default]
    EaseInOutCubic,
    EaseInQuart,
    EaseOutQuart,
    EaseInOutQuart,
    EaseInQuint,
    EaseOutQuint,
    EaseInOutQuint,
    EaseInExpo,
    EaseOutExpo,
    EaseInOutExpo,
    EaseInCirc,
    EaseOutCirc,
    EaseInOutCirc,
    EaseInBack,
    EaseOutBack,
    EaseInOutBack,
    EaseInBounce,
    EaseOutBounce,
    EaseInOutBounce,
    EaseInElastic,
    EaseOutElastic,
    EaseInOutElastic,
}
impl Easing {
    pub const ALL: [Self; 33] = [
        Self::Linear,
        Self::Sin,
        Self::Cos,
        Self::EaseInSine,
        Self::EaseOutSine,
        Self::EaseInOutSine,
        Self::EaseInQuad,
        Self::EaseOutQuad,
        Self::EaseInOutQuad,
        Self::EaseInCubic,
        Self::EaseOutCubic,
        Self::EaseInOutCubic,
        Self::EaseInQuart,
        Self::EaseOutQuart,
        Self::EaseInOutQuart,
        Self::EaseInQuint,
        Self::EaseOutQuint,
        Self::EaseInOutQuint,
        Self::EaseInExpo,
        Self::EaseOutExpo,
        Self::EaseInOutExpo,
        Self::EaseInCirc,
        Self::EaseOutCirc,
        Self::EaseInOutCirc,
        Self::EaseInBack,
        Self::EaseOutBack,
        Self::EaseInOutBack,
        Self::EaseInBounce,
        Self::EaseOutBounce,
        Self::EaseInOutBounce,
        Self::EaseInElastic,
        Self::EaseOutElastic,
        Self::EaseInOutElastic,
    ];
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Cue {
    pub id: String,
    pub name: String,
    pub time_ms: u64,
    pub duration_ms: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Transition {
    pub kind: TransitionKind,
    pub duration_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area: Option<[f64; 4]>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransitionKind {
    Fade,
    SlideLeft,
    SlideRight,
    SlideUp,
    SlideDown,
    ZoomIn,
    ZoomOut,
}
impl TransitionKind {
    pub const ALL: [Self; 7] = [
        Self::Fade,
        Self::SlideLeft,
        Self::SlideRight,
        Self::SlideUp,
        Self::SlideDown,
        Self::ZoomIn,
        Self::ZoomOut,
    ];
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    pub id: String,
    pub kind: AssetKind,
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
    #[serde(default)]
    pub dimensions: Option<[u32; 2]>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub provenance: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    Image,
    Svg,
    Video,
    Audio,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AudioTrack {
    pub id: String,
    pub asset: String,
    #[serde(default)]
    pub offset_ms: i64,
    pub volume: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Component {
    Title,
    Subtitle,
    Badge,
    Panel,
    TerminalWindow,
    CodePanel,
    ArchitectureNode,
    ArchitectureEdge,
    CapabilityChip,
    MetricCounter,
    BrowserFrame,
    AppCard,
    Callout,
    LogoLockup,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnimationPreset {
    Fade,
    Slide,
    Reveal,
    ScalePunch,
    TrackingExpansion,
    WordReveal,
    LineReveal,
    Counter,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GroupMode {
    Parallel,
    Sequence,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RenderProfile {
    pub first_frame: u64,
    pub end_frame_exclusive: u64,
    #[serde(default)]
    pub scale: RenderScale,
    #[serde(default)]
    pub transparent: bool,
    pub timeout_ms: u64,
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RenderScale {
    Quarter,
    Half,
    #[default]
    Full,
    Double,
}
impl RenderScale {
    pub fn ratio(self) -> (u32, u32) {
        match self {
            Self::Quarter => (1, 4),
            Self::Half => (1, 2),
            Self::Full => (1, 1),
            Self::Double => (2, 1),
        }
    }
}
