    // I fixture: replace the first E asset with the same E island's typed
    // transform revision. Keep every preceding D12 assertion and add checks.
    let revised_digest = std::env::var("E_REVISION_GLB_SHA256").unwrap();
    semwright_semantic_composition::Digest::parse(revised_digest.clone()).unwrap();
    assert_ne!(e_digest, revised_digest);
    let second_handoff = broker_call(&handoff_broker, &session, "artifact.handoff", json!({
        "source_root":"e-blender-output","source_path":"revised_articulated.glb",
        "destination_root":"godot-authoring-input","destination_path":"revised_articulated.glb",
        "expected_sha256":revised_digest,"max_bytes":16_777_216,
        "semantic_type":"model/3d","media_type":"model/gltf-binary"
    })).await;
    assert_eq!(second_handoff["copied"], true);
    assert_eq!(second_handoff["atomic"], true);
    let mut second_spec = replacement_spec.clone();
    second_spec["assets"][0]["file"] = json!("revised_articulated.glb");
    second_spec["assets"][0]["sha256"] = json!(revised_digest);
    let second_plan = broker_call(&host.broker, &session, "driver.godot.composition.plan",
        json!({"spec":second_spec})).await;
    let second_id = second_plan["plan_id"].as_str().unwrap().to_owned();
    assert!(!second_plan["writes"].as_array().unwrap().iter()
        .any(|path|path.as_str()==Some("scripts/arena.gd")));
    let second_apply = broker_call(&host.broker, &session, "driver.godot.composition.apply",
        json!({"plan_id":second_id})).await;
    assert_eq!(second_apply["execution_status"], "completed");
    assert_eq!(digest(&behavior_path), baseline_behavior_sha);
    assert_eq!(digest(&product_project.join("assets/articulated.glb")), e_digest);
    assert_eq!(digest(&product_project.join("assets/revised_articulated.glb")), revised_digest);
    let second_snapshot = broker_call(&host.broker, &session, "driver.godot.composition.inspect",
        json!({"project":"cross_app_articulated"})).await;
    assert_eq!(second_snapshot["status"], "IN_SYNC");
    assert_eq!(second_snapshot["bindings"], baseline_bindings);
    let second_inspected = broker_call(&host.broker, &session, "driver.godot.composition.native.verify",
        json!({"plan_id":second_id,"scene":"arena","verification":{"kind":"inspect"}})).await;
    assert_eq!(gameplay_collision_signature(&second_inspected), baseline_collision);
    assert!(matches!(effect_rule_verdict(&second_inspected,"godot.native_readback.arena.v1").unwrap(), "PASS"|"UNKNOWN"));
    assert!(second_inspected["observation"]["dependencies"].as_array().unwrap().iter().any(|dependency|
        dependency["path"].as_str().is_some_and(|path|path.ends_with("assets/revised_articulated.glb"))
        && dependency["exists"]==true && dependency["sha256"]==revised_digest));
    // Blender bakes the skinned object's translation into mesh POSITION data.
    // Follow the actual node -> mesh resource reference and read native AABB;
    // the local MeshInstance3D transform is expected to remain unchanged.
    let body_bounds = |report: &Value| {
        let rows = report["observation"]["authored"]["nodes"].as_array().unwrap();
        let root=managed_native_node(report,"arena/imported_model");
        let prefix=format!("{}/",root["path"].as_str().unwrap());
        let body=rows.iter().filter(|row|row["class"]=="MeshInstance3D" && row["path"].as_str()
            .is_some_and(|path|path.starts_with(&prefix)&&path.ends_with("_body"))).collect::<Vec<_>>();
        assert_eq!(body.len(),1,"exactly one E body in native Godot readback");
        let reference=&body[0]["properties"]["mesh"];
        assert_eq!(reference["type"],"resource");
        let binding=format!("{}:mesh",body[0]["path"].as_str().unwrap());
        let meshes=report["observation"]["authored"]["resources"].as_array().unwrap().iter()
            .filter(|row|row["binding"]==binding && row["resource"]==reference["value"])
            .collect::<Vec<_>>();
        assert_eq!(meshes.len(),1,"native mesh reference must resolve exactly once");
        let properties=&meshes[0]["properties"];
        assert_eq!(properties["bounds_position"]["type"],"vector3");
        assert_eq!(properties["bounds_size"]["type"],"vector3");
        let position=properties["bounds_position"]["value"].as_array().unwrap();
        let size=properties["bounds_size"]["value"].as_array().unwrap();
        assert_eq!(position.len(),3);assert_eq!(size.len(),3);
        let center=std::array::from_fn::<f64,3,_>(|i|position[i].as_f64().unwrap()+size[i].as_f64().unwrap()/2.0);
        let extent=std::array::from_fn::<f64,3,_>(|i|size[i].as_f64().unwrap());
        assert!(center.iter().chain(extent.iter()).all(|v|v.is_finite()&&v.abs()<100.0));
        assert!(extent.iter().all(|v|*v>0.0));
        (center,extent,body[0]["properties"]["transform"].clone())
    };
    let (first_center,first_size,first_transform)=body_bounds(&inspected);
    let (second_center,second_size,second_transform)=body_bounds(&second_inspected);
    eprintln!("I_NATIVE_BODY_BOUNDS initial={first_center:?} revised={second_center:?}");
    assert_eq!(first_transform,second_transform,"skinned mesh local transform remains unchanged");
    assert!((second_center[0]-first_center[0]-0.2).abs()<0.0001,
        "typed Blender translation must reach actual Godot mesh bounds readback");
    for i in 0..3 {assert!((first_size[i]-second_size[i]).abs()<0.0001);}
    for i in 1..3 {assert!((first_center[i]-second_center[i]).abs()<0.0001);}
    let second_persisted = broker_call(&host.broker, &session, "driver.godot.composition.native.verify",
        json!({"plan_id":second_id,"scene":"arena","verification":{"kind":"persistence"}})).await;
    assert!(effect_rule_passes(&second_persisted,"godot.native_persistence.arena.v1"));
    assert_ne!(second_persisted["writer"]["process_id"],second_persisted["reader"]["process_id"]);
    let second_played = broker_call(&host.broker, &session, "driver.godot.composition.native.verify", json!({
        "plan_id":second_id,"scene":"arena","verification":{
            "kind":"play","ticks":12,"inputs":[
                {"tick":1,"action":"start","pressed":true},{"tick":2,"action":"start","pressed":false},
                {"tick":2,"action":"right","pressed":true},{"tick":9,"action":"right","pressed":false}],
            "checkpoints":[1,2,9,12],"variables":["score"],"capture":false}
    })).await;
    assert!(effect_rule_passes(&second_played,"godot.native_runtime.arena.v1"));
    assert_eq!(second_played["observation"]["inputs_delivered"],4);
    let second_frames=second_played["observation"]["frames"].as_array().unwrap();
    assert_eq!(second_frames.len(),4);
    assert_eq!(second_frames.last().unwrap()["state"],last["state"]);
    assert_eq!(second_frames.last().unwrap()["variables"],last["variables"]);
    assert!(second_frames.last().unwrap()["fault"].is_null());
    let second_verified = broker_call(&host.broker,&session,"driver.godot.composition.verify",
        json!({"plan_id":second_id})).await;
    let second_receipt=&second_verified["receipt"];
    assert_eq!(second_receipt["operation"]["capability"],"driver.godot.composition.apply");
    assert!(second_receipt["outputs"].as_array().unwrap().iter()
        .any(|output|output["fingerprint"]["bytes"]==revised_digest));
    assert_eq!(second_receipt["coverage"]["complete"],false);
    let second_runtime=broker_call(&host.broker,&session,"driver.godot.project.run_test",
        json!({"managed_project":"cross_app_articulated","frames":10})).await;
    assert_eq!(second_runtime["success"],true);
    let joint_export=broker_call_with_native_diagnostic(&host.broker,&session,"driver.godot.export.build",
        json!({"managed_project":"cross_app_articulated","preset":"Linux",
            "output":"joint_revised.x86_64","debug":false}),host.fixture._state.path()).await;
    assert_eq!(joint_export["success"],true);
    let joint_binary=host.fixture.artifacts.path().join("joint_revised.x86_64");
    assert!(joint_binary.is_file());
    let joint_home=tempfile::tempdir().unwrap();
    let joint_launch=Command::new("/usr/bin/timeout")
        .args(["5",joint_binary.to_str().unwrap(),"--headless"])
        .env_clear().env("HOME",joint_home.path()).output().unwrap();
    assert!(joint_launch.status.success()||joint_launch.status.code()==Some(124));
    let joint_launch_log=format!("{}\n{}",String::from_utf8_lossy(&joint_launch.stdout),String::from_utf8_lossy(&joint_launch.stderr));
    assert!(!joint_launch_log.contains("SCRIPT ERROR:")&&!joint_launch_log.contains("Parse Error:"));
    let joint_out=PathBuf::from(std::env::var("SEMWRIGHT_TEST_JOINT_D_OUT").unwrap());
    std::fs::create_dir_all(&joint_out).unwrap();
    for (name,value) in [("initial-native.json",&inspected),("revised-native.json",&second_inspected),
        ("initial-receipt.json",receipt),("revised-receipt.json",second_receipt)] {
        std::fs::write(joint_out.join(name),serde_json::to_vec_pretty(value).unwrap()).unwrap();
    }
    std::fs::write(joint_out.join("joint-d.json"),serde_json::to_vec_pretty(&json!({
        "source_sha":std::env::var("SEMWRIGHT_TEST_SOURCE_SHA").unwrap(),
        "suite_sha":std::env::var("GITHUB_SHA").unwrap(),
        "e_initial_glb_sha256":e_digest,"e_revised_glb_sha256":revised_digest,
        "initial_body_bounds_center":first_center,"revised_body_bounds_center":second_center,
        "native_revision_measurement":"mesh-aabb-center-linked-by-body-node-resource-reference",
        "skinned_body_local_transform_unchanged":true,
        "native_asset_revision_verified":true,"behavior_preserved":true,"collision_mapping_preserved":true,
        "fresh_persistence_verified":true,"standalone_export_launch_verified":true,
        "coverage_complete":false,"godot_frame_capture_verified":false,"r16_closed":false
    })).unwrap()).unwrap();
