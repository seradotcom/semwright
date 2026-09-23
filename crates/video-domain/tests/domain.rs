use semwright_video_domain::{
    edit::{self, Edit},
    model::{
        Clip, Editability, Effect, Interpolation, Keyframe, MediaAsset, Profile, Project, Resource,
        Timeline, Track, Transition,
    },
    refs::RefStore,
    support::{MutationSupport, VideoOperation},
    time::FrameRange,
};

fn fixture() -> Project {
    let mut project = Project::new(Profile::default()).unwrap();
    project.assets.insert(
        "asset-a".into(),
        MediaAsset {
            id: "asset-a".into(),
            name: "A".into(),
            kind: "video".into(),
            resource: Resource::Relative("a.mp4".into()),
            frames: Some(2_000),
            editability: Editability::Editable,
        },
    );
    project.sequences[0].tracks.push(Track {
        id: "track-a".into(),
        name: "Video 1".into(),
        kind: "video".into(),
        muted: false,
        hidden: false,
        lanes: vec![Timeline {
            id: "lane-a".into(),
            clips: vec![
                Clip {
                    id: "clip-a".into(),
                    name: "A1".into(),
                    asset: "asset-a".into(),
                    start: 0,
                    source: FrameRange::new(0, 100).unwrap(),
                    effects: vec![],
                    speed: (1, 1),
                },
                Clip {
                    id: "clip-b".into(),
                    name: "A2".into(),
                    asset: "asset-a".into(),
                    start: 100,
                    source: FrameRange::new(100, 200).unwrap(),
                    effects: vec![],
                    speed: (1, 1),
                },
            ],
        }],
        effects: vec![],
        editability: Editability::Editable,
    });
    project.validate().unwrap();
    project
}

#[test]
fn model_roundtrips_as_backend_neutral_json() {
    let project = fixture();
    let encoded = serde_json::to_vec(&project).unwrap();
    let decoded: Project = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded, project);
    let text = String::from_utf8(encoded).unwrap();
    assert!(!text.contains("mlt_service"));
    assert!(!text.contains("XmlBinding"));
}

#[test]
fn video_operation_contract_is_unique_typed_and_serializable() {
    let mut names = std::collections::BTreeSet::new();
    assert_eq!(VideoOperation::ALL.len(), 32);
    for operation in VideoOperation::ALL {
        assert!(names.insert(operation.as_str()));
        assert_eq!(
            operation.as_str().parse::<VideoOperation>().unwrap(),
            *operation
        );
        let encoded = serde_json::to_string(operation).unwrap();
        assert_eq!(
            serde_json::from_str::<VideoOperation>(&encoded).unwrap(),
            *operation
        );
    }
    assert!("unknown.operation".parse::<VideoOperation>().is_err());
    assert_eq!(
        serde_json::to_string(&MutationSupport::SafeRoundtrip).unwrap(),
        "\"safe_roundtrip\""
    );
}

#[test]
fn semantic_digest_is_deterministic_and_content_sensitive() {
    let project = fixture();
    assert_eq!(
        project.semantic_digest().unwrap(),
        project.semantic_digest().unwrap()
    );
    let mut changed = project.clone();
    changed.sequences[0].name = "Renamed".into();
    assert_ne!(
        project.semantic_digest().unwrap(),
        changed.semantic_digest().unwrap()
    );
}

#[test]
fn insert_ripple_moves_following_clip() {
    let project = fixture();
    let outcome = edit::apply_with_identity_base(
        &project,
        Edit::Insert {
            sequence: "sequence0".into(),
            track: "track-a".into(),
            asset: "asset-a".into(),
            start: 100,
            source: FrameRange::new(300, 325).unwrap(),
            name: "Inserted".into(),
            ripple: true,
        },
        "seed",
        "native-revision",
    )
    .unwrap();
    assert_eq!(
        outcome.result.clip("sequence0", "clip-b").unwrap().start,
        125
    );
    assert_eq!(outcome.created.len(), 1);
    assert_eq!(outcome.before_frames, 200);
    assert_eq!(outcome.after_frames, 225);
}

#[test]
fn deterministic_identity_base_matches_across_backends() {
    let project = fixture();
    let edit = Edit::MarkerAdd {
        sequence: "sequence0".into(),
        frame: 10,
        label: "beat".into(),
    };
    let a = edit::apply_with_identity_base(&project, edit.clone(), "seed", "rev-42").unwrap();
    let b = edit::apply_with_identity_base(&project, edit, "seed", "rev-42").unwrap();
    assert_eq!(a.created, b.created);
    assert_eq!(a.result, b.result);
}

#[test]
fn duplicate_gets_new_identity_without_aliasing_source() {
    let project = fixture();
    let outcome = edit::apply_with_identity_base(
        &project,
        Edit::Duplicate {
            sequence: "sequence0".into(),
            clip: "clip-a".into(),
            track: "track-a".into(),
            start: 250,
        },
        "dup",
        "rev",
    )
    .unwrap();
    assert_eq!(outcome.created.len(), 1);
    assert_ne!(outcome.created[0], "clip-a");
    assert_eq!(
        outcome
            .result
            .clip("sequence0", &outcome.created[0])
            .unwrap()
            .source,
        FrameRange::new(0, 100).unwrap()
    );
}

#[test]
fn split_rebases_animated_effects() {
    let mut project = fixture();
    project
        .clip_mut("sequence0", "clip-a")
        .unwrap()
        .effects
        .push(Effect {
            id: "effect-a".into(),
            kind: "volume".into(),
            parameter: "level".into(),
            value: -10_000,
            enabled: true,
            keyframes: vec![
                Keyframe {
                    frame: 0,
                    value: -10_000,
                    interpolation: Interpolation::Linear,
                },
                Keyframe {
                    frame: 99,
                    value: 0,
                    interpolation: Interpolation::Linear,
                },
            ],
            editability: Editability::Editable,
        });
    project.validate().unwrap();

    let outcome = edit::apply_with_identity_base(
        &project,
        Edit::Split {
            sequence: "sequence0".into(),
            clip: "clip-a".into(),
            at: 40,
        },
        "split",
        "rev",
    )
    .unwrap();
    let left = outcome.result.clip("sequence0", "clip-a").unwrap();
    let right = outcome
        .result
        .clip("sequence0", &outcome.created[0])
        .unwrap();
    assert_eq!(left.duration(), 40);
    assert_eq!(right.duration(), 60);
    assert_eq!(right.effects[0].keyframes[0].frame, 0);
    assert!(right.effects[0].keyframes.iter().all(|key| key.frame < 60));
}

#[test]
fn read_only_track_blocks_insertion() {
    let mut project = fixture();
    project.sequences[0].tracks[0].editability =
        Editability::read_only("native graph cannot be round-tripped");
    let error = edit::apply(
        &project,
        Edit::Insert {
            sequence: "sequence0".into(),
            track: "track-a".into(),
            asset: "asset-a".into(),
            start: 250,
            source: FrameRange::new(0, 10).unwrap(),
            name: "blocked".into(),
            ripple: false,
        },
        "seed",
    )
    .unwrap_err();
    assert_eq!(error.code, "Unsupported");
}

#[test]
fn read_only_effect_cannot_be_mutated() {
    let mut project = fixture();
    project
        .clip_mut("sequence0", "clip-a")
        .unwrap()
        .effects
        .push(Effect {
            id: "effect-native".into(),
            kind: "opaque-native".into(),
            parameter: "opaque".into(),
            value: 0,
            enabled: true,
            keyframes: vec![],
            editability: Editability::read_only("native effect"),
        });
    project.validate().unwrap();
    let error = edit::apply(
        &project,
        Edit::EffectPatch {
            sequence: "sequence0".into(),
            clip: "clip-a".into(),
            id: "effect-native".into(),
            value: 1,
        },
        "seed",
    )
    .unwrap_err();
    assert_eq!(error.code, "Unsupported");
}

#[test]
fn transition_requires_sources_covering_full_range() {
    let mut project = fixture();
    project.sequences[0].tracks.push(Track {
        id: "track-b".into(),
        name: "Video 2".into(),
        kind: "video".into(),
        muted: false,
        hidden: false,
        lanes: vec![Timeline {
            id: "lane-b".into(),
            clips: vec![Clip {
                id: "clip-c".into(),
                name: "B1".into(),
                asset: "asset-a".into(),
                start: 50,
                source: FrameRange::new(0, 100).unwrap(),
                effects: vec![],
                speed: (1, 1),
            }],
        }],
        effects: vec![],
        editability: Editability::Editable,
    });
    project.sequences[0].transitions.push(Transition {
        id: "transition-a".into(),
        kind: "dissolve".into(),
        a_track: "track-a".into(),
        b_track: "track-b".into(),
        range: FrameRange::new(60, 90).unwrap(),
        reverse: false,
        editability: Editability::Editable,
    });
    project.validate().unwrap();

    project.sequences[0].transitions[0].range = FrameRange::new(10, 30).unwrap();
    assert_eq!(project.validate().unwrap_err().code, "InvalidArgument");
}

#[test]
fn structural_diff_reports_moved_clip() {
    let before = fixture();
    let mut after = before.clone();
    after.clip_mut("sequence0", "clip-b").unwrap().start = 120;
    let diff = edit::diff(&before, &after);
    assert_eq!(diff.changed, vec!["clip-b"]);
    assert!(diff.added.is_empty());
    assert!(diff.removed.is_empty());
    assert_eq!(diff.before_frames, 200);
    assert_eq!(diff.after_frames, 220);
}

#[test]
fn duplicate_semantic_ids_are_rejected() {
    let mut project = fixture();
    project.sequences[0].tracks[0].lanes[0].id = "clip-a".into();
    assert_eq!(project.validate().unwrap_err().code, "InvalidArgument");
}

#[test]
fn refs_are_stable_per_revision_and_invalidated_per_project() {
    let mut refs = RefStore::new(8);
    let a = refs.issue("p", "r1", "clip", "c").unwrap();
    let b = refs.issue("p", "r1", "clip", "c").unwrap();
    assert_eq!(a, b);
    assert_eq!(refs.resolve(&a, "p", "r1", "clip").unwrap(), "c");
    assert!(refs.resolve(&a, "p", "r2", "clip").is_err());
    refs.invalidate("p");
    assert!(refs.get(&a, "clip").is_err());
}

#[test]
fn mutation_support_is_capability_not_authority() {
    assert!(MutationSupport::SafeRoundtrip.allows_mutation());
    assert!(MutationSupport::MetadataRisk.allows_mutation());
    assert!(MutationSupport::MetadataRisk.requires_metadata_acknowledgement());
    assert!(!MutationSupport::RenderOnly.allows_mutation());
    assert!(!MutationSupport::Unsupported.allows_mutation());
}

#[test]
fn shared_conformance_accepts_equivalent_backend_projection() {
    let before = fixture();
    let intent = Edit::MarkerAdd {
        sequence: "sequence0".into(),
        frame: 42,
        label: "sync".into(),
    };
    let expected =
        edit::apply_with_identity_base(&before, intent.clone(), "seed", "native-revision").unwrap();
    let verified = semwright_video_domain::conformance::verify_backend_mutation(
        &before,
        intent,
        "seed",
        "native-revision",
        &expected.result,
        &expected.affected,
        &expected.created,
    )
    .unwrap();
    assert_eq!(verified, expected);
}

#[test]
fn shared_conformance_rejects_backend_semantic_drift() {
    let before = fixture();
    let intent = Edit::MarkerAdd {
        sequence: "sequence0".into(),
        frame: 42,
        label: "sync".into(),
    };
    let expected =
        edit::apply_with_identity_base(&before, intent.clone(), "seed", "native-revision").unwrap();
    let mut divergent = expected.result.clone();
    divergent.sequences[0].name = "backend-only drift".into();

    let error = semwright_video_domain::conformance::verify_backend_mutation(
        &before,
        intent,
        "seed",
        "native-revision",
        &divergent,
        &expected.affected,
        &expected.created,
    )
    .unwrap_err();
    assert_eq!(error.code, "BackendFailed");
}
