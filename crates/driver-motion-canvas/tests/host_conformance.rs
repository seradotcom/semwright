#![cfg(target_os = "linux")]
use semwright_backend_api::{Context, Provider};
use semwright_driver_host::DriverProvider;
use semwright_driver_motion_canvas::model::{Node, NodeKind, Project, Properties, Scene};
use semwright_driver_sdk::{
    ApplicationMatch, DriverInterfaces, DriverMount, DriverResources, Manifest, Transport,
};
use semwright_policy::FilesystemGrant;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
use tokio_util::sync::CancellationToken;

fn digest(path: &Path) -> String {
    format!("{:x}", Sha256::digest(std::fs::read(path).unwrap()))
}
fn fixture() -> Project {
    let mut p = Project::empty("host-fixture".into());
    p.generation = "0123456789abcdef0123456789abcdef".into();
    p.scenes.push(Scene {
        id: "main".into(),
        name: "Main".into(),
        duration_ms: 1000,
        nodes: vec![Node {
            id: "title".into(),
            name: "Title".into(),
            kind: NodeKind::Text,
            parent: None,
            properties: Properties {
                text: Some("through host".into()),
                ..Default::default()
            },
        }],
        animations: vec![],
        cues: vec![],
        transition: None,
    });
    p
}
fn find<'a>(
    caps: &'a [semwright_backend_api::ProvidedCapability],
    name: &str,
) -> &'a semwright_backend_api::ProvidedCapability {
    caps.iter().find(|c| c.descriptor.name == name).unwrap()
}
async fn call(
    provider: &DriverProvider,
    caps: &[semwright_backend_api::ProvidedCapability],
    name: &str,
    args: Value,
) -> semwright_types::Result<Value> {
    Provider::execute(
        provider,
        &Context {
            session: "motion-host-conformance".into(),
            request_id: semwright_types::unique_id(),
            cancellation: CancellationToken::new(),
        },
        &find(caps, name).descriptor,
        &args,
    )
    .await
}

#[tokio::test]
#[ignore = "requires bubblewrap, Landlock and semwright-sandbox"]
async fn motion_driver_runs_through_real_driver_host_without_network() {
    if std::env::var_os("SEMWRIGHT_TEST_MOTION_CANVAS").is_none() {
        return;
    }
    let cargo_executable = PathBuf::from(env!("CARGO_BIN_EXE_semwright-motion-canvas-driver"));
    let binary_dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(binary_dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let executable = binary_dir.path().join("semwright-motion-canvas-driver");
    std::fs::copy(&cargo_executable, &executable).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    let helper = PathBuf::from(
        std::env::var_os("SEMWRIGHT_TEST_SANDBOX_HELPER")
            .expect("SEMWRIGHT_TEST_SANDBOX_HELPER required"),
    );
    let state = tempfile::tempdir().unwrap();
    std::fs::set_permissions(state.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let project = tempfile::tempdir().unwrap();
    std::fs::set_permissions(project.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(
        project.path().join("semwright-motion.json"),
        serde_json::to_vec_pretty(&fixture()).unwrap(),
    )
    .unwrap();
    let project_root = std::fs::canonicalize(project.path()).unwrap();
    let manifest = Manifest {
        manifest_version: 1,
        protocol: 1,
        id: "motion-canvas".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        publisher: "semwright-tests".into(),
        executable: executable.clone(),
        sha256: digest(&executable),
        application: ApplicationMatch {
            desktop_id: None,
            process_names: vec!["node".into()],
            supported_versions: vec![],
        },
        transport: Transport::StdioV1,
        mounts: vec![DriverMount {
            root: "project".into(),
            read_only: false,
            execute: false,
        }],
        system_config: vec![],
        secrets: vec![],
        network: false,
        loopback_port: None,
        resources: DriverResources {
            open_files: 128,
            processes: 32,
            cpu_seconds: 60,
            address_space_bytes: 1_073_741_824,
            file_size_bytes: 268_435_456,
        },
        request_timeout_ms: 10_000,
        interfaces: DriverInterfaces::default(),
    };
    let grants = vec![FilesystemGrant {
        name: "project".into(),
        path: project_root,
        read: true,
        write: true,
    }];
    let provider = DriverProvider::connect(manifest, state.path(), &helper, &grants, false)
        .await
        .unwrap();
    let caps = Provider::capabilities(provider.as_ref()).await.unwrap();
    assert_eq!(caps.len(), 18);
    assert!(
        caps.iter()
            .all(|c| c.descriptor.name.starts_with("driver.motion-canvas."))
    );
    let inspected = call(
        provider.as_ref(),
        &caps,
        "driver.motion-canvas.project.inspect",
        json!({}),
    )
    .await
    .unwrap();
    assert_eq!(inspected["project"]["id"], "host-fixture");
    let fp = inspected["fingerprint"].as_str().unwrap();
    let dry=call(provider.as_ref(),&caps,"driver.motion-canvas.project.apply",json!({"expected_fingerprint":fp,"operations":[{"op":"settings_patch","patch":{"fps":60}}],"dry_run":true})).await.unwrap();
    assert_eq!(dry["applied"], false);
    let checked = call(
        provider.as_ref(),
        &caps,
        "driver.motion-canvas.project.inspect",
        json!({}),
    )
    .await
    .unwrap();
    assert_eq!(checked["project"]["settings"]["fps"], 30);
    let doctor = call(
        provider.as_ref(),
        &caps,
        "driver.motion-canvas.doctor",
        json!({}),
    )
    .await
    .unwrap();
    assert_eq!(doctor["network"], false);
    assert_eq!(doctor["render_available"], false);
    Provider::shutdown(provider.as_ref()).await.unwrap();
}

#[tokio::test]
#[ignore = "requires semwright-sandbox; network opt-in must remain false"]
async fn manifest_requesting_network_is_denied_without_owner_opt_in() {
    if std::env::var_os("SEMWRIGHT_TEST_MOTION_CANVAS").is_none() {
        return;
    }
    let cargo_executable = PathBuf::from(env!("CARGO_BIN_EXE_semwright-motion-canvas-driver"));
    let binary_dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(binary_dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let executable = binary_dir.path().join("driver");
    std::fs::copy(&cargo_executable, &executable).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    let helper = PathBuf::from(std::env::var_os("SEMWRIGHT_TEST_SANDBOX_HELPER").unwrap());
    let state = tempfile::tempdir().unwrap();
    std::fs::set_permissions(state.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let manifest = Manifest {
        manifest_version: 1,
        protocol: 1,
        id: "motion-canvas".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        publisher: "semwright-tests".into(),
        executable: executable.clone(),
        sha256: digest(&executable),
        application: ApplicationMatch {
            desktop_id: None,
            process_names: vec!["node".into()],
            supported_versions: vec![],
        },
        transport: Transport::StdioV1,
        mounts: vec![],
        system_config: vec![],
        secrets: vec![],
        network: true,
        loopback_port: None,
        resources: DriverResources::default(),
        request_timeout_ms: 2000,
        interfaces: DriverInterfaces::default(),
    };
    let error = match DriverProvider::connect(manifest, state.path(), &helper, &[], false).await {
        Ok(_) => panic!("network driver started without owner opt-in"),
        Err(e) => e,
    };
    assert_eq!(error.code, semwright_types::ErrorCode::PolicyDenied);
}
