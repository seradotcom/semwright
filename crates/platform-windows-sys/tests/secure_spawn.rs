#![cfg(target_os = "windows")]

use semwright_platform_api::launch::{ResourceLimits, SandboxKind, SandboxLauncher, SandboxSpec};
use semwright_platform_windows_sys::launch::WindowsSandbox;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn spec(network: bool) -> SandboxSpec {
    SandboxSpec {
        kind: SandboxKind::Driver,
        staged_executable: env!("CARGO_BIN_EXE_windows_sandbox_fixture").into(),
        helper: std::env::current_exe().expect("current test executable"),
        mounts: vec![],
        args: vec![],
        environment: vec![("SEMWRIGHT_FIXTURE".into(), "native".into())],
        sealed_tools: vec![],
        network,
        limits: Some(ResourceLimits {
            open_files: 32,
            processes: 8,
            cpu_seconds: 5,
            address_space_bytes: 256 * 1024 * 1024,
            file_size_bytes: 1024 * 1024,
        }),
    }
}

#[tokio::test]
async fn appcontainer_spawn_roundtrips_only_allowlisted_stdio() {
    let mut process = WindowsSandbox
        .spawn(&spec(false))
        .expect("secure Windows spawn");
    assert!(process.id().is_some());

    let mut stdin = process.take_stdin().expect("sandbox stdin");
    let mut stdout = process.take_stdout().expect("sandbox stdout");
    stdin.write_all(b"ping").await.expect("write fixture stdin");
    stdin.shutdown().await.expect("close fixture stdin");
    drop(stdin);

    let mut output = Vec::new();
    stdout
        .read_to_end(&mut output)
        .await
        .expect("read fixture stdout");
    process.wait().await.expect("wait for sandbox child");
    assert_eq!(output, b"native|path=false|network=false|ping");
}

#[tokio::test]
async fn network_authority_materializes_only_internet_client_capability() {
    let mut process = WindowsSandbox
        .spawn(&spec(true))
        .expect("network-enabled Windows spawn");
    let mut stdin = process.take_stdin().expect("sandbox stdin");
    let mut stdout = process.take_stdout().expect("sandbox stdout");
    stdin.shutdown().await.expect("close fixture stdin");
    drop(stdin);

    let mut output = Vec::new();
    stdout
        .read_to_end(&mut output)
        .await
        .expect("read fixture stdout");
    process.wait().await.expect("wait for sandbox child");
    assert_eq!(output, b"native|path=false|network=true|");
}
