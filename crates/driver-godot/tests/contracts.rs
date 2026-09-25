use semwright_driver_sdk::{Driver, descriptor_digest};
use semwright_godot_driver::{
    GodotDriver,
    catalog::Catalog,
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
fn catalog_declares_generic_artifact_ports() {
    let catalog = Catalog::load().unwrap();
    let rescan = catalog.get("driver.godot.assets.rescan").unwrap();
    for expected in [
        "artifact-in:model/3d",
        "artifact-in:image/raster",
        "artifact-in:image/vector",
        "artifact-in:audio/sample",
    ] {
        assert!(rescan.capability.tags.iter().any(|tag| tag == expected));
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
fn catalog_plugin_routes_have_editor_handlers() {
    use semwright_godot_driver::catalog::Route;
    let catalog = Catalog::load().unwrap();
    let plugin = include_str!("../../../integrations/godot/addons/semwright/plugin.gd");
    let names = catalog.names_for(Route::Plugin);
    assert_eq!(names.len(), 175);
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
    use semwright_godot_driver::catalog::Route;
    let catalog = Catalog::load().unwrap();
    assert_eq!(catalog.names_for(Route::Local).len(), 3);
    assert_eq!(catalog.names_for(Route::Plugin).len(), 175);
    assert_eq!(catalog.names_for(Route::Runner).len(), 6);
    assert_eq!(catalog.capabilities().len(), 184);
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
