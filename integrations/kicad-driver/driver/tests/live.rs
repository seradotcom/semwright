// SPDX-License-Identifier: GPL-3.0-or-later
use semwright_backend_api::{Context, Provider};
use semwright_driver_host::DriverProvider;
use semwright_driver_sdk::{
    ApplicationMatch, DriverInterfaces, DriverMount, DriverResources, Manifest, Transport,
};
use semwright_policy::FilesystemGrant;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    io::{BufRead, BufReader},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};
use tokio_util::sync::CancellationToken;

struct Fixture(Child);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn digest(path: &Path) -> String {
    format!("{:x}", Sha256::digest(std::fs::read(path).unwrap()))
}

async fn call(
    provider: &DriverProvider,
    capabilities: &[semwright_backend_api::ProvidedCapability],
    name: &str,
    args: Value,
) -> semwright_types::Result<Value> {
    let descriptor = &capabilities
        .iter()
        .find(|capability| capability.descriptor.name == name)
        .unwrap()
        .descriptor;
    Provider::execute(
        provider,
        &Context {
            session: "kicad-live".into(),
            request_id: semwright_types::unique_id(),
            cancellation: CancellationToken::new(),
        },
        descriptor,
        &args,
    )
    .await
}

#[tokio::test]
#[ignore = "requires bubblewrap and Python on a Linux host"]
async fn fake_kicad_ipc_runs_through_real_sandboxed_driver_host() {
    if std::env::var_os("SEMWRIGHT_TEST_KICAD").is_none() {
        return;
    }
    let cargo_executable = PathBuf::from(env!("CARGO_BIN_EXE_semwright-kicad-driver"));
    let binary_dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(binary_dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let executable = binary_dir.path().join("semwright-kicad-driver");
    std::fs::copy(&cargo_executable, &executable).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    let helper = PathBuf::from(
        std::env::var_os("SEMWRIGHT_TEST_SANDBOX_HELPER")
            .expect("SEMWRIGHT_TEST_SANDBOX_HELPER must point to semwright-sandbox"),
    );

    let config = tempfile::tempdir().unwrap();
    let ipc = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    for directory in [config.path(), ipc.path(), state.path()] {
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let socket = ipc.path().join("api.sock");
    let fixture_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/fake-kicad/server.py");
    let mut fixture = Command::new("python3")
        .arg("-u")
        .arg(fixture_path)
        .arg("--socket")
        .arg(&socket)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut ready = String::new();
    BufReader::new(fixture.stdout.take().unwrap())
        .read_line(&mut ready)
        .unwrap();
    assert!(ready.contains("\"ready\": true"));
    let _fixture = Fixture(fixture);

    let config_file = config.path().join("connection.json");
    std::fs::write(
        &config_file,
        serde_json::to_vec(&json!({
            "schema_version": 1,
            "instances": [{"id":"pcb-disposable","socket":"/workspace/kicad-ipc/api.sock"}],
            "selected_instance": "pcb-disposable",
            "allowed_documents": [],
            "enable_mutations": false,
            "timeout_ms": 750
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::set_permissions(&config_file, std::fs::Permissions::from_mode(0o600)).unwrap();

    let manifest = Manifest {
        manifest_version: 1,
        protocol: 1,
        id: "kicad".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        publisher: "semwright-tests".into(),
        executable: executable.clone(),
        sha256: digest(&executable),
        application: ApplicationMatch {
            desktop_id: Some("org.kicad.pcbnew".into()),
            process_names: vec!["pcbnew".into()],
            supported_versions: vec!["9".into(), "10".into()],
        },
        transport: Transport::StdioV1,
        mounts: vec![
            DriverMount {
                root: "kicad-config".into(),
                read_only: true,
            },
            DriverMount {
                root: "kicad-ipc".into(),
                read_only: true,
            },
        ],
        system_config: vec![],
        network: false,
        loopback_port: None,
        resources: DriverResources {
            address_space_bytes: 2_147_483_648,
            ..DriverResources::default()
        },
        request_timeout_ms: 30_000,
        interfaces: DriverInterfaces::default(),
    };
    let grants = vec![
        FilesystemGrant {
            name: "kicad-config".into(),
            path: config.path().canonicalize().unwrap(),
            read: true,
            write: false,
        },
        FilesystemGrant {
            name: "kicad-ipc".into(),
            path: ipc.path().canonicalize().unwrap(),
            read: true,
            write: false,
        },
    ];
    let provider = DriverProvider::connect(manifest, state.path(), &helper, &grants, false)
        .await
        .unwrap();
    let capabilities = Provider::capabilities(provider.as_ref()).await.unwrap();
    assert_eq!(capabilities.len(), 19);
    let version = call(
        provider.as_ref(),
        &capabilities,
        "driver.kicad.version",
        json!({}),
    )
    .await
    .unwrap();
    assert_eq!(version["major"], 10);
    assert_eq!(version["minor"], 0);
    assert_eq!(version["patch"], 6);
    assert_eq!(version["full"], "10.0.6");
    assert_eq!(version["supported"], true);
    let summary = call(
        provider.as_ref(),
        &capabilities,
        "driver.kicad.board.summary",
        json!({}),
    )
    .await
    .unwrap();
    assert_eq!(summary["footprints"], 1);
    assert_eq!(summary["tracks"], 1);
    assert_eq!(summary["vias"], 1);
    Provider::shutdown(provider.as_ref()).await.unwrap();
}
