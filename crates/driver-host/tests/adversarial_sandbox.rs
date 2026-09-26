#![cfg(all(target_os = "linux", feature = "test-tools"))]

use semwright_backend_api::{Context, Provider};
use semwright_driver_host::DriverProvider;
use semwright_driver_sdk::{
    ApplicationMatch, DriverInterfaces, DriverMount, DriverResources, DriverToolMount, Manifest,
    Transport,
};
use semwright_policy::FilesystemGrant;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    net::TcpListener,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

fn digest(path: &Path) -> String {
    format!("{:x}", Sha256::digest(std::fs::read(path).unwrap()))
}

fn fixture_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_semwright-adversarial-driver-fixture"))
}

fn sandbox_helper() -> PathBuf {
    PathBuf::from(
        std::env::var_os("SEMWRIGHT_TEST_SANDBOX_HELPER")
            .expect("SEMWRIGHT_TEST_SANDBOX_HELPER must point to semwright-sandbox"),
    )
}

async fn execute(
    provider: &DriverProvider,
    capabilities: &[semwright_backend_api::ProvidedCapability],
    name: &str,
    args: Value,
) -> semwright_types::Result<Value> {
    let descriptor = capabilities
        .iter()
        .find(|capability| capability.descriptor.name == name)
        .unwrap_or_else(|| panic!("missing capability {name}"));
    Provider::execute(
        provider,
        &Context {
            session: "adversarial-driver".into(),
            request_id: semwright_types::unique_id(),
            cancellation: CancellationToken::new(),
        },
        &descriptor.descriptor,
        &args,
    )
    .await
}

#[tokio::test]
#[ignore = "requires real Bubblewrap + Landlock support"]
async fn hostile_driver_is_confined_and_descendants_die_with_provider() {
    assert!(std::env::var_os("SEMWRIGHT_TEST_DRIVER_SANDBOX").is_some());
    assert!(Path::new("/usr/bin/bwrap").is_file());

    let state = tempfile::tempdir().unwrap();
    let ro = tempfile::tempdir().unwrap();
    let rw = tempfile::tempdir().unwrap();
    let secret = tempfile::tempdir().unwrap();
    let binary = tempfile::tempdir().unwrap();
    let tool = tempfile::tempdir().unwrap();
    for directory in [
        state.path(),
        ro.path(),
        rw.path(),
        secret.path(),
        binary.path(),
        tool.path(),
    ] {
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    }

    std::fs::write(ro.path().join("allowed.txt"), b"allowed").unwrap();
    let ro_tool = ro.path().join("tool");
    std::fs::copy("/usr/bin/true", &ro_tool).unwrap();
    std::fs::set_permissions(&ro_tool, std::fs::Permissions::from_mode(0o500)).unwrap();
    let secret_path = secret.path().join("host-secret.txt");
    std::fs::write(&secret_path, b"must-not-be-visible").unwrap();
    std::fs::set_permissions(&secret_path, std::fs::Permissions::from_mode(0o600)).unwrap();

    let executable = binary.path().join("driver");
    std::fs::copy(fixture_binary(), &executable).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o500)).unwrap();

    let tool_source = tool.path().join("probe");
    std::fs::copy("/usr/bin/true", &tool_source).unwrap();
    std::fs::set_permissions(&tool_source, std::fs::Permissions::from_mode(0o500)).unwrap();
    let tool_source = tool_source.canonicalize().unwrap();
    let tool_digest = digest(&tool_source);

    let resources = DriverResources {
        open_files: 64,
        processes: 16,
        cpu_seconds: 20,
        address_space_bytes: 536_870_912,
        file_size_bytes: 16_777_216,
    };
    let manifest = Manifest {
        manifest_version: 1,
        protocol: 1,
        id: "adversarial".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        publisher: "semwright-tests".into(),
        executable: executable.clone(),
        sha256: digest(&executable),
        application: ApplicationMatch {
            desktop_id: Some("org.semwright.AdversarialDriver".into()),
            process_names: vec![],
            supported_versions: vec![],
        },
        transport: Transport::StdioV1,
        mounts: vec![
            DriverMount {
                root: "ro".into(),
                read_only: true,
                execute: false,
            },
            DriverMount {
                root: "rw".into(),
                read_only: false,
                execute: false,
            },
        ],
        system_config: vec![],
        secrets: vec![],
        tools: vec![DriverToolMount {
            root: "probe-tool".into(),
            name: "probe".into(),
            sha256: tool_digest,
        }],
        network: false,
        loopback_port: None,
        resources,
        request_timeout_ms: 5_000,
        interfaces: DriverInterfaces::default(),
    };

    let ro_path = std::fs::canonicalize(ro.path()).unwrap();
    let rw_path = std::fs::canonicalize(rw.path()).unwrap();
    let grants = vec![
        FilesystemGrant {
            name: "ro".into(),
            path: ro_path,
            read: true,
            write: false,
        },
        FilesystemGrant {
            name: "rw".into(),
            path: rw_path.clone(),
            read: true,
            write: true,
        },
        FilesystemGrant {
            name: "probe-tool".into(),
            path: tool_source.clone(),
            read: true,
            write: false,
        },
    ];

    let provider =
        DriverProvider::connect(manifest, state.path(), &sandbox_helper(), &grants, false)
            .await
            .unwrap();
    let capabilities = Provider::capabilities(provider.as_ref()).await.unwrap();

    // The source disappears after the Host has verified/sealed it. The driver must
    // execute only the immutable /plugin/tools/probe mount retained by the provider.
    std::fs::remove_file(&tool_source).unwrap();
    let tool_result = execute(
        provider.as_ref(),
        &capabilities,
        "driver.adversarial.tool_probe",
        json!({}),
    )
    .await
    .unwrap();
    assert_eq!(tool_result["tool_executed"], true);

    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let result = execute(
        provider.as_ref(),
        &capabilities,
        "driver.adversarial.probe",
        json!({
            "host_pid": std::process::id(),
            "host_port": port,
            "host_secret": secret_path,
        }),
    )
    .await
    .unwrap();

    assert_eq!(result["allowed_read"], true);
    assert_eq!(result["allowed_write"], true);
    assert_eq!(result["readonly_write"], false);
    assert_eq!(result["readonly_execute"], false);
    assert_eq!(result["outside_home_write"], false);
    assert_eq!(result["outside_etc_write"], false);
    assert_eq!(result["host_secret_visible"], false);
    assert_eq!(result["host_pid_visible"], false);
    assert_eq!(result["host_loopback_connected"], false);
    assert_eq!(result["private_shm_write"], true);
    assert_eq!(result["nofile"], 64);

    let environment = result["environment"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        environment,
        vec![
            "HOME",
            "LANG",
            "PATH",
            "PWD",
            "SEMWRIGHT_DRIVER_SANDBOX",
            "XDG_CACHE_HOME",
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
        ]
    );

    execute(
        provider.as_ref(),
        &capabilities,
        "driver.adversarial.spawn_descendant",
        json!({}),
    )
    .await
    .unwrap();
    Provider::shutdown(provider.as_ref()).await.unwrap();
    tokio::time::sleep(Duration::from_millis(1400)).await;

    assert!(
        !rw_path.join("driver-descendant.txt").exists(),
        "a driver descendant survived provider shutdown"
    );
}
