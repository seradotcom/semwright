#![cfg(target_os = "linux")]

use semwright_backend_api::{Backend, Provider};
use semwright_core::{Approval, Approver, Broker, audit::Audit};
use semwright_driver_host::DriverProvider;
use semwright_driver_sdk::{
    ApplicationMatch, DRIVER_MANIFEST_VERSION, DRIVER_PROTOCOL_VERSION, DriverInterfaces,
    DriverMount, DriverResources, DriverToolMount, Manifest, Transport,
};
use semwright_platform_common::artifact::ArtifactHandoff;
use semwright_policy::{FilesystemGrant, Policy, PolicyConfig};
use semwright_types::{ExecuteRequest, unique_id};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};
use tokio_util::sync::CancellationToken;

fn digest(path: &Path) -> String {
    format!("{:x}", Sha256::digest(std::fs::read(path).unwrap()))
}

fn copy_tree(source: &Path, destination: &Path) {
    std::fs::create_dir_all(destination).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path).unwrap();
        assert!(
            !metadata.file_type().is_symlink(),
            "product output contains symlink"
        );
        let target = destination.join(entry.file_name());
        if metadata.is_dir() {
            copy_tree(&path, &target);
        } else {
            assert!(metadata.is_file(), "product output contains special file");
            std::fs::copy(&path, &target).unwrap();
        }
    }
}

async fn staged_driver() -> (tempfile::TempDir, PathBuf) {
    let source = PathBuf::from(env!("CARGO_BIN_EXE_semwright-godot-driver"));
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let executable = dir.path().join("semwright-godot-driver");
    std::fs::copy(source, &executable).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    (dir, executable)
}

struct Fixture {
    _config: tempfile::TempDir,
    output: tempfile::TempDir,
    artifacts: tempfile::TempDir,
    _state: tempfile::TempDir,
    _input: tempfile::TempDir,
    runtime_sha256: String,
    roots: Vec<FilesystemGrant>,
}

fn fixture() -> Fixture {
    let config = tempfile::tempdir().unwrap();
    let output = tempfile::tempdir().unwrap();
    let artifacts = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let input = tempfile::tempdir().unwrap();
    for dir in [&config, &output, &artifacts, &state, &input] {
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let runtime =
        PathBuf::from(std::env::var_os("GODOT_BIN").expect("GODOT_BIN must point to pinned Godot"))
            .canonicalize()
            .unwrap();
    let runtime_sha256 = digest(&runtime);
    if let Some(templates) = std::env::var_os("SEMWRIGHT_TEST_GODOT_EXPORT_TEMPLATES") {
        let destination = artifacts
            .path()
            .join(".semwright-home/data/godot/export_templates/4.7.2.stable");
        copy_tree(&PathBuf::from(templates), &destination);
    }
    std::fs::write(
        input.path().join("triangle.glb"),
        include_bytes!("fixtures/authoring/triangle.glb"),
    )
    .unwrap();
    std::fs::write(
        input.path().join("start_cue.wav"),
        include_bytes!("fixtures/authoring/start_cue.wav"),
    )
    .unwrap();

    let config_path = config.path().join("config.json");
    std::fs::write(
        &config_path,
        serde_json::to_vec(&json!({
            "port": 9877,
            "development_mode": false,
            "projects": [],
            "runner": {
                "executable": "/plugin/tools/godot",
                "sha256": runtime_sha256,
                "output_root": "/workspace/godot-authoring-artifacts",
                "display": null
            },
            "authoring": {
                "output_root": "/workspace/godot-authoring-output",
                "state_root": "/workspace/godot-authoring-state",
                "input_root": "/workspace/godot-authoring-input"
            }
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::set_permissions(&config_path, std::fs::Permissions::from_mode(0o600)).unwrap();

    let roots = vec![
        FilesystemGrant {
            name: "godot-config".into(),
            path: config.path().canonicalize().unwrap(),
            read: true,
            write: false,
        },
        FilesystemGrant {
            name: "godot-authoring-output".into(),
            path: output.path().canonicalize().unwrap(),
            read: true,
            write: true,
        },
        FilesystemGrant {
            name: "godot-authoring-state".into(),
            path: state.path().canonicalize().unwrap(),
            read: true,
            write: true,
        },
        FilesystemGrant {
            name: "godot-authoring-input".into(),
            path: input.path().canonicalize().unwrap(),
            read: true,
            write: false,
        },
        FilesystemGrant {
            name: "godot-authoring-artifacts".into(),
            path: artifacts.path().canonicalize().unwrap(),
            read: true,
            write: true,
        },
        FilesystemGrant {
            name: "godot-authoring-runtime".into(),
            path: runtime,
            read: true,
            write: false,
        },
    ];
    Fixture {
        _config: config,
        output,
        artifacts,
        _state: state,
        _input: input,
        runtime_sha256,
        roots,
    }
}

fn manifest(executable: PathBuf, runtime_sha256: String) -> Manifest {
    Manifest {
        manifest_version: DRIVER_MANIFEST_VERSION,
        protocol: DRIVER_PROTOCOL_VERSION,
        id: "godot".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        publisher: "semwright-tests".into(),
        executable: executable.clone(),
        sha256: digest(&executable),
        application: ApplicationMatch {
            desktop_id: Some("org.godotengine.Godot".into()),
            process_names: vec!["godot".into(), "godot4".into()],
            supported_versions: vec!["4.7".into()],
        },
        transport: Transport::StdioV1,
        mounts: vec![
            DriverMount {
                root: "godot-config".into(),
                read_only: true,
                execute: false,
            },
            DriverMount {
                root: "godot-authoring-output".into(),
                read_only: false,
                execute: false,
            },
            DriverMount {
                root: "godot-authoring-state".into(),
                read_only: false,
                execute: false,
            },
            DriverMount {
                root: "godot-authoring-input".into(),
                read_only: true,
                execute: false,
            },
            DriverMount {
                root: "godot-authoring-artifacts".into(),
                read_only: false,
                execute: false,
            },
        ],
        system_config: vec![],
        secrets: vec![],
        tools: vec![DriverToolMount {
            root: "godot-authoring-runtime".into(),
            name: "godot".into(),
            sha256: runtime_sha256,
        }],
        network: false,
        loopback_port: None,
        resources: DriverResources {
            open_files: 128,
            processes: 32,
            cpu_seconds: 120,
            operation_cpu_seconds: 0,
            address_space_bytes: 2_147_483_648,
            file_size_bytes: 268_435_456,
        },
        request_timeout_ms: 30_000,
        interfaces: DriverInterfaces {
            cooperative_cancellation: true,
            events: true,
            progress: true,
            artifacts: true,
            health: true,
            native_refs: true,
            ..DriverInterfaces::default()
        },
    }
}

struct TestApprover;

#[async_trait::async_trait]
impl Approver for TestApprover {
    async fn approve(
        &self,
        approval: Approval,
        cancellation: CancellationToken,
    ) -> semwright_types::Result<bool> {
        if cancellation.is_cancelled() {
            return Ok(false);
        }
        Ok(matches!(
            approval.command.as_str(),
            "artifact.handoff"
                | "driver.godot.composition.native.verify"
                | "driver.godot.composition.native.query"
                | "driver.godot.composition.native.tracks.page"
                | "driver.godot.composition.native.keys.page"
                | "driver.godot.project.validate"
                | "driver.godot.project.run_test"
                | "driver.godot.export.build"
        ))
    }
}

async fn broker_call(broker: &Arc<Broker>, session: &str, command: &str, args: Value) -> Value {
    let envelope = broker
        .clone()
        .execute(
            session.to_owned(),
            unique_id(),
            ExecuteRequest {
                command: command.into(),
                args,
                dry_run: false,
                backend: None,
            },
            CancellationToken::new(),
        )
        .await;
    assert!(envelope.ok, "{command}: {envelope:?}");
    envelope.data.unwrap()
}

async fn broker_call_with_native_diagnostic(
    broker: &Arc<Broker>,
    session: &str,
    command: &str,
    args: Value,
    state_root: &Path,
) -> Value {
    let gate = state_root.join(".enable-native-diagnostics");
    let diagnostic = state_root.join(".native-verify-error");
    let _ = std::fs::remove_file(&diagnostic);
    std::fs::write(&gate, b"1").unwrap();
    std::fs::set_permissions(&gate, std::fs::Permissions::from_mode(0o600)).unwrap();
    let envelope = broker
        .clone()
        .execute(
            session.to_owned(),
            unique_id(),
            ExecuteRequest {
                command: command.into(),
                args,
                dry_run: false,
                backend: None,
            },
            CancellationToken::new(),
        )
        .await;
    let _ = std::fs::remove_file(&gate);
    if !envelope.ok {
        let private = std::fs::read_to_string(&diagnostic)
            .unwrap_or_else(|error| format!("<native diagnostic unavailable: {error}>"));
        panic!("{command}: {envelope:?}; private diagnostic: {private}");
    }
    let _ = std::fs::remove_file(&diagnostic);
    envelope.data.unwrap()
}

fn effect_rule_verdict<'a>(response: &'a Value, rule: &str) -> Option<&'a str> {
    response["effects"]["report"]["validation"]["checks"]
        .as_array()?
        .iter()
        .find(|check| check["rule"] == rule)?
        .get("verdict")?
        .as_str()
}

fn effect_rule_passes(response: &Value, rule: &str) -> bool {
    let verdict = effect_rule_verdict(response, rule);
    if verdict != Some("PASS") {
        let check = response["effects"]["report"]["validation"]["checks"]
            .as_array()
            .and_then(|checks| checks.iter().find(|check| check["rule"] == rule))
            .cloned()
            .unwrap_or(Value::Null);
        let reopened = if rule.contains("native_persistence") {
            response.get("evidence").cloned().unwrap_or(Value::Null)
        } else {
            Value::Null
        };
        eprintln!(
            "EFFECT_RULE_NOT_PASS rule={rule} check={} reopened={}",
            check, reopened
        );
    }
    verdict == Some("PASS")
}

fn managed_native_node<'a>(response: &'a Value, logical_key: &str) -> &'a Value {
    response["observation"]["authored"]["nodes"]
        .as_array()
        .and_then(|nodes| {
            nodes
                .iter()
                .find(|node| node["logical_key"].as_str() == Some(logical_key))
        })
        .unwrap_or_else(|| panic!("missing managed native node {logical_key}"))
}

fn managed_native_resource_property<'a>(
    response: &'a Value,
    logical_key: &str,
    property: &str,
) -> &'a Value {
    &managed_native_node(response, logical_key)["properties"][property]["value"]
}

fn gameplay_collision_signature(response: &Value) -> Value {
    let nodes = response["observation"]["authored"]["nodes"]
        .as_array()
        .expect("native node array");
    let bodies = ["arena/player", "arena/collectible", "arena/obstacle"]
        .into_iter()
        .map(|key| {
            let node = managed_native_node(response, key);
            json!({
                "key":key,
                "class":node["class"].clone(),
                "layer":node["properties"]["collision_layer"].clone(),
                "mask":node["properties"]["collision_mask"].clone(),
            })
        })
        .collect::<Vec<_>>();
    let shapes = ["player", "collectible", "obstacle"]
        .into_iter()
        .map(|parent| {
            let node = nodes
                .iter()
                .find(|node| {
                    node["class"] == "CollisionShape3D" && node["parent"].as_str() == Some(parent)
                })
                .unwrap_or_else(|| panic!("missing CollisionShape3D under {parent}"));
            json!({
                "parent":parent,
                "shape_class":node["properties"]["shape"]["value"]["class"].clone(),
                "disabled":node["properties"]["disabled"].clone(),
            })
        })
        .collect::<Vec<_>>();
    json!({"bodies":bodies,"shapes":shapes})
}

struct HostedAuthoring {
    _binary_dir: tempfile::TempDir,
    _driver_state: tempfile::TempDir,
    _audit_dir: tempfile::TempDir,
    fixture: Fixture,
    provider: Arc<DriverProvider>,
    broker: Arc<Broker>,
    session: String,
}

async fn hosted_authoring(test_name: &str) -> HostedAuthoring {
    let (binary_dir, executable) = staged_driver().await;
    let helper = PathBuf::from(
        std::env::var_os("SEMWRIGHT_TEST_SANDBOX_HELPER")
            .expect("SEMWRIGHT_TEST_SANDBOX_HELPER must point to semwright-sandbox"),
    );
    let fixture = fixture();
    let startup_gate = fixture._state.path().join(".enable-startup-diagnostics");
    let startup_diagnostic = fixture._state.path().join(".driver-startup-error");
    std::fs::write(&startup_gate, b"1").unwrap();
    std::fs::set_permissions(&startup_gate, std::fs::Permissions::from_mode(0o600)).unwrap();

    let driver_state = tempfile::tempdir().unwrap();
    std::fs::set_permissions(driver_state.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let provider = match DriverProvider::connect(
        manifest(executable, fixture.runtime_sha256.clone()),
        driver_state.path(),
        &helper,
        &fixture.roots,
        false,
    )
    .await
    {
        Ok(provider) => provider,
        Err(error) => {
            let diagnostic =
                std::fs::read_to_string(&startup_diagnostic).unwrap_or_else(|diagnostic_error| {
                    format!("<startup diagnostic unavailable: {diagnostic_error}>")
                });
            panic!("Driver Host startup failed: {error:?}; private diagnostic: {diagnostic}");
        }
    };
    std::fs::remove_file(&startup_gate).unwrap();
    let _ = std::fs::remove_file(&startup_diagnostic);

    let audit_dir = tempfile::tempdir().unwrap();
    let audit = Audit::open(&audit_dir.path().join("audit"), 65_536, 2).unwrap();
    let policy = Policy::new(PolicyConfig {
        allow: ["driver:godot".into()].into(),
        ..Default::default()
    })
    .unwrap();
    let broker = Broker::new(
        policy,
        vec![],
        audit,
        Arc::new(TestApprover),
        None,
        json!({"test":test_name}),
        false,
    )
    .unwrap();
    broker.mount_provider(provider.clone()).await.unwrap();

    HostedAuthoring {
        _binary_dir: binary_dir,
        _driver_state: driver_state,
        _audit_dir: audit_dir,
        fixture,
        provider,
        broker,
        session: unique_id(),
    }
}

async fn shutdown_hosted(host: HostedAuthoring) {
    host.broker.remove_provider("driver:godot").await.unwrap();
    Provider::shutdown(host.provider.as_ref()).await.unwrap();
}

#[tokio::test]
#[ignore = "requires bubblewrap/Landlock sandbox helper and pinned Godot"]
async fn driver_host_handshake_control_reaches_capabilities() {
    let host = hosted_authoring("godot-authoring-handshake-control").await;
    let capabilities = Provider::capabilities(host.provider.as_ref())
        .await
        .unwrap();
    for required in [
        "driver.godot.composition.plan",
        "driver.godot.composition.native.verify",
        "driver.godot.project.validate",
    ] {
        assert!(
            capabilities
                .iter()
                .any(|capability| capability.descriptor.name == required),
            "handshake control missing {required}"
        );
    }
    shutdown_hosted(host).await;
}

#[tokio::test]
#[ignore = "requires bubblewrap/Landlock sandbox helper and production driver binary"]
async fn empty_project_authoring_flows_through_broker_driver_host_and_provider() {
    if std::env::var_os("SEMWRIGHT_TEST_GODOT_AUTHORING_HOST").is_none() {
        return;
    }
    let (_binary_dir, executable) = staged_driver().await;
    let helper = PathBuf::from(
        std::env::var_os("SEMWRIGHT_TEST_SANDBOX_HELPER")
            .expect("SEMWRIGHT_TEST_SANDBOX_HELPER must point to semwright-sandbox"),
    );
    let fixture = fixture();
    let driver_state = tempfile::tempdir().unwrap();
    std::fs::set_permissions(driver_state.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let provider = DriverProvider::connect(
        manifest(executable, fixture.runtime_sha256.clone()),
        driver_state.path(),
        &helper,
        &fixture.roots,
        false,
    )
    .await
    .unwrap();

    let audit_dir = tempfile::tempdir().unwrap();
    let audit = Audit::open(&audit_dir.path().join("audit"), 65_536, 2).unwrap();
    let policy = Policy::new(PolicyConfig {
        allow: ["driver:godot".into()].into(),
        ..Default::default()
    })
    .unwrap();
    let broker = Broker::new(
        policy,
        vec![],
        audit,
        Arc::new(TestApprover),
        None,
        json!({"test":"godot-authoring-empty-root"}),
        false,
    )
    .unwrap();
    broker.mount_provider(provider.clone()).await.unwrap();

    let capabilities = Provider::capabilities(provider.as_ref()).await.unwrap();
    for required in [
        "driver.godot.composition.inspect",
        "driver.godot.composition.plan",
        "driver.godot.composition.apply",
        "driver.godot.composition.measure",
        "driver.godot.composition.validate",
        "driver.godot.composition.repair.plan",
        "driver.godot.composition.repair.apply",
        "driver.godot.composition.verify",
        "driver.godot.composition.native.verify",
        "driver.godot.composition.native.query",
        "driver.godot.composition.native.tracks.page",
        "driver.godot.composition.native.keys.page",
    ] {
        assert!(
            capabilities
                .iter()
                .any(|capability| capability.descriptor.name == required),
            "{required}"
        );
    }

    let session = unique_id();
    let spec: Value =
        serde_json::from_slice(include_bytes!("fixtures/authoring/two_d.json")).unwrap();
    let plan = broker_call(
        &broker,
        &session,
        "driver.godot.composition.plan",
        json!({"spec":spec}),
    )
    .await;
    let plan_id = plan["plan_id"].as_str().unwrap().to_owned();
    let project_slug = spec["project"].as_str().unwrap();
    let product_project = fixture.output.path().join(project_slug);
    assert!(
        !product_project.exists(),
        "planning must not create the target project"
    );

    let applied = broker_call(
        &broker,
        &session,
        "driver.godot.composition.apply",
        json!({"plan_id":plan_id}),
    )
    .await;
    assert_eq!(applied["execution_status"], "completed");
    assert!(product_project.join("project.godot").is_file());

    let verified = broker_call(
        &broker,
        &session,
        "driver.godot.composition.verify",
        json!({"plan_id":plan_id}),
    )
    .await;
    assert_eq!(verified["report"]["execution_status"], "completed");
    assert_eq!(verified["receipt"]["owner"]["session"], session);
    assert_eq!(
        verified["receipt"]["operation"]["capability"],
        "driver.godot.composition.apply"
    );
    assert_eq!(verified["receipt"]["coverage"]["complete"], false);

    let native_inspect = broker_call_with_native_diagnostic(
        &broker,
        &session,
        "driver.godot.composition.native.verify",
        json!({
            "plan_id":plan_id,
            "scene":"arena",
            "verification":{"kind":"inspect"}
        }),
        fixture._state.path(),
    )
    .await;
    assert_eq!(native_inspect["kind"], "inspect");
    assert_eq!(
        native_inspect["binding"]["plan_digest"],
        plan["plan_digest"]
    );
    assert_eq!(native_inspect["binding"]["slug"], project_slug);
    assert_eq!(native_inspect["binding"]["scene"], "arena");
    let readback_verdict =
        effect_rule_verdict(&native_inspect, "godot.native_readback.arena.v1").unwrap();
    assert!(matches!(readback_verdict, "PASS" | "UNKNOWN"));
    assert_eq!(native_inspect["observation"]["dependency_complete"], true);
    let has_unknown = native_inspect["observation"]["authored"]["unknown"]
        .as_array()
        .is_some_and(|unknown| !unknown.is_empty());
    assert_eq!(readback_verdict == "UNKNOWN", has_unknown);
    assert!(
        native_inspect["observation"]["authored"]["nodes"]
            .as_array()
            .is_some_and(|nodes| !nodes.is_empty())
    );
    assert!(
        native_inspect["observation"]["authored"]["animations"]
            .as_array()
            .is_some_and(|animations| !animations.is_empty())
    );
    assert!(
        native_inspect["observation"]["authored"]["nodes"]
            .as_array()
            .is_some_and(|nodes| nodes.iter().any(|node| {
                node["logical_key"] == "arena/start_sfx" && node["class"] == "AudioStreamPlayer"
            })),
        "technical 2D game missing native audio cue player"
    );
    let start_cue_sha = digest(&fixture._input.path().join("start_cue.wav"));
    assert!(
        native_inspect["observation"]["dependencies"]
            .as_array()
            .is_some_and(|dependencies| dependencies.iter().any(|dependency| {
                dependency["path"]
                    .as_str()
                    .is_some_and(|path| path.ends_with("assets/start_cue.wav"))
                    && dependency["exists"] == true
                    && dependency["sha256"] == start_cue_sha
            })),
        "technical 2D game missing pinned audio cue dependency"
    );

    let native_persistence = broker_call_with_native_diagnostic(
        &broker,
        &session,
        "driver.godot.composition.native.verify",
        json!({
            "plan_id":plan_id,
            "scene":"arena",
            "verification":{"kind":"persistence"}
        }),
        fixture._state.path(),
    )
    .await;
    assert_eq!(native_persistence["kind"], "persistence");
    assert!(effect_rule_passes(
        &native_persistence,
        "godot.native_persistence.arena.v1"
    ));
    assert_eq!(native_persistence["evidence"]["kind"], "reopened");
    assert_ne!(
        native_persistence["writer"]["process_id"],
        native_persistence["reader"]["process_id"]
    );

    let native_play = broker_call_with_native_diagnostic(
        &broker,
        &session,
        "driver.godot.composition.native.verify",
        json!({
            "plan_id":plan_id,
            "scene":"arena",
            "verification":{
                "kind":"play",
                "ticks":10,
                "inputs":[
                    {"tick":1,"action":"start","pressed":true},
                    {"tick":2,"action":"start","pressed":false}
                ],
                "checkpoints":[1,2,10],
                "variables":["score"],
                "capture":false
            }
        }),
        fixture._state.path(),
    )
    .await;
    assert_eq!(native_play["kind"], "play");
    assert!(effect_rule_passes(
        &native_play,
        "godot.native_runtime.arena.v1"
    ));
    assert_eq!(native_play["observation"]["inputs_delivered"], 2);
    let native_frames = native_play["observation"]["frames"].as_array().unwrap();
    assert_eq!(native_frames.len(), 3);
    let final_frame = native_frames.last().unwrap();
    assert_eq!(
        final_frame["state"], "play",
        "native GDScript state diverged from typed Rust start->play intent"
    );
    assert_eq!(
        final_frame["variables"]["score"]["value"], "0",
        "native GDScript score diverged from typed Rust initial value"
    );
    assert!(final_frame["fault"].is_null());
    assert!(
        native_play["observation"]["elapsed_physics_frames"]
            .as_u64()
            .is_some_and(|frames| frames >= 10)
    );

    let validated_project = broker_call(
        &broker,
        &session,
        "driver.godot.project.validate",
        json!({"managed_project":project_slug}),
    )
    .await;
    assert_eq!(validated_project["success"], true);

    let runtime = broker_call(
        &broker,
        &session,
        "driver.godot.project.run_test",
        json!({"managed_project":project_slug,"frames":10}),
    )
    .await;
    assert_eq!(runtime["success"], true);

    assert!(
        std::env::var_os("SEMWRIGHT_TEST_GODOT_EXPORT_TEMPLATES").is_some(),
        "standalone export acceptance requires pinned export templates"
    );
    let exported = broker_call(
        &broker,
        &session,
        "driver.godot.export.build",
        json!({
            "managed_project":project_slug,
            "preset":"Linux",
            "output":"technical_two.x86_64",
            "debug":false
        }),
    )
    .await;
    assert_eq!(exported["success"], true);
    let artifact = exported["artifact"].as_str().unwrap();
    assert_eq!(artifact, "technical_two.x86_64");
    let binary = fixture.artifacts.path().join(artifact);
    assert!(binary.is_file());
    let bytes = std::fs::read(&binary).unwrap();
    for forbidden in [
        b"project.semwright.json".as_slice(),
        b"native_observer.gd".as_slice(),
        b"/workspace/godot-authoring-state".as_slice(),
        b"SEMWRIGHT_GODOT_PORT".as_slice(),
    ] {
        assert!(
            !bytes
                .windows(forbidden.len())
                .any(|window| window == forbidden),
            "standalone export leaked authoring-only marker"
        );
    }
    let standalone_home = tempfile::tempdir().unwrap();
    let launched = Command::new("/usr/bin/timeout")
        .args(["5", binary.to_str().unwrap(), "--headless"])
        .env_clear()
        .env("HOME", standalone_home.path())
        .output()
        .unwrap();
    assert!(
        launched.status.success() || launched.status.code() == Some(124),
        "standalone export failed to launch: {}",
        String::from_utf8_lossy(&launched.stderr)
    );
    let launch_log = format!(
        "{}\n{}",
        String::from_utf8_lossy(&launched.stdout),
        String::from_utf8_lossy(&launched.stderr)
    );
    for marker in ["SCRIPT ERROR:", "Parse Error:"] {
        assert!(!launch_log.contains(marker), "{launch_log}");
    }

    let before_incremental = broker_call(
        &broker,
        &session,
        "driver.godot.composition.inspect",
        json!({"project":project_slug}),
    )
    .await;
    assert_eq!(before_incremental["status"], "IN_SYNC");
    let before_bindings = before_incremental["bindings"].as_object().unwrap().clone();
    let behavior_path = product_project.join("scripts/arena.gd");
    let animation_path = product_project.join("resources/arena_pulse.tres");
    let audio_path = product_project.join("assets/start_cue.wav");
    let behavior_before = digest(&behavior_path);
    let animation_before = digest(&animation_path);
    let audio_before = digest(&audio_path);

    let mut incremental_spec = spec.clone();
    incremental_spec["scenes"][0]["behavior"]["variables"][0]["initial"]["value"] = json!(4);
    let entities = incremental_spec["scenes"][0]["entities"]
        .as_array_mut()
        .unwrap();
    let hud = entities
        .iter_mut()
        .find(|entity| entity["id"] == "hud")
        .unwrap();
    hud["node"]["text"] = json!("Ready for the next round");
    entities.push(json!({
        "id":"bonus_marker",
        "parent":null,
        "position":[12.0,48.0,0.0],
        "rotation":[0.0,0.0,0.0],
        "scale":[1.0,1.0,1.0],
        "groups":["incremental"],
        "node":{
            "kind":"visual2d",
            "size":[18.0,18.0],
            "color":[0.2,0.7,0.9,1.0]
        }
    }));

    let incremental_plan = broker_call(
        &broker,
        &session,
        "driver.godot.composition.plan",
        json!({"spec":incremental_spec}),
    )
    .await;
    let incremental_plan_id = incremental_plan["plan_id"].as_str().unwrap().to_owned();
    let incremental_writes = incremental_plan["writes"].as_array().unwrap();
    assert!(
        incremental_writes
            .iter()
            .any(|path| path.as_str() == Some("scripts/arena.gd"))
    );
    assert!(
        incremental_writes
            .iter()
            .any(|path| path.as_str() == Some("scenes/arena.tscn"))
    );
    assert!(
        !incremental_writes
            .iter()
            .any(|path| path.as_str() == Some("resources/arena_pulse.tres"))
    );
    assert!(
        !incremental_writes
            .iter()
            .any(|path| path.as_str() == Some("assets/start_cue.wav"))
    );

    let incremental_apply = broker_call(
        &broker,
        &session,
        "driver.godot.composition.apply",
        json!({"plan_id":incremental_plan_id}),
    )
    .await;
    assert_eq!(incremental_apply["execution_status"], "completed");
    assert_ne!(digest(&behavior_path), behavior_before);
    assert_eq!(digest(&animation_path), animation_before);
    assert_eq!(digest(&audio_path), audio_before);

    let after_incremental = broker_call(
        &broker,
        &session,
        "driver.godot.composition.inspect",
        json!({"project":project_slug}),
    )
    .await;
    assert_eq!(after_incremental["status"], "IN_SYNC");
    let after_bindings = after_incremental["bindings"].as_object().unwrap();
    for (key, identity) in before_bindings {
        assert_eq!(
            after_bindings.get(&key),
            Some(&identity),
            "incremental update changed pre-existing logical identity {key}"
        );
    }
    assert!(after_bindings.contains_key("entity:arena/bonus_marker"));

    let incremental_native = broker_call(
        &broker,
        &session,
        "driver.godot.composition.native.verify",
        json!({
            "plan_id":incremental_plan_id,
            "scene":"arena",
            "verification":{"kind":"inspect"}
        }),
    )
    .await;
    assert!(
        incremental_native["observation"]["authored"]["nodes"]
            .as_array()
            .is_some_and(|nodes| nodes.iter().any(|node| {
                node["logical_key"] == "arena/bonus_marker" && node["class"] == "Polygon2D"
            })),
        "incremental entity count change missing from native readback"
    );
    let hud_native = managed_native_node(&incremental_native, "arena/hud");
    assert_eq!(
        hud_native["properties"]["text"]["value"],
        "Ready for the next round"
    );

    let spec3: Value =
        serde_json::from_slice(include_bytes!("fixtures/authoring/three_d.json")).unwrap();
    let plan3 = broker_call(
        &broker,
        &session,
        "driver.godot.composition.plan",
        json!({"spec":spec3}),
    )
    .await;
    let plan3_id = plan3["plan_id"].as_str().unwrap().to_owned();
    let project3 = spec3["project"].as_str().unwrap();
    assert!(!fixture.output.path().join(project3).exists());

    let applied3 = broker_call(
        &broker,
        &session,
        "driver.godot.composition.apply",
        json!({"plan_id":plan3_id}),
    )
    .await;
    assert_eq!(applied3["execution_status"], "completed");
    assert!(
        fixture
            .output
            .path()
            .join(project3)
            .join("assets/triangle.glb")
            .is_file()
    );

    let native3 = broker_call(
        &broker,
        &session,
        "driver.godot.composition.native.verify",
        json!({
            "plan_id":plan3_id,
            "scene":"arena",
            "verification":{"kind":"inspect"}
        }),
    )
    .await;
    let nodes3 = native3["observation"]["authored"]["nodes"]
        .as_array()
        .unwrap();
    for class in ["MeshInstance3D", "Camera3D", "CollisionShape3D"] {
        assert!(
            nodes3.iter().any(|node| node["class"] == class),
            "3D native readback missing {class}: {nodes3:?}"
        );
    }
    let animations3 = native3["observation"]["authored"]["animations"]
        .as_array()
        .unwrap();
    assert!(
        animations3
            .iter()
            .any(|animation| animation["name"] == "pulse"
                && animation["tracks"]
                    .as_array()
                    .is_some_and(|tracks| !tracks.is_empty())),
        "3D authored animation was not observed"
    );
    assert!(
        native3["observation"]["dependencies"]
            .as_array()
            .is_some_and(|dependencies| dependencies.iter().any(|dependency| {
                dependency["path"]
                    .as_str()
                    .is_some_and(|path| path.ends_with("triangle.glb"))
                    && dependency["exists"] == true
            })),
        "imported GLB dependency missing from native closure"
    );

    let persisted3 = broker_call(
        &broker,
        &session,
        "driver.godot.composition.native.verify",
        json!({
            "plan_id":plan3_id,
            "scene":"arena",
            "verification":{"kind":"persistence"}
        }),
    )
    .await;
    assert!(effect_rule_passes(
        &persisted3,
        "godot.native_persistence.arena.v1"
    ));
    assert_ne!(
        persisted3["writer"]["process_id"],
        persisted3["reader"]["process_id"]
    );

    let played3 = broker_call(
        &broker,
        &session,
        "driver.godot.composition.native.verify",
        json!({
            "plan_id":plan3_id,
            "scene":"arena",
            "verification":{
                "kind":"play",
                "ticks":10,
                "inputs":[
                    {"tick":1,"action":"start","pressed":true},
                    {"tick":2,"action":"start","pressed":false}
                ],
                "checkpoints":[1,2,10],
                "variables":["score"],
                "capture":false
            }
        }),
    )
    .await;
    assert!(effect_rule_passes(
        &played3,
        "godot.native_runtime.arena.v1"
    ));
    assert_eq!(
        played3["observation"]["frames"].as_array().map(Vec::len),
        Some(3)
    );

    let validated3 = broker_call(
        &broker,
        &session,
        "driver.godot.project.validate",
        json!({"managed_project":project3}),
    )
    .await;
    assert_eq!(validated3["success"], true);

    let paging_spec: Value =
        serde_json::from_slice(include_bytes!("fixtures/authoring/paging.json")).unwrap();
    let paging_plan = broker_call(
        &broker,
        &session,
        "driver.godot.composition.plan",
        json!({"spec":paging_spec}),
    )
    .await;
    let paging_plan_id = paging_plan["plan_id"].as_str().unwrap().to_owned();
    let paging_apply = broker_call(
        &broker,
        &session,
        "driver.godot.composition.apply",
        json!({"plan_id":paging_plan_id}),
    )
    .await;
    assert_eq!(paging_apply["execution_status"], "completed");

    let first_page = broker_call(
        &broker,
        &session,
        "driver.godot.composition.native.tracks.page",
        json!({
            "plan_id":paging_plan_id,
            "scene":"arena",
            "cursor":null,
            "limit":64
        }),
    )
    .await;
    assert!(effect_rule_passes(
        &first_page,
        "godot.native_readback.arena.v1"
    ));
    assert!(first_page.get("observation").is_none());
    assert_eq!(first_page["page"]["total"], 70);
    assert_eq!(
        first_page["page"]["tracks"].as_array().map(Vec::len),
        Some(64)
    );
    assert_eq!(first_page["page"]["tracks"][63]["index"], 63);
    let cursor = first_page["page"]["next_cursor"]
        .as_str()
        .expect("first native track page must continue")
        .to_owned();

    let second_page = broker_call(
        &broker,
        &session,
        "driver.godot.composition.native.tracks.page",
        json!({
            "plan_id":paging_plan_id,
            "scene":"arena",
            "cursor":cursor,
            "limit":64
        }),
    )
    .await;
    assert!(effect_rule_passes(
        &second_page,
        "godot.native_readback.arena.v1"
    ));
    assert_eq!(
        second_page["page"]["snapshot"],
        first_page["page"]["snapshot"]
    );
    assert_eq!(
        second_page["page"]["source_fingerprint"],
        first_page["page"]["source_fingerprint"]
    );
    assert_eq!(
        second_page["page"]["tracks"].as_array().map(Vec::len),
        Some(6)
    );
    assert_eq!(second_page["page"]["tracks"][5]["index"], 69);
    assert!(second_page["page"]["next_cursor"].is_null());

    let first_track = &first_page["page"]["tracks"][0];
    let first_keys = broker_call(
        &broker,
        &session,
        "driver.godot.composition.native.keys.page",
        json!({
            "plan_id":paging_plan_id,
            "scene":"arena",
            "player":first_track["player"],
            "library":first_track["library"],
            "animation":first_track["animation"],
            "track_index":first_track["index"],
            "cursor":null,
            "limit":64
        }),
    )
    .await;
    assert!(effect_rule_passes(
        &first_keys,
        "godot.native_readback.arena.v1"
    ));
    assert_eq!(
        first_keys["page"]["snapshot"],
        first_page["page"]["snapshot"]
    );
    assert_eq!(first_keys["page"]["total"], 70);
    assert_eq!(
        first_keys["page"]["keys"].as_array().map(Vec::len),
        Some(64)
    );
    assert_eq!(first_keys["page"]["keys"][63]["index"], 63);
    let key_cursor = first_keys["page"]["next_cursor"]
        .as_str()
        .expect("first native key page must continue")
        .to_owned();

    let second_keys = broker_call(
        &broker,
        &session,
        "driver.godot.composition.native.keys.page",
        json!({
            "plan_id":paging_plan_id,
            "scene":"arena",
            "player":first_track["player"],
            "library":first_track["library"],
            "animation":first_track["animation"],
            "track_index":first_track["index"],
            "cursor":key_cursor,
            "limit":64
        }),
    )
    .await;
    assert!(effect_rule_passes(
        &second_keys,
        "godot.native_readback.arena.v1"
    ));
    assert_eq!(
        second_keys["page"]["snapshot"],
        first_keys["page"]["snapshot"]
    );
    assert_eq!(
        second_keys["page"]["query_digest"],
        first_keys["page"]["query_digest"]
    );
    assert_eq!(
        second_keys["page"]["keys"].as_array().map(Vec::len),
        Some(6)
    );
    assert_eq!(second_keys["page"]["keys"][5]["index"], 69);
    assert!(second_keys["page"]["next_cursor"].is_null());

    let foreign = broker
        .clone()
        .execute(
            unique_id(),
            unique_id(),
            ExecuteRequest {
                command: "driver.godot.composition.apply".into(),
                args: json!({"plan_id":plan_id}),
                dry_run: false,
                backend: None,
            },
            CancellationToken::new(),
        )
        .await;
    assert!(!foreign.ok, "{foreign:?}");
    assert_eq!(
        foreign.error.unwrap().code,
        semwright_types::ErrorCode::PermissionDenied
    );

    broker.remove_provider("driver:godot").await.unwrap();
    Provider::shutdown(provider.as_ref()).await.unwrap();
}

#[tokio::test]
#[ignore = "requires bubblewrap/Landlock sandbox helper and pinned Godot"]
async fn animation_tree_state_machine_and_blend_space_round_trip_natively() {
    let host = hosted_authoring("godot-authoring-animation-tree").await;
    let spec: Value =
        serde_json::from_slice(include_bytes!("fixtures/authoring/animation_graphs.json")).unwrap();
    let plan = broker_call(
        &host.broker,
        &host.session,
        "driver.godot.composition.plan",
        json!({"spec":spec}),
    )
    .await;
    let plan_id = plan["plan_id"].as_str().unwrap().to_owned();
    let applied = broker_call(
        &host.broker,
        &host.session,
        "driver.godot.composition.apply",
        json!({"plan_id":plan_id}),
    )
    .await;
    assert_eq!(applied["execution_status"], "completed");

    let inspected = broker_call(
        &host.broker,
        &host.session,
        "driver.godot.composition.native.verify",
        json!({
            "plan_id":plan_id,
            "scene":"arena",
            "verification":{"kind":"inspect"}
        }),
    )
    .await;
    let nodes = inspected["observation"]["authored"]["nodes"]
        .as_array()
        .unwrap();
    let motion = nodes
        .iter()
        .find(|node| node["path"] == "_sw_animtree_motion")
        .expect("native state machine AnimationTree");
    let speed = nodes
        .iter()
        .find(|node| node["path"] == "_sw_animtree_speed")
        .expect("native blend AnimationTree");
    assert_eq!(motion["class"], "AnimationTree");
    assert_eq!(speed["class"], "AnimationTree");
    assert_eq!(motion["logical_key"], "animation_graph/arena/motion");
    assert_eq!(speed["logical_key"], "animation_graph/arena/speed");
    assert!(
        motion["logical_id"]
            .as_str()
            .is_some_and(|id| id.starts_with("asset_"))
    );
    assert!(
        speed["logical_id"]
            .as_str()
            .is_some_and(|id| id.starts_with("asset_"))
    );
    assert_eq!(motion["properties"]["active"]["value"], true);
    assert_eq!(speed["properties"]["active"]["value"], true);

    let resources = inspected["observation"]["authored"]["resources"]
        .as_array()
        .unwrap();
    let motion_root_id = motion["properties"]["tree_root"]["value"]["instance_id"]
        .as_str()
        .unwrap();
    let speed_root_id = speed["properties"]["tree_root"]["value"]["instance_id"]
        .as_str()
        .unwrap();
    let motion_root = resources
        .iter()
        .find(|resource| resource["resource"]["instance_id"] == motion_root_id)
        .expect("state machine root resource");
    let speed_root = resources
        .iter()
        .find(|resource| resource["resource"]["instance_id"] == speed_root_id)
        .expect("blend root resource");
    assert_eq!(
        motion_root["resource"]["class"],
        "AnimationNodeStateMachine"
    );
    assert_eq!(motion_root["properties"]["state_count"]["value"], "2");
    assert_eq!(motion_root["properties"]["transition_count"]["value"], "2");
    assert_eq!(speed_root["resource"]["class"], "AnimationNodeBlendSpace1D");
    assert_eq!(speed_root["properties"]["point_count"]["value"], "2");
    assert_eq!(speed_root["properties"]["sync_mode"]["value"], "1");

    let persisted = broker_call(
        &host.broker,
        &host.session,
        "driver.godot.composition.native.verify",
        json!({
            "plan_id":plan_id,
            "scene":"arena",
            "verification":{"kind":"persistence"}
        }),
    )
    .await;
    assert!(effect_rule_passes(
        &persisted,
        "godot.native_persistence.arena.v1"
    ));
    assert_eq!(persisted["evidence"]["kind"], "reopened");
    assert_ne!(
        persisted["writer"]["process_id"],
        persisted["reader"]["process_id"]
    );

    let played = broker_call(
        &host.broker,
        &host.session,
        "driver.godot.composition.native.verify",
        json!({
            "plan_id":plan_id,
            "scene":"arena",
            "verification":{
                "kind":"play",
                "ticks":3,
                "inputs":[],
                "checkpoints":[1,3],
                "variables":[],
                "capture":false
            }
        }),
    )
    .await;
    assert!(effect_rule_passes(&played, "godot.native_runtime.arena.v1"));
    let live_nodes = played["observation"]["live"]["nodes"].as_array().unwrap();
    let live_motion = live_nodes
        .iter()
        .find(|node| node["path"] == "_sw_animtree_motion")
        .expect("live state tree");
    let live_speed = live_nodes
        .iter()
        .find(|node| node["path"] == "_sw_animtree_speed")
        .expect("live blend tree");
    assert_eq!(live_motion["properties"]["current_state"]["value"], "run");
    assert_eq!(live_speed["properties"]["blend_position"]["value"], 0.75);
    assert!(
        played["observation"]["frames"]
            .as_array()
            .unwrap()
            .iter()
            .all(|frame| frame["fault"].is_null())
    );

    shutdown_hosted(host).await;
}

#[tokio::test]
#[ignore = "requires bubblewrap/Landlock sandbox helper and pinned Godot"]
async fn shared_and_local_to_scene_materials_are_native_and_isolated() {
    let host = hosted_authoring("godot-authoring-resource-sharing").await;
    let mut spec: Value =
        serde_json::from_slice(include_bytes!("fixtures/authoring/resource_sharing.json")).unwrap();
    let plan = broker_call(
        &host.broker,
        &host.session,
        "driver.godot.composition.plan",
        json!({"spec":spec}),
    )
    .await;
    let plan_id = plan["plan_id"].as_str().unwrap().to_owned();
    let applied = broker_call(
        &host.broker,
        &host.session,
        "driver.godot.composition.apply",
        json!({"plan_id":plan_id}),
    )
    .await;
    assert_eq!(applied["execution_status"], "completed");

    let observed = broker_call(
        &host.broker,
        &host.session,
        "driver.godot.composition.native.verify",
        json!({
            "plan_id":plan_id,
            "scene":"arena",
            "verification":{"kind":"inspect"}
        }),
    )
    .await;
    let shared_a =
        managed_native_resource_property(&observed, "arena/shared_a", "material_override");
    let shared_b =
        managed_native_resource_property(&observed, "arena/shared_b", "material_override");
    assert_eq!(shared_a["path"], shared_b["path"]);
    assert_eq!(shared_a["instance_id"], shared_b["instance_id"]);
    assert_eq!(shared_a["local_to_scene"], false);

    let local_a = managed_native_resource_property(&observed, "arena/local_a", "material_override");
    let local_b = managed_native_resource_property(&observed, "arena/local_b", "material_override");
    assert_eq!(local_a["local_to_scene"], true);
    assert_eq!(local_b["local_to_scene"], true);
    assert_eq!(local_a["path"], "");
    assert_eq!(local_b["path"], "");
    assert_ne!(local_a["instance_id"], local_b["instance_id"]);
    let resources = observed["observation"]["authored"]["resources"]
        .as_array()
        .expect("native resource array");
    let local_resource_a = resources
        .iter()
        .find(|resource| resource["resource"]["instance_id"] == local_a["instance_id"])
        .expect("local A material resource");
    let local_resource_b = resources
        .iter()
        .find(|resource| resource["resource"]["instance_id"] == local_b["instance_id"])
        .expect("local B material resource");
    let roughness_a = local_resource_a["properties"]["roughness"]["value"]
        .as_f64()
        .expect("local A roughness");
    let roughness_b = local_resource_b["properties"]["roughness"]["value"]
        .as_f64()
        .expect("local B roughness");
    assert!((roughness_a - 0.25).abs() < 1.0e-5, "{roughness_a}");
    assert!((roughness_b - 0.7).abs() < 1.0e-5, "{roughness_b}");
    let color_a = local_resource_a["properties"]["albedo_color"]["value"]
        .as_array()
        .expect("local A albedo");
    let color_b = local_resource_b["properties"]["albedo_color"]["value"]
        .as_array()
        .expect("local B albedo");
    for (actual, expected) in color_a.iter().zip([0.8, 0.2, 0.1, 1.0]) {
        assert!((actual.as_f64().expect("local A albedo component") - expected).abs() < 1.0e-5);
    }
    for (actual, expected) in color_b.iter().zip([0.1, 0.4, 0.9, 1.0]) {
        assert!((actual.as_f64().expect("local B albedo component") - expected).abs() < 1.0e-5);
    }

    let node_query = broker_call(
        &host.broker,
        &host.session,
        "driver.godot.composition.native.query",
        json!({
            "plan_id":plan_id,
            "scene":"arena",
            "target":{"kind":"node","logical_key":"arena/shared_a"},
            "properties":["material_override"]
        }),
    )
    .await;
    assert!(node_query.get("observation").is_none());
    assert_eq!(node_query["query"]["kind"], "node");
    assert_eq!(
        node_query["query"]["value"]["logical_key"],
        "arena/shared_a"
    );
    assert_eq!(
        node_query["query"]["value"]["properties"]
            .as_object()
            .map(serde_json::Map::len),
        Some(1)
    );
    let shared_resource_path =
        node_query["query"]["value"]["properties"]["material_override"]["value"]["path"]
            .as_str()
            .expect("shared material resource path")
            .to_owned();

    let resource_query = broker_call(
        &host.broker,
        &host.session,
        "driver.godot.composition.native.query",
        json!({
            "plan_id":plan_id,
            "scene":"arena",
            "target":{"kind":"resource","path":shared_resource_path},
            "properties":["roughness"]
        }),
    )
    .await;
    assert!(resource_query.get("observation").is_none());
    assert_eq!(resource_query["query"]["kind"], "resource");
    assert_eq!(
        resource_query["query"]["value"]["resource"]["path"],
        shared_resource_path
    );
    assert_eq!(
        resource_query["query"]["value"]["properties"]
            .as_object()
            .map(serde_json::Map::len),
        Some(1)
    );
    let shared_roughness = resource_query["query"]["value"]["properties"]["roughness"]["value"]
        .as_f64()
        .expect("shared roughness");
    assert!(
        (shared_roughness - 0.6).abs() < 1.0e-5,
        "{shared_roughness}"
    );

    let project = host.fixture.output.path().join("resource_sharing");
    let shared_path = project.join("resources/arena_material_bronze.tres");
    let local_a_path = project.join("resources/arena_local_a_material_local.tres");
    let local_b_path = project.join("resources/arena_local_b_material_local.tres");
    let shared_before = digest(&shared_path);
    let local_a_before = digest(&local_a_path);
    let local_b_before = digest(&local_b_path);

    spec["scenes"][0]["entities"][2]["node"]["material"]["color_override"] =
        json!([0.2, 0.9, 0.3, 1.0]);
    let changed = broker_call(
        &host.broker,
        &host.session,
        "driver.godot.composition.plan",
        json!({"spec":spec}),
    )
    .await;
    let changed_id = changed["plan_id"].as_str().unwrap().to_owned();
    let writes = changed["writes"].as_array().unwrap();
    assert!(
        writes
            .iter()
            .any(|value| value == "resources/arena_local_a_material_local.tres")
    );
    assert!(
        !writes
            .iter()
            .any(|value| value == "resources/arena_material_bronze.tres")
    );
    assert!(
        !writes
            .iter()
            .any(|value| value == "resources/arena_local_b_material_local.tres")
    );
    let changed_apply = broker_call(
        &host.broker,
        &host.session,
        "driver.godot.composition.apply",
        json!({"plan_id":changed_id}),
    )
    .await;
    assert_eq!(changed_apply["execution_status"], "completed");
    assert_eq!(digest(&shared_path), shared_before);
    assert_eq!(digest(&local_b_path), local_b_before);
    assert_ne!(digest(&local_a_path), local_a_before);

    let reopened = broker_call(
        &host.broker,
        &host.session,
        "driver.godot.composition.native.verify",
        json!({
            "plan_id":changed_id,
            "scene":"arena",
            "verification":{"kind":"inspect"}
        }),
    )
    .await;
    let local_a_after = &managed_native_node(&reopened, "arena/local_a")["properties"]["material_override"]
        ["value"];
    let local_b_after = &managed_native_node(&reopened, "arena/local_b")["properties"]["material_override"]
        ["value"];
    assert_eq!(local_a_after["local_to_scene"], true);
    assert_eq!(local_b_after["local_to_scene"], true);
    assert_eq!(local_a_after["path"], "");
    assert_eq!(local_b_after["path"], "");
    assert_ne!(local_a_after["instance_id"], local_b_after["instance_id"]);

    shutdown_hosted(host).await;
}

#[tokio::test]
#[ignore = "requires bubblewrap/Landlock sandbox helper and pinned Godot"]
async fn persistence_lane_reopens_in_fresh_process_and_preserves_dependencies() {
    let host = hosted_authoring("godot-authoring-persistence").await;
    let spec: Value =
        serde_json::from_slice(include_bytes!("fixtures/authoring/two_d.json")).unwrap();
    let plan = broker_call(
        &host.broker,
        &host.session,
        "driver.godot.composition.plan",
        json!({"spec":spec}),
    )
    .await;
    let plan_id = plan["plan_id"].as_str().unwrap().to_owned();
    let applied = broker_call(
        &host.broker,
        &host.session,
        "driver.godot.composition.apply",
        json!({"plan_id":plan_id}),
    )
    .await;
    assert_eq!(applied["execution_status"], "completed");

    let persisted = broker_call_with_native_diagnostic(
        &host.broker,
        &host.session,
        "driver.godot.composition.native.verify",
        json!({
            "plan_id":plan_id,
            "scene":"arena",
            "verification":{"kind":"persistence"}
        }),
        host.fixture._state.path(),
    )
    .await;
    assert_eq!(persisted["kind"], "persistence");
    assert!(effect_rule_passes(
        &persisted,
        "godot.native_persistence.arena.v1"
    ));
    assert_eq!(persisted["evidence"]["kind"], "reopened");
    assert_ne!(
        persisted["writer"]["process_id"],
        persisted["reader"]["process_id"]
    );
    assert_ne!(persisted["writer"]["nonce"], persisted["reader"]["nonce"]);
    assert_eq!(persisted["writer"]["dependency_complete"], true);
    assert_eq!(persisted["reader"]["dependency_complete"], true);

    let cue_sha = digest(&host.fixture._input.path().join("start_cue.wav"));
    for side in ["writer", "reader"] {
        assert!(
            persisted[side]["dependencies"]
                .as_array()
                .is_some_and(|dependencies| dependencies.iter().any(|dependency| {
                    dependency["path"]
                        .as_str()
                        .is_some_and(|path| path.ends_with("assets/start_cue.wav"))
                        && dependency["exists"] == true
                        && dependency["sha256"] == cue_sha
                })),
            "{side} missing unchanged external audio dependency sentinel"
        );
    }

    shutdown_hosted(host).await;
}

#[tokio::test]
#[ignore = "requires bubblewrap/Landlock sandbox helper, pinned Godot and export template"]
async fn export_lane_builds_and_launches_without_editor_or_semwright() {
    assert!(
        std::env::var_os("SEMWRIGHT_TEST_GODOT_EXPORT_TEMPLATES").is_some(),
        "export lane requires pinned Godot export template"
    );
    let host = hosted_authoring("godot-authoring-export").await;
    let spec: Value =
        serde_json::from_slice(include_bytes!("fixtures/authoring/two_d.json")).unwrap();
    let project = spec["project"].as_str().unwrap().to_owned();
    let plan = broker_call(
        &host.broker,
        &host.session,
        "driver.godot.composition.plan",
        json!({"spec":spec}),
    )
    .await;
    let plan_id = plan["plan_id"].as_str().unwrap().to_owned();
    let applied = broker_call(
        &host.broker,
        &host.session,
        "driver.godot.composition.apply",
        json!({"plan_id":plan_id}),
    )
    .await;
    assert_eq!(applied["execution_status"], "completed");

    let validated = broker_call(
        &host.broker,
        &host.session,
        "driver.godot.project.validate",
        json!({"managed_project":project}),
    )
    .await;
    assert_eq!(validated["success"], true);
    let runtime = broker_call(
        &host.broker,
        &host.session,
        "driver.godot.project.run_test",
        json!({"managed_project":project,"frames":10}),
    )
    .await;
    assert_eq!(runtime["success"], true);

    let exported = broker_call(
        &host.broker,
        &host.session,
        "driver.godot.export.build",
        json!({
            "managed_project":project,
            "preset":"Linux",
            "output":"persistence_export_lane.x86_64",
            "debug":false
        }),
    )
    .await;
    assert_eq!(exported["success"], true);
    assert_eq!(exported["artifact"], "persistence_export_lane.x86_64");
    let binary = host
        .fixture
        .artifacts
        .path()
        .join("persistence_export_lane.x86_64");
    assert!(binary.is_file());

    let bytes = std::fs::read(&binary).unwrap();
    for forbidden in [
        b"project.semwright.json".as_slice(),
        b"authoring-spec.json".as_slice(),
        b"native_observer.gd".as_slice(),
        b"/workspace/godot-authoring-state".as_slice(),
        b"SEMWRIGHT_GODOT_PORT".as_slice(),
        b"GH_TOKEN".as_slice(),
    ] {
        assert!(
            !bytes
                .windows(forbidden.len())
                .any(|window| window == forbidden),
            "standalone export leaked authoring-only marker"
        );
    }

    let standalone_home = tempfile::tempdir().unwrap();
    let launched = Command::new("/usr/bin/timeout")
        .args(["5", binary.to_str().unwrap(), "--headless"])
        .env_clear()
        .env("HOME", standalone_home.path())
        .output()
        .unwrap();
    assert!(
        launched.status.success() || launched.status.code() == Some(124),
        "standalone export failed to launch: {}",
        String::from_utf8_lossy(&launched.stderr)
    );
    let launch_log = format!(
        "{}\n{}",
        String::from_utf8_lossy(&launched.stdout),
        String::from_utf8_lossy(&launched.stderr)
    );
    for marker in ["SCRIPT ERROR:", "Parse Error:"] {
        assert!(!launch_log.contains(marker), "{launch_log}");
    }

    shutdown_hosted(host).await;
}

#[tokio::test]
#[ignore = "requires bubblewrap/Landlock sandbox helper and pinned Godot"]
async fn typed_transform_and_reparent_actions_round_trip_natively() {
    let host = hosted_authoring("godot-authoring-transforms").await;
    let mut spec: Value =
        serde_json::from_slice(include_bytes!("fixtures/authoring/two_d.json")).unwrap();
    spec["project"] = json!("transform_actions");

    let expressions = spec["scenes"][0]["behavior"]["expressions"]
        .as_array_mut()
        .unwrap();
    expressions.push(json!({
        "kind":"literal",
        "value":{"kind":"scalar","value":0.25}
    }));
    expressions.push(json!({
        "kind":"literal",
        "value":{"kind":"scalar","value":1.5}
    }));
    expressions.push(json!({"kind":"vector2","x":11,"y":11}));
    expressions.push(json!({
        "kind":"literal",
        "value":{"kind":"scalar","value":60.0}
    }));
    expressions.push(json!({"kind":"vector2","x":0,"y":13}));
    spec["scenes"][0]["behavior"]["handlers"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "id":"transform_ready",
            "event":{"kind":"ready"},
            "state":"start",
            "repeat":1,
            "actions":[
                {"kind":"rotation","entity":"player","value":10},
                {"kind":"scale","entity":"visual","value":12},
                {
                    "kind":"reparent",
                    "entity":"collectible_visual",
                    "parent":"player",
                    "keep_global":true
                }
            ]
        }));
    let physics = spec["scenes"][0]["behavior"]["handlers"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|handler| handler["event"]["kind"].as_str() == Some("physics_tick"))
        .expect("physics handler");
    physics["actions"] = json!([{
        "kind":"accelerate2d",
        "entity":"player",
        "acceleration":14,
        "max_speed":120.0
    }]);

    let plan = broker_call(
        &host.broker,
        &host.session,
        "driver.godot.composition.plan",
        json!({"spec":spec}),
    )
    .await;
    let plan_id = plan["plan_id"].as_str().unwrap().to_owned();
    let applied = broker_call(
        &host.broker,
        &host.session,
        "driver.godot.composition.apply",
        json!({"plan_id":plan_id}),
    )
    .await;
    assert_eq!(applied["execution_status"], "completed");

    let played = broker_call(
        &host.broker,
        &host.session,
        "driver.godot.composition.native.verify",
        json!({
            "plan_id":plan_id,
            "scene":"arena",
            "verification":{
                "kind":"play",
                "ticks":12,
                "inputs":[
                    {"tick":1,"action":"start","pressed":true},
                    {"tick":2,"action":"start","pressed":false}
                ],
                "checkpoints":[2,12],
                "variables":["score"],
                "capture":false
            }
        }),
    )
    .await;
    assert!(effect_rule_passes(&played, "godot.native_runtime.arena.v1"));
    assert_eq!(played["observation"]["inputs_delivered"], 2);
    assert_eq!(played["observation"]["failures"], json!([]));
    assert!(
        played["observation"]["live"].is_object(),
        "post-play live projection missing"
    );

    let live_nodes = played["observation"]["live"]["nodes"]
        .as_array()
        .expect("live native nodes");
    let live_node = |logical_key: &str| {
        live_nodes
            .iter()
            .find(|node| node["logical_key"].as_str() == Some(logical_key))
            .unwrap_or_else(|| panic!("missing live native node {logical_key}"))
    };

    let player = live_node("arena/player");
    assert_eq!(player["properties"]["rotation"]["type"], "float");
    let rotation = player["properties"]["rotation"]["value"]
        .as_f64()
        .expect("player live rotation");
    assert!((rotation - 0.25).abs() < 1.0e-5, "{rotation}");

    let visual = live_node("arena/visual");
    assert_eq!(visual["properties"]["scale"]["type"], "vector2");
    assert_eq!(visual["properties"]["scale"]["value"], json!([1.5, 1.5]));

    let collectible_visual = live_node("arena/collectible_visual");
    assert_eq!(collectible_visual["parent"], "player");

    let last = played["observation"]["frames"]
        .as_array()
        .and_then(|frames| frames.last())
        .expect("gravity play checkpoint");
    assert_eq!(last["state"], "play");
    assert!(last["fault"].is_null());
    let position = &last["positions"]["arena/player"];
    assert_eq!(position["type"], "vector2");
    let y = position["value"][1].as_f64().expect("player y");
    assert!(y > 160.0, "gravity acceleration did not move player: {y}");

    shutdown_hosted(host).await;
}

const E_ARTICULATED_GLB_SHA256: &str =
    "f756e288afb978993488e2f59b07179b1c97f7801d5f4314166de0d2b6db7ca5";

#[tokio::test]
#[ignore = "requires pinned E Blender GLB artifact, bubblewrap/Landlock and pinned Godot"]
async fn blender_glb_handoff_preserves_godot_semantics_and_gameplay() {
    if std::env::var_os("SEMWRIGHT_TEST_E_GLB").is_none() {
        return;
    }

    // Reuse the exact provider/Broker bootstrap that is independently green in
    // native, persistence and export acceptance. Cross-app transport gets its
    // own Broker so no artifact filesystem authority is added to the Godot
    // provider's policy boundary.
    let host = hosted_authoring("godot-cross-app-blender-glb").await;
    let godot_input = host.fixture._input.path().canonicalize().unwrap();

    let e_glb = PathBuf::from(
        std::env::var_os("SEMWRIGHT_TEST_E_GLB").expect("cross-app GLB path disappeared"),
    )
    .canonicalize()
    .unwrap();
    assert_eq!(
        e_glb.file_name().and_then(|name| name.to_str()),
        Some("articulated.glb")
    );
    assert_eq!(digest(&e_glb), E_ARTICULATED_GLB_SHA256);
    let e_root = e_glb.parent().unwrap().canonicalize().unwrap();

    let handoff_grants = vec![
        FilesystemGrant {
            name: "e-blender-output".into(),
            path: e_root,
            read: true,
            write: false,
        },
        FilesystemGrant {
            name: "godot-authoring-input".into(),
            path: godot_input.clone(),
            read: true,
            write: true,
        },
    ];
    let artifact_backend: Arc<dyn Backend> =
        Arc::new(ArtifactHandoff::new(&handoff_grants).unwrap());

    let artifact_audit_dir = tempfile::tempdir().unwrap();
    let artifact_audit = Audit::open(&artifact_audit_dir.path().join("audit"), 65_536, 2).unwrap();
    let artifact_policy = Policy::new(PolicyConfig {
        filesystem: handoff_grants.clone(),
        ..Default::default()
    })
    .unwrap();
    let handoff_broker = Broker::new(
        artifact_policy,
        vec![artifact_backend],
        artifact_audit,
        Arc::new(TestApprover),
        None,
        json!({
            "test":"godot-cross-app-artifact-handoff",
            "e_source_sha":"d80471b9ceb828a2a531dfac9eaa4e970ea589a9",
            "e_run_id":36543391358u64,
            "e_glb_sha256":E_ARTICULATED_GLB_SHA256
        }),
        false,
    )
    .unwrap();

    let session = host.session.clone();
    let mut baseline_spec: Value =
        serde_json::from_slice(include_bytes!("fixtures/authoring/three_d.json")).unwrap();
    baseline_spec["project"] = json!("cross_app_articulated");
    baseline_spec["title"] = json!("Cross-app GLB replacement");

    let baseline_plan = broker_call(
        &host.broker,
        &session,
        "driver.godot.composition.plan",
        json!({"spec":baseline_spec}),
    )
    .await;
    let baseline_plan_id = baseline_plan["plan_id"].as_str().unwrap().to_owned();
    let baseline_apply = broker_call(
        &host.broker,
        &session,
        "driver.godot.composition.apply",
        json!({"plan_id":baseline_plan_id}),
    )
    .await;
    assert_eq!(baseline_apply["execution_status"], "completed");

    let baseline_snapshot = broker_call(
        &host.broker,
        &session,
        "driver.godot.composition.inspect",
        json!({"project":"cross_app_articulated"}),
    )
    .await;
    assert_eq!(baseline_snapshot["status"], "IN_SYNC");
    let baseline_bindings = baseline_snapshot["bindings"].clone();
    let product_project = host.fixture.output.path().join("cross_app_articulated");
    let behavior_path = product_project.join("scripts/arena.gd");
    let baseline_behavior_sha = digest(&behavior_path);
    let triangle_path = product_project.join("assets/triangle.glb");
    let baseline_triangle_sha = digest(&triangle_path);
    assert_eq!(
        baseline_triangle_sha,
        "150f5ad8fcc374e0c2b2058868207117a6a89446bbb1607c426d0e9bf42246c6"
    );

    let baseline_native = broker_call(
        &host.broker,
        &session,
        "driver.godot.composition.native.verify",
        json!({
            "plan_id":baseline_plan_id,
            "scene":"arena",
            "verification":{"kind":"inspect"}
        }),
    )
    .await;
    let baseline_collision = gameplay_collision_signature(&baseline_native);
    let baseline_import = managed_native_node(&baseline_native, "arena/imported_model");
    assert_eq!(baseline_import["class"], "Node3D");
    assert!(
        baseline_native["observation"]["dependencies"]
            .as_array()
            .is_some_and(|dependencies| dependencies.iter().any(|dependency| {
                dependency["path"]
                    .as_str()
                    .is_some_and(|path| path.ends_with("assets/triangle.glb"))
                    && dependency["exists"] == true
                    && dependency["sha256"] == baseline_triangle_sha
            })),
        "baseline GLB dependency was not observed"
    );

    let baseline_play = broker_call(
        &host.broker,
        &session,
        "driver.godot.composition.native.verify",
        json!({
            "plan_id":baseline_plan_id,
            "scene":"arena",
            "verification":{
                "kind":"play",
                "ticks":12,
                "inputs":[
                    {"tick":1,"action":"start","pressed":true},
                    {"tick":2,"action":"start","pressed":false},
                    {"tick":2,"action":"right","pressed":true},
                    {"tick":9,"action":"right","pressed":false}
                ],
                "checkpoints":[1,2,9,12],
                "variables":["score"],
                "capture":false
            }
        }),
    )
    .await;
    assert!(effect_rule_passes(
        &baseline_play,
        "godot.native_runtime.arena.v1"
    ));
    let baseline_last = baseline_play["observation"]["frames"]
        .as_array()
        .and_then(|frames| frames.last())
        .unwrap();
    assert_eq!(baseline_last["state"], "play");
    assert!(baseline_last["fault"].is_null());

    let handoff = broker_call(
        &handoff_broker,
        &session,
        "artifact.handoff",
        json!({
            "source_root":"e-blender-output",
            "source_path":"articulated.glb",
            "destination_root":"godot-authoring-input",
            "destination_path":"articulated.glb",
            "expected_sha256":E_ARTICULATED_GLB_SHA256,
            "max_bytes":16_777_216,
            "semantic_type":"model/3d",
            "media_type":"model/gltf-binary"
        }),
    )
    .await;
    assert_eq!(handoff["copied"], true);
    assert_eq!(handoff["atomic"], true);
    assert_eq!(handoff["sha256"], E_ARTICULATED_GLB_SHA256);
    assert_eq!(
        digest(&godot_input.join("articulated.glb")),
        E_ARTICULATED_GLB_SHA256
    );

    let mut replacement_spec = baseline_spec.clone();
    replacement_spec["assets"][0]["file"] = json!("articulated.glb");
    replacement_spec["assets"][0]["sha256"] = json!(E_ARTICULATED_GLB_SHA256);

    let replacement_plan = broker_call(
        &host.broker,
        &session,
        "driver.godot.composition.plan",
        json!({"spec":replacement_spec}),
    )
    .await;
    let replacement_plan_id = replacement_plan["plan_id"].as_str().unwrap().to_owned();
    let writes = replacement_plan["writes"].as_array().unwrap();
    assert!(
        writes
            .iter()
            .any(|path| path.as_str() == Some("assets/articulated.glb")),
        "replacement plan did not add Blender GLB"
    );
    assert!(
        writes
            .iter()
            .any(|path| path.as_str() == Some("scenes/arena.tscn")),
        "replacement plan did not update native scene binding"
    );
    assert!(
        !writes
            .iter()
            .any(|path| path.as_str() == Some("scripts/arena.gd")),
        "asset substitution unexpectedly rewrote behavior"
    );

    let replacement_apply = broker_call(
        &host.broker,
        &session,
        "driver.godot.composition.apply",
        json!({"plan_id":replacement_plan_id}),
    )
    .await;
    assert_eq!(replacement_apply["execution_status"], "completed");
    assert_eq!(digest(&behavior_path), baseline_behavior_sha);
    assert_eq!(digest(&triangle_path), baseline_triangle_sha);
    assert_eq!(
        digest(&product_project.join("assets/articulated.glb")),
        E_ARTICULATED_GLB_SHA256
    );

    let replacement_snapshot = broker_call(
        &host.broker,
        &session,
        "driver.godot.composition.inspect",
        json!({"project":"cross_app_articulated"}),
    )
    .await;
    assert_eq!(replacement_snapshot["status"], "IN_SYNC");
    assert_eq!(
        replacement_snapshot["bindings"], baseline_bindings,
        "asset substitution changed logical scene/entity identities"
    );

    let inspected = broker_call(
        &host.broker,
        &session,
        "driver.godot.composition.native.verify",
        json!({
            "plan_id":replacement_plan_id,
            "scene":"arena",
            "verification":{"kind":"inspect"}
        }),
    )
    .await;
    let readback = effect_rule_verdict(&inspected, "godot.native_readback.arena.v1").unwrap();
    assert!(matches!(readback, "PASS" | "UNKNOWN"));
    assert_eq!(
        gameplay_collision_signature(&inspected),
        baseline_collision,
        "Blender asset substitution changed declared gameplay collision mapping"
    );

    let dependencies = inspected["observation"]["dependencies"].as_array().unwrap();
    assert!(
        dependencies.iter().any(|dependency| {
            dependency["path"]
                .as_str()
                .is_some_and(|path| path.ends_with("assets/articulated.glb"))
                && dependency["exists"] == true
                && dependency["sha256"] == E_ARTICULATED_GLB_SHA256
        }),
        "native dependency closure did not pin E articulated.glb"
    );

    let nodes = inspected["observation"]["authored"]["nodes"]
        .as_array()
        .unwrap();
    let skeletons = nodes
        .iter()
        .filter(|node| node["class"] == "Skeleton3D")
        .collect::<Vec<_>>();
    assert!(!skeletons.is_empty(), "E GLB imported without Skeleton3D");
    assert!(
        skeletons.iter().any(|node| {
            let Some(properties) = node["properties"].as_object() else {
                return false;
            };
            let names = properties
                .iter()
                .filter_map(|(name, value)| {
                    name.starts_with("bone_name_")
                        .then(|| value["value"].as_str())
                        .flatten()
                })
                .collect::<std::collections::BTreeSet<_>>();
            names.contains("base") && names.contains("hinge")
        }),
        "E GLB base/hinge bones missing from Godot native readback"
    );
    assert!(
        nodes.iter().any(|node| {
            node["class"] == "MeshInstance3D"
                && node["properties"]["skeleton"]["value"]
                    .as_str()
                    .is_some_and(|path| !path.is_empty())
        }),
        "E GLB skin binding missing from Godot MeshInstance3D readback"
    );

    let resources = inspected["observation"]["authored"]["resources"]
        .as_array()
        .unwrap();
    let imported_materials = resources
        .iter()
        .filter(|resource| {
            resource["resource"]["path"]
                .as_str()
                .is_some_and(|path| path.starts_with("res://assets/articulated.glb"))
                && resource["resource"]["class"]
                    .as_str()
                    .is_some_and(|class| class.contains("Material"))
        })
        .count();
    assert!(
        imported_materials >= 2,
        "expected E GLB material bindings in Godot readback, got {imported_materials}"
    );

    let animations = inspected["observation"]["authored"]["animations"]
        .as_array()
        .unwrap();
    assert!(
        animations.iter().any(|animation| {
            animation["tracks"].as_array().is_some_and(|tracks| {
                tracks
                    .iter()
                    .map(|track| track["key_count"].as_u64().unwrap_or_default())
                    .sum::<u64>()
                    > 2
            })
        }),
        "E GLB animation did not survive Godot import"
    );

    let persisted = broker_call(
        &host.broker,
        &session,
        "driver.godot.composition.native.verify",
        json!({
            "plan_id":replacement_plan_id,
            "scene":"arena",
            "verification":{"kind":"persistence"}
        }),
    )
    .await;
    assert!(effect_rule_passes(
        &persisted,
        "godot.native_persistence.arena.v1"
    ));
    assert_eq!(persisted["evidence"]["kind"], "reopened");
    assert_ne!(
        persisted["writer"]["process_id"],
        persisted["reader"]["process_id"]
    );

    let played = broker_call(
        &host.broker,
        &session,
        "driver.godot.composition.native.verify",
        json!({
            "plan_id":replacement_plan_id,
            "scene":"arena",
            "verification":{
                "kind":"play",
                "ticks":12,
                "inputs":[
                    {"tick":1,"action":"start","pressed":true},
                    {"tick":2,"action":"start","pressed":false},
                    {"tick":2,"action":"right","pressed":true},
                    {"tick":9,"action":"right","pressed":false}
                ],
                "checkpoints":[1,2,9,12],
                "variables":["score"],
                "capture":false
            }
        }),
    )
    .await;
    assert!(effect_rule_passes(&played, "godot.native_runtime.arena.v1"));
    assert_eq!(played["observation"]["inputs_delivered"], 4);
    let frames = played["observation"]["frames"].as_array().unwrap();
    assert_eq!(frames.len(), 4);
    let last = frames.last().unwrap();
    assert_eq!(last["state"], baseline_last["state"]);
    assert_eq!(last["variables"], baseline_last["variables"]);
    assert!(last["fault"].is_null());

    // Close D12's Project Graph side with the actual C receipt emitted by the
    // product for the replacement apply. This is not a synthetic readback
    // receipt: it authenticates the mutating Composition apply that introduced
    // articulated.glb and pins the managed GLB asset revision/bytes.
    let source_verified = broker_call(
        &host.broker,
        &session,
        "driver.godot.composition.verify",
        json!({"plan_id":replacement_plan_id}),
    )
    .await;
    let receipt = &source_verified["receipt"];
    assert_eq!(
        receipt["operation"]["capability"],
        "driver.godot.composition.apply"
    );
    let receipt_id = receipt["id"]
        .as_str()
        .expect("cross-app C receipt id")
        .to_owned();
    assert!(receipt_id.starts_with("receipt_"));

    let articulated_file = replacement_snapshot["files"]
        .as_array()
        .and_then(|files| {
            files
                .iter()
                .find(|file| file["path"] == "assets/articulated.glb")
        })
        .expect("managed articulated GLB file identity");
    let articulated_asset = articulated_file["asset"]
        .as_str()
        .expect("managed articulated GLB asset id");
    let articulated_revision = articulated_file["revision"]
        .as_str()
        .expect("managed articulated GLB revision");
    let receipt_output = receipt["outputs"]
        .as_array()
        .and_then(|outputs| {
            outputs
                .iter()
                .find(|output| output["asset"].as_str() == Some(articulated_asset))
        })
        .expect("cross-app C receipt output pin");
    assert_eq!(receipt_output["revision"], articulated_revision);
    assert_eq!(
        receipt_output["fingerprint"]["bytes"],
        E_ARTICULATED_GLB_SHA256
    );
    assert_eq!(receipt["coverage"]["complete"], false);

    if let Some(path) = std::env::var_os("SEMWRIGHT_TEST_D12_C_RECEIPT_OUT") {
        let path = PathBuf::from(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(
            path,
            serde_json::to_vec_pretty(&json!({
                "schema_version":1,
                "receipt_id":receipt_id,
                "operation":receipt["operation"]["capability"].clone(),
                "project":receipt["project"].clone(),
                "asset":articulated_asset,
                "revision":articulated_revision,
                "sha256":E_ARTICULATED_GLB_SHA256,
                "coverage_complete":receipt["coverage"]["complete"].clone()
            }))
            .unwrap(),
        )
        .unwrap();
    }

    let validated = broker_call(
        &host.broker,
        &session,
        "driver.godot.project.validate",
        json!({"managed_project":"cross_app_articulated"}),
    )
    .await;
    assert_eq!(validated["success"], true);

    shutdown_hosted(host).await;
}
