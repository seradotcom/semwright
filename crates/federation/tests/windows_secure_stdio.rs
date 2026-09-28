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

const EXTERNAL_ENDPOINTS: [(&str, u16); 2] = [("1.1.1.1", 443), ("1.0.0.1", 443)];

fn config(program: std::path::PathBuf, network: bool, slug: &str) -> StdioUpstreamConfig {
    StdioUpstreamConfig {
        slug: slug.into(),
        program: program.clone(),
        sha256: digest(&program),
        args: vec![],
        mounts: vec![],
        network,
        resources: StdioUpstreamResources::default(),
        expected_name: Some("semwright-fixture-upstream".into()),
        expected_version: Some("1.0.0".into()),
        request_timeout_ms: 5_000,
        enabled: true,
    }
}

async fn mcp_probe(
    provider: &ExternalMcpProvider,
    host: &str,
    port: u16,
    host_path: &Path,
    request_id: &str,
) -> serde_json::Value {
    let capabilities = Provider::capabilities(provider)
        .await
        .expect("MCP capabilities");
    let probe = capabilities
        .iter()
        .find(|capability| {
            capability
                .aliases
                .iter()
                .any(|alias| alias == "sandbox_probe")
        })
        .expect("sandbox_probe capability")
        .descriptor
        .clone();
    let context = Context {
        session: "windows-network-authority".into(),
        request_id: request_id.into(),
        cancellation: CancellationToken::new(),
    };
    let args = json!({
        "host": host,
        "port": port,
        "host_path": host_path.to_string_lossy(),
    });
    Provider::execute(provider, &context, &probe, &args)
        .await
        .expect("execute MCP sandbox network probe")
}

async fn mcp_can_reach_external(
    provider: &ExternalMcpProvider,
    host_path: &Path,
    prefix: &str,
) -> bool {
    for (index, (host, port)) in EXTERNAL_ENDPOINTS.iter().enumerate() {
        let output = mcp_probe(
            provider,
            host,
            *port,
            host_path,
            &format!("{prefix}-{index}"),
        )
        .await;
        assert_eq!(output["host_visible"], false);
        if output["network_reachable"].as_bool().unwrap_or(false) {
            return true;
        }
    }
    false
}

#[tokio::test]
async fn secure_windows_external_mcp_roundtrips_echo() {
    let source = std::path::PathBuf::from(env!("CARGO_BIN_EXE_semwright-mcp-fixture"));
    let binary_dir = tempfile::tempdir().expect("fixture directory");
    let copied = binary_dir.path().join("mcp-fixture.exe");
    std::fs::copy(&source, &copied).expect("copy MCP fixture");
    harden_fixture(&copied);
    assert!(
        copied.is_absolute(),
        "temp MCP fixture path must be absolute"
    );

    let config = config(copied.clone(), false, "windows-secure");

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

#[tokio::test]
async fn secure_windows_external_mcp_network_is_owner_gated_internet_only_and_restart_stable() {
    let source = std::path::PathBuf::from(env!("CARGO_BIN_EXE_semwright-mcp-fixture"));
    let binary_dir = tempfile::tempdir().expect("fixture directory");
    let copied = binary_dir.path().join("mcp-fixture.exe");
    std::fs::copy(&source, &copied).expect("copy MCP fixture");
    harden_fixture(&copied);

    let host_dir = tempfile::tempdir().expect("host-only directory");
    let host_path = host_dir.path().join("host-only.txt");
    std::fs::write(&host_path, b"must-remain-host-only").expect("write host-only fixture");
    harden_fixture(&host_path);
    let helper = std::env::current_exe().expect("current test executable");

    let denied_state = tempfile::tempdir().expect("denied MCP state");
    let error = match ExternalMcpProvider::connect_sandboxed_stdio(
        config(copied.clone(), true, "windows-network-denied"),
        denied_state.path(),
        &helper,
        false,
    )
    .await
    {
        Ok(_) => panic!("owner-disabled external MCP network must be denied before spawn"),
        Err(error) => error,
    };
    assert_eq!(error.code, semwright_types::ErrorCode::PolicyDenied);

    let offline_state = tempfile::tempdir().expect("offline MCP state");
    let offline = ExternalMcpProvider::connect_sandboxed_stdio(
        config(copied.clone(), false, "windows-network-offline"),
        offline_state.path(),
        &helper,
        true,
    )
    .await
    .expect("connect network-denied Windows MCP");
    for (index, (host, port)) in EXTERNAL_ENDPOINTS.iter().enumerate() {
        let output = mcp_probe(
            offline.as_ref(),
            host,
            *port,
            &host_path,
            &format!("offline-{index}"),
        )
        .await;
        assert_eq!(
            output["network_reachable"], false,
            "network=false must deny outbound Internet"
        );
        assert_eq!(output["host_visible"], false);
    }
    let denied_listener =
        std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind denied host loopback");
    let denied_loopback_port = denied_listener
        .local_addr()
        .expect("denied loopback address")
        .port();
    let output = mcp_probe(
        offline.as_ref(),
        "127.0.0.1",
        denied_loopback_port,
        &host_path,
        "offline-loopback",
    )
    .await;
    assert_eq!(output["network_reachable"], false);
    assert_eq!(output["host_visible"], false);
    Provider::shutdown(offline.as_ref())
        .await
        .expect("shutdown offline Windows MCP");

    let allowed_state = tempfile::tempdir().expect("allowed MCP state");
    let allowed = ExternalMcpProvider::connect_sandboxed_stdio(
        config(copied.clone(), true, "windows-network-allowed"),
        allowed_state.path(),
        &helper,
        true,
    )
    .await
    .expect("connect owner-authorized Windows MCP network");
    assert!(
        mcp_can_reach_external(allowed.as_ref(), &host_path, "allowed").await,
        "internetClient must permit at least one stable outbound Internet endpoint"
    );

    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind host loopback");
    let loopback_port = listener.local_addr().expect("loopback address").port();
    let output = mcp_probe(
        allowed.as_ref(),
        "127.0.0.1",
        loopback_port,
        &host_path,
        "loopback",
    )
    .await;
    assert_eq!(
        output["network_reachable"], false,
        "internetClient must not grant direct host loopback"
    );
    assert_eq!(output["host_visible"], false);
    Provider::shutdown(allowed.as_ref())
        .await
        .expect("shutdown network-enabled Windows MCP");

    let restarted = ExternalMcpProvider::connect_sandboxed_stdio(
        config(copied, true, "windows-network-restart"),
        allowed_state.path(),
        &helper,
        true,
    )
    .await
    .expect("restart owner-authorized Windows MCP network");
    assert!(
        mcp_can_reach_external(restarted.as_ref(), &host_path, "restart").await,
        "external MCP network authority must remain deterministic after restart"
    );
    Provider::shutdown(restarted.as_ref())
        .await
        .expect("shutdown restarted Windows MCP");
}
