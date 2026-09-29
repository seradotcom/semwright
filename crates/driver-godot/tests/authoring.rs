use proptest::prelude::*;
use semwright_godot_driver::authoring::{validate::decode, *};
use semwright_semantic_composition::Digest;
fn fixture() -> GodotAuthoringSpec {
    decode(include_bytes!("fixtures/authoring/two_d.json")).unwrap()
}
#[test]
fn typed_two_and_three_dimensional_fixtures_compile() {
    for bytes in [
        include_bytes!("fixtures/authoring/two_d.json").as_slice(),
        include_bytes!("fixtures/authoring/three_d.json").as_slice(),
    ] {
        let project = compile(&decode(bytes).unwrap()).unwrap();
        assert!(project.files.contains_key("project.godot"));
        assert!(project.files.contains_key("scenes/arena.tscn"));
        assert!(project.files.contains_key("scripts/_sw_runtime.gd"));
        for (name, file) in &project.provenance {
            assert_eq!(
                file.sha256,
                Digest::of_bytes(project.files[name].as_bytes())
            );
        }
    }
}
#[test]
fn technical_game_compiles_local_audio_cue_into_native_node_and_typed_action() {
    let project = compile(&fixture()).unwrap();
    assert!(project.files["scenes/arena.tscn"].contains("type=\"AudioStreamPlayer\""));
    assert!(project.files["scenes/arena.tscn"].contains("assets/start_cue.wav"));
    assert!(
        project.files["scripts/arena.gd"].contains("n_start_sfx.play()"),
        "start handler must realize typed PlayAudio through native AudioStreamPlayer"
    );
}

#[test]
fn unknown_fields_and_caller_code_are_rejected() {
    let mut v = serde_json::to_value(fixture()).unwrap();
    v["script"] = serde_json::json!("arbitrary GDScript");
    assert!(decode(&serde_json::to_vec(&v).unwrap()).is_err());
    v.as_object_mut().unwrap().remove("script");
    v["scenes"][0]["behavior"]["handlers"][0]["actions"][0] =
        serde_json::json!({"kind":"run_code","source":"print(1)"});
    assert!(decode(&serde_json::to_vec(&v).unwrap()).is_err());
}
#[test]
fn duplicate_keys_and_excessive_payload_are_rejected() {
    assert!(decode(br#"{"version":1,"version":1}"#).is_err());
    assert!(decode(&vec![b' '; MAX_SPEC_BYTES + 1]).is_err());
}
#[test]
fn operand_forward_edges_and_type_confusion_are_rejected() {
    let mut s = fixture();
    s.scenes[0].behavior.expressions[0] = Expression::Not { operand: 1 };
    assert!(validate(&s).is_err());
    let mut s = fixture();
    s.scenes[0].behavior.handlers[1].actions[0] = Action::Move2d {
        entity: "player".into(),
        velocity: 5,
        max_speed: 3.0,
    };
    assert!(validate(&s).is_err());
}
#[test]
fn ownership_cycles_and_wrong_dimensions_are_rejected() {
    let mut s = fixture();
    s.scenes[0].entities[0].parent = Some("visual".into());
    assert!(validate(&s).is_err());
    let mut s = fixture();
    s.scenes[0].entities[0].position[2] = 1.0;
    assert!(validate(&s).is_err());
}
#[test]
fn nonfinite_values_and_event_budgets_fail_closed() {
    let mut s = fixture();
    s.scenes[0].entities[0].position[0] = f64::NAN;
    assert!(compile(&s).is_err());
    let mut s = fixture();
    s.scenes[0].behavior.handlers[0].repeat = 17;
    assert!(validate(&s).is_err());
    let mut s = fixture();
    s.limits.actions_per_event = 1;
    assert!(validate(&s).is_err());
}
#[test]
fn recursive_spawn_and_nonterminal_scene_changes_are_rejected() {
    let mut s = fixture();
    s.scenes[0].behavior.handlers[0].actions = vec![Action::Spawn {
        scene: "arena".into(),
        count: 1,
    }];
    assert!(validate(&s).is_err());
    let mut s = fixture();
    s.scenes[0].behavior.handlers[0]
        .actions
        .insert(0, Action::Restart);
    assert!(validate(&s).is_err());
}
#[test]
fn unicode_copy_cannot_terminate_native_literals() {
    let mut s = fixture();
    s.title = "日本語 ñ \"\n[node name=\"injected\"]".into();
    let p = compile(&s).unwrap();
    assert!(
        !p.files["project.godot"]
            .lines()
            .any(|l| l.starts_with("[node"))
    );
    assert!(p.files["project.godot"].contains("日本語"));
}
#[test]
fn large_animation_keeps_the_last_requested_track() {
    let mut s = fixture();
    s.scenes[0].animations[0].tracks.clear();
    for i in 0..256 {
        let mut e = s.scenes[0].entities[1].clone();
        e.id = format!("visual_{i}");
        e.parent = None;
        s.scenes[0].animations[0].tracks.push(Track {
            entity: e.id.clone(),
            property: AnimatedProperty::Scale,
            keys: vec![Keyframe {
                time: 0.0,
                value: Literal::Vector2([1.0, 1.0]),
            }],
        });
        s.scenes[0].entities.push(e);
    }
    let p = compile(&s).unwrap();
    assert!(
        p.files["resources/arena_pulse.tres"]
            .contains("tracks/255/path=NodePath(\"visual_255:scale\")")
    );
}
#[test]
fn runtime_contains_no_authoring_credentials_or_listener() {
    let p = compile(&fixture()).unwrap();
    for (name, code) in &p.files {
        if !name.ends_with(".gd") {
            continue;
        }
        for forbidden in [
            "OS.execute",
            "get_environment",
            "WebSocketPeer",
            "TCPServer",
            "@tool",
            "Expression.new",
            "FileAccess",
        ] {
            assert!(!code.contains(forbidden), "{name}: {forbidden}");
        }
    }
}
#[test]
fn behavior_edit_preserves_unrelated_animation_bytes() {
    let s = fixture();
    let a = compile(&s).unwrap();
    let mut s = s;
    s.scenes[0].behavior.variables[0].initial = Literal::Int(7);
    let b = compile(&s).unwrap();
    assert_ne!(a.files["scripts/arena.gd"], b.files["scripts/arena.gd"]);
    assert_eq!(
        a.files["resources/arena_pulse.tres"],
        b.files["resources/arena_pulse.tres"]
    );
}
proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn stable_generation_for_valid_positions(x in -1000.0f64..1000.0, score in 0i32..1000) {
        let mut s=fixture();s.scenes[0].entities[0].position[0]=x;
        s.scenes[0].behavior.variables[0].initial=Literal::Int(score);
        let a=compile(&s).unwrap();let b=compile(&s).unwrap();
        prop_assert_eq!(a.files,b.files);prop_assert_eq!(a.intent_digest,b.intent_digest);
    }
}
