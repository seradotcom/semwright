#![cfg(target_os = "windows")]

use semwright_backend_api::{Context, Provider};
use semwright_driver_host::DriverProvider;
use semwright_driver_sdk::Manifest;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{path::PathBuf, process::Command, time::Duration};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn figma_authenticates_and_mutates_through_the_windows_host_pipe() {
    let executable = PathBuf::from(env!("CARGO_BIN_EXE_semwright-figma-driver"));
    let digest = hex::encode(Sha256::digest(
        std::fs::read(&executable).expect("driver bytes"),
    ));
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/windows/prepare-figma-driver.ps1");
    let install = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-File"])
        .arg(&script)
        .arg("-DriverPath")
        .arg(&executable)
        .arg("-ExpectedSha256")
        .arg(&digest)
        .output()
        .expect("owner installation fixture");
    assert!(
        install.status.success(),
        "installer failed: {}",
        String::from_utf8_lossy(&install.stderr)
    );
    let receipt: Value = serde_json::from_slice(&install.stdout).expect("installer JSON receipt");
    assert_eq!(receipt["policy_grants_changed"], false);
    let manifest_path = PathBuf::from(receipt["manifest"].as_str().expect("manifest path"));
    let manifest: Manifest =
        serde_json::from_slice(&std::fs::read(&manifest_path).expect("manifest bytes"))
            .expect("manifest JSON without a BOM");
    assert!(
        !manifest.network,
        "Figma must not gain ambient network authority"
    );
    assert_eq!(manifest.loopback_port, Some(38471));
    let state = tempfile::tempdir().expect("isolated Host state");
    let helper = PathBuf::from(
        std::env::var_os("SEMWRIGHT_TEST_SANDBOX_HELPER")
            .expect("build and select the native sandbox helper"),
    );
    let provider = DriverProvider::connect(manifest, state.path(), &helper, &[], false)
        .await
        .expect("start production Figma driver in Windows AppContainer");
    let caps = Provider::capabilities(provider.as_ref())
        .await
        .expect("capabilities");
    let call = |name: &'static str, args: Value| {
        let descriptor = caps
            .iter()
            .find(|cap| cap.descriptor.name == name)
            .expect("descriptor")
            .descriptor
            .clone();
        let provider = provider.clone();
        async move {
            let result = Provider::execute(
                provider.as_ref(),
                &Context {
                    session: "windows-figma-fixture".into(),
                    request_id: uuid::Uuid::new_v4().to_string(),
                    cancellation: CancellationToken::new(),
                },
                &descriptor,
                &args,
            )
            .await
            .expect("synthetic Figma capability");
            if name == "driver.figma.pairing.begin" {
                let validator = jsonschema::validator_for(&descriptor.output_schema)
                    .expect("pinned pairing output schema");
                assert!(
                    validator.is_valid(&result),
                    "Windows pairing result must pass Broker validation"
                );
            }
            result
        }
    };
    // Provider-only fixture access retrieves a synthetic pairing secret; production callers
    // still reach this operation through the Broker's SecretAccess approval gate.
    let pairing = call("driver.figma.pairing.begin", json!({})).await;
    let code = pairing["pairing_code"]
        .as_str()
        .expect("synthetic pairing code");
    let mut fake = tokio::process::Command::new(env!("CARGO_BIN_EXE_semwright-fake-figma"))
        .arg("ws://127.0.0.1:38471")
        .env("SEMWRIGHT_FIGMA_PAIRING_SECRET", code)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .expect("independent fake plugin");
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let sessions = call("driver.figma.session.list", json!({})).await;
            if !sessions.as_array().expect("sessions array").is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("authenticated session through Host-owned loopback");
    let before = call("driver.figma.document.status", json!({})).await;
    let created = call(
        "driver.figma.frame.create",
        json!({"name":"Windows pipe fixture"}),
    )
    .await;
    assert!(created.is_object());
    let after = call("driver.figma.document.status", json!({})).await;
    assert_ne!(
        before, after,
        "mutation must be observable through the authenticated bridge"
    );
    fake.kill().await.expect("stop fake plugin");
    fake.wait().await.expect("reap fake plugin");
    Provider::shutdown(provider.as_ref())
        .await
        .expect("contained driver shutdown");
    // The helper chooses a new private directory for each fixture; never delete a source tree.
    let installed = manifest_path.parent().expect("installed version directory");
    let root = PathBuf::from(std::env::var_os("LOCALAPPDATA").expect("LOCALAPPDATA"))
        .join("Semwright/drivers/figma");
    assert!(installed.starts_with(&root) && installed != root);
    std::fs::remove_dir_all(installed).expect("remove owned private fixture installation");
}
