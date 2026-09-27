#![cfg(target_os = "windows")]

use semwright_backend_api::{Context, Provider};
use semwright_driver_host::DriverProvider;
use semwright_driver_sdk::{
    ApplicationMatch, DriverInterfaces, DriverMount, DriverResources, DriverSecretMount, Manifest,
    Transport,
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

fn grant_all_application_packages_modify(path: &Path) {
    // S-1-15-2-1 is ALL APPLICATION PACKAGES. Granting Modify here creates the
    // adversarial broad-group allow that the per-AppContainer deny ACE must override.
    let status = std::process::Command::new("icacls")
        .arg(path)
        .arg("/grant")
        .arg("*S-1-15-2-1:(OI)(CI)(M)")
        .status()
        .expect("grant ALL APPLICATION PACKAGES modify");
    assert!(
        status.success(),
        "broad application-package workspace grant must succeed"
    );
}

fn grant_all_application_packages_file_modify(path: &Path) {
    let status = std::process::Command::new("icacls")
        .arg(path)
        .arg("/grant")
        .arg("*S-1-15-2-1:(M)")
        .status()
        .expect("grant ALL APPLICATION PACKAGES file modify");
    assert!(
        status.success(),
        "broad application-package secret mutation grant must succeed"
    );
}

fn grant_all_application_packages_file_read(path: &Path) {
    let status = std::process::Command::new("icacls")
        .arg(path)
        .arg("/grant")
        .arg("*S-1-15-2-1:(R)")
        .status()
        .expect("grant ALL APPLICATION PACKAGES file read");
    assert!(
        status.success(),
        "broad application-package secret read grant must succeed"
    );
}

async fn execute_mount_probe(
    read_only: bool,
    owner_write: bool,
    broad_app_write: bool,
) -> (serde_json::Value, tempfile::TempDir) {
    let source = PathBuf::from(env!("CARGO_BIN_EXE_semwright-driver-fixture"));
    let binary_dir = tempfile::tempdir().expect("fixture directory");
    let executable = binary_dir.path().join("driver.exe");
    std::fs::copy(&source, &executable).expect("copy driver fixture");
    harden_fixture(&executable);

    let workspace = tempfile::tempdir().expect("workspace grant");
    std::fs::write(workspace.path().join("input.txt"), b"mounted-data")
        .expect("write workspace fixture");
    if broad_app_write {
        grant_all_application_packages_modify(workspace.path());
    }

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
    let (output, workspace) = execute_mount_probe(true, false, true).await;
    assert_eq!(output["read"], "mounted-data");
    assert_eq!(output["write_ok"], false);
    assert!(!workspace.path().join("child.txt").exists());
}

#[tokio::test]
async fn secure_windows_driver_workspace_read_write_is_enforced() {
    let (output, workspace) = execute_mount_probe(false, true, false).await;
    assert_eq!(output["read"], "mounted-data");
    assert_eq!(output["write_ok"], true);
    assert_eq!(
        std::fs::read(workspace.path().join("child.txt")).expect("read driver-created file"),
        b"written"
    );
}

async fn execute_secret_probe(secret: &Path) -> serde_json::Value {
    let source = PathBuf::from(env!("CARGO_BIN_EXE_semwright-driver-fixture"));
    let binary_dir = tempfile::tempdir().expect("fixture directory");
    let executable = binary_dir.path().join("driver.exe");
    std::fs::copy(&source, &executable).expect("copy driver fixture");
    harden_fixture(&executable);

    let mut manifest = manifest(executable);
    manifest.secrets = vec![DriverSecretMount {
        root: "fixture-secret-root".into(),
        name: "fixture-secret".into(),
    }];
    let roots = vec![FilesystemGrant {
        name: "fixture-secret-root".into(),
        path: secret.to_path_buf(),
        read: true,
        write: false,
    }];

    let state = tempfile::tempdir().expect("driver state");
    let helper = std::env::current_exe().expect("current test executable");
    let provider = DriverProvider::connect(manifest, state.path(), &helper, &roots, false)
        .await
        .expect("Windows Driver Host secret mount");
    let capabilities = Provider::capabilities(provider.as_ref())
        .await
        .expect("driver capabilities");
    let probe = capabilities
        .iter()
        .find(|capability| capability.descriptor.name == "driver.fixture.secret_probe")
        .expect("fixture secret capability")
        .descriptor
        .clone();
    let output = Provider::execute(
        provider.as_ref(),
        &Context {
            session: "windows-secret-mount".into(),
            request_id: "windows-secret-read-only".into(),
            cancellation: CancellationToken::new(),
        },
        &probe,
        &serde_json::json!({}),
    )
    .await
    .expect("secret probe through LPAC");
    Provider::shutdown(provider.as_ref())
        .await
        .expect("secret Driver Host shutdown");
    output
}

#[tokio::test]
async fn secure_windows_driver_secret_is_private_and_read_only() {
    let directory = tempfile::tempdir().expect("secret directory");
    let secret = directory.path().join("secret.txt");
    std::fs::write(&secret, b"sensitive-secret").expect("write secret");
    harden_fixture(&secret);

    let output = execute_secret_probe(&secret).await;
    assert_eq!(output["read"], "sensitive-secret");
    assert_eq!(output["write_ok"], false);
    assert_eq!(
        std::fs::read(&secret).expect("read unchanged secret"),
        b"sensitive-secret"
    );
}

#[tokio::test]
async fn secure_windows_driver_rejects_mutable_secret_source_before_spawn() {
    let source = PathBuf::from(env!("CARGO_BIN_EXE_semwright-driver-fixture"));
    let binary_dir = tempfile::tempdir().expect("fixture directory");
    let executable = binary_dir.path().join("driver.exe");
    std::fs::copy(&source, &executable).expect("copy driver fixture");
    harden_fixture(&executable);

    let directory = tempfile::tempdir().expect("secret directory");
    let secret = directory.path().join("secret.txt");
    std::fs::write(&secret, b"sensitive-secret").expect("write secret");
    harden_fixture(&secret);
    grant_all_application_packages_file_modify(&secret);

    let mut candidate = manifest(executable);
    candidate.secrets = vec![DriverSecretMount {
        root: "fixture-secret-root".into(),
        name: "fixture-secret".into(),
    }];
    let roots = vec![FilesystemGrant {
        name: "fixture-secret-root".into(),
        path: secret,
        read: true,
        write: false,
    }];
    let state = tempfile::tempdir().expect("driver state");
    let helper = std::env::current_exe().expect("current test executable");
    let error = DriverProvider::connect(candidate, state.path(), &helper, &roots, false)
        .await
        .expect_err("mutable secret source must be rejected before spawn");
    assert_eq!(error.code, semwright_types::ErrorCode::PermissionDenied);
}

#[tokio::test]
async fn secure_windows_driver_rejects_nonprivate_secret_source_before_spawn() {
    let source = PathBuf::from(env!("CARGO_BIN_EXE_semwright-driver-fixture"));
    let binary_dir = tempfile::tempdir().expect("fixture directory");
    let executable = binary_dir.path().join("driver.exe");
    std::fs::copy(&source, &executable).expect("copy driver fixture");
    harden_fixture(&executable);

    let directory = tempfile::tempdir().expect("secret directory");
    let secret = directory.path().join("secret.txt");
    std::fs::write(&secret, b"sensitive-secret").expect("write secret");
    harden_fixture(&secret);
    grant_all_application_packages_file_read(&secret);

    let mut candidate = manifest(executable);
    candidate.secrets = vec![DriverSecretMount {
        root: "fixture-secret-root".into(),
        name: "fixture-secret".into(),
    }];
    let roots = vec![FilesystemGrant {
        name: "fixture-secret-root".into(),
        path: secret,
        read: true,
        write: false,
    }];
    let state = tempfile::tempdir().expect("driver state");
    let helper = std::env::current_exe().expect("current test executable");
    let error = DriverProvider::connect(candidate, state.path(), &helper, &roots, false)
        .await
        .expect_err("non-private secret source must be rejected before spawn");
    assert_eq!(error.code, semwright_types::ErrorCode::PermissionDenied);
}

#[tokio::test]
async fn secure_windows_driver_operation_cpu_budget_terminates_job() {
    let source = PathBuf::from(env!("CARGO_BIN_EXE_semwright-driver-fixture"));
    let binary_dir = tempfile::tempdir().expect("fixture directory");
    let executable = binary_dir.path().join("driver.exe");
    std::fs::copy(&source, &executable).expect("copy driver fixture");
    harden_fixture(&executable);

    let state = tempfile::tempdir().expect("driver state");
    let helper = std::env::current_exe().expect("current test executable");
    let mut candidate = manifest(executable);
    candidate.resources.operation_cpu_seconds = 1;
    let provider = DriverProvider::connect(candidate, state.path(), &helper, &[], false)
        .await
        .expect("Windows Driver Host with Job CPU accounting");

    let capabilities = Provider::capabilities(provider.as_ref())
        .await
        .expect("driver capabilities");
    let ping = capabilities
        .iter()
        .find(|capability| capability.descriptor.name == "driver.fixture.ping")
        .expect("fixture ping capability")
        .descriptor
        .clone();

    let error = Provider::execute(
        provider.as_ref(),
        &Context {
            session: "windows-operation-cpu".into(),
            request_id: "windows-operation-cpu-spin".into(),
            cancellation: CancellationToken::new(),
        },
        &ping,
        &serde_json::json!({"cpu_ms":3000}),
    )
    .await
    .expect_err("CPU-heavy operation must exceed the one-second Job budget");
    assert_eq!(error.code, semwright_types::ErrorCode::ResourceExhausted);

    let _ = Provider::shutdown(provider.as_ref()).await;
}

#[tokio::test]
async fn concurrent_windows_operations_do_not_charge_each_others_cpu() {
    let source = PathBuf::from(env!("CARGO_BIN_EXE_semwright-driver-fixture"));
    let binary_dir = tempfile::tempdir().expect("fixture directory");
    let executable = binary_dir.path().join("driver.exe");
    std::fs::copy(&source, &executable).expect("copy driver fixture");
    harden_fixture(&executable);

    let state = tempfile::tempdir().expect("driver state");
    let helper = std::env::current_exe().expect("current test executable");
    let mut candidate = manifest(executable);
    candidate.resources.operation_cpu_seconds = 1;
    let provider = DriverProvider::connect(candidate, state.path(), &helper, &[], false)
        .await
        .expect("Windows Driver Host with serialized Job CPU accounting");

    let capabilities = Provider::capabilities(provider.as_ref())
        .await
        .expect("driver capabilities");
    let ping = capabilities
        .iter()
        .find(|capability| capability.descriptor.name == "driver.fixture.ping")
        .expect("fixture ping capability")
        .descriptor
        .clone();
    let first_ping = ping.clone();
    let second_ping = ping.clone();
    let first_context = Context {
        session: "windows-operation-cpu-concurrent".into(),
        request_id: "windows-operation-cpu-first".into(),
        cancellation: CancellationToken::new(),
    };
    let second_context = Context {
        session: "windows-operation-cpu-concurrent".into(),
        request_id: "windows-operation-cpu-second".into(),
        cancellation: CancellationToken::new(),
    };

    let first_args = serde_json::json!({"cpu_ms":650});
    let second_args = serde_json::json!({"cpu_ms":650});
    let first = Provider::execute(provider.as_ref(), &first_context, &first_ping, &first_args);
    let second = Provider::execute(
        provider.as_ref(),
        &second_context,
        &second_ping,
        &second_args,
    );
    let (first, second) = tokio::join!(first, second);
    assert_eq!(first.expect("first CPU-bounded operation")["ok"], true);
    assert_eq!(second.expect("second CPU-bounded operation")["ok"], true);

    let _ = Provider::shutdown(provider.as_ref()).await;
}
