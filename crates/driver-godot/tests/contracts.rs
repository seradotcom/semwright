use semwright_driver_sdk::{Driver, descriptor_digest};
use semwright_godot_driver::{
    GodotDriver,
    catalog::{Catalog, Route},
    config::{Config, ProjectConfig},
    model::semantic_diff,
};
use serde_json::json;

#[test]
fn initial_catalog_is_strict_and_digests_are_stable() {
    let catalog = Catalog::load().unwrap();
    let caps = catalog.capabilities();
    assert!(caps.len() >= 15);
    for cap in caps {
        assert_eq!(cap.descriptor.input_schema["additionalProperties"], false);
        assert_eq!(cap.descriptor.output_schema["additionalProperties"], false);
        let entry = catalog.get(&cap.descriptor.name).unwrap();
        assert_eq!(entry.digest, descriptor_digest(&cap.descriptor).unwrap());
    }
}

#[test]
fn all_driver_schemas_fit_external_registry_budget() {
    let catalog = Catalog::load().unwrap();
    for capability in catalog.capabilities() {
        for (side, schema) in [
            ("input", &capability.descriptor.input_schema),
            ("output", &capability.descriptor.output_schema),
        ] {
            semwright_registry::bounds::schema_budget(schema, true).unwrap_or_else(|error| {
                panic!(
                    "{} {} schema exceeds external registry budget: {}",
                    capability.descriptor.name, side, error
                )
            });
        }
    }
}

#[test]
fn catalog_declares_generic_artifact_ports() {
    let catalog = Catalog::load().unwrap();
    for command in [
        "driver.godot.assets.rescan",
        "driver.godot.composition.plan",
    ] {
        let capability = catalog.get(command).unwrap();
        for expected in [
            "artifact-in:model/3d",
            "artifact-in:image/raster",
            "artifact-in:image/vector",
            "artifact-in:audio/sample",
        ] {
            assert!(
                capability.capability.tags.iter().any(|tag| tag == expected),
                "{command} missing {expected}"
            );
        }
    }

    for (name, expected) in [
        ("driver.godot.export.pack", "artifact-out:game/package"),
        (
            "driver.godot.export.build",
            "artifact-out:application/binary",
        ),
        ("driver.godot.movie.capture", "artifact-out:video/clip"),
    ] {
        let capability = catalog.get(name).unwrap();
        assert!(
            capability.capability.tags.iter().any(|tag| tag == expected),
            "{name} missing {expected}"
        );
    }
}

#[test]
fn diff_is_bounded_and_pointer_escaped() {
    let diff = semantic_diff(&json!({"a/b": 1}), &json!({"a/b": 2}));
    assert_eq!(diff["changes"][0]["path"], "/a~1b");
}

#[tokio::test]
async fn local_driver_routes_work() {
    let dir = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    std::fs::write(dir.path().join("project.godot"), "config_version=5\n").unwrap();
    let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = socket.local_addr().unwrap().port();
    drop(socket);
    let config = Config {
        port,
        development_mode: true,
        projects: vec![ProjectConfig {
            project: "a".repeat(64),
            root: dir.path().canonicalize().unwrap(),
            secret: "b".repeat(64),
        }],
        runner: None,
        authoring: None,
    };
    let mut driver = GodotDriver::new(config).await.unwrap();
    let catalog = Catalog::load().unwrap();
    let doctor = catalog.get("driver.godot.doctor").unwrap();
    let value = driver
        .execute(
            &doctor.capability.descriptor.name,
            &doctor.digest,
            json!({}),
        )
        .await
        .unwrap();
    assert_eq!(value["bridge"], 1);
}

#[test]
fn import_scalar_schema_accepts_integer_values() {
    let catalog = Catalog::load().unwrap();
    let entry = catalog.get("driver.godot.asset.import.configure").unwrap();
    let input = json!({
        "session": "a".repeat(32),
        "path": "res://assets/icon.svg",
        "params": [{"key": "compress/mode", "value": 0}],
        "expect": {"revision": 1, "fingerprint": "a".repeat(64)},
        "dry_run": false
    });
    entry.validate_input(&input).unwrap();
}

#[test]
fn translation_creation_requires_runtime_loadable_format() {
    let catalog = Catalog::load().unwrap();
    let entry = catalog.get("driver.godot.translation.create").unwrap();
    let base = json!({
        "session": "a".repeat(32),
        "path": "res://locale/es.translation",
        "locale": "es",
        "register": true,
        "expect": {"revision": 1, "fingerprint": "a".repeat(64)},
        "dry_run": false
    });
    entry.validate_input(&base).unwrap();

    let mut invalid = base;
    invalid["path"] = json!("res://locale/es.tres");
    assert!(entry.validate_input(&invalid).is_err());
}

#[test]
fn editor_mutations_enforce_optimistic_preconditions() {
    let source = include_str!("../../../integrations/godot/addons/semwright/ops/editor_ops.gd");
    for function in ["selection_set", "run_start", "run_stop"] {
        let marker = format!("static func {function}(");
        let start = source
            .find(&marker)
            .unwrap_or_else(|| panic!("missing editor handler {function}"));
        let tail = &source[start..];
        let end = tail[marker.len()..]
            .find("\nstatic func ")
            .map(|offset| marker.len() + offset)
            .unwrap_or(tail.len());
        let body = &tail[..end];
        assert!(
            body.contains("_check_expect(args)"),
            "{function} must enforce expect revision/fingerprint before mutation"
        );
    }
}

#[test]
fn api_introspection_contracts_are_read_only_and_bounded() {
    let catalog = Catalog::load().unwrap();
    for name in [
        "driver.godot.api.search",
        "driver.godot.api.describe",
        "driver.godot.project.class.list",
        "driver.godot.project.class.describe",
    ] {
        let entry = catalog.get(name).unwrap();
        assert_eq!(
            entry.capability.descriptor.input_schema["additionalProperties"],
            false
        );
        assert!(!entry.capability.descriptor.dry_run);
        assert!(!entry.mutation);
        let output = &entry.capability.descriptor.output_schema;
        assert_eq!(output["additionalProperties"], false);
        let data = &output["properties"]["data"];
        if let Some(branches) = data.get("oneOf").and_then(|value| value.as_array()) {
            assert!(
                branches
                    .iter()
                    .all(|branch| branch["additionalProperties"] == false),
                "{name} output variants must remain structurally closed"
            );
        } else {
            assert_eq!(
                data["additionalProperties"], false,
                "{name} data output must remain structurally closed"
            );
        }
    }

    let search = catalog.get("driver.godot.api.search").unwrap();
    search
        .validate_input(&json!({
            "session": "a".repeat(32),
            "query": "Camera",
            "base": "Node",
            "limit": 64
        }))
        .unwrap();
}

#[test]
fn expanded_variant_schema_accepts_structured_values_and_rejects_unknown_tags() {
    let catalog = Catalog::load().unwrap();
    let entry = catalog.get("driver.godot.node.patch").unwrap();
    let base = |value| {
        json!({
            "session": "a".repeat(32),
            "target": "Player",
            "properties": [{"name": "transform", "value": value}],
            "expect": {"revision": 1, "fingerprint": "a".repeat(64)},
            "dry_run": false
        })
    };

    for value in [
        json!({"$type":"Transform3D","value":[1,0,0,0,1,0,0,0,1,0,0,0]}),
        json!({"$type":"Quaternion","value":[0,0,0,1]}),
        json!({"$type":"PackedVector3Array","value":[[0,0,0],[1,2,3]]}),
        json!({"$type":"NodeRef","path":"Player/Camera"}),
        json!({"$type":"Array","value":[1,true,"semantic"]}),
        json!({"$type":"Dictionary","entries":[{"key":"mode","value":2}]}),
    ] {
        entry.validate_input(&base(value)).unwrap();
    }

    assert!(
        entry
            .validate_input(&base(json!({"$type":"ArbitraryObject","value":[]})))
            .is_err()
    );
    assert!(
        entry
            .validate_input(&base(json!({
                "$type":"Array",
                "value":[1,2,3],
                "length":1000,
                "truncated":true
            })))
            .is_err(),
        "truncated read envelopes must never be accepted as writes"
    );
}

#[test]
fn api_introspection_source_cannot_invoke_discovered_methods() {
    let source = include_str!("../../../integrations/godot/addons/semwright/ops/api_ops.gd");
    for forbidden in ["Object.call", ".callv(", "OS.execute", "Expression.execute"] {
        assert!(
            !source.contains(forbidden),
            "api introspection must remain descriptive-only: {forbidden}"
        );
    }
}

#[test]
fn provider_ref_contracts_are_strict_and_provider_bound() {
    let catalog = Catalog::load().unwrap();
    let issue = catalog.get("driver.godot.ref.node").unwrap();
    issue
        .validate_input(&json!({"session":"a".repeat(32),"path":"Player/Camera"}))
        .unwrap();

    let resolve = catalog.get("driver.godot.ref.resolve").unwrap();
    let valid_ref = json!({
        "provider":"godot",
        "project":"b".repeat(64),
        "session":"a".repeat(32),
        "generation":"c".repeat(32),
        "revision":1,
        "fingerprint":"d".repeat(64),
        "kind":"node",
        "path":"Player/Camera",
        "class":"Camera3D",
        "scene":"res://scenes/lab_room.tscn"
    });
    resolve
        .validate_input(&json!({
            "session":"a".repeat(32),
            "ref":valid_ref.clone(),
            "require_current":true
        }))
        .unwrap();

    let mut foreign = valid_ref.clone();
    foreign["provider"] = json!("other");
    assert!(
        resolve
            .validate_input(&json!({
                "session":"a".repeat(32),
                "ref":foreign,
                "require_current":false
            }))
            .is_err()
    );
}

#[test]
fn catalog_plugin_routes_have_editor_handlers() {
    use semwright_godot_driver::catalog::Route;
    let catalog = Catalog::load().unwrap();
    let plugin = include_str!("../../../integrations/godot/addons/semwright/plugin.gd");
    let names = catalog.names_for(Route::Plugin);
    assert_eq!(names.len(), 179);
    for name in names {
        let op = name.strip_prefix("driver.godot.").unwrap();
        let marker = format!("\"{op}\": return ");
        assert!(
            plugin.contains(&marker),
            "missing EditorPlugin handler for {name}"
        );
    }
}

#[test]
fn catalog_routes_partition_the_full_surface() {
    let catalog = Catalog::load().unwrap();
    assert_eq!(catalog.names_for(Route::Local).len(), 3);
    assert_eq!(catalog.names_for(Route::Plugin).len(), 179);
    assert_eq!(catalog.names_for(Route::Runner).len(), 6);
    assert_eq!(catalog.names_for(Route::Authoring).len(), 8);
    assert_eq!(catalog.names_for(Route::AuthoringRunner).len(), 3);
    assert_eq!(catalog.capabilities().len(), 199);
}

#[test]
fn gridmap_and_path_schemas_reject_out_of_domain_values() {
    let catalog = Catalog::load().unwrap();

    let grid = catalog.get("driver.godot.gridmap.cell.set").unwrap();
    let bad_orientation = json!({
        "session":"a".repeat(32),
        "target":"Grid",
        "position":[0,0,0],
        "item":0,
        "orientation":24,
        "expect":{"revision":1,"fingerprint":"a".repeat(64)},
        "dry_run":false
    });
    assert!(grid.validate_input(&bad_orientation).is_err());

    let follow = catalog.get("driver.godot.path.follow.configure").unwrap();
    let conflicting_progress = json!({
        "session":"a".repeat(32),
        "target":"Rail/Follower",
        "progress":5.0,
        "progress_ratio":0.5,
        "expect":{"revision":1,"fingerprint":"a".repeat(64)},
        "dry_run":false
    });
    // The cross-field conflict is intentionally enforced by the Godot handler,
    // while both values remain individually valid at the transport schema layer.
    follow.validate_input(&conflicting_progress).unwrap();
}

#[test]
fn animation_graph_schemas_separate_layout_from_blend_positions() {
    let catalog = Catalog::load().unwrap();
    let stamp = json!({"revision":1,"fingerprint":"a".repeat(64)});

    let add = catalog
        .get("driver.godot.animation_tree.blend_tree.node.add")
        .unwrap();
    add.validate_input(&json!({
        "session":"a".repeat(32),
        "tree":"AnimationTree",
        "graph":"BlendGraph",
        "name":"SpeedSpace",
        "kind":"blend_space_1d",
        "graph_position":[460.0,0.0],
        "min_space":-1.0,
        "max_space":1.0,
        "snap":0.1,
        "expect":stamp,
        "dry_run":false
    }))
    .unwrap();

    let ambiguous = json!({
        "session":"a".repeat(32),
        "tree":"AnimationTree",
        "graph":"BlendGraph",
        "name":"SpeedSpace",
        "kind":"blend_space_1d",
        "position":[460.0,0.0],
        "expect":{"revision":1,"fingerprint":"a".repeat(64)},
        "dry_run":false
    });
    assert!(add.validate_input(&ambiguous).is_err());

    let point = catalog
        .get("driver.godot.animation_tree.blend_space.point.add")
        .unwrap();
    point
        .validate_input(&json!({
            "session":"a".repeat(32),
            "tree":"AnimationTree",
            "graph":"BlendGraph/SpeedSpace",
            "name":"Slow",
            "kind":"animation",
            "position":-1.0,
            "animation":"door_open",
            "index":-1,
            "expect":{"revision":1,"fingerprint":"a".repeat(64)},
            "dry_run":false
        }))
        .unwrap();
}

#[test]
fn input_schema_accepts_gamepad_button_and_axis_bindings() {
    let catalog = Catalog::load().unwrap();
    let entry = catalog.get("driver.godot.input.set").unwrap();
    let input = json!({
        "session": "a".repeat(32),
        "name": "gamepad_move",
        "deadzone": 0.2,
        "events": [
            {"type":"joy_button","code":0,"device":-1},
            {"type":"joy_axis","axis":0,"value":1.0,"device":-1}
        ],
        "expect": {"revision": 1, "fingerprint": "a".repeat(64)},
        "dry_run": false
    });
    entry.validate_input(&input).unwrap();
}

#[test]
fn navigation_schema_accepts_2d_vectors_and_polygon_resources() {
    let catalog = Catalog::load().unwrap();
    let agent = catalog
        .get("driver.godot.navigation.agent.configure")
        .unwrap();
    agent
        .validate_input(&json!({
            "session":"a".repeat(32),
            "target":"TwoD/Player/Agent",
            "target_position":[120.0,80.0],
            "avoidance_enabled":true,
            "expect":{"revision":1,"fingerprint":"a".repeat(64)},
            "dry_run":false
        }))
        .unwrap();

    let region = catalog
        .get("driver.godot.navigation.region.configure")
        .unwrap();
    region
        .validate_input(&json!({
            "session":"a".repeat(32),
            "target":"TwoD/NavRegion",
            "navigation_polygon":"res://assets/navigation2d.tres",
            "expect":{"revision":1,"fingerprint":"a".repeat(64)},
            "dry_run":false
        }))
        .unwrap();
}

#[test]
fn physics_schema_accepts_2d_velocity_and_angular_scalar() {
    let catalog = Catalog::load().unwrap();
    let body = catalog.get("driver.godot.physics.body.configure").unwrap();
    body.validate_input(&json!({
        "session":"a".repeat(32),
        "target":"TwoD/Player",
        "linear_velocity":[10.0,20.0],
        "angular_velocity":1.5,
        "continuous_cd":1,
        "expect":{"revision":1,"fingerprint":"a".repeat(64)},
        "dry_run":false
    }))
    .unwrap();

    let area = catalog.get("driver.godot.physics.area.configure").unwrap();
    area.validate_input(&json!({
        "session":"a".repeat(32),
        "target":"TwoD/Area",
        "gravity_point":false,
        "gravity_direction":[0.0,1.0],
        "expect":{"revision":1,"fingerprint":"a".repeat(64)},
        "dry_run":false
    }))
    .unwrap();
}

#[test]
fn physics_area_schema_rejects_ambiguous_gravity_vector_modes() {
    let catalog = Catalog::load().unwrap();
    let area = catalog.get("driver.godot.physics.area.configure").unwrap();
    let ambiguous = json!({
        "session":"a".repeat(32),
        "target":"TwoD/Area",
        "gravity_point":false,
        "gravity_direction":[0.0,1.0],
        "gravity_point_center":[20.0,20.0],
        "expect":{"revision":1,"fingerprint":"a".repeat(64)},
        "dry_run":false
    });
    assert!(area.validate_input(&ambiguous).is_err());
}

#[test]
fn companion_list_exactly_tracks_the_editor_plugin_tree() {
    use std::{collections::BTreeSet, path::Path};

    fn collect(root: &Path, dir: &Path, out: &mut BTreeSet<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                collect(root, &path, out);
            } else if path.is_file() {
                out.insert(
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }

    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let plugin_root = crate_root
        .join("../../integrations/godot/addons/semwright")
        .canonicalize()
        .unwrap();
    let mut actual = BTreeSet::new();
    collect(&plugin_root, &plugin_root, &mut actual);

    let list = std::fs::read_to_string(crate_root.join("companions.list")).unwrap();
    let mut declared = BTreeSet::new();
    for line in list.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (destination, source) = line.split_once('=').unwrap();
        let relative = destination.strip_prefix("addons/semwright/").unwrap();
        assert!(declared.insert(relative.to_owned()));
        assert_eq!(
            crate_root.join(source).canonicalize().unwrap(),
            plugin_root.join(relative).canonicalize().unwrap()
        );
    }
    assert_eq!(declared, actual);
}

#[test]
fn companion_declares_bounded_session_resume_contract() {
    const PLUGIN: &str = include_str!("../../../integrations/godot/addons/semwright/plugin.gd");
    for required in [
        "RECONNECT_BASE_MS := 250",
        "RECONNECT_MAX_MS := 4000",
        "PRE_READY_FAILURE_LIMIT := 8",
        "hello[\"resume_session\"] = _session",
        "_authenticated_once = true",
        "_schedule_reconnect(\"socket closed\")",
        "kind == \"ping\"",
        "\"type\":\"pong\"",
    ] {
        assert!(
            PLUGIN.contains(required),
            "Godot companion lost continuity invariant: {required}"
        );
    }
}

#[test]
fn readback_inputs_are_bounded_and_legacy_inputs_remain_valid() {
    let catalog = Catalog::load().unwrap();
    let node = catalog.get("driver.godot.node.inspect").unwrap();
    let base = json!({"session":"a".repeat(32),"path":"Camera"});
    node.validate_input(&base).unwrap();
    for flag in [json!(true), json!(false)] {
        let mut input = base.clone();
        input["include_vertex_bounds"] = flag;
        node.validate_input(&input).unwrap();
    }
    for flag in [json!(1), json!("true"), json!(null)] {
        let mut input = base.clone();
        input["include_vertex_bounds"] = flag;
        assert!(node.validate_input(&input).is_err());
    }
    for selectors in [json!(["attributes"]), json!([])] {
        let mut input = base.clone();
        input["resource_properties"] = selectors;
        node.validate_input(&input).unwrap();
    }
    for selectors in [
        json!(["attributes", "attributes"]),
        json!([true]),
        json!([""]),
        json!(["a", "b", "c", "d", "e", "f", "g", "h", "i"]),
    ] {
        let mut input = base.clone();
        input["resource_properties"] = selectors;
        assert!(node.validate_input(&input).is_err());
    }
    let animation = catalog.get("driver.godot.animation.inspect").unwrap();
    let base = json!({"session":"a".repeat(32),"player":"AnimationPlayer"});
    animation.validate_input(&base).unwrap();
    for (offset, limit) in [(0, 0), (0, 16), (528, 12), (1000000, 64)] {
        let mut input = base.clone();
        input["keys_offset"] = json!(offset);
        input["keys_limit"] = json!(limit);
        animation.validate_input(&input).unwrap();
    }
    for (field, value) in [
        ("keys_offset", json!(-1)),
        ("keys_offset", json!(1000001)),
        ("keys_limit", json!(65)),
        ("keys_limit", json!(true)),
        ("method", json!("play")),
    ] {
        let mut input = base.clone();
        input[field] = value;
        assert!(animation.validate_input(&input).is_err());
    }
}

#[test]
fn readback_outputs_reject_partial_bounds_and_malformed_pages() {
    let catalog = Catalog::load().unwrap();
    let node = catalog.get("driver.godot.node.inspect").unwrap();
    let mut output = json!({"stamp":{"revision":0,"fingerprint":"a".repeat(64)},"data":{
        "path":"Mesh","name":"Mesh","class":"MeshInstance3D","ref":format!("native:{}","b".repeat(32)),"properties":{}}});
    node.validate_output(&output).unwrap();
    output["data"]["observed"] = json!({"scope":"edited_scene","owner_path":".","parent_path":".",
        "position":[0,0,0],"rotation":[0,0,0],"scale":[1,1,1],"global_position":[0,0,0],"rotation_order":2,
        "resource_properties":{"material_override":null},
        "vertex_bounds":{"global_min":[0,0,0],"global_max":[1,2,3],"vertex_count":3,"surface_count":1,"complete":true,"space":"global","geometry":"base_mesh_vertices"}});
    node.validate_output(&output).unwrap();
    for (field, value) in [
        ("complete", json!(false)),
        ("vertex_count", json!(250001)),
        ("global_min", json!([0, 0])),
    ] {
        let mut bad = output.clone();
        bad["data"]["observed"]["vertex_bounds"][field] = value;
        assert!(node.validate_output(&bad).is_err());
    }
    let mut bad = output.clone();
    bad["data"]["observed"]["owner_path"] = json!(true);
    assert!(node.validate_output(&bad).is_err());
    output["data"]["observed"]["light_size"] = json!(2.0);
    node.validate_output(&output).unwrap();
    for invalid in [json!(true), json!("2.0"), json!(null)] {
        let mut invalid_output = output.clone();
        invalid_output["data"]["observed"]["light_size"] = invalid;
        assert!(node.validate_output(&invalid_output).is_err());
    }
    let animation = catalog.get("driver.godot.animation.inspect").unwrap();
    let key = json!({"index":0,"time":0,"transition":1,"value":1});
    let track = json!({"index":0,"type":0,"path":"Camera:position","enabled":true,
        "key_count":540,"keys_offset":0,"keys_limit":16,"next_offset":16,"keys_complete":false,
        "interpolation_type":1,"interpolation_loop_wrap":true,"update_mode":0,"keys":[key]});
    let animation_data = json!({"name":"reveal","length":18,"loop_mode":0,
        "step":0.033333,"resource_path":"","tracks":[track]});
    let library = json!({"name":"presentation","resource_path":"res://animation.tres",
        "animations":[animation_data]});
    let data = json!({"scope":"edited_scene","metadata_complete":true,"keys_offset":0,
        "keys_limit":16,"returned_keys":1,
        "player_state":{"is_playing":false,"speed_scale":0,"autoplay":""},"libraries":[library]});
    let mut page = json!({"stamp":{"revision":0,"fingerprint":"a".repeat(64)},"data":data});
    animation.validate_output(&page).unwrap();
    page["data"]["metadata_complete"] = json!(false);
    assert!(animation.validate_output(&page).is_err());
    page["data"]["metadata_complete"] = json!(true);
    page["data"]["libraries"][0]["animations"][0]["tracks"][0]["keys"] = json!(vec![
        json!({"index":0,"time":0,"transition":1,"value":0});
        65
    ]);
    assert!(animation.validate_output(&page).is_err());
}

#[test]
fn scene_save_external_resource_policy_is_explicit_and_typed() {
    let catalog = Catalog::load().unwrap();
    let save = catalog.get("driver.godot.scene.save").unwrap();
    let base = json!({"session":"a".repeat(32),"expect":{"revision":0,"fingerprint":"b".repeat(64)},"dry_run":false});
    save.validate_input(&base).unwrap();
    for policy in [json!(true), json!(false)] {
        let mut input = base.clone();
        input["save_external_resources"] = policy;
        save.validate_input(&input).unwrap();
    }
    for policy in [json!("false"), json!(0), json!(null)] {
        let mut input = base.clone();
        input["save_external_resources"] = policy;
        assert!(save.validate_input(&input).is_err());
    }
    let mut out_of_scope = base.clone();
    out_of_scope["path"] = json!("res://another.tscn");
    assert!(save.validate_input(&out_of_scope).is_err());
}

#[test]
fn runner_capabilities_accept_owned_managed_projects_without_accepting_paths() {
    let catalog = Catalog::load().unwrap();
    for name in [
        "driver.godot.project.validate",
        "driver.godot.project.run_test",
        "driver.godot.export.pack",
        "driver.godot.export.build",
        "driver.godot.movie.capture",
    ] {
        let entry = catalog.get(name).unwrap();
        let mut input = match name {
            "driver.godot.export.pack" => {
                json!({"managed_project":"technical_two","preset":"Linux","output":"game.pck"})
            }
            "driver.godot.export.build" => {
                json!({"managed_project":"technical_two","preset":"Linux","output":"game.x86_64","debug":false})
            }
            "driver.godot.movie.capture" => {
                json!({"managed_project":"technical_two","output":"game.avi","frames":30,"fps":30})
            }
            _ => json!({"managed_project":"technical_two"}),
        };
        entry.validate_input(&input).unwrap();

        input["project"] = json!("a".repeat(64));
        assert!(
            entry.validate_input(&input).is_err(),
            "{name} must reject simultaneous paired and managed selectors"
        );

        input.as_object_mut().unwrap().remove("project");
        input["managed_project"] = json!("../escape");
        assert!(
            entry.validate_input(&input).is_err(),
            "{name} must reject path-shaped managed project selectors"
        );
    }
}

#[test]
fn native_authoring_verification_is_code_execution_and_not_a_plain_runner() {
    let catalog = Catalog::load().unwrap();
    for name in [
        "driver.godot.composition.native.verify",
        "driver.godot.composition.native.tracks.page",
        "driver.godot.composition.native.keys.page",
    ] {
        let entry = catalog.get(name).unwrap();
        assert_eq!(entry.route, Route::AuthoringRunner);
        assert_eq!(
            entry.capability.descriptor.risk,
            semwright_types::Risk::CodeExecution
        );
        assert!(entry.capability.descriptor.interactive_consent);
        assert!(
            !catalog
                .capabilities_for_runtime(true, false)
                .iter()
                .any(|capability| capability.descriptor.name == name)
        );
        assert!(
            !catalog
                .capabilities_for_runtime(false, true)
                .iter()
                .any(|capability| capability.descriptor.name == name)
        );
        assert!(
            catalog
                .capabilities_for_runtime(true, true)
                .iter()
                .any(|capability| capability.descriptor.name == name)
        );
    }

    let page = catalog
        .get("driver.godot.composition.native.tracks.page")
        .unwrap();
    let valid = json!({
        "plan_id":"godot_plan_1234",
        "scene":"arena",
        "cursor":null,
        "limit":64
    });
    page.validate_input(&valid).unwrap();
    for (field, value) in [
        ("limit", json!(0)),
        ("limit", json!(65)),
        ("cursor", json!("gtr1.not-a-digest.64")),
        ("scene", json!("../arena")),
    ] {
        let mut invalid = valid.clone();
        invalid[field] = value;
        assert!(page.validate_input(&invalid).is_err(), "{field}");
    }

    let keys = catalog
        .get("driver.godot.composition.native.keys.page")
        .unwrap();
    let valid_keys = json!({
        "plan_id":"godot_plan_1234",
        "scene":"arena",
        "player":".",
        "library":"",
        "animation":"paged_tracks",
        "track_index":0,
        "cursor":null,
        "limit":64
    });
    keys.validate_input(&valid_keys).unwrap();
    for (field, value) in [
        ("limit", json!(0)),
        ("limit", json!(65)),
        ("track_index", json!(2048)),
        ("cursor", json!("gky1.not-a-digest.64")),
        ("animation", json!("")),
    ] {
        let mut invalid = valid_keys.clone();
        invalid[field] = value;
        assert!(keys.validate_input(&invalid).is_err(), "{field}");
    }
}
