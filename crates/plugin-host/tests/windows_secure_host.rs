#![cfg(all(target_os = "windows", feature = "test-tools"))]

use semwright_plugin_host::{
    Host,
    windows_fixture::{COMMAND, NAME, VERSION, commands},
};
use semwright_plugin_sdk::{Manifest, PLUGIN_PROTOCOL_VERSION};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::Path;
use tokio_util::sync::CancellationToken;

fn digest(path: &Path) -> String {
    format!(
        "{:x}",
        Sha256::digest(std::fs::read(path).expect("read plugin fixture"))
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

const EXTERNAL_ENDPOINTS: [&str; 2] = ["1.1.1.1:443", "1.0.0.1:443"];

fn manifest(executable: std::path::PathBuf, network: bool) -> Manifest {
    Manifest {
        protocol: PLUGIN_PROTOCOL_VERSION,
        name: NAME.into(),
        version: VERSION.into(),
        executable: executable.clone(),
        sha256: digest(&executable),
        commands: commands(),
        mounts: vec![],
        network,
        timeout_ms: Some(5_000),
    }
}

async fn plugin_probe(
    host: &Host,
    address: &str,
    host_path: &Path,
    message: &str,
) -> serde_json::Value {
    host.execute(
        COMMAND,
        json!({
            "message": message,
            "address": address,
            "host_path": host_path.to_string_lossy(),
        }),
        CancellationToken::new(),
    )
    .await
    .expect("execute Windows plugin network probe")
}

#[tokio::test]
async fn secure_windows_plugin_host_roundtrips_attested_protocol() {
    let source = std::path::PathBuf::from(env!("CARGO_BIN_EXE_semwright-windows-plugin-fixture"));
    let binary_dir = tempfile::tempdir().expect("fixture directory");
    let executable = binary_dir.path().join("plugin.exe");
    std::fs::copy(&source, &executable).expect("copy plugin fixture");
    harden_fixture(&executable);

    let manifest = manifest(executable.clone(), false);

    let state = tempfile::tempdir().expect("plugin state");
    let helper = std::env::current_exe().expect("current test executable");
    let host = Host::new(state.path().to_path_buf(), helper, vec![], false)
        .expect("construct Plugin Host");
    host.install(manifest).expect("install plugin fixture");

    let value = host
        .execute(
            COMMAND,
            json!({"message":"secure-plugin"}),
            CancellationToken::new(),
        )
        .await
        .expect("execute Windows secure plugin");

    assert_eq!(value["message"], "secure-plugin");
    assert_eq!(value["path_present"], false);
    assert_eq!(value["local_app_data_present"], true);
    assert_eq!(value["temp_present"], true);
    assert_eq!(value["network_reachable"], false);
    assert_eq!(value["host_visible"], false);
}

#[tokio::test]
async fn secure_windows_plugin_network_is_owner_gated_internet_only_and_restart_stable() {
    let source = std::path::PathBuf::from(env!("CARGO_BIN_EXE_semwright-windows-plugin-fixture"));
    let binary_dir = tempfile::tempdir().expect("fixture directory");
    let executable = binary_dir.path().join("plugin.exe");
    std::fs::copy(&source, &executable).expect("copy plugin fixture");
    harden_fixture(&executable);

    let host_dir = tempfile::tempdir().expect("host-only directory");
    let host_path = host_dir.path().join("host-only.txt");
    std::fs::write(&host_path, b"must-remain-host-only").expect("write host-only fixture");
    harden_fixture(&host_path);
    let helper = std::env::current_exe().expect("current test executable");

    let denied_state = tempfile::tempdir().expect("denied plugin state");
    let denied_host = Host::new(
        denied_state.path().to_path_buf(),
        helper.clone(),
        vec![],
        false,
    )
    .expect("construct owner-denied Plugin Host");
    let error = denied_host
        .install(manifest(executable.clone(), true))
        .expect_err("owner-disabled Plugin network must be denied before spawn");
    assert_eq!(error.code, semwright_types::ErrorCode::PolicyDenied);

    let offline_state = tempfile::tempdir().expect("offline plugin state");
    let offline_host = Host::new(
        offline_state.path().to_path_buf(),
        helper.clone(),
        vec![],
        true,
    )
    .expect("construct offline Plugin Host");
    offline_host
        .install(manifest(executable.clone(), false))
        .expect("install network-denied plugin");
    for (index, endpoint) in EXTERNAL_ENDPOINTS.iter().enumerate() {
        let value = plugin_probe(
            &offline_host,
            endpoint,
            &host_path,
            &format!("offline-{index}"),
        )
        .await;
        assert_eq!(value["network_reachable"], false);
        assert_eq!(value["host_visible"], false);
    }
    let denied_listener =
        std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind denied host loopback");
    let denied_loopback = denied_listener
        .local_addr()
        .expect("denied loopback address")
        .to_string();
    let value = plugin_probe(
        &offline_host,
        &denied_loopback,
        &host_path,
        "offline-loopback",
    )
    .await;
    assert_eq!(value["network_reachable"], false);
    assert_eq!(value["host_visible"], false);

    let allowed_state = tempfile::tempdir().expect("allowed plugin state");
    let allowed_host = Host::new(allowed_state.path().to_path_buf(), helper, vec![], true)
        .expect("construct network-enabled Plugin Host");
    allowed_host
        .install(manifest(executable, true))
        .expect("install owner-authorized network plugin");

    let mut reachable = false;
    for (index, endpoint) in EXTERNAL_ENDPOINTS.iter().enumerate() {
        let value = plugin_probe(
            &allowed_host,
            endpoint,
            &host_path,
            &format!("allowed-{index}"),
        )
        .await;
        assert_eq!(value["host_visible"], false);
        reachable |= value["network_reachable"].as_bool().unwrap_or(false);
        if reachable {
            break;
        }
    }
    assert!(
        reachable,
        "internetClient must permit at least one stable outbound Internet endpoint"
    );

    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind host loopback");
    let loopback = listener.local_addr().expect("loopback address").to_string();
    let value = plugin_probe(&allowed_host, &loopback, &host_path, "loopback").await;
    assert_eq!(
        value["network_reachable"], false,
        "internetClient must not grant direct host loopback"
    );
    assert_eq!(value["host_visible"], false);

    let mut restarted_reachable = false;
    for (index, endpoint) in EXTERNAL_ENDPOINTS.iter().enumerate() {
        let value = plugin_probe(
            &allowed_host,
            endpoint,
            &host_path,
            &format!("restart-{index}"),
        )
        .await;
        assert_eq!(value["host_visible"], false);
        restarted_reachable |= value["network_reachable"].as_bool().unwrap_or(false);
        if restarted_reachable {
            break;
        }
    }
    assert!(
        restarted_reachable,
        "per-invocation Plugin Host restart must preserve network authority"
    );
}
