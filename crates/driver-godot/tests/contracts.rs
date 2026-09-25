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
fn extended_variant_schema_accepts_semantic_value_types() {
    let catalog = Catalog::load().unwrap();
    let entry = catalog.get("driver.godot.node.patch").unwrap();
    let base = json!({
        "session": "a".repeat(32),
        "target": "Player",
        "properties": [
            {"name":"transform","value":{"$type":"Transform3D","value":[
                1.0,0.0,0.0, 0.0,1.0,0.0, 0.0,0.0,1.0, 2.0,3.0,4.0
            ]}},
            {"name":"path","value":{"$type":"NodePath","value":"Camera"}},
            {"name":"points","value":{"$type":"PackedVector3Array","value":[
                [0.0,0.0,0.0],[1.0,2.0,3.0]
            ]}}
        ],
        "expect": {"revision":1,"fingerprint":"b".repeat(64)},
        "dry_run": false
    });
    entry.validate_input(&base).unwrap();

    let mut invalid = base;
    invalid["properties"][0]["value"] = json!({"$type":"Callable","value":"forbidden"});
    assert!(entry.validate_input(&invalid).is_err());
}

#[test]
fn api_and_ref_contracts_are_strict() {
    let catalog = Catalog::load().unwrap();
    let search = catalog.get("driver.godot.api.search").unwrap();
    search
        .validate_input(&json!({
            "session":"a".repeat(32),"query":"Camera","source":"engine","base":"Node","limit":32
        }))
        .unwrap();
    let issue = catalog.get("driver.godot.ref.node").unwrap();
    issue
        .validate_input(&json!({"session":"a".repeat(32),"path":"Player/Camera"}))
        .unwrap();
}

#[test]
fn catalog_plugin_routes_have_editor_handlers() {
    use semwright_godot_driver::catalog::Route;
    let catalog = Catalog::load().unwrap();
    let plugin = include_str!("../../../integrations/godot/addons/semwright/plugin.gd");
    let names = catalog.names_for(Route::Plugin);
    assert_eq!(names.len(), 146);
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
    assert_eq!(catalog.names_for(Route::Plugin).len(), 146);
    assert_eq!(catalog.names_for(Route::Runner).len(), 6);
    assert_eq!(catalog.capabilities().len(), 155);
}
