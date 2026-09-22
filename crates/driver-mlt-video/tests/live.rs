use semwright_backend_api::{Context, Provider};
use semwright_driver_host::DriverProvider;
use semwright_driver_sdk::{
    ApplicationMatch, DriverInterfaces, DriverMount, DriverResources, Manifest, Transport,
};
use semwright_policy::FilesystemGrant;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
use tokio_util::sync::CancellationToken;

fn digest(path: &Path) -> String {
    format!("{:x}", Sha256::digest(std::fs::read(path).unwrap()))
}

async fn call(
    provider: &DriverProvider,
    capabilities: &[semwright_backend_api::ProvidedCapability],
    name: &str,
    args: Value,
) -> semwright_types::Result<Value> {
    let descriptor = &capabilities
        .iter()
        .find(|capability| capability.descriptor.name == name)
        .unwrap()
        .descriptor;
    Provider::execute(
        provider,
        &Context {
            session: "mlt-video-live".into(),
            cancellation: CancellationToken::new(),
        },
        descriptor,
        &args,
    )
    .await
}

#[tokio::test]
#[ignore = "requires bubblewrap on a Linux host"]
async fn real_mlt_video_driver_runs_inside_sandbox() {
    if std::env::var_os("SEMWRIGHT_TEST_MLT_VIDEO").is_none() {
        return;
    }
    let cargo_executable = PathBuf::from(env!("CARGO_BIN_EXE_semwright-mlt-video-driver"));
    let binary_dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(binary_dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let executable = binary_dir.path().join("semwright-mlt-video-driver");
    std::fs::copy(&cargo_executable, &executable).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    let helper = PathBuf::from(
        std::env::var_os("SEMWRIGHT_TEST_SANDBOX_HELPER")
            .expect("SEMWRIGHT_TEST_SANDBOX_HELPER must point to semwright-sandbox"),
    );

    let project = tempfile::tempdir().unwrap();
    let media = tempfile::tempdir().unwrap();
    let output = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    std::fs::set_permissions(state.path(), std::fs::Permissions::from_mode(0o700)).unwrap();

    let manifest = Manifest {
        manifest_version: 1,
        protocol: 1,
        id: "mlt-video".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        publisher: "semwright-tests".into(),
        executable: executable.clone(),
        sha256: digest(&executable),
        application: ApplicationMatch {
            desktop_id: Some("org.mltframework.melt".into()),
            process_names: vec!["melt".into()],
            supported_versions: vec![],
        },
        transport: Transport::StdioV1,
        mounts: vec![
            DriverMount {
                root: "project".into(),
                read_only: true,
            },
            DriverMount {
                root: "media".into(),
                read_only: true,
            },
            DriverMount {
                root: "output".into(),
                read_only: false,
            },
        ],
        system_config: vec![],
        network: false,
        resources: DriverResources {
            address_space_bytes: 1_073_741_824,
            cpu_seconds: 120,
            file_size_bytes: 1_073_741_824,
            ..DriverResources::default()
        },
        request_timeout_ms: 30_000,
        interfaces: DriverInterfaces::default(),
    };
    let grants = vec![
        FilesystemGrant {
            name: "project".into(),
            path: project.path().canonicalize().unwrap(),
            read: true,
            write: false,
        },
        FilesystemGrant {
            name: "media".into(),
            path: media.path().canonicalize().unwrap(),
            read: true,
            write: false,
        },
        FilesystemGrant {
            name: "output".into(),
            path: output.path().canonicalize().unwrap(),
            read: true,
            write: true,
        },
    ];
    let provider = DriverProvider::connect(manifest, state.path(), &helper, &grants, false)
        .await
        .unwrap();
    let capabilities = Provider::capabilities(provider.as_ref()).await.unwrap();
    assert_eq!(capabilities.len(), 68);
    assert!(
        capabilities
            .iter()
            .all(|capability| capability.descriptor.name.starts_with("driver.mlt-video."))
    );

    let doctor = call(
        provider.as_ref(),
        &capabilities,
        "driver.mlt-video.doctor",
        json!({}),
    )
    .await
    .unwrap();
    assert_eq!(doctor["capabilities"], 68);
    assert_eq!(doctor["network"], false);
    let created = call(
        provider.as_ref(),
        &capabilities,
        "driver.mlt-video.project.create",
        json!({}),
    )
    .await
    .unwrap();
    let project_ref = created["project"].as_str().unwrap();
    let inspected = call(
        provider.as_ref(),
        &capabilities,
        "driver.mlt-video.project.inspect",
        json!({"project": project_ref}),
    )
    .await
    .unwrap();
    assert_eq!(inspected["deep_editable"], true);
    Provider::shutdown(provider.as_ref()).await.unwrap();
}
