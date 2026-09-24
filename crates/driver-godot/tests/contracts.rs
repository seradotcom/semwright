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
fn catalog_plugin_routes_have_editor_handlers() {
    use semwright_godot_driver::catalog::Route;
    let catalog = Catalog::load().unwrap();
    let plugin = include_str!("../../../integrations/godot/addons/semwright/plugin.gd");
    let names = catalog.names_for(Route::Plugin);
    assert_eq!(names.len(), 44);
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
    assert_eq!(catalog.names_for(Route::Plugin).len(), 44);
    assert_eq!(catalog.names_for(Route::Runner).len(), 6);
    assert_eq!(catalog.capabilities().len(), 53);
}
