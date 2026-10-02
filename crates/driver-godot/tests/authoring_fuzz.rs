use proptest::prelude::*;
use semwright_godot_driver::authoring::{
    compile,
    native_observation::{
        InputStep, MAX_NATIVE_KEYS, MAX_NATIVE_NODES, MAX_NATIVE_TRACKS, NATIVE_VERSION,
        NativeAnimation, NativeNode, NativeObservation, NativeProjection, NativeRequest,
        NativeResourceRef, NativeTrack, PROBE_SOURCE, ProbeMode, key_page, track_page,
    },
    profile::PlanRequest,
    validate,
};
use semwright_semantic_composition::{Digest, strict_decode};
use std::collections::{BTreeMap, BTreeSet};

fn digest(label: &str) -> Digest {
    Digest::of_bytes(label.as_bytes())
}

fn cursor_observation() -> NativeObservation {
    let resource = NativeResourceRef {
        class: "Animation".into(),
        path: "".into(),
        uid: None,
        instance_id: "7".into(),
        local_to_scene: false,
    };
    NativeObservation {
        version: NATIVE_VERSION,
        nonce: "fuzz_native_nonce_0001".into(),
        source_fingerprint: digest("source"),
        mode: ProbeMode::Inspect,
        engine_version: "4.7.2.stable.official.fuzz".into(),
        process_id: "42".into(),
        loaded_scene: "res://scenes/arena.tscn".into(),
        loaded_scene_sha256: digest("scene"),
        candidate_sha256: None,
        authored: NativeProjection {
            nodes: vec![NativeNode {
                path: ".".into(),
                class: "Node2D".into(),
                instance_id: "1".into(),
                parent: None,
                owner: None,
                scene_file: "res://scenes/arena.tscn".into(),
                logical_id: None,
                logical_key: Some("scene:arena".into()),
                groups: vec![],
                properties: BTreeMap::new(),
            }],
            resources: vec![],
            animations: vec![NativeAnimation {
                player: ".".into(),
                library: "".into(),
                name: "clip".into(),
                root: ".".into(),
                length: 1.0,
                loop_mode: 0,
                resource,
                track_count: 2,
                tracks: (0..2)
                    .map(|index| NativeTrack {
                        index,
                        track_type: 0,
                        path: format!("Node:value_{index}"),
                        enabled: true,
                        interpolation: 1,
                        imported: false,
                        key_count: 0,
                        keys: vec![],
                    })
                    .collect(),
            }],
            connections: vec![],
            unknown: vec![],
        },
        live: None,
        frames: vec![],
        dependencies: vec![],
        dependency_complete: true,
        inputs_delivered: 0,
        elapsed_physics_frames: 0,
        failures: vec![],
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(
        std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(512)
    ))]

    #[test]
    fn arbitrary_intent_bytes_never_escape_typed_decoder(bytes in prop::collection::vec(any::<u8>(), 0..8192)) {
        if let Ok(spec) = validate::decode(&bytes) {
            prop_assert!(compile(&spec).is_ok());
        }
    }

    #[test]
    fn arbitrary_plan_wire_never_panics(bytes in prop::collection::vec(any::<u8>(), 0..8192)) {
        let _: Result<PlanRequest, _> = strict_decode(&bytes);
    }

    #[test]
    fn native_request_validation_is_total(
        nonce in "[A-Za-z0-9_]{0,100}",
        scene in "[A-Za-z0-9_./:-]{0,120}",
        ticks in 0u32..5000,
        action in "[a-z0-9_]{0,64}",
        pressed in any::<bool>(),
    ) {
        let request = NativeRequest {
            version: NATIVE_VERSION,
            nonce,
            source_fingerprint: digest("source"),
            mode: ProbeMode::Play,
            scene,
            ticks,
            inputs: vec![InputStep { tick: ticks, action, pressed }],
            checkpoints: if ticks == 0 { vec![] } else { vec![ticks] },
            variables: vec![],
            capture: false,
        };
        let declared: BTreeSet<String> = ["left".to_owned(), "right".to_owned()].into();
        let _ = request.validate(&declared);
    }

    #[test]
    fn cursor_parser_is_snapshot_bound_and_total(
        cursor in "\\PC{0,180}",
        limit in any::<u16>(),
    ) {
        let observation = cursor_observation();
        let source = digest("source");
        let _ = track_page(&observation, &source, Some(&cursor), limit);
    }

    #[test]
    fn key_cursor_and_selector_parser_are_total(
        cursor in "\\PC{0,180}",
        player in "\\PC{0,128}",
        library in "\\PC{0,96}",
        animation in "\\PC{0,96}",
        track_index in 0u32..4096,
        limit in any::<u16>(),
    ) {
        let observation = cursor_observation();
        let source = digest("source");
        let _ = key_page(
            &observation,
            &source,
            &player,
            &library,
            &animation,
            track_index,
            Some(&cursor),
            limit,
        );
    }
}

#[test]
fn rust_and_fixed_probe_share_hard_collection_budgets() {
    for expected in [
        format!("const MAX_NODES: int = {MAX_NATIVE_NODES}"),
        format!("const MAX_TRACKS: int = {MAX_NATIVE_TRACKS}"),
        format!("const MAX_KEYS: int = {MAX_NATIVE_KEYS}"),
    ] {
        assert!(PROBE_SOURCE.contains(&expected), "{expected}");
    }
}
