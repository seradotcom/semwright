#![cfg(target_os = "linux")]

use semwright_backend_api::Provider;
use semwright_core::{Broker, NoApprover, audit::Audit};
use semwright_driver_host::DriverProvider;
use semwright_driver_sdk::{
    ApplicationMatch, DRIVER_MANIFEST_VERSION, DRIVER_PROTOCOL_VERSION, DriverInterfaces,
    DriverMount, DriverResources, Manifest, Transport,
};
use semwright_policy::{FilesystemGrant, Policy, PolicyConfig};
use semwright_types::{ExecuteRequest, unique_id};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
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
    _state: tempfile::TempDir,
    _input: tempfile::TempDir,
    roots: Vec<FilesystemGrant>,
}

fn fixture() -> Fixture {
    let config = tempfile::tempdir().unwrap();
    let output = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let input = tempfile::tempdir().unwrap();
    for dir in [&config, &output, &state, &input] {
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let config_path = config.path().join("config.json");
    std::fs::write(
        &config_path,
        serde_json::to_vec(&json!({
            "port": 9877,
            "development_mode": false,
            "projects": [],
            "runner": null,
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
    ];
    Fixture {
        _config: config,
        output,
        _state: state,
        _input: input,
        roots,
    }
}

fn manifest(executable: PathBuf) -> Manifest {
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
        ],
        system_config: vec![],
        secrets: vec![],
        tools: vec![],
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
        manifest(executable),
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
        Arc::new(NoApprover),
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

    if let Some(destination) = std::env::var_os("SEMWRIGHT_TEST_GODOT_AUTHORING_EXPORT_DIR") {
        let destination = PathBuf::from(destination);
        assert!(
            !destination.exists(),
            "native handoff destination must start empty"
        );
        copy_tree(&product_project, &destination);
    }

    broker.remove_provider("driver:godot").await.unwrap();
    Provider::shutdown(provider.as_ref()).await.unwrap();
}
