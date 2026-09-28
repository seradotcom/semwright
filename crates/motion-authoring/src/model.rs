use crate::{Invocation, TemporalGraph};
use schemars::JsonSchema;
use semwright_media_time::{CueGraph, Rate, Rational};
use semwright_semantic_composition::Digest;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Insets {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AspectFamily {
    Landscape,
    Portrait,
    Square,
    Arbitrary,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OutputProfile {
    pub width: u32,
    pub height: u32,
    pub frame_rate: Rate,
    pub aspect: AspectFamily,
    pub safe_area: Insets,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FontFallback {
    Deny,
    AllowAndReport,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FontSpec {
    pub family: String,
    pub asset_digest: Option<Digest>,
    pub fallback: FontFallback,
    pub permitted_fallbacks: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditorialSystem {
    pub version: u32,
    pub font: FontSpec,
    pub mono_font: FontSpec,
    pub type_scale: BTreeMap<String, f64>,
    pub colors: BTreeMap<String, String>,
    pub spacing: BTreeMap<String, f64>,
    pub stroke: f64,
    pub corner_radius: f64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextRun {
    pub text: String,
    pub weight: u16,
    pub color: Option<String>,
    pub emphasis: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TextDirection {
    Auto,
    Ltr,
    Rtl,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MediaFit {
    Contain,
    Cover,
    Stretch,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CodeLanguage {
    Plain,
    Javascript,
    Typescript,
    Python,
    Rust,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SubjectContent {
    Group,
    Text {
        runs: Vec<TextRun>,
        style: String,
        direction: TextDirection,
        language: String,
        wrap: bool,
        truncate: bool,
    },
    Rectangle {
        fill: String,
        stroke: Option<String>,
        radius: f64,
    },
    Circle {
        fill: String,
        stroke: Option<String>,
    },
    Path {
        points: Vec<Point>,
        closed: bool,
        stroke: String,
        stroke_width: f64,
    },
    Image {
        asset_id: String,
        fit: MediaFit,
        ratio: f64,
    },
    Video {
        asset_id: String,
        fit: MediaFit,
        ratio: f64,
        source_offset: Rational,
    },
    Code {
        source: String,
        language: CodeLanguage,
        font_style: String,
    },
    Camera {
        zoom: f64,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    Horizontal,
    Vertical,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    Start,
    Center,
    End,
    Stretch,
    Baseline,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SpatialIntent {
    Flow {
        grow: f64,
        align: Align,
    },
    Fixed {
        position: Point,
        size: Size,
    },
    Stack {
        axis: Axis,
        gap: f64,
        padding: Insets,
        align: Align,
    },
    Split {
        ratio: f64,
        gap: f64,
        portrait_axis: Axis,
    },
    Grid {
        columns: u16,
        gap: f64,
        portrait_columns: u16,
    },
    Overlay {
        anchor: String,
        offset: Point,
        intentional: bool,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Subject {
    pub id: String,
    pub role: String,
    pub parent: Option<String>,
    pub layer: String,
    pub content: SubjectContent,
    pub layout: SpatialIntent,
    pub initially_visible: bool,
    pub clip_intentional: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    pub id: String,
    pub order: i32,
    pub intentional_overlay: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Annotation {
    pub id: String,
    pub subject: String,
    pub anchor_subject: String,
    pub offset: Point,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Caption {
    pub id: String,
    pub subject: String,
    pub cue_id: String,
    pub language: String,
    pub text: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Archetype {
    Statement,
    SplitExplanation,
    ArchitectureReveal,
    Comparison,
    Metric,
    Timeline,
    CodeFocus,
    DiagramBuild,
    ObjectSpotlight,
    EvidenceFrame,
    ProductProof,
    Endcard,
}
impl Archetype {
    pub const ALL: [Self; 12] = [
        Self::Statement,
        Self::SplitExplanation,
        Self::ArchitectureReveal,
        Self::Comparison,
        Self::Metric,
        Self::Timeline,
        Self::CodeFocus,
        Self::DiagramBuild,
        Self::ObjectSpotlight,
        Self::EvidenceFrame,
        Self::ProductProof,
        Self::Endcard,
    ];
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Shot {
    pub id: String,
    pub span_id: String,
    pub archetype: Archetype,
    pub subjects: Vec<Subject>,
    pub layers: Vec<Layer>,
    #[serde(default)]
    pub annotations: Vec<Annotation>,
    #[serde(default)]
    pub captions: Vec<Caption>,
    #[serde(default)]
    pub motion: Vec<Invocation>,
    #[serde(default)]
    pub constraints: Vec<VisualConstraint>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NarrativeRole {
    Hook,
    Problem,
    Mechanism,
    Evidence,
    Comparison,
    Reveal,
    Payoff,
    Cta,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Beat {
    pub id: String,
    pub role: NarrativeRole,
    pub shots: Vec<Shot>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Sequence {
    pub id: String,
    pub span_id: String,
    pub beats: Vec<Beat>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AssetRef {
    pub id: String,
    pub sha256: Digest,
    pub media_type: String,
    pub provenance: Option<String>,
    pub license: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "rule", rename_all = "snake_case", deny_unknown_fields)]
pub enum VisualConstraint {
    SafeArea {
        subject: String,
        tolerance: f64,
    },
    NoOverlap {
        subject: String,
        other: String,
        tolerance: f64,
    },
    MinimumVisible {
        subject: String,
        duration: Rational,
    },
    AspectRatio {
        subject: String,
        ratio: f64,
        tolerance: f64,
    },
    NativeText {
        subject: String,
    },
    FontLoaded {
        subject: String,
    },
    NoTruncation {
        subject: String,
    },
    MaximumSpeed {
        subject: String,
        pixels_per_second: f64,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Film {
    pub version: u32,
    pub id: String,
    pub output: OutputProfile,
    pub editorial: EditorialSystem,
    pub timing: TemporalGraph,
    pub sequences: Vec<Sequence>,
    pub cues: CueGraph,
    #[serde(default)]
    pub assets: Vec<AssetRef>,
}
impl Film {
    pub fn shots(&self) -> impl Iterator<Item = &Shot> {
        self.sequences
            .iter()
            .flat_map(|s| &s.beats)
            .flat_map(|b| &b.shots)
    }
}
