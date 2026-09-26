#![cfg(target_os = "windows")]

use semwright_backend_api::{Context, Provider};
use semwright_federation::{ExternalMcpProvider, StdioUpstreamConfig, StdioUpstreamResources};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::Path;
use tokio_util::sync::CancellationToken;

fn digest(path: &Path) -> String {
    format!(
        "{:x}",
        Sha256::digest(std::fs::read(path).expect("read MCP fixture"))
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
async fn secure_windows_external_mcp_roundtrips_echo() {
    let source = std::path::PathBuf::from(env!("CARGO_BIN_EXE_semwright-mcp-fixture"));
    let binary_dir = tempfile::tempdir().expect("fixture directory");
    let copied = binary_dir.path().join("mcp-fixture.exe");
    std::fs::copy(&source, &copied).expect("copy MCP fixture");
    harden_fixture(&copied);
    let program = std::fs::canonicalize(&copied).expect("canonical MCP fixture");

    let config = StdioUpstreamConfig {
        slug: "windows-secure".into(),
        program: program.clone(),
        sha256: digest(&program),
        args: vec![],
        mounts: vec![],
        network: false,
        resources: StdioUpstreamResources::default(),
        expected_name: Some("semwright-fixture-upstream".into()),
        expected_version: Some("1.0.0".into()),
        request_timeout_ms: 5_000,
        enabled: true,
    };

    let state = tempfile::tempdir().expect("MCP state");
    let helper = std::env::current_exe().expect("current test executable");
    let provider =
        ExternalMcpProvider::connect_sandboxed_stdio(config, state.path(), &helper, false)
            .await
            .expect("connect secure Windows MCP");

    let capabilities = Provider::capabilities(provider.as_ref())
        .await
        .expect("MCP capabilities");
    let echo = capabilities
        .iter()
        .find(|capability| capability.aliases.iter().any(|alias| alias == "echo"))
        .expect("echo capability")
        .descriptor
        .clone();

    let output = Provider::execute(
        provider.as_ref(),
        &Context {
            session: "windows-secure-mcp".into(),
            request_id: "windows-secure-mcp-echo".into(),
            cancellation: CancellationToken::new(),
        },
        &echo,
        &json!({"text":"secure-mcp"}),
    )
    .await
    .expect("execute MCP echo through Windows AppContainer");
    assert_eq!(output["echoed"], "secure-mcp");

    Provider::shutdown(provider.as_ref())
        .await
        .expect("shutdown Windows MCP provider");
}
