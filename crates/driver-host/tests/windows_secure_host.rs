#![cfg(target_os = "windows")]

use semwright_backend_api::{Context, Provider};
use semwright_driver_host::DriverProvider;
use semwright_driver_sdk::{
    ApplicationMatch, DriverInterfaces, DriverMount, DriverResources, Manifest, Transport,
};
use semwright_policy::FilesystemGrant;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;

fn digest(path: &Path) -> String {
    format!(
        "{:x}",
        Sha256::digest(std::fs::read(path).expect("read fixture"))
    )
}

fn harden_fixture(path: &Path) {
    let user = std::env::var("USERNAME").expect("Windows USERNAME");
    let principal = match std::env::var("USERDOMAIN") {
        Ok(domain) if !domain.is_empty() => format!(r"{domain}\{user}"),
        _ => user,
    };
    let status = std::process::Command::new("icacls")
        .arg(path)
        .arg("/inheritance:r")
        .arg("/grant:r")
        .arg(format!("{principal}:(F)"))
        .status()
        .expect("run icacls");
    assert!(status.success(), "fixture DACL hardening must succeed");
}

fn manifest(executable: PathBuf) -> Manifest {
    Manifest {
        manifest_version: 1,
        protocol: 2,
        id: "fixture".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        publisher: "semwright-tests".into(),
        executable: executable.clone(),
        sha256: digest(&executable),
        application: ApplicationMatch {
            desktop_id: Some("org.semwright.DriverFixture".into()),
            process_names: vec![],
            supported_versions: vec![],
        },
        transport: Transport::StdioV1,
        mounts: vec![],
        system_config: vec![],
        secrets: vec![],
        tools: vec![],
        network: false,
        loopback_port: None,
        resources: DriverResources {
            open_files: 128,
            processes: 8,
            cpu_seconds: 20,
            operation_cpu_seconds: 0,
            address_space_bytes: 512 * 1024 * 1024,
            file_size_bytes: 16 * 1024 * 1024,
        },
        request_timeout_ms: 5_000,
        interfaces: DriverInterfaces {
            dynamic_capabilities: true,
            cooperative_cancellation: true,
            events: true,
            progress: true,
            artifacts: true,
            health: true,
            native_refs: false,
        },
    }
}

#[tokio::test]
async fn secure_windows_driver_host_roundtrips_protocol_v2() {
    let source = PathBuf::from(env!("CARGO_BIN_EXE_semwright-driver-fixture"));
    let binary_dir = tempfile::tempdir().expect("fixture directory");
    let executable = binary_dir.path().join("driver.exe");
    std::fs::copy(&source, &executable).expect("copy driver fixture");
    harden_fixture(&executable);

    let state = tempfile::tempdir().expect("driver state");
    let helper = std::env::current_exe().expect("current test executable");
    let provider = DriverProvider::connect(manifest(executable), state.path(), &helper, &[], false)
        .await
        .expect("Windows secure Driver Host connection");

    let interfaces = Provider::interfaces(provider.as_ref());
    assert!(interfaces.dynamic_capabilities);
    assert!(interfaces.cooperative_cancellation);
    assert!(interfaces.events);

    let capabilities = Provider::capabilities(provider.as_ref())
        .await
        .expect("driver capabilities");
    let ping = capabilities
        .iter()
        .find(|capability| capability.descriptor.name == "driver.fixture.ping")
        .expect("fixture ping capability")
        .descriptor
        .clone();

    let output = Provider::execute(
        provider.as_ref(),
        &Context {
            session: "windows-secure-host".into(),
            request_id: "windows-secure-host-ping".into(),
            cancellation: CancellationToken::new(),
        },
        &ping,
        &serde_json::json!({}),
    )
    .await
    .expect("fixture ping through secure Windows Driver Host");
    assert_eq!(output["ok"], true);

    Provider::shutdown(provider.as_ref())
        .await
        .expect("secure Windows Driver Host shutdown");
}

async fn execute_mount_probe(
    read_only: bool,
    owner_write: bool,
) -> (serde_json::Value, tempfile::TempDir) {
    let source = PathBuf::from(env!("CARGO_BIN_EXE_semwright-driver-fixture"));
    let binary_dir = tempfile::tempdir().expect("fixture directory");
    let executable = binary_dir.path().join("driver.exe");
    std::fs::copy(&source, &executable).expect("copy driver fixture");
    harden_fixture(&executable);

    let workspace = tempfile::tempdir().expect("workspace grant");
    std::fs::write(workspace.path().join("input.txt"), b"mounted-data")
        .expect("write workspace fixture");

    let mut manifest = manifest(executable);
    manifest.mounts = vec![DriverMount {
        root: "fixture-data".into(),
        read_only,
        execute: false,
    }];
    let roots = vec![FilesystemGrant {
        name: "fixture-data".into(),
        path: workspace.path().to_path_buf(),
        read: true,
        write: owner_write,
    }];

    let state = tempfile::tempdir().expect("driver state");
    let helper = std::env::current_exe().expect("current test executable");
    let provider = DriverProvider::connect(manifest, state.path(), &helper, &roots, false)
        .await
        .expect("Windows Driver Host workspace mount");

    let capabilities = Provider::capabilities(provider.as_ref())
        .await
        .expect("driver capabilities");
    let probe = capabilities
        .iter()
        .find(|capability| capability.descriptor.name == "driver.fixture.mount_probe")
        .expect("fixture mount capability")
        .descriptor
        .clone();
    let output = Provider::execute(
        provider.as_ref(),
        &Context {
            session: "windows-workspace-mount".into(),
            request_id: format!("windows-workspace-mount-{read_only}"),
            cancellation: CancellationToken::new(),
        },
        &probe,
        &serde_json::json!({}),
    )
    .await
    .expect("workspace probe through AppContainer");

    Provider::shutdown(provider.as_ref())
        .await
        .expect("workspace Driver Host shutdown");
    drop(binary_dir);
    (output, workspace)
}

#[tokio::test]
async fn secure_windows_driver_workspace_read_only_is_enforced() {
    let (output, workspace) = execute_mount_probe(true, false).await;
    assert_eq!(output["read"], "mounted-data");
    assert_eq!(output["write_ok"], false);
    assert!(!workspace.path().join("child.txt").exists());
}

#[tokio::test]
async fn secure_windows_driver_workspace_read_write_is_enforced() {
    let (output, workspace) = execute_mount_probe(false, true).await;
    assert_eq!(output["read"], "mounted-data");
    assert_eq!(output["write_ok"], true);
    assert_eq!(
        std::fs::read(workspace.path().join("child.txt")).expect("read driver-created file"),
        b"written"
    );
}
