#![cfg(all(target_os = "linux", feature = "test-tools"))]

use semwright_plugin_host::{Host, adversarial_fixture};
use semwright_plugin_sdk::{Manifest, Mount, PLUGIN_PROTOCOL_VERSION};
use semwright_policy::FilesystemGrant;
use semwright_types::{ErrorCode, Result};
use serde_json::json;
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
    PathBuf::from(env!("CARGO_BIN_EXE_semwright-adversarial-plugin-fixture"))
}

fn sandbox_helper() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_semwright-sandbox"))
}

struct Harness {
    _state: tempfile::TempDir,
    _ro: tempfile::TempDir,
    _rw: tempfile::TempDir,
    _secret: tempfile::TempDir,
    _binary: tempfile::TempDir,
    host: Host,
    manifest: Manifest,
    rw_path: PathBuf,
    secret_path: PathBuf,
}

impl Harness {
    fn new() -> Self {
        let state = tempfile::tempdir().unwrap();
        let ro = tempfile::tempdir().unwrap();
        let rw = tempfile::tempdir().unwrap();
        let secret = tempfile::tempdir().unwrap();
        let binary = tempfile::tempdir().unwrap();

        for directory in [
            state.path(),
            ro.path(),
            rw.path(),
            secret.path(),
            binary.path(),
        ] {
            std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        std::fs::write(ro.path().join("allowed.txt"), b"allowed").unwrap();
        let secret_path = secret.path().join("host-secret.txt");
        std::fs::write(&secret_path, b"must-not-be-visible").unwrap();
        std::fs::set_permissions(&secret_path, std::fs::Permissions::from_mode(0o600)).unwrap();

        let executable = binary.path().join("plugin");
        std::fs::copy(fixture_binary(), &executable).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o500)).unwrap();
        let manifest = Manifest {
            protocol: PLUGIN_PROTOCOL_VERSION,
            name: adversarial_fixture::NAME.into(),
            version: adversarial_fixture::VERSION.into(),
            executable: executable.clone(),
            sha256: digest(&executable),
            commands: adversarial_fixture::commands(),
            mounts: vec![
                Mount {
                    root: "ro".into(),
                    read_only: true,
                },
                Mount {
                    root: "rw".into(),
                    read_only: false,
                },
            ],
            network: false,
            timeout_ms: Some(1500),
        };
        let ro_path = std::fs::canonicalize(ro.path()).unwrap();
        let rw_path = std::fs::canonicalize(rw.path()).unwrap();
        let roots = vec![
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
        ];
        let host = Host::new(
            std::fs::canonicalize(state.path()).unwrap(),
            sandbox_helper(),
            roots,
            false,
        )
        .unwrap();
        Self {
            _state: state,
            _ro: ro,
            _rw: rw,
            _secret: secret,
            _binary: binary,
            host,
            manifest,
            rw_path,
            secret_path,
        }
    }

    fn install(&self) {
        self.host.install(self.manifest.clone()).unwrap();
    }
}

fn require_real_sandbox() {
    assert!(
        std::env::var_os("SEMWRIGHT_TEST_PLUGIN_SANDBOX").is_some(),
        "set SEMWRIGHT_TEST_PLUGIN_SANDBOX=1 only in an isolated Linux sandbox test environment"
    );
    assert!(Path::new("/usr/bin/bwrap").is_file());
    assert!(sandbox_helper().is_file());
}

#[tokio::test]
#[ignore = "requires real Bubblewrap + Landlock support"]
async fn hostile_plugin_cannot_escape_filesystem_network_process_or_environment() -> Result<()> {
    require_real_sandbox();
    let harness = Harness::new();
    harness.install();

    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let value = harness
        .host
        .execute(
            "plugin.adversarial.probe",
            json!({
                "host_pid": std::process::id(),
                "host_port": port,
                "host_secret": harness.secret_path,
            }),
            CancellationToken::new(),
        )
        .await?;

    assert_eq!(value["allowed_read"], true);
    assert_eq!(value["allowed_write"], true);
    assert_eq!(value["readonly_write"], false);
    assert_eq!(value["outside_home_write"], false);
    assert_eq!(value["outside_etc_write"], false);
    assert_eq!(value["host_secret_visible"], false);
    assert_eq!(value["host_pid_visible"], false);
    assert_eq!(value["host_loopback_connected"], false);

    let environment = value["environment"].as_array().unwrap();
    let keys = environment
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect::<Vec<_>>();
    // Bubblewrap sets PWD after --chdir /tmp. No host-provided environment
    // variables survive the explicit clearenv/allowlist boundary.
    assert_eq!(keys, vec!["HOME", "LANG", "PATH", "PWD"]);
    assert_eq!(
        std::fs::read(harness.rw_path.join("allowed.txt")).unwrap(),
        b"allowed"
    );
    assert!(!harness.rw_path.join("descendant.txt").exists());
    Ok(())
}

#[tokio::test]
#[ignore = "requires real Bubblewrap + Landlock support"]
async fn timeout_watchdog_kills_descendants_before_they_can_mutate_grants() {
    require_real_sandbox();
    let harness = Harness::new();
    harness.install();

    let error = harness
        .host
        .execute(
            "plugin.adversarial.hang",
            json!({}),
            CancellationToken::new(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Timeout);

    tokio::time::sleep(Duration::from_millis(1400)).await;
    assert!(
        !harness.rw_path.join("descendant.txt").exists(),
        "a descendant survived the watchdog and mutated a writable grant"
    );
}

#[tokio::test]
#[ignore = "requires real Bubblewrap + Landlock support"]
async fn manifest_version_and_descriptor_digest_are_attested_by_the_child() {
    require_real_sandbox();

    // A mismatched child is meaningful evidence only after the same sandbox/fixture
    // has completed a valid handshake and invocation in this environment.
    let control = Harness::new();
    control.install();
    control
        .host
        .execute(
            "plugin.adversarial.probe",
            json!({
                "host_pid": std::process::id(),
                "host_port": 9,
                "host_secret": control.secret_path,
            }),
            CancellationToken::new(),
        )
        .await
        .expect("valid attested plugin must execute before mismatch checks");

    let mut version_harness = Harness::new();
    version_harness.manifest.version = "9.9.9".into();
    version_harness.install();
    let _version_error = version_harness
        .host
        .execute(
            "plugin.adversarial.probe",
            json!({
                "host_pid": std::process::id(),
                "host_port": 9,
                "host_secret": version_harness.secret_path,
            }),
            CancellationToken::new(),
        )
        .await
        .unwrap_err();

    let mut descriptor_harness = Harness::new();
    descriptor_harness.manifest.commands[0].description =
        "tampered owner manifest descriptor".into();
    descriptor_harness.install();
    let _digest_error = descriptor_harness
        .host
        .execute(
            "plugin.adversarial.probe",
            json!({
                "host_pid": std::process::id(),
                "host_port": 9,
                "host_secret": descriptor_harness.secret_path,
            }),
            CancellationToken::new(),
        )
        .await
        .unwrap_err();
}
