//! Editor intent rather than a mirror of XML services. Two-lane tracks remain two-lane tracks.
use crate::{
    Error, Result,
    hash::sha256,
    json::{Value, array, obj},
    time::{FrameRange, FrameRate, MAX_FRAME},
    xml::Node,
};
use std::collections::{BTreeMap, BTreeSet};
pub const MAX_TRACKS: usize = 128;
pub const MAX_CLIPS: usize = 10_000;
pub const MAX_EFFECTS: usize = 4_000;
pub const MAX_ASSETS: usize = 10_000;
pub const MAX_SEQUENCES: usize = 32;
pub const MAX_KEYFRAMES: usize = 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    GenericMlt,
    Kdenlive,
    Shotcut,
    ShotcutExport,
    ShotcutVirtual,
}
impl Format {
    pub fn name(self) -> &'static str {
        match self {
            Self::GenericMlt => "generic_mlt",
            Self::Kdenlive => "kdenlive",
            Self::Shotcut => "shotcut",
            Self::ShotcutExport => "shotcut_export",
            Self::ShotcutVirtual => "shotcut_virtual",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    pub width: u32,
    pub height: u32,
    pub fps: FrameRate,
    pub progressive: bool,
    pub sample_aspect: (u32, u32),
    pub display_aspect: (u32, u32),
    pub colorspace: u32,
    pub audio_channels: u32,
}
impl Default for Profile {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            fps: FrameRate { num: 25, den: 1 },
            progressive: true,
            sample_aspect: (1, 1),
            display_aspect: (16, 9),
            colorspace: 709,
            audio_channels: 2,
        }
    }
}
impl Profile {
    pub fn validate(&self) -> Result<()> {
        self.fps.validate()?;
        if self.width < 2
            || self.height < 2
            || self.width > 4096
            || self.height > 4096
            || self.audio_channels == 0
            || self.audio_channels > 8
            || self.sample_aspect.0 == 0
            || self.sample_aspect.1 == 0
            || self.display_aspect.0 == 0
            || self.display_aspect.1 == 0
        {
            return Err(Error::invalid("Invalid or excessive project profile"));
        }
        Ok(())
    }
    pub fn json(&self) -> Value {
        obj([
            ("width", u64::from(self.width).into()),
            ("height", u64::from(self.height).into()),
            ("fps_num", u64::from(self.fps.num).into()),
            ("fps_den", u64::from(self.fps.den).into()),
            ("progressive", self.progressive.into()),
            ("sample_aspect_num", u64::from(self.sample_aspect.0).into()),
            ("sample_aspect_den", u64::from(self.sample_aspect.1).into()),
            (
                "display_aspect_num",
                u64::from(self.display_aspect.0).into(),
            ),
            (
                "display_aspect_den",
                u64::from(self.display_aspect.1).into(),
            ),
            ("colorspace", u64::from(self.colorspace).into()),
            ("audio_channels", u64::from(self.audio_channels).into()),
        ])
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resource {
    Scoped { root: String, path: String },
    Relative(String),
    External(String),
    Color(String),
    Opaque(String),
}
impl Resource {
    pub fn text(&self) -> String {
        match self {
            Self::Scoped { root, path } => format!("{root}:{path}"),
            Self::Relative(s) | Self::External(s) | Self::Color(s) | Self::Opaque(s) => s.clone(),
        }
    }
    pub fn status(&self) -> &'static str {
        match self {
            Self::Scoped { .. } => "scoped_unopened",
            Self::Relative(_) => "relative_unopened",
            Self::External(_) => "outside_scope_unopened",
            Self::Color(_) => "generator",
            Self::Opaque(_) => "opaque_unopened",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaAsset {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub resource: Resource,
    pub frames: Option<u64>,
    pub service: String,
    pub original: Option<String>,
    pub proxy: Option<String>,
    pub opaque: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interpolation {
    Linear,
    Hold,
}
impl Interpolation {
    pub fn name(self) -> &'static str {
        match self {
            Self::Linear => "linear",
            Self::Hold => "hold",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keyframe {
    pub frame: u64,
    pub value: i64,
    pub interpolation: Interpolation,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Effect {
    pub id: String,
    pub service: String,
    pub property: String,
    pub value: i64,
    pub enabled: bool,
    pub keyframes: Vec<Keyframe>,
    pub opaque: Option<Node>,
}
impl Effect {
    pub fn validate(&self, duration: u64) -> Result<()> {
        if self.keyframes.len() > MAX_KEYFRAMES {
            return Err(Error::limit("Keyframe budget exceeded"));
        }
        if self.opaque.is_some() {
            return Ok(());
        }
        validate_effect_value(&self.service, &self.property, self.value)?;
        let mut prev = None;
        for k in &self.keyframes {
            if k.frame >= duration || prev.is_some_and(|p| p >= k.frame) {
                return Err(Error::invalid(
                    "Keyframes must be ordered, unique and inside target",
                ));
            }
            validate_effect_value(&self.service, &self.property, k.value)?;
            prev = Some(k.frame);
        }
        Ok(())
    }
    pub fn at(&self, frame: u64) -> i64 {
        let Some(first) = self.keyframes.first() else {
            return self.value;
        };
        if frame <= first.frame {
            return first.value;
        }
        for pair in self.keyframes.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            if frame < b.frame {
                if a.interpolation == Interpolation::Hold {
                    return a.value;
                }
                return (i128::from(a.value)
                    + (i128::from(b.value - a.value) * i128::from(frame - a.frame))
                        / i128::from(b.frame - a.frame)) as i64;
            }
        }
        self.keyframes.last().map_or(self.value, |k| k.value)
    }
    pub fn cropped(&self, offset: u64, duration: u64) -> Result<Self> {
        if self.opaque.is_some() {
            return Err(Error::unsupported("Opaque effects cannot be retimed"));
        }
        let mut out = self.clone();
        if !self.keyframes.is_empty() {
            let end = offset
                .checked_add(duration)
                .ok_or_else(|| Error::invalid("Effect time overflow"))?;
            let interpolation = self
                .keyframes
                .iter()
                .rev()
                .find(|k| k.frame <= offset)
                .map_or(Interpolation::Linear, |k| k.interpolation);
            out.keyframes = vec![Keyframe {
                frame: 0,
                value: self.at(offset),
                interpolation,
            }];
            for k in &self.keyframes {
                if k.frame > offset && k.frame < end {
                    let mut k = k.clone();
                    k.frame -= offset;
                    out.keyframes.push(k);
                }
            }
            if duration > 1 && out.keyframes.last().is_none_or(|k| k.frame != duration - 1) {
                out.keyframes.push(Keyframe {
                    frame: duration - 1,
                    value: self.at(end - 1),
                    interpolation: Interpolation::Linear,
                });
            }
        }
        out.value = out.keyframes.first().map_or(out.value, |k| k.value);
        Ok(out)
    }
}
pub fn validate_effect_value(service: &str, property: &str, value: i64) -> Result<()> {
    match (service, property) {
        ("volume", "level") if (-60000..=24000).contains(&value) => Ok(()),
        ("brightness", "level") if (0..=2000).contains(&value) => Ok(()),
        _ => Err(Error::unsupported(
            "Effect/property/value is outside the curated numeric domain",
        )),
    }
}
pub fn decimal(value: i64) -> String {
    let sign = if value < 0 { "-" } else { "" };
    let n = value.unsigned_abs();
    format!("{sign}{}.{:03}", n / 1000, n % 1000)
}
pub fn parse_decimal(text: &str) -> Result<i64> {
    if text.len() > 24 {
        return Err(Error::invalid("Effect value token too long"));
    }
    let text = text.strip_suffix("dB").unwrap_or(text);
    let neg = text.starts_with('-');
    let body = text.strip_prefix('-').unwrap_or(text);
    let (a, b) = body.split_once('.').unwrap_or((body, ""));
    if a.is_empty() || b.len() > 3 || !a.bytes().chain(b.bytes()).all(|c| c.is_ascii_digit()) {
        return Err(Error::invalid(
            "Expected fixed point with <=3 decimal digits",
        ));
    }
    let whole = a
        .parse::<i64>()
        .map_err(|_| Error::invalid("Effect value overflow"))?;
    let frac = if b.is_empty() {
        0
    } else {
        b.parse::<i64>()
            .map_err(|_| Error::invalid("Effect fraction"))?
            * 10i64.pow(3 - b.len() as u32)
    };
    let value = whole
        .checked_mul(1000)
        .and_then(|v| v.checked_add(frac))
        .ok_or_else(|| Error::invalid("Effect value overflow"))?;
    Ok(if neg { -value } else { value })
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XmlBinding {
    pub playlist: String,
    pub entry: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clip {
    pub id: String,
    pub name: String,
    pub asset: String,
    pub start: u64,
    pub source: FrameRange,
    pub effects: Vec<Effect>,
    pub binding: Option<XmlBinding>,
    pub speed: (u32, u32),
}
impl Clip {
    pub fn duration(&self) -> u64 {
        self.source.duration()
    }
    pub fn end(&self) -> Result<u64> {
        self.start
            .checked_add(self.duration())
            .filter(|n| *n <= MAX_FRAME)
            .ok_or_else(|| Error::invalid("Clip timeline end overflow"))
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gap {
    pub start: u64,
    pub duration: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Timeline {
    pub id: String,
    pub clips: Vec<Clip>,
}
impl Timeline {
    pub fn duration(&self) -> u64 {
        self.clips
            .iter()
            .filter_map(|c| c.end().ok())
            .max()
            .unwrap_or(0)
    }
    pub fn gaps(&self) -> Vec<Gap> {
        let mut cursor = 0;
        let mut out = vec![];
        for c in &self.clips {
            if c.start > cursor {
                out.push(Gap {
                    start: cursor,
                    duration: c.start - cursor,
                });
            }
            cursor = c.start + c.duration();
        }
        out
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Track {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub muted: bool,
    pub hidden: bool,
    pub lanes: Vec<Timeline>,
    pub effects: Vec<Effect>,
    pub opaque: bool,
}
impl Track {
    pub fn duration(&self) -> u64 {
        self.lanes.iter().map(Timeline::duration).max().unwrap_or(0)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transition {
    pub id: String,
    pub kind: String,
    pub a_track: String,
    pub b_track: String,
    pub range: FrameRange,
    pub reverse: bool,
    pub opaque: Option<Node>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Marker {
    pub id: String,
    pub frame: u64,
    pub label: String,
    pub tags: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubtitleReference {
    pub resource: String,
    pub representation: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sequence {
    pub id: String,
    pub name: String,
    pub tracks: Vec<Track>,
    pub transitions: Vec<Transition>,
    pub markers: Vec<Marker>,
    pub nested: Vec<String>,
    pub subtitles: Vec<SubtitleReference>,
    pub opaque: bool,
}
impl Sequence {
    pub fn duration(&self) -> u64 {
        self.tracks.iter().map(Track::duration).max().unwrap_or(0)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Project {
    pub id: String,
    pub format: Format,
    pub profile: Profile,
    pub assets: BTreeMap<String, MediaAsset>,
    pub sequences: Vec<Sequence>,
    pub original: Option<Node>,
    pub format_version: Option<String>,
    pub warnings: Vec<String>,
    pub source_root: Option<String>,
    pub source_dir: String,
    pub generated: bool,
}
impl Project {
    pub fn new(profile: Profile) -> Result<Self> {
        profile.validate()?;
        Ok(Self {
            id: "project".into(),
            format: Format::GenericMlt,
            profile,
            assets: BTreeMap::new(),
            sequences: vec![Sequence {
                id: "sequence0".into(),
                name: "Main".into(),
                tracks: vec![],
                transitions: vec![],
                markers: vec![],
                nested: vec![],
                subtitles: vec![],
                opaque: false,
            }],
            original: None,
            format_version: Some("semwright-normal-form-1".into()),
            warnings: vec![],
            source_root: None,
            source_dir: String::new(),
            generated: true,
        })
    }
    pub fn sequence(&self, id: &str) -> Result<&Sequence> {
        self.sequences
            .iter()
            .find(|s| s.id == id)
            .ok_or_else(|| Error::new("NotFound", "Sequence not found"))
    }
    pub fn sequence_mut(&mut self, id: &str) -> Result<&mut Sequence> {
        self.sequences
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or_else(|| Error::new("NotFound", "Sequence not found"))
    }
    pub fn track(&self, sequence: &str, id: &str) -> Result<&Track> {
        self.sequence(sequence)?
            .tracks
            .iter()
            .find(|t| t.id == id)
            .ok_or_else(|| Error::new("NotFound", "Track not found"))
    }
    pub fn track_mut(&mut self, sequence: &str, id: &str) -> Result<&mut Track> {
        self.sequence_mut(sequence)?
            .tracks
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or_else(|| Error::new("NotFound", "Track not found"))
    }
    pub fn clip(&self, sequence: &str, id: &str) -> Result<&Clip> {
        self.sequence(sequence)?
            .tracks
            .iter()
            .flat_map(|t| &t.lanes)
            .flat_map(|l| &l.clips)
            .find(|c| c.id == id)
            .ok_or_else(|| Error::new("NotFound", "Clip not found"))
    }
    pub fn clip_mut(&mut self, sequence: &str, id: &str) -> Result<&mut Clip> {
        self.sequence_mut(sequence)?
            .tracks
            .iter_mut()
            .flat_map(|t| &mut t.lanes)
            .flat_map(|l| &mut l.clips)
            .find(|c| c.id == id)
            .ok_or_else(|| Error::new("NotFound", "Clip not found"))
    }
    pub fn validate(&self) -> Result<()> {
        self.profile.validate()?;
        if self.assets.len() > MAX_ASSETS || self.sequences.len() > MAX_SEQUENCES {
            return Err(Error::limit("Project collection budget exceeded"));
        }
        let mut ids = BTreeSet::new();
        let mut clips = 0;
        let mut effects = 0;
        let mut tracks = 0;
        fn unique(ids: &mut BTreeSet<String>, id: &str) -> Result<()> {
            if id.is_empty() || id.len() > 256 || !ids.insert(id.to_string()) {
                return Err(Error::invalid("Invalid or reused semantic identity"));
            }
            Ok(())
        }
        for a in self.assets.values() {
            unique(&mut ids, &a.id)?;
            if a.name.len() > 65536 || a.frames.is_some_and(|f| f > MAX_FRAME) {
                return Err(Error::limit("Asset budget exceeded"));
            }
        }
        for s in &self.sequences {
            unique(&mut ids, &s.id)?;
            if s.markers.len() > 10000 {
                return Err(Error::limit("Marker limit"));
            }
            for t in &s.tracks {
                tracks += 1;
                unique(&mut ids, &t.id)?;
                if t.lanes.len() > 8 {
                    return Err(Error::limit("Lane limit"));
                }
                for l in &t.lanes {
                    unique(&mut ids, &l.id)?;
                    let mut end = 0;
                    for c in &l.clips {
                        clips += 1;
                        unique(&mut ids, &c.id)?;
                        FrameRange::new(c.source.start.0, c.source.end.0)?;
                        if c.start < end {
                            return Err(Error::invalid("Same-lane overlaps are forbidden"));
                        }
                        end = c.end()?;
                        let a = self
                            .assets
                            .get(&c.asset)
                            .ok_or_else(|| Error::invalid("Clip asset missing"))?;
                        if a.frames.is_some_and(|f| c.source.end.0 > f) {
                            return Err(Error::invalid("Clip exceeds source duration"));
                        }
                        if c.speed != (1, 1) {
                            return Err(Error::unsupported("Non-unit speed is inspection-only"));
                        }
                        for e in &c.effects {
                            effects += 1;
                            unique(&mut ids, &e.id)?;
                            e.validate(c.duration())?;
                        }
                    }
                }
                for e in &t.effects {
                    effects += 1;
                    unique(&mut ids, &e.id)?;
                    e.validate(t.duration().max(1))?;
                }
            }
            for m in &s.markers {
                unique(&mut ids, &m.id)?;
                if m.frame > MAX_FRAME || m.label.len() > 4096 || m.tags.len() > 16 {
                    return Err(Error::limit("Marker field exceeds budget"));
                }
            }
            for tr in &s.transitions {
                unique(&mut ids, &tr.id)?;
                FrameRange::new(tr.range.start.0, tr.range.end.0)?;
                if tr.opaque.is_some() {
                    continue;
                }
                if tr.a_track == tr.b_track || !matches!(tr.kind.as_str(), "dissolve" | "audio_mix")
                {
                    return Err(Error::invalid("Invalid curated transition"));
                }
                for id in [&tr.a_track, &tr.b_track] {
                    let t = s
                        .tracks
                        .iter()
                        .find(|t| &t.id == id)
                        .ok_or_else(|| Error::invalid("Transition track absent"))?;
                    if !t.lanes.iter().flat_map(|l| &l.clips).any(|c| {
                        c.start <= tr.range.start.0
                            && c.end().is_ok_and(|end| end >= tr.range.end.0)
                    }) {
                        return Err(Error::invalid(
                            "Transition needs both sources throughout overlap",
                        ));
                    }
                }
            }
        }
        if tracks > MAX_TRACKS || clips > MAX_CLIPS || effects > MAX_EFFECTS {
            return Err(Error::limit("Timeline resource budget exceeded"));
        }
        Ok(())
    }
    /// Full domain digest excludes formatting only. Unknown XML remains separately fingerprinted.
    pub fn semantic_json(&self) -> Value {
        fn effects(es: &[Effect]) -> Value {
            array(es.iter().map(|e| {
                let keys = array(e.keyframes.iter().map(|k| {
                    obj([
                        ("frame", k.frame.into()),
                        ("value", k.value.into()),
                        ("interpolation", k.interpolation.name().into()),
                    ])
                }));
                let opaque = e
                    .opaque
                    .as_ref()
                    .and_then(|n| crate::xml::serialize(n).ok())
                    .map_or(Value::Null, Into::into);
                obj([
                    ("id", e.id.clone().into()),
                    ("service", e.service.clone().into()),
                    ("property", e.property.clone().into()),
                    ("value", e.value.into()),
                    ("enabled", e.enabled.into()),
                    ("keyframes", keys),
                    ("opaque", opaque),
                ])
            }))
        }
        let assets = array(self.assets.values().map(|a| {
            obj([
                ("id", a.id.clone().into()),
                ("name", a.name.clone().into()),
                ("resource", a.resource.text().into()),
                ("service", a.service.clone().into()),
                ("frames", a.frames.map_or(Value::Null, Into::into)),
            ])
        }));
        let mut sequences = vec![];
        for s in &self.sequences {
            let mut tracks = vec![];
            for t in &s.tracks {
                let mut lanes = vec![];
                for l in &t.lanes {
                    let clips = array(l.clips.iter().map(|c| {
                        obj([
                            ("id", c.id.clone().into()),
                            ("name", c.name.clone().into()),
                            ("asset", c.asset.clone().into()),
                            ("start", c.start.into()),
                            ("source_in", c.source.start.0.into()),
                            ("source_out", c.source.end.0.into()),
                            ("effects", effects(&c.effects)),
                        ])
                    }));
                    lanes.push(obj([("id", l.id.clone().into()), ("clips", clips)]));
                }
                tracks.push(obj([
                    ("id", t.id.clone().into()),
                    ("name", t.name.clone().into()),
                    ("kind", t.kind.clone().into()),
                    ("muted", t.muted.into()),
                    ("hidden", t.hidden.into()),
                    ("effects", effects(&t.effects)),
                    ("lanes", array(lanes)),
                ]));
            }
            let transitions = array(s.transitions.iter().map(|t| {
                obj([
                    ("id", t.id.clone().into()),
                    ("kind", t.kind.clone().into()),
                    ("a_track", t.a_track.clone().into()),
                    ("b_track", t.b_track.clone().into()),
                    ("start", t.range.start.0.into()),
                    ("end", t.range.end.0.into()),
                    ("reverse", t.reverse.into()),
                ])
            }));
            let markers = array(s.markers.iter().map(|m| {
                obj([
                    ("id", m.id.clone().into()),
                    ("frame", m.frame.into()),
                    ("label", m.label.clone().into()),
                    ("tags", array(m.tags.iter().cloned().map(Into::into))),
                ])
            }));
            sequences.push(obj([
                ("id", s.id.clone().into()),
                ("name", s.name.clone().into()),
                ("tracks", array(tracks)),
                ("transitions", transitions),
                ("markers", markers),
                ("nested", array(s.nested.iter().cloned().map(Into::into))),
                ("opaque", s.opaque.into()),
            ]));
        }
        obj([
            ("profile", self.profile.json()),
            ("format", self.format.name().into()),
            ("assets", assets),
            ("sequences", array(sequences)),
        ])
    }
    pub fn semantic_digest(&self) -> String {
        sha256(self.semantic_json().encode().as_bytes())
    }
}
