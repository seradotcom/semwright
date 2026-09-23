//! Whole-project validation precedes every write and render.
use crate::{Error, Result, model::*, security};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

fn ensure(condition: bool, message: &str) -> Result<()> {
    if condition { Ok(()) } else { Err(Error::invalid(message)) }
}
fn number(value: f64, min: f64, max: f64) -> bool { value.is_finite() && (min..=max).contains(&value) }
fn unique<'a>(ids: impl Iterator<Item = &'a str>, max: usize) -> Result<()> {
    let mut seen = BTreeSet::new();
    for id in ids {
        ensure(security::identifier(id) && seen.insert(id) && seen.len() <= max, "Invalid, duplicate or excessive IDs")?;
    }
    Ok(())
}
pub fn parse(bytes: &[u8]) -> Result<Project> {
    ensure(bytes.len() <= MAX_PROJECT_BYTES, "Project exceeds byte limit")?;
    let project: Project = serde_json::from_slice(bytes)?;
    project_valid(&project)?;
    Ok(project)
}
pub fn color(value: &str, theme: &Theme) -> Result<String> {
    let value = if let Some(key) = value.strip_prefix('@') {
        theme.colors.get(key).ok_or_else(|| Error::invalid("Unknown theme color"))?.as_str()
    } else { value };
    ensure(security::literal_color(value), "Color must be a literal color or theme token")?;
    Ok(value.into())
}
pub fn settings(settings: &Settings) -> Result<()> {
    ensure((16..=4096).contains(&settings.width) && (16..=4096).contains(&settings.height)
        && u64::from(settings.width) * u64::from(settings.height) <= 8_847_360, "Resolution exceeds bounds")?;
    ensure((1..=120).contains(&settings.fps), "Frame rate must be an integer from 1 to 120")?;
    if let Some(bg) = &settings.background { ensure(security::literal_color(bg), "Background must be a literal color")?; }
    Ok(())
}
pub fn theme(theme: &Theme) -> Result<()> {
    ensure(security::font(&theme.font_family) && security::font(&theme.mono_family), "Invalid font family")?;
    ensure(number(theme.font_size, 4.0, 512.0) && (100..=900).contains(&theme.font_weight)
        && number(theme.spacing, 0.0, 1024.0) && number(theme.line_width, 0.0, 64.0)
        && number(theme.radius, 0.0, 1024.0), "Theme numeric bounds exceeded")?;
    ensure(theme.colors.len() <= 32 && theme.colors.contains_key("ink") && theme.colors.contains_key("surface"), "Theme requires ink/surface and at most 32 colors")?;
    for (key, value) in &theme.colors {
        ensure(security::identifier(key) && security::literal_color(value), "Invalid theme color")?;
    }
    Ok(())
}
fn allows(kind: NodeKind, key: &str) -> bool {
    use NodeKind::*;
    match key {
        "position" | "scale" | "rotation" | "opacity" => true,
        "width" | "height" => !matches!(kind, Group | Camera),
        "fill" | "stroke" | "stroke_width" => matches!(kind, Rect | Circle | Line | Text | Code | Svg | Latex),
        "radius" => kind == Rect,
        "font_family" | "font_size" | "font_weight" | "line_height" | "letter_spacing" => matches!(kind, Text | Code),
        "text_align" | "wrap" | "text" => kind == Text,
        "code" | "language" | "selection" => kind == Code,
        "points" | "start" | "end" | "start_arrow" | "end_arrow" | "arrow_size" | "edge" => kind == Line,
        "dash" => matches!(kind, Rect | Circle | Line),
        "svg" => kind == Svg,
        "latex" => kind == Latex,
        "asset" => matches!(kind, Image | Video | Svg),
        "media_offset_ms" | "playback_rate" | "loop_media" => kind == Video,
        "layout" | "clip" => matches!(kind, Layout | Rect),
        "zoom" => kind == Camera,
        _ => false,
    }
}
pub fn properties(node: &Node, theme: &Theme) -> Result<()> {
    let p = &node.properties;
    let object = serde_json::to_value(p)?;
    for key in object.as_object().expect("properties serialize as object").keys() {
        ensure(allows(node.kind, key), "Property is not supported for this node kind")?;
    }
    for v in p.position.iter().flatten() { ensure(number(*v, -32768.0, 32768.0), "Position exceeds bounds")?; }
    for v in p.scale.iter().flatten() { ensure(number(*v, 0.001, 100.0), "Scale exceeds bounds")?; }
    for (v,min,max) in [
        (p.rotation,-36000.0,36000.0),(p.opacity,0.0,1.0),(p.width,0.0,16384.0),
        (p.height,0.0,16384.0),(p.stroke_width,0.0,64.0),(p.radius,0.0,8192.0),
        (p.font_size,4.0,512.0),(p.line_height,0.1,10.0),(p.letter_spacing,-100.0,200.0),
        (p.start,0.0,1.0),(p.end,0.0,1.0),(p.arrow_size,0.0,256.0),
        (p.playback_rate,0.1,8.0),(p.zoom,0.01,100.0),
    ] { if let Some(v) = v { ensure(number(v,min,max), "Property numeric bounds exceeded")?; } }
    if let Some(v) = p.font_weight { ensure((100..=900).contains(&v), "Font weight exceeds bounds")?; }
    if let Some(v) = &p.font_family { ensure(security::font(v), "Invalid font family")?; }
    for v in [&p.fill,&p.stroke].into_iter().flatten() { color(v,theme)?; }
    if let Some(v) = &p.text { ensure(v.len() <= MAX_TEXT, "Text exceeds bounds")?; }
    if let Some(v) = &p.code { ensure(v.len() <= MAX_CODE, "Code text exceeds bounds")?; }
    if let Some(v) = &p.svg { security::validate_svg(v)?; }
    if let Some(v) = &p.latex { security::validate_latex(v)?; }
    if let Some(v) = p.media_offset_ms { ensure(v <= MAX_PROJECT_MS, "Media offset exceeds bounds")?; }
    if let Some(points) = &p.points {
        ensure((2..=512).contains(&points.len()) && points.iter().flatten().all(|v| number(*v,-32768.0,32768.0)), "Invalid line points")?;
    }
    if let Some(dash) = &p.dash { ensure(dash.len() <= 16 && dash.iter().all(|v| number(*v,0.0,1024.0)), "Invalid line dash")?; }
    if let Some(l) = &p.layout {
        ensure(number(l.gap,0.0,4096.0) && number(l.grow,0.0,100.0)
            && l.padding.iter().all(|v| number(*v,0.0,4096.0))
            && l.basis.is_none_or(|v| number(v,0.0,16384.0)), "Layout exceeds bounds")?;
    }
    if let Some(selection) = &p.selection {
        let lines = p.code.as_deref().unwrap_or("").split('\n').collect::<Vec<_>>();
        match selection {
            CodeSelection::Lines {start,end} => ensure(start <= end && (*end as usize) < lines.len(), "Code line selection is out of range")?,
            CodeSelection::Word {line,start,length} => {
                let Some(text) = lines.get(*line as usize) else { return Err(Error::invalid("Code line selection is out of range")); };
                ensure(u64::from(*start) + u64::from(*length) <= text.chars().count() as u64, "Code word selection is out of range")?;
            }
        }
    }
    Ok(())
}

pub fn animation_times(scene: &Scene, animation: &Animation) -> Result<(u64,u64)> {
    let base = if let Some(id) = &animation.at.cue {
        scene.cues.iter().find(|c| &c.id == id).ok_or_else(|| Error::invalid("Unknown animation cue"))?.time_ms
    } else { 0 };
    let start = i128::from(base) + i128::from(animation.at.offset_ms);
    ensure(start >= 0 && start <= i128::from(MAX_SCENE_MS), "Animation start exceeds bounds")?;
    let duration = if let Some(id) = &animation.duration_cue {
        scene.cues.iter().find(|c| &c.id == id).ok_or_else(|| Error::invalid("Unknown duration cue"))?.duration_ms
    } else { animation.duration_ms };
    let end = u64::try_from(start).expect("bounded start").checked_add(duration).ok_or_else(|| Error::invalid("Animation time overflow"))?;
    ensure(end <= scene.duration_ms, "Animation ends after the scene")?;
    Ok((start as u64,end))
}
fn animated_value(property: AnimatedProperty, value: &AnimatedValue, theme: &Theme) -> Result<()> {
    use AnimatedProperty::*;
    match (property,value) {
        (Position,AnimatedValue::Vector(v)) => ensure(v.iter().all(|v| number(*v,-32768.0,32768.0)), "Animated position exceeds bounds"),
        (Scale,AnimatedValue::Vector(v)) => ensure(v.iter().all(|v| number(*v,0.001,100.0)), "Animated scale exceeds bounds"),
        (Fill | Stroke,AnimatedValue::Text(s)) => color(s,theme).map(|_| ()),
        (Text,AnimatedValue::Text(s)) => ensure(s.len() <= MAX_TEXT, "Animated text exceeds bounds"),
        (Code,AnimatedValue::Text(s)) => ensure(s.len() <= MAX_CODE, "Animated code exceeds bounds"),
        (Opacity | LineStart | LineEnd,AnimatedValue::Number(v)) => ensure(number(*v,0.0,1.0), "Animated progress exceeds bounds"),
        (Width | Height | Radius,AnimatedValue::Number(v)) => ensure(number(*v,0.0,16384.0), "Animated size exceeds bounds"),
        (FontSize,AnimatedValue::Number(v)) => ensure(number(*v,4.0,512.0), "Animated font size exceeds bounds"),
        (LetterSpacing,AnimatedValue::Number(v)) => ensure(number(*v,-100.0,200.0), "Animated spacing exceeds bounds"),
        (CameraZoom,AnimatedValue::Number(v)) => ensure(number(*v,0.01,100.0), "Animated camera zoom exceeds bounds"),
        (X | Y,AnimatedValue::Number(v)) => ensure(number(*v,-32768.0,32768.0), "Animated position exceeds bounds"),
        (Rotation,AnimatedValue::Number(v)) => ensure(number(*v,-36000.0,36000.0), "Animated rotation exceeds bounds"),
        (CameraFocus,AnimatedValue::Text(s)) => ensure(security::identifier(s), "Invalid camera focus target"),
        (Counter,AnimatedValue::Number(v)) => ensure(number(*v,-1e9,1e9), "Counter exceeds bounds"),
        _ => Err(Error::invalid("Animation value has the wrong type")),
    }
}
fn property_key(property: AnimatedProperty) -> &'static str {
    use AnimatedProperty::*;
    match property { Position | X | Y | CameraFocus => "position", Scale => "scale", Rotation => "rotation", Opacity => "opacity",
        Fill => "fill", Stroke => "stroke", Width => "width", Height => "height", Radius => "radius", Text | Counter => "text",
        Code => "code", LineStart => "start", LineEnd => "end", FontSize => "font_size", LetterSpacing => "letter_spacing", CameraZoom => "zoom" }
}
fn conflicting(a: AnimatedProperty,b: AnimatedProperty) -> bool {
    use AnimatedProperty::*;
    property_key(a) == property_key(b) && !matches!((a,b),(X,Y)|(Y,X))
}
pub fn animations(scene: &Scene, theme: &Theme) -> Result<()> {
    let mut intervals: Vec<(&Animation,u64,u64)> = Vec::new();
    for animation in &scene.animations {
        let node = scene.nodes.iter().find(|n| n.id == animation.target).ok_or_else(|| Error::invalid("Unknown animation target"))?;
        ensure(allows(node.kind,property_key(animation.property)), "Animation property unsupported by target")?;
        if animation.property == AnimatedProperty::CameraFocus {
            ensure(node.kind == NodeKind::Camera && animation.from.is_none(), "Camera focus requires a camera and has no from value")?;
            if let AnimatedValue::Text(id) = &animation.to {
                ensure(scene.nodes.iter().any(|n| &n.id == id), "Unknown camera focus target")?;
            }
        }
        animated_value(animation.property,&animation.to,theme)?;
        if let Some(v) = &animation.from { animated_value(animation.property,v,theme)?; }
        let (start,end) = animation_times(scene,animation)?;
        for (other,os,oe) in &intervals {
            ensure(!(other.target == animation.target && conflicting(other.property,animation.property)
                && ((start < *oe && *os < end) || (start == end && start == *os && *os == *oe))), "Overlapping animations drive the same property")?;
        }
        intervals.push((animation,start,end));
    }
    Ok(())
}

pub fn project_valid(project: &Project) -> Result<()> {
    ensure(project.schema_version == SCHEMA_VERSION && project.component_version == COMPONENT_VERSION, "Unsupported managed project version")?;
    ensure(security::identifier(&project.id) && project.generation.len() == 32
        && project.generation.bytes().all(|b| b.is_ascii_hexdigit()) && project.revision > 0, "Invalid project identity")?;
    ensure(serde_json::to_vec(project)?.len() <= MAX_PROJECT_BYTES, "Project exceeds byte limit")?;
    settings(&project.settings)?; theme(&project.theme)?;
    unique(project.scenes.iter().map(|s| s.id.as_str()),MAX_SCENES)?;
    unique(project.scenes.iter().flat_map(|s| s.nodes.iter().map(|n| n.id.as_str())),MAX_NODES)?;
    unique(project.scenes.iter().flat_map(|s| s.animations.iter().map(|a| a.id.as_str())),MAX_ANIMATIONS)?;
    unique(project.scenes.iter().flat_map(|s| s.cues.iter().map(|c| c.id.as_str())),MAX_CUES)?;
    unique(project.assets.iter().map(|a| a.id.as_str()),MAX_ASSETS)?;
    unique(project.audio.iter().map(|a| a.id.as_str()),16)?;
    ensure(project.scenes.iter().all(|s| (1..=MAX_SCENE_MS).contains(&s.duration_ms)) && project.duration_ms() <= MAX_PROJECT_MS, "Project duration exceeds bounds")?;
    for asset in &project.assets {
        security::relative_path(&asset.path)?;
        ensure(asset.path.starts_with("assets/") && security::digest(&asset.sha256)
            && asset.bytes > 0 && asset.bytes <= MAX_ASSET_BYTES as u64, "Invalid asset metadata")?;
        ensure(asset.provenance.as_ref().is_none_or(|s| s.len() <= 2048) && asset.license.as_ref().is_none_or(|s| s.len() <= 512), "Asset provenance exceeds bounds")?;
        if let Some([w,h]) = asset.dimensions { ensure(w > 0 && h > 0 && w <= 4096 && h <= 4096 && u64::from(w)*u64::from(h) <= 8_847_360, "Asset dimensions exceed bounds")?; }
        if let Some(ms) = asset.duration_ms { ensure(ms <= MAX_PROJECT_MS, "Asset duration exceeds bounds")?; }
    }
    for audio in &project.audio {
        ensure(project.assets.iter().any(|a| a.id == audio.asset && a.kind == AssetKind::Audio)
            && number(audio.volume,0.0,1.0) && audio.offset_ms.unsigned_abs() <= MAX_PROJECT_MS, "Invalid audio track")?;
    }
    for scene in &project.scenes {
        ensure(scene.name.len() <= 256, "Scene name exceeds bounds")?;
        if let Some(t) = &scene.transition { ensure(t.duration_ms <= scene.duration_ms && t.duration_ms <= 5000, "Transition duration exceeds bounds")?; }
        let map = scene.nodes.iter().map(|n| (n.id.as_str(),n)).collect::<BTreeMap<_,_>>();
        for node in &scene.nodes {
            ensure(node.name.len() <= 256, "Node name exceeds bounds")?;
            properties(node,&project.theme)?;
            let mut current = node;
            let mut seen = BTreeSet::new();
            while let Some(parent) = &current.parent {
                ensure(seen.insert(parent.as_str()) && seen.len() <= 64, "Node hierarchy contains a cycle or exceeds depth")?;
                current = map.get(parent.as_str()).ok_or_else(|| Error::invalid("Unknown node parent"))?;
                ensure(matches!(current.kind,NodeKind::Group|NodeKind::Layout|NodeKind::Rect|NodeKind::Circle|NodeKind::Camera), "Node kind cannot contain children")?;
            }
            if let Some(edge) = &node.properties.edge {
                ensure(node.properties.points.is_none(), "A diagram edge cannot also define fixed points")?;
                let from = map.get(edge.from.as_str()).ok_or_else(|| Error::invalid("Unknown edge start"))?;
                let to = map.get(edge.to.as_str()).ok_or_else(|| Error::invalid("Unknown edge end"))?;
                ensure(from.parent == node.parent && to.parent == node.parent, "Diagram endpoints must share the edge's parent coordinate space")?;
            }
            if let Some(id) = &node.properties.asset {
                let asset = project.assets.iter().find(|a| &a.id == id).ok_or_else(|| Error::invalid("Unknown asset reference"))?;
                let expected = match node.kind { NodeKind::Image => AssetKind::Image, NodeKind::Video => AssetKind::Video, NodeKind::Svg => AssetKind::Svg, _ => return Err(Error::invalid("Node does not support assets")) };
                ensure(asset.kind == expected, "Asset kind does not match node")?;
            }
            if matches!(node.kind,NodeKind::Image|NodeKind::Video) { ensure(node.properties.asset.is_some(), "Media node requires an asset")?; }
            if node.kind == NodeKind::Svg { ensure(node.properties.svg.is_some() ^ node.properties.asset.is_some(), "SVG requires exactly one inline or local source")?; }
        }
        let mut names = BTreeSet::new();
        for cue in &scene.cues {
            ensure(!cue.name.is_empty() && cue.name.len() <= 128 && names.insert(&cue.name)
                && cue.time_ms <= scene.duration_ms && cue.duration_ms <= scene.duration_ms - cue.time_ms, "Invalid, duplicate or out-of-range cue")?;
        }
        animations(scene,&project.theme)?;
    }
    Ok(())
}

/// Round half up using integer arithmetic; no cumulative float drift.
pub fn ms_to_frames(ms: u64,fps: u32) -> Result<u64> {
    ensure((1..=120).contains(&fps), "Invalid integer FPS")?;
    u64::try_from((u128::from(ms)*u128::from(fps)+500)/1000).map_err(|_| Error::invalid("Frame conversion overflow"))
}
pub fn seconds(ms: u64) -> String { format!("{}.{:03}",ms/1000,ms%1000) }
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RenderPlan {
    pub renderer: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub first_frame: u64,
    pub end_frame_exclusive: u64,
    pub frame_count: u64,
    pub project_duration_ms: u64,
    pub alpha: bool,
    pub color_space: ColorSpace,
    pub timeout_ms: u64,
}
pub fn render_plan(project: &Project,profile: &RenderProfile) -> Result<RenderPlan> {
    project_valid(project)?;
    let (n,d) = profile.scale.ratio();
    let width = project.settings.width*n/d;
    let height = project.settings.height*n/d;
    let duration = ms_to_frames(project.duration_ms(),project.settings.fps)?;
    ensure(project.settings.width*n % d == 0 && project.settings.height*n % d == 0
        && width > 0 && height > 0 && width <= 4096 && height <= 4096
        && u64::from(width)*u64::from(height) <= 8_847_360, "Scaled render resolution exceeds bounds")?;
    ensure(profile.first_frame < profile.end_frame_exclusive && profile.end_frame_exclusive <= duration
        && profile.end_frame_exclusive-profile.first_frame <= MAX_FRAMES, "Invalid render frame range")?;
    ensure((1000..=300_000).contains(&profile.timeout_ms), "Render timeout exceeds bounds")?;
    Ok(RenderPlan { renderer: "bundled_browser_v1".into(), width,height,fps: project.settings.fps,
        first_frame: profile.first_frame,end_frame_exclusive:profile.end_frame_exclusive,
        frame_count:profile.end_frame_exclusive-profile.first_frame,project_duration_ms:project.duration_ms(),
        alpha:profile.transparent || project.settings.background.is_none(),color_space:project.settings.color_space,timeout_ms:profile.timeout_ms })
}
