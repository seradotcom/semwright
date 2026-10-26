//! Twelve editable Film inputs for the FixedTxt RED/GREEN experiment.
//! This module authors intent and expectations; it never renders or captures Host input.
use semwright_media_time::{Anchor, Cue, CueGraph, Rate, Rational as Q};
use semwright_motion_authoring::*;
use semwright_semantic_composition::{Digest, strict_decode};
use serde::Serialize;
use sha2::{Digest as ShaDigest, Sha256};
use std::collections::BTreeMap;

type Result<T> = std::result::Result<T, String>;
pub const CAPTION_TEXT: &str = "An editable caption stays in place.";
pub const FIXTURE_KEYS: [&str; 12] = [
    "fixed-causal",
    "flow-control-base",
    "flow-control-with-fixed",
    "flow-text-control",
    "fixed-multirun",
    "fixed-multirun-control",
    "fixed-transform-clip",
    "fixed-camera-scene",
    "fixed-counter-captions",
    "fixed-layer-behind",
    "fixed-layer-equal",
    "fixed-layer-front",
];
const FRAMES: &[u32] = &[0, 45, 120, 179];

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BoxExpectation {
    pub subject: String,
    pub frames: Vec<u32>,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TextExpectation {
    pub subject: String,
    pub frame: u32,
    pub text: String,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Thresholds {
    pub status: &'static str,
    pub guard_pixels: u32,
    pub minimum_ink_pixels: u32,
    pub unchanged_control_rgba_difference: u32,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Expectations {
    pub status: &'static str,
    pub authority: &'static str,
    pub fps: u32,
    pub frame_count: u32,
    pub frames: Vec<u32>,
    pub boxes: Vec<BoxExpectation>,
    pub checks: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compare_with: Option<String>,
    pub texts: Vec<TextExpectation>,
    pub thresholds: Thresholds,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Fixture {
    pub key: String,
    pub film: Film,
    pub aliases: BTreeMap<String, String>,
    pub expected: Expectations,
}
fn require(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}
fn q(num: i64, den: i64) -> Q {
    Q::new(num, den).expect("fixed fixture rational")
}
fn span(id: String, start: Q, duration: Q) -> TemporalSpan {
    TemporalSpan {
        id,
        minimum: duration,
        preferred: duration,
        maximum: duration,
        anchor: StartAnchor::Absolute { time: start },
        preference_priority: 0,
    }
}
fn fixed(x: f64, y: f64, width: f64, height: f64) -> SpatialIntent {
    SpatialIntent::Fixed {
        position: Point { x, y },
        size: Size { width, height },
    }
}
fn run(text: &str, weight: u16, color: &str, emphasis: bool) -> TextRun {
    TextRun {
        text: text.into(),
        weight,
        color: Some(color.into()),
        emphasis,
    }
}
fn text_runs(runs: Vec<TextRun>, style: &str) -> SubjectContent {
    SubjectContent::Text {
        runs,
        style: style.into(),
        direction: TextDirection::Ltr,
        language: "en".into(),
        wrap: true,
        truncate: false,
    }
}
fn text(value: &str, style: &str) -> SubjectContent {
    text_runs(vec![run(value, 400, "#F4F4F4", false)], style)
}
fn rect(fill: &str) -> SubjectContent {
    SubjectContent::Rectangle {
        fill: fill.into(),
        stroke: None,
        radius: 0.0,
    }
}
#[derive(Clone, Copy)]
enum Plane {
    Under,
    Text,
}
#[derive(Clone)]
struct Builder {
    fixture: Fixture,
    under: String,
    text: String,
    motion_count: u32,
}
impl Builder {
    fn new(source: &Film, key: &str, archetype: Archetype) -> Self {
        let sequence_id = format!("{key}-sequence");
        let shot_id = format!("{key}-shot");
        let sequence_span = format!("{key}-sequence-span");
        let shot_span = format!("{key}-shot-span");
        let under = format!("{key}-under");
        let text_layer = format!("{key}-text-layer");
        let over = format!("{key}-over");
        let film = Film {
            version: 1,
            id: source.id.clone(),
            output: OutputProfile {
                width: 1920,
                height: 1080,
                frame_rate: Rate::new(30, 1).expect("fixed fixture rate"),
                aspect: AspectFamily::Landscape,
                safe_area: Insets {
                    top: 64.0,
                    right: 64.0,
                    bottom: 64.0,
                    left: 64.0,
                },
            },
            editorial: EditorialSystem {
                version: 1,
                font: source.editorial.font.clone(),
                mono_font: source.editorial.mono_font.clone(),
                type_scale: BTreeMap::from([
                    ("title".into(), 56.0),
                    ("body".into(), 36.0),
                    ("small".into(), 28.0),
                ]),
                colors: BTreeMap::from([
                    ("text".into(), "#F4F4F4".into()),
                    ("background".into(), "#111418".into()),
                    ("accent".into(), "#D4A62A".into()),
                ]),
                spacing: BTreeMap::from([("base".into(), 24.0), ("block".into(), 24.0)]),
                stroke: 0.0,
                corner_radius: 0.0,
            },
            timing: TemporalGraph {
                version: 1,
                duration: q(6, 1),
                spans: vec![
                    span(sequence_span.clone(), Q::ZERO, q(6, 1)),
                    span(shot_span.clone(), Q::ZERO, q(6, 1)),
                ],
                constraints: vec![TemporalConstraint::Contains {
                    id: format!("{key}-shot-contained"),
                    parent: sequence_span.clone(),
                    child: shot_span.clone(),
                }],
            },
            sequences: vec![Sequence {
                id: sequence_id,
                span_id: sequence_span,
                beats: vec![Beat {
                    id: format!("{key}-beat"),
                    role: NarrativeRole::Mechanism,
                    shots: vec![Shot {
                        id: shot_id,
                        span_id: shot_span,
                        archetype,
                        subjects: vec![],
                        layers: vec![
                            Layer {
                                id: under.clone(),
                                order: 0,
                                intentional_overlay: false,
                            },
                            Layer {
                                id: text_layer.clone(),
                                order: 10,
                                intentional_overlay: true,
                            },
                            Layer {
                                id: over,
                                order: 20,
                                intentional_overlay: true,
                            },
                        ],
                        annotations: vec![],
                        captions: vec![],
                        motion: vec![],
                        constraints: vec![],
                    }],
                }],
            }],
            cues: CueGraph {
                version: 1,
                cues: vec![],
            },
            assets: source.assets.clone(),
        };
        Self {
            fixture: Fixture {
                key: key.into(),
                film,
                aliases: BTreeMap::new(),
                expected: Expectations {
                    status: "EXPECTATIONS_ONLY",
                    authority: "ORIGINAL_NATIVE_PNGS_AND_ACTUAL_PROBES",
                    fps: 30,
                    frame_count: 180,
                    frames: FRAMES.to_vec(),
                    boxes: vec![],
                    checks: vec![],
                    compare_with: None,
                    texts: vec![],
                    thresholds: Thresholds {
                        status: "PROPOSED_NOT_CALIBRATED",
                        guard_pixels: 2,
                        minimum_ink_pixels: 32,
                        unchanged_control_rgba_difference: 0,
                    },
                },
            },
            under,
            text: text_layer,
            motion_count: 0,
        }
    }
    fn shot(&mut self) -> &mut Shot {
        &mut self.fixture.film.sequences[0].beats[0].shots[0]
    }
    fn add(
        &mut self,
        alias: &str,
        content: SubjectContent,
        layout: SpatialIntent,
        attachment: (Option<&str>, Plane),
        role: &str,
        clip: bool,
    ) -> String {
        let (parent, plane) = attachment;
        let id = format!("{}-{alias}", self.fixture.key);
        let layer = match plane {
            Plane::Under => self.under.clone(),
            Plane::Text => self.text.clone(),
        };
        assert!(
            self.fixture
                .aliases
                .insert(alias.into(), id.clone())
                .is_none(),
            "duplicate fixture alias"
        );
        self.shot().subjects.push(Subject {
            id: id.clone(),
            role: role.into(),
            parent: parent.map(str::to_owned),
            layer,
            content,
            layout,
            initially_visible: true,
            clip_intentional: clip,
        });
        id
    }
    fn motion(&mut self, primitive: Primitive, start: Q, duration: Q) {
        self.motion_count += 1;
        let id = format!("{}-motion-{}", self.fixture.key, self.motion_count);
        let span_id = format!("{id}-span");
        let parent = self.shot().span_id.clone();
        self.fixture
            .film
            .timing
            .spans
            .push(span(span_id.clone(), start, duration));
        self.fixture
            .film
            .timing
            .constraints
            .push(TemporalConstraint::Contains {
                id: format!("{id}-contained"),
                parent,
                child: span_id.clone(),
            });
        self.shot().motion.push(Invocation {
            id,
            span_id,
            easing: MotionEasing::Linear,
            primitive,
        });
    }
    fn set(&mut self, target: &str, channel: Channel, value: Operand) {
        self.motion(
            Primitive::GeometrySet {
                target: target.into(),
                channel,
                value,
            },
            Q::ZERO,
            q(1, 30),
        );
    }
    fn boxed(&mut self, subject: &str, x: f64, y: f64, width: f64, height: f64, frames: &[u32]) {
        self.fixture.expected.boxes.push(BoxExpectation {
            subject: subject.into(),
            frames: frames.to_vec(),
            x,
            y,
            width,
            height,
        });
    }
    fn checks(&mut self, checks: &[&str]) {
        self.fixture
            .expected
            .checks
            .extend(checks.iter().map(|s| (*s).to_owned()));
    }
}

/// Only this admission entry is called by the component CLI. Font and asset
/// identities are inherited from original input; it creates no managed asset.
pub fn build_fixtures(source: &Film, caption_source: &[u8]) -> Result<Vec<Fixture>> {
    source
        .validate()
        .map_err(|e| format!("Invalid original Film: {e}"))?;
    for font in [&source.editorial.font, &source.editorial.mono_font] {
        require(
            font.fallback == FontFallback::Deny && font.permitted_fallbacks.is_empty(),
            "FixedTxt fixture execution requires explicit fonts without fallback",
        )?;
        if let Some(digest) = &font.asset_digest {
            require(
                source.assets.iter().any(|asset| &asset.sha256 == digest),
                "Original Film must declare each admitted font asset",
            )?;
        } else {
            require(
                matches!(
                    font.family.as_str(),
                    "Instrument Sans Variable" | "IBM Plex Mono"
                ),
                "An unpinned font must be an original bundled family",
            )?;
        }
    }
    let fixtures = compose(source, caption_source)?;
    for fixture in &fixtures {
        let realized =
            realize(&fixture.film).map_err(|e| format!("Invalid {}: {e}", fixture.key))?;
        require(
            realized
                .schedule
                .frame_count(&fixture.film.output)
                .map_err(|e| e.to_string())?
                == 180,
            "Fixture did not preserve its exact 180-frame clock",
        )?;
    }
    Ok(fixtures)
}

fn compose(source: &Film, caption_source: &[u8]) -> Result<Vec<Fixture>> {
    require(
        caption_source == CAPTION_TEXT.as_bytes(),
        "Caption source must contain the exact authored UTF-8 caption without an added newline",
    )?;
    let digest_json = serde_json::to_vec(&hex::encode(Sha256::digest(caption_source)))
        .map_err(|e| format!("Cannot encode caption source digest: {e}"))?;
    let caption_digest: Digest =
        strict_decode(&digest_json).map_err(|e| format!("Invalid caption source digest: {e}"))?;
    let mut out = Vec::with_capacity(12);
    {
        let mut c = Builder::new(source, "fixed-causal", Archetype::Statement);
        let title = c.add(
            "title",
            text("FIXED TITLE 180 140", "title"),
            fixed(-260.0, -340.0, 1040.0, 120.0),
            (None, Plane::Text),
            "primary",
            false,
        );
        let body = c.add(
            "body",
            text("Editable body remains in its authored box.", "body"),
            fixed(-260.0, -150.0, 1040.0, 120.0),
            (None, Plane::Text),
            "secondary",
            false,
        );
        let gold = c.add(
            "gold",
            rect("#D4A62A"),
            fixed(425.0, -105.0, 150.0, 150.0),
            (None, Plane::Under),
            "evidence",
            false,
        );
        c.motion(
            Primitive::GeometryTween {
                target: title.clone(),
                channel: Channel::Position,
                from: Operand::Vector(Point {
                    x: -260.0,
                    y: -340.0,
                }),
                to: Operand::Vector(Point {
                    x: -260.0,
                    y: -320.0,
                }),
            },
            q(3, 1),
            q(1, 1),
        );
        c.boxed(&title, 180.0, 140.0, 1040.0, 120.0, &[0, 45]);
        c.boxed(&title, 180.0, 160.0, 1040.0, 120.0, &[120, 179]);
        c.boxed(&body, 180.0, 330.0, 1040.0, 120.0, FRAMES);
        c.boxed(&gold, 1310.0, 360.0, 150.0, 150.0, FRAMES);
        c.checks(&[
            "Fixed text follows authored position; local y moves exactly 20 units once.",
            "Gold control geometry and original RGBA remain identical between RED/GREEN.",
            "Ink is visible inside authorial boxes; layout boxes are not asserted equal to glyph cache bounds.",
            "No displaced duplicate text ink exists; logical subject identities remain Txt.",
        ]);
        out.push(c.fixture);
    }
    {
        let mut c = Builder::new(source, "flow-control-base", Archetype::Metric);
        for (alias, color) in [("flowA", "#3B82F6"), ("flowB", "#10B981")] {
            let subject = c.add(
                alias,
                rect(color),
                SpatialIntent::Flow {
                    grow: 1.0,
                    align: Align::Stretch,
                },
                (None, Plane::Under),
                "primary",
                false,
            );
            c.set(&subject, Channel::Width, Operand::Number(320.0));
            c.set(&subject, Channel::Height, Operand::Number(140.0));
        }
        c.fixture.expected.frames = vec![45, 120, 179];
        c.checks(&["Record actual nonzero flow rectangles for paired comparison."]);
        // Preserve original flow, layer, shot, sequence, motion and span identities.
        let mut pair = c.clone();
        out.push(c.fixture);
        pair.fixture.key = "flow-control-with-fixed".into();
        pair.fixture.expected.compare_with = Some("flow-control-base".into());
        let subject = pair.add(
            "fixedText",
            text("FIXED TEXT MUST NOT CONSUME FLOW", "body"),
            fixed(-260.0, -340.0, 1040.0, 120.0),
            (None, Plane::Text),
            "primary",
            false,
        );
        pair.boxed(&subject, 180.0, 140.0, 1040.0, 120.0, &[45, 120, 179]);
        pair.checks(&["Paired flow subjects keep exactly their actual centers and sizes; fixed text consumes no flex space or gap."]);
        out.push(pair.fixture);
    }
    {
        let mut c = Builder::new(source, "flow-text-control", Archetype::Statement);
        c.add(
            "flowTitle",
            text("THIS TEXT REMAINS IN FLOW", "title"),
            SpatialIntent::Flow {
                grow: 0.0,
                align: Align::Start,
            },
            (None, Plane::Text),
            "primary",
            false,
        );
        c.add(
            "flowBody",
            text(
                "Only fixed text receives a technical layout parent.",
                "body",
            ),
            SpatialIntent::Flow {
                grow: 0.0,
                align: Align::Start,
            },
            (None, Plane::Text),
            "secondary",
            false,
        );
        c.checks(&["RED/GREEN flow geometry and rasterization are unchanged; no technical parent on flow text."]);
        out.push(c.fixture);
    }
    for control in [false, true] {
        let key = if control {
            "fixed-multirun-control"
        } else {
            "fixed-multirun"
        };
        let mut c = Builder::new(source, key, Archetype::Statement);
        let parent = control.then(|| {
            c.add(
                "disabledParent",
                SubjectContent::Group,
                fixed(0.0, 0.0, 1920.0, 1080.0),
                (None, Plane::Under),
                "primary",
                false,
            )
        });
        let subject = c.add(
            "text",
            text_runs(
                vec![
                    run("Preserve ", 400, "#F4F4F4", false),
                    run("bold ", 700, "#D4A62A", false),
                    run("and italic.\n", 400, "#3B82F6", true),
                    run(
                        "Second line retains editable text and wrap.",
                        400,
                        "#F4F4F4",
                        false,
                    ),
                ],
                "body",
            ),
            fixed(-260.0, -200.0, 1040.0, 240.0),
            (parent.as_deref(), Plane::Text),
            "secondary",
            false,
        );
        c.boxed(&subject, 180.0, 220.0, 1040.0, 240.0, FRAMES);
        if !control {
            c.fixture.expected.compare_with = Some("fixed-multirun-control".into());
        }
        c.checks(&[
            "Styled runs/newline/wrap match the same real Txt under an already layout-disabled identity parent.",
            "Use the original control PNG, not a fabricated font measurement.",
        ]);
        out.push(c.fixture);
    }
    {
        let mut c = Builder::new(source, "fixed-transform-clip", Archetype::Statement);
        let parent = c.add(
            "parent",
            SubjectContent::Group,
            fixed(0.0, 0.0, 900.0, 500.0),
            (None, Plane::Under),
            "primary",
            true,
        );
        c.add(
            "text",
            text("Clipped text inside a transformed parent.", "title"),
            fixed(250.0, 0.0, 700.0, 160.0),
            (Some(&parent), Plane::Text),
            "primary",
            false,
        );
        c.set(&parent, Channel::Rotation, Operand::Number(8.0));
        c.set(
            &parent,
            Channel::Scale,
            Operand::Vector(Point { x: 1.05, y: 0.95 }),
        );
        c.fixture.expected.frames = vec![45, 120, 179];
        c.checks(&[
            "Parent transform applies once; local text position/size remain (250,0)/(700,160).",
            "Inherited clipping remains active and technical parent has identity transform/no clip; ink respects the transformed clip.",
        ]);
        out.push(c.fixture);
    }
    {
        let mut c = Builder::new(source, "fixed-camera-scene", Archetype::Statement);
        let camera = c.add(
            "camera",
            SubjectContent::Camera { zoom: 1.0 },
            SpatialIntent::Flow {
                grow: 0.0,
                align: Align::Start,
            },
            (None, Plane::Under),
            "primary",
            false,
        );
        let subject = c.add(
            "text",
            text("TEXT INSIDE CAMERA SCENE", "title"),
            fixed(-260.0, -340.0, 1040.0, 120.0),
            (Some(&camera), Plane::Text),
            "primary",
            false,
        );
        let gold = c.add(
            "gold",
            rect("#D4A62A"),
            fixed(425.0, -105.0, 150.0, 150.0),
            (Some(&camera), Plane::Under),
            "primary",
            false,
        );
        c.boxed(&subject, 180.0, 140.0, 1040.0, 120.0, FRAMES);
        c.boxed(&gold, 1310.0, 360.0, 150.0, 150.0, FRAMES);
        c.checks(&[
            "Attach through the original Camera.scene; no second camera or logical node replacement.",
            "Verify actual identity-camera mapping and gold control before checking text.",
        ]);
        out.push(c.fixture);
    }
    {
        let mut c = Builder::new(source, "fixed-counter-captions", Archetype::Statement);
        let counter = c.add(
            "counter",
            text("0.0%", "title"),
            fixed(-260.0, -340.0, 1040.0, 120.0),
            (None, Plane::Text),
            "primary",
            false,
        );
        let caption = c.add(
            "caption",
            text("Caption pending", "body"),
            fixed(-260.0, -100.0, 1040.0, 160.0),
            (None, Plane::Text),
            "caption",
            false,
        );
        c.motion(
            Primitive::Counter {
                target: counter.clone(),
                from: 0.0,
                to: 100.0,
                decimal_places: 1,
                prefix: String::new(),
                suffix: "%".into(),
            },
            Q::ZERO,
            q(6, 1),
        );
        let cue_id = "fixed-counter-captions-caption-cue".to_owned();
        c.fixture.film.cues.cues.push(Cue {
            id: cue_id.clone(),
            anchor: Anchor::Absolute { time: q(1, 1) },
            duration: q(3, 1),
            source: caption_digest.clone(),
            method: "manual-timing".into(),
            version: 1,
            confidence: None,
        });
        c.shot().captions.push(Caption {
            id: "fixed-counter-captions-caption-binding".into(),
            subject: caption.clone(),
            cue_id,
            language: "en".into(),
            text: CAPTION_TEXT.into(),
        });
        let frames = [0, 29, 30, 45, 119, 120, 179];
        c.fixture.expected.frames = frames.to_vec();
        c.boxed(&counter, 180.0, 140.0, 1040.0, 120.0, &frames);
        c.boxed(&caption, 180.0, 360.0, 1040.0, 160.0, &[30, 45, 119]);
        for (frame, value) in [
            (0, "0.0%"),
            (29, "16.1%"),
            (30, "16.7%"),
            (45, "25.0%"),
            (119, "66.1%"),
            (120, "66.7%"),
            (179, "99.4%"),
        ] {
            c.fixture.expected.texts.push(TextExpectation {
                subject: counter.clone(),
                frame,
                text: value.into(),
            });
        }
        for frame in [30, 45, 119] {
            c.fixture.expected.texts.push(TextExpectation {
                subject: caption.clone(),
                frame,
                text: CAPTION_TEXT.into(),
            });
        }
        c.checks(&[
            "Existing counter/caption instanceof Txt checks succeed; visible edits remain in place.",
            "Caption active at45, inactive before30 and at/after120; verify actual frame timestamps before numeric text checks.",
            "Do not assert terminal six-second counter value at frame179.",
        ]);
        out.push(c.fixture);
    }
    for (suffix, order, visible) in [
        ("behind", -1, false),
        ("equal", 0, true),
        ("front", 1, true),
    ] {
        let key = format!("fixed-layer-{suffix}");
        let mut c = Builder::new(source, &key, Archetype::Statement);
        let text_layer = c.text.clone();
        c.shot()
            .layers
            .iter_mut()
            .find(|layer| layer.id == text_layer)
            .expect("fixture text layer")
            .order = order;
        let card = c.add(
            "opaqueControl",
            rect("#D4A62A"),
            fixed(0.0, 0.0, 1040.0, 160.0),
            (None, Plane::Under),
            "primary",
            false,
        );
        let subject = c.add(
            "text",
            text("LAYER ORDER", "title"),
            fixed(0.0, 0.0, 900.0, 120.0),
            (None, Plane::Text),
            "primary",
            false,
        );
        c.boxed(&card, 440.0, 460.0, 1040.0, 160.0, FRAMES);
        c.boxed(&subject, 510.0, 480.0, 900.0, 120.0, FRAMES);
        let visibility = format!("Visible text over opaque control must be {visible}.");
        c.checks(&[&visibility,
            "Post-attach zIndex assignment remains effective and reactive; equal layers preserve insertion order."]);
        out.push(c.fixture);
    }
    require(
        out.iter().map(|f| f.key.as_str()).eq(FIXTURE_KEYS),
        "FixedTxt fixture roster changed",
    )?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use semwright_driver_motion_canvas::authoring;
    use serde::Deserialize;
    use serde_json::{Value, json};

    fn template() -> Film {
        strict_decode(include_bytes!(
            "../../../../fixtures/composition/motion/technical.json"
        ))
        .unwrap()
    }
    fn source_fixtures() -> Vec<Fixture> {
        // Source-only technical input; not runtime font admission.
        compose(&template(), CAPTION_TEXT.as_bytes()).unwrap()
    }
    fn by_key<'a>(fixtures: &'a [Fixture], key: &str) -> &'a Fixture {
        fixtures.iter().find(|fixture| fixture.key == key).unwrap()
    }
    fn subject<'a>(fixture: &'a Fixture, alias: &str) -> &'a Subject {
        let id = &fixture.aliases[alias];
        fixture
            .film
            .shots()
            .flat_map(|shot| &shot.subjects)
            .find(|subject| &subject.id == id)
            .unwrap()
    }
    fn emitted_scene(bytes: &[u8]) -> Value {
        let source = std::str::from_utf8(bytes).unwrap();
        let tail = source.split_once("const data=JSON.parse(").unwrap().1;
        let mut parser = serde_json::Deserializer::from_str(tail);
        let encoded = String::deserialize(&mut parser).unwrap();
        serde_json::from_str(&encoded).unwrap()
    }
    #[test]
    fn twelve_films_validate_realize_project_and_compile_with_exact_scene_identity() {
        let source = template();
        let source_original = source.clone();
        let (base, _) = authoring::project(&source, None).unwrap();
        let base_bytes = crate::encode(&base).unwrap();
        let fixtures = source_fixtures();
        assert_eq!(fixtures.len(), 12);
        for fixture in fixtures {
            fixture.film.validate().unwrap();
            assert_eq!(fixture.film.id, source.id);
            assert_eq!(fixture.film.assets, source.assets);
            assert_eq!(fixture.film.editorial.font, source.editorial.font);
            assert_eq!(fixture.film.editorial.mono_font, source.editorial.mono_font);
            let compiled =
                crate::compile_inputs(&crate::encode(&fixture.film).unwrap(), &base_bytes).unwrap();
            assert_eq!(
                compiled
                    .realization
                    .schedule
                    .frame_count(&fixture.film.output)
                    .unwrap(),
                180
            );
            assert_eq!(authoring::frame_count(&compiled.project).unwrap(), 180);
            authoring::check_binding(&compiled.project).unwrap();
            assert_eq!(
                compiled.project.authoring.as_ref().unwrap().intent,
                fixture.film
            );
            assert_eq!(compiled.project.scenes.len(), 1);
            for scene in &compiled.project.scenes {
                let path = format!("src/scenes/{}.tsx", scene.id);
                let data = emitted_scene(&compiled.generated.files[&path]);
                assert_eq!(data["id"].as_str(), Some(scene.id.as_str()));
                assert_eq!(data["editorial"]["version"], json!(1));
                assert_eq!(
                    data["shots"][0]["subjects"],
                    serde_json::to_value(&fixture.film.sequences[0].beats[0].shots[0].subjects)
                        .unwrap()
                );
                let instructions: Vec<_> = compiled
                    .realization
                    .instructions
                    .iter()
                    .filter(|instruction| instruction.sequence == scene.id)
                    .collect();
                assert_eq!(
                    data["instructions"],
                    serde_json::to_value(&instructions).unwrap()
                );
                for instruction in data["instructions"].as_array().unwrap() {
                    assert_eq!(instruction["sequence"].as_str(), Some(scene.id.as_str()));
                }
            }
            assert_eq!(fixture.expected.frame_count, 180);
            assert_eq!(fixture.expected.status, "EXPECTATIONS_ONLY");
            assert_eq!(
                fixture.expected.thresholds.status,
                "PROPOSED_NOT_CALIBRATED"
            );
        }
        assert_eq!(source, source_original);
        assert_eq!(crate::encode(&base).unwrap(), base_bytes);
    }
    #[test]
    fn exactly_four_fixtures_use_the_closed_geometry_extension() {
        let fixtures = source_fixtures();
        let keys: Vec<_> = fixtures
            .iter()
            .filter(|fixture| {
                fixture
                    .film
                    .shots()
                    .flat_map(|shot| &shot.motion)
                    .any(|invocation| {
                        matches!(
                            &invocation.primitive,
                            Primitive::GeometrySet { .. } | Primitive::GeometryTween { .. }
                        )
                    })
            })
            .map(|fixture| fixture.key.as_str())
            .collect();
        assert_eq!(
            keys,
            [
                "fixed-causal",
                "flow-control-base",
                "flow-control-with-fixed",
                "fixed-transform-clip"
            ]
        );
        assert_eq!(fixtures.len() - keys.len(), 8);
        let causal = by_key(&fixtures, "fixed-causal");
        let realized = realize(&causal.film).unwrap();
        assert_eq!(realized.instructions.len(), 1);
        assert_eq!(realized.instructions[0].start, q(3, 1));
        assert_eq!(realized.instructions[0].duration, q(1, 1));
        assert_eq!(
            realized.instructions[0].operation,
            NativeOp::Tween {
                target: causal.aliases["title"].clone(),
                channel: Channel::Position,
                from: Some(Operand::Vector(Point {
                    x: -260.0,
                    y: -340.0
                })),
                to: Operand::Vector(Point {
                    x: -260.0,
                    y: -320.0
                }),
            }
        );
        for key in ["flow-control-base", "flow-control-with-fixed"] {
            let fixture = by_key(&fixtures, key);
            let realized = realize(&fixture.film).unwrap();
            assert_eq!(realized.instructions.len(), 4);
            for instruction in &realized.instructions {
                assert_eq!(instruction.start, Q::ZERO);
                assert_eq!(instruction.duration, Q::ZERO);
                let NativeOp::Set {
                    target,
                    channel,
                    value,
                } = &instruction.operation
                else {
                    panic!("expected Set")
                };
                assert!([&fixture.aliases["flowA"], &fixture.aliases["flowB"]].contains(&target));
                let expected = match channel {
                    Channel::Width => 320.0,
                    Channel::Height => 140.0,
                    _ => panic!("unexpected dimension channel"),
                };
                assert_eq!(*value, Operand::Number(expected));
            }
            for invocation in &fixture.film.sequences[0].beats[0].shots[0].motion {
                assert_eq!(
                    realized
                        .schedule
                        .interval(&invocation.span_id)
                        .unwrap()
                        .duration()
                        .unwrap(),
                    q(1, 30)
                );
            }
        }
        let transform = by_key(&fixtures, "fixed-transform-clip");
        let realized = realize(&transform.film).unwrap();
        assert_eq!(realized.instructions.len(), 2);
        assert!(
            realized
                .instructions
                .iter()
                .all(|instruction| instruction.start == Q::ZERO && instruction.duration == Q::ZERO)
        );
        assert!(
            realized
                .instructions
                .iter()
                .any(|instruction| instruction.operation
                    == NativeOp::Set {
                        target: transform.aliases["parent"].clone(),
                        channel: Channel::Rotation,
                        value: Operand::Number(8.0),
                    })
        );
        assert!(
            realized
                .instructions
                .iter()
                .any(|instruction| instruction.operation
                    == NativeOp::Set {
                        target: transform.aliases["parent"].clone(),
                        channel: Channel::Scale,
                        value: Operand::Vector(Point { x: 1.05, y: 0.95 }),
                    })
        );
        assert!(subject(transform, "parent").clip_intentional);
        assert_eq!(
            subject(transform, "text").layout,
            fixed(250.0, 0.0, 700.0, 160.0)
        );
    }
    #[test]
    fn paired_flow_diff_is_only_the_added_fixed_text_subject() {
        let fixtures = source_fixtures();
        let original = by_key(&fixtures, "flow-control-base");
        let paired = by_key(&fixtures, "flow-control-with-fixed");
        assert_eq!(subject(original, "flowA"), subject(paired, "flowA"));
        assert_eq!(subject(original, "flowB"), subject(paired, "flowB"));
        let mut recovered = paired.film.clone();
        recovered.sequences[0].beats[0].shots[0]
            .subjects
            .retain(|subject| subject.id != paired.aliases["fixedText"]);
        assert_eq!(recovered, original.film);
        assert_eq!(
            paired.expected.compare_with.as_deref(),
            Some("flow-control-base")
        );
    }
    #[test]
    fn captions_use_original_source_digest_and_exact_manual_cue_boundaries() {
        let fixtures = source_fixtures();
        let fixture = by_key(&fixtures, "fixed-counter-captions");
        let cue = &fixture.film.cues.cues[0];
        assert_eq!(
            cue.source.as_str(),
            hex::encode(Sha256::digest(CAPTION_TEXT.as_bytes()))
        );
        assert_eq!(cue.method, "manual-timing");
        assert_eq!(cue.confidence, None);
        assert_eq!(
            fixture.film.cues.resolve().unwrap()[&cue.id],
            semwright_media_time::ResolvedCue::Resolved {
                start: q(1, 1),
                end: q(4, 1)
            }
        );
        assert_eq!(fixture.expected.frames, [0, 29, 30, 45, 119, 120, 179]);
        let counter: Vec<_> = fixture
            .expected
            .texts
            .iter()
            .filter(|item| item.subject == fixture.aliases["counter"])
            .map(|item| (item.frame, item.text.as_str()))
            .collect();
        assert_eq!(
            counter,
            [
                (0, "0.0%"),
                (29, "16.1%"),
                (30, "16.7%"),
                (45, "25.0%"),
                (119, "66.1%"),
                (120, "66.7%"),
                (179, "99.4%")
            ]
        );
        let caption: Vec<_> = fixture
            .expected
            .texts
            .iter()
            .filter(|item| item.subject == fixture.aliases["caption"])
            .map(|item| (item.frame, item.text.as_str()))
            .collect();
        assert_eq!(
            caption,
            [(30, CAPTION_TEXT), (45, CAPTION_TEXT), (119, CAPTION_TEXT)]
        );
    }
    #[test]
    fn styled_runs_camera_parenting_and_layer_order_remain_authored() {
        let fixtures = source_fixtures();
        for key in ["fixed-multirun", "fixed-multirun-control"] {
            let fixture = by_key(&fixtures, key);
            let SubjectContent::Text {
                runs,
                wrap,
                truncate,
                ..
            } = &subject(fixture, "text").content
            else {
                panic!("native text")
            };
            assert_eq!(runs.len(), 4);
            assert_eq!(runs[1].weight, 700);
            assert_eq!(runs[1].color.as_deref(), Some("#D4A62A"));
            assert_eq!(runs[2].text, "and italic.\n");
            assert!(runs[2].emphasis && *wrap && !*truncate);
        }
        let control = by_key(&fixtures, "fixed-multirun-control");
        assert_eq!(
            subject(control, "text").parent.as_deref(),
            Some(control.aliases["disabledParent"].as_str())
        );
        assert_eq!(
            subject(control, "disabledParent").layout,
            fixed(0.0, 0.0, 1920.0, 1080.0)
        );
        let camera = by_key(&fixtures, "fixed-camera-scene");
        assert_eq!(
            subject(camera, "camera").content,
            SubjectContent::Camera { zoom: 1.0 }
        );
        for alias in ["text", "gold"] {
            assert_eq!(
                subject(camera, alias).parent.as_deref(),
                Some(camera.aliases["camera"].as_str())
            );
        }
        for (key, expected) in [
            ("fixed-layer-behind", -1),
            ("fixed-layer-equal", 0),
            ("fixed-layer-front", 1),
        ] {
            let fixture = by_key(&fixtures, key);
            let text = subject(fixture, "text");
            let shot = &fixture.film.sequences[0].beats[0].shots[0];
            assert_eq!(
                shot.layers
                    .iter()
                    .find(|layer| layer.id == text.layer)
                    .unwrap()
                    .order,
                expected
            );
            assert_eq!(shot.subjects[0].id, fixture.aliases["opaqueControl"]);
            assert_eq!(shot.subjects[1].id, text.id);
        }
    }
    #[test]
    fn every_film_reopens_as_editable_intent_and_recompiles_an_actual_edit() {
        let (base, _) = authoring::project(&template(), None).unwrap();
        let base_bytes = crate::encode(&base).unwrap();
        for fixture in source_fixtures() {
            let original = crate::encode(&fixture.film).unwrap();
            let before = crate::compile_inputs(&original, &base_bytes).unwrap();
            let mut reopened: Film = strict_decode(&original).unwrap();
            let shot = &mut reopened.sequences[0].beats[0].shots[0];
            let index = shot
                .subjects
                .iter()
                .position(|subject| matches!(&subject.content, SubjectContent::Text { .. }))
                .unwrap_or(0);
            match &mut shot.subjects[index].content {
                SubjectContent::Text { runs, .. } => runs[0].text.push_str(" EDITED"),
                SubjectContent::Rectangle { fill, .. } => *fill = "#80C0FF".into(),
                _ => panic!("fixture has no editable text or flow rectangle"),
            }
            let after = crate::compile_inputs(
                &crate::encode(&reopened).unwrap(),
                &crate::encode(&before.project).unwrap(),
            )
            .unwrap();
            assert_ne!(
                before.generated.fingerprint().unwrap(),
                after.generated.fingerprint().unwrap()
            );
            assert_eq!(after.project.authoring.as_ref().unwrap().intent, reopened);
            assert_eq!(authoring::frame_count(&after.project).unwrap(), 180);
            assert_eq!(strict_decode::<Film>(&original).unwrap(), fixture.film);
        }
    }
    #[test]
    fn runtime_fixture_admission_preserves_bundled_faces_and_emphasis() {
        let source = template();
        let original = source.clone();
        assert!(source.editorial.font.asset_digest.is_none());
        assert!(source.editorial.mono_font.asset_digest.is_none());
        let fixtures = build_fixtures(&source, CAPTION_TEXT.as_bytes()).unwrap();
        assert_eq!(fixtures.len(), 12);
        let (base, _) = authoring::project(&source, None).unwrap();
        let base_bytes = crate::encode(&base).unwrap();
        for key in ["fixed-multirun", "fixed-multirun-control"] {
            let fixture = by_key(&fixtures, key);
            assert_eq!(fixture.film.editorial.font, source.editorial.font);
            assert_eq!(fixture.film.editorial.mono_font, source.editorial.mono_font);
            assert_eq!(fixture.film.assets, source.assets);
            let SubjectContent::Text { runs, .. } = &subject(fixture, "text").content else {
                panic!("expected original authored text");
            };
            assert!(runs[2].emphasis);
            assert_eq!(runs[2].text, "and italic.\n");
            let compiled =
                crate::compile_inputs(&crate::encode(&fixture.film).unwrap(), &base_bytes).unwrap();
            assert_eq!(
                compiled.project.authoring.as_ref().unwrap().intent,
                fixture.film
            );
            assert_eq!(authoring::frame_count(&compiled.project).unwrap(), 180);
        }
        assert_eq!(source, original);
    }
    #[test]
    fn runtime_fixture_admission_rejects_unpinned_faces_and_fallback() {
        let source = template();
        let mut unpinned = source.clone();
        unpinned.editorial.font.family = "Unpinned test face".into();
        assert!(build_fixtures(&unpinned, CAPTION_TEXT.as_bytes()).is_err());
        let mut fallback = source.clone();
        fallback.editorial.font.fallback = FontFallback::AllowAndReport;
        assert!(build_fixtures(&fallback, CAPTION_TEXT.as_bytes()).is_err());
        assert_eq!(source, template());
    }
    #[test]
    fn runtime_fixture_admission_requires_original_imported_asset() {
        let mut source = template();
        source.editorial.font.family = "Original imported test face".into();
        let encoded = format!("\"{}\"", "ab".repeat(32));
        source.editorial.font.asset_digest =
            Some(strict_decode::<Digest>(encoded.as_bytes()).unwrap());
        let original = source.clone();
        assert!(build_fixtures(&source, CAPTION_TEXT.as_bytes()).is_err());
        assert_eq!(source, original);
    }
    #[test]
    fn caption_source_is_exact_and_cannot_be_replaced_by_a_claimed_digest() {
        let source = template();
        assert!(compose(&source, b"different caption").is_err());
        assert!(compose(&source, format!("{CAPTION_TEXT}\n").as_bytes()).is_err());
        assert!(
            compose(
                &source,
                hex::encode(Sha256::digest(CAPTION_TEXT.as_bytes())).as_bytes()
            )
            .is_err()
        );
        assert_eq!(
            compose(&source, CAPTION_TEXT.as_bytes()).unwrap(),
            source_fixtures()
        );
    }
}
