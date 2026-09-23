//! Real official-SDK client -> stdio MCP binary -> Unix IPC -> broker -> fake desktop.
use rmcp::{
    ClientLifecycleMode, ClientServiceExt, ServiceExt,
    model::{CallToolRequestParams, ClientConfig, ProtocolVersion},
};
use semwright_backends::fake::FakeDesktop;
use semwright_core::{Broker, NoApprover, audit::Audit};
use semwright_policy::{Policy, PolicyConfig, Profile};
use serde_json::{Value, json};
use std::{process::Stdio, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

async fn exercise(discover: bool, profile: Profile) {
    let directory = tempfile::tempdir().unwrap();
    let runtime = directory.path().join("runtime");
    semwright_protocol::private_directory(&runtime).unwrap();
    let socket = runtime.join("broker.sock");
    let desktop = Arc::new(FakeDesktop::new());
    let broker = Broker::new(
        Policy::new(PolicyConfig {
            profile,
            ..Default::default()
        })
        .unwrap(),
        vec![desktop.clone()],
        Audit::open(&directory.path().join("audit"), 65536, 2).unwrap(),
        Arc::new(NoApprover),
        None,
        json!({"test":"official-sdk-stdio"}),
        true,
    )
    .unwrap();
    let stop = CancellationToken::new();
    let cancel_on_drop = stop.clone().drop_guard();
    let daemon = tokio::spawn({
        let socket = socket.clone();
        let stop = stop.clone();
        async move { semwright_daemon::server::serve(&socket, broker, stop).await }
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        while !socket.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_semwright-mcp"))
        .arg("--socket")
        .arg(&socket)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let transport = (child.stdout.take().unwrap(), child.stdin.take().unwrap());
    let client = if discover {
        ClientConfig::default()
            .serve_with_lifecycle(
                transport,
                ClientLifecycleMode::Discover {
                    preferred_versions: vec![ProtocolVersion::V_2026_07_28],
                },
            )
            .await
            .unwrap()
    } else {
        ClientConfig::default().serve(transport).await.unwrap()
    };
    let tools = client.list_all_tools().await.unwrap();
    assert!(tools.len() <= 12, "Gateway must remain context efficient");
    assert!(!tools.iter().any(|tool| tool.name.contains("approve")));
    let catalog = client
        .call_tool(
            CallToolRequestParams::new("capabilities_search").with_arguments(
                json!({"query":"ui.find","limit":1})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap()
        .structured_content
        .unwrap();
    assert_eq!(catalog["data"]["capabilities"][0]["id"], "ui.find");
    assert_eq!(catalog["data"]["availability_is_authorization"], false);
    let doctor = client
        .call_tool(CallToolRequestParams::new("semwright_doctor"))
        .await
        .unwrap();
    assert_eq!(doctor.structured_content.unwrap()["data"]["fake"], true);
    let find = client
        .call_tool(
            CallToolRequestParams::new("semwright_find").with_arguments(
                json!({"selector":{"name":{"op":"exact","value":"Export"}}})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap()
        .structured_content
        .unwrap();
    let reference = find["data"]["nodes"][0]["ref"].clone();
    assert!(reference.is_string());
    let invoke_args = json!({"command":"ui.invoke","args":{"ref":reference,"action":"click"}});
    let invoke = client
        .call_tool(
            CallToolRequestParams::new("semwright_execute")
                .with_arguments(invoke_args.as_object().unwrap().clone()),
        )
        .await
        .unwrap()
        .structured_content
        .unwrap();
    if profile == Profile::Observe {
        assert_eq!(invoke["ok"], false);
        assert_eq!(invoke["error"]["code"], "PolicyDenied");
        assert_eq!(desktop.invocations(), 0);
    } else {
        assert_eq!(invoke["ok"], true, "{invoke}");
        assert_eq!(desktop.invocations(), 1);
        let stale = client
            .call_tool(
                CallToolRequestParams::new("semwright_execute")
                    .with_arguments(invoke_args.as_object().unwrap().clone()),
            )
            .await
            .unwrap()
            .structured_content
            .unwrap();
        assert_eq!(stale["error"]["code"], "StaleReference");
    }
    let invalid = json!({"command":"ui.invoke","args":{"ref":reference,"confirmed":true}});
    let rejected = client
        .call_tool(
            CallToolRequestParams::new("semwright_execute")
                .with_arguments(invalid.as_object().unwrap().clone()),
        )
        .await
        .unwrap();
    assert_eq!(rejected.is_error, Some(true));
    let audit = client
        .call_tool(
            CallToolRequestParams::new("semwright_audit_tail")
                .with_arguments(json!({"limit":100}).as_object().unwrap().clone()),
        )
        .await
        .unwrap()
        .structured_content
        .unwrap();
    assert!(
        audit["data"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["phase"] == "finish")
    );
    let _: Value = audit;
    client.cancel().await.unwrap();
    let _ = child.kill().await;
    child.wait().await.unwrap();
    stop.cancel();
    tokio::time::timeout(Duration::from_secs(10), daemon)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    drop(cancel_on_drop);
    assert!(!socket.exists(), "Daemon must clean its own socket");
}

#[tokio::test]
async fn modern_discover_executes_and_invalidates_refs() {
    tokio::time::timeout(Duration::from_secs(30), exercise(true, Profile::Desktop))
        .await
        .unwrap();
}

#[tokio::test]
async fn legacy_initialization_preserves_read_only_policy() {
    tokio::time::timeout(Duration::from_secs(30), exercise(false, Profile::Observe))
        .await
        .unwrap();
}
