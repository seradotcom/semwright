//! Backend-neutral semantic model for non-linear video editing.
//!
//! Native/application-specific round-trip data belongs to the backend envelope,
//! never to these types. Unknown native structures should project as read-only
//! semantic objects or remain solely in the backend envelope.

use crate::{
    Error, Result,
    hash::sha256,
    time::{FrameRange, FrameRate, MAX_FRAME},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const MODEL_VERSION: u32 = 1;
pub const MAX_TRACKS: usize = 128;
pub const MAX_CLIPS: usize = 10_000;
pub const MAX_EFFECTS: usize = 4_000;
pub const MAX_ASSETS: usize = 10_000;
pub const MAX_SEQUENCES: usize = 32;
pub const MAX_KEYFRAMES: usize = 1024;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Editability {
    #[default]
    Editable,
    ReadOnly {
        reason: String,
    },
}

impl Editability {
    pub fn read_only(reason: impl Into<String>) -> Self {
        Self::ReadOnly {
            reason: reason.into(),
        }
    }

    pub fn is_read_only(&self) -> bool {
        matches!(self, Self::ReadOnly { .. })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
            || self.width > 16_384
            || self.height > 16_384
            || self.audio_channels == 0
            || self.audio_channels > 64
            || self.sample_aspect.0 == 0
            || self.sample_aspect.1 == 0
            || self.display_aspect.0 == 0
            || self.display_aspect.1 == 0
        {
            return Err(Error::invalid("Invalid or excessive project profile"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
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
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaAsset {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub resource: Resource,
    pub frames: Option<u64>,
    #[serde(default)]
    pub editability: Editability,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Interpolation {
    Linear,
    Hold,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Keyframe {
    pub frame: u64,
    pub value: i64,
    pub interpolation: Interpolation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Effect {
    pub id: String,
    /// Stable semantic effect identifier, for example volume or brightness.
    pub kind: String,
    /// Stable semantic parameter identifier, for example level.
    pub parameter: String,
    pub value: i64,
    pub enabled: bool,
    pub keyframes: Vec<Keyframe>,
    #[serde(default)]
    pub editability: Editability,
}

impl Effect {
    pub fn validate(&self, duration: u64) -> Result<()> {
        if self.keyframes.len() > MAX_KEYFRAMES {
            return Err(Error::limit("Keyframe budget exceeded"));
        }
        if self.editability.is_read_only() {
            return Ok(());
        }
        validate_effect_value(&self.kind, &self.parameter, self.value)?;
        let mut prev = None;
        for keyframe in &self.keyframes {
            if keyframe.frame >= duration || prev.is_some_and(|p| p >= keyframe.frame) {
                return Err(Error::invalid(
                    "Keyframes must be ordered, unique and inside target",
                ));
            }
            validate_effect_value(&self.kind, &self.parameter, keyframe.value)?;
            prev = Some(keyframe.frame);
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
        self.keyframes
            .last()
            .map_or(self.value, |keyframe| keyframe.value)
    }

    pub fn cropped(&self, offset: u64, duration: u64) -> Result<Self> {
        if self.editability.is_read_only() {
            return Err(Error::unsupported("Read-only effects cannot be retimed"));
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
                .find(|keyframe| keyframe.frame <= offset)
                .map_or(Interpolation::Linear, |keyframe| keyframe.interpolation);
            out.keyframes = vec![Keyframe {
                frame: 0,
                value: self.at(offset),
                interpolation,
            }];
            for keyframe in &self.keyframes {
                if keyframe.frame > offset && keyframe.frame < end {
                    let mut keyframe = keyframe.clone();
                    keyframe.frame -= offset;
                    out.keyframes.push(keyframe);
                }
            }
            if duration > 1
                && out
                    .keyframes
                    .last()
                    .is_none_or(|keyframe| keyframe.frame != duration - 1)
            {
                out.keyframes.push(Keyframe {
                    frame: duration - 1,
                    value: self.at(end - 1),
                    interpolation: Interpolation::Linear,
                });
            }
        }
        out.value = out
            .keyframes
            .first()
            .map_or(out.value, |keyframe| keyframe.value);
        Ok(out)
    }
}

pub fn validate_effect_value(kind: &str, parameter: &str, value: i64) -> Result<()> {
    match (kind, parameter) {
        ("volume", "level") if (-60_000..=24_000).contains(&value) => Ok(()),
        ("brightness", "level") if (0..=2_000).contains(&value) => Ok(()),
        _ => Err(Error::unsupported(
            "Effect/parameter/value is outside the curated semantic domain",
        )),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Clip {
    pub id: String,
    pub name: String,
    pub asset: String,
    pub start: u64,
    pub source: FrameRange,
    pub effects: Vec<Effect>,
    pub speed: (u32, u32),
}

impl Clip {
    pub fn duration(&self) -> u64 {
        self.source.duration()
    }

    pub fn end(&self) -> Result<u64> {
        self.start
            .checked_add(self.duration())
            .filter(|end| *end <= MAX_FRAME)
            .ok_or_else(|| Error::invalid("Clip timeline end overflow"))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gap {
    pub start: u64,
    pub duration: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Timeline {
    pub id: String,
    pub clips: Vec<Clip>,
}

impl Timeline {
    pub fn duration(&self) -> u64 {
        self.clips
            .iter()
            .filter_map(|clip| clip.end().ok())
            .max()
            .unwrap_or(0)
    }

    pub fn gaps(&self) -> Vec<Gap> {
        let mut cursor = 0;
        let mut gaps = vec![];
        for clip in &self.clips {
            if clip.start > cursor {
                gaps.push(Gap {
                    start: cursor,
                    duration: clip.start - cursor,
                });
            }
            cursor = clip.start + clip.duration();
        }
        gaps
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Track {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub muted: bool,
    pub hidden: bool,
    pub lanes: Vec<Timeline>,
    pub effects: Vec<Effect>,
    #[serde(default)]
    pub editability: Editability,
}

impl Track {
    pub fn duration(&self) -> u64 {
        self.lanes.iter().map(Timeline::duration).max().unwrap_or(0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transition {
    pub id: String,
    pub kind: String,
    pub a_track: String,
    pub b_track: String,
    pub range: FrameRange,
    pub reverse: bool,
    #[serde(default)]
    pub editability: Editability,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marker {
    pub id: String,
    pub frame: u64,
    pub label: String,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubtitleReference {
    pub resource: String,
    pub representation: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sequence {
    pub id: String,
    pub name: String,
    pub tracks: Vec<Track>,
    pub transitions: Vec<Transition>,
    pub markers: Vec<Marker>,
    pub nested: Vec<String>,
    pub subtitles: Vec<SubtitleReference>,
    #[serde(default)]
    pub editability: Editability,
}

impl Sequence {
    pub fn duration(&self) -> u64 {
        self.tracks.iter().map(Track::duration).max().unwrap_or(0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub model_version: u32,
    pub id: String,
    pub profile: Profile,
    pub assets: BTreeMap<String, MediaAsset>,
    pub sequences: Vec<Sequence>,
    pub warnings: Vec<String>,
}

impl Project {
    pub fn new(profile: Profile) -> Result<Self> {
        profile.validate()?;
        Ok(Self {
            model_version: MODEL_VERSION,
            id: "project".into(),
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
                editability: Editability::Editable,
            }],
            warnings: vec![],
        })
    }

    pub fn sequence(&self, id: &str) -> Result<&Sequence> {
        self.sequences
            .iter()
            .find(|sequence| sequence.id == id)
            .ok_or_else(|| Error::new("NotFound", "Sequence not found"))
    }

    pub fn sequence_mut(&mut self, id: &str) -> Result<&mut Sequence> {
        self.sequences
            .iter_mut()
            .find(|sequence| sequence.id == id)
            .ok_or_else(|| Error::new("NotFound", "Sequence not found"))
    }

    pub fn track(&self, sequence: &str, id: &str) -> Result<&Track> {
        self.sequence(sequence)?
            .tracks
            .iter()
            .find(|track| track.id == id)
            .ok_or_else(|| Error::new("NotFound", "Track not found"))
    }

    pub fn track_mut(&mut self, sequence: &str, id: &str) -> Result<&mut Track> {
        self.sequence_mut(sequence)?
            .tracks
            .iter_mut()
            .find(|track| track.id == id)
            .ok_or_else(|| Error::new("NotFound", "Track not found"))
    }

    pub fn clip(&self, sequence: &str, id: &str) -> Result<&Clip> {
        self.sequence(sequence)?
            .tracks
            .iter()
            .flat_map(|track| &track.lanes)
            .flat_map(|lane| &lane.clips)
            .find(|clip| clip.id == id)
            .ok_or_else(|| Error::new("NotFound", "Clip not found"))
    }

    pub fn clip_mut(&mut self, sequence: &str, id: &str) -> Result<&mut Clip> {
        self.sequence_mut(sequence)?
            .tracks
            .iter_mut()
            .flat_map(|track| &mut track.lanes)
            .flat_map(|lane| &mut lane.clips)
            .find(|clip| clip.id == id)
            .ok_or_else(|| Error::new("NotFound", "Clip not found"))
    }

    pub fn duration(&self) -> u64 {
        self.sequences
            .iter()
            .map(Sequence::duration)
            .max()
            .unwrap_or(0)
    }

    pub fn validate(&self) -> Result<()> {
        if self.model_version != MODEL_VERSION {
            return Err(Error::unsupported(
                "Unsupported semantic video model version",
            ));
        }
        self.profile.validate()?;
        if self.assets.len() > MAX_ASSETS || self.sequences.len() > MAX_SEQUENCES {
            return Err(Error::limit("Project collection budget exceeded"));
        }

        let mut ids = BTreeSet::new();
        let mut clip_count = 0usize;
        let mut effect_count = 0usize;
        let mut track_count = 0usize;

        fn unique(ids: &mut BTreeSet<String>, id: &str) -> Result<()> {
            if id.is_empty() || id.len() > 256 || !ids.insert(id.to_owned()) {
                return Err(Error::invalid("Invalid or reused semantic identity"));
            }
            Ok(())
        }

        for asset in self.assets.values() {
            unique(&mut ids, &asset.id)?;
            if asset.name.len() > 65_536 || asset.frames.is_some_and(|frames| frames > MAX_FRAME) {
                return Err(Error::limit("Asset budget exceeded"));
            }
        }

        for sequence in &self.sequences {
            unique(&mut ids, &sequence.id)?;
            if sequence.markers.len() > 10_000 {
                return Err(Error::limit("Marker limit"));
            }
            for track in &sequence.tracks {
                track_count += 1;
                unique(&mut ids, &track.id)?;
                if track.lanes.len() > 8 {
                    return Err(Error::limit("Lane limit"));
                }
                for lane in &track.lanes {
                    unique(&mut ids, &lane.id)?;
                    let mut end = 0;
                    for clip in &lane.clips {
                        clip_count += 1;
                        unique(&mut ids, &clip.id)?;
                        FrameRange::new(clip.source.start.0, clip.source.end.0)?;
                        if clip.start < end {
                            return Err(Error::invalid("Same-lane overlaps are forbidden"));
                        }
                        end = clip.end()?;
                        let asset = self
                            .assets
                            .get(&clip.asset)
                            .ok_or_else(|| Error::invalid("Clip asset missing"))?;
                        if asset
                            .frames
                            .is_some_and(|frames| clip.source.end.0 > frames)
                        {
                            return Err(Error::invalid("Clip exceeds source duration"));
                        }
                        if clip.speed != (1, 1) {
                            return Err(Error::unsupported("Non-unit speed is inspection-only"));
                        }
                        for effect in &clip.effects {
                            effect_count += 1;
                            unique(&mut ids, &effect.id)?;
                            effect.validate(clip.duration())?;
                        }
                    }
                }
                for effect in &track.effects {
                    effect_count += 1;
                    unique(&mut ids, &effect.id)?;
                    effect.validate(track.duration().max(1))?;
                }
            }
            for marker in &sequence.markers {
                unique(&mut ids, &marker.id)?;
                if marker.frame > MAX_FRAME || marker.label.len() > 4096 || marker.tags.len() > 16 {
                    return Err(Error::limit("Marker field exceeds budget"));
                }
            }
            for transition in &sequence.transitions {
                unique(&mut ids, &transition.id)?;
                FrameRange::new(transition.range.start.0, transition.range.end.0)?;
                if transition.editability.is_read_only() {
                    continue;
                }
                if transition.a_track == transition.b_track
                    || !matches!(transition.kind.as_str(), "dissolve" | "audio_mix")
                {
                    return Err(Error::invalid("Invalid curated transition"));
                }
                for id in [&transition.a_track, &transition.b_track] {
                    let track = sequence
                        .tracks
                        .iter()
                        .find(|track| &track.id == id)
                        .ok_or_else(|| Error::invalid("Transition track absent"))?;
                    if !track.lanes.iter().flat_map(|lane| &lane.clips).any(|clip| {
                        clip.start <= transition.range.start.0
                            && clip.end().is_ok_and(|end| end >= transition.range.end.0)
                    }) {
                        return Err(Error::invalid(
                            "Transition needs both sources throughout overlap",
                        ));
                    }
                }
            }
        }

        if track_count > MAX_TRACKS || clip_count > MAX_CLIPS || effect_count > MAX_EFFECTS {
            return Err(Error::limit("Timeline resource budget exceeded"));
        }
        Ok(())
    }

    pub fn semantic_digest(&self) -> Result<String> {
        self.validate()?;
        let bytes = serde_json::to_vec(self)
            .map_err(|_| Error::new("BackendFailed", "Could not encode semantic video model"))?;
        Ok(sha256(&bytes))
    }
}
