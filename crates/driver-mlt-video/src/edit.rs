//! Transactional timeline engine. Validate on a clone; commit is the caller's explicit choice.
use crate::{
    Error, Result,
    adapters::{self, Support},
    hash::sha256,
    json::{Value, array, obj},
    model::*,
    time::FrameRange,
};
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
        service: String,
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
    pub fn operation(&self) -> &'static str {
        match self {
            Self::Profile(_) => "project.profile.set",
            Self::SequenceCreate { .. } => "sequence.create",
            Self::AssetImport { .. } => "asset.import",
            Self::AssetRelink { .. } => "asset.relink",
            Self::TrackCreate { .. } => "track.create",
            Self::TrackRemove { .. } => "track.remove",
            Self::TrackRename { .. } => "track.rename",
            Self::TrackMute { .. } => "track.mute",
            Self::TrackHide { .. } => "track.hide",
            Self::TrackReorder { .. } => "track.reorder",
            Self::Insert { .. } => "clip.insert",
            Self::Move { .. } => "clip.move",
            Self::Trim { .. } => "clip.trim",
            Self::Split { .. } => "clip.split",
            Self::Remove { .. } => "clip.remove",
            Self::Duplicate { .. } => "clip.duplicate",
            Self::TransitionAdd { .. } => "transition.add",
            Self::TransitionPatch { .. } => "transition.patch",
            Self::TransitionRemove { .. } => "transition.remove",
            Self::EffectAdd { .. } => "effect.add",
            Self::EffectPatch { .. } => "effect.patch",
            Self::EffectRemove { .. } => "effect.remove",
            Self::EffectEnable { value: true, .. } => "effect.enable",
            Self::EffectEnable { .. } => "effect.disable",
            Self::KeyframeSet { .. } => "keyframe.set",
            Self::KeyframeRemove { .. } => "keyframe.remove",
            Self::MarkerAdd { .. } => "marker.add",
            Self::MarkerPatch { .. } => "marker.patch",
            Self::MarkerRemove { .. } => "marker.remove",
            Self::AudioVolume { .. } => "audio.volume.set",
            Self::AudioFade { fade_in: true, .. } => "audio.fade_in",
            Self::AudioFade { .. } => "audio.fade_out",
        }
    }
}
#[derive(Clone, Debug)]
pub struct ChangePlan {
    pub expected_revision: String,
    pub resulting_revision: String,
    pub operation: String,
    pub affected: Vec<String>,
    pub created: Vec<String>,
    pub before_frames: u64,
    pub after_frames: u64,
    pub support: Support,
    pub warnings: Vec<String>,
    pub result: Project,
}
impl ChangePlan {
    pub fn json(&self, applied: bool) -> Value {
        obj([
            ("applied", applied.into()),
            ("expected_revision", self.expected_revision.clone().into()),
            ("resulting_revision", self.resulting_revision.clone().into()),
            ("operation", self.operation.clone().into()),
            (
                "affected",
                array(self.affected.iter().cloned().map(Into::into)),
            ),
            (
                "created",
                array(self.created.iter().cloned().map(Into::into)),
            ),
            ("before_frames", self.before_frames.into()),
            ("after_frames", self.after_frames.into()),
            ("support", adapters::support_name(self.support).into()),
            (
                "warnings",
                array(self.warnings.iter().cloned().map(Into::into)),
            ),
        ])
    }
}
pub fn revision(p: &Project) -> Result<String> {
    Ok(sha256(adapters::save(p)?.as_bytes()))
}
fn duration(p: &Project) -> u64 {
    p.sequences
        .iter()
        .map(Sequence::duration)
        .max()
        .unwrap_or(0)
}
fn semantic_id(rev: &str, seed: &str, prefix: &str) -> String {
    format!(
        "{prefix}_{}",
        &sha256(format!("{rev}:{seed}:{prefix}").as_bytes())[..24]
    )
}
pub fn plan(
    project: &Project,
    expected: &str,
    edit: Edit,
    seed: &str,
    allow_metadata_risk: bool,
) -> Result<ChangePlan> {
    let actual = revision(project)?;
    if actual != expected {
        return Err(Error::stale());
    }
    let operation = edit.operation();
    let support = crate::domain::capabilities(project)?.support(operation)?;
    if support == Support::Unsupported || support == Support::RenderOnly {
        return Err(Error::unsupported(
            "Mutation is not available for this project graph/format",
        ));
    }
    if support == Support::MetadataRisk && !allow_metadata_risk {
        return Err(Error::unsupported(
            "Native metadata-risk mutation requires explicit acknowledgement; no GUI guarantee",
        ));
    }

    // The portable video domain is an executable semantic specification.
    // Backend-specific mutation below must remain observationally equivalent.
    let portable_before = crate::domain::project(project);
    let portable_edit = crate::domain::edit(&edit);

    let mut result = project.clone();
    let mut created = vec![];
    let mut affected = vec![];
    let new = |prefix: &str| semantic_id(expected, seed, prefix);
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
                opaque: false,
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
            if a.resource.text() != expected {
                return Err(Error::stale());
            }
            if a.proxy
                .as_deref()
                .is_some_and(|s| s != "-" && !s.is_empty())
                || a.original.is_some()
            {
                return Err(Error::unsupported(
                    "Proxy/original relink is not safe in this adapter",
                ));
            }
            if matches!(a.resource, Resource::Color(_) | Resource::Opaque(_)) {
                return Err(Error::unsupported(
                    "Cannot relink generator or opaque producer",
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
                opaque: false,
            });
            created.push(id);
        }
        Edit::TrackRemove { sequence, track } => {
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
            if project.format == Format::GenericMlt {
                result.track_mut(&sequence, &track)?.name = name;
            } else {
                adapters::rename_native_track(&mut result, &sequence, &track, &name)?;
            }
            affected.push(track);
        }
        Edit::TrackMute {
            sequence,
            track,
            value,
        } => {
            result.track_mut(&sequence, &track)?.muted = value;
            affected.push(track);
        }
        Edit::TrackHide {
            sequence,
            track,
            value,
        } => {
            result.track_mut(&sequence, &track)?.hidden = value;
            affected.push(track);
        }
        Edit::TrackReorder {
            sequence,
            track,
            index,
        } => {
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
                binding: None,
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
            right_clip.binding = None;
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
            let mut c = result.clip(&sequence, &clip)?.clone();
            let id = new("c");
            c.id = id.clone();
            c.start = start;
            c.binding = None;
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
                    opaque: None,
                });
            created.push(id);
        }
        Edit::TransitionPatch {
            sequence,
            id,
            range,
            reverse,
        } => {
            let t = result
                .sequence_mut(&sequence)?
                .transitions
                .iter_mut()
                .find(|t| t.id == id)
                .ok_or_else(|| Error::new("NotFound", "Transition missing"))?;
            if t.opaque.is_some() {
                return Err(Error::unsupported("Opaque transition"));
            }
            t.range = range;
            t.reverse = reverse;
            affected.push(id);
        }
        Edit::TransitionRemove { sequence, id } => {
            let seq = result.sequence_mut(&sequence)?;
            let i = seq
                .transitions
                .iter()
                .position(|t| t.id == id)
                .ok_or_else(|| Error::new("NotFound", "Transition missing"))?;
            seq.transitions.remove(i);
            affected.push(id);
        }
        Edit::EffectAdd {
            sequence,
            clip,
            service,
            value,
        } => {
            let id = new("ef");
            validate_effect_value(&service, "level", value)?;
            result.clip_mut(&sequence, &clip)?.effects.push(Effect {
                id: id.clone(),
                service,
                property: "level".into(),
                value,
                enabled: true,
                keyframes: vec![],
                opaque: None,
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
            validate_effect_value(&e.service, &e.property, value)?;
            if !e.keyframes.is_empty() {
                return Err(Error::invalid(
                    "Constant patch cannot erase animation; remove keyframes explicitly",
                ));
            }
            e.value = value;
            affected.push(id);
        }
        Edit::EffectRemove { sequence, clip, id } => {
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
            validate_effect_value(&e.service, &e.property, keyframe.value)?;
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
            validate_effect_value("volume", "level", value)?;
            let c = result.clip_mut(&sequence, &clip)?;
            let existing: Vec<_> = c
                .effects
                .iter()
                .enumerate()
                .filter(|(_, e)| e.service == "volume")
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
                if e.opaque.is_some() || !e.keyframes.is_empty() {
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
                    service: "volume".into(),
                    property: "level".into(),
                    value,
                    enabled: true,
                    keyframes: vec![],
                    opaque: None,
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
                service: "volume".into(),
                property: "level".into(),
                value: a.value,
                enabled: true,
                keyframes: vec![a, b],
                opaque: None,
            });
            created.push(id);
            affected.push(clip);
        }
    }
    result.validate()?;

    let projected_result = crate::domain::project(&result);
    semwright_video_domain::conformance::verify_backend_mutation(
        &portable_before,
        portable_edit,
        seed,
        &actual,
        &projected_result,
        &affected,
        &created,
    )?;

    let serialized = adapters::save(&result)?;
    let reloaded = adapters::load(serialized.as_bytes())?;
    if reloaded.semantic_json() != result.semantic_json() {
        return Err(Error::new(
            "BackendFailed",
            "Semantic serialize/reparse validation differs; edit not committed",
        ));
    }
    let resulting_revision = sha256(serialized.as_bytes());
    let warnings = if support == Support::MetadataRisk {
        vec!["Native GUI reopen has not been certified".into()]
    } else {
        vec![]
    };
    Ok(ChangePlan {
        expected_revision: actual,
        resulting_revision,
        operation: operation.into(),
        affected,
        created,
        before_frames: duration(project),
        after_frames: duration(&result),
        support,
        warnings,
        result,
    })
}
fn effect_mut<'a>(
    p: &'a mut Project,
    sequence: &str,
    clip: &str,
    id: &str,
) -> Result<&'a mut Effect> {
    let e = p
        .clip_mut(sequence, clip)?
        .effects
        .iter_mut()
        .find(|e| e.id == id)
        .ok_or_else(|| Error::new("NotFound", "Effect missing"))?;
    if e.opaque.is_some() {
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
    if t.opaque || t.lanes.len() != 1 {
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
/// Compact structural diff, not raw XML and not an undo log.
pub fn diff(before: &Project, after: &Project) -> Value {
    let clips = |p: &Project| -> std::collections::BTreeMap<String, (String, u64, u64, u64)> {
        p.sequences
            .iter()
            .flat_map(|s| {
                s.tracks.iter().flat_map(|t| {
                    t.lanes.iter().flat_map(move |l| {
                        l.clips.iter().map(move |c| {
                            (
                                c.id.clone(),
                                (t.id.clone(), c.start, c.source.start.0, c.source.end.0),
                            )
                        })
                    })
                })
            })
            .collect()
    };
    let a = clips(before);
    let b = clips(after);
    obj([
        (
            "added",
            array(
                b.keys()
                    .filter(|k| !a.contains_key(*k))
                    .cloned()
                    .map(Into::into),
            ),
        ),
        (
            "removed",
            array(
                a.keys()
                    .filter(|k| !b.contains_key(*k))
                    .cloned()
                    .map(Into::into),
            ),
        ),
        (
            "changed",
            array(
                a.iter()
                    .filter(|(k, v)| b.get(*k).is_some_and(|w| w != *v))
                    .map(|(k, _)| k.clone().into()),
            ),
        ),
        ("profile_changed", (before.profile != after.profile).into()),
        ("before_frames", duration(before).into()),
        ("after_frames", duration(after).into()),
    ])
}
