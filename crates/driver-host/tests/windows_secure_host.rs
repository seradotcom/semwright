#![cfg(target_os = "windows")]

use semwright_backend_api::{Context, Provider};
use semwright_driver_host::DriverProvider;
use semwright_driver_sdk::{
    ApplicationMatch, DriverInterfaces, DriverMount, DriverResources, DriverSecretMount,
    DriverToolMount, Manifest, SystemConfigMount, Transport,
};
use semwright_policy::FilesystemGrant;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
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
            host_tools: false,
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

#[tokio::test]
async fn secure_windows_driver_sealed_tool_is_staged_immutable_and_executable() {
    let driver_source = PathBuf::from(env!("CARGO_BIN_EXE_semwright-driver-fixture"));
    let tool_source = PathBuf::from(env!("CARGO_BIN_EXE_semwright-tool-fixture"));

    let binary_dir = tempfile::tempdir().expect("fixture directory");
    let executable = binary_dir.path().join("driver.exe");
    let owner_tool = binary_dir.path().join("owner-tool.exe");
    std::fs::copy(&driver_source, &executable).expect("copy driver fixture");
    std::fs::copy(&tool_source, &owner_tool).expect("copy tool fixture");
    harden_fixture(&executable);
    harden_fixture(&owner_tool);

    let mut candidate = manifest(executable);
    if let Ok(value) = std::env::var("SEMWRIGHT_TEST_ADDRESS_SPACE_BYTES") {
        candidate.resources.address_space_bytes = value
            .parse()
            .expect("valid sealed-tool compatibility memory budget");
    }
    candidate.protocol = 4;
    candidate.interfaces.host_tools = true;
    candidate.tools = vec![DriverToolMount {
        root: "fixture-tool-root".into(),
        name: "probe".into(),
        sha256: digest(&owner_tool),
    }];
    let roots = vec![FilesystemGrant {
        name: "fixture-tool-root".into(),
        path: owner_tool.clone(),
        read: true,
        write: false,
    }];

    let state = tempfile::tempdir().expect("driver state");
    let helper = std::env::current_exe().expect("current test executable");
    let provider = DriverProvider::connect(candidate, state.path(), &helper, &roots, false)
        .await
        .expect("Windows Driver Host sealed tool");

    // The owner source is not the executable granted to the LPAC child. Mutating it after
    // connect must not change the Host-staged, re-attested tool.
    std::fs::write(&owner_tool, b"tampered-after-connect").expect("mutate owner tool source");

    let capabilities = Provider::capabilities(provider.as_ref())
        .await
        .expect("driver capabilities");
    let probe = capabilities
        .iter()
        .find(|capability| capability.descriptor.name == "driver.fixture.tool_probe")
        .expect("fixture tool capability")
        .descriptor
        .clone();
    let output = Provider::execute(
        provider.as_ref(),
        &Context {
            session: "windows-sealed-tool".into(),
            request_id: "windows-sealed-tool-probe".into(),
            cancellation: CancellationToken::new(),
        },
        &probe,
        &serde_json::json!({}),
    )
    .await
    .expect("sealed tool probe through LPAC");

    assert_eq!(
        output["spawn_error_kind"], "",
        "sealed tool spawn diagnostics: {output}"
    );
    assert_eq!(
        output["exit_code"], 0,
        "sealed tool exit diagnostics: {output}"
    );
    assert_eq!(
        output["stdout"], "tool-ok|appcontainer=1",
        "sealed tool must execute without breaking out of AppContainer: {output}"
    );
    assert_eq!(
        output["read_ok"], false,
        "Host-mediated sealed tools must not expose a direct executable path to the driver: {output}"
    );
    assert_eq!(output["execute_open_ok"], false);
    assert_eq!(output["self_spawn_ok"], false);
    assert_eq!(output["null_spawn_ok"], false);
    assert_eq!(output["write_ok"], false);

    Provider::shutdown(provider.as_ref())
        .await
        .expect("sealed tool Driver Host shutdown");
}

#[tokio::test]
async fn secure_windows_driver_sealed_tool_rejects_digest_mismatch() {
    let driver_source = PathBuf::from(env!("CARGO_BIN_EXE_semwright-driver-fixture"));
    let tool_source = PathBuf::from(env!("CARGO_BIN_EXE_semwright-tool-fixture"));

    let binary_dir = tempfile::tempdir().expect("fixture directory");
    let executable = binary_dir.path().join("driver.exe");
    let owner_tool = binary_dir.path().join("owner-tool.exe");
    std::fs::copy(&driver_source, &executable).expect("copy driver fixture");
    std::fs::copy(&tool_source, &owner_tool).expect("copy tool fixture");
    harden_fixture(&executable);
    harden_fixture(&owner_tool);

    let mut candidate = manifest(executable);
    candidate.protocol = 4;
    candidate.interfaces.host_tools = true;
    candidate.tools = vec![DriverToolMount {
        root: "fixture-tool-root".into(),
        name: "probe".into(),
        sha256: "0".repeat(64),
    }];
    let roots = vec![FilesystemGrant {
        name: "fixture-tool-root".into(),
        path: owner_tool,
        read: true,
        write: false,
    }];

    let state = tempfile::tempdir().expect("driver state");
    let helper = std::env::current_exe().expect("current test executable");
    let error = match DriverProvider::connect(candidate, state.path(), &helper, &roots, false).await
    {
        Ok(provider) => {
            let _ = Provider::shutdown(provider.as_ref()).await;
            panic!("sealed tool digest mismatch must fail before child launch");
        }
        Err(error) => error,
    };
    assert_eq!(error.code, semwright_types::ErrorCode::PermissionDenied);
}

fn free_loopback_port() -> u16 {
    let listener =
        std::net::TcpListener::bind(("127.0.0.1", 0)).expect("reserve loopback test port");
    let port = listener.local_addr().expect("loopback test address").port();
    drop(listener);
    port
}

#[tokio::test]
async fn secure_windows_driver_loopback_is_host_mediated_without_network_capability() {
    let source = PathBuf::from(env!("CARGO_BIN_EXE_semwright-driver-fixture"));
    let binary_dir = tempfile::tempdir().expect("fixture directory");
    let executable = binary_dir.path().join("driver.exe");
    std::fs::copy(&source, &executable).expect("copy driver fixture");
    harden_fixture(&executable);

    let port = free_loopback_port();
    let mut manifest = manifest(executable);
    manifest.loopback_port = Some(port);
    manifest.network = false;

    let state = tempfile::tempdir().expect("driver state");
    let helper = std::env::current_exe().expect("current test executable");
    let provider = DriverProvider::connect(manifest, state.path(), &helper, &[], false)
        .await
        .expect("Windows loopback Driver Host connection");

    let mut client = TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("connect to Host-owned loopback proxy");
    client
        .write_all(b"semwright-loopback")
        .await
        .expect("write Host loopback probe");
    let mut reply = [0u8; 18];
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        client.read_exact(&mut reply),
    )
    .await
    .expect("Host loopback round-trip timed out")
    .expect("read Host loopback reply");
    assert_eq!(&reply, b"semwright-loopback");

    Provider::shutdown(provider.as_ref())
        .await
        .expect("loopback Driver Host shutdown");
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

async fn execute_system_config_probe() -> (serde_json::Value, tempfile::TempDir) {
    let source = PathBuf::from(env!("CARGO_BIN_EXE_semwright-driver-fixture"));
    let binary_dir = tempfile::tempdir().expect("fixture directory");
    let executable = binary_dir.path().join("driver.exe");
    std::fs::copy(&source, &executable).expect("copy driver fixture");
    harden_fixture(&executable);

    let config = tempfile::tempdir().expect("system config grant");
    std::fs::write(config.path().join("config.txt"), b"system-config")
        .expect("write system config fixture");
    grant_all_application_packages_modify(config.path());

    let mut manifest = manifest(executable);
    manifest.system_config = vec![SystemConfigMount {
        root: "fixture-config-root".into(),
        destination: "/etc/fixture-config".into(),
    }];
    let roots = vec![FilesystemGrant {
        name: "fixture-config-root".into(),
        path: config.path().to_path_buf(),
        read: true,
        write: false,
    }];

    let state = tempfile::tempdir().expect("driver state");
    let helper = std::env::current_exe().expect("current test executable");
    let provider = DriverProvider::connect(manifest, state.path(), &helper, &roots, false)
        .await
        .expect("Windows Driver Host system config mount");

    let capabilities = Provider::capabilities(provider.as_ref())
        .await
        .expect("driver capabilities");
    let probe = capabilities
        .iter()
        .find(|capability| capability.descriptor.name == "driver.fixture.config_probe")
        .expect("fixture config capability")
        .descriptor
        .clone();
    let output = Provider::execute(
        provider.as_ref(),
        &Context {
            session: "windows-system-config".into(),
            request_id: "windows-system-config-read-only".into(),
            cancellation: CancellationToken::new(),
        },
        &probe,
        &serde_json::json!({}),
    )
    .await
    .expect("system config probe through LPAC");

    Provider::shutdown(provider.as_ref())
        .await
        .expect("system config Driver Host shutdown");
    drop(binary_dir);
    (output, config)
}

#[tokio::test]
async fn secure_windows_driver_system_config_is_read_only() {
    let (output, config) = execute_system_config_probe().await;
    assert_eq!(output["read"], "system-config");
    assert_eq!(output["write_ok"], false);
    assert!(!config.path().join("child.txt").exists());
}

async fn execute_secret_probe() -> (serde_json::Value, tempfile::TempDir) {
    let source = PathBuf::from(env!("CARGO_BIN_EXE_semwright-driver-fixture"));
    let binary_dir = tempfile::tempdir().expect("fixture directory");
    let executable = binary_dir.path().join("driver.exe");
    std::fs::copy(&source, &executable).expect("copy driver fixture");
    harden_fixture(&executable);

    let secret_dir = tempfile::tempdir().expect("secret grant");
    let secret_path = secret_dir.path().join("secret.txt");
    std::fs::write(&secret_path, b"owner-secret").expect("write secret fixture");
    harden_fixture(&secret_path);

    let mut manifest = manifest(executable);
    manifest.secrets = vec![DriverSecretMount {
        root: "fixture-secret-root".into(),
        name: "fixture-secret".into(),
    }];
    let roots = vec![FilesystemGrant {
        name: "fixture-secret-root".into(),
        path: secret_path.clone(),
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
            session: "windows-secret".into(),
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
    drop(binary_dir);
    (output, secret_dir)
}

#[tokio::test]
async fn secure_windows_driver_secret_is_private_and_read_only() {
    let (output, secret_dir) = execute_secret_probe().await;
    assert_eq!(output["read"], "owner-secret");
    assert_eq!(output["write_ok"], false);
    assert_eq!(
        std::fs::read(secret_dir.path().join("secret.txt")).expect("read secret after shutdown"),
        b"owner-secret"
    );
}

#[tokio::test]
async fn secure_windows_driver_secret_rejects_source_with_broad_mutation_acl() {
    let source = PathBuf::from(env!("CARGO_BIN_EXE_semwright-driver-fixture"));
    let binary_dir = tempfile::tempdir().expect("fixture directory");
    let executable = binary_dir.path().join("driver.exe");
    std::fs::copy(&source, &executable).expect("copy driver fixture");
    harden_fixture(&executable);

    let secret_dir = tempfile::tempdir().expect("secret grant");
    let secret_path = secret_dir.path().join("secret.txt");
    std::fs::write(&secret_path, b"owner-secret").expect("write secret fixture");
    harden_fixture(&secret_path);
    grant_all_application_packages_file_modify(&secret_path);

    let mut candidate = manifest(executable);
    candidate.secrets = vec![DriverSecretMount {
        root: "fixture-secret-root".into(),
        name: "fixture-secret".into(),
    }];
    let roots = vec![FilesystemGrant {
        name: "fixture-secret-root".into(),
        path: secret_path,
        read: true,
        write: false,
    }];
    let state = tempfile::tempdir().expect("driver state");
    let helper = std::env::current_exe().expect("current test executable");

    let error = match DriverProvider::connect(candidate, state.path(), &helper, &roots, false).await
    {
        Ok(provider) => {
            let _ = Provider::shutdown(provider.as_ref()).await;
            panic!("mutable secret source must be rejected before child launch");
        }
        Err(error) => error,
    };
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
    let error = match DriverProvider::connect(candidate, state.path(), &helper, &roots, false).await
    {
        Ok(_) => panic!("non-private secret source must be rejected before spawn"),
        Err(error) => error,
    };
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
