//! Backend-neutral transactional semantic video edit engine.
//!
//! Backend policy, native revision checks, serialization and round-trip
//! validation intentionally live outside this module.

use crate::{Error, Result, hash::sha256, model::*, support::VideoOperation, time::FrameRange};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
pub enum Edit {
    Profile(Profile),
    SequenceCreate {
        name: String,
    },
    AssetImport {
        asset: MediaAsset,
    },
    AssetRelink {
        id: String,
        expected: String,
        resource: Resource,
    },
    TrackCreate {
        sequence: String,
        name: String,
        kind: String,
    },
    TrackRemove {
        sequence: String,
        track: String,
    },
    TrackRename {
        sequence: String,
        track: String,
        name: String,
    },
    TrackMute {
        sequence: String,
        track: String,
        value: bool,
    },
    TrackHide {
        sequence: String,
        track: String,
        value: bool,
    },
    TrackReorder {
        sequence: String,
        track: String,
        index: usize,
    },
    Insert {
        sequence: String,
        track: String,
        asset: String,
        start: u64,
        source: FrameRange,
        name: String,
        ripple: bool,
    },
    Move {
        sequence: String,
        clip: String,
        track: String,
        start: u64,
    },
    Trim {
        sequence: String,
        clip: String,
        source: FrameRange,
    },
    Split {
        sequence: String,
        clip: String,
        at: u64,
    },
    Remove {
        sequence: String,
        clip: String,
        ripple: bool,
    },
    Duplicate {
        sequence: String,
        clip: String,
        track: String,
        start: u64,
    },
    TransitionAdd {
        sequence: String,
        kind: String,
        a_track: String,
        b_track: String,
        range: FrameRange,
    },
    TransitionPatch {
        sequence: String,
        id: String,
        range: FrameRange,
        reverse: bool,
    },
    TransitionRemove {
        sequence: String,
        id: String,
    },
    EffectAdd {
        sequence: String,
        clip: String,
        kind: String,
        value: i64,
    },
    EffectPatch {
        sequence: String,
        clip: String,
        id: String,
        value: i64,
    },
    EffectRemove {
        sequence: String,
        clip: String,
        id: String,
    },
    EffectEnable {
        sequence: String,
        clip: String,
        id: String,
        value: bool,
    },
    KeyframeSet {
        sequence: String,
        clip: String,
        effect: String,
        keyframe: Keyframe,
    },
    KeyframeRemove {
        sequence: String,
        clip: String,
        effect: String,
        frame: u64,
    },
    MarkerAdd {
        sequence: String,
        frame: u64,
        label: String,
    },
    MarkerPatch {
        sequence: String,
        id: String,
        frame: u64,
        label: String,
    },
    MarkerRemove {
        sequence: String,
        id: String,
    },
    AudioVolume {
        sequence: String,
        clip: String,
        value: i64,
    },
    AudioFade {
        sequence: String,
        clip: String,
        frames: u64,
        fade_in: bool,
    },
}
impl Edit {
    pub fn operation(&self) -> VideoOperation {
        match self {
            Self::Profile(_) => VideoOperation::ProjectProfileSet,
            Self::SequenceCreate { .. } => VideoOperation::SequenceCreate,
            Self::AssetImport { .. } => VideoOperation::AssetImport,
            Self::AssetRelink { .. } => VideoOperation::AssetRelink,
            Self::TrackCreate { .. } => VideoOperation::TrackCreate,
            Self::TrackRemove { .. } => VideoOperation::TrackRemove,
            Self::TrackRename { .. } => VideoOperation::TrackRename,
            Self::TrackMute { .. } => VideoOperation::TrackMute,
            Self::TrackHide { .. } => VideoOperation::TrackHide,
            Self::TrackReorder { .. } => VideoOperation::TrackReorder,
            Self::Insert { .. } => VideoOperation::ClipInsert,
            Self::Move { .. } => VideoOperation::ClipMove,
            Self::Trim { .. } => VideoOperation::ClipTrim,
            Self::Split { .. } => VideoOperation::ClipSplit,
            Self::Remove { .. } => VideoOperation::ClipRemove,
            Self::Duplicate { .. } => VideoOperation::ClipDuplicate,
            Self::TransitionAdd { .. } => VideoOperation::TransitionAdd,
            Self::TransitionPatch { .. } => VideoOperation::TransitionPatch,
            Self::TransitionRemove { .. } => VideoOperation::TransitionRemove,
            Self::EffectAdd { .. } => VideoOperation::EffectAdd,
            Self::EffectPatch { .. } => VideoOperation::EffectPatch,
            Self::EffectRemove { .. } => VideoOperation::EffectRemove,
            Self::EffectEnable { value: true, .. } => VideoOperation::EffectEnable,
            Self::EffectEnable { .. } => VideoOperation::EffectDisable,
            Self::KeyframeSet { .. } => VideoOperation::KeyframeSet,
            Self::KeyframeRemove { .. } => VideoOperation::KeyframeRemove,
            Self::MarkerAdd { .. } => VideoOperation::MarkerAdd,
            Self::MarkerPatch { .. } => VideoOperation::MarkerPatch,
            Self::MarkerRemove { .. } => VideoOperation::MarkerRemove,
            Self::AudioVolume { .. } => VideoOperation::AudioVolumeSet,
            Self::AudioFade { fade_in: true, .. } => VideoOperation::AudioFadeIn,
            Self::AudioFade { .. } => VideoOperation::AudioFadeOut,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditOutcome {
    pub operation: String,
    pub affected: Vec<String>,
    pub created: Vec<String>,
    pub before_frames: u64,
    pub after_frames: u64,
    pub result: Project,
}

fn semantic_id(base: &str, seed: &str, prefix: &str) -> String {
    format!(
        "{prefix}_{}",
        &sha256(format!("{base}:{seed}:{prefix}").as_bytes())[..24]
    )
}

pub fn apply(project: &Project, edit: Edit, seed: &str) -> Result<EditOutcome> {
    let base = project.semantic_digest()?;
    apply_with_identity_base(project, edit, seed, &base)
}

/// Apply an edit while deriving any newly created semantic IDs from a caller
/// supplied revision/identity base. Backends use this to keep semantic IDs
/// stable with their native optimistic-concurrency revision.
pub fn apply_with_identity_base(
    project: &Project,
    edit: Edit,
    seed: &str,
    identity_base: &str,
) -> Result<EditOutcome> {
    project.validate()?;
    let base = identity_base;
    let operation = edit.operation();
    let mut result = project.clone();
    let mut created = vec![];
    let mut affected = vec![];
    let new = |prefix: &str| semantic_id(base, seed, prefix);
    match edit {
        Edit::Profile(profile) => {
            profile.validate()?;
            result.profile = profile;
            affected.push("profile".into());
        }
        Edit::SequenceCreate { name } => {
            let id = new("s");
            result.sequences.push(Sequence {
                id: id.clone(),
                name,
                tracks: vec![],
                transitions: vec![],
                markers: vec![],
                nested: vec![],
                subtitles: vec![],
                editability: Editability::Editable,
            });
            created.push(id);
        }
        Edit::AssetImport { mut asset } => {
            asset.id = new("a");
            created.push(asset.id.clone());
            result.assets.insert(asset.id.clone(), asset);
        }
        Edit::AssetRelink {
            id,
            expected,
            resource,
        } => {
            let a = result
                .assets
                .get_mut(&id)
                .ok_or_else(|| Error::new("NotFound", "Asset missing"))?;
            if a.editability.is_read_only() {
                return Err(Error::unsupported("Read-only asset cannot be relinked"));
            }
            if a.resource.text() != expected {
                return Err(Error::stale());
            }
            if matches!(a.resource, Resource::Color(_) | Resource::Opaque(_)) {
                return Err(Error::unsupported(
                    "Cannot relink generator or opaque resource",
                ));
            }
            a.resource = resource;
            affected.push(id);
        }
        Edit::TrackCreate {
            sequence,
            name,
            kind,
        } => {
            ensure_sequence_editable(&result, &sequence)?;
            if !matches!(kind.as_str(), "audio" | "video" | "av") {
                return Err(Error::invalid("Track kind"));
            }
            let id = new("t");
            let lane = new("lane");
            result.sequence_mut(&sequence)?.tracks.push(Track {
                id: id.clone(),
                name,
                kind,
                muted: false,
                hidden: false,
                lanes: vec![Timeline {
                    id: lane,
                    clips: vec![],
                }],
                effects: vec![],
                editability: Editability::Editable,
            });
            created.push(id);
        }
        Edit::TrackRemove { sequence, track } => {
            ensure_track_editable(&result, &sequence, &track)?;
            let seq = result.sequence_mut(&sequence)?;
            if seq
                .transitions
                .iter()
                .any(|t| t.a_track == track || t.b_track == track)
            {
                return Err(Error::invalid("Remove transitions referencing track first"));
            }
            let i = seq
                .tracks
                .iter()
                .position(|t| t.id == track)
                .ok_or_else(|| Error::new("NotFound", "Track missing"))?;
            seq.tracks.remove(i);
            affected.push(track);
        }
        Edit::TrackRename {
            sequence,
            track,
            name,
        } => {
            ensure_track_editable(&result, &sequence, &track)?;
            result.track_mut(&sequence, &track)?.name = name;
            affected.push(track);
        }
        Edit::TrackMute {
            sequence,
            track,
            value,
        } => {
            ensure_track_editable(&result, &sequence, &track)?;
            result.track_mut(&sequence, &track)?.muted = value;
            affected.push(track);
        }
        Edit::TrackHide {
            sequence,
            track,
            value,
        } => {
            ensure_track_editable(&result, &sequence, &track)?;
            result.track_mut(&sequence, &track)?.hidden = value;
            affected.push(track);
        }
        Edit::TrackReorder {
            sequence,
            track,
            index,
        } => {
            ensure_track_editable(&result, &sequence, &track)?;
            let seq = result.sequence_mut(&sequence)?;
            if index >= seq.tracks.len() {
                return Err(Error::invalid("Track insertion index out of range"));
            }
            let i = seq
                .tracks
                .iter()
                .position(|t| t.id == track)
                .ok_or_else(|| Error::new("NotFound", "Track missing"))?;
            let t = seq.tracks.remove(i);
            seq.tracks.insert(index, t);
            affected.push(track);
        }
        Edit::Insert {
            sequence,
            track,
            asset,
            start,
            source,
            name,
            ripple,
        } => {
            let id = new("c");
            let clip = Clip {
                id: id.clone(),
                name,
                asset,
                start,
                source,
                effects: vec![],
                speed: (1, 1),
            };
            insert(&mut result, &sequence, &track, clip, ripple)?;
            created.push(id);
            affected.push(track);
        }
        Edit::Move {
            sequence,
            clip,
            track,
            start,
        } => {
            let mut c = take_clip(&mut result, &sequence, &clip, false)?;
            c.start = start;
            insert(&mut result, &sequence, &track, c, false)?;
            affected.extend([clip, track]);
        }
        Edit::Trim {
            sequence,
            clip,
            source,
        } => {
            ensure_clip_editable(&result, &sequence, &clip)?;
            let c = result.clip_mut(&sequence, &clip)?;
            if !c.effects.is_empty() {
                if source.start.0 < c.source.start.0 || source.end.0 > c.source.end.0 {
                    return Err(Error::unsupported(
                        "Extending effect-bearing clip handles is not implemented",
                    ));
                }
                let offset = source.start.0 - c.source.start.0;
                c.effects = c
                    .effects
                    .iter()
                    .map(|e| e.cropped(offset, source.duration()))
                    .collect::<Result<_>>()?;
            }
            c.source = source;
            affected.push(clip);
        }
        Edit::Split { sequence, clip, at } => {
            ensure_clip_editable(&result, &sequence, &clip)?;
            let original = result.clip(&sequence, &clip)?.clone();
            if at <= original.start || at >= original.end()? {
                return Err(Error::invalid(
                    "Split must be strictly inside the clip on timeline",
                ));
            }
            let left_duration = at - original.start;
            let (left, right) = original
                .source
                .split(original.source.start.0 + left_duration)?;
            let mut right_clip = original.clone();
            let right_id = new("c");
            right_clip.id = right_id.clone();
            right_clip.start = at;
            right_clip.source = right;
            right_clip.effects = original
                .effects
                .iter()
                .map(|e| {
                    let mut e = e.cropped(left_duration, right.duration())?;
                    e.id = format!(
                        "ef_{}",
                        &sha256(format!("{right_id}:{}", e.id).as_bytes())[..24]
                    );
                    Ok(e)
                })
                .collect::<Result<_>>()?;
            let c = result.clip_mut(&sequence, &clip)?;
            c.source = left;
            c.effects = original
                .effects
                .iter()
                .map(|e| e.cropped(0, left.duration()))
                .collect::<Result<_>>()?;
            let seq = result.sequence_mut(&sequence)?;
            let lane = seq
                .tracks
                .iter_mut()
                .flat_map(|t| &mut t.lanes)
                .find(|l| l.clips.iter().any(|c| c.id == clip))
                .ok_or_else(|| Error::new("NotFound", "Split lane missing"))?;
            lane.clips.push(right_clip);
            lane.clips.sort_by_key(|c| c.start);
            affected.push(clip);
            created.push(right_id);
        }
        Edit::Remove {
            sequence,
            clip,
            ripple,
        } => {
            take_clip(&mut result, &sequence, &clip, ripple)?;
            affected.push(clip);
        }
        Edit::Duplicate {
            sequence,
            clip,
            track,
            start,
        } => {
            ensure_clip_editable(&result, &sequence, &clip)?;
            let mut c = result.clip(&sequence, &clip)?.clone();
            let id = new("c");
            c.id = id.clone();
            c.start = start;
            for e in &mut c.effects {
                e.id = format!("ef_{}", &sha256(format!("{id}:{}", e.id).as_bytes())[..24]);
            }
            insert(&mut result, &sequence, &track, c, false)?;
            created.push(id);
            affected.push(track);
        }
        Edit::TransitionAdd {
            sequence,
            kind,
            a_track,
            b_track,
            range,
        } => {
            ensure_sequence_editable(&result, &sequence)?;
            let id = new("tr");
            result
                .sequence_mut(&sequence)?
                .transitions
                .push(Transition {
                    id: id.clone(),
                    kind,
                    a_track,
                    b_track,
                    range,
                    reverse: false,
                    editability: Editability::Editable,
                });
            created.push(id);
        }
        Edit::TransitionPatch {
            sequence,
            id,
            range,
            reverse,
        } => {
            ensure_sequence_editable(&result, &sequence)?;
            let t = result
                .sequence_mut(&sequence)?
                .transitions
                .iter_mut()
                .find(|t| t.id == id)
                .ok_or_else(|| Error::new("NotFound", "Transition missing"))?;
            if t.editability.is_read_only() {
                return Err(Error::unsupported("Opaque transition"));
            }
            t.range = range;
            t.reverse = reverse;
            affected.push(id);
        }
        Edit::TransitionRemove { sequence, id } => {
            ensure_sequence_editable(&result, &sequence)?;
            let seq = result.sequence_mut(&sequence)?;
            let i = seq
                .transitions
                .iter()
                .position(|t| t.id == id)
                .ok_or_else(|| Error::new("NotFound", "Transition missing"))?;
            if seq.transitions[i].editability.is_read_only() {
                return Err(Error::unsupported("Read-only transition cannot be removed"));
            }
            seq.transitions.remove(i);
            affected.push(id);
        }
        Edit::EffectAdd {
            sequence,
            clip,
            kind,
            value,
        } => {
            ensure_clip_editable(&result, &sequence, &clip)?;
            let id = new("ef");
            validate_effect_value(&kind, "level", value)?;
            result.clip_mut(&sequence, &clip)?.effects.push(Effect {
                id: id.clone(),
                kind,
                parameter: "level".into(),
                value,
                enabled: true,
                keyframes: vec![],
                editability: Editability::Editable,
            });
            created.push(id);
            affected.push(clip);
        }
        Edit::EffectPatch {
            sequence,
            clip,
            id,
            value,
        } => {
            let e = effect_mut(&mut result, &sequence, &clip, &id)?;
            validate_effect_value(&e.kind, &e.parameter, value)?;
            if !e.keyframes.is_empty() {
                return Err(Error::invalid(
                    "Constant patch cannot erase animation; remove keyframes explicitly",
                ));
            }
            e.value = value;
            affected.push(id);
        }
        Edit::EffectRemove { sequence, clip, id } => {
            ensure_clip_editable(&result, &sequence, &clip)?;
            let c = result.clip_mut(&sequence, &clip)?;
            let i = c
                .effects
                .iter()
                .position(|e| e.id == id)
                .ok_or_else(|| Error::new("NotFound", "Effect missing"))?;
            c.effects.remove(i);
            affected.push(id);
        }
        Edit::EffectEnable {
            sequence,
            clip,
            id,
            value,
        } => {
            effect_mut(&mut result, &sequence, &clip, &id)?.enabled = value;
            affected.push(id);
        }
        Edit::KeyframeSet {
            sequence,
            clip,
            effect,
            keyframe,
        } => {
            let e = effect_mut(&mut result, &sequence, &clip, &effect)?;
            validate_effect_value(&e.kind, &e.parameter, keyframe.value)?;
            if let Some(k) = e.keyframes.iter_mut().find(|k| k.frame == keyframe.frame) {
                *k = keyframe;
            } else {
                e.keyframes.push(keyframe);
            }
            e.keyframes.sort_by_key(|k| k.frame);
            e.value = e.keyframes.first().map_or(e.value, |k| k.value);
            affected.push(effect);
        }
        Edit::KeyframeRemove {
            sequence,
            clip,
            effect,
            frame,
        } => {
            let e = effect_mut(&mut result, &sequence, &clip, &effect)?;
            let i = e
                .keyframes
                .iter()
                .position(|k| k.frame == frame)
                .ok_or_else(|| Error::new("NotFound", "Keyframe absent"))?;
            e.keyframes.remove(i);
            e.value = e.keyframes.first().map_or(e.value, |k| k.value);
            affected.push(effect);
        }
        Edit::MarkerAdd {
            sequence,
            frame,
            label,
        } => {
            ensure_sequence_editable(&result, &sequence)?;
            let id = new("m");
            result.sequence_mut(&sequence)?.markers.push(Marker {
                id: id.clone(),
                frame,
                label,
                tags: vec![],
            });
            created.push(id);
        }
        Edit::MarkerPatch {
            sequence,
            id,
            frame,
            label,
        } => {
            ensure_sequence_editable(&result, &sequence)?;
            let m = result
                .sequence_mut(&sequence)?
                .markers
                .iter_mut()
                .find(|m| m.id == id)
                .ok_or_else(|| Error::new("NotFound", "Marker absent"))?;
            m.frame = frame;
            m.label = label;
            affected.push(id);
        }
        Edit::MarkerRemove { sequence, id } => {
            ensure_sequence_editable(&result, &sequence)?;
            let seq = result.sequence_mut(&sequence)?;
            let i = seq
                .markers
                .iter()
                .position(|m| m.id == id)
                .ok_or_else(|| Error::new("NotFound", "Marker absent"))?;
            seq.markers.remove(i);
            affected.push(id);
        }
        Edit::AudioVolume {
            sequence,
            clip,
            value,
        } => {
            ensure_clip_editable(&result, &sequence, &clip)?;
            validate_effect_value("volume", "level", value)?;
            let c = result.clip_mut(&sequence, &clip)?;
            let existing: Vec<_> = c
                .effects
                .iter()
                .enumerate()
                .filter(|(_, e)| e.kind == "volume")
                .map(|(i, _)| i)
                .collect();
            if existing.len() > 1 {
                return Err(Error::new(
                    "AmbiguousTarget",
                    "Multiple volume effects; use explicit effect ref",
                ));
            }
            if let Some(i) = existing.first() {
                let e = &mut c.effects[*i];
                if e.editability.is_read_only() || !e.keyframes.is_empty() {
                    return Err(Error::unsupported(
                        "Volume automation requires explicit keyframe edits",
                    ));
                }
                e.value = value;
                affected.push(e.id.clone());
            } else {
                let id = new("ef");
                c.effects.push(Effect {
                    id: id.clone(),
                    kind: "volume".into(),
                    parameter: "level".into(),
                    value,
                    enabled: true,
                    keyframes: vec![],
                    editability: Editability::Editable,
                });
                created.push(id);
            }
            affected.push(clip);
        }
        Edit::AudioFade {
            sequence,
            clip,
            frames,
            fade_in,
        } => {
            ensure_clip_editable(&result, &sequence, &clip)?;
            let c = result.clip_mut(&sequence, &clip)?;
            if frames < 2 || frames > c.duration() {
                return Err(Error::invalid("Fade length must be 2..clip duration"));
            }
            let id = new("ef");
            let (a, b) = if fade_in {
                (
                    Keyframe {
                        frame: 0,
                        value: -60000,
                        interpolation: Interpolation::Linear,
                    },
                    Keyframe {
                        frame: frames - 1,
                        value: 0,
                        interpolation: Interpolation::Linear,
                    },
                )
            } else {
                (
                    Keyframe {
                        frame: c.duration() - frames,
                        value: 0,
                        interpolation: Interpolation::Linear,
                    },
                    Keyframe {
                        frame: c.duration() - 1,
                        value: -60000,
                        interpolation: Interpolation::Linear,
                    },
                )
            };
            c.effects.push(Effect {
                id: id.clone(),
                kind: "volume".into(),
                parameter: "level".into(),
                value: a.value,
                enabled: true,
                keyframes: vec![a, b],
                editability: Editability::Editable,
            });
            created.push(id);
            affected.push(clip);
        }
    }
    result.validate()?;
    Ok(EditOutcome {
        operation: operation.as_str().into(),
        affected,
        created,
        before_frames: project.duration(),
        after_frames: result.duration(),
        result,
    })
}

fn ensure_sequence_editable(project: &Project, sequence: &str) -> Result<()> {
    if project.sequence(sequence)?.editability.is_read_only() {
        return Err(Error::unsupported("Read-only sequence cannot be mutated"));
    }
    Ok(())
}

fn ensure_track_editable(project: &Project, sequence: &str, track: &str) -> Result<()> {
    ensure_sequence_editable(project, sequence)?;
    if project.track(sequence, track)?.editability.is_read_only() {
        return Err(Error::unsupported("Read-only track cannot be mutated"));
    }
    Ok(())
}

fn ensure_clip_editable(project: &Project, sequence: &str, clip: &str) -> Result<()> {
    ensure_sequence_editable(project, sequence)?;
    let sequence = project.sequence(sequence)?;
    let track = sequence
        .tracks
        .iter()
        .find(|track| {
            track
                .lanes
                .iter()
                .flat_map(|lane| &lane.clips)
                .any(|candidate| candidate.id == clip)
        })
        .ok_or_else(|| Error::new("NotFound", "Clip missing"))?;
    if track.editability.is_read_only() {
        return Err(Error::unsupported(
            "Clip belongs to a read-only track and cannot be mutated",
        ));
    }
    Ok(())
}

fn effect_mut<'a>(
    p: &'a mut Project,
    sequence: &str,
    clip: &str,
    id: &str,
) -> Result<&'a mut Effect> {
    ensure_clip_editable(p, sequence, clip)?;
    let e = p
        .clip_mut(sequence, clip)?
        .effects
        .iter_mut()
        .find(|e| e.id == id)
        .ok_or_else(|| Error::new("NotFound", "Effect missing"))?;
    if e.editability.is_read_only() {
        return Err(Error::unsupported(
            "Opaque effect is preserved, not editable",
        ));
    }
    Ok(e)
}
fn insert(p: &mut Project, sequence: &str, track: &str, clip: Clip, ripple: bool) -> Result<()> {
    if ripple && !p.sequence(sequence)?.transitions.is_empty() {
        return Err(Error::unsupported(
            "Ripple with transitions requires an explicit coordinated plan",
        ));
    }
    let t = p.track_mut(sequence, track)?;
    if t.editability.is_read_only() || t.lanes.len() != 1 {
        return Err(Error::unsupported(
            "Insert requires an editable single-lane track",
        ));
    }
    let lane = &mut t.lanes[0];
    let duration = clip.duration();
    if ripple {
        if lane
            .clips
            .iter()
            .any(|c| c.start < clip.start && c.end().is_ok_and(|end| end > clip.start))
        {
            return Err(Error::invalid(
                "Ripple insert cannot split an existing clip implicitly",
            ));
        }
        for c in &mut lane.clips {
            if c.start >= clip.start {
                c.start = c
                    .start
                    .checked_add(duration)
                    .ok_or_else(|| Error::invalid("Ripple overflow"))?;
            }
        }
    }
    lane.clips.push(clip);
    lane.clips.sort_by_key(|c| c.start);
    Ok(())
}
fn take_clip(p: &mut Project, sequence: &str, id: &str, ripple: bool) -> Result<Clip> {
    ensure_clip_editable(p, sequence, id)?;
    let seq = p.sequence_mut(sequence)?;
    for t in &mut seq.tracks {
        for l in &mut t.lanes {
            if let Some(i) = l.clips.iter().position(|c| c.id == id) {
                if ripple && !seq.transitions.is_empty() {
                    return Err(Error::unsupported(
                        "Ripple with transitions requires an explicit coordinated plan",
                    ));
                }
                let c = l.clips.remove(i);
                if ripple {
                    let end = c.end()?;
                    for other in &mut l.clips {
                        if other.start >= end {
                            other.start -= c.duration();
                        }
                    }
                }
                return Ok(c);
            }
        }
    }
    Err(Error::new("NotFound", "Clip missing"))
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectDiff {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub changed: Vec<String>,
    pub profile_changed: bool,
    pub before_frames: u64,
    pub after_frames: u64,
}

pub fn diff(before: &Project, after: &Project) -> ProjectDiff {
    let clips = |project: &Project| -> std::collections::BTreeMap<String, (String, u64, u64, u64)> {
        project
            .sequences
            .iter()
            .flat_map(|sequence| {
                sequence.tracks.iter().flat_map(|track| {
                    track.lanes.iter().flat_map(move |lane| {
                        lane.clips.iter().map(move |clip| {
                            (
                                clip.id.clone(),
                                (
                                    track.id.clone(),
                                    clip.start,
                                    clip.source.start.0,
                                    clip.source.end.0,
                                ),
                            )
                        })
                    })
                })
            })
            .collect()
    };
    let a = clips(before);
    let b = clips(after);
    ProjectDiff {
        added: b
            .keys()
            .filter(|key| !a.contains_key(*key))
            .cloned()
            .collect(),
        removed: a
            .keys()
            .filter(|key| !b.contains_key(*key))
            .cloned()
            .collect(),
        changed: a
            .iter()
            .filter(|(key, value)| b.get(*key).is_some_and(|other| other != *value))
            .map(|(key, _)| key.clone())
            .collect(),
        profile_changed: before.profile != after.profile,
        before_frames: before.duration(),
        after_frames: after.duration(),
    }
}
