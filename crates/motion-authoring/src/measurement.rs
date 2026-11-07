//! Streaming native-frame verification. Desired geometry is never measurement.
use crate::*;
use c::{Digest, Result, Verdict, ensure};
use schemars::JsonSchema;
use semwright_media_time::{Rate, Rational as Q};
use semwright_semantic_composition as c;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum Known<T> {
    Known { value: T },
    Unknown { reason: String },
}
impl<T> Known<T> {
    pub fn value(&self) -> Option<&T> {
        match self {
            Self::Known { value } => Some(value),
            Self::Unknown { .. } => None,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
impl Bounds {
    fn validate(self) -> Result<()> {
        crate::validate::point(Point {
            x: self.x,
            y: self.y,
        })?;
        crate::validate::finite(self.width, 0.0, 1e6)?;
        crate::validate::finite(self.height, 0.0, 1e6)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextObservation {
    pub logical_text_digest: Digest,
    pub observed_text_digest: Digest,
    pub truncated: Known<bool>,
    pub lines: Known<u32>,
    pub font_family: Known<String>,
    pub font_ready: Known<bool>,
    pub fallback_used: Known<bool>,
    pub direction: Known<String>,
}
/// Runtime class observed on the original native node, never the authored kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum NativeNodeType {
    Node,
    Layout,
    Rect,
    Circle,
    Line,
    Txt,
    Code,
    Img,
    Video,
    Camera,
}
impl NativeNodeType {
    fn is_layout(self) -> bool {
        !matches!(self, Self::Node | Self::Camera)
    }
}

fn validate_observation_reason<T>(value: &Known<T>) -> Result<()> {
    if let Known::Unknown { reason } = value {
        ensure(
            !reason.is_empty() && reason.len() <= 256,
            "native layout unknown reason bounds",
        )?;
    }
    Ok(())
}

/// Four ordered native rectangle corners (TL, TR, BR, BL), not an AABB.
pub type NativeScreenPolygon = [[f64; 2]; 4];
pub type NativeDrawMatrix = [f64; 6];

fn required_nullable_string<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NativeLineageNode {
    pub native_key: String,
    #[serde(deserialize_with = "required_nullable_string")]
    pub subject_id: Option<String>,
    pub native_type: NativeNodeType,
    #[serde(deserialize_with = "required_nullable_string")]
    pub parent_key: Option<String>,
    pub local_to_parent: NativeDrawMatrix,
    pub opacity: f64,
    pub layout_enabled: Known<bool>,
    pub clip: Known<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NativeRectClip {
    pub native_key: String,
    pub local_box: Bounds,
    pub matrix: NativeDrawMatrix,
    pub screen_polygon: NativeScreenPolygon,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeOwnClip {
    None,
    Rect { clip: NativeRectClip },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NativeGeometryObservation {
    pub native_key: String,
    pub screen_layout_polygon: Known<NativeScreenPolygon>,
    pub lineage: Known<Vec<NativeLineageNode>>,
    pub own_clip: Known<NativeOwnClip>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NativeCameraIdentity {
    pub native_key: String,
    #[serde(deserialize_with = "required_nullable_string")]
    pub subject_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NativeCameraSceneObservation {
    pub native_key: String,
    pub original_scene_key: String,
    pub current_scene_key: String,
    pub current_scene_is_original: bool,
    pub draw_scene_key: String,
    pub draw_uses_original_scene: bool,
    pub scene_transform: Known<NativeDrawMatrix>,
    pub position: [f64; 2],
    pub rotation: f64,
    pub zoom: f64,
    pub cameras: Known<Vec<NativeCameraIdentity>>,
}

/// The affected pixels of actual Canvas glyph operations on the native scene
/// buffer. This does not assert final visibility, isolated glyph coverage or GOLD.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NativeTextPixelObservation {
    pub native_key: String,
    pub frame: u64,
    pub width: u32,
    pub height: u32,
    pub playback_fps: f64,
    pub fps_num: u32,
    pub fps_den: u32,
    pub selected_frames: Vec<u64>,
    pub render_input_digest: Digest,
    #[serde(deserialize_with = "required_nullable_string")]
    pub output_background: Option<String>,
    pub calls: Vec<NativeTextPixelCall>,
    pub mask: NativeTextPixelMask,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NativeTextPixelOperation {
    FillText,
    StrokeText,
}

fn required_nullable_f64<'de, D>(deserializer: D) -> std::result::Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<f64>::deserialize(deserializer)
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NativeTextPixelCall {
    pub serial: u32,
    pub operation: NativeTextPixelOperation,
    pub matrix: NativeDrawMatrix,
    pub text: String,
    pub text_digest: Digest,
    pub font: String,
    pub color: [u8; 3],
    pub x: f64,
    pub y: f64,
    #[serde(deserialize_with = "required_nullable_f64")]
    pub max_width: Option<f64>,
    pub changed_pixels: u32,
    pub global_alpha: f64,
    pub composite: String,
    pub filter: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NativeTextPixelMask {
    pub encoding: String,
    pub data: String,
    pub runs: u32,
    pub changed_pixels: u32,
}

impl NativeTextPixelMask {
    fn validate(&self, width: u32, height: u32) -> Result<()> {
        ensure(
            self.encoding == "row-runs-u16le-base64"
                && self.data.len() <= 87_376
                && self.data.len().is_multiple_of(8)
                && self.runs as usize == self.data.len() / 8,
            "native glyph mask encoding or transport bound",
        )?;
        let mut previous: Option<(u32, u32)> = None;
        let mut changed = 0_u32;
        // Each canonical six-byte run is exactly eight base64 characters, so
        // padding and alternate encodings are unnecessary and rejected.
        for run in self.data.as_bytes().chunks_exact(8) {
            let mut word = 0_u64;
            for byte in run {
                let part = match byte {
                    b'A'..=b'Z' => u64::from(byte - b'A'),
                    b'a'..=b'z' => u64::from(byte - b'a' + 26),
                    b'0'..=b'9' => u64::from(byte - b'0' + 52),
                    b'+' => 62,
                    b'/' => 63,
                    _ => return Err(c::ContractError::Invalid("native glyph mask base64".into())),
                };
                word = (word << 6) | part;
            }
            let bytes = word.to_be_bytes();
            let y = u32::from(u16::from_le_bytes([bytes[2], bytes[3]]));
            let x = u32::from(u16::from_le_bytes([bytes[4], bytes[5]]));
            let count = u32::from(u16::from_le_bytes([bytes[6], bytes[7]]));
            ensure(
                y < height && x < width && count > 0 && x + count <= width,
                "native glyph mask run leaves the observed canvas",
            )?;
            if let Some((old_y, old_end)) = previous {
                ensure(
                    y > old_y || (y == old_y && x > old_end),
                    "native glyph mask runs overlap, touch or are unordered",
                )?;
            }
            previous = Some((y, x + count));
            changed += count;
        }
        ensure(
            changed == self.changed_pixels && changed <= width * height,
            "native glyph mask changed-pixel count differs",
        )
    }
}

impl FrameObservation {
    fn validate_native_text_pixels(&self) -> Result<()> {
        let mut known_count = 0_usize;
        let mut calls = BTreeSet::new();
        let mut binding: Option<&NativeTextPixelObservation> = None;
        for subject in &self.subjects {
            let Some(observed) = &subject.native_text_pixels else {
                continue;
            };
            validate_observation_reason(observed)?;
            let Some(value) = observed.value() else {
                continue;
            };
            known_count += 1;
            ensure(known_count <= 2, "native glyph observed subject budget")?;
            validate_native_key(&value.native_key)?;
            ensure(
                subject.native_type.as_ref().and_then(Known::value) == Some(&NativeNodeType::Txt)
                    && subject.text.is_some()
                    && subject
                        .native_geometry
                        .as_ref()
                        .and_then(Known::value)
                        .is_some_and(|geometry| geometry.native_key == value.native_key),
                "native glyph identity requires its observed native text node",
            )?;
            ensure(
                value.frame == self.frame
                    && (1..=4096).contains(&value.width)
                    && (1..=4096).contains(&value.height)
                    && u64::from(value.width) * u64::from(value.height) <= 1920 * 1080
                    && !value.selected_frames.is_empty()
                    && value.selected_frames.len() <= 7
                    && value.selected_frames.contains(&self.frame)
                    && value
                        .selected_frames
                        .windows(2)
                        .all(|pair| pair[0] < pair[1])
                    && value.selected_frames.iter().all(|frame| *frame <= 18_000),
                "native glyph frame, canvas or selected roster differs",
            )?;
            ensure(
                value.fps_den > 0
                    && value.fps_num >= value.fps_den
                    && u64::from(value.fps_num) <= 120 * u64::from(value.fps_den)
                    && Rate::new(value.fps_num, value.fps_den)
                        .is_ok_and(|rate| rate.num == value.fps_num && rate.den == value.fps_den)
                    && value.playback_fps == f64::from(value.fps_num) / f64::from(value.fps_den),
                "native glyph playback/export rate differs",
            )?;
            if let Some(color) = &value.output_background {
                ensure(
                    color.len() == 7
                        && color.starts_with('#')
                        && color.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit),
                    "native glyph output background must be opaque or absent",
                )?;
            }
            if let Some(first) = binding {
                ensure(
                    value.width == first.width
                        && value.height == first.height
                        && value.fps_num == first.fps_num
                        && value.fps_den == first.fps_den
                        && value.playback_fps == first.playback_fps
                        && value.selected_frames == first.selected_frames
                        && value.render_input_digest == first.render_input_digest
                        && value.output_background == first.output_background,
                    "native glyph subjects have different render bindings",
                )?;
            } else {
                binding = Some(value);
            }
            ensure(
                value.calls.len() <= 64,
                "native glyph paint operation budget",
            )?;
            let mut prior_serial = None;
            let mut summed_changes = 0_u64;
            for call in &value.calls {
                ensure(
                    call.serial < 64
                        && calls.insert(call.serial)
                        && prior_serial.is_none_or(|prior| call.serial > prior),
                    "native glyph operation serial is repeated or unordered",
                )?;
                prior_serial = Some(call.serial);
                validate_native_matrix(&call.matrix)?;
                crate::validate::finite(call.x, -2e6, 2e6)?;
                crate::validate::finite(call.y, -2e6, 2e6)?;
                if let Some(width) = call.max_width {
                    crate::validate::finite(width, 0.0, 2e6)?;
                    ensure(width > 0.0, "native glyph maximum width must be positive")?;
                }
                ensure(
                    call.text.len() <= 8192
                        && call.text_digest == Digest::of_bytes(call.text.as_bytes())
                        && !call.font.is_empty()
                        && call.font.len() <= 512
                        && call.global_alpha == 1.0
                        && call.composite == "source-over"
                        && call.filter == "none"
                        && call.changed_pixels <= value.width * value.height,
                    "native glyph text, style or paint context differs",
                )?;
                if let Some(background) = &value.output_background {
                    let same = (0..3).all(|index| {
                        u8::from_str_radix(&background[1 + index * 2..3 + index * 2], 16)
                            .is_ok_and(|channel| channel == call.color[index])
                    });
                    ensure(!same, "native glyph has no output background contrast")?;
                }
                summed_changes += u64::from(call.changed_pixels);
            }
            value.mask.validate(value.width, value.height)?;
            ensure(
                u64::from(value.mask.changed_pixels) <= summed_changes
                    && (value.mask.changed_pixels > 0
                        || value.calls.iter().all(|call| call.changed_pixels == 0)),
                "native glyph union is inconsistent with its real paint operations",
            )?;
            if value.calls.is_empty() {
                ensure(
                    subject.opacity.value() == Some(&0.0)
                        || subject
                            .text
                            .as_ref()
                            .is_some_and(|text| text.observed_text_digest == Digest::of_bytes(b"")),
                    "native glyph calls absent without observed empty or transparent text",
                )?;
            }
        }
        Ok(())
    }
}

fn validate_native_key(key: &str) -> Result<()> {
    ensure(
        !key.is_empty() && key.len() <= 512 && key.bytes().all(|b| (32..=126).contains(&b)),
        "native object key bounds",
    )
}

fn validate_native_matrix(matrix: &NativeDrawMatrix) -> Result<()> {
    for value in matrix {
        crate::validate::finite(*value, -1e6, 1e6)?;
    }
    Ok(())
}

fn validate_native_polygon(polygon: &NativeScreenPolygon) -> Result<()> {
    for point in polygon {
        for value in point {
            crate::validate::finite(*value, -2e6, 2e6)?;
        }
    }
    Ok(())
}

fn native_extent_agrees(declared: f64, minimum: f64, maximum: f64) -> bool {
    // JSON decimal parsing can move an endpoint by one f64 ULP. Subtracting
    // the parsed endpoints then need not equal the independently serialized
    // native width/height bit for bit. This is only a numerical consistency
    // bound; the separate physical geometry oracle retains its pixel guard.
    let scale = 1.0_f64
        .max(minimum.abs())
        .max(maximum.abs())
        .max(declared.abs());
    (declared - (maximum - minimum)).abs() <= 4.0 * f64::EPSILON * scale
}

impl SubjectObservation {
    fn validate_native_geometry(&self) -> Result<()> {
        let present =
            usize::from(self.native_geometry.is_some()) + usize::from(self.camera_scene.is_some());
        ensure(
            present == 0 || present == 2,
            "native geometry extension must be complete or absent",
        )?;
        if present == 0 {
            return Ok(());
        }
        ensure(
            self.native_type.is_some()
                && self.layout_root.is_some()
                && self.screen_layout_box.is_some(),
            "native geometry requires the typed layout extension",
        )?;
        let kind = self.native_type.as_ref().and_then(Known::value);
        let geometry = self.native_geometry.as_ref().unwrap();
        let camera = self.camera_scene.as_ref().unwrap();
        validate_observation_reason(geometry)?;
        validate_observation_reason(camera)?;
        if let Some(value) = geometry.value() {
            validate_native_key(&value.native_key)?;
            ensure(
                kind.is_some(),
                "native geometry identity requires observed native type",
            )?;
            validate_observation_reason(&value.screen_layout_polygon)?;
            validate_observation_reason(&value.lineage)?;
            validate_observation_reason(&value.own_clip)?;
            if let Some(polygon) = value.screen_layout_polygon.value() {
                validate_native_polygon(polygon)?;
                ensure(
                    self.drawn
                        && kind.is_some_and(|k| k.is_layout())
                        && self.layout_root.as_ref().and_then(Known::value).is_some()
                        && self.transform.value().is_some(),
                    "native polygon requires a completed mapped Layout draw",
                )?;
                let bounds = self.screen_layout_box.as_ref().and_then(Known::value);
                ensure(
                    bounds.is_some(),
                    "native polygon requires its separately observed AABB",
                )?;
                let bounds = bounds.unwrap();
                let min_x = polygon.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
                let min_y = polygon.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
                let max_x = polygon
                    .iter()
                    .map(|p| p[0])
                    .fold(f64::NEG_INFINITY, f64::max);
                let max_y = polygon
                    .iter()
                    .map(|p| p[1])
                    .fold(f64::NEG_INFINITY, f64::max);
                ensure(
                    bounds.x == min_x
                        && bounds.y == min_y
                        && native_extent_agrees(bounds.width, min_x, max_x)
                        && native_extent_agrees(bounds.height, min_y, max_y),
                    "native polygon and AABB disagree",
                )?;
            }
            if let Some(lineage) = value.lineage.value() {
                ensure(
                    self.drawn && !lineage.is_empty() && lineage.len() <= 64,
                    "native lineage draw or bounds",
                )?;
                let mut keys = BTreeSet::new();
                let mut subjects = BTreeSet::new();
                for (index, node) in lineage.iter().enumerate() {
                    validate_native_key(&node.native_key)?;
                    ensure(
                        keys.insert(&node.native_key),
                        "native lineage duplicate or cycle",
                    )?;
                    if let Some(subject) = &node.subject_id {
                        crate::validate::id(subject)?;
                        ensure(
                            subjects.insert(subject),
                            "native lineage semantic identity repeated",
                        )?;
                    }
                    if let Some(parent) = &node.parent_key {
                        validate_native_key(parent)?;
                    }
                    ensure(
                        node.parent_key.as_deref()
                            == lineage.get(index + 1).map(|p| p.native_key.as_str()),
                        "native lineage parent edge incomplete",
                    )?;
                    validate_native_matrix(&node.local_to_parent)?;
                    crate::validate::finite(node.opacity, 0.0, 1.0)?;
                    validate_observation_reason(&node.layout_enabled)?;
                    validate_observation_reason(&node.clip)?;
                    if node.layout_enabled.value().is_some() || node.clip.value().is_some() {
                        ensure(
                            node.native_type.is_layout(),
                            "native Layout signal on non-Layout ancestor",
                        )?;
                    }
                }
                ensure(
                    lineage[0].native_key == value.native_key
                        && lineage[0].subject_id.as_deref() == Some(self.id.as_str())
                        && Some(&lineage[0].native_type) == kind,
                    "native lineage does not start at its observed subject",
                )?;
            }
            if let Some(clip) = value.own_clip.value() {
                ensure(
                    self.drawn && kind == Some(&NativeNodeType::Layout),
                    "native rectangular clip requires exact Layout draw",
                )?;
                let first = value.lineage.value().and_then(|rows| rows.first());
                match clip {
                    NativeOwnClip::None => {
                        if let Some(first) = first {
                            ensure(
                                first.clip.value() == Some(&false),
                                "native no-clip observation contradicts signal",
                            )?;
                        }
                    }
                    NativeOwnClip::Rect { clip } => {
                        if let Some(first) = first {
                            ensure(
                                first.clip.value() == Some(&true),
                                "native rect clip observation contradicts signal",
                            )?;
                        }
                        validate_native_key(&clip.native_key)?;
                        ensure(
                            clip.native_key == value.native_key,
                            "native clip attributed to another node",
                        )?;
                        clip.local_box.validate()?;
                        ensure(
                            clip.local_box.width > 0.0 && clip.local_box.height > 0.0,
                            "native clip rectangle is empty",
                        )?;
                        validate_native_matrix(&clip.matrix)?;
                        validate_native_polygon(&clip.screen_polygon)?;
                        ensure(
                            self.transform.value().is_some(),
                            "native clip has no admitted output transform",
                        )?;
                    }
                }
            }
        }
        if let Some(value) = camera.value() {
            ensure(
                self.drawn && kind == Some(&NativeNodeType::Camera),
                "native camera scene requires completed Camera draw",
            )?;
            validate_native_key(&value.native_key)?;
            for key in [
                &value.original_scene_key,
                &value.current_scene_key,
                &value.draw_scene_key,
            ] {
                validate_native_key(key)?;
            }
            ensure(
                geometry
                    .value()
                    .is_some_and(|g| g.native_key == value.native_key),
                "native camera geometry identity differs",
            )?;
            if value.current_scene_is_original {
                ensure(
                    value.current_scene_key == value.original_scene_key,
                    "native current Camera.scene identity contradicts keys",
                )?;
            }
            if value.draw_uses_original_scene {
                ensure(
                    value.draw_scene_key == value.original_scene_key,
                    "native drawn Camera.scene identity contradicts keys",
                )?;
            }
            for scalar in value.position {
                crate::validate::finite(scalar, -1e6, 1e6)?;
            }
            crate::validate::finite(value.rotation, -1e6, 1e6)?;
            crate::validate::finite(value.zoom, 0.0, 1e6)?;
            ensure(value.zoom > 0.0, "native camera zoom must be positive")?;
            validate_observation_reason(&value.scene_transform)?;
            if let Some(matrix) = value.scene_transform.value() {
                validate_native_matrix(matrix)?;
            }
            validate_observation_reason(&value.cameras)?;
            if let Some(cameras) = value.cameras.value() {
                ensure(cameras.len() <= 64, "native camera inventory bound")?;
                let mut keys = BTreeSet::new();
                let mut subjects = BTreeSet::new();
                for camera in cameras {
                    validate_native_key(&camera.native_key)?;
                    ensure(
                        keys.insert(&camera.native_key),
                        "native camera inventory duplicate",
                    )?;
                    if let Some(id) = &camera.subject_id {
                        crate::validate::id(id)?;
                        ensure(
                            subjects.insert(id),
                            "native camera semantic identity repeated",
                        )?;
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SubjectObservation {
    pub id: String,
    /// Absent together in legacy observations. Absence is not native evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_type: Option<Known<NativeNodeType>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout_root: Option<Known<bool>>,
    /// Native centered layout rectangle projected through the actual draw matrix.
    /// This is separate from bounds, which describes drawable/cache contents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen_layout_box: Option<Known<Bounds>>,
    /// Optional together with camera_scene; old observations remain distinguishable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_geometry: Option<Known<NativeGeometryObservation>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera_scene: Option<Known<NativeCameraSceneObservation>>,
    /// Real per-operation pixel changes; absence and UNKNOWN prove no ink coverage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_text_pixels: Option<Known<NativeTextPixelObservation>>,
    pub local_size: Known<Size>,
    pub bounds: Known<Bounds>,
    pub transform: Known<[f64; 6]>,
    pub opacity: Known<f64>,
    pub drawn: bool,
    pub z_order: Known<u32>,
    pub clip_active: Known<bool>,
    pub pixel_visibility: Known<bool>,
    pub text: Option<TextObservation>,
    pub asset_ready: Known<bool>,
}
impl SubjectObservation {
    fn validate_native_layout(&self) -> Result<()> {
        let present = [
            self.native_type.is_some(),
            self.layout_root.is_some(),
            self.screen_layout_box.is_some(),
        ]
        .into_iter()
        .filter(|value| *value)
        .count();
        ensure(
            present == 0 || present == 3,
            "native layout observation extension must be complete or absent",
        )?;
        if let (Some(kind), Some(root), Some(screen)) = (
            &self.native_type,
            &self.layout_root,
            &self.screen_layout_box,
        ) {
            validate_observation_reason(kind)?;
            validate_observation_reason(root)?;
            validate_observation_reason(screen)?;
            let native_layout = kind.value().is_some_and(|kind| kind.is_layout());
            if root.value().is_some() {
                ensure(
                    self.drawn && native_layout,
                    "layout root requires a completed native Layout draw",
                )?;
            }
            if let Some(bounds) = screen.value() {
                bounds.validate()?;
                ensure(
                    self.drawn
                        && native_layout
                        && root.value().is_some()
                        && self.transform.value().is_some(),
                    "screen layout box requires native layout and output transform observations",
                )?;
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransitionObservation {
    pub invocation_id: String,
    pub finished: Known<bool>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CaptionObservation {
    pub id: String,
    pub active: Known<bool>,
    pub cue_start: Known<Q>,
    pub cue_end: Known<Q>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrameObservation {
    pub frame: u64,
    pub subjects: Vec<SubjectObservation>,
    pub transitions: Vec<TransitionObservation>,
    pub captions: Vec<CaptionObservation>,
    pub unresolved_cues: Vec<String>,
}
impl FrameObservation {
    pub fn validate(&self) -> Result<()> {
        ensure(
            self.subjects.len() <= 1024
                && self.transitions.len() <= 1024
                && self.captions.len() <= 512
                && self.unresolved_cues.len() <= 512,
            "native observation collection budget",
        )?;
        self.validate_native_text_pixels()?;
        let mut seen = BTreeSet::new();
        for subject in &self.subjects {
            crate::validate::id(&subject.id)?;
            ensure(seen.insert(&subject.id), "duplicate observed subject")?;
            subject.validate_native_layout()?;
            subject.validate_native_geometry()?;
            if let Some(b) = subject.bounds.value() {
                b.validate()?;
            }
            if let Some(s) = subject.local_size.value() {
                crate::validate::finite(s.width, 0.0, 1e6)?;
                crate::validate::finite(s.height, 0.0, 1e6)?;
            }
            if let Some(t) = subject.transform.value() {
                for n in t {
                    crate::validate::finite(*n, -1e6, 1e6)?;
                }
            }
            if let Some(alpha) = subject.opacity.value() {
                crate::validate::finite(*alpha, 0.0, 1.0)?;
            }
        }
        let mut seen = BTreeSet::new();
        for t in &self.transitions {
            crate::validate::id(&t.invocation_id)?;
            ensure(
                seen.insert(&t.invocation_id),
                "duplicate observed transition",
            )?;
        }
        let mut seen = BTreeSet::new();
        for caption in &self.captions {
            crate::validate::id(&caption.id)?;
            ensure(seen.insert(&caption.id), "duplicate observed caption")?;
            for q in [caption.cue_start.value(), caption.cue_end.value()]
                .into_iter()
                .flatten()
            {
                q.validate()?;
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RangeCoverage {
    pub first_frame: u64,
    pub end_frame_exclusive: u64,
    pub observed_frames: u64,
    pub exhaustive: bool,
    pub fps: Rate,
    pub method: String,
    pub observation_digest: Digest,
    pub render_input_digest: Digest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_resources_digest: Option<Digest>,
    pub artifact_digest: Digest,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MetricSummary {
    pub subject: String,
    pub native_draw_frames: u64,
    pub pixel_visible_frames: u64,
    pub pixel_visibility_unknown_frames: u64,
    pub observed_frames: u64,
    pub maximum_speed: Option<f64>,
    pub maximum_acceleration: Option<f64>,
    pub kinematics_unknown_frames: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MotionFinding {
    pub id: String,
    pub rule: String,
    pub subject: String,
    pub verdict: Verdict,
    pub evidence_class: c::EvidenceClass,
    pub first_frame: Option<u64>,
    pub affected_frames: u64,
    pub reason: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MotionMeasurement {
    pub version: u32,
    pub intent_digest: Digest,
    pub coverage: RangeCoverage,
    pub metrics: Vec<MetricSummary>,
    pub findings: Vec<MotionFinding>,
    pub validation: c::ValidationReport,
}
struct Metric {
    summary: MetricSummary,
    previous: Option<(u64, Point, f64)>,
    known_velocity: bool,
}
struct RuleState {
    subject: String,
    failed: u64,
    unknown: u64,
    checked: u64,
    first: Option<u64>,
    reason: String,
}
fn combine(s: &mut RuleState, verdict: Verdict, frame: u64, reason: &str) {
    s.checked += 1;
    match verdict {
        Verdict::Pass => {}
        Verdict::Fail => {
            s.failed += 1;
            s.first.get_or_insert(frame);
            s.reason = reason.into();
        }
        Verdict::Unknown => {
            s.unknown += 1;
            if s.failed == 0 {
                s.reason = reason.into();
            }
        }
    }
}
fn rule_subject(rule: &VisualConstraint) -> &str {
    match rule {
        VisualConstraint::SafeArea { subject, .. }
        | VisualConstraint::NoOverlap { subject, .. }
        | VisualConstraint::MinimumVisible { subject, .. }
        | VisualConstraint::AspectRatio { subject, .. }
        | VisualConstraint::NativeText { subject }
        | VisualConstraint::FontLoaded { subject }
        | VisualConstraint::NoTruncation { subject }
        | VisualConstraint::MaximumSpeed { subject, .. } => subject,
    }
}
fn verdict(value: bool) -> Verdict {
    if value { Verdict::Pass } else { Verdict::Fail }
}
fn rectangle(subject: &Subject) -> bool {
    matches!(subject.content,SubjectContent::Rectangle{radius,..}if radius==0.0)
}
fn axis_aligned(o: &SubjectObservation) -> bool {
    o.transform
        .value()
        .is_some_and(|m| m[1].abs() < 1e-9 && m[2].abs() < 1e-9)
}
/// `coverage` and `base` must be built from a Host-owned completed render receipt.
/// This pure function does not authenticate user JSON; production adapters must
/// never accept a claimed native stream from capability arguments.
pub fn measure<I>(
    film: &Film,
    plan_digest: Digest,
    base: c::BaseStateSet,
    mut coverage: RangeCoverage,
    frames: I,
) -> Result<MotionMeasurement>
where
    I: IntoIterator<Item = Result<FrameObservation>>,
{
    let realized = realize(film)?;
    base.validate()?;
    coverage.fps.validate()?;
    ensure(
        coverage.fps == film.output.frame_rate
            && coverage.first_frame < coverage.end_frame_exclusive,
        "measurement range/rate mismatch",
    )?;
    ensure(
        coverage.end_frame_exclusive <= realized.schedule.frame_count(&film.output)?,
        "measurement range exceeds Film",
    )?;
    let mut states = BTreeMap::new();
    let mut rules = vec![];
    let mut subjects = BTreeMap::new();
    let mut metrics = BTreeMap::new();
    for shot in film.shots() {
        for subject in &shot.subjects {
            subjects.insert(subject.id.as_str(), subject);
            metrics.insert(
                subject.id.clone(),
                Metric {
                    summary: MetricSummary {
                        subject: subject.id.clone(),
                        native_draw_frames: 0,
                        pixel_visible_frames: 0,
                        pixel_visibility_unknown_frames: 0,
                        observed_frames: 0,
                        maximum_speed: None,
                        maximum_acceleration: None,
                        kinematics_unknown_frames: 0,
                    },
                    previous: None,
                    known_velocity: false,
                },
            );
        }
        for (i, rule) in shot.constraints.iter().enumerate() {
            let name = format!("{}:constraint:{i}", shot.id);
            states.insert(
                name.clone(),
                RuleState {
                    subject: rule_subject(rule).into(),
                    failed: 0,
                    unknown: 0,
                    checked: 0,
                    first: None,
                    reason: "required native observation absent".into(),
                },
            );
            rules.push((name, shot, rule));
        }
    }
    let mut previous = None;
    let mut observed = 0;
    let mut cue_failure = false;
    let mut caption_failure = false;
    // Redacted per-frame diagnostic only. No text, source path, cue or provider
    // reference is stored, and a mismatch still fails native verification.
    let mut caption_first_mismatch = None;
    let mut caption_mismatch_frames = 0_u64;
    let transition_required = film
        .shots()
        .flat_map(|shot| shot.motion.iter().map(|invocation| invocation.id.clone()))
        .collect::<BTreeSet<_>>();
    let mut transition_observed = BTreeSet::new();
    let mut transition_failure = false;
    let mut transition_unknown = false;
    let cues = film.cues.resolve()?;
    for result in frames {
        let frame = result?;
        frame.validate()?;
        ensure(
            frame.frame >= coverage.first_frame && frame.frame < coverage.end_frame_exclusive,
            "frame outside declared measurement range",
        )?;
        if let Some(prev) = previous {
            ensure(frame.frame > prev, "duplicate/unordered native frame")?;
        }
        previous = Some(frame.frame);
        observed += 1;
        let now = coverage.fps.at(i64::try_from(frame.frame)
            .map_err(|_| c::ContractError::Limit("frame index".into()))?)?;
        let observations = frame
            .subjects
            .iter()
            .map(|s| (s.id.as_str(), s))
            .collect::<BTreeMap<_, _>>();
        ensure(
            observations.keys().all(|id| subjects.contains_key(id)),
            "renderer produced unowned subject identity",
        )?;
        for o in &frame.subjects {
            let m = metrics.get_mut(&o.id).expect("known subject");
            m.summary.observed_frames += 1;
            if o.drawn {
                m.summary.native_draw_frames += 1;
            }
            match o.pixel_visibility.value() {
                Some(true) => m.summary.pixel_visible_frames += 1,
                Some(false) => {}
                None => m.summary.pixel_visibility_unknown_frames += 1,
            }
            if let Some(b) = o.bounds.value() {
                let p = Point {
                    x: b.x + b.width / 2.0,
                    y: b.y + b.height / 2.0,
                };
                if let Some((last, old, speed)) = m.previous {
                    if frame.frame == last + 1 {
                        let dt = f64::from(coverage.fps.den) / f64::from(coverage.fps.num);
                        let v = ((p.x - old.x).powi(2) + (p.y - old.y).powi(2)).sqrt() / dt;
                        m.summary.maximum_speed =
                            Some(m.summary.maximum_speed.map_or(v, |n| n.max(v)));
                        if m.known_velocity {
                            let a = (v - speed).abs() / dt;
                            m.summary.maximum_acceleration =
                                Some(m.summary.maximum_acceleration.map_or(a, |n| n.max(a)));
                        }
                        m.previous = Some((frame.frame, p, v));
                        m.known_velocity = true;
                    } else {
                        m.summary.kinematics_unknown_frames += 1;
                        m.previous = Some((frame.frame, p, 0.0));
                        m.known_velocity = false;
                    }
                } else {
                    m.previous = Some((frame.frame, p, 0.0));
                }
            } else {
                m.summary.kinematics_unknown_frames += 1;
                m.previous = None;
                m.known_velocity = false;
            }
        }
        for (name, shot, rule) in &rules {
            if !realized.schedule.interval(&shot.span_id)?.contains(now) {
                continue;
            }
            let state = states.get_mut(name).expect("registered rule");
            let Some(o) = observations.get(rule_subject(rule)) else {
                combine(
                    state,
                    Verdict::Unknown,
                    frame.frame,
                    "subject was not observed in its active shot",
                );
                continue;
            };
            let (v, reason) = match rule {
                VisualConstraint::SafeArea { tolerance, .. } => {
                    if let Some(b) = o.bounds.value() {
                        let safe = film.output.safe_area;
                        (
                            verdict(
                                b.x >= safe.left - tolerance
                                    && b.y >= safe.top - tolerance
                                    && b.x + b.width
                                        <= f64::from(film.output.width) - safe.right + tolerance
                                    && b.y + b.height
                                        <= f64::from(film.output.height) - safe.bottom + tolerance,
                            ),
                            "native transformed bounds exceed declared safe area",
                        )
                    } else {
                        (Verdict::Unknown, "native output bounds unobservable")
                    }
                }
                VisualConstraint::NoOverlap {
                    other, tolerance, ..
                } => match (o.bounds.value(), observations.get(other.as_str())) {
                    (Some(a), Some(other_o)) => {
                        if let Some(b) = other_o.bounds.value() {
                            let w = (a.x + a.width).min(b.x + b.width) - a.x.max(b.x);
                            let h = (a.y + a.height).min(b.y + b.height) - a.y.max(b.y);
                            if w <= *tolerance || h <= *tolerance {
                                (Verdict::Pass, "disjoint observed bounds")
                            } else if rectangle(subjects[o.id.as_str()])
                                && rectangle(subjects[other.as_str()])
                                && axis_aligned(o)
                                && axis_aligned(other_o)
                                && o.clip_active.value() == Some(&false)
                                && other_o.clip_active.value() == Some(&false)
                            {
                                (
                                    Verdict::Fail,
                                    "explicit no-overlap rectangle constraint violated",
                                )
                            } else {
                                (
                                    Verdict::Unknown,
                                    "intersecting bounds do not prove overlap of clipped/transformed geometry",
                                )
                            }
                        } else {
                            (Verdict::Unknown, "other bounds unavailable")
                        }
                    }
                    _ => (Verdict::Unknown, "overlap measurements incomplete"),
                },
                VisualConstraint::MinimumVisible { .. } => (
                    Verdict::Pass,
                    "aggregated over declared frame range after stream",
                ),
                VisualConstraint::AspectRatio {
                    ratio, tolerance, ..
                } => {
                    if let Some(s) = o.local_size.value() {
                        (
                            verdict(if s.height == 0.0 {
                                false
                            } else {
                                (s.width / s.height - ratio).abs() <= *tolerance
                            }),
                            "observed aspect ratio differs from declared ratio",
                        )
                    } else {
                        (Verdict::Unknown, "native local size unavailable")
                    }
                }
                VisualConstraint::NativeText { .. } => {
                    if let Some(t) = &o.text {
                        (
                            verdict(t.logical_text_digest == t.observed_text_digest),
                            "native observed text differs from declared text",
                        )
                    } else {
                        (Verdict::Fail, "subject has no native text observation")
                    }
                }
                VisualConstraint::FontLoaded { .. } => {
                    match o.text.as_ref().and_then(|t| t.font_ready.value()) {
                        Some(v) => (verdict(*v), "required font face/glyph evidence missing"),
                        None => (Verdict::Unknown, "native font readiness unobservable"),
                    }
                }
                VisualConstraint::NoTruncation { .. } => {
                    match o.text.as_ref().and_then(|t| t.truncated.value()) {
                        Some(v) => (verdict(!v), "native text metrics report clipping"),
                        None => (Verdict::Unknown, "native text clipping unobservable"),
                    }
                }
                VisualConstraint::MaximumSpeed {
                    pixels_per_second, ..
                } => {
                    let m = &metrics[o.id.as_str()].summary;
                    if m.kinematics_unknown_frames > 0 {
                        (
                            Verdict::Unknown,
                            "trajectory has missing/nonconsecutive observations",
                        )
                    } else if let Some(v) = m.maximum_speed {
                        (
                            verdict(v <= *pixels_per_second),
                            "finite-difference speed exceeds configured profile bound",
                        )
                    } else {
                        // The initial frame has no velocity yet. Defer missing
                        // evidence until the stream ends; otherwise its UNKNOWN
                        // would prevent a complete, valid trajectory from passing.
                        (Verdict::Pass, "speed evidence pending subsequent frames")
                    }
                }
            };
            combine(state, v, frame.frame, reason);
        }
        cue_failure |= !frame.unresolved_cues.is_empty();
        let mut frame_caption_mismatch = false;
        for shot in film.shots() {
            if !realized.schedule.interval(&shot.span_id)?.contains(now) {
                continue;
            }
            for caption in &shot.captions {
                let Some(semwright_media_time::ResolvedCue::Resolved { start, end }) =
                    cues.get(&caption.cue_id)
                else {
                    cue_failure = true;
                    continue;
                };
                let expected = *start <= now && now < *end;
                frame_caption_mismatch |= frame
                    .captions
                    .iter()
                    .find(|c| c.id == caption.id)
                    .and_then(|c| c.active.value())
                    .is_none_or(|active| *active != expected);
            }
            for invocation in &shot.motion {
                let end = realized.schedule.interval(&invocation.span_id)?.end;
                // Motion Canvas generators can finish while evaluating the frame
                // exactly at the half-open interval boundary. That frame proves
                // terminal visual state, but not yet that the generator's
                // continuation set the completion receipt. Require the first
                // observed frame strictly after the interval end.
                if now > end {
                    match frame
                        .transitions
                        .iter()
                        .find(|t| t.invocation_id == invocation.id)
                        .and_then(|t| t.finished.value())
                    {
                        Some(true) => {
                            transition_observed.insert(invocation.id.clone());
                        }
                        Some(false) => transition_failure = true,
                        None => transition_unknown = true,
                    }
                }
            }
        }
        if frame_caption_mismatch {
            caption_failure = true;
            caption_first_mismatch.get_or_insert(frame.frame);
            caption_mismatch_frames = caption_mismatch_frames
                .checked_add(1)
                .ok_or_else(|| c::ContractError::Limit("caption diagnostic frame count".into()))?;
        }
    }
    // Every frame has already proved that its integer index is in range and
    // strictly increasing. Such a sequence covers the entire range exactly
    // when its cardinality equals the range length; separate first/last/gap
    // checks would duplicate this invariant.
    let exhaustive = observed == coverage.end_frame_exclusive - coverage.first_frame;
    transition_unknown |= !transition_required.is_subset(&transition_observed);
    // Full-Film rules cannot PASS from a partial range even when that range is exhaustive.
    let full = coverage.first_frame == 0
        && coverage.end_frame_exclusive == realized.schedule.frame_count(&film.output)?;
    coverage.observed_frames = observed;
    coverage.exhaustive = exhaustive;
    for (name, _, rule) in &rules {
        if let VisualConstraint::MaximumSpeed { subject, .. } = rule
            && metrics[subject].summary.maximum_speed.is_none()
        {
            let state = states.get_mut(name).expect("rule");
            state.unknown += 1;
            state.reason = "at least two consecutive frames needed for speed".into();
        }
        if let VisualConstraint::MinimumVisible { subject, duration } = rule {
            let m = &metrics[subject].summary;
            let state = states.get_mut(name).expect("rule");
            let visible = coverage.fps.at(m.pixel_visible_frames as i64)?;
            if m.pixel_visibility_unknown_frames > 0 || !exhaustive || !full {
                state.unknown += 1;
                state.reason="drawn opacity is not proof of visible pixel contribution; incomplete pixel evidence".into();
            } else if visible < *duration {
                state.failed += 1;
                state.reason = "measured pixel-visible duration below declared minimum".into();
            }
        }
    }
    let resource = base.0.first().expect("validated base").key.clone();
    let observation = c::ObservationRef {
        id: format!("render-{}", &coverage.observation_digest.as_str()[..16]),
        base: base.clone(),
        source: c::EvidenceSource::RendererState,
        method: coverage.method.clone(),
        method_version: 1,
        scope: vec![c::Address {
            resource: resource.clone(),
            logical_id: film.id.clone(),
            property: "native-frame-state".into(),
        }],
        artifact: Some(coverage.artifact_digest.clone()),
        exhaustive: exhaustive && full,
    };
    let mut required = BTreeSet::new();
    let mut checks = vec![];
    let mut findings = vec![];
    for (name, state) in states {
        let verdict = if state.failed > 0 {
            Verdict::Fail
        } else if state.unknown > 0 || state.checked == 0 {
            Verdict::Unknown
        } else {
            Verdict::Pass
        };
        required.insert(name.clone());
        checks.push(c::RuleResult {
            rule: name.clone(),
            version: 1,
            verdict,
            evidence_class: c::EvidenceClass::Deterministic,
            evidence: vec![observation.clone()],
            reason: if verdict == Verdict::Pass {
                None
            } else {
                Some(state.reason.clone())
            },
        });
        if verdict != Verdict::Pass {
            findings.push(MotionFinding {
                id: name.clone(),
                rule: name,
                subject: state.subject,
                verdict,
                evidence_class: c::EvidenceClass::Deterministic,
                first_frame: state.first,
                affected_frames: state.failed + state.unknown,
                reason: state.reason,
            });
        }
    }
    if caption_failure {
        // This diagnostic is intentionally source-neutral and bounded by the
        // requested native frame range. It never stores caption text, source
        // voice data, user paths, opaque provider refs or a claimed PASS.
        findings.push(MotionFinding {
            id: "caption-timing".into(),
            rule: "caption-timing".into(),
            subject: "native-caption-observation".into(),
            verdict: Verdict::Fail,
            evidence_class: c::EvidenceClass::Deterministic,
            first_frame: caption_first_mismatch,
            affected_frames: caption_mismatch_frames,
            reason: "caption state differs from resolved cue".into(),
        });
    }
    for (name, passed, reason) in [
        (
            "native-frame-coverage",
            exhaustive && full,
            "full requested native frame coverage missing",
        ),
        ("resolved-cues", !cue_failure, "unresolved cue observed"),
        (
            "caption-timing",
            !caption_failure,
            "caption state differs from resolved cue",
        ),
    ] {
        required.insert(name.into());
        checks.push(c::RuleResult {
            rule: name.into(),
            version: 1,
            verdict: if passed {
                Verdict::Pass
            } else if name == "native-frame-coverage" {
                Verdict::Unknown
            } else {
                Verdict::Fail
            },
            evidence_class: c::EvidenceClass::Deterministic,
            evidence: vec![observation.clone()],
            reason: if passed { None } else { Some(reason.into()) },
        });
    }
    required.insert("transition-completion".into());
    checks.push(c::RuleResult {
        rule: "transition-completion".into(),
        version: 1,
        verdict: if transition_failure {
            Verdict::Fail
        } else if transition_unknown {
            Verdict::Unknown
        } else {
            Verdict::Pass
        },
        evidence_class: c::EvidenceClass::Deterministic,
        evidence: vec![observation.clone()],
        reason: if transition_failure {
            Some("transition observed incomplete after its half-open interval".into())
        } else if transition_unknown {
            Some("no post-interval native frame proved transition completion".into())
        } else {
            None
        },
    });
    let validation = c::ValidationReport {
        plan_digest,
        base,
        required_rules: required,
        checks,
    };
    validation.verdict()?;
    Ok(MotionMeasurement {
        version: 1,
        intent_digest: c::canonical_digest(film)?,
        coverage,
        metrics: metrics.into_values().map(|m| m.summary).collect(),
        findings,
        validation,
    })
}
