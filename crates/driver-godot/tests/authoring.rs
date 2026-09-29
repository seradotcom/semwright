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
fn typed_transform_and_reparent_actions_are_bounded_and_dimension_checked() {
    let mut value: serde_json::Value =
        serde_json::from_slice(include_bytes!("fixtures/authoring/two_d.json")).unwrap();
    let expressions = value["scenes"][0]["behavior"]["expressions"]
        .as_array_mut()
        .unwrap();
    expressions.push(serde_json::json!({
        "kind":"literal",
        "value":{"kind":"scalar","value":0.25}
    }));
    expressions.push(serde_json::json!({
        "kind":"literal",
        "value":{"kind":"scalar","value":1.5}
    }));
    expressions.push(serde_json::json!({"kind":"vector2","x":11,"y":11}));

    let actions = value["scenes"][0]["behavior"]["handlers"][0]["actions"]
        .as_array_mut()
        .unwrap();
    actions.push(serde_json::json!({
        "kind":"rotation","entity":"player","value":10
    }));
    actions.push(serde_json::json!({
        "kind":"scale","entity":"visual","value":12
    }));
    actions.push(serde_json::json!({
        "kind":"reparent",
        "entity":"collectible_visual",
        "parent":"player",
        "keep_global":true
    }));

    let spec = decode(&serde_json::to_vec(&value).unwrap()).unwrap();
    let project = compile(&spec).unwrap();
    let script = &project.files["scripts/arena.gd"];
    assert!(script.contains("_sw_rotation(n_player, e[10])"));
    assert!(script.contains("_sw_scale(n_visual, e[12])"));
    assert!(script.contains("_sw_reparent(n_collectible_visual, n_player, true)"));
    let runtime = &project.files["scripts/_sw_runtime.gd"];
    assert!(runtime.contains("func _sw_rotation(node: Node, value: Variant) -> bool:"));
    assert!(runtime.contains("func _sw_scale(node: Node, value: Variant) -> bool:"));
    assert!(
        runtime.contains("func _sw_reparent(node: Node, parent: Node, keep_global: bool) -> bool:")
    );
    assert!(runtime.contains("scale_zero_component"));
    assert!(runtime.contains("scale_mixed_sign"));
    assert!(!runtime.contains("set_script("));
    assert!(!runtime.contains("OS.execute"));

    let mut descendant = value.clone();
    descendant["scenes"][0]["behavior"]["handlers"][0]["actions"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "kind":"reparent",
            "entity":"player",
            "parent":"visual",
            "keep_global":true
        }));
    assert!(decode(&serde_json::to_vec(&descendant).unwrap()).is_err());

    let mut wrong_rotation = value.clone();
    wrong_rotation["scenes"][0]["behavior"]["handlers"][0]["actions"][3]["value"] =
        serde_json::json!(12);
    assert!(decode(&serde_json::to_vec(&wrong_rotation).unwrap()).is_err());

    let mut nonspatial_keep_global = value;
    nonspatial_keep_global["scenes"][0]["behavior"]["handlers"][0]["actions"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "kind":"reparent",
            "entity":"hud",
            "parent":"player",
            "keep_global":true
        }));
    assert!(decode(&serde_json::to_vec(&nonspatial_keep_global).unwrap()).is_err());

    let mut three_d: serde_json::Value =
        serde_json::from_slice(include_bytes!("fixtures/authoring/three_d.json")).unwrap();
    let expressions = three_d["scenes"][0]["behavior"]["expressions"]
        .as_array_mut()
        .unwrap();
    expressions.push(serde_json::json!({
        "kind":"literal","value":{"kind":"scalar","value":0.1}
    }));
    expressions.push(serde_json::json!({
        "kind":"literal","value":{"kind":"scalar","value":0.2}
    }));
    expressions.push(serde_json::json!({
        "kind":"literal","value":{"kind":"scalar","value":0.3}
    }));
    expressions.push(serde_json::json!({
        "kind":"vector3","x":10,"y":11,"z":12
    }));
    expressions.push(serde_json::json!({
        "kind":"literal","value":{"kind":"scalar","value":1.2}
    }));
    expressions.push(serde_json::json!({
        "kind":"vector3","x":14,"y":14,"z":14
    }));
    let actions = three_d["scenes"][0]["behavior"]["handlers"][0]["actions"]
        .as_array_mut()
        .unwrap();
    actions.push(serde_json::json!({
        "kind":"rotation","entity":"player","value":13
    }));
    actions.push(serde_json::json!({
        "kind":"scale","entity":"visual","value":15
    }));
    let project_3d = compile(&decode(&serde_json::to_vec(&three_d).unwrap()).unwrap()).unwrap();
    let script_3d = &project_3d.files["scripts/arena.gd"];
    assert!(script_3d.contains("_sw_rotation(n_player, e[13])"));
    assert!(script_3d.contains("_sw_scale(n_visual, e[15])"));
}

#[test]
fn typed_lerp_expression_is_bounded_and_type_checked() {
    let mut value: serde_json::Value =
        serde_json::from_slice(include_bytes!("fixtures/authoring/two_d.json")).unwrap();
    let expressions = value["scenes"][0]["behavior"]["expressions"]
        .as_array_mut()
        .unwrap();
    expressions.push(serde_json::json!({
        "kind":"literal","value":{"kind":"scalar","value":10.0}
    }));
    expressions.push(serde_json::json!({
        "kind":"literal","value":{"kind":"scalar","value":20.0}
    }));
    expressions.push(serde_json::json!({
        "kind":"literal","value":{"kind":"scalar","value":0.25}
    }));
    expressions.push(serde_json::json!({
        "kind":"lerp","from":10,"to":11,"weight":12
    }));

    let spec = decode(&serde_json::to_vec(&value).unwrap()).unwrap();
    let project = compile(&spec).unwrap();
    let script = &project.files["scripts/arena.gd"];
    assert!(script.contains("if e[12] < 0.0 or e[12] > 1.0:"));
    assert!(script.contains("lerp(e[10], e[11], float(e[12]))"));
    assert!(script.contains("_sw_fail(\"lerp_weight\")"));

    let mut endpoint_mismatch = value.clone();
    endpoint_mismatch["scenes"][0]["behavior"]["expressions"][13]["to"] = serde_json::json!(4);
    assert!(decode(&serde_json::to_vec(&endpoint_mismatch).unwrap()).is_err());

    let mut weight_type = value.clone();
    weight_type["scenes"][0]["behavior"]["expressions"][13]["weight"] = serde_json::json!(6);
    assert!(decode(&serde_json::to_vec(&weight_type).unwrap()).is_err());

    let mut out_of_range = value;
    out_of_range["scenes"][0]["behavior"]["expressions"][12]["value"]["value"] =
        serde_json::json!(1.25);
    assert!(decode(&serde_json::to_vec(&out_of_range).unwrap()).is_err());
}

#[test]
fn typed_acceleration_actions_are_physics_tick_only_and_dimension_checked() {
    let mut two_d = fixture();
    let y = two_d.scenes[0].behavior.expressions.len() as u16;
    two_d.scenes[0]
        .behavior
        .expressions
        .push(Expression::Literal {
            value: Literal::Scalar(60.0),
        });
    let acceleration = two_d.scenes[0].behavior.expressions.len() as u16;
    two_d.scenes[0]
        .behavior
        .expressions
        .push(Expression::Vector2 { x: 0, y });
    let physics = two_d.scenes[0]
        .behavior
        .handlers
        .iter_mut()
        .find(|handler| matches!(handler.event, Event::PhysicsTick))
        .expect("2D physics handler");
    physics.actions.push(Action::Accelerate2d {
        entity: "player".into(),
        acceleration,
        max_speed: 120.0,
    });
    validate(&two_d).unwrap();
    let project = compile(&two_d).unwrap();
    assert!(project.files["scripts/arena.gd"].contains("Vector2(n_player.velocity) + Vector2(e["));
    assert!(
        project.files["scripts/arena.gd"]
            .contains("get_physics_process_delta_time()).limit_length(120.0)")
    );

    let mut wrong_event = two_d.clone();
    wrong_event.scenes[0]
        .behavior
        .handlers
        .iter_mut()
        .find(|handler| matches!(handler.event, Event::PhysicsTick))
        .expect("physics handler")
        .event = Event::Ready;
    assert!(validate(&wrong_event).is_err());

    let mut zero_limit = two_d.clone();
    let action = zero_limit.scenes[0]
        .behavior
        .handlers
        .iter_mut()
        .flat_map(|handler| handler.actions.iter_mut())
        .find(|action| matches!(action, Action::Accelerate2d { .. }))
        .expect("typed acceleration action");
    let Action::Accelerate2d { max_speed, .. } = action else {
        unreachable!()
    };
    *max_speed = 0.0;
    assert!(validate(&zero_limit).is_err());

    let mut three_d = decode(include_bytes!("fixtures/authoring/three_d.json")).unwrap();
    let y = three_d.scenes[0].behavior.expressions.len() as u16;
    three_d.scenes[0]
        .behavior
        .expressions
        .push(Expression::Literal {
            value: Literal::Scalar(-9.8),
        });
    let acceleration = three_d.scenes[0].behavior.expressions.len() as u16;
    three_d.scenes[0]
        .behavior
        .expressions
        .push(Expression::Vector3 { x: 0, y, z: 0 });
    three_d.scenes[0]
        .behavior
        .handlers
        .iter_mut()
        .find(|handler| matches!(handler.event, Event::PhysicsTick))
        .expect("3D physics handler")
        .actions
        .push(Action::Accelerate3d {
            entity: "player".into(),
            acceleration,
            max_speed: 12.0,
        });
    validate(&three_d).unwrap();
    let project = compile(&three_d).unwrap();
    assert!(project.files["scripts/arena.gd"].contains("Vector3(n_player.velocity) + Vector3(e["));
    assert!(
        project.files["scripts/arena.gd"]
            .contains("get_physics_process_delta_time()).limit_length(12.0)")
    );

    let mut wrong_dimension = two_d;
    let action = wrong_dimension.scenes[0]
        .behavior
        .handlers
        .iter_mut()
        .flat_map(|handler| handler.actions.iter_mut())
        .find(|action| matches!(action, Action::Accelerate2d { .. }))
        .expect("typed acceleration action");
    let (entity, acceleration, max_speed) = match action {
        Action::Accelerate2d {
            entity,
            acceleration,
            max_speed,
        } => (entity.clone(), *acceleration, *max_speed),
        _ => unreachable!(),
    };
    *action = Action::Accelerate3d {
        entity,
        acceleration,
        max_speed,
    };
    assert!(validate(&wrong_dimension).is_err());
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
