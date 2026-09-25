use semwright_backend_api::{Context, Provider};
use semwright_driver_host::DriverProvider;
use semwright_driver_sdk::{
    ApplicationMatch, DriverInterfaces, DriverMount, DriverResources, Manifest, SystemConfigMount,
    Transport,
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
    let bytes = std::fs::read(path).unwrap();
    format!("{:x}", Sha256::digest(bytes))
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
            session: "libreoffice-live".into(),
            request_id: semwright_types::unique_id(),
            cancellation: CancellationToken::new(),
        },
        &find(capabilities, name).descriptor,
        &args,
    )
    .await
}

#[tokio::test]
#[ignore = "requires LibreOffice, pyuno and bubblewrap on a Linux host"]
async fn real_libreoffice_driver_runs_inside_sandbox() {
    if std::env::var_os("SEMWRIGHT_TEST_LIBREOFFICE").is_none() {
        return;
    }
    let cargo_executable = PathBuf::from(env!("CARGO_BIN_EXE_semwright-libreoffice-driver"));
    let binary_dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(binary_dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let executable = binary_dir.path().join("semwright-libreoffice-driver");
    std::fs::copy(&cargo_executable, &executable).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    let helper = PathBuf::from(
        std::env::var_os("SEMWRIGHT_TEST_SANDBOX_HELPER")
            .expect("SEMWRIGHT_TEST_SANDBOX_HELPER must point to semwright-sandbox"),
    );
    assert!(Path::new("/etc/libreoffice").is_dir());
    assert!(Path::new("/usr/bin/soffice").exists());

    let workspace = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    std::fs::set_permissions(state.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let workspace_path = std::fs::canonicalize(workspace.path()).unwrap();
    let config_path = std::fs::canonicalize("/etc/libreoffice").unwrap();

    let manifest = Manifest {
        manifest_version: 1,
        protocol: 1,
        id: "libreoffice".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        publisher: "semwright-tests".into(),
        executable: executable.clone(),
        sha256: digest(&executable),
        application: ApplicationMatch {
            desktop_id: Some("org.libreoffice.LibreOffice".into()),
            process_names: vec!["soffice.bin".into()],
            supported_versions: vec![],
        },
        transport: Transport::StdioV1,
        mounts: vec![DriverMount {
            root: "workspace".into(),
            read_only: false,
            execute: false,
        }],
        system_config: vec![
            SystemConfigMount {
                root: "libreoffice-config".into(),
                destination: "/etc/libreoffice".into(),
            },
            SystemConfigMount {
                root: "font-config".into(),
                destination: "/etc/fonts".into(),
            },
        ],
        network: false,
        resources: DriverResources {
            address_space_bytes: 2_147_483_648,
            cpu_seconds: 120,
            ..DriverResources::default()
        },
        request_timeout_ms: 30_000,
        interfaces: DriverInterfaces::default(),
    };
    let grants = vec![
        FilesystemGrant {
            name: "workspace".into(),
            path: workspace_path.clone(),
            read: true,
            write: true,
        },
        FilesystemGrant {
            name: "libreoffice-config".into(),
            path: config_path,
            read: true,
            write: false,
        },
        FilesystemGrant {
            name: "font-config".into(),
            path: std::fs::canonicalize("/etc/fonts").unwrap(),
            read: true,
            write: false,
        },
    ];

    let provider = DriverProvider::connect(manifest, state.path(), &helper, &grants, false)
        .await
        .unwrap();
    let capabilities = Provider::capabilities(provider.as_ref()).await.unwrap();
    assert_eq!(capabilities.len(), 7);
    assert!(capabilities.iter().all(|capability| {
        capability
            .descriptor
            .name
            .starts_with("driver.libreoffice.")
    }));

    let status = call(
        provider.as_ref(),
        &capabilities,
        "driver.libreoffice.status",
        json!({}),
    )
    .await
    .unwrap();
    assert_eq!(status["connected"], true);
    assert_eq!(status["product"], "LibreOffice");

    call(
        provider.as_ref(),
        &capabilities,
        "driver.libreoffice.writer.create",
        json!({"path":"note.odt","text":"Semwright UNO integration"}),
    )
    .await
    .unwrap();
    let read = call(
        provider.as_ref(),
        &capabilities,
        "driver.libreoffice.writer.read",
        json!({"path":"note.odt"}),
    )
    .await
    .unwrap();
    assert_eq!(read["text"], "Semwright UNO integration");

    call(
        provider.as_ref(),
        &capabilities,
        "driver.libreoffice.calc.create",
        json!({"path":"sheet.ods","cells":{"A1":"hello","B1":0,"C1":42.5}}),
    )
    .await
    .unwrap();
    let zero = call(
        provider.as_ref(),
        &capabilities,
        "driver.libreoffice.calc.get",
        json!({"path":"sheet.ods","cell":"B1"}),
    )
    .await
    .unwrap();
    assert_eq!(zero["kind"], "number");
    assert_eq!(zero["value"], 0.0);

    call(
        provider.as_ref(),
        &capabilities,
        "driver.libreoffice.calc.set",
        json!({"path":"sheet.ods","cell":"A2","value":"updated"}),
    )
    .await
    .unwrap();
    let updated = call(
        provider.as_ref(),
        &capabilities,
        "driver.libreoffice.calc.get",
        json!({"path":"sheet.ods","cell":"A2"}),
    )
    .await
    .unwrap();
    assert_eq!(updated["kind"], "text");
    assert_eq!(updated["value"], "updated");

    call(
        provider.as_ref(),
        &capabilities,
        "driver.libreoffice.export.pdf",
        json!({"path":"note.odt","output":"note.pdf"}),
    )
    .await
    .unwrap();
    assert!(workspace_path.join("note.odt").metadata().unwrap().len() > 0);
    assert!(workspace_path.join("sheet.ods").metadata().unwrap().len() > 0);
    let pdf = std::fs::read(workspace_path.join("note.pdf")).unwrap();
    assert!(pdf.starts_with(b"%PDF-"));

    let duplicate = call(
        provider.as_ref(),
        &capabilities,
        "driver.libreoffice.writer.create",
        json!({"path":"note.odt","text":"must not overwrite"}),
    )
    .await
    .unwrap_err();
    assert_eq!(duplicate.code, semwright_types::ErrorCode::Conflict);

    Provider::shutdown(provider.as_ref()).await.unwrap();
}
