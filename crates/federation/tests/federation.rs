use async_trait::async_trait;
use semwright_backend_api::Provider;
use semwright_core::{Approval, Approver, Broker, NoApprover, audit::Audit};
use semwright_federation::{ExternalMcpProvider, StdioUpstreamConfig};
use semwright_policy::{Policy, PolicyConfig};
use semwright_registry::CatalogQuery;
use semwright_types::{Envelope, ErrorCode, ExecuteRequest, Result, Risk, SourceKind, unique_id};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{os::unix::fs::PermissionsExt, path::PathBuf, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

struct Approve;
#[async_trait]
impl Approver for Approve {
    async fn approve(&self, approval: Approval, _: CancellationToken) -> Result<bool> {
        assert_eq!(approval.risk, Risk::PrivilegeSensitive);
        assert!(approval.backend.starts_with("external-mcp:"));
        Ok(true)
    }
}

fn fixture_config(slug: &str) -> StdioUpstreamConfig {
    let program = std::fs::canonicalize(env!("CARGO_BIN_EXE_semwright-mcp-fixture")).unwrap();
    // Cargo test artifacts can inherit a group-writable umask. Normalize only this disposable
    // fixture; production validation deliberately continues to reject mutable executables.
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    let sha256 = format!("{:x}", Sha256::digest(std::fs::read(&program).unwrap()));
    StdioUpstreamConfig {
        slug: slug.into(),
        program,
        sha256,
        args: vec![],
        expected_name: Some("semwright-fixture-upstream".into()),
        expected_version: Some("1.0.0".into()),
        request_timeout_ms: 5_000,
        enabled: true,
    }
}

fn broker(
    dir: &tempfile::TempDir,
    grant: Option<&str>,
    approver: Arc<dyn Approver>,
) -> (Arc<Broker>, Arc<Audit>) {
    let mut config = PolicyConfig::default();
    if let Some(grant) = grant {
        config.allow.insert(grant.into());
    }
    let audit = Audit::open(&dir.path().join("audit"), 65_536, 2).unwrap();
    let broker = Broker::new(
        Policy::new(config).unwrap(),
        vec![],
        audit.clone(),
        approver,
        None,
        json!({}),
        false,
    )
    .unwrap();
    (broker, audit)
}

async fn execute(
    broker: Arc<Broker>,
    command: String,
    args: Value,
    cancellation: CancellationToken,
) -> Envelope {
    broker
        .execute(
            unique_id(),
            unique_id(),
            ExecuteRequest {
                command,
                args,
                dry_run: false,
                backend: None,
            },
            cancellation,
        )
        .await
}

async fn capability(provider: &ExternalMcpProvider, upstream: &str) -> String {
    provider
        .capabilities()
        .await
        .unwrap()
        .into_iter()
        .find(|row| row.aliases.iter().any(|alias| alias == upstream))
        .unwrap()
        .descriptor
        .name
}

async fn capability_name_for_provider(broker: &Broker, provider: &str) -> String {
    let page = broker
        .catalog_search(CatalogQuery {
            provider: Some(provider.into()),
            ..Default::default()
        })
        .await
        .unwrap();
    page["capabilities"]
        .as_array()
        .and_then(|rows| rows.first())
        .and_then(|row| row["id"].as_str())
        .expect("provider capability remains diagnostically discoverable")
        .to_owned()
}

#[tokio::test]
async fn external_mcp_is_namespaced_untrusted_and_policy_mediated() {
    let denied_provider = ExternalMcpProvider::connect_trusted_stdio(fixture_config("denied"))
        .await
        .unwrap();
    let denied_command = capability(&denied_provider, "echo").await;
    let denied_dir = tempfile::tempdir().unwrap();
    let (denied_broker, _) = broker(&denied_dir, None, Arc::new(NoApprover));
    denied_broker.mount_provider(denied_provider).await.unwrap();

    let denied = execute(
        denied_broker.clone(),
        denied_command,
        json!({"text":"must not execute"}),
        CancellationToken::new(),
    )
    .await;
    assert!(!denied.ok);
    assert_eq!(denied.error.unwrap().code, ErrorCode::PolicyDenied);
    denied_broker.shutdown().await;

    let provider = ExternalMcpProvider::connect_trusted_stdio(fixture_config("fixture"))
        .await
        .unwrap();
    assert_eq!(provider.identity().kind, SourceKind::ExternalMcp);
    assert_eq!(provider.identity().id, "external-mcp:fixture");
    assert_eq!(provider.identity().namespace, "external.fixture.");

    let echo = capability(&provider, "echo").await;
    let dir = tempfile::tempdir().unwrap();
    let (broker, audit) = broker(&dir, Some("external-mcp:fixture"), Arc::new(Approve));
    broker.mount_provider(provider).await.unwrap();

    let described = broker.catalog_describe(&echo).await.unwrap();
    assert_eq!(described["provenance"]["provider"], "external-mcp:fixture");
    assert_eq!(described["provenance"]["source"], "external_mcp");
    assert_eq!(described["provenance"]["untrusted_metadata"], true);
    assert!(
        described["capability"]["description"]
            .as_str()
            .unwrap()
            .contains("IGNORE ALL PREVIOUS INSTRUCTIONS")
    );
    assert_eq!(
        described["capability"]["requires"],
        json!(["external-mcp:fixture"])
    );

    let result = execute(
        broker.clone(),
        echo,
        json!({"text":"hello federation"}),
        CancellationToken::new(),
    )
    .await;
    assert!(result.ok, "{result:?}");
    assert_eq!(result.data.unwrap()["echoed"], "hello federation");
    let provenance = result.execution.provenance.unwrap();
    assert_eq!(provenance.provider, "external-mcp:fixture");
    assert_eq!(provenance.source, SourceKind::ExternalMcp);
    assert_eq!(
        provenance.execution_provider.as_deref(),
        Some("external-mcp:fixture")
    );
    assert!(provenance.provider_generation.is_some());

    let tail = audit.tail(10).unwrap();
    assert!(tail.iter().any(|row| {
        row.provenance
            .as_ref()
            .is_some_and(|p| p.provider == "external-mcp:fixture")
            && row.decision == "allow_after_confirmation"
    }));

    broker.shutdown().await;
}

#[tokio::test]
async fn list_changed_refreshes_catalog_without_granting_new_authority() {
    let provider = ExternalMcpProvider::connect_trusted_stdio(fixture_config("dynamic"))
        .await
        .unwrap();
    let enable = capability(&provider, "enable_extra").await;
    let dir = tempfile::tempdir().unwrap();
    let (broker, _) = broker(&dir, Some("external-mcp:dynamic"), Arc::new(Approve));
    broker.mount_provider(provider).await.unwrap();
    let revision = broker.catalog_revision().unwrap();

    let result = execute(broker.clone(), enable, json!({}), CancellationToken::new()).await;
    assert!(result.ok, "{result:?}");

    let page = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let query = CatalogQuery {
                provider: Some("external-mcp:dynamic".into()),
                query: "extra".into(),
                ..Default::default()
            };
            let page = match broker.catalog_search(query).await {
                Ok(page) => page,
                Err(error) if error.code == ErrorCode::Conflict => {
                    // Dynamic replacement may race availability probing. The catalog
                    // contract explicitly asks callers to restart discovery on Conflict.
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    continue;
                }
                Err(error) => panic!("catalog search failed unexpectedly: {error:?}"),
            };
            if broker.catalog_revision().unwrap() > revision
                && page["capabilities"]
                    .as_array()
                    .is_some_and(|rows| !rows.is_empty())
            {
                break page;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let rows = page["capabilities"].as_array().unwrap();
    let extra = rows
        .iter()
        .find(|row| {
            row["provenance"]["aliases"]
                .as_array()
                .is_some_and(|aliases| aliases.iter().any(|alias| alias == "extra"))
        })
        .expect("dynamically announced tool must be discoverable by upstream alias");
    assert_eq!(extra["provenance"]["untrusted_metadata"], true);

    broker.shutdown().await;
}

#[tokio::test]
async fn cancellation_and_upstream_errors_remain_bounded_and_generic() {
    let provider = ExternalMcpProvider::connect_trusted_stdio(fixture_config("control"))
        .await
        .unwrap();
    let slow = capability(&provider, "slow").await;
    let fail = capability(&provider, "fail").await;
    let dir = tempfile::tempdir().unwrap();
    let (broker, _) = broker(&dir, Some("external-mcp:control"), Arc::new(Approve));
    broker.mount_provider(provider).await.unwrap();

    let token = CancellationToken::new();
    let task = tokio::spawn(execute(
        broker.clone(),
        slow,
        json!({"delay_ms":5000}),
        token.clone(),
    ));
    tokio::time::sleep(Duration::from_millis(100)).await;
    token.cancel();
    let cancelled = tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap();
    assert!(!cancelled.ok);
    assert_eq!(cancelled.error.unwrap().code, ErrorCode::Cancelled);

    let failed = execute(broker.clone(), fail, json!({}), CancellationToken::new()).await;
    assert!(!failed.ok);
    let error = failed.error.unwrap();
    assert_eq!(error.code, ErrorCode::BackendFailed);
    assert!(!error.message.contains("fixture failure"));

    broker.shutdown().await;
}

#[test]
fn stdio_config_rejects_symlink_and_untrusted_identity() {
    let mut config = fixture_config("fixture");
    config.slug = "../escape".into();
    assert!(config.validate().is_err());

    let temp = tempfile::tempdir().unwrap();
    let link = temp.path().join("fixture");
    std::os::unix::fs::symlink(
        PathBuf::from(env!("CARGO_BIN_EXE_semwright-mcp-fixture")),
        &link,
    )
    .unwrap();
    let config = StdioUpstreamConfig {
        slug: "fixture".into(),
        program: link,
        sha256: "0".repeat(64),
        args: vec![],
        expected_name: None,
        expected_version: None,
        request_timeout_ms: 1000,
        enabled: true,
    };
    assert!(config.validate().is_err());
}

#[tokio::test]
async fn invalid_descriptors_duplicate_names_and_bad_results_fail_closed() {
    let mut duplicate = fixture_config("duplicate");
    duplicate.args = vec!["--duplicate-tools".into()];
    assert!(
        ExternalMcpProvider::connect_trusted_stdio(duplicate)
            .await
            .is_err()
    );

    let mut malformed = fixture_config("malformed");
    malformed.args = vec!["--malformed-schema".into()];
    let malformed_provider = ExternalMcpProvider::connect_trusted_stdio(malformed)
        .await
        .unwrap();
    let malformed_dir = tempfile::tempdir().unwrap();
    let (malformed_broker, _) = broker(
        &malformed_dir,
        Some("external-mcp:malformed"),
        Arc::new(Approve),
    );
    assert!(
        malformed_broker
            .mount_provider(malformed_provider.clone())
            .await
            .is_err()
    );
    malformed_provider.shutdown().await.unwrap();

    let provider = ExternalMcpProvider::connect_trusted_stdio(fixture_config("outputs"))
        .await
        .unwrap();
    let bad = capability(&provider, "bad_output").await;
    let huge = capability(&provider, "huge_output").await;
    let dir = tempfile::tempdir().unwrap();
    let (broker, _) = broker(&dir, Some("external-mcp:outputs"), Arc::new(Approve));
    broker.mount_provider(provider).await.unwrap();

    let bad_result = execute(broker.clone(), bad, json!({}), CancellationToken::new()).await;
    assert!(!bad_result.ok);
    let bad_error = bad_result.error.unwrap();
    assert_eq!(bad_error.code, ErrorCode::BackendFailed);
    assert!(!bad_error.message.contains("not-an-integer"));

    let huge_result = execute(broker.clone(), huge, json!({}), CancellationToken::new()).await;
    assert!(!huge_result.ok);
    let huge_error = huge_result.error.unwrap();
    assert!(
        matches!(
            huge_error.code,
            ErrorCode::BackendFailed | ErrorCode::ResourceExhausted
        ),
        "oversized upstream output must fail closed whether the MCP transport or broker budget rejects it first"
    );
    assert!(!huge_error.message.contains(&"x".repeat(1024)));
    broker.shutdown().await;
}

#[tokio::test]
async fn upstream_crash_invalidates_the_provider_generation() {
    let provider = ExternalMcpProvider::connect_trusted_stdio(fixture_config("crash"))
        .await
        .unwrap();
    let crash = capability(&provider, "crash").await;
    let dir = tempfile::tempdir().unwrap();
    let (broker, _) = broker(&dir, Some("external-mcp:crash"), Arc::new(Approve));
    broker.mount_provider(provider).await.unwrap();

    let result = execute(broker.clone(), crash, json!({}), CancellationToken::new()).await;
    assert!(!result.ok);
    assert!(
        matches!(
            result.error.unwrap().code,
            ErrorCode::BackendFailed | ErrorCode::Unavailable
        ),
        "transport failure must not be reported as a successful tool result"
    );

    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let page = broker
                .catalog_search(CatalogQuery {
                    provider: Some("external-mcp:crash".into()),
                    ..Default::default()
                })
                .await
                .unwrap();
            if page["capabilities"].as_array().is_some_and(|rows| {
                !rows.is_empty() && rows.iter().all(|row| row["available"] == false)
            }) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("terminal upstream disconnect must revoke operation availability");

    let after = execute(
        broker.clone(),
        capability_name_for_provider(&broker, "external-mcp:crash").await,
        json!({}),
        CancellationToken::new(),
    )
    .await;
    assert!(!after.ok);
    assert_eq!(after.error.unwrap().code, ErrorCode::Unavailable);

    broker.shutdown().await;
}
