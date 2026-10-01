#![cfg(target_os = "macos")]

use semwright_platform_api::launch::{
    Mount, MountClass, ResourceLimits, SandboxKind, SandboxLauncher, SandboxSpec,
};
use semwright_platform_macos_sys::launch::MacSandbox;
use std::{os::unix::fs::PermissionsExt, path::PathBuf};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn harden(path: &std::path::Path) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o500))
        .expect("harden executable");
}

#[tokio::test]
#[ignore = "requires macOS App Sandbox and built semwright-sandbox helper"]
async fn app_sandbox_spawn_preserves_mount_boundaries_for_pinned_payload() {
    let helper = PathBuf::from(
        std::env::var("SEMWRIGHT_TEST_SANDBOX_HELPER").expect("SEMWRIGHT_TEST_SANDBOX_HELPER"),
    );
    let fixture = PathBuf::from(env!("CARGO_BIN_EXE_semwright-macos-sandbox-fixture"));
    harden(&fixture);

    let root = tempfile::tempdir().expect("sandbox fixture root");
    let ro = root.path().join("ro");
    let rw = root.path().join("rw");
    let denied = root.path().join("denied");
    std::fs::create_dir(&ro).unwrap();
    std::fs::create_dir(&rw).unwrap();
    std::fs::create_dir(&denied).unwrap();
    std::fs::write(ro.join("input.txt"), b"allowed-ro").unwrap();
    std::fs::write(denied.join("secret.txt"), b"denied").unwrap();

    let spec = SandboxSpec {
        kind: SandboxKind::Driver,
        staged_executable: fixture,
        helper,
        mounts: vec![
            Mount {
                source: std::fs::canonicalize(&ro).unwrap(),
                class: MountClass::Workspace,
                logical_name: "readonly".into(),
                read_only: true,
                execute: false,
            },
            Mount {
                source: std::fs::canonicalize(&rw).unwrap(),
                class: MountClass::Workspace,
                logical_name: "writable".into(),
                read_only: false,
                execute: false,
            },
        ],
        args: vec![
            ro.to_string_lossy().into_owned(),
            rw.to_string_lossy().into_owned(),
            denied.to_string_lossy().into_owned(),
        ],
        environment: vec![],
        sealed_tools: vec![],
        network: false,
        limits: Some(ResourceLimits {
            open_files: 64,
            processes: 8,
            cpu_seconds: 10,
            address_space_bytes: 512 * 1024 * 1024,
            file_size_bytes: 16 * 1024 * 1024,
        }),
    };

    let mut child = MacSandbox.spawn(&spec).expect("spawn App Sandbox fixture");
    let mut stdin = child.take_stdin().unwrap();
    stdin.shutdown().await.unwrap();
    drop(stdin);
    let mut stdout = child.take_stdout().unwrap();
    let mut output = String::new();
    stdout.read_to_string(&mut output).await.unwrap();
    let exit = child.wait_exit_code().await.unwrap();

    assert_eq!(exit, Some(0), "fixture output: {output}");
    assert!(output.contains("read_ok=true"), "{output}");
    assert!(output.contains("ro_write_denied=true"), "{output}");
    assert!(output.contains("write_ok=true"), "{output}");
    assert!(output.contains("denied_ok=true"), "{output}");
    assert!(output.contains("sandbox=macos-app-sandbox-v1"), "{output}");
    assert_eq!(std::fs::read(rw.join("output.txt")).unwrap(), b"written");
    assert!(!ro.join("blocked.txt").exists());
}
