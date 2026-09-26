//! Deterministic TSX generation from validated semantic data.
use crate::{Error, Result, model::*, security, semantic, validate};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub const COMPILER_VERSION: u32 = 2;
#[derive(Debug, Clone)]
pub struct Generated {
    pub files: BTreeMap<String, Vec<u8>>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GeneratedFile {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
}
impl Generated {
    pub fn inventory(&self) -> Vec<GeneratedFile> {
        self.files
            .iter()
            .map(|(path, bytes)| GeneratedFile {
                path: path.clone(),
                sha256: security::sha256(bytes),
                bytes: bytes.len() as u64,
            })
            .collect()
    }
    pub fn fingerprint(&self) -> Result<String> {
        Ok(security::sha256(&serde_json::to_vec(&self.inventory())?))
    }
}
fn class(kind: NodeKind) -> &'static str {
    semantic::node_class(kind)
}

fn expression(value: &Value) -> Result<String> {
    if let Value::String(s) = value {
        Ok(security::js_string(s))
    } else {
        Ok(serde_json::to_string(value)?)
    }
}
fn attr(out: &mut String, name: &str, value: Value) -> Result<()> {
    out.push_str(&format!(" {name}={{{}}}", expression(&value)?));
    Ok(())
}
pub(crate) fn easing(value: Easing) -> &'static str {
    match value {
        Easing::Linear => "linear",
        Easing::Sin => "sin",
        Easing::Cos => "cos",
        Easing::EaseInSine => "easeInSine",
        Easing::EaseOutSine => "easeOutSine",
        Easing::EaseInOutSine => "easeInOutSine",
        Easing::EaseInQuad => "easeInQuad",
        Easing::EaseOutQuad => "easeOutQuad",
        Easing::EaseInOutQuad => "easeInOutQuad",
        Easing::EaseInCubic => "easeInCubic",
        Easing::EaseOutCubic => "easeOutCubic",
        Easing::EaseInOutCubic => "easeInOutCubic",
        Easing::EaseInQuart => "easeInQuart",
        Easing::EaseOutQuart => "easeOutQuart",
        Easing::EaseInOutQuart => "easeInOutQuart",
        Easing::EaseInQuint => "easeInQuint",
        Easing::EaseOutQuint => "easeOutQuint",
        Easing::EaseInOutQuint => "easeInOutQuint",
        Easing::EaseInExpo => "easeInExpo",
        Easing::EaseOutExpo => "easeOutExpo",
        Easing::EaseInOutExpo => "easeInOutExpo",
        Easing::EaseInCirc => "easeInCirc",
        Easing::EaseOutCirc => "easeOutCirc",
        Easing::EaseInOutCirc => "easeInOutCirc",
        Easing::EaseInBack => "easeInBack",
        Easing::EaseOutBack => "easeOutBack",
        Easing::EaseInOutBack => "easeInOutBack",
        Easing::EaseInBounce => "easeInBounce",
        Easing::EaseOutBounce => "easeOutBounce",
        Easing::EaseInOutBounce => "easeInOutBounce",
        Easing::EaseInElastic => "easeInElastic",
        Easing::EaseOutElastic => "easeOutElastic",
        Easing::EaseInOutElastic => "easeInOutElastic",
    }
}

pub(crate) fn transition_function(value: TransitionKind) -> &'static str {
    match value {
        TransitionKind::Fade => "fadeTransition",
        TransitionKind::SlideLeft
        | TransitionKind::SlideRight
        | TransitionKind::SlideUp
        | TransitionKind::SlideDown => "slideTransition",
        TransitionKind::ZoomIn => "zoomInTransition",
        TransitionKind::ZoomOut => "zoomOutTransition",
    }
}
fn transition_expression(value: TransitionKind, duration: &str) -> String {
    let function = transition_function(value);
    match value {
        TransitionKind::SlideLeft => format!("{function}(Direction.Left,{duration})"),
        TransitionKind::SlideRight => format!("{function}(Direction.Right,{duration})"),
        TransitionKind::SlideUp => format!("{function}(Direction.Top,{duration})"),
        TransitionKind::SlideDown => format!("{function}(Direction.Bottom,{duration})"),
        _ => format!("{function}({duration})"),
    }
}

fn signal(kind: NodeKind, property: &AnimatedProperty) -> Result<String> {
    use AnimatedProperty::*;
    let value = match property {
        Position => "position",
        X => "x",
        Y => "y",
        Scale => "scale",
        Rotation => "rotation",
        Opacity => "opacity",
        Fill => "fill",
        Stroke => "stroke",
        Width => "width",
        Height => "height",
        Radius => "radius",
        Text | Counter => "text",
        Code => "code",
        LineStart => "start",
        LineEnd => "end",
        FontSize => "fontSize",
        LetterSpacing => "letterSpacing",
        CameraZoom => "zoom",
        CameraFocus => "centerOn",
        Semantic(name) => {
            let descriptor = semantic::property(kind, name)
                .ok_or_else(|| Error::invalid("Unknown semantic animation property"))?;
            if !descriptor.animatable {
                return Err(Error::invalid("Semantic property is not safely animatable"));
            }
            return Ok(descriptor.upstream_name);
        }
    };
    Ok(value.into())
}

fn filter_name(value: FilterKind) -> &'static str {
    match value {
        FilterKind::Invert => "invert",
        FilterKind::Sepia => "sepia",
        FilterKind::Grayscale => "grayscale",
        FilterKind::Brightness => "brightness",
        FilterKind::Contrast => "contrast",
        FilterKind::Saturate => "saturate",
        FilterKind::Hue => "hue",
        FilterKind::Blur => "blur",
    }
}

fn language(value: Language) -> &'static str {
    match value {
        Language::Javascript | Language::Typescript => "jsParser",
        Language::Python => "pythonParser",
        Language::Rust => "rustParser",
        Language::Plain => "",
    }
}
fn animated(
    kind: NodeKind,
    value: &AnimatedValue,
    property: &AnimatedProperty,
    theme: &Theme,
) -> Result<String> {
    if matches!(property, AnimatedProperty::Fill | AnimatedProperty::Stroke) {
        let AnimatedValue::Text(s) = value else {
            return Err(Error::invalid("Expected animated color"));
        };
        return Ok(security::js_string(&validate::color(s, theme)?));
    }
    if let AnimatedProperty::Semantic(name) = property {
        let semantic_value = match value {
            AnimatedValue::Number(v) => SemanticValue::Number(*v),
            AnimatedValue::Vector(v) => SemanticValue::Vec2(*v),
            AnimatedValue::Text(v) => SemanticValue::Text(v.clone()),
        };
        return expression(&semantic::emitted_value(
            kind,
            name,
            &semantic_value,
            theme,
        )?);
    }
    expression(&serde_json::to_value(value)?)
}

fn node_source(
    scene: &Scene,
    node: &Node,
    project: &Project,
    indices: &BTreeMap<&str, usize>,
    assets: &BTreeMap<&str, usize>,
    indent: usize,
) -> Result<String> {
    let p = &node.properties;
    let index = indices[node.id.as_str()];
    let mut out = format!(
        "{}<{} ref={{n{index}}}",
        " ".repeat(indent),
        class(node.kind)
    );
    let values = serde_json::to_value(p)?;
    for (key, value) in values.as_object().expect("property object") {
        let mapped = match key.as_str() {
            "position" => "position",
            "scale" => "scale",
            "rotation" => "rotation",
            "opacity" => "opacity",
            "width" => "width",
            "height" => "height",
            "stroke_width" => "lineWidth",
            "radius" => "radius",
            "font_family" => "fontFamily",
            "font_size" => "fontSize",
            "font_weight" => "fontWeight",
            "letter_spacing" => "letterSpacing",
            "text_align" => "textAlign",
            "wrap" => "textWrap",
            "text" => "text",
            "code" => "code",
            "points" => "points",
            "start" => "start",
            "end" => "end",
            "start_arrow" => "startArrow",
            "end_arrow" => "endArrow",
            "arrow_size" => "arrowSize",
            "dash" => "lineDash",
            "svg" => "svg",
            "latex" => "tex",
            "loop_media" => "loop",
            "playback_rate" => "playbackRate",
            "clip" => "clip",
            "zoom" => "zoom",
            _ => continue,
        };
        attr(&mut out, mapped, value.clone())?;
    }
    for (key, value) in &p.semantic {
        let descriptor = semantic::property(node.kind, key)
            .ok_or_else(|| Error::invalid("Unknown semantic property"))?;
        let value = semantic::emitted_value(node.kind, key, value, &project.theme)?;
        attr(&mut out, &descriptor.upstream_name, value)?;
    }
    if !p.filters.is_empty() {
        let rendered = p
            .filters
            .iter()
            .map(|filter| format!("{}({})", filter_name(filter.kind), filter.value))
            .collect::<Vec<_>>()
            .join(",");
        out.push_str(&format!(" filters={{[{}]}}", rendered));
    }
    for (key, value) in [("fill", &p.fill), ("stroke", &p.stroke)] {
        if let Some(value) = value {
            attr(
                &mut out,
                key,
                json!(validate::color(value, &project.theme)?),
            )?;
        }
    }
    if matches!(node.kind, NodeKind::Text | NodeKind::Code) {
        if p.font_family.is_none() {
            attr(
                &mut out,
                "fontFamily",
                json!(if node.kind == NodeKind::Code {
                    &project.theme.mono_family
                } else {
                    &project.theme.font_family
                }),
            )?;
        }
        if p.font_size.is_none() {
            attr(&mut out, "fontSize", json!(project.theme.font_size))?;
        }
        if p.font_weight.is_none() {
            attr(&mut out, "fontWeight", json!(project.theme.font_weight))?;
        }
        if p.fill.is_none() {
            attr(&mut out, "fill", json!(project.theme.colors["ink"]))?;
        }
        if let Some(v) = p.line_height {
            attr(
                &mut out,
                "lineHeight",
                json!(v * p.font_size.unwrap_or(project.theme.font_size)),
            )?;
        }
    }
    if let Some(selection) = &p.selection {
        let selection = match selection {
            CodeSelection::Lines { start, end } => format!("lines({start},{end})"),
            CodeSelection::Word {
                line,
                start,
                length,
            } => format!("word({line},{start},{length})"),
        };
        out.push_str(&format!(" selection={{{selection}}}"));
    }
    if let Some(lang) = p.language.filter(|l| *l != Language::Plain) {
        out.push_str(&format!(
            " highlighter={{new LezerHighlighter({})}}",
            language(lang)
        ));
    }
    if let Some(l) = &p.layout {
        attr(&mut out, "layout", json!(true))?;
        attr(
            &mut out,
            "direction",
            json!(match l.direction {
                LayoutDirection::Row => "row",
                LayoutDirection::Column => "column",
            }),
        )?;
        attr(&mut out, "gap", json!(l.gap))?;
        attr(&mut out, "padding", json!(l.padding))?;
        attr(
            &mut out,
            "alignItems",
            json!(match l.align {
                Align::Start => "start",
                Align::Center => "center",
                Align::End => "end",
                Align::Stretch => "stretch",
            }),
        )?;
        attr(
            &mut out,
            "justifyContent",
            json!(match l.justify {
                Justify::Start => "start",
                Justify::Center => "center",
                Justify::End => "end",
                Justify::SpaceBetween => "space-between",
                Justify::SpaceAround => "space-around",
            }),
        )?;
        attr(&mut out, "grow", json!(l.grow))?;
        if let Some(v) = l.basis {
            attr(&mut out, "basis", json!(v))?;
        }
    }
    if let Some(edge) = &p.edge {
        let a = indices[edge.from.as_str()];
        let b = indices[edge.to.as_str()];
        out.push_str(&format!(
            " points={{() => [n{a}().position(), n{b}().position()]}}"
        ));
    }
    if let Some(id) = &p.asset {
        let asset = assets[id.as_str()];
        out.push_str(&format!(
            " {}={{a{asset}}}",
            if node.kind == NodeKind::Svg {
                "svg"
            } else {
                "src"
            }
        ));
    }
    if node.kind == NodeKind::Video {
        out.push_str(" play={true}");
        if let Some(offset) = p.media_offset_ms {
            out.push_str(&format!(" time={{{}}}", validate::seconds(offset)));
        }
    }
    let children = scene
        .nodes
        .iter()
        .filter(|n| n.parent.as_deref() == Some(node.id.as_str()))
        .collect::<Vec<_>>();
    if children.is_empty() {
        out.push_str(" />\n");
    } else {
        out.push_str(">\n");
        for child in children {
            out.push_str(&node_source(
                scene,
                child,
                project,
                indices,
                assets,
                indent + 2,
            )?);
        }
        out.push_str(&format!("{}</{}>\n", " ".repeat(indent), class(node.kind)));
    }
    Ok(out)
}
fn scene_source(scene: &Scene, project: &Project) -> Result<String> {
    let indices = scene
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.as_str(), i))
        .collect::<BTreeMap<_, _>>();
    let assets = project
        .assets
        .iter()
        .enumerate()
        .map(|(i, a)| (a.id.as_str(), i))
        .collect::<BTreeMap<_, _>>();
    let mut out =
        String::from("// Generated by Semwright motion compiler v2. Edit semwright-motion.json.\n");
    out.push_str("import {makeScene2D,Node,Layout,Rect,Circle,Line,Txt,Code,SVG,Img,Video,Latex,Camera,Grid,Polygon,Path,CubicBezier,QuadBezier,Spline,Knot,Ray,invert,sepia,grayscale,brightness,contrast,saturate,hue,blur,LezerHighlighter,lines,word} from '@motion-canvas/2d';\n");
    out.push_str("import {all,delay,waitFor,createRef,linear,sin,cos,easeInSine,easeOutSine,easeInOutSine,easeInQuad,easeOutQuad,easeInOutQuad,easeInCubic,easeOutCubic,easeInOutCubic,easeInQuart,easeOutQuart,easeInOutQuart,easeInQuint,easeOutQuint,easeInOutQuint,easeInExpo,easeOutExpo,easeInOutExpo,easeInCirc,easeOutCirc,easeInOutCirc,easeInBack,easeOutBack,easeInOutBack,easeInBounce,easeOutBounce,easeInOutBounce,easeInElastic,easeOutElastic,easeInOutElastic,tween,fadeTransition,slideTransition,zoomInTransition,zoomOutTransition,Direction} from '@motion-canvas/core';\n");
    out.push_str("import {parser as jsParser} from '@lezer/javascript';\nimport {parser as pythonParser} from '@lezer/python';\nimport {parser as rustParser} from '@lezer/rust';\n");
    for (index, asset) in project.assets.iter().enumerate() {
        let query = if asset.kind == AssetKind::Svg {
            "raw"
        } else {
            "url"
        };
        out.push_str(&format!(
            "import a{index} from {};\n",
            security::js_string(&format!("../../{}?{query}", asset.path))
        ));
    }
    out.push_str("export default makeScene2D(function* (view) {\n");
    for (index, node) in scene.nodes.iter().enumerate() {
        out.push_str(&format!(
            "  const n{index} = createRef<{}>();\n",
            class(node.kind)
        ));
    }
    out.push_str("  view.add(<>\n");
    for node in scene.nodes.iter().filter(|n| n.parent.is_none()) {
        out.push_str(&node_source(scene, node, project, &indices, &assets, 4)?);
    }
    out.push_str("  </>);\n  yield* all(\n");
    out.push_str(&format!(
        "    waitFor({}),\n",
        validate::seconds(scene.duration_ms)
    ));
    if let Some(t) = &scene.transition {
        let duration = validate::seconds(t.duration_ms);
        let expr = transition_expression(t.kind, &duration);
        out.push_str(&format!("    {expr},\n"));
    }
    for animation in &scene.animations {
        let (start, end) = validate::animation_times(scene, animation)?;
        let target = indices[animation.target.as_str()];
        let target_kind = scene
            .nodes
            .iter()
            .find(|node| node.id == animation.target)
            .expect("validated target")
            .kind;
        let property = signal(target_kind, &animation.property)?;
        let duration = validate::seconds(end - start);
        let timing = easing(animation.easing);
        out.push_str(&format!(
            "    delay({}, (function* () {{\n",
            validate::seconds(start)
        ));
        match &animation.property {
            AnimatedProperty::CameraFocus => {
                let AnimatedValue::Text(id) = &animation.to else {
                    return Err(Error::invalid("Camera focus target must be a node id"));
                };
                out.push_str(&format!(
                    "      yield* n{target}().centerOn(n{}(),{duration},{timing});\n",
                    indices[id.as_str()]
                ));
            }
            AnimatedProperty::Counter => {
                let default_from = AnimatedValue::Number(0.0);
                let from = animation.from.as_ref().unwrap_or(&default_from);
                out.push_str(&format!("      yield* tween({duration}, v => n{target}().text(Math.round(({}) + (({}) - ({})) * {timing}(v)).toString()));\n", animated(target_kind, from, &animation.property, &project.theme)?, animated(target_kind, &animation.to, &animation.property, &project.theme)?, animated(target_kind, from, &animation.property, &project.theme)?));
            }
            _ => {
                if let Some(from) = &animation.from {
                    out.push_str(&format!(
                        "      n{target}().{property}({});\n",
                        animated(target_kind, from, &animation.property, &project.theme)?
                    ));
                }
                out.push_str(&format!(
                    "      yield* n{target}().{property}({},{duration},{timing});\n",
                    animated(
                        target_kind,
                        &animation.to,
                        &animation.property,
                        &project.theme
                    )?
                ));
            }
        }
        out.push_str("    })()),\n");
    }
    out.push_str("  );\n});\n");
    Ok(out)
}
pub fn compile(project: &Project) -> Result<Generated> {
    validate::project_valid(project)?;
    crate::audio_codegen::validate(project)?;
    let mut files = BTreeMap::new();
    let mut source = String::from(
        "// Generated by Semwright motion compiler v2.\nimport {makeProject} from '@motion-canvas/core';\nimport {semwrightExporterPlugin} from './semwright-exporter';\nimport '@fontsource-variable/instrument-sans';\nimport '@fontsource/ibm-plex-mono/400.css';\n",
    );
    for (i, scene) in project.scenes.iter().enumerate() {
        source.push_str(&format!(
            "import s{i} from {};\n",
            security::js_string(&format!("./scenes/{}?scene", scene.id))
        ));
        files.insert(
            format!("src/scenes/{}.tsx", scene.id),
            scene_source(scene, project)?.into_bytes(),
        );
        files.insert(format!("src/scenes/{}.meta", scene.id), b"{}\n".to_vec());
    }
    if let Some(audio) = project.audio.first() {
        let asset = project
            .assets
            .iter()
            .find(|a| a.id == audio.asset)
            .ok_or_else(|| Error::invalid("Missing audio asset"))?;
        source.push_str(&format!(
            "import audio from {};\n",
            security::js_string(&format!("../{}?url", asset.path))
        ));
    }
    let variables = if project.variables.is_empty() {
        String::new()
    } else {
        format!(",variables:{}", serde_json::to_string(&project.variables)?)
    };
    source.push_str(&format!(
        "export default makeProject({{name:{},scenes:[{}],plugins:[semwrightExporterPlugin]{}{} }});\n",
        security::js_string(&project.id),
        (0..project.scenes.len())
            .map(|i| format!("s{i}"))
            .collect::<Vec<_>>()
            .join(","),
        if project.audio.is_empty() {
            ""
        } else {
            ",audio"
        },
        variables,
    ));

    files.insert(
        "src/semwright-exporter.ts".into(),
        br#"// Fixed Semwright exporter. Frame bytes leave the browser only through an owner-controlled Playwright binding.
import {ObjectMetaField} from '@motion-canvas/core';
import type {Exporter, Plugin, Project, RendererSettings} from '@motion-canvas/core';
declare global { var __SEMWRIGHT_EXPORT_FRAME__: undefined | ((frame: {frame:number; data:string}) => Promise<void>); }
class SemwrightImageExporter implements Exporter {
  static readonly id='@semwright/driver/image-sequence';
  static readonly displayName='Semwright image sequence';
  static meta(){ return new ObjectMetaField('Semwright image sequence', {}); }
  static async create(_project: Project, _settings: RendererSettings){ return new SemwrightImageExporter(); }
  async handleFrame(canvas: HTMLCanvasElement, frame: number, _sceneFrame: number, _sceneName: string, signal: AbortSignal){
    if(signal.aborted) return;
    const send=globalThis.__SEMWRIGHT_EXPORT_FRAME__;
    if(typeof send !== 'function') throw new Error('Semwright exporter binding unavailable');
    await send({frame,data:canvas.toDataURL('image/png')});
  }
}
export const semwrightExporterPlugin: Plugin={name:'semwright-driver-exporter-v1',exporters:()=>[SemwrightImageExporter]};
"#.to_vec(),
    );
    files.insert("src/project.ts".into(), source.into_bytes());
    files.insert("src/project.meta".into(), serde_json::to_vec_pretty(&json!({"version":0,
        "shared":{"size":[project.settings.width,project.settings.height],"background":project.settings.background,
            "audioOffset":project.audio.first().map_or(0.0, |a| a.offset_ms as f64 / 1000.0)},
        "preview":{"fps":project.settings.fps,"resolutionScale":1},
        "rendering":{"fps":project.settings.fps,"resolutionScale":1,
            "colorSpace":match project.settings.color_space { ColorSpace::Srgb => "srgb", ColorSpace::DisplayP3 => "display-p3" },
            "exporter":{"name":"@semwright/driver/image-sequence","options":{}}}}))?);
    files.insert("src/modules.d.ts".into(), b"declare module '*?scene' { const scene: import('@motion-canvas/core').FullSceneDescription; export default scene; }\ndeclare module '*?url' { const url: string; export default url; }\ndeclare module '*?raw' { const text: string; export default text; }\n".to_vec());
    files.insert("tsconfig.json".into(), serde_json::to_vec_pretty(&json!({"compilerOptions":{
        "target":"ES2022","module":"ESNext","moduleResolution":"Bundler","strict":true,"skipLibCheck":true,
        "jsx":"react-jsx","jsxImportSource":"@motion-canvas/2d/lib","allowSyntheticDefaultImports":true,
        "resolveJsonModule":true},"include":["src"]}))?);
    files.insert("vite.config.ts".into(), b"import {defineConfig} from 'vite';\nimport motionCanvasModule from '@motion-canvas/vite-plugin';\nconst motionCanvas = typeof motionCanvasModule === 'function' ? motionCanvasModule : (motionCanvasModule as unknown as {default: typeof motionCanvasModule}).default;\nexport default defineConfig({plugins:[motionCanvas({project:'./src/project.ts'})]});\n".to_vec());
    let mut package: Value = serde_json::from_str(include_str!(
        "../../../integrations/motion-canvas/runtime/package.json"
    ))?;
    package["scripts"] =
        json!({"typecheck":"tsc --noEmit","build":"vite build","serve":"vite --host 127.0.0.1"});
    files.insert("package.json".into(), serde_json::to_vec_pretty(&package)?);
    files.insert(
        "package-lock.json".into(),
        include_bytes!("../../../integrations/motion-canvas/runtime/package-lock.json").to_vec(),
    );
    files.insert("semwright-compiler.json".into(), serde_json::to_vec_pretty(&json!({
        "schema_version":1,"compiler_version":COMPILER_VERSION,"motion_canvas_version":MOTION_CANVAS_VERSION,
        "semantic_sha256":security::sha256(&serde_json::to_vec(project)?)}))?);
    if files.values().map(Vec::len).sum::<usize>() > 8_388_608 {
        return Err(Error::invalid("Generated source exceeds byte budget"));
    }
    Ok(Generated { files })
}
