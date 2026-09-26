use semwright_backend_api::{Context, Provider};
use semwright_driver_host::DriverProvider;
use semwright_driver_sdk::{
    ApplicationMatch, DriverInterfaces, DriverResources, Manifest, SystemConfigMount, Transport,
};
use semwright_policy::FilesystemGrant;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
};
use tokio_util::sync::CancellationToken;

fn digest(path: &Path) -> String {
    format!("{:x}", Sha256::digest(std::fs::read(path).unwrap()))
}

struct FakeObs {
    child: Child,
    port: u16,
}
impl FakeObs {
    async fn start() -> Self {
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/fake-obs/server.py");
        let mut child = Command::new("python3")
            .arg(script)
            .args(["--mode", "normal"])
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut reader = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut line))
            .await
            .unwrap()
            .unwrap();
        let port = serde_json::from_str::<Value>(&line).unwrap()["port"]
            .as_u64()
            .unwrap() as u16;
        Self { child, port }
    }
    async fn stop(mut self) {
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
    }
}

fn find<'a>(
    capabilities: &'a [semwright_backend_api::ProvidedCapability],
    name: &str,
) -> &'a semwright_backend_api::ProvidedCapability {
    capabilities
        .iter()
        .find(|capability| capability.descriptor.name == name)
        .unwrap()
}

async fn call(
    provider: &DriverProvider,
    capabilities: &[semwright_backend_api::ProvidedCapability],
    name: &str,
    args: Value,
) -> semwright_types::Result<Value> {
    Provider::execute(
        provider,
        &Context {
            session: "obs-host-conformance".into(),
            request_id: semwright_types::unique_id(),
            cancellation: CancellationToken::new(),
        },
        &find(capabilities, name).descriptor,
        &args,
    )
    .await
}

#[tokio::test]
#[ignore = "requires bubblewrap, Landlock, fake OBS networking and semwright-sandbox"]
async fn obs_driver_runs_through_real_driver_host() {
    if std::env::var_os("SEMWRIGHT_TEST_OBS").is_none() {
        return;
    }
    let fake = FakeObs::start().await;

    let cargo_executable = PathBuf::from(env!("CARGO_BIN_EXE_semwright-obs-driver"));
    let binary_dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(binary_dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let executable = binary_dir.path().join("semwright-obs-driver");
    std::fs::copy(&cargo_executable, &executable).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();

    let helper = PathBuf::from(
        std::env::var_os("SEMWRIGHT_TEST_SANDBOX_HELPER")
            .expect("SEMWRIGHT_TEST_SANDBOX_HELPER must point to semwright-sandbox"),
    );
    let state = tempfile::tempdir().unwrap();
    std::fs::set_permissions(state.path(), std::fs::Permissions::from_mode(0o700)).unwrap();

    let config = tempfile::tempdir().unwrap();
    std::fs::set_permissions(config.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let config_file = config.path().join("config.json");
    std::fs::write(
        &config_file,
        serde_json::to_vec(&json!({
            "address":"127.0.0.1",
            "port":fake.port,
            "connect_timeout_ms":2000,
            "request_timeout_ms":3000,
            "reconnect_limit":2,
            "event_capacity":64,
            "secret_socket":false,
            "allow_stream_start":false
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::set_permissions(&config_file, std::fs::Permissions::from_mode(0o600)).unwrap();

    let config_root = std::fs::canonicalize(config.path()).unwrap();
    let manifest = Manifest {
        manifest_version: 1,
        protocol: 1,
        id: "obs".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        publisher: "semwright-tests".into(),
        executable: executable.clone(),
        sha256: digest(&executable),
        application: ApplicationMatch {
            desktop_id: Some("com.obsproject.Studio".into()),
            process_names: vec!["obs".into()],
            supported_versions: vec![],
        },
        transport: Transport::StdioV1,
        mounts: vec![],
        system_config: vec![SystemConfigMount {
            root: "obs-config".into(),
            destination: "/etc/semwright-obs".into(),
        }],
        secrets: vec![],
        tools: vec![],
        network: true,
        loopback_port: None,
        resources: DriverResources {
            cpu_seconds: 60,
            address_space_bytes: 1_073_741_824,
            ..DriverResources::default()
        },
        request_timeout_ms: 5000,
        interfaces: DriverInterfaces::default(),
    };
    let grants = vec![FilesystemGrant {
        name: "obs-config".into(),
        path: config_root,
        read: true,
        write: false,
    }];

    let provider = DriverProvider::connect(manifest, state.path(), &helper, &grants, true)
        .await
        .unwrap();
    let capabilities = Provider::capabilities(provider.as_ref()).await.unwrap();
    assert_eq!(capabilities.len(), 65);
    assert!(
        capabilities
            .iter()
            .all(|capability| capability.descriptor.name.starts_with("driver.obs."))
    );

    let version = call(
        provider.as_ref(),
        &capabilities,
        "driver.obs.version",
        json!({}),
    )
    .await
    .unwrap();
    assert_eq!(version["untrusted"], true);
    assert_eq!(version["data"]["rpc_version"], 1);
    assert!(
        version["data"]["available_requests"]
            .as_array()
            .unwrap()
            .len()
            > 20
    );

    let status = call(
        provider.as_ref(),
        &capabilities,
        "driver.obs.status",
        json!({}),
    )
    .await
    .unwrap();
    assert_eq!(status["data"]["connected"], true);
    assert_eq!(status["data"]["authenticated"], false);

    Provider::shutdown(provider.as_ref()).await.unwrap();
    fake.stop().await;
}

#[tokio::test]
#[ignore = "requires semwright-sandbox; verifies owner network consent fails closed"]
async fn obs_driver_network_requires_owner_opt_in() {
    if std::env::var_os("SEMWRIGHT_TEST_OBS").is_none() {
        return;
    }
    let cargo_executable = PathBuf::from(env!("CARGO_BIN_EXE_semwright-obs-driver"));
    let binary_dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(binary_dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let executable = binary_dir.path().join("semwright-obs-driver");
    std::fs::copy(&cargo_executable, &executable).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    let helper = PathBuf::from(
        std::env::var_os("SEMWRIGHT_TEST_SANDBOX_HELPER")
            .expect("SEMWRIGHT_TEST_SANDBOX_HELPER must point to semwright-sandbox"),
    );
    let state = tempfile::tempdir().unwrap();
    std::fs::set_permissions(state.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let manifest = Manifest {
        manifest_version: 1,
        protocol: 1,
        id: "obs".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        publisher: "semwright-tests".into(),
        executable: executable.clone(),
        sha256: digest(&executable),
        application: ApplicationMatch {
            desktop_id: Some("com.obsproject.Studio".into()),
            process_names: vec!["obs".into()],
            supported_versions: vec![],
        },
        transport: Transport::StdioV1,
        mounts: vec![],
        system_config: vec![],
        secrets: vec![],
        tools: vec![],
        network: true,
        loopback_port: None,
        resources: DriverResources::default(),
        request_timeout_ms: 2000,
        interfaces: DriverInterfaces::default(),
    };
    let error = match DriverProvider::connect(manifest, state.path(), &helper, &[], false).await {
        Ok(_) => panic!("network driver unexpectedly started without owner opt-in"),
        Err(error) => error,
    };
    assert_eq!(error.code, semwright_types::ErrorCode::PolicyDenied);
}
