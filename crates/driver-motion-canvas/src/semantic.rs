//! Version-pinned semantic substrate for public Motion Canvas 2D node properties.
use crate::{
    Error, Result,
    model::{MAX_SVG, MAX_TEXT, NodeKind, SemanticValue, Theme},
    security,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ValueKind {
    Boolean,
    Number,
    Integer,
    Text,
    Color,
    Vec2,
    Spacing,
    NumberList,
    Vec2List,
    Enum,
    PathData,
    FilterList,
    LayoutBundle,
    CodeSelection,
    AssetRef,
    EdgeRef,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PropertyStorage {
    Explicit,
    Semantic,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct PropertyDescriptor {
    pub semantic_name: String,
    pub upstream_name: String,
    pub value_kind: ValueKind,
    pub storage: PropertyStorage,
    pub animatable: bool,
    pub enum_values: Vec<String>,
    pub source_interface: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct NodeTypeDescriptor {
    pub kind: NodeKind,
    pub upstream_class: String,
    pub can_have_children: bool,
    pub properties: Vec<PropertyDescriptor>,
}

#[derive(Clone, Copy)]
struct Spec {
    name: &'static str,
    upstream: &'static str,
    kind: ValueKind,
    storage: PropertyStorage,
    animatable: bool,
    enums: &'static [&'static str],
    source: &'static str,
}
const fn e(
    name: &'static str,
    upstream: &'static str,
    kind: ValueKind,
    animatable: bool,
    source: &'static str,
) -> Spec {
    Spec {
        name,
        upstream,
        kind,
        storage: PropertyStorage::Explicit,
        animatable,
        enums: &[],
        source,
    }
}
const fn ee(
    name: &'static str,
    upstream: &'static str,
    values: &'static [&'static str],
    animatable: bool,
    source: &'static str,
) -> Spec {
    Spec {
        name,
        upstream,
        kind: ValueKind::Enum,
        storage: PropertyStorage::Explicit,
        animatable,
        enums: values,
        source,
    }
}
const fn s(
    name: &'static str,
    upstream: &'static str,
    kind: ValueKind,
    animatable: bool,
    source: &'static str,
) -> Spec {
    Spec {
        name,
        upstream,
        kind,
        storage: PropertyStorage::Semantic,
        animatable,
        enums: &[],
        source,
    }
}
const fn se(
    name: &'static str,
    upstream: &'static str,
    values: &'static [&'static str],
    animatable: bool,
    source: &'static str,
) -> Spec {
    Spec {
        name,
        upstream,
        kind: ValueKind::Enum,
        storage: PropertyStorage::Semantic,
        animatable,
        enums: values,
        source,
    }
}

const COMPOSITE: &[&str] = &[
    "source-over",
    "source-in",
    "source-out",
    "source-atop",
    "destination-over",
    "destination-in",
    "destination-out",
    "destination-atop",
    "lighter",
    "copy",
    "xor",
    "multiply",
    "screen",
    "overlay",
    "darken",
    "lighten",
    "color-dodge",
    "color-burn",
    "hard-light",
    "soft-light",
    "difference",
    "exclusion",
    "hue",
    "saturation",
    "color",
    "luminosity",
];
const LINE_JOIN: &[&str] = &["bevel", "round", "miter"];
const LINE_CAP: &[&str] = &["butt", "round", "square"];
const TEXT_ALIGN: &[&str] = &["left", "center", "right"];
const LANGUAGE: &[&str] = &["plain", "javascript", "typescript", "python", "rust"];
const FONT_STYLE: &[&str] = &["normal", "italic", "oblique"];
const TEXT_DIRECTION: &[&str] = &["inherit", "ltr", "rtl"];
const FLEX_WRAP: &[&str] = &["nowrap", "wrap", "wrap-reverse"];
const FLEX_CONTENT: &[&str] = &[
    "start",
    "end",
    "center",
    "space-between",
    "space-around",
    "space-evenly",
    "stretch",
];
const FLEX_ITEMS: &[&str] = &["start", "end", "center", "stretch"];

const NODE: &[Spec] = &[
    e("position", "position", ValueKind::Vec2, true, "NodeProps"),
    e("scale", "scale", ValueKind::Vec2, true, "NodeProps"),
    e("rotation", "rotation", ValueKind::Number, true, "NodeProps"),
    e("opacity", "opacity", ValueKind::Number, true, "NodeProps"),
    e(
        "filters",
        "filters",
        ValueKind::FilterList,
        false,
        "NodeProps",
    ),
    s("skew", "skew", ValueKind::Vec2, true, "NodeProps"),
    s("z_index", "zIndex", ValueKind::Integer, true, "NodeProps"),
    s(
        "shadow_color",
        "shadowColor",
        ValueKind::Color,
        true,
        "NodeProps",
    ),
    s(
        "shadow_blur",
        "shadowBlur",
        ValueKind::Number,
        true,
        "NodeProps",
    ),
    s(
        "shadow_offset",
        "shadowOffset",
        ValueKind::Vec2,
        true,
        "NodeProps",
    ),
    s("cache", "cache", ValueKind::Boolean, false, "NodeProps"),
    s(
        "cache_padding",
        "cachePadding",
        ValueKind::Spacing,
        false,
        "NodeProps",
    ),
    s(
        "composite",
        "composite",
        ValueKind::Boolean,
        false,
        "NodeProps",
    ),
    se(
        "composite_operation",
        "compositeOperation",
        COMPOSITE,
        false,
        "NodeProps",
    ),
];
const LAYOUT: &[Spec] = &[
    e("width", "width", ValueKind::Number, true, "LayoutProps"),
    e("height", "height", ValueKind::Number, true, "LayoutProps"),
    e(
        "font_family",
        "fontFamily",
        ValueKind::Text,
        true,
        "LayoutProps",
    ),
    e(
        "font_size",
        "fontSize",
        ValueKind::Number,
        true,
        "LayoutProps",
    ),
    e(
        "font_weight",
        "fontWeight",
        ValueKind::Integer,
        true,
        "LayoutProps",
    ),
    e(
        "line_height",
        "lineHeight",
        ValueKind::Number,
        true,
        "LayoutProps",
    ),
    e(
        "letter_spacing",
        "letterSpacing",
        ValueKind::Number,
        true,
        "LayoutProps",
    ),
    ee("text_align", "textAlign", TEXT_ALIGN, true, "LayoutProps"),
    e("wrap", "textWrap", ValueKind::Boolean, true, "LayoutProps"),
    e(
        "layout",
        "layout",
        ValueKind::LayoutBundle,
        false,
        "LayoutProps",
    ),
    e("clip", "clip", ValueKind::Boolean, true, "LayoutProps"),
    s(
        "min_width",
        "minWidth",
        ValueKind::Number,
        true,
        "LayoutProps",
    ),
    s(
        "max_width",
        "maxWidth",
        ValueKind::Number,
        true,
        "LayoutProps",
    ),
    s(
        "min_height",
        "minHeight",
        ValueKind::Number,
        true,
        "LayoutProps",
    ),
    s(
        "max_height",
        "maxHeight",
        ValueKind::Number,
        true,
        "LayoutProps",
    ),
    s("ratio", "ratio", ValueKind::Number, true, "LayoutProps"),
    s("margin", "margin", ValueKind::Spacing, true, "LayoutProps"),
    s(
        "padding",
        "padding",
        ValueKind::Spacing,
        true,
        "LayoutProps",
    ),
    s("shrink", "shrink", ValueKind::Number, true, "LayoutProps"),
    s("row_gap", "rowGap", ValueKind::Number, true, "LayoutProps"),
    s(
        "column_gap",
        "columnGap",
        ValueKind::Number,
        true,
        "LayoutProps",
    ),
    se("flex_wrap", "wrap", FLEX_WRAP, true, "LayoutProps"),
    se(
        "align_content",
        "alignContent",
        FLEX_CONTENT,
        true,
        "LayoutProps",
    ),
    se("align_self", "alignSelf", FLEX_ITEMS, true, "LayoutProps"),
    se("font_style", "fontStyle", FONT_STYLE, true, "LayoutProps"),
    se(
        "text_direction",
        "textDirection",
        TEXT_DIRECTION,
        true,
        "LayoutProps",
    ),
];
const SHAPE: &[Spec] = &[
    e("fill", "fill", ValueKind::Color, true, "ShapeProps"),
    e("stroke", "stroke", ValueKind::Color, true, "ShapeProps"),
    e(
        "stroke_width",
        "lineWidth",
        ValueKind::Number,
        true,
        "ShapeProps",
    ),
    e(
        "dash",
        "lineDash",
        ValueKind::NumberList,
        true,
        "ShapeProps",
    ),
    s(
        "stroke_first",
        "strokeFirst",
        ValueKind::Boolean,
        true,
        "ShapeProps",
    ),
    se("line_join", "lineJoin", LINE_JOIN, true, "ShapeProps"),
    se("line_cap", "lineCap", LINE_CAP, true, "ShapeProps"),
    s(
        "line_dash_offset",
        "lineDashOffset",
        ValueKind::Number,
        true,
        "ShapeProps",
    ),
    s(
        "antialiased",
        "antialiased",
        ValueKind::Boolean,
        false,
        "ShapeProps",
    ),
];
const CURVE: &[Spec] = &[
    e("start", "start", ValueKind::Number, true, "CurveProps"),
    e("end", "end", ValueKind::Number, true, "CurveProps"),
    e(
        "start_arrow",
        "startArrow",
        ValueKind::Boolean,
        true,
        "CurveProps",
    ),
    e(
        "end_arrow",
        "endArrow",
        ValueKind::Boolean,
        true,
        "CurveProps",
    ),
    e(
        "arrow_size",
        "arrowSize",
        ValueKind::Number,
        true,
        "CurveProps",
    ),
    s("closed", "closed", ValueKind::Boolean, true, "CurveProps"),
    s(
        "start_offset",
        "startOffset",
        ValueKind::Number,
        true,
        "CurveProps",
    ),
    s(
        "end_offset",
        "endOffset",
        ValueKind::Number,
        true,
        "CurveProps",
    ),
];

fn layout_like(k: NodeKind) -> bool {
    !matches!(k, NodeKind::Group | NodeKind::Camera | NodeKind::Knot)
}
fn shape_like(k: NodeKind) -> bool {
    matches!(
        k,
        NodeKind::Rect
            | NodeKind::Circle
            | NodeKind::Line
            | NodeKind::Text
            | NodeKind::Code
            | NodeKind::Svg
            | NodeKind::Image
            | NodeKind::Video
            | NodeKind::Latex
            | NodeKind::Grid
            | NodeKind::Polygon
            | NodeKind::Path
            | NodeKind::CubicBezier
            | NodeKind::QuadBezier
            | NodeKind::Spline
            | NodeKind::Ray
    )
}
fn curve_like(k: NodeKind) -> bool {
    matches!(
        k,
        NodeKind::Rect
            | NodeKind::Circle
            | NodeKind::Line
            | NodeKind::Image
            | NodeKind::Video
            | NodeKind::Polygon
            | NodeKind::Path
            | NodeKind::CubicBezier
            | NodeKind::QuadBezier
            | NodeKind::Spline
            | NodeKind::Ray
    )
}
fn can_children(k: NodeKind) -> bool {
    !matches!(
        k,
        NodeKind::Text | NodeKind::Code | NodeKind::Svg | NodeKind::Latex | NodeKind::Knot
    )
}

pub fn node_class(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Group => "Node",
        NodeKind::Layout => "Layout",
        NodeKind::Rect => "Rect",
        NodeKind::Circle => "Circle",
        NodeKind::Line => "Line",
        NodeKind::Text => "Txt",
        NodeKind::Code => "Code",
        NodeKind::Svg => "SVG",
        NodeKind::Image => "Img",
        NodeKind::Video => "Video",
        NodeKind::Latex => "Latex",
        NodeKind::Camera => "Camera",
        NodeKind::Grid => "Grid",
        NodeKind::Polygon => "Polygon",
        NodeKind::Path => "Path",
        NodeKind::CubicBezier => "CubicBezier",
        NodeKind::QuadBezier => "QuadBezier",
        NodeKind::Spline => "Spline",
        NodeKind::Knot => "Knot",
        NodeKind::Ray => "Ray",
    }
}

fn specific(kind: NodeKind) -> Vec<Spec> {
    match kind {
        NodeKind::Rect => vec![
            e("radius", "radius", ValueKind::Number, true, "RectProps"),
            s(
                "smooth_corners",
                "smoothCorners",
                ValueKind::Boolean,
                true,
                "RectProps",
            ),
            s(
                "corner_sharpness",
                "cornerSharpness",
                ValueKind::Number,
                true,
                "RectProps",
            ),
        ],
        NodeKind::Circle => vec![
            s(
                "start_angle",
                "startAngle",
                ValueKind::Number,
                true,
                "CircleProps",
            ),
            s(
                "end_angle",
                "endAngle",
                ValueKind::Number,
                true,
                "CircleProps",
            ),
            s(
                "counterclockwise",
                "counterclockwise",
                ValueKind::Boolean,
                true,
                "CircleProps",
            ),
        ],
        NodeKind::Line => vec![
            e("points", "points", ValueKind::Vec2List, false, "LineProps"),
            e("edge", "points", ValueKind::EdgeRef, false, "LineProps"),
            e("radius", "radius", ValueKind::Number, true, "LineProps"),
        ],
        NodeKind::Text => vec![e("text", "text", ValueKind::Text, true, "TxtProps")],
        NodeKind::Code => vec![
            e("code", "code", ValueKind::Text, true, "CodeProps"),
            ee("language", "highlighter", LANGUAGE, false, "CodeProps"),
            e(
                "selection",
                "selection",
                ValueKind::CodeSelection,
                true,
                "CodeProps",
            ),
        ],
        NodeKind::Svg => vec![
            e("svg", "svg", ValueKind::Text, true, "SVGProps"),
            e("asset", "svg", ValueKind::AssetRef, false, "SVGProps"),
        ],
        NodeKind::Image => vec![
            e("asset", "src", ValueKind::AssetRef, false, "ImgProps"),
            s("alpha", "alpha", ValueKind::Number, true, "ImgProps"),
            s(
                "smoothing",
                "smoothing",
                ValueKind::Boolean,
                true,
                "ImgProps",
            ),
        ],
        NodeKind::Video => vec![
            e("asset", "src", ValueKind::AssetRef, false, "VideoProps"),
            e(
                "media_offset_ms",
                "time",
                ValueKind::Number,
                true,
                "VideoProps",
            ),
            e(
                "playback_rate",
                "playbackRate",
                ValueKind::Number,
                true,
                "VideoProps",
            ),
            e(
                "loop_media",
                "loop",
                ValueKind::Boolean,
                false,
                "VideoProps",
            ),
            s("alpha", "alpha", ValueKind::Number, true, "VideoProps"),
            s(
                "smoothing",
                "smoothing",
                ValueKind::Boolean,
                true,
                "VideoProps",
            ),
        ],
        NodeKind::Latex => vec![e("latex", "tex", ValueKind::Text, true, "LatexProps")],
        NodeKind::Camera => vec![e("zoom", "zoom", ValueKind::Number, true, "CameraProps")],
        NodeKind::Grid => vec![
            s("spacing", "spacing", ValueKind::Vec2, true, "GridProps"),
            e("start", "start", ValueKind::Number, true, "GridProps"),
            e("end", "end", ValueKind::Number, true, "GridProps"),
        ],
        NodeKind::Polygon => vec![
            s("sides", "sides", ValueKind::Integer, true, "PolygonProps"),
            e("radius", "radius", ValueKind::Number, true, "PolygonProps"),
        ],
        NodeKind::Path => vec![s("data", "data", ValueKind::PathData, true, "PathProps")],
        NodeKind::CubicBezier => vec![
            s("p0", "p0", ValueKind::Vec2, true, "CubicBezierProps"),
            s("p1", "p1", ValueKind::Vec2, true, "CubicBezierProps"),
            s("p2", "p2", ValueKind::Vec2, true, "CubicBezierProps"),
            s("p3", "p3", ValueKind::Vec2, true, "CubicBezierProps"),
        ],
        NodeKind::QuadBezier => vec![
            s("p0", "p0", ValueKind::Vec2, true, "QuadBezierProps"),
            s("p1", "p1", ValueKind::Vec2, true, "QuadBezierProps"),
            s("p2", "p2", ValueKind::Vec2, true, "QuadBezierProps"),
        ],
        NodeKind::Spline => vec![
            e(
                "points",
                "points",
                ValueKind::Vec2List,
                false,
                "SplineProps",
            ),
            s(
                "smoothness",
                "smoothness",
                ValueKind::Number,
                true,
                "SplineProps",
            ),
        ],
        NodeKind::Knot => vec![
            s(
                "start_handle",
                "startHandle",
                ValueKind::Vec2,
                true,
                "KnotProps",
            ),
            s(
                "end_handle",
                "endHandle",
                ValueKind::Vec2,
                true,
                "KnotProps",
            ),
            s(
                "start_handle_auto",
                "startHandleAuto",
                ValueKind::Number,
                true,
                "KnotProps",
            ),
            s(
                "end_handle_auto",
                "endHandleAuto",
                ValueKind::Number,
                true,
                "KnotProps",
            ),
        ],
        NodeKind::Ray => vec![
            s("from", "from", ValueKind::Vec2, true, "RayProps"),
            s("to", "to", ValueKind::Vec2, true, "RayProps"),
        ],
        _ => vec![],
    }
}

fn spec(kind: NodeKind, name: &str) -> Option<Spec> {
    for v in NODE {
        if v.name == name {
            return Some(*v);
        }
    }
    if layout_like(kind) {
        for v in LAYOUT {
            if v.name == name {
                return Some(*v);
            }
        }
    }
    if shape_like(kind) {
        for v in SHAPE {
            if v.name == name {
                return Some(*v);
            }
        }
    }
    if curve_like(kind) {
        for v in CURVE {
            if v.name == name {
                return Some(*v);
            }
        }
    }
    specific(kind).into_iter().find(|v| v.name == name)
}
fn desc(v: Spec) -> PropertyDescriptor {
    PropertyDescriptor {
        semantic_name: v.name.into(),
        upstream_name: v.upstream.into(),
        value_kind: v.kind,
        storage: v.storage,
        animatable: v.animatable
            && (v.storage == PropertyStorage::Explicit
                || matches!(
                    v.kind,
                    ValueKind::Number | ValueKind::Integer | ValueKind::Vec2 | ValueKind::Color
                )),
        enum_values: v.enums.iter().map(|x| (*x).into()).collect(),
        source_interface: v.source.into(),
    }
}
pub fn property(kind: NodeKind, name: &str) -> Option<PropertyDescriptor> {
    spec(kind, name).map(desc)
}
pub fn properties(kind: NodeKind) -> Vec<PropertyDescriptor> {
    let mut out = Vec::new();
    for group in [
        NODE,
        if layout_like(kind) { LAYOUT } else { &[] },
        if shape_like(kind) { SHAPE } else { &[] },
        if curve_like(kind) { CURVE } else { &[] },
    ] {
        for v in group {
            if !out
                .iter()
                .any(|x: &PropertyDescriptor| x.semantic_name == v.name)
            {
                out.push(desc(*v));
            }
        }
    }
    for v in specific(kind) {
        if !out.iter().any(|x| x.semantic_name == v.name) {
            out.push(desc(v));
        }
    }
    out
}

pub fn node_types() -> Vec<NodeTypeDescriptor> {
    use NodeKind::*;
    [
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
    ]
    .into_iter()
    .map(|kind| NodeTypeDescriptor {
        kind,
        upstream_class: node_class(kind).into(),
        can_have_children: can_children(kind),
        properties: properties(kind),
    })
    .collect()
}
pub fn can_have_children(kind: NodeKind) -> bool {
    can_children(kind)
}

fn number(v: f64, min: f64, max: f64) -> bool {
    v.is_finite() && v >= min && v <= max
}
fn color(v: &str, theme: &Theme) -> bool {
    if let Some(name) = v.strip_prefix('@') {
        theme
            .colors
            .get(name)
            .is_some_and(|x| security::literal_color(x))
    } else {
        security::literal_color(v)
    }
}
pub fn validate_value(
    kind: NodeKind,
    name: &str,
    value: &SemanticValue,
    theme: &Theme,
) -> Result<()> {
    let p = spec(kind, name)
        .ok_or_else(|| Error::invalid("Property is not supported for this node kind"))?;
    if p.storage != PropertyStorage::Semantic {
        return Err(Error::invalid(
            "Explicit property cannot be supplied through semantic property map",
        ));
    }
    let ok = match (p.kind, value) {
        (ValueKind::Boolean, SemanticValue::Bool(_)) => true,
        (ValueKind::Number, SemanticValue::Number(v)) => number(*v, -32768.0, 32768.0),
        (ValueKind::Integer, SemanticValue::Number(v)) => {
            number(*v, -32768.0, 32768.0) && v.fract() == 0.0
        }
        (ValueKind::Text, SemanticValue::Text(v)) => {
            v.len() <= MAX_TEXT && !v.chars().any(char::is_control)
        }
        (ValueKind::Color, SemanticValue::Text(v)) => color(v, theme),
        (ValueKind::Vec2, SemanticValue::Vec2(v)) => {
            v.iter().all(|x| number(*x, -32768.0, 32768.0))
        }
        (ValueKind::Spacing, SemanticValue::Spacing(v)) => {
            v.iter().all(|x| number(*x, -8192.0, 8192.0))
        }
        (ValueKind::NumberList, SemanticValue::NumberList(v)) => {
            v.len() <= 512 && v.iter().all(|x| number(*x, -32768.0, 32768.0))
        }
        (ValueKind::Enum, SemanticValue::Text(v)) => p.enums.contains(&v.as_str()),
        (ValueKind::PathData, SemanticValue::Text(v)) => {
            v.len() <= MAX_SVG
                && !v
                    .chars()
                    .any(|c| c.is_control() && !c.is_ascii_whitespace())
        }
        _ => false,
    };
    if ok {
        Ok(())
    } else {
        Err(Error::invalid(
            "Semantic property value has the wrong type or exceeds bounds",
        ))
    }
}
pub fn emitted_value(
    kind: NodeKind,
    name: &str,
    value: &SemanticValue,
    theme: &Theme,
) -> Result<serde_json::Value> {
    validate_value(kind, name, value, theme)?;
    let p = spec(kind, name).unwrap();
    if p.kind == ValueKind::Color
        && let SemanticValue::Text(v) = value
    {
        let resolved = if let Some(n) = v.strip_prefix('@') {
            theme.colors[n].clone()
        } else {
            v.clone()
        };
        return Ok(serde_json::Value::String(resolved));
    }
    Ok(serde_json::to_value(value)?)
}

#[cfg(test)]
mod coverage_tests {
    use super::*;
    use crate::model::{Easing, MOTION_CANVAS_VERSION, TransitionKind};
    use serde_json::Value;
    use std::collections::BTreeSet;

    #[test]
    fn api_coverage_managed_properties_exist_in_registry() {
        let coverage: Value = serde_json::from_str(include_str!(
            "../../../docs/motion-canvas/API_COVERAGE.json"
        ))
        .unwrap();
        assert_eq!(coverage["motion_canvas_version"], MOTION_CANVAS_VERSION);
        let components = coverage["components"].as_object().unwrap();
        for (name, component) in components {
            let witness = component["managed_kind"]
                .as_str()
                .or_else(|| component["witness_kind"].as_str());
            let Some(witness) = witness else { continue };
            let kind: NodeKind = serde_json::from_value(Value::String(witness.to_owned())).unwrap();
            for (upstream, property) in component["properties"].as_object().unwrap() {
                if property["status"] != "managed" {
                    continue;
                }
                let semantic_name = property["semantic"].as_str().unwrap();
                assert!(
                    super::property(kind, semantic_name).is_some(),
                    "coverage maps {name}.{upstream} to missing semantic property {semantic_name}"
                );
            }
        }
    }

    #[test]
    fn api_coverage_managed_and_represented_properties_resolve() {
        let coverage: Value = serde_json::from_str(include_str!(
            "../../../docs/motion-canvas/API_COVERAGE.json"
        ))
        .unwrap();
        for (component_name, component) in coverage["components"].as_object().unwrap() {
            let witness = component["managed_kind"]
                .as_str()
                .or_else(|| component["witness_kind"].as_str());
            let Some(witness) = witness else { continue };
            let kind: NodeKind = serde_json::from_value(Value::String(witness.to_owned())).unwrap();
            for (upstream_name, entry) in component["properties"].as_object().unwrap() {
                let status = entry["status"].as_str().unwrap();
                if !matches!(status, "managed" | "represented_by") {
                    continue;
                }
                let mapping = entry["semantic"].as_str().unwrap();
                for semantic_name in mapping.split('+') {
                    assert!(
                        super::property(kind, semantic_name).is_some(),
                        "coverage maps {component_name}.{upstream_name} to missing semantic property {semantic_name}"
                    );
                }
            }
        }
    }

    #[test]
    fn semantic_storage_properties_are_backed_by_upstream_coverage() {
        let coverage: Value = serde_json::from_str(include_str!(
            "../../../docs/motion-canvas/API_COVERAGE.json"
        ))
        .unwrap();
        let mapped = coverage["components"]
            .as_object()
            .unwrap()
            .values()
            .flat_map(|component| component["properties"].as_object().into_iter().flatten())
            .filter_map(|(_, entry)| {
                let status = entry["status"].as_str()?;
                matches!(status, "managed" | "represented_by").then(|| entry["semantic"].as_str())
            })
            .flatten()
            .flat_map(|mapping| mapping.split('+'))
            .collect::<BTreeSet<_>>();
        for ty in node_types() {
            for property in ty.properties {
                if property.storage == PropertyStorage::Semantic {
                    assert!(
                        mapped.contains(property.semantic_name.as_str()),
                        "semantic registry property {} on {:?} has no upstream coverage witness",
                        property.semantic_name,
                        ty.kind
                    );
                }
            }
        }
    }

    #[test]
    fn core_coverage_managed_easings_and_transitions_match_compiler() {
        let coverage: Value = serde_json::from_str(include_str!(
            "../../../docs/motion-canvas/CORE_API_COVERAGE.json"
        ))
        .unwrap();
        let tweening = coverage["authoring_exports"]["tweening"]
            .as_object()
            .unwrap();
        let represented_easings = Easing::ALL
            .into_iter()
            .map(crate::compiler::easing)
            .collect::<BTreeSet<_>>();
        for name in &represented_easings {
            assert_eq!(
                tweening[*name]["status"], "managed",
                "compiler easing {name} is not classified as managed"
            );
        }
        let managed_timing = tweening
            .iter()
            .filter(|(_, entry)| entry["status"] == "managed")
            .map(|(name, _)| name.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(managed_timing, represented_easings);

        let transitions = coverage["authoring_exports"]["transitions"]
            .as_object()
            .unwrap();
        for kind in TransitionKind::ALL {
            let name = crate::compiler::transition_function(kind);
            assert_eq!(
                transitions[name]["status"], "managed",
                "compiler transition {name} is not classified as managed"
            );
        }
    }

    #[test]
    fn managed_node_registry_is_unique_and_complete() {
        let types = node_types();
        assert_eq!(types.len(), 20);
        let kinds = types
            .iter()
            .map(|item| serde_json::to_string(&item.kind).unwrap())
            .collect::<BTreeSet<_>>();
        let classes = types
            .iter()
            .map(|item| item.upstream_class.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(kinds.len(), types.len());
        assert_eq!(classes.len(), types.len());
    }
}
