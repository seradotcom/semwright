#![cfg(target_os = "linux")]

use futures_util::{SinkExt, StreamExt};
use semwright_backend_api::{Context, Provider};
use semwright_driver_host::DriverProvider;
use semwright_driver_sdk::{
    ApplicationMatch, DRIVER_MANIFEST_VERSION, DRIVER_PROTOCOL_VERSION, DriverInterfaces,
    DriverMount, DriverResources, DriverSecretMount, Manifest, Transport,
};
use semwright_godot_driver::bridge::{proof, transcript};
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

fn free_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.local_addr().unwrap().port()
}

fn interfaces() -> DriverInterfaces {
    DriverInterfaces {
        cooperative_cancellation: true,
        events: true,
        progress: true,
        artifacts: true,
        health: true,
        native_refs: true,
        ..DriverInterfaces::default()
    }
}

fn manifest(executable: PathBuf, network: bool, loopback_port: Option<u16>) -> Manifest {
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
            },
            DriverMount {
                root: "godot-project".into(),
                read_only: false,
            },
        ],
        system_config: vec![],
        secrets: vec![DriverSecretMount {
            root: "godot-pairing".into(),
            name: "godot-pairing".into(),
        }],
        network,
        loopback_port,
        resources: DriverResources {
            open_files: 128,
            processes: 32,
            cpu_seconds: 60,
            address_space_bytes: 536_870_912,
            file_size_bytes: 16_777_216,
        },
        request_timeout_ms: 5_000,
        interfaces: interfaces(),
    }
}

async fn staged_driver() -> (tempfile::TempDir, PathBuf) {
    let cargo_executable = PathBuf::from(env!("CARGO_BIN_EXE_semwright-godot-driver"));
    let binary_dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(binary_dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let executable = binary_dir.path().join("semwright-godot-driver");
    std::fs::copy(&cargo_executable, &executable).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    (binary_dir, executable)
}

struct Fixture {
    _config: tempfile::TempDir,
    _project: tempfile::TempDir,
    _secret: tempfile::TempDir,
    roots: Vec<FilesystemGrant>,
    port: u16,
    project_id: String,
    secret: String,
}

fn fixture() -> Fixture {
    let config = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let secret_dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(config.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::set_permissions(project.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::set_permissions(secret_dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(project.path().join("project.godot"), "config_version=5\n").unwrap();

    let port = free_port();
    let project_id = "a".repeat(64);
    let secret = "b".repeat(64);
    let secret_path = secret_dir.path().join("pairing");
    std::fs::write(&secret_path, format!("{secret}\n")).unwrap();
    std::fs::set_permissions(&secret_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let secret_path = secret_path.canonicalize().unwrap();
    let config_path = config.path().join("config.json");
    std::fs::write(
        &config_path,
        serde_json::to_vec(&json!({
            "port": port,
            "development_mode": false,
            "projects": [{
                "project": project_id,
                "root": "/workspace/godot-project",
                "secret_file": "/run/secrets/godot-pairing"
            }],
            "runner": null
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
            name: "godot-project".into(),
            path: project.path().canonicalize().unwrap(),
            read: true,
            write: true,
        },
        FilesystemGrant {
            name: "godot-pairing".into(),
            path: secret_path,
            read: true,
            write: false,
        },
    ];
    Fixture {
        _config: config,
        _project: project,
        _secret: secret_dir,
        roots,
        port,
        project_id,
        secret,
    }
}

async fn call(
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
            session: "godot-host-conformance".into(),
            request_id: semwright_types::unique_id(),
            cancellation: CancellationToken::new(),
        },
        &descriptor.descriptor,
        &args,
    )
    .await
}

async fn wait_for_session(
    provider: &DriverProvider,
    capabilities: &[semwright_backend_api::ProvidedCapability],
) -> Value {
    for _ in 0..100 {
        let sessions = call(
            provider,
            capabilities,
            "driver.godot.session.list",
            json!({}),
        )
        .await
        .unwrap();
        if let Some(session) = sessions["sessions"]
            .as_array()
            .and_then(|values| values.first())
        {
            return session.clone();
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("fake Godot did not establish an authenticated session");
}

async fn fake_editor(port: u16, project: String, secret_hex: String) {
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}"))
        .await
        .unwrap();
    let client_nonce = "c".repeat(32);
    send(
        &mut ws,
        json!({
            "type":"hello",
            "project":project,
            "nonce":client_nonce,
            "plugin_version":"0.1.0",
            "engine_version":"4.7.2-stable (host-fixture)"
        }),
    )
    .await;
    let challenge = recv(&mut ws).await;
    let server_nonce = challenge["nonce"].as_str().unwrap();
    let generation = challenge["generation"].as_str().unwrap();
    let session = challenge["session"].as_str().unwrap();
    let secret = hex::decode(secret_hex).unwrap();
    assert_eq!(
        challenge["proof"],
        proof(
            &secret,
            &transcript(
                "server",
                &project,
                &client_nonce,
                server_nonce,
                generation,
                session
            )
        )
        .unwrap()
    );
    send(
        &mut ws,
        json!({
            "type":"authenticate",
            "proof":proof(
                &secret,
                &transcript(
                    "client",
                    &project,
                    &client_nonce,
                    server_nonce,
                    generation,
                    session
                )
            ).unwrap()
        }),
    )
    .await;
    assert_eq!(recv(&mut ws).await["type"], "ready");

    let request = recv(&mut ws).await;
    assert_eq!(request["op"], "project.inspect");
    send(
        &mut ws,
        json!({
            "type":"response",
            "id":request["id"],
            "ok":true,
            "value":{
                "stamp":{"revision":1,"fingerprint":"d".repeat(64)},
                "data":{
                    "name":"Sandbox fixture",
                    "engine":"4.7.2-stable (host-fixture)",
                    "scene":"",
                    "unsaved":false
                }
            }
        }),
    )
    .await;
}

async fn send(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    value: Value,
) {
    ws.send(tokio_tungstenite::tungstenite::Message::Text(
        value.to_string().into(),
    ))
    .await
    .unwrap();
}

async fn recv(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> Value {
    let message = ws.next().await.unwrap().unwrap();
    serde_json::from_slice(&message.into_data()).unwrap()
}

#[tokio::test]
#[ignore = "requires bubblewrap/Landlock sandbox helper"]
async fn godot_driver_runs_through_real_driver_host() {
    if std::env::var_os("SEMWRIGHT_TEST_GODOT").is_none() {
        return;
    }
    let (_binary_dir, executable) = staged_driver().await;
    let helper = PathBuf::from(
        std::env::var_os("SEMWRIGHT_TEST_SANDBOX_HELPER")
            .expect("SEMWRIGHT_TEST_SANDBOX_HELPER must point to semwright-sandbox"),
    );
    let state = tempfile::tempdir().unwrap();
    std::fs::set_permissions(state.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let fixture = fixture();

    let provider = DriverProvider::connect(
        manifest(executable, false, Some(fixture.port)),
        state.path(),
        &helper,
        &fixture.roots,
        false,
    )
    .await
    .unwrap();
    let provider_interfaces = provider.interfaces();
    assert!(!provider_interfaces.dynamic_capabilities);
    assert!(provider_interfaces.events);
    assert!(provider_interfaces.cooperative_cancellation);
    assert!(provider_interfaces.progress);
    assert!(provider_interfaces.artifacts);
    assert!(provider_interfaces.health);

    let capabilities = Provider::capabilities(provider.as_ref()).await.unwrap();
    // This host fixture deliberately omits runner configuration, so the six
    // digest-pinned headless/runtime capabilities must not be advertised.
    assert_eq!(capabilities.len(), 182);
    assert!(
        capabilities
            .iter()
            .all(|capability| capability.descriptor.name.starts_with("driver.godot."))
    );
    assert!(capabilities.iter().all(|capability| !matches!(
        capability.descriptor.name.as_str(),
        "driver.godot.project.validate"
            | "driver.godot.script.validate"
            | "driver.godot.project.run_test"
            | "driver.godot.export.pack"
            | "driver.godot.export.build"
            | "driver.godot.movie.capture"
    )));

    let fake = tokio::spawn(fake_editor(
        fixture.port,
        fixture.project_id.clone(),
        fixture.secret.clone(),
    ));
    let session = wait_for_session(provider.as_ref(), &capabilities).await;
    let value = call(
        provider.as_ref(),
        &capabilities,
        "driver.godot.project.inspect",
        json!({"session":session["session"]}),
    )
    .await
    .unwrap();
    assert_eq!(value["data"]["name"], "Sandbox fixture");
    assert_eq!(value["data"]["engine"], "4.7.2-stable (host-fixture)");

    fake.await.unwrap();
    Provider::shutdown(provider.as_ref()).await.unwrap();
}

#[tokio::test]
#[ignore = "requires sandbox helper; validates owner-controlled network grant"]
async fn godot_driver_network_requires_owner_opt_in() {
    if std::env::var_os("SEMWRIGHT_TEST_GODOT").is_none() {
        return;
    }
    let (_binary_dir, executable) = staged_driver().await;
    let helper = PathBuf::from(
        std::env::var_os("SEMWRIGHT_TEST_SANDBOX_HELPER")
            .expect("SEMWRIGHT_TEST_SANDBOX_HELPER must point to semwright-sandbox"),
    );
    let state = tempfile::tempdir().unwrap();
    std::fs::set_permissions(state.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let fixture = fixture();

    let error = match DriverProvider::connect(
        manifest(executable, true, None),
        state.path(),
        &helper,
        &fixture.roots,
        false,
    )
    .await
    {
        Ok(_) => panic!("networked Godot driver unexpectedly started without owner opt-in"),
        Err(error) => error,
    };
    assert_eq!(error.code, semwright_types::ErrorCode::PolicyDenied);
}
