#![cfg(all(target_os = "linux", feature = "test-tools"))]

use semwright_backend_api::{Context, Provider};
use semwright_driver_host::DriverProvider;
use semwright_driver_sdk::{
    ApplicationMatch, DriverInterfaces, DriverMount, DriverResources, DriverToolMount, Manifest,
    Transport,
};
use semwright_policy::FilesystemGrant;
use semwright_types::ErrorCode;
use sha2::{Digest, Sha256};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
use tokio_util::sync::CancellationToken;

fn digest(path: &Path) -> String {
    format!(
        "{:x}",
        Sha256::digest(std::fs::read(path).expect("read runtime-tool fixture"))
    )
}

fn harden(path: &Path) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o500))
        .expect("harden runtime-tool fixture");
}

fn sandbox_helper() -> PathBuf {
    PathBuf::from(
        std::env::var_os("SEMWRIGHT_TEST_SANDBOX_HELPER")
            .expect("SEMWRIGHT_TEST_SANDBOX_HELPER must point to semwright-sandbox"),
    )
}

fn manifest(executable: PathBuf, tool_digest: String) -> Manifest {
    Manifest {
        manifest_version: 1,
        protocol: 5,
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
        mounts: vec![
            DriverMount {
                root: "tool-workspace".into(),
                read_only: false,
                execute: false,
            },
            DriverMount {
                root: "other-workspace".into(),
                read_only: false,
                execute: false,
            },
        ],
        system_config: vec![],
        secrets: vec![],
        tools: vec![DriverToolMount {
            root: "fixture-tool-root".into(),
            name: "probe".into(),
            sha256: tool_digest,
            mounts: vec!["tool-workspace".into()],
        }],
        network: false,
        loopback_port: None,
        resources: DriverResources {
            open_files: 128,
            processes: 16,
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
            host_tools: true,
        },
    }
}

#[tokio::test]
#[ignore = "requires real Bubblewrap + Landlock support"]
async fn linux_v5_runtime_tool_is_host_mediated_and_mount_scoped() {
    assert!(Path::new("/usr/bin/bwrap").is_file());

    let driver_source = PathBuf::from(env!("CARGO_BIN_EXE_semwright-driver-fixture"));
    let tool_source = PathBuf::from(env!("CARGO_BIN_EXE_semwright-tool-fixture"));
    let binary_dir = tempfile::tempdir().expect("fixture directory");
    let driver = binary_dir.path().join("driver");
    let tool = binary_dir.path().join("tool");
    std::fs::copy(&driver_source, &driver).expect("copy driver fixture");
    std::fs::copy(&tool_source, &tool).expect("copy tool fixture");
    harden(&driver);
    harden(&tool);

    let allowed = tempfile::tempdir().expect("allowed tool workspace");
    let denied = tempfile::tempdir().expect("driver-only workspace");
    std::fs::write(allowed.path().join("allowed.txt"), b"allowed").unwrap();
    std::fs::write(denied.path().join("denied.txt"), b"denied").unwrap();

    let roots = vec![
        FilesystemGrant {
            name: "fixture-tool-root".into(),
            path: tool.canonicalize().expect("canonical tool"),
            read: true,
            write: false,
        },
        FilesystemGrant {
            name: "tool-workspace".into(),
            path: allowed
                .path()
                .canonicalize()
                .expect("canonical allowed workspace"),
            read: true,
            write: true,
        },
        FilesystemGrant {
            name: "other-workspace".into(),
            path: denied
                .path()
                .canonicalize()
                .expect("canonical denied workspace"),
            read: true,
            write: true,
        },
    ];

    let state = tempfile::tempdir().expect("driver state");
    std::fs::set_permissions(state.path(), std::fs::Permissions::from_mode(0o700))
        .expect("harden driver state");
    let provider = DriverProvider::connect(
        manifest(driver, digest(&tool)),
        state.path(),
        &sandbox_helper(),
        &roots,
        false,
    )
    .await
    .expect("Linux v5 runtime-tool Driver Host");

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
            session: "linux-runtime-tool-cwd".into(),
            request_id: "linux-runtime-tool-cwd-allowed".into(),
            cancellation: CancellationToken::new(),
        },
        &probe,
        &serde_json::json!({"cwd_mount":"tool-workspace","cwd_relative":""}),
    )
    .await
    .expect("runtime tool should execute inside delegated Linux cwd");

    assert_eq!(output["exit_code"], 0);
    assert_eq!(
        output["read_ok"], false,
        "protocol-v5 Linux driver must not resolve a direct sealed-tool path: {output}"
    );
    assert_eq!(
        output["stdout"], "tool-ok|cwd=/workspace/tool-workspace",
        "Host-mediated Linux runtime tool must start at the logical delegated mount: {output}"
    );

    let error = Provider::execute(
        provider.as_ref(),
        &Context {
            session: "linux-runtime-tool-cwd".into(),
            request_id: "linux-runtime-tool-cwd-denied".into(),
            cancellation: CancellationToken::new(),
        },
        &probe,
        &serde_json::json!({"cwd_mount":"other-workspace","cwd_relative":""}),
    )
    .await
    .expect_err("Linux runtime tool must not inherit an undeclared workspace mount");
    assert_eq!(error.code, ErrorCode::PolicyDenied);

    Provider::shutdown(provider.as_ref())
        .await
        .expect("runtime-tool Driver Host shutdown");
}
