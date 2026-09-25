#![cfg(target_os = "linux")]

use semwright_backend_api::{Context, Provider, ProviderSignal};
use semwright_driver_host::DriverProvider;
use semwright_driver_sdk::{
    ApplicationMatch, DriverInterfaces, DriverResources, Manifest, Transport,
};
use semwright_types::{ErrorCode, JobArtifact};
use sha2::{Digest, Sha256};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

fn digest(path: &Path) -> String {
    format!("{:x}", Sha256::digest(std::fs::read(path).unwrap()))
}

fn helper() -> PathBuf {
    PathBuf::from(
        std::env::var_os("SEMWRIGHT_TEST_SANDBOX_HELPER")
            .expect("SEMWRIGHT_TEST_SANDBOX_HELPER must point to semwright-sandbox"),
    )
}

fn manifest(executable: PathBuf) -> Manifest {
    Manifest {
        manifest_version: 1,
        protocol: 2,
        id: "fixture".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        publisher: "semwright-tests".into(),
        executable: executable.clone(),
        sha256: digest(&executable),
        application: ApplicationMatch {
            desktop_id: Some("org.semwright.DriverFixture".into()),
            process_names: vec![],
            supported_versions: vec![],
        },
        transport: Transport::StdioV1,
        mounts: vec![],
        system_config: vec![],
        network: false,
        resources: DriverResources {
            open_files: 128,
            processes: 256,
            cpu_seconds: 20,
            address_space_bytes: 1_073_741_824,
            file_size_bytes: 16_777_216,
        },
        request_timeout_ms: 5_000,
        interfaces: DriverInterfaces {
            dynamic_capabilities: true,
            cooperative_cancellation: true,
            events: true,
            progress: true,
            artifacts: true,
            health: true,
            native_refs: false,
        },
    }
}

#[tokio::test]
#[ignore = "requires real Bubblewrap + Landlock support"]
async fn protocol_v2_streams_events_progress_artifacts_and_cancels_cooperatively() {
    assert!(std::env::var_os("SEMWRIGHT_TEST_DRIVER_V2").is_some());
    let cargo_executable = PathBuf::from(env!("CARGO_BIN_EXE_semwright-driver-fixture"));
    let binary_dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(binary_dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let executable = binary_dir.path().join("driver");
    std::fs::copy(cargo_executable, &executable).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o500)).unwrap();

    let state = tempfile::tempdir().unwrap();
    std::fs::set_permissions(state.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let provider =
        DriverProvider::connect(manifest(executable), state.path(), &helper(), &[], false)
            .await
            .unwrap();
    let interfaces = Provider::interfaces(provider.as_ref());
    assert!(interfaces.dynamic_capabilities);
    assert!(interfaces.cooperative_cancellation);
    assert!(interfaces.events);
    assert!(interfaces.progress);
    assert!(interfaces.artifacts);

    let capabilities = Provider::capabilities(provider.as_ref()).await.unwrap();
    let long = capabilities
        .iter()
        .find(|capability| capability.descriptor.name == "driver.fixture.long")
        .unwrap()
        .descriptor
        .clone();

    let mut signals = Provider::events(provider.as_ref()).expect("v2 signal receiver");
    let result = Provider::execute(
        provider.as_ref(),
        &Context {
            session: "v2-complete".into(),
            request_id: "job-v2-complete".into(),
            cancellation: CancellationToken::new(),
        },
        &long,
        &serde_json::json!({}),
    )
    .await
    .unwrap();
    assert_eq!(result["done"], true);

    let mut saw_event = false;
    let mut saw_catalog = false;
    let mut saw_artifact = false;
    let mut last_progress = 0;
    while let Ok(signal) = signals.try_recv() {
        match signal {
            ProviderSignal::Event { kind, .. } if kind == "fixture.started" => saw_event = true,
            ProviderSignal::CapabilitiesChanged => saw_catalog = true,
            ProviderSignal::Progress {
                request_id,
                progress,
                artifacts,
            } if request_id == "job-v2-complete" => {
                last_progress = last_progress.max(progress.completed);
                if artifacts
                    == vec![JobArtifact {
                        name: "preview".into(),
                        reference: "artifact:fixture-preview".into(),
                        media_type: Some("application/octet-stream".into()),
                        sha256: Some("b".repeat(64)),
                        bytes: Some(16),
                    }]
                {
                    saw_artifact = true;
                }
            }
            _ => {}
        }
    }
    assert!(saw_event && saw_catalog && saw_artifact);
    assert_eq!(last_progress, 4);

    let cancellation = CancellationToken::new();
    let cancel_for_task = cancellation.clone();
    let provider_for_task = provider.clone();
    let long_for_task = long.clone();
    let task = tokio::spawn(async move {
        Provider::execute(
            provider_for_task.as_ref(),
            &Context {
                session: "v2-cancel".into(),
                request_id: "job-v2-cancel".into(),
                cancellation: cancel_for_task,
            },
            &long_for_task,
            &serde_json::json!({}),
        )
        .await
    });
    tokio::time::sleep(Duration::from_millis(120)).await;
    cancellation.cancel();
    let error = tokio::time::timeout(Duration::from_secs(4), task)
        .await
        .expect("cooperative cancellation timed out")
        .unwrap()
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Cancelled);

    Provider::shutdown(provider.as_ref()).await.unwrap();
}
