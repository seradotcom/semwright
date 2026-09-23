#![cfg(target_os = "linux")]
use semwright_backend_api::{Context, Provider};
use semwright_driver_host::DriverProvider;
use semwright_driver_motion_canvas::model::{Node, NodeKind, Project, Properties, Scene};
use semwright_driver_sdk::{
    ApplicationMatch, DriverInterfaces, DriverMount, DriverResources, Manifest, Transport,
};
use semwright_platform_api::launch::{Mount, ResourceLimits, SandboxKind, SandboxSpec};
use semwright_policy::FilesystemGrant;
use semwright_protocol::{read_frame, write_frame};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::io::AsyncReadExt;
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
async fn direct_sandbox_protocol_probe(
    manifest: &Manifest,
    helper: &Path,
    project: &Path,
) -> Result<(), String> {
    let spec = SandboxSpec {
        kind: SandboxKind::Driver,
        staged_executable: manifest.executable.clone(),
        helper: helper.to_path_buf(),
        mounts: vec![Mount {
            source: project.to_path_buf(),
            destination: "/workspace/project".into(),
            read_only: false,
        }],
        system_config: vec![],
        network: false,
        limits: Some(ResourceLimits {
            open_files: manifest.resources.open_files,
            processes: manifest.resources.processes,
            cpu_seconds: manifest.resources.cpu_seconds,
            address_space_bytes: manifest.resources.address_space_bytes,
            file_size_bytes: manifest.resources.file_size_bytes,
        }),
    };
    let mut command = semwright_platform_services::sandbox_command(&spec)
        .map_err(|e| format!("sandbox command: {e}"))?;
    command.stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|e| format!("sandbox spawn: {e}"))?;
    let mut input = child.stdin.take().ok_or("diagnostic stdin missing")?;
    let mut output = child.stdout.take().ok_or("diagnostic stdout missing")?;
    let mut stderr = child.stderr.take().ok_or("diagnostic stderr missing")?;
    let identity = manifest.identity().map_err(|e| e.to_string())?;
    let hello = semwright_driver_sdk::Request::Hello {
        protocol: 1,
        provider: identity,
        executable_sha256: manifest.sha256.clone(),
    };
    let result = async {
        write_frame(&mut input, &hello)
            .await
            .map_err(|e| format!("write hello: {e}"))?;
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            read_frame::<_, semwright_driver_sdk::Response>(&mut output),
        )
        .await
        .map_err(|_| "ready timeout".to_string())?
        .map_err(|e| format!("read ready: {e}"))?
        {
            semwright_driver_sdk::Response::Ready { .. } => {}
            other => return Err(format!("unexpected ready response: {other:?}")),
        }
        write_frame(
            &mut input,
            &semwright_driver_sdk::Request::Capabilities { id: "probe".into() },
        )
        .await
        .map_err(|e| format!("write capabilities: {e}"))?;
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            read_frame::<_, semwright_driver_sdk::Response>(&mut output),
        )
        .await
        .map_err(|_| "capabilities timeout".to_string())?
        .map_err(|e| format!("read capabilities: {e}"))?
        {
            semwright_driver_sdk::Response::Capabilities { .. } => {}
            other => return Err(format!("unexpected capabilities response: {other:?}")),
        }
        write_frame(
            &mut input,
            &semwright_driver_sdk::Request::Shutdown { id: "stop".into() },
        )
        .await
        .map_err(|e| format!("write shutdown: {e}"))?;
        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            read_frame::<_, semwright_driver_sdk::Response>(&mut output),
        )
        .await;
        Ok::<_, String>(())
    }
    .await;
    drop(input);
    if result.is_err() {
        let _ = child.kill().await;
    }
    let _ = child.wait().await;
    let mut diagnostics = Vec::new();
    let _ = stderr.take(65_536).read_to_end(&mut diagnostics).await;
    result.map_err(|error| {
        format!(
            "{error}; sandbox stderr: {}",
            String::from_utf8_lossy(&diagnostics)
                .chars()
                .take(4000)
                .collect::<String>()
        )
    })
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
        }],
        system_config: vec![],
        network: false,
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
        path: project_root.clone(),
        read: true,
        write: true,
    }];
    direct_sandbox_protocol_probe(&manifest, &helper, &project_root)
        .await
        .unwrap();
    let provider = DriverProvider::connect(manifest, state.path(), &helper, &grants, false)
        .await
        .unwrap();
    let caps = Provider::capabilities(provider.as_ref()).await.unwrap();
    assert_eq!(caps.len(), 16);
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
        network: true,
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
