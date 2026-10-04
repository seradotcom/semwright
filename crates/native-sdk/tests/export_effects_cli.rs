//! New source-cut adoption contract: real SDK exports and the real Effects CLI.
//! The exact-byte admission copy is a fixture. No Broker handoff, Host, daemon,
//! external application, native execution proof, or OS IPC is claimed.
#![cfg(target_os = "linux")]
#[allow(dead_code)]
#[path = "../../../examples/native/scene.rs"]
mod scene;
#[allow(dead_code)]
#[path = "../../../examples/native/table.rs"]
mod table;
use semwright_effect_conformance::{composition::*, *};
use semwright_native_sdk::{Model, NativeApp, Value, effects_readback::*, json};
use std::{
    fs,
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn mutation<M: Model>(app: &NativeApp<M>, key: &str, parameters: Value) -> Value {
    let view = app.inspect().unwrap();
    json!({"expected_revision":view["revision"],"expected_generation":view["generation"],"operation_key":key,"parameters":parameters})
}
fn exported<M: Model>(app: &NativeApp<M>, root: &Path, key: &str, slot: &str) -> (Value, Vec<u8>) {
    let result = app
        .apply(
            "export",
            &mutation(app, key, json!({"output_namespace":"owned","slot":slot})),
            || false,
        )
        .unwrap();
    let artifact = result["artifact"].clone();
    let bytes = fs::read(
        root.join("outputs")
            .join(artifact["path"].as_str().unwrap()),
    )
    .unwrap();
    assert_eq!(semwright_native_sdk::sha256(&bytes), artifact["sha256"]);
    assert_eq!(bytes.len().to_string(), artifact["bytes"]);
    (artifact, bytes)
}
fn run_cli(spec: &ProtectedSpec, path: &Path, admitted: &Path) -> Value {
    let bytes = serde_json::to_vec(spec).unwrap();
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(&bytes).unwrap();
    file.sync_all().unwrap();
    drop(file);
    let mut child = Command::new(env!("CARGO_BIN_EXE_semwright-native-effects"))
        .args([
            "--spec",
            path.to_str().unwrap(),
            "--spec-sha256",
            Digest::of_bytes(&bytes).as_str(),
            "--artifact-root",
            admitted.to_str().unwrap(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let output = child.wait_with_output().unwrap();
            panic!(
                "Owned Effects CLI exceeded the process deadline: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.len() <= 1_048_576);
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn real_scene_and_table_exports_receive_only_scoped_cli_property_results() {
    let temp = tempfile::tempdir().unwrap();
    let scene_root = temp.path().join("scene");
    let table_root = temp.path().join("table");
    let admitted = temp.path().join("admitted");
    let protected = temp.path().join("protected");
    fs::create_dir(&admitted).unwrap();
    fs::create_dir(&protected).unwrap();
    let scene = NativeApp::create(&scene_root, scene::Scene).unwrap();
    let table = NativeApp::create(&table_root, table::Table).unwrap();
    scene
        .apply(
            "set-object",
            &mutation(
                &scene,
                "edit_scene",
                json!({"object_id":"cube","color":"#112233"}),
            ),
            || false,
        )
        .unwrap();
    table
        .apply(
            "set-cell",
            &mutation(&table, "edit_table", json!({"cell":"A1","value":13})),
            || false,
        )
        .unwrap();
    let (scene_artifact, scene_bytes) = exported(&scene, &scene_root, "export_scene", "scene_json");
    let (table_artifact, table_bytes) = exported(&table, &table_root, "export_table", "table_csv");
    // This copies the exact exported bytes. It does not manufacture app output
    // or substitute for the canonical Broker's production admission handoff.
    for (name, bytes, artifact) in [
        ("scene.json", &scene_bytes, &scene_artifact),
        ("table.csv", &table_bytes, &table_artifact),
    ] {
        fs::write(admitted.join(name), bytes).unwrap();
        assert_eq!(fs::read(admitted.join(name)).unwrap(), *bytes);
        assert_eq!(
            semwright_native_sdk::sha256(&fs::read(admitted.join(name)).unwrap()),
            artifact["sha256"]
        );
    }
    let definition = SpecificationInput {
        owner: Owner {
            session: "owned-export-test".into(),
            principal: PrincipalBinding::Named("sdk-contract-owner".into()),
        },
        request_id: "owned_exports".into(),
        source_digest: Digest::of_bytes(b"declared-owned-source"),
        runtime_digest: Digest::of_bytes(b"declared-owned-test-runtime"),
        declared_producer_execution_status: ExecutionStatus::Unknown,
        application_roots: vec![
            scene_root.to_str().unwrap().into(),
            table_root.to_str().unwrap().into(),
        ],
        artifacts: vec![
            ArtifactBinding {
                slot: "scene".into(),
                path: "scene.json".into(),
                sha256: Digest::of_bytes(&scene_bytes),
                bytes: scene_bytes.len() as u64,
                mime_type: "application/json".into(),
            },
            ArtifactBinding {
                slot: "table".into(),
                path: "table.csv".into(),
                sha256: Digest::of_bytes(&table_bytes),
                bytes: table_bytes.len() as u64,
                mime_type: "text/csv".into(),
            },
        ],
        checks: vec![
            PropertyCheck {
                id: "scene_color".into(),
                artifact_slot: "scene".into(),
                selector: Selector::Json {
                    pointer: "/objects/cube/color".into(),
                    scalar: ScalarKind::Text,
                },
                predicate: Predicate::Equals {
                    expected: ObservedValue::Text {
                        value: "#112233".into(),
                    },
                },
            },
            PropertyCheck {
                id: "table_cell".into(),
                artifact_slot: "table".into(),
                selector: Selector::Csv {
                    row: 2,
                    column: "cell".into(),
                    scalar: ScalarKind::Text,
                },
                predicate: Predicate::Equals {
                    expected: ObservedValue::Text { value: "A3".into() },
                },
            },
            PropertyCheck {
                id: "table_value".into(),
                artifact_slot: "table".into(),
                selector: Selector::Csv {
                    row: 2,
                    column: "value".into(),
                    scalar: ScalarKind::Number {
                        units: "number".into(),
                    },
                },
                predicate: Predicate::Equals {
                    expected: ObservedValue::Number {
                        value: 33.0,
                        units: "number".into(),
                    },
                },
            },
        ],
    };
    let result = run_cli(
        &prepare_spec(definition.clone()).unwrap(),
        &protected.join("correct.json"),
        &admitted,
    );
    assert_eq!(result["verdict"], "PASS");
    assert_eq!(result["scope"], SCOPE);
    assert_eq!(result["execution_authority"], false);
    assert_eq!(
        result["declared_producer_execution_status"],
        serde_json::to_value(ExecutionStatus::Unknown).unwrap()
    );
    assert_eq!(result["private_measurements"].as_array().unwrap().len(), 3);
    let mut wrong = definition;
    wrong.request_id = "wrong_expected_value".into();
    wrong.checks[2].predicate = Predicate::Equals {
        expected: ObservedValue::Number {
            value: 999.0,
            units: "number".into(),
        },
    };
    let result = run_cli(
        &prepare_spec(wrong).unwrap(),
        &protected.join("wrong.json"),
        &admitted,
    );
    assert_eq!(result["verdict"], "FAIL");
    assert_eq!(result["execution_authority"], false);
    assert_eq!(
        result["declared_producer_execution_status"],
        serde_json::to_value(ExecutionStatus::Unknown).unwrap()
    );
    assert_eq!(fs::read(admitted.join("scene.json")).unwrap(), scene_bytes);
    assert_eq!(fs::read(admitted.join("table.csv")).unwrap(), table_bytes);
}
