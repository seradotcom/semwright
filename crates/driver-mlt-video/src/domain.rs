//! Projection between the MLT round-trip model and the backend-neutral video domain.
//!
//! XML/MLT details stop at this boundary. Native structures that cannot be
//! edited semantically project as read-only objects.

use crate::{edit as mlt_edit, model as mlt};
use semwright_video_domain::{edit as video_edit, model as video};

fn access(read_only: bool, reason: &str) -> video::Editability {
    if read_only {
        video::Editability::read_only(reason)
    } else {
        video::Editability::Editable
    }
}

fn profile(v: &mlt::Profile) -> video::Profile {
    video::Profile {
        width: v.width,
        height: v.height,
        fps: v.fps,
        progressive: v.progressive,
        sample_aspect: v.sample_aspect,
        display_aspect: v.display_aspect,
        colorspace: v.colorspace,
        audio_channels: v.audio_channels,
    }
}

fn resource(v: &mlt::Resource) -> video::Resource {
    match v {
        mlt::Resource::Scoped { root, path } => video::Resource::Scoped {
            root: root.clone(),
            path: path.clone(),
        },
        mlt::Resource::Relative(v) => video::Resource::Relative(v.clone()),
        mlt::Resource::External(v) => video::Resource::External(v.clone()),
        mlt::Resource::Color(v) => video::Resource::Color(v.clone()),
        mlt::Resource::Opaque(v) => video::Resource::Opaque(v.clone()),
    }
}

fn asset(v: &mlt::MediaAsset) -> video::MediaAsset {
    video::MediaAsset {
        id: v.id.clone(),
        name: v.name.clone(),
        kind: v.kind.clone(),
        resource: resource(&v.resource),
        frames: v.frames,
        editability: access(v.opaque, "backend-native media producer is inspection-only"),
    }
}

fn interpolation(v: mlt::Interpolation) -> video::Interpolation {
    match v {
        mlt::Interpolation::Linear => video::Interpolation::Linear,
        mlt::Interpolation::Hold => video::Interpolation::Hold,
    }
}

fn keyframe(v: &mlt::Keyframe) -> video::Keyframe {
    video::Keyframe {
        frame: v.frame,
        value: v.value,
        interpolation: interpolation(v.interpolation),
    }
}

fn effect(v: &mlt::Effect) -> video::Effect {
    video::Effect {
        id: v.id.clone(),
        kind: v.service.clone(),
        parameter: v.property.clone(),
        value: v.value,
        enabled: v.enabled,
        keyframes: v.keyframes.iter().map(keyframe).collect(),
        editability: access(
            v.opaque.is_some(),
            "backend-native effect is inspection-only",
        ),
    }
}

fn clip(v: &mlt::Clip) -> video::Clip {
    video::Clip {
        id: v.id.clone(),
        name: v.name.clone(),
        asset: v.asset.clone(),
        start: v.start,
        source: v.source,
        effects: v.effects.iter().map(effect).collect(),
        speed: v.speed,
    }
}

fn track(v: &mlt::Track) -> video::Track {
    video::Track {
        id: v.id.clone(),
        name: v.name.clone(),
        kind: v.kind.clone(),
        muted: v.muted,
        hidden: v.hidden,
        lanes: v
            .lanes
            .iter()
            .map(|lane| video::Timeline {
                id: lane.id.clone(),
                clips: lane.clips.iter().map(clip).collect(),
            })
            .collect(),
        effects: v.effects.iter().map(effect).collect(),
        editability: access(v.opaque, "backend-native track graph is inspection-only"),
    }
}

fn transition(v: &mlt::Transition) -> video::Transition {
    video::Transition {
        id: v.id.clone(),
        kind: v.kind.clone(),
        a_track: v.a_track.clone(),
        b_track: v.b_track.clone(),
        range: v.range,
        reverse: v.reverse,
        editability: access(
            v.opaque.is_some(),
            "backend-native transition is inspection-only",
        ),
    }
}

fn sequence(v: &mlt::Sequence) -> video::Sequence {
    video::Sequence {
        id: v.id.clone(),
        name: v.name.clone(),
        tracks: v.tracks.iter().map(track).collect(),
        transitions: v.transitions.iter().map(transition).collect(),
        markers: v
            .markers
            .iter()
            .map(|m| video::Marker {
                id: m.id.clone(),
                frame: m.frame,
                label: m.label.clone(),
                tags: m.tags.clone(),
            })
            .collect(),
        nested: v.nested.clone(),
        subtitles: v
            .subtitles
            .iter()
            .map(|s| video::SubtitleReference {
                resource: s.resource.clone(),
                representation: s.representation.clone(),
            })
            .collect(),
        editability: access(v.opaque, "backend-native sequence graph is inspection-only"),
    }
}

pub fn project(v: &mlt::Project) -> video::Project {
    video::Project {
        model_version: video::MODEL_VERSION,
        id: v.id.clone(),
        profile: profile(&v.profile),
        assets: v
            .assets
            .iter()
            .map(|(id, item)| (id.clone(), asset(item)))
            .collect(),
        sequences: v.sequences.iter().map(sequence).collect(),
        warnings: v.warnings.clone(),
    }
}

pub fn edit(v: &mlt_edit::Edit) -> video_edit::Edit {
    match v {
        mlt_edit::Edit::Profile(v) => video_edit::Edit::Profile(profile(v)),
        mlt_edit::Edit::SequenceCreate { name } => {
            video_edit::Edit::SequenceCreate { name: name.clone() }
        }
        mlt_edit::Edit::AssetImport { asset: v } => {
            video_edit::Edit::AssetImport { asset: asset(v) }
        }
        mlt_edit::Edit::AssetRelink {
            id,
            expected,
            resource: v,
        } => video_edit::Edit::AssetRelink {
            id: id.clone(),
            expected: expected.clone(),
            resource: resource(v),
        },
        mlt_edit::Edit::TrackCreate {
            sequence,
            name,
            kind,
        } => video_edit::Edit::TrackCreate {
            sequence: sequence.clone(),
            name: name.clone(),
            kind: kind.clone(),
        },
        mlt_edit::Edit::TrackRemove { sequence, track } => video_edit::Edit::TrackRemove {
            sequence: sequence.clone(),
            track: track.clone(),
        },
        mlt_edit::Edit::TrackRename {
            sequence,
            track,
            name,
        } => video_edit::Edit::TrackRename {
            sequence: sequence.clone(),
            track: track.clone(),
            name: name.clone(),
        },
        mlt_edit::Edit::TrackMute {
            sequence,
            track,
            value,
        } => video_edit::Edit::TrackMute {
            sequence: sequence.clone(),
            track: track.clone(),
            value: *value,
        },
        mlt_edit::Edit::TrackHide {
            sequence,
            track,
            value,
        } => video_edit::Edit::TrackHide {
            sequence: sequence.clone(),
            track: track.clone(),
            value: *value,
        },
        mlt_edit::Edit::TrackReorder {
            sequence,
            track,
            index,
        } => video_edit::Edit::TrackReorder {
            sequence: sequence.clone(),
            track: track.clone(),
            index: *index,
        },
        mlt_edit::Edit::Insert {
            sequence,
            track,
            asset,
            start,
            source,
            name,
            ripple,
        } => video_edit::Edit::Insert {
            sequence: sequence.clone(),
            track: track.clone(),
            asset: asset.clone(),
            start: *start,
            source: *source,
            name: name.clone(),
            ripple: *ripple,
        },
        mlt_edit::Edit::Move {
            sequence,
            clip,
            track,
            start,
        } => video_edit::Edit::Move {
            sequence: sequence.clone(),
            clip: clip.clone(),
            track: track.clone(),
            start: *start,
        },
        mlt_edit::Edit::Trim {
            sequence,
            clip,
            source,
        } => video_edit::Edit::Trim {
            sequence: sequence.clone(),
            clip: clip.clone(),
            source: *source,
        },
        mlt_edit::Edit::Split { sequence, clip, at } => video_edit::Edit::Split {
            sequence: sequence.clone(),
            clip: clip.clone(),
            at: *at,
        },
        mlt_edit::Edit::Remove {
            sequence,
            clip,
            ripple,
        } => video_edit::Edit::Remove {
            sequence: sequence.clone(),
            clip: clip.clone(),
            ripple: *ripple,
        },
        mlt_edit::Edit::Duplicate {
            sequence,
            clip,
            track,
            start,
        } => video_edit::Edit::Duplicate {
            sequence: sequence.clone(),
            clip: clip.clone(),
            track: track.clone(),
            start: *start,
        },
        mlt_edit::Edit::TransitionAdd {
            sequence,
            kind,
            a_track,
            b_track,
            range,
        } => video_edit::Edit::TransitionAdd {
            sequence: sequence.clone(),
            kind: kind.clone(),
            a_track: a_track.clone(),
            b_track: b_track.clone(),
            range: *range,
        },
        mlt_edit::Edit::TransitionPatch {
            sequence,
            id,
            range,
            reverse,
        } => video_edit::Edit::TransitionPatch {
            sequence: sequence.clone(),
            id: id.clone(),
            range: *range,
            reverse: *reverse,
        },
        mlt_edit::Edit::TransitionRemove { sequence, id } => video_edit::Edit::TransitionRemove {
            sequence: sequence.clone(),
            id: id.clone(),
        },
        mlt_edit::Edit::EffectAdd {
            sequence,
            clip,
            service,
            value,
        } => video_edit::Edit::EffectAdd {
            sequence: sequence.clone(),
            clip: clip.clone(),
            kind: service.clone(),
            value: *value,
        },
        mlt_edit::Edit::EffectPatch {
            sequence,
            clip,
            id,
            value,
        } => video_edit::Edit::EffectPatch {
            sequence: sequence.clone(),
            clip: clip.clone(),
            id: id.clone(),
            value: *value,
        },
        mlt_edit::Edit::EffectRemove { sequence, clip, id } => video_edit::Edit::EffectRemove {
            sequence: sequence.clone(),
            clip: clip.clone(),
            id: id.clone(),
        },
        mlt_edit::Edit::EffectEnable {
            sequence,
            clip,
            id,
            value,
        } => video_edit::Edit::EffectEnable {
            sequence: sequence.clone(),
            clip: clip.clone(),
            id: id.clone(),
            value: *value,
        },
        mlt_edit::Edit::KeyframeSet {
            sequence,
            clip,
            effect,
            keyframe: v,
        } => video_edit::Edit::KeyframeSet {
            sequence: sequence.clone(),
            clip: clip.clone(),
            effect: effect.clone(),
            keyframe: keyframe(v),
        },
        mlt_edit::Edit::KeyframeRemove {
            sequence,
            clip,
            effect,
            frame,
        } => video_edit::Edit::KeyframeRemove {
            sequence: sequence.clone(),
            clip: clip.clone(),
            effect: effect.clone(),
            frame: *frame,
        },
        mlt_edit::Edit::MarkerAdd {
            sequence,
            frame,
            label,
        } => video_edit::Edit::MarkerAdd {
            sequence: sequence.clone(),
            frame: *frame,
            label: label.clone(),
        },
        mlt_edit::Edit::MarkerPatch {
            sequence,
            id,
            frame,
            label,
        } => video_edit::Edit::MarkerPatch {
            sequence: sequence.clone(),
            id: id.clone(),
            frame: *frame,
            label: label.clone(),
        },
        mlt_edit::Edit::MarkerRemove { sequence, id } => video_edit::Edit::MarkerRemove {
            sequence: sequence.clone(),
            id: id.clone(),
        },
        mlt_edit::Edit::AudioVolume {
            sequence,
            clip,
            value,
        } => video_edit::Edit::AudioVolume {
            sequence: sequence.clone(),
            clip: clip.clone(),
            value: *value,
        },
        mlt_edit::Edit::AudioFade {
            sequence,
            clip,
            frames,
            fade_in,
        } => video_edit::Edit::AudioFade {
            sequence: sequence.clone(),
            clip: clip.clone(),
            frames: *frames,
            fade_in: *fade_in,
        },
    }
}
