#![cfg(unix)]

use semwright_backend_api::{Context, Provider};
use semwright_driver_host::DriverProvider;
use semwright_driver_sdk::{
    ApplicationMatch, DRIVER_MANIFEST_VERSION, DRIVER_PROTOCOL_VERSION, DriverInterfaces,
    DriverResources, Manifest, Transport,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::process::{Child, Command};
use tokio_util::sync::CancellationToken;

fn digest(path: &Path) -> String {
    format!("{:x}", Sha256::digest(std::fs::read(path).unwrap()))
}

fn find<'a>(
    capabilities: &'a [semwright_backend_api::ProvidedCapability],
    name: &str,
) -> &'a semwright_backend_api::ProvidedCapability {
    capabilities
        .iter()
        .find(|capability| capability.descriptor.name == name)
        .unwrap()
}
async fn call(
    provider: &DriverProvider,
    capabilities: &[semwright_backend_api::ProvidedCapability],
    name: &str,
    args: Value,
) -> semwright_types::Result<Value> {
    Provider::execute(
        provider,
        &Context {
            session: "figma-host-conformance".into(),
            request_id: semwright_types::unique_id(),
            cancellation: CancellationToken::new(),
        },
        &find(capabilities, name).descriptor,
        &args,
    )
    .await
}

struct FakeFigma {
    child: Child,
}

impl FakeFigma {
    async fn start(url: String, secret: String) -> Self {
        let executable = PathBuf::from(env!("CARGO_BIN_EXE_semwright-fake-figma"));
        let child = Command::new(executable)
            .arg(url)
            .env("SEMWRIGHT_FIGMA_PAIRING_SECRET", secret)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        Self { child }
    }

    async fn stop(mut self) {
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
    }
}

fn manifest(executable: PathBuf) -> Manifest {
    Manifest {
        manifest_version: DRIVER_MANIFEST_VERSION,
        protocol: DRIVER_PROTOCOL_VERSION,
        id: "figma".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        publisher: "semwright-tests".into(),
        sha256: digest(&executable),
        executable,
        application: ApplicationMatch {
            desktop_id: None,
            process_names: vec!["figma".into(), "Figma".into()],
            supported_versions: vec![],
        },
        transport: Transport::StdioV1,
        mounts: vec![],
        system_config: vec![],
        secrets: vec![],
        tools: vec![],
        network: true,
        loopback_port: None,
        resources: DriverResources {
            cpu_seconds: 60,
            operation_cpu_seconds: 0,
            ..DriverResources::default()
        },
        request_timeout_ms: 5_000,
        interfaces: DriverInterfaces {
            events: true,
            progress: true,
            artifacts: true,
            health: true,
            ..DriverInterfaces::default()
        },
    }
}

async fn staged_driver() -> (tempfile::TempDir, PathBuf) {
    let cargo_executable = PathBuf::from(env!("CARGO_BIN_EXE_semwright-figma-driver"));
    let binary_dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(binary_dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let executable = binary_dir.path().join("semwright-figma-driver");
    std::fs::copy(&cargo_executable, &executable).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    (binary_dir, executable)
}

async fn wait_for_session(
    provider: &DriverProvider,
    capabilities: &[semwright_backend_api::ProvidedCapability],
) -> Vec<Value> {
    for _ in 0..50 {
        let value = call(
            provider,
            capabilities,
            "driver.figma.session.list",
            json!({}),
        )
        .await
        .unwrap();
        let sessions = value.as_array().cloned().unwrap_or_default();
        if !sessions.is_empty() {
            return sessions;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("fake Figma did not establish an authenticated session");
}

#[tokio::test]
#[ignore = "requires bubblewrap, Landlock, semwright-sandbox and test-tools"]
async fn figma_driver_runs_through_real_driver_host() {
    if std::env::var_os("SEMWRIGHT_TEST_FIGMA").is_none() {
        return;
    }

    let (_binary_dir, executable) = staged_driver().await;
    let helper = PathBuf::from(
        std::env::var_os("SEMWRIGHT_TEST_SANDBOX_HELPER")
            .expect("SEMWRIGHT_TEST_SANDBOX_HELPER must point to semwright-sandbox"),
    );
    let state = tempfile::tempdir().unwrap();
    std::fs::set_permissions(state.path(), std::fs::Permissions::from_mode(0o700)).unwrap();

    let provider = DriverProvider::connect(manifest(executable), state.path(), &helper, &[], true)
        .await
        .unwrap();
    let capabilities = Provider::capabilities(provider.as_ref()).await.unwrap();
    assert!(!capabilities.is_empty());
    assert!(
        capabilities
            .iter()
            .all(|capability| capability.descriptor.name.starts_with("driver.figma."))
    );
    let names = capabilities
        .iter()
        .map(|capability| capability.descriptor.name.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        names.len(),
        capabilities.len(),
        "duplicate Figma capabilities"
    );
    for required in [
        "driver.figma.pairing.begin",
        "driver.figma.document.status",
        "driver.figma.compose.apply",
        "driver.figma.composition.plan",
        "driver.figma.composition.apply",
        "driver.figma.composition.validate",
        "driver.figma.composition.repair.plan",
        "driver.figma.composition.repair.apply",
        "driver.figma.composition.verify",
        "driver.figma.export.node",
        "driver.figma.payments.status",
        "driver.figma.cloud.status",
    ] {
        assert!(
            names.contains(required),
            "missing required semantic surface: {required}"
        );
    }

    let pairing = call(
        provider.as_ref(),
        &capabilities,
        "driver.figma.pairing.begin",
        json!({}),
    )
    .await
    .unwrap();
    assert_eq!(pairing["listen_host"], "127.0.0.1");
    let port = pairing["listen_port"].as_u64().unwrap() as u16;
    let secret = pairing["pairing_code"].as_str().unwrap().to_owned();

    let fake = FakeFigma::start(format!("ws://127.0.0.1:{port}"), secret).await;
    let sessions = wait_for_session(provider.as_ref(), &capabilities).await;
    let session_id = sessions[0]["session_id"].as_str().unwrap().to_owned();

    let status = call(
        provider.as_ref(),
        &capabilities,
        "driver.figma.document.status",
        json!({"session_id": session_id}),
    )
    .await
    .unwrap();
    assert_eq!(status["documentId"], "fake-doc");

    let frame = call(
        provider.as_ref(),
        &capabilities,
        "driver.figma.frame.create",
        json!({
            "session_id": sessions[0]["session_id"],
            "expected_revision": 0,
            "name": "Host conformance frame"
        }),
    )
    .await
    .unwrap();
    let node_id = frame["id"].as_str().unwrap().to_owned();

    let layout = call(
        provider.as_ref(),
        &capabilities,
        "driver.figma.layout.patch",
        json!({
            "session_id": sessions[0]["session_id"],
            "expected_revision": 1,
            "nodeId": node_id,
            "layoutMode": "VERTICAL",
            "itemSpacing": 16
        }),
    )
    .await
    .unwrap();
    assert_eq!(layout["layoutMode"], "VERTICAL");

    let motion = call(
        provider.as_ref(),
        &capabilities,
        "driver.figma.motion.styles.list",
        json!({"session_id": sessions[0]["session_id"]}),
    )
    .await
    .unwrap();
    assert!(!motion.as_array().unwrap().is_empty());

    let semantic_spec = json!({
        "version": 1,
        "target": {"page_id": null, "parent_node_id": null},
        "nodes": [
            {
                "id":"hero",
                "kind":"stack",
                "name":"Semantic Hero",
                "parent":null,
                "order":0,
                "role":"hero",
                "layout":{"direction":"vertical","gap":8}
            },
            {
                "id":"heading",
                "kind":"text",
                "name":"Heading",
                "parent":"hero",
                "order":0,
                "role":"heading",
                "text":{"characters":"Meaning before pixels."}
            },
            {
                "id":"body",
                "kind":"text",
                "name":"Body",
                "parent":"hero",
                "order":1,
                "role":"body",
                "text":{"characters":"Observed state decides whether the contract passed."}
            }
        ],
        "relationships": [{
            "kind":"minimum_gap",
            "subject":"heading",
            "object":"body",
            "value":24,
            "tolerance":0.5
        }]
    });
    let semantic_plan = call(
        provider.as_ref(),
        &capabilities,
        "driver.figma.composition.plan",
        json!({
            "session_id":sessions[0]["session_id"],
            "expected_revision":2,
            "spec":semantic_spec
        }),
    )
    .await
    .unwrap();
    assert_eq!(semantic_plan["purpose"], "composition");
    assert_eq!(semantic_plan["base"]["revision"], 2);
    assert_eq!(semantic_plan["digest"].as_str().unwrap().len(), 64);

    let semantic_apply = call(
        provider.as_ref(),
        &capabilities,
        "driver.figma.composition.apply",
        json!({
            "session_id":sessions[0]["session_id"],
            "expected_revision":2,
            "plan":semantic_plan
        }),
    )
    .await
    .unwrap();
    assert_eq!(semantic_apply["observedRevision"], 3);
    let semantic_root = semantic_apply["rootNodeIds"][0]
        .as_str()
        .unwrap()
        .to_owned();

    let semantic_validation = call(
        provider.as_ref(),
        &capabilities,
        "driver.figma.composition.validate",
        json!({
            "session_id":sessions[0]["session_id"],
            "expected_revision":3,
            "root_node_id":semantic_root,
            "spec":semantic_plan["spec"],
            "max_findings":32
        }),
    )
    .await
    .unwrap();
    assert_eq!(semantic_validation["status"], "FAIL");
    assert_eq!(semantic_validation["observedRevision"], 3);
    assert_eq!(
        semantic_validation["findings"][0]["category"],
        "declared_spacing"
    );

    let repair_plan = call(
        provider.as_ref(),
        &capabilities,
        "driver.figma.composition.repair.plan",
        json!({
            "session_id":sessions[0]["session_id"],
            "expected_revision":3,
            "plan":semantic_plan,
            "findings":semantic_validation["findings"]
        }),
    )
    .await
    .unwrap();
    assert_eq!(repair_plan["purpose"], "repair");
    assert_eq!(repair_plan["base"]["revision"], 3);
    assert_eq!(
        repair_plan["changeset"]["modifies"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let repaired = call(
        provider.as_ref(),
        &capabilities,
        "driver.figma.composition.repair.apply",
        json!({
            "session_id":sessions[0]["session_id"],
            "expected_revision":3,
            "plan":repair_plan
        }),
    )
    .await
    .unwrap();
    assert_eq!(repaired["observedRevision"], 4);

    let revalidated = call(
        provider.as_ref(),
        &capabilities,
        "driver.figma.composition.validate",
        json!({
            "session_id":sessions[0]["session_id"],
            "expected_revision":4,
            "root_node_id":semantic_root,
            "spec":semantic_plan["spec"],
            "max_findings":32
        }),
    )
    .await
    .unwrap();
    assert_eq!(revalidated["status"], "PASS");

    let verified = call(
        provider.as_ref(),
        &capabilities,
        "driver.figma.composition.verify",
        json!({
            "session_id":sessions[0]["session_id"],
            "expected_revision":4,
            "root_node_id":semantic_root,
            "spec":semantic_plan["spec"],
            "scale":1.0,
            "name":"host-semantic.png",
            "max_findings":32
        }),
    )
    .await
    .unwrap();
    assert_eq!(verified["mediaType"], "image/png");
    assert_eq!(verified["validation"]["status"], "PASS");

    let stale = call(
        provider.as_ref(),
        &capabilities,
        "driver.figma.composition.apply",
        json!({
            "session_id":sessions[0]["session_id"],
            "expected_revision":4,
            "plan":semantic_plan
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(stale.code, semwright_types::ErrorCode::StaleReference);

    Provider::shutdown(provider.as_ref()).await.unwrap();
    fake.stop().await;
}

#[tokio::test]
#[ignore = "requires semwright-sandbox; verifies network authority remains owner-controlled"]
async fn figma_driver_network_requires_owner_opt_in() {
    if std::env::var_os("SEMWRIGHT_TEST_FIGMA").is_none() {
        return;
    }
    let (_binary_dir, executable) = staged_driver().await;
    let helper = PathBuf::from(
        std::env::var_os("SEMWRIGHT_TEST_SANDBOX_HELPER")
            .expect("SEMWRIGHT_TEST_SANDBOX_HELPER must point to semwright-sandbox"),
    );
    let state = tempfile::tempdir().unwrap();
    std::fs::set_permissions(state.path(), std::fs::Permissions::from_mode(0o700)).unwrap();

    let error = match DriverProvider::connect(
        manifest(executable),
        state.path(),
        &helper,
        &[],
        false,
    )
    .await
    {
        Ok(_) => panic!("networked Figma driver unexpectedly started without owner opt-in"),
        Err(error) => error,
    };
    assert_eq!(error.code, semwright_types::ErrorCode::PolicyDenied);
}
