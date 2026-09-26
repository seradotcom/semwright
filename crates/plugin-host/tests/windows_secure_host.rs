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

#[tokio::test]
async fn secure_windows_plugin_host_roundtrips_attested_protocol() {
    let source = std::path::PathBuf::from(env!("CARGO_BIN_EXE_semwright-windows-plugin-fixture"));
    let binary_dir = tempfile::tempdir().expect("fixture directory");
    let executable = binary_dir.path().join("plugin.exe");
    std::fs::copy(&source, &executable).expect("copy plugin fixture");
    harden_fixture(&executable);

    let manifest = Manifest {
        protocol: PLUGIN_PROTOCOL_VERSION,
        name: NAME.into(),
        version: VERSION.into(),
        executable: executable.clone(),
        sha256: digest(&executable),
        commands: commands(),
        mounts: vec![],
        network: false,
        timeout_ms: Some(5_000),
    };

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
}
