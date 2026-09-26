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
        .unwrap_or_else(|| panic!("missing capability {name}"))
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
            session: "blender-live".into(),
            request_id: semwright_types::unique_id(),
            cancellation: CancellationToken::new(),
        },
        &find(capabilities, name).descriptor,
        &args,
    )
    .await
}

#[tokio::test]
#[ignore = "requires Blender 4.5 LTS and bubblewrap on a Linux host"]
async fn real_blender_driver_introspects_rna_renders_and_saves_inside_sandbox() {
    if std::env::var_os("SEMWRIGHT_TEST_BLENDER").is_none() {
        return;
    }

    let cargo_executable = PathBuf::from(env!("CARGO_BIN_EXE_semwright-blender-driver"));
    let binary_dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(binary_dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let executable = binary_dir.path().join("semwright-blender-driver");
    std::fs::copy(&cargo_executable, &executable).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();

    let helper = PathBuf::from(
        std::env::var_os("SEMWRIGHT_TEST_SANDBOX_HELPER")
            .expect("SEMWRIGHT_TEST_SANDBOX_HELPER must point to semwright-sandbox"),
    );
    assert!(Path::new("/usr/local/bin/blender").exists() || Path::new("/usr/bin/blender").exists());
    assert!(Path::new("/etc/fonts").is_dir());

    let workspace = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    std::fs::set_permissions(state.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let workspace_path = std::fs::canonicalize(workspace.path()).unwrap();

    let manifest = Manifest {
        manifest_version: 1,
        protocol: 1,
        id: "blender".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        publisher: "semwright-tests".into(),
        executable: executable.clone(),
        sha256: digest(&executable),
        application: ApplicationMatch {
            desktop_id: Some("org.blender.Blender".into()),
            process_names: vec!["blender".into()],
            supported_versions: vec!["4.5.14".into()],
        },
        transport: Transport::StdioV1,
        mounts: vec![DriverMount {
            root: "workspace".into(),
            read_only: false,
            execute: false,
        }],
        system_config: vec![SystemConfigMount {
            root: "font-config".into(),
            destination: "/etc/fonts".into(),
        }],
        network: false,
        loopback_port: None,
        resources: DriverResources {
            open_files: 256,
            processes: 64,
            cpu_seconds: 300,
            address_space_bytes: 4_294_967_296,
            file_size_bytes: 1_073_741_824,
        },
        request_timeout_ms: 120_000,
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
    assert!(capabilities.len() >= 22);
    assert!(
        capabilities
            .iter()
            .all(|capability| capability.descriptor.name.starts_with("driver.blender."))
    );

    let status = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.status",
        json!({}),
    )
    .await
    .unwrap();
    assert_eq!(status["connected"], true);
    assert_eq!(status["arbitrary_python"], false);

    let summary = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.introspect.summary",
        json!({}),
    )
    .await
    .unwrap();
    assert_eq!(summary["version"][0], 4);
    assert_eq!(summary["version"][1], 5);
    assert!(summary["rna_types"].as_u64().unwrap() > 100);
    assert!(summary["operators"].as_u64().unwrap() > 100);
    assert_eq!(summary["arbitrary_python"], false);
    assert_eq!(summary["generic_operator_invoke"], false);
    let operators = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.introspect.operators",
        json!({"query":"primitive_cube_add","limit":32}),
    )
    .await
    .unwrap();
    let operator_id = operators["items"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|item| {
            let id = item["id"].as_str()?;
            id.contains("primitive_cube_add").then(|| id.to_owned())
        })
        .expect("cube operator should be discoverable");

    let operator = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.introspect.operator.describe",
        json!({"id":operator_id}),
    )
    .await
    .unwrap();
    assert!(operator["properties"].as_array().unwrap().len() > 1);

    let types = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.introspect.types",
        json!({"query":"Mesh","limit":32}),
    )
    .await
    .unwrap();
    assert!(
        types["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| { item["identifier"].as_str() == Some("Mesh") })
    );

    let _addons = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.introspect.addons",
        json!({"limit":32}),
    )
    .await
    .unwrap();

    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.object.create",
        json!({"name":"SemwrightCube","primitive":"cube","location":[1.0,2.0,3.0]}),
    )
    .await
    .unwrap();
    let object = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.object.get",
        json!({"name":"SemwrightCube"}),
    )
    .await
    .unwrap();
    assert_eq!(object["object"]["name"], "SemwrightCube");

    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.material.create",
        json!({
            "name":"SemwrightMaterial",
            "color":[0.2,0.4,0.8,1.0],
            "roughness":0.35,
            "metallic":0.1
        }),
    )
    .await
    .unwrap();
    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.material.assign",
        json!({"object":"SemwrightCube","material":"SemwrightMaterial"}),
    )
    .await
    .unwrap();

    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.render.settings",
        json!({"width":64,"height":64,"engine":"CYCLES","samples":1}),
    )
    .await
    .unwrap();
    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.render",
        json!({"path":"preview.png"}),
    )
    .await
    .unwrap();
    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.file.save",
        json!({"path":"scene.blend"}),
    )
    .await
    .unwrap();

    let png = std::fs::read(workspace_path.join("preview.png")).unwrap();
    assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
    let blend = std::fs::read(workspace_path.join("scene.blend")).unwrap();
    assert!(blend.starts_with(b"BLENDER"));

    Provider::shutdown(provider.as_ref()).await.unwrap();
}
