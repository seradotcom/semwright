use crate::*;
use schemars::JsonSchema;
use semwright_media_time::Rational as Q;
use semwright_semantic_composition::{ContractError as Error, Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    Y,
    Position,
    WorldPosition,
    Scale,
    WorldScale,
    Opacity,
    Rotation,
    Width,
    Height,
    LineStart,
    LineEnd,
    FontSize,
    Fill,
    LetterSpacing,
    CameraZoom,
    Code,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "value",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Operand {
    Number(f64),
    Vector(Point),
    Color(String),
    Text(String),
    Original,
    OriginalOffset(Point),
    OriginalScale(f64),
    Peer { subject: String, channel: Channel },
    PeerOffset { subject: String, offset: Point },
    Exploded { origin: Point, spread: f64 },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeOp {
    Tween {
        target: String,
        channel: Channel,
        from: Option<Operand>,
        to: Operand,
    },
    Set {
        target: String,
        channel: Channel,
        value: Operand,
    },
    ReactiveConnection {
        path: String,
        from: String,
        to: String,
    },
    PathFollow {
        path: String,
        marker: String,
        from: f64,
        to: f64,
        orient: bool,
    },
    CameraFollow {
        camera: String,
        target: String,
    },
    MorphPoints {
        target: String,
        from: Vec<Point>,
        to: Vec<Point>,
        closed: bool,
    },
    CodeSelection {
        target: String,
        first_line: u32,
        end_line_exclusive: u32,
    },
    Counter {
        target: String,
        from: f64,
        to: f64,
        decimal_places: u8,
        prefix: String,
        suffix: String,
    },
    Hold {
        targets: Vec<String>,
    },
    LocalRegion {
        target: String,
        overlay: String,
        center: Point,
        size: Size,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MotionEasing {
    Linear,
    InCubic,
    OutCubic,
    InOutCubic,
    OutBack,
    OutExpo,
    InOutSine,
    OutElastic,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Instruction {
    pub id: String,
    pub invocation: String,
    pub sequence: String,
    pub shot: String,
    pub start: Q,
    pub duration: Q,
    pub easing: MotionEasing,
    pub operation: NativeOp,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Invocation {
    pub id: String,
    pub span_id: String,
    pub easing: MotionEasing,
    pub primitive: Primitive,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Primitive {
    FadeIn {
        target: String,
    },
    FadeOut {
        target: String,
    },
    SlideIn {
        target: String,
        offset: Point,
    },
    SlideOut {
        target: String,
        offset: Point,
    },
    Reveal {
        target: String,
    },
    Conceal {
        target: String,
    },
    Draw {
        target: String,
    },
    Erase {
        target: String,
    },
    Settle {
        target: String,
        offset: Point,
        rotation: f64,
    },
    Emerge {
        target: String,
        scale: f64,
    },
    Resolve {
        target: String,
        tracking: f64,
    },
    Connect {
        path: String,
        from: String,
        to: String,
    },
    Group {
        targets: Vec<String>,
        anchor: String,
        gap: f64,
    },
    Separate {
        targets: Vec<String>,
        displacement: Point,
    },
    Compare {
        left: String,
        right: String,
        center: Point,
        gap: f64,
    },
    Replace {
        outgoing: String,
        incoming: String,
    },
    Swap {
        left: String,
        right: String,
    },
    Focus {
        target: String,
        others: Vec<String>,
        scale: f64,
        dim: f64,
    },
    Deemphasize {
        target: String,
        opacity: f64,
    },
    SharedElement {
        target: String,
        source: String,
    },
    CarryForward {
        target: String,
        source: String,
        offset: Point,
    },
    MatchPosition {
        target: String,
        source: String,
    },
    MatchScale {
        target: String,
        source: String,
    },
    CameraFollow {
        camera: String,
        target: String,
    },
    Morph {
        target: String,
        points: Vec<Point>,
        closed: bool,
    },
    ProgressiveDisclosure {
        targets: Vec<String>,
    },
    StepThrough {
        targets: Vec<String>,
    },
    TracePath {
        path: String,
        marker: String,
        orient: bool,
    },
    HighlightRegion {
        target: String,
        overlay: String,
        center: Point,
        size: Size,
    },
    Annotate {
        label: String,
        target: String,
        offset: Point,
    },
    ZoomContext {
        camera: String,
        zoom: f64,
    },
    ExplodeStructure {
        targets: Vec<String>,
        origin: Point,
        spread: f64,
    },
    Counter {
        target: String,
        from: f64,
        to: f64,
        decimal_places: u8,
        prefix: String,
        suffix: String,
    },
    ChartBuild {
        bars: Vec<String>,
    },
    DataTransition {
        bars: Vec<String>,
        heights: Vec<f64>,
    },
    CodeFocus {
        target: String,
        first_line: u32,
        end_line_exclusive: u32,
    },
    CodeDiff {
        target: String,
        replacement: String,
    },
    TextEmphasis {
        target: String,
        scale: f64,
        color: String,
    },
    Stagger {
        targets: Vec<String>,
        offset: Point,
    },
    Hold {
        targets: Vec<String>,
    },
}
impl Primitive {
    pub fn name(&self) -> &'static str {
        match self {
            Self::FadeIn { .. } => "fade_in",
            Self::FadeOut { .. } => "fade_out",
            Self::SlideIn { .. } => "slide_in",
            Self::SlideOut { .. } => "slide_out",
            Self::Reveal { .. } => "reveal",
            Self::Conceal { .. } => "conceal",
            Self::Draw { .. } => "draw",
            Self::Erase { .. } => "erase",
            Self::Settle { .. } => "settle",
            Self::Emerge { .. } => "emerge",
            Self::Resolve { .. } => "resolve",
            Self::Connect { .. } => "connect",
            Self::Group { .. } => "group",
            Self::Separate { .. } => "separate",
            Self::Compare { .. } => "compare",
            Self::Replace { .. } => "replace",
            Self::Swap { .. } => "swap",
            Self::Focus { .. } => "focus",
            Self::Deemphasize { .. } => "deemphasize",
            Self::SharedElement { .. } => "shared_element",
            Self::CarryForward { .. } => "carry_forward",
            Self::MatchPosition { .. } => "match_position",
            Self::MatchScale { .. } => "match_scale",
            Self::CameraFollow { .. } => "camera_follow",
            Self::Morph { .. } => "morph",
            Self::ProgressiveDisclosure { .. } => "progressive_disclosure",
            Self::StepThrough { .. } => "step_through",
            Self::TracePath { .. } => "trace_path",
            Self::HighlightRegion { .. } => "highlight_region",
            Self::Annotate { .. } => "annotate",
            Self::ZoomContext { .. } => "zoom_context",
            Self::ExplodeStructure { .. } => "explode_structure",
            Self::Counter { .. } => "counter",
            Self::ChartBuild { .. } => "chart_build",
            Self::DataTransition { .. } => "data_transition",
            Self::CodeFocus { .. } => "code_focus",
            Self::CodeDiff { .. } => "code_diff",
            Self::TextEmphasis { .. } => "text_emphasis",
            Self::Stagger { .. } => "stagger",
            Self::Hold { .. } => "hold",
        }
    }
    pub fn references(&self) -> Vec<&str> {
        match self {
            Self::Connect { path, from, to } => vec![path, from, to],
            Self::Group {
                targets, anchor, ..
            } => targets
                .iter()
                .map(String::as_str)
                .chain(std::iter::once(anchor.as_str()))
                .collect(),
            Self::Separate { targets, .. }
            | Self::ProgressiveDisclosure { targets }
            | Self::StepThrough { targets }
            | Self::ExplodeStructure { targets, .. }
            | Self::Stagger { targets, .. }
            | Self::Hold { targets } => targets.iter().map(String::as_str).collect(),
            Self::ChartBuild { bars } | Self::DataTransition { bars, .. } => {
                bars.iter().map(String::as_str).collect()
            }
            Self::Compare { left, right, .. } | Self::Swap { left, right } => vec![left, right],
            Self::Replace { outgoing, incoming } => vec![outgoing, incoming],
            Self::Focus { target, others, .. } => std::iter::once(target.as_str())
                .chain(others.iter().map(String::as_str))
                .collect(),
            Self::SharedElement { target, source }
            | Self::CarryForward { target, source, .. }
            | Self::MatchPosition { target, source }
            | Self::MatchScale { target, source } => vec![target, source],
            Self::CameraFollow { camera, target } => vec![camera, target],
            Self::TracePath { path, marker, .. } => vec![path, marker],
            Self::HighlightRegion {
                target, overlay, ..
            } => vec![target, overlay],
            Self::Annotate { label, target, .. } => vec![label, target],
            Self::ZoomContext { camera, .. } => vec![camera],
            Self::FadeIn { target }
            | Self::FadeOut { target }
            | Self::SlideIn { target, .. }
            | Self::SlideOut { target, .. }
            | Self::Reveal { target }
            | Self::Conceal { target }
            | Self::Draw { target }
            | Self::Erase { target }
            | Self::Settle { target, .. }
            | Self::Emerge { target, .. }
            | Self::Resolve { target, .. }
            | Self::Deemphasize { target, .. }
            | Self::Morph { target, .. }
            | Self::Counter { target, .. }
            | Self::CodeFocus { target, .. }
            | Self::CodeDiff { target, .. }
            | Self::TextEmphasis { target, .. } => vec![target],
        }
    }
    pub fn validate(&self, shot: &Shot) -> Result<()> {
        use crate::validate::{color, finite, id, point};
        let refs = self.references();
        ensure(
            !refs.is_empty() && refs.len() <= 128,
            "grammar subject budget",
        )?;
        let mut unique = BTreeSet::new();
        for r in refs {
            id(r)?;
            ensure(unique.insert(r), "duplicate grammar subjects")?;
        }
        let content = |target: &str| {
            shot.subjects
                .iter()
                .find(|s| s.id == target)
                .map(|s| &s.content)
                .ok_or_else(|| {
                    Error::Invalid(format!("primitive target {target} must belong to the shot"))
                })
        };
        match self {
            Self::SlideIn { offset, .. }
            | Self::SlideOut { offset, .. }
            | Self::CarryForward { offset, .. }
            | Self::Annotate { offset, .. }
            | Self::Stagger { offset, .. } => point(*offset)?,
            Self::Settle {
                offset, rotation, ..
            } => {
                point(*offset)?;
                finite(*rotation, -360.0, 360.0)?;
            }
            Self::Separate { displacement, .. } => point(*displacement)?,
            Self::Emerge { scale, .. } => finite(*scale, 0.01, 10.0)?,
            Self::Resolve { target, tracking } => {
                finite(*tracking, -100.0, 100.0)?;
                ensure(
                    matches!(content(target)?, SubjectContent::Text { .. }),
                    "resolve typography requires text",
                )?;
            }
            Self::Group { gap, .. } => finite(*gap, 0.0, 4096.0)?,
            Self::Compare { center, gap, .. } => {
                point(*center)?;
                finite(*gap, 0.0, 16384.0)?;
            }
            Self::Focus { scale, dim, .. } => {
                finite(*scale, 0.01, 10.0)?;
                finite(*dim, 0.0, 1.0)?;
            }
            Self::Deemphasize { opacity, .. } => finite(*opacity, 0.0, 1.0)?,
            Self::Draw { target } | Self::Erase { target } => ensure(
                matches!(content(target)?, SubjectContent::Path { .. }),
                "draw/erase requires path",
            )?,
            Self::Connect { path, .. } | Self::TracePath { path, .. } => ensure(
                matches!(content(path)?, SubjectContent::Path { .. }),
                "path primitive requires native path",
            )?,
            Self::CameraFollow { camera, .. } | Self::ZoomContext { camera, .. } => {
                ensure(
                    matches!(content(camera)?, SubjectContent::Camera { .. }),
                    "camera primitive requires camera",
                )?;
                if let Self::ZoomContext { zoom, .. } = self {
                    finite(*zoom, 0.01, 100.0)?;
                }
            }
            Self::Morph {
                target,
                points,
                closed,
            } => {
                let SubjectContent::Path {
                    points: old,
                    closed: was,
                    ..
                } = content(target)?
                else {
                    return Err(Error::Invalid("morph requires path".into()));
                };
                ensure(
                    old.len() == points.len() && was == closed,
                    "morph topology/closure mismatch",
                )?;
                for p in points {
                    point(*p)?;
                }
            }
            Self::HighlightRegion {
                overlay,
                center,
                size,
                ..
            } => {
                point(*center)?;
                finite(size.width, 0.0, 16384.0)?;
                finite(size.height, 0.0, 16384.0)?;
                ensure(
                    !matches!(content(overlay)?, SubjectContent::Camera { .. }),
                    "highlight overlay requires a layout box",
                )?;
            }
            Self::ExplodeStructure { origin, spread, .. } => {
                point(*origin)?;
                finite(*spread, 0.0, 10.0)?;
            }
            Self::Counter {
                target,
                from,
                to,
                decimal_places,
                prefix,
                suffix,
            } => {
                finite(*from, -1e12, 1e12)?;
                finite(*to, -1e12, 1e12)?;
                ensure(
                    *decimal_places <= 6 && prefix.len() <= 128 && suffix.len() <= 128,
                    "counter format bounds",
                )?;
                ensure(
                    matches!(content(target)?, SubjectContent::Text { .. }),
                    "counter requires native text",
                )?;
            }
            Self::ChartBuild { bars } | Self::DataTransition { bars, .. } => {
                for b in bars {
                    ensure(
                        matches!(content(b)?, SubjectContent::Rectangle { .. }),
                        "bar visualization requires rectangle",
                    )?;
                }
                if let Self::DataTransition { heights, .. } = self {
                    ensure(heights.len() == bars.len(), "data transition cardinality")?;
                    for v in heights {
                        finite(*v, 0.0, 16384.0)?;
                    }
                }
            }
            Self::CodeFocus {
                target,
                first_line,
                end_line_exclusive,
            } => {
                let SubjectContent::Code { source, .. } = content(target)? else {
                    return Err(Error::Invalid("code focus requires code".into()));
                };
                ensure(
                    first_line < end_line_exclusive
                        && (*end_line_exclusive as usize) <= source.lines().count(),
                    "code selection range",
                )?;
            }
            Self::CodeDiff {
                target,
                replacement,
            } => {
                ensure(
                    matches!(content(target)?, SubjectContent::Code { .. })
                        && replacement.len() <= 65536,
                    "code diff requires bounded code",
                )?;
            }
            Self::TextEmphasis {
                target,
                scale,
                color: fill,
            } => {
                ensure(
                    matches!(content(target)?, SubjectContent::Text { .. }),
                    "text emphasis requires text",
                )?;
                finite(*scale, 0.01, 10.0)?;
                color(fill)?;
            }
            Self::Reveal { target } | Self::Conceal { target } => ensure(
                !matches!(content(target)?, SubjectContent::Camera { .. }),
                "width reveal requires layout-capable subject",
            )?,
            _ => {}
        }
        Ok(())
    }
}
fn tween(target: &str, channel: Channel, from: Option<Operand>, to: Operand) -> NativeOp {
    NativeOp::Tween {
        target: target.into(),
        channel,
        from,
        to,
    }
}
pub fn compile_grammar(
    sequence: &str,
    shot: &Shot,
    schedule: &Schedule,
) -> Result<Vec<Instruction>> {
    let shot_range = schedule.interval(&shot.span_id)?;
    let mut out = vec![];
    for invocation in &shot.motion {
        invocation.primitive.validate(shot)?;
        let range = schedule.interval(&invocation.span_id)?;
        ensure(
            range.start >= shot_range.start && range.end <= shot_range.end,
            "motion interval outside shot",
        )?;
        let duration = range.duration()?;
        let mut serial = 0;
        let mut add = |op: NativeOp, start_fraction: Q, duration_fraction: Q| -> Result<()> {
            serial += 1;
            let d = duration.checked_mul(duration_fraction)?;
            out.push(Instruction {
                id: format!("{}-{serial}", invocation.id),
                invocation: invocation.id.clone(),
                sequence: sequence.into(),
                shot: shot.id.clone(),
                start: range
                    .start
                    .checked_add(duration.checked_mul(start_fraction)?)?,
                duration: d,
                easing: invocation.easing,
                operation: op,
            });
            Ok(())
        };
        use Channel::*;
        use Operand::*;
        use Primitive::*;
        let zero = Q::ZERO;
        let one = Q::ONE;
        match &invocation.primitive {
            FadeIn { target } => add(
                tween(target, Opacity, Some(Number(0.0)), Number(1.0)),
                zero,
                one,
            )?,
            FadeOut { target } => add(tween(target, Opacity, None, Number(0.0)), zero, one)?,
            SlideIn { target, offset } => {
                add(
                    tween(target, Position, Some(OriginalOffset(*offset)), Original),
                    zero,
                    one,
                )?;
                add(
                    tween(target, Opacity, Some(Number(0.0)), Number(1.0)),
                    zero,
                    one,
                )?;
            }
            SlideOut { target, offset } => {
                add(
                    tween(target, Position, None, OriginalOffset(*offset)),
                    zero,
                    one,
                )?;
                add(tween(target, Opacity, None, Number(0.0)), zero, one)?;
            }
            Reveal { target } => add(
                tween(target, Width, Some(OriginalScale(0.0)), Original),
                zero,
                one,
            )?,
            Conceal { target } => add(tween(target, Width, None, Number(0.0)), zero, one)?,
            Draw { target } => add(
                tween(target, LineEnd, Some(Number(0.0)), Number(1.0)),
                zero,
                one,
            )?,
            Erase { target } => add(
                tween(target, LineStart, Some(Number(0.0)), Number(1.0)),
                zero,
                one,
            )?,
            Settle {
                target,
                offset,
                rotation,
            } => {
                add(
                    tween(target, Position, Some(OriginalOffset(*offset)), Original),
                    zero,
                    one,
                )?;
                add(
                    tween(target, Rotation, Some(Number(*rotation)), Original),
                    zero,
                    one,
                )?;
            }
            Emerge { target, scale } => {
                add(
                    tween(target, Scale, Some(OriginalScale(*scale)), Original),
                    zero,
                    one,
                )?;
                add(
                    tween(target, Opacity, Some(Number(0.0)), Number(1.0)),
                    zero,
                    one,
                )?;
            }
            Resolve { target, tracking } => {
                add(
                    tween(target, LetterSpacing, Some(Number(*tracking)), Original),
                    zero,
                    one,
                )?;
                add(
                    tween(target, Opacity, Some(Number(0.0)), Number(1.0)),
                    zero,
                    one,
                )?;
            }
            Connect { path, from, to } => {
                add(
                    NativeOp::ReactiveConnection {
                        path: path.clone(),
                        from: from.clone(),
                        to: to.clone(),
                    },
                    zero,
                    one,
                )?;
                add(
                    tween(path, LineEnd, Some(Number(0.0)), Number(1.0)),
                    zero,
                    one,
                )?;
            }
            Group {
                targets,
                anchor,
                gap,
            } => {
                for (i, target) in targets.iter().enumerate() {
                    let offset = Point {
                        x: (i as f64 - (targets.len() - 1) as f64 / 2.0) * gap,
                        y: 0.0,
                    };
                    add(
                        tween(
                            target,
                            WorldPosition,
                            None,
                            PeerOffset {
                                subject: anchor.clone(),
                                offset,
                            },
                        ),
                        zero,
                        one,
                    )?;
                }
            }
            Separate {
                targets,
                displacement,
            } => {
                for (i, target) in targets.iter().enumerate() {
                    let factor = i as f64 - (targets.len() - 1) as f64 / 2.0;
                    add(
                        tween(
                            target,
                            Position,
                            None,
                            OriginalOffset(Point {
                                x: displacement.x * factor,
                                y: displacement.y * factor,
                            }),
                        ),
                        zero,
                        one,
                    )?;
                }
            }
            Compare {
                left,
                right,
                center,
                gap,
            } => {
                add(
                    tween(
                        left,
                        WorldPosition,
                        None,
                        Vector(Point {
                            x: center.x - gap / 2.0,
                            y: center.y,
                        }),
                    ),
                    zero,
                    one,
                )?;
                add(
                    tween(
                        right,
                        WorldPosition,
                        None,
                        Vector(Point {
                            x: center.x + gap / 2.0,
                            y: center.y,
                        }),
                    ),
                    zero,
                    one,
                )?;
            }
            Replace { outgoing, incoming } => {
                add(
                    NativeOp::Set {
                        target: incoming.clone(),
                        channel: WorldPosition,
                        value: Peer {
                            subject: outgoing.clone(),
                            channel: WorldPosition,
                        },
                    },
                    zero,
                    zero,
                )?;
                add(tween(outgoing, Opacity, None, Number(0.0)), zero, one)?;
                add(
                    tween(incoming, Opacity, Some(Number(0.0)), Number(1.0)),
                    zero,
                    one,
                )?;
            }
            Swap { left, right } => {
                add(
                    tween(
                        left,
                        WorldPosition,
                        None,
                        Peer {
                            subject: right.clone(),
                            channel: WorldPosition,
                        },
                    ),
                    zero,
                    one,
                )?;
                add(
                    tween(
                        right,
                        WorldPosition,
                        None,
                        Peer {
                            subject: left.clone(),
                            channel: WorldPosition,
                        },
                    ),
                    zero,
                    one,
                )?;
            }
            Focus {
                target,
                others,
                scale,
                dim,
            } => {
                add(tween(target, Scale, None, OriginalScale(*scale)), zero, one)?;
                for other in others {
                    add(tween(other, Opacity, None, Number(*dim)), zero, one)?;
                }
            }
            Deemphasize { target, opacity } => {
                add(tween(target, Opacity, None, Number(*opacity)), zero, one)?
            }
            SharedElement { target, source } => {
                add(
                    tween(
                        target,
                        WorldPosition,
                        Some(Peer {
                            subject: source.clone(),
                            channel: WorldPosition,
                        }),
                        Original,
                    ),
                    zero,
                    one,
                )?;
                add(
                    tween(
                        target,
                        WorldScale,
                        Some(Peer {
                            subject: source.clone(),
                            channel: WorldScale,
                        }),
                        Original,
                    ),
                    zero,
                    one,
                )?;
            }
            CarryForward {
                target,
                source,
                offset,
            } => {
                add(
                    NativeOp::Set {
                        target: target.clone(),
                        channel: WorldPosition,
                        value: PeerOffset {
                            subject: source.clone(),
                            offset: *offset,
                        },
                    },
                    zero,
                    zero,
                )?;
                add(
                    tween(target, Opacity, Some(Number(0.0)), Number(1.0)),
                    zero,
                    one,
                )?;
            }
            MatchPosition { target, source } => add(
                tween(
                    target,
                    WorldPosition,
                    None,
                    Peer {
                        subject: source.clone(),
                        channel: WorldPosition,
                    },
                ),
                zero,
                one,
            )?,
            MatchScale { target, source } => add(
                tween(
                    target,
                    WorldScale,
                    None,
                    Peer {
                        subject: source.clone(),
                        channel: WorldScale,
                    },
                ),
                zero,
                one,
            )?,
            CameraFollow { camera, target } => add(
                NativeOp::CameraFollow {
                    camera: camera.clone(),
                    target: target.clone(),
                },
                zero,
                one,
            )?,
            Morph {
                target,
                points,
                closed,
            } => {
                let SubjectContent::Path { points: old, .. } = &shot
                    .subjects
                    .iter()
                    .find(|s| s.id == *target)
                    .ok_or_else(|| Error::Invalid("morph target".into()))?
                    .content
                else {
                    unreachable!("validated path")
                };
                add(
                    NativeOp::MorphPoints {
                        target: target.clone(),
                        from: old.clone(),
                        to: points.clone(),
                        closed: *closed,
                    },
                    zero,
                    one,
                )?;
            }
            ProgressiveDisclosure { targets } => {
                let n = targets.len() as i64;
                for (i, target) in targets.iter().enumerate() {
                    add(
                        tween(target, Opacity, Some(Number(0.0)), Number(1.0)),
                        Q::new(i as i64, n)?,
                        Q::new(1, n)?,
                    )?;
                }
            }
            StepThrough { targets } => {
                let n = targets.len() as i64;
                for (i, target) in targets.iter().enumerate() {
                    add(
                        tween(target, Opacity, Some(Number(0.0)), Number(1.0)),
                        Q::new(i as i64, n)?,
                        Q::new(1, 2 * n)?,
                    )?;
                    add(
                        tween(target, Opacity, None, Number(0.0)),
                        Q::new(2 * i as i64 + 1, 2 * n)?,
                        Q::new(1, 2 * n)?,
                    )?;
                }
            }
            TracePath {
                path,
                marker,
                orient,
            } => add(
                NativeOp::PathFollow {
                    path: path.clone(),
                    marker: marker.clone(),
                    from: 0.0,
                    to: 1.0,
                    orient: *orient,
                },
                zero,
                one,
            )?,
            HighlightRegion {
                target,
                overlay,
                center,
                size,
            } => {
                add(
                    NativeOp::LocalRegion {
                        target: target.clone(),
                        overlay: overlay.clone(),
                        center: *center,
                        size: *size,
                    },
                    zero,
                    zero,
                )?;
                add(
                    tween(overlay, Opacity, Some(Number(0.0)), Number(1.0)),
                    zero,
                    one,
                )?;
            }
            Annotate {
                label,
                target,
                offset,
            } => {
                add(
                    NativeOp::Set {
                        target: label.clone(),
                        channel: WorldPosition,
                        value: PeerOffset {
                            subject: target.clone(),
                            offset: *offset,
                        },
                    },
                    zero,
                    zero,
                )?;
                add(
                    tween(label, Opacity, Some(Number(0.0)), Number(1.0)),
                    zero,
                    one,
                )?;
            }
            ZoomContext { camera, zoom } => {
                add(tween(camera, CameraZoom, None, Number(*zoom)), zero, one)?
            }
            ExplodeStructure {
                targets,
                origin,
                spread,
            } => {
                for target in targets {
                    add(
                        tween(
                            target,
                            WorldPosition,
                            None,
                            Exploded {
                                origin: *origin,
                                spread: *spread,
                            },
                        ),
                        zero,
                        one,
                    )?;
                }
            }
            Counter {
                target,
                from,
                to,
                decimal_places,
                prefix,
                suffix,
            } => add(
                NativeOp::Counter {
                    target: target.clone(),
                    from: *from,
                    to: *to,
                    decimal_places: *decimal_places,
                    prefix: prefix.clone(),
                    suffix: suffix.clone(),
                },
                zero,
                one,
            )?,
            ChartBuild { bars } => {
                let n = bars.len() as i64;
                for (i, b) in bars.iter().enumerate() {
                    add(
                        tween(b, Height, Some(Number(0.0)), Original),
                        Q::new(i as i64, 2 * n)?,
                        Q::new(1, 2)?,
                    )?;
                }
            }
            DataTransition { bars, heights } => {
                for (b, h) in bars.iter().zip(heights) {
                    add(tween(b, Height, None, Number(*h)), zero, one)?;
                }
            }
            CodeFocus {
                target,
                first_line,
                end_line_exclusive,
            } => add(
                NativeOp::CodeSelection {
                    target: target.clone(),
                    first_line: *first_line,
                    end_line_exclusive: *end_line_exclusive,
                },
                zero,
                one,
            )?,
            CodeDiff {
                target,
                replacement,
            } => add(
                tween(target, Code, None, Text(replacement.clone())),
                zero,
                one,
            )?,
            TextEmphasis {
                target,
                scale,
                color,
            } => {
                add(
                    tween(target, FontSize, None, OriginalScale(*scale)),
                    zero,
                    one,
                )?;
                add(tween(target, Fill, None, Color(color.clone())), zero, one)?;
            }
            Stagger { targets, offset } => {
                let n = targets.len() as i64;
                for (i, target) in targets.iter().enumerate() {
                    let start = Q::new(i as i64, 2 * n)?;
                    let half = Q::new(1, 2)?;
                    add(
                        tween(target, Position, Some(OriginalOffset(*offset)), Original),
                        start,
                        half,
                    )?;
                    add(
                        tween(target, Opacity, Some(Number(0.0)), Number(1.0)),
                        start,
                        half,
                    )?;
                }
            }
            Hold { targets } => add(
                NativeOp::Hold {
                    targets: targets.clone(),
                },
                zero,
                one,
            )?,
        }
    }
    Ok(out)
}
impl NativeOp {
    pub fn writes(&self) -> Vec<(&str, &'static str)> {
        match self {
            Self::Tween {
                target, channel, ..
            }
            | Self::Set {
                target, channel, ..
            } => vec![(
                target,
                match channel {
                    Channel::Y | Channel::Position | Channel::WorldPosition => "position",
                    Channel::Scale | Channel::WorldScale => "scale",
                    Channel::Opacity => "opacity",
                    Channel::Rotation => "rotation",
                    Channel::Width => "width",
                    Channel::Height => "height",
                    Channel::LineStart => "line_start",
                    Channel::LineEnd => "line_end",
                    Channel::FontSize => "font_size",
                    Channel::Fill => "fill",
                    Channel::LetterSpacing => "letter_spacing",
                    Channel::CameraZoom => "camera_zoom",
                    Channel::Code => "code",
                },
            )],
            Self::ReactiveConnection { path, .. } | Self::MorphPoints { target: path, .. } => {
                vec![(path, "path_points")]
            }
            Self::PathFollow { marker, orient, .. } => {
                if *orient {
                    vec![(marker, "position"), (marker, "rotation")]
                } else {
                    vec![(marker, "position")]
                }
            }
            Self::CameraFollow { camera, .. } => vec![(camera, "position")],
            Self::CodeSelection { target, .. } => vec![(target, "code_selection")],
            Self::Counter { target, .. } => vec![(target, "text")],
            Self::LocalRegion { overlay, .. } => vec![
                (overlay, "position"),
                (overlay, "width"),
                (overlay, "height"),
            ],
            Self::Hold { .. } => vec![],
        }
    }
}
pub fn validate_channels(instructions: &[Instruction]) -> Result<()> {
    let mut tracks: BTreeMap<(&str, &str), Vec<&Instruction>> = BTreeMap::new();
    for i in instructions {
        for key in i.operation.writes() {
            tracks.entry(key).or_default().push(i);
        }
    }
    for (key, items) in &mut tracks {
        items.sort_by(|a, b| a.start.cmp(&b.start).then(a.id.cmp(&b.id)));
        for pair in items.windows(2) {
            let a = pair[0];
            let b = pair[1];
            let end = a.start.checked_add(a.duration)?;
            ensure(
                end <= b.start || (a.duration == Q::ZERO && a.invocation == b.invocation),
                &format!(
                    "concurrent writes to {}:{} by {} and {}",
                    key.0, key.1, a.id, b.id
                ),
            )?;
        }
    }
    Ok(())
}
