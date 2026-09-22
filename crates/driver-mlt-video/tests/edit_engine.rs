mod common;
use semwright_mlt_video::{
    adapters,
    edit::{self, Edit},
    model::*,
    time::FrameRange,
};
fn run(p: &Project, edit: Edit) -> semwright_mlt_video::Result<edit::ChangePlan> {
    edit::plan(p, &edit::revision(p)?, edit, "test", false)
}
fn insert(start: u64, ripple: bool) -> Edit {
    Edit::Insert {
        sequence: "sequence0".into(),
        track: "t".into(),
        asset: "a".into(),
        start,
        source: FrameRange::new(0, 10).unwrap(),
        name: "B roll".into(),
        ripple,
    }
}
#[test]
fn reject_stale_plan_without_mutation() {
    let p = common::small();
    let old = p.semantic_json();
    assert!(edit::plan(&p, &"0".repeat(64), insert(100, false), "test", false).is_err());
    assert_eq!(old, p.semantic_json());
}
#[test]
fn preview_does_not_modify_original() {
    let p = common::small();
    let r = run(&p, insert(100, false)).unwrap();
    assert_eq!(p.sequences[0].duration(), 100);
    assert_eq!(r.after_frames, 110);
}
#[test]
fn identical_plan_has_same_digest() {
    let p = common::small();
    let a = run(&p, insert(100, false)).unwrap();
    let b = run(&p, insert(100, false)).unwrap();
    assert_eq!(a.resulting_revision, b.resulting_revision);
    assert_eq!(a.created, b.created);
}
#[test]
fn overlapping_insert_rejected() {
    let p = common::small();
    assert!(run(&p, insert(30, false)).is_err());
}
#[test]
fn ripple_insert_moves_following_clip() {
    let p = common::small();
    let r = run(&p, insert(0, true)).unwrap();
    assert_eq!(r.after_frames, 110);
    assert_eq!(r.result.clip("sequence0", "c").unwrap().start, 10);
}
#[test]
fn ripple_insert_cannot_implicitly_split() {
    assert!(run(&common::small(), insert(50, true)).is_err());
}
#[test]
fn lift_delete_leaves_other_position() {
    let p = run(&common::small(), insert(100, false)).unwrap().result;
    let r = run(
        &p,
        Edit::Remove {
            sequence: "sequence0".into(),
            clip: "c".into(),
            ripple: false,
        },
    )
    .unwrap();
    assert_eq!(r.result.sequences[0].tracks[0].lanes[0].clips[0].start, 100);
}
#[test]
fn ripple_delete_is_track_local() {
    let mut p = run(&common::small(), insert(100, false)).unwrap().result;
    let mut t = common::track("other");
    t.lanes[0]
        .clips
        .push(common::clip("elsewhere", "a", 30, 10));
    p.sequences[0].tracks.push(t);
    let r = run(
        &p,
        Edit::Remove {
            sequence: "sequence0".into(),
            clip: "c".into(),
            ripple: true,
        },
    )
    .unwrap()
    .result;
    assert_eq!(r.sequences[0].tracks[0].lanes[0].clips[0].start, 0);
    assert_eq!(r.clip("sequence0", "elsewhere").unwrap().start, 30);
}
#[test]
fn move_lifts_to_explicit_track() {
    let mut p = common::small();
    p.sequences[0].tracks.push(common::track("other"));
    let r = run(
        &p,
        Edit::Move {
            sequence: "sequence0".into(),
            clip: "c".into(),
            track: "other".into(),
            start: 20,
        },
    )
    .unwrap()
    .result;
    assert!(r.sequences[0].tracks[0].lanes[0].clips.is_empty());
    assert_eq!(r.clip("sequence0", "c").unwrap().start, 20);
}
#[test]
fn trim_preserves_timeline_anchor() {
    let r = run(
        &common::small(),
        Edit::Trim {
            sequence: "sequence0".into(),
            clip: "c".into(),
            source: FrameRange::new(20, 70).unwrap(),
        },
    )
    .unwrap();
    assert_eq!(r.result.clip("sequence0", "c").unwrap().start, 0);
    assert_eq!(r.after_frames, 50);
}
#[test]
fn source_bounds_validated() {
    let mut p = common::small();
    p.assets.get_mut("a").unwrap().frames = Some(100);
    assert!(
        run(
            &p,
            Edit::Trim {
                sequence: "sequence0".into(),
                clip: "c".into(),
                source: FrameRange::new(20, 101).unwrap()
            }
        )
        .is_err()
    );
}
#[test]
fn duplicate_creates_fresh_identity() {
    let p = common::small();
    let r = run(
        &p,
        Edit::Duplicate {
            sequence: "sequence0".into(),
            clip: "c".into(),
            track: "t".into(),
            start: 100,
        },
    )
    .unwrap();
    assert_ne!(r.created[0], "c");
    assert_eq!(r.result.sequences[0].tracks[0].lanes[0].clips.len(), 2);
}
#[test]
fn track_order_changes_without_identity_alias() {
    let mut p = common::small();
    p.sequences[0].tracks.push(common::track("other"));
    let r = run(
        &p,
        Edit::TrackReorder {
            sequence: "sequence0".into(),
            track: "other".into(),
            index: 0,
        },
    )
    .unwrap();
    assert_eq!(r.result.sequences[0].tracks[0].id, "other");
    assert_eq!(r.result.sequences[0].tracks[1].id, "t");
}
#[test]
fn mute_hide_are_independent() {
    let p = run(
        &common::small(),
        Edit::TrackMute {
            sequence: "sequence0".into(),
            track: "t".into(),
            value: true,
        },
    )
    .unwrap()
    .result;
    assert!(p.track("sequence0", "t").unwrap().muted);
    assert!(!p.track("sequence0", "t").unwrap().hidden);
}
#[test]
fn transition_requires_two_covered_tracks() {
    let p = common::small();
    assert!(
        run(
            &p,
            Edit::TransitionAdd {
                sequence: "sequence0".into(),
                kind: "dissolve".into(),
                a_track: "t".into(),
                b_track: "missing".into(),
                range: FrameRange::new(0, 20).unwrap()
            }
        )
        .is_err()
    );
}
#[test]
fn dissolve_has_explicit_overlap() {
    let mut p = common::small();
    let mut t = common::track("other");
    t.lanes[0].clips.push(common::clip("b", "a", 0, 100));
    p.sequences[0].tracks.push(t);
    let r = run(
        &p,
        Edit::TransitionAdd {
            sequence: "sequence0".into(),
            kind: "dissolve".into(),
            a_track: "t".into(),
            b_track: "other".into(),
            range: FrameRange::new(10, 20).unwrap(),
        },
    )
    .unwrap();
    assert_eq!(r.result.sequences[0].transitions[0].range.duration(), 10);
    let xml = adapters::save(&r.result).unwrap();
    assert!(xml.contains("luma"));
}
#[test]
fn remove_track_rejects_dependent_transition() {
    let mut p = common::small();
    let mut t = common::track("other");
    t.lanes[0].clips.push(common::clip("b", "a", 0, 100));
    p.sequences[0].tracks.push(t);
    let p = run(
        &p,
        Edit::TransitionAdd {
            sequence: "sequence0".into(),
            kind: "audio_mix".into(),
            a_track: "t".into(),
            b_track: "other".into(),
            range: FrameRange::new(10, 20).unwrap(),
        },
    )
    .unwrap()
    .result;
    assert!(
        run(
            &p,
            Edit::TrackRemove {
                sequence: "sequence0".into(),
                track: "other".into()
            }
        )
        .is_err()
    );
}
#[test]
fn effect_filter_order_preserved() {
    let p = run(
        &common::small(),
        Edit::EffectAdd {
            sequence: "sequence0".into(),
            clip: "c".into(),
            service: "volume".into(),
            value: -6000,
        },
    )
    .unwrap()
    .result;
    let p = edit::plan(
        &p,
        &edit::revision(&p).unwrap(),
        Edit::EffectAdd {
            sequence: "sequence0".into(),
            clip: "c".into(),
            service: "brightness".into(),
            value: 1200,
        },
        "second",
        false,
    )
    .unwrap()
    .result;
    assert_eq!(
        p.clip("sequence0", "c")
            .unwrap()
            .effects
            .iter()
            .map(|e| e.service.as_str())
            .collect::<Vec<_>>(),
        vec!["volume", "brightness"]
    );
}
#[test]
fn animated_effect_split_rebases_both_halves() {
    let p = run(
        &common::small(),
        Edit::AudioFade {
            sequence: "sequence0".into(),
            clip: "c".into(),
            frames: 100,
            fade_in: true,
        },
    )
    .unwrap()
    .result;
    let r = run(
        &p,
        Edit::Split {
            sequence: "sequence0".into(),
            clip: "c".into(),
            at: 50,
        },
    )
    .unwrap();
    let clips = &r.result.sequences[0].tracks[0].lanes[0].clips;
    assert_ne!(clips[0].effects[0].id, clips[1].effects[0].id);
    assert_eq!(clips[1].effects[0].keyframes[0].frame, 0);
    assert_eq!(
        clips[1].effects[0].at(0),
        p.clip("sequence0", "c").unwrap().effects[0].at(50)
    );
}
#[test]
fn fade_must_fit_clip() {
    assert!(
        run(
            &common::small(),
            Edit::AudioFade {
                sequence: "sequence0".into(),
                clip: "c".into(),
                frames: 101,
                fade_in: true
            }
        )
        .is_err()
    );
}
#[test]
fn marker_identity_survives_roundtrip() {
    let r = run(
        &common::small(),
        Edit::MarkerAdd {
            sequence: "sequence0".into(),
            frame: 20,
            label: "Cut here".into(),
        },
    )
    .unwrap();
    let back = adapters::load(adapters::save(&r.result).unwrap().as_bytes()).unwrap();
    assert_eq!(back.sequences[0].markers[0].id, r.created[0]);
}
#[test]
fn profile_fps_change_preserves_frame_numbers() {
    let p = common::small();
    let mut q = p.profile.clone();
    q.fps = semwright_mlt_video::time::FrameRate::new(30000, 1001).unwrap();
    let r = run(&p, Edit::Profile(q)).unwrap();
    assert_eq!(r.result.clip("sequence0", "c").unwrap().duration(), 100);
}
#[test]
fn unknown_metadata_does_not_gain_edit_permission() {
    let p = adapters::load(&common::fixture("mlt/unknown-element.mlt")).unwrap();
    assert!(!p.generated);
    assert!(
        run(
            &p,
            Edit::MarkerAdd {
                sequence: "sequence0".into(),
                frame: 0,
                label: "x".into()
            }
        )
        .is_err()
    );
}
#[test]
fn non_unit_speed_cannot_silently_desynchronize() {
    let mut p = common::small();
    p.clip_mut("sequence0", "c").unwrap().speed = (2, 1);
    assert!(p.validate().is_err());
}
#[test]
fn compact_diff_reports_moved_clip() {
    let p = common::small();
    let q = run(
        &p,
        Edit::Move {
            sequence: "sequence0".into(),
            clip: "c".into(),
            track: "t".into(),
            start: 5,
        },
    )
    .unwrap()
    .result;
    assert_eq!(
        edit::diff(&p, &q)
            .get("changed")
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
#[test]
fn stress_20_tracks_1000_clips_100_effects() {
    let mut p = Project::new(Profile::default()).unwrap();
    p.assets.insert("a".into(), common::color("a"));
    for ti in 0..20 {
        let mut t = common::track(&format!("t{ti}"));
        for ci in 0..50 {
            let mut c = common::clip(&format!("c{ti}_{ci}"), "a", ci * 2, 2);
            if ci < 5 {
                c.effects.push(Effect {
                    id: format!("e{ti}_{ci}"),
                    service: "brightness".into(),
                    property: "level".into(),
                    value: 1000,
                    enabled: true,
                    keyframes: vec![],
                    opaque: None,
                });
            }
            t.lanes[0].clips.push(c);
        }
        p.sequences[0].tracks.push(t);
    }
    let start = std::time::Instant::now();
    p.validate().unwrap();
    let xml = adapters::save(&p).unwrap();
    let back = adapters::load(xml.as_bytes()).unwrap();
    assert_eq!(p.semantic_json(), back.semantic_json());
    eprintln!(
        "stress elapsed_ms={} xml_bytes={}",
        start.elapsed().as_millis(),
        xml.len()
    );
}
