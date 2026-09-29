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
        include_bytes!("fixtures/authoring/resource_sharing.json").as_slice(),
        include_bytes!("fixtures/authoring/animation_graphs.json").as_slice(),
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
    let script = &project.files["scripts/arena.gd"];
    assert!(
        script.contains("n_start_sfx.play()"),
        "start handler must realize typed PlayAudio through native AudioStreamPlayer"
    );
    assert!(
        script.contains("sw_state = \"play\""),
        "typed start transition must compile to the expected String state"
    );
    assert!(
        script.contains("var v_score: int = 0"),
        "typed initial score must compile deterministically"
    );
}

#[test]
fn unknown_fields_and_caller_code_are_rejected() {
    let baseline = serde_json::to_value(fixture()).unwrap();

    let mut script = baseline.clone();
    script["script"] = serde_json::json!("arbitrary GDScript");
    assert!(decode(&serde_json::to_vec(&script).unwrap()).is_err());

    let mut run_code = baseline.clone();
    run_code["scenes"][0]["behavior"]["handlers"][0]["actions"][0] =
        serde_json::json!({"kind":"run_code","source":"print(1)"});
    assert!(decode(&serde_json::to_vec(&run_code).unwrap()).is_err());

    let mut method = baseline.clone();
    method["scenes"][0]["behavior"]["handlers"][0]["actions"][0] = serde_json::json!({
        "kind":"call_method","entity":"player","method":"_notification","args":[1001]
    });
    assert!(decode(&serde_json::to_vec(&method).unwrap()).is_err());

    let mut callback = baseline.clone();
    callback["scenes"][0]["behavior"]["handlers"][0]["callback"] =
        serde_json::json!("res://evil.gd::_ready");
    assert!(decode(&serde_json::to_vec(&callback).unwrap()).is_err());

    let mut plugin = baseline;
    plugin["plugins"] = serde_json::json!([
        {"url":"https://example.invalid/addon.zip","autoload":"Remote"}
    ]);
    assert!(decode(&serde_json::to_vec(&plugin).unwrap()).is_err());
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
fn animation_tree_state_machine_and_blend_space_are_typed_and_deterministic() {
    let bytes = include_bytes!("fixtures/authoring/animation_graphs.json");
    let spec = decode(bytes).unwrap();
    let project = compile(&spec).unwrap();
    let scene = &project.files["scenes/arena.tscn"];
    let script = &project.files["scripts/arena.gd"];

    assert!(scene.contains("type=\"AnimationTree\""));
    assert!(scene.contains("type=\"AnimationNodeStateMachine\""));
    assert!(scene.contains("type=\"AnimationNodeStateMachineTransition\""));
    assert!(scene.contains("advance_mode=1"));
    assert!(scene.contains("xfade_time=0.15"));
    assert!(!scene.contains("advance_expression"));
    assert!(scene.contains("type=\"AnimationNodeBlendSpace1D\""));
    assert!(scene.contains("sync_mode=1"));
    assert!(scene.contains("blend_point_1/pos=1.0"));
    assert!(script.contains("_sw_animation_state(ag_motion, &\"run\")"));
    assert!(script.contains("_sw_animation_blend(ag_speed, float(e[0]))"));
    assert!(script.contains("_sw_animation_start(ag_motion, &\"idle\")"));

    let again = compile(&spec).unwrap();
    assert_eq!(project.files, again.files);
    assert_eq!(project.intent_digest, again.intent_digest);

    let mut wrong_action = spec.clone();
    wrong_action.scenes[0].behavior.handlers[0].actions[0] = Action::AnimationState {
        graph: "speed".into(),
        state: "run".into(),
    };
    assert!(validate(&wrong_action).is_err());

    let mut missing_cycle = spec;
    let AnimationGraphRoot::BlendSpace1d {
        sync_mode,
        cyclic_length,
        ..
    } = &mut missing_cycle.scenes[0].animation_graphs[1].root
    else {
        panic!("fixture blend graph changed");
    };
    *sync_mode = AnimationBlendSyncMode::CyclicConstant;
    *cyclic_length = None;
    assert!(validate(&missing_cycle).is_err());

    let mut injected: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    injected["scenes"][0]["animation_graphs"][0]["root"]["transitions"][0]["advance_expression"] =
        serde_json::json!("OS.execute('no')");
    assert!(decode(&serde_json::to_vec(&injected).unwrap()).is_err());
}

#[test]
fn shared_material_and_local_to_scene_copy_are_explicit_and_incremental() {
    let bytes = include_bytes!("fixtures/authoring/resource_sharing.json");
    let spec = decode(bytes).unwrap();
    let first = compile(&spec).unwrap();
    let scene = &first.files["scenes/arena.tscn"];
    assert_eq!(
        scene
            .matches("ExtResource(\"shared_material_bronze\")")
            .count(),
        2
    );
    assert!(scene.contains("ExtResource(\"local_material_local_a\")"));
    assert!(scene.contains("ExtResource(\"local_material_local_b\")"));

    let shared = &first.files["resources/arena_material_bronze.tres"];
    let local_a = &first.files["resources/arena_local_a_material_local.tres"];
    let local_b = &first.files["resources/arena_local_b_material_local.tres"];
    assert!(shared.contains("resource_local_to_scene=false"));
    assert!(local_a.contains("resource_local_to_scene=true"));
    assert!(local_b.contains("resource_local_to_scene=true"));
    assert_ne!(local_a, local_b);

    let mut changed = spec.clone();
    let binding = match &mut changed.scenes[0].entities[2].node {
        NativeNode::Mesh3dMaterial { material, .. } => material,
        other => panic!("unexpected local material fixture node: {other:?}"),
    };
    binding.color_override = Some([0.2, 0.9, 0.3, 1.0]);
    let second = compile(&changed).unwrap();
    assert_eq!(
        first.files["resources/arena_material_bronze.tres"],
        second.files["resources/arena_material_bronze.tres"]
    );
    assert_eq!(
        first.files["resources/arena_local_b_material_local.tres"],
        second.files["resources/arena_local_b_material_local.tres"]
    );
    assert_ne!(
        first.files["resources/arena_local_a_material_local.tres"],
        second.files["resources/arena_local_a_material_local.tres"]
    );

    let mut illegal = spec;
    let shared_binding = match &mut illegal.scenes[0].entities[0].node {
        NativeNode::Mesh3dMaterial { material, .. } => material,
        other => panic!("unexpected shared material fixture node: {other:?}"),
    };
    shared_binding.color_override = Some([1.0, 0.0, 0.0, 1.0]);
    assert!(validate(&illegal).is_err());
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
