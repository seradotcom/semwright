use semwright_backend_api::{Context, Provider};
use semwright_driver_host::DriverProvider;
use semwright_driver_sdk::{
    ApplicationMatch, DriverInterfaces, DriverMount, DriverResources, DriverToolMount, Manifest,
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
    hex::encode(Sha256::digest(bytes))
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
    let cargo_runner = PathBuf::from(env!("CARGO_BIN_EXE_semwright-blender-session-runner"));
    let binary_dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(binary_dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let executable = binary_dir.path().join("semwright-blender-driver");
    std::fs::copy(&cargo_executable, &executable).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    let session_runner = binary_dir.path().join("semwright-blender-session-runner");
    std::fs::copy(&cargo_runner, &session_runner).unwrap();
    std::fs::set_permissions(&session_runner, std::fs::Permissions::from_mode(0o700)).unwrap();

    let helper = PathBuf::from(
        std::env::var_os("SEMWRIGHT_TEST_SANDBOX_HELPER")
            .expect("SEMWRIGHT_TEST_SANDBOX_HELPER must point to semwright-sandbox"),
    );
    let blender_runtime = std::fs::canonicalize(PathBuf::from(
        std::env::var_os("SEMWRIGHT_TEST_BLENDER_ROOT")
            .expect("SEMWRIGHT_TEST_BLENDER_ROOT must point to Blender 4.5.14 LTS"),
    ))
    .unwrap();
    let blender_tool = blender_runtime.join("blender");
    assert!(blender_tool.is_file());
    for relative in [
        "lib",
        "4.5/scripts",
        "4.5/extensions",
        "4.5/datafiles",
        "4.5/python",
    ] {
        assert!(blender_runtime.join(relative).is_dir());
    }
    assert!(Path::new("/etc/fonts").is_dir());

    let workspace = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    std::fs::set_permissions(state.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let workspace_path = std::fs::canonicalize(workspace.path()).unwrap();

    let manifest = Manifest {
        manifest_version: 1,
        protocol: 8,
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
        mounts: vec![
            DriverMount {
                root: "workspace".into(),
                read_only: false,
                execute: false,
            },
            DriverMount {
                root: "blender-runtime".into(),
                read_only: true,
                execute: false,
            },
            DriverMount {
                root: "scratch".into(),
                read_only: false,
                execute: false,
            },
            DriverMount {
                root: "font-config".into(),
                read_only: true,
                execute: false,
            },
        ],
        system_config: vec![],
        secrets: vec![],
        tools: vec![
            DriverToolMount {
                root: "blender-session-runner".into(),
                name: "blender-session-runner".into(),
                mounts: vec![
                    "workspace".into(),
                    "blender-runtime".into(),
                    "scratch".into(),
                    "font-config".into(),
                ],
                system_config: vec![],
                dependencies: vec!["blender".into()],
                nvidia_gpu: false,
                resources: None,
                sha256: digest(&session_runner),
            },
            DriverToolMount {
                root: "blender-executable".into(),
                name: "blender".into(),
                mounts: vec![],
                system_config: vec![],
                dependencies: vec![],
                nvidia_gpu: false,
                resources: None,
                sha256: digest(&blender_tool),
            },
        ],
        network: false,
        nvidia_gpu: false,
        loopback_port: None,
        resources: DriverResources {
            open_files: 256,
            processes: 64,
            cpu_seconds: 300,
            operation_cpu_seconds: 0,
            address_space_bytes: 4_294_967_296,
            file_size_bytes: 1_073_741_824,
        },
        request_timeout_ms: 300_000,
        interfaces: DriverInterfaces {
            cooperative_cancellation: true,
            progress: true,
            health: true,
            host_tools: true,
            ..DriverInterfaces::default()
        },
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
        FilesystemGrant {
            name: "blender-runtime".into(),
            path: blender_runtime.clone(),
            read: true,
            write: false,
        },
        FilesystemGrant {
            name: "scratch".into(),
            path: std::fs::canonicalize(scratch.path()).unwrap(),
            read: true,
            write: true,
        },
        FilesystemGrant {
            name: "blender-session-runner".into(),
            path: session_runner.clone(),
            read: true,
            write: false,
        },
        FilesystemGrant {
            name: "blender-executable".into(),
            path: blender_tool.clone(),
            read: true,
            write: false,
        },
    ];

    let provider = DriverProvider::connect(manifest, state.path(), &helper, &grants, false)
        .await
        .unwrap();
    let capabilities = Provider::capabilities(provider.as_ref()).await.unwrap();
    assert!(capabilities.len() >= 80);
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

    let semantic_summary = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.summary",
        json!({}),
    )
    .await
    .unwrap();
    assert_eq!(semantic_summary["schema"], "blender-rna-semantic/v1");
    assert_eq!(semantic_summary["blender_version"][0], 4);
    assert_eq!(semantic_summary["blender_version"][1], 5);
    assert_eq!(semantic_summary["arbitrary_python"], false);

    for (root, name, kind) in [
        ("brushes", "SemwrightBrush", None),
        ("linestyles", "SemwrightLineStyle", None),
        ("particles", "SemwrightParticles", None),
        ("textures", "SemwrightTexture", Some("CLOUDS")),
        ("scenes", "SemwrightSecondaryScene", None),
    ] {
        let args = match kind {
            Some(kind) => json!({"root":root,"name":name,"kind":kind}),
            None => json!({"root":root,"name":name}),
        };
        let created = call(
            provider.as_ref(),
            &capabilities,
            "driver.blender.semantic.datablock.create",
            args,
        )
        .await
        .unwrap();
        assert_eq!(created["root"], root);
        let reference = created["ref"].as_str().unwrap().to_owned();
        let removed = call(
            provider.as_ref(),
            &capabilities,
            "driver.blender.semantic.datablock.remove",
            json!({"ref":reference}),
        )
        .await
        .unwrap();
        assert_eq!(removed["changed"], true);
    }

    let object_type = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.type.describe",
        json!({"root":"objects"}),
    )
    .await
    .unwrap();
    assert_eq!(object_type["identifier"], "Object");
    assert!(
        object_type["properties"]
            .as_array()
            .unwrap()
            .iter()
            .any(|property| { property["id"] == "location" && property["status"] == "managed" })
    );
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

    let mesh_query = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.query",
        json!({"root":"objects","property":"type","operator":"eq","value":"MESH","limit":16}),
    )
    .await
    .unwrap();
    assert!(
        mesh_query["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["name"] == "SemwrightCube")
    );

    let semantic_objects = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"objects","query":"SemwrightCube","limit":8}),
    )
    .await
    .unwrap();
    let semantic_ref = semantic_objects["items"][0]["ref"]
        .as_str()
        .unwrap()
        .to_owned();
    let location = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.property.get",
        json!({"ref":semantic_ref,"property":"location"}),
    )
    .await
    .unwrap();
    assert_eq!(location["value"], json!([1.0, 2.0, 3.0]));

    let object_data = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":semantic_ref,"property":"data","limit":8}),
    )
    .await
    .unwrap();
    assert_eq!(object_data["items"][0]["rna_type"], "Mesh");
    let mesh_ref = object_data["items"][0]["ref"].as_str().unwrap().to_owned();
    let vertices = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":mesh_ref,"property":"vertices","limit":8,"offset":0}),
    )
    .await
    .unwrap();
    assert_eq!(vertices["items"].as_array().unwrap().len(), 8);
    let vertex_ref = vertices["items"][0]["ref"].as_str().unwrap().to_owned();
    let vertex = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.property.get",
        json!({"ref":vertex_ref,"property":"co"}),
    )
    .await
    .unwrap();
    assert_eq!(vertex["descriptor"]["status"], "managed");

    let hide = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.property.set",
        json!({"ref":semantic_ref,"property":"hide_render","value":true}),
    )
    .await
    .unwrap();
    assert_eq!(hide["value"], true);
    let fresh_ref = hide["ref"].as_str().unwrap().to_owned();

    let stale = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.property.get",
        json!({"ref":semantic_ref,"property":"hide_render"}),
    )
    .await
    .unwrap_err();
    assert_eq!(stale.code, semwright_types::ErrorCode::StaleReference);

    let reset = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.property.reset",
        json!({"ref":fresh_ref,"property":"hide_render"}),
    )
    .await
    .unwrap();
    assert_eq!(reset["value"], false);
    let mut authored_object_ref = reset["ref"].as_str().unwrap().to_owned();

    let modifier = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.modifier.add",
        json!({"object_ref":authored_object_ref,"name":"SemwrightSubdivision","type":"SUBSURF"}),
    )
    .await
    .unwrap();
    assert_eq!(modifier["type"], "SUBSURF");
    let modifier_type = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.rna.describe",
        json!({"identifier":"SubsurfModifier"}),
    )
    .await
    .unwrap();
    assert!(
        modifier_type["properties"]
            .as_array()
            .unwrap()
            .iter()
            .any(|property| property["id"] == "levels")
    );
    let modifier_ref = modifier["ref"].as_str().unwrap().to_owned();
    let renamed_modifier = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.rename",
        json!({"ref":modifier_ref,"name":"SemwrightSubdivisionRenamed"}),
    )
    .await
    .unwrap();
    assert_eq!(renamed_modifier["name"], "SemwrightSubdivisionRenamed");
    let modifier_ref = renamed_modifier["ref"].as_str().unwrap().to_owned();
    let modifier_visibility = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.property.get",
        json!({"ref":modifier_ref,"property":"show_viewport"}),
    )
    .await
    .unwrap();
    assert!(modifier_visibility["value"].is_boolean());
    let removed_modifier = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.modifier.remove",
        json!({"ref":modifier_ref}),
    )
    .await
    .unwrap();
    authored_object_ref = removed_modifier["object_ref"].as_str().unwrap().to_owned();

    let constraint = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.constraint.add",
        json!({"object_ref":authored_object_ref,"name":"SemwrightLimit","type":"LIMIT_LOCATION"}),
    )
    .await
    .unwrap();
    let constraint_ref = constraint["ref"].as_str().unwrap().to_owned();
    let constraint_mutation = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.property.set",
        json!({"ref":constraint_ref,"property":"use_min_x","value":true}),
    )
    .await
    .unwrap();
    let constraint_ref = constraint_mutation["ref"].as_str().unwrap().to_owned();
    let removed_constraint = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.constraint.remove",
        json!({"ref":constraint_ref}),
    )
    .await
    .unwrap();
    authored_object_ref = removed_constraint["object_ref"]
        .as_str()
        .unwrap()
        .to_owned();

    let keyed = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.animation.keyframe.insert",
        json!({"ref":authored_object_ref,"property":"location","frame":1.0,"index":0}),
    )
    .await
    .unwrap();
    authored_object_ref = keyed["ref"].as_str().unwrap().to_owned();

    let actions = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"actions","limit":16}),
    )
    .await
    .unwrap();
    assert!(!actions["items"].as_array().unwrap().is_empty());
    let nla_track = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.nla.track.add",
        json!({"owner_ref":authored_object_ref,"name":"SemwrightTrack"}),
    )
    .await
    .unwrap();
    let track_ref = nla_track["ref"].as_str().unwrap().to_owned();
    let actions = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"actions","limit":16}),
    )
    .await
    .unwrap();
    let action_ref = actions["items"][0]["ref"].as_str().unwrap().to_owned();
    let nla_strip = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.nla.strip.add",
        json!({"track_ref":track_ref,"name":"SemwrightStrip","start":1,"action_ref":action_ref}),
    )
    .await
    .unwrap();
    let strip_ref = nla_strip["ref"].as_str().unwrap().to_owned();
    let removed_strip = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.nla.strip.remove",
        json!({"ref":strip_ref}),
    )
    .await
    .unwrap();
    let track_ref = removed_strip["track_ref"].as_str().unwrap().to_owned();
    let removed_track = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.nla.track.remove",
        json!({"ref":track_ref}),
    )
    .await
    .unwrap();
    authored_object_ref = removed_track["owner_ref"].as_str().unwrap().to_owned();

    let unkeyed = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.animation.keyframe.delete",
        json!({"ref":authored_object_ref,"property":"location","frame":1.0,"index":0}),
    )
    .await
    .unwrap();
    assert_eq!(unkeyed["changed"], true);

    // Exercise Blender 4.5's layered/slotted Action API without relying on
    // legacy Action.fcurves.
    let layered_action = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.datablock.create",
        json!({"root":"actions","name":"SemwrightLayeredAction"}),
    )
    .await
    .unwrap();
    let action_ref = layered_action["ref"].as_str().unwrap().to_owned();
    let slot = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.action.slot.add",
        json!({"action_ref":action_ref,"id_type":"OBJECT","name":"SemwrightCube"}),
    )
    .await
    .unwrap();
    assert_eq!(slot["id_type"], "OBJECT");

    let actions = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"actions","query":"SemwrightLayeredAction","limit":8}),
    )
    .await
    .unwrap();
    let action_ref = actions["items"][0]["ref"].as_str().unwrap().to_owned();
    let layer = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.action.layer.add",
        json!({"action_ref":action_ref,"name":"SemwrightLayer"}),
    )
    .await
    .unwrap();
    let layer_ref = layer["ref"].as_str().unwrap().to_owned();
    let strip = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.action.strip.add",
        json!({"layer_ref":layer_ref}),
    )
    .await
    .unwrap();
    let strip_ref = strip["ref"].as_str().unwrap().to_owned();

    let actions = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"actions","query":"SemwrightLayeredAction","limit":8}),
    )
    .await
    .unwrap();
    let action_ref = actions["items"][0]["ref"].as_str().unwrap().to_owned();
    let slots = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":action_ref,"property":"slots","limit":8}),
    )
    .await
    .unwrap();
    let slot_ref = slots["items"][0]["ref"].as_str().unwrap().to_owned();
    let channelbag = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.action.channelbag.ensure",
        json!({"strip_ref":strip_ref,"slot_ref":slot_ref}),
    )
    .await
    .unwrap();
    let channelbag_ref = channelbag["ref"].as_str().unwrap().to_owned();
    let action_curve = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.action.fcurve.ensure",
        json!({"channelbag_ref":channelbag_ref,"data_path":"location","index":0,"group_name":"Semwright"}),
    )
    .await
    .unwrap();
    assert_eq!(action_curve["data_path"], "location");
    let curve_ref = action_curve["ref"].as_str().unwrap().to_owned();
    let point = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.fcurve.keyframe.add",
        json!({"fcurve_ref":curve_ref,"frame":10.0,"value":2.0,"keyframe_type":"KEYFRAME"}),
    )
    .await
    .unwrap();
    assert_eq!(point["frame"], 10.0);

    let actions = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"actions","query":"SemwrightLayeredAction","limit":8}),
    )
    .await
    .unwrap();
    let action_ref = actions["items"][0]["ref"].as_str().unwrap().to_owned();
    let layers = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":action_ref,"property":"layers","limit":8}),
    )
    .await
    .unwrap();
    let layer_ref = layers["items"][0]["ref"].as_str().unwrap().to_owned();
    let strips = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":layer_ref,"property":"strips","limit":8}),
    )
    .await
    .unwrap();
    let strip_ref = strips["items"][0]["ref"].as_str().unwrap().to_owned();
    let bags = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":strip_ref,"property":"channelbags","limit":8}),
    )
    .await
    .unwrap();
    let bag_ref = bags["items"][0]["ref"].as_str().unwrap().to_owned();
    let curves = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":bag_ref,"property":"fcurves","limit":8}),
    )
    .await
    .unwrap();
    let curve_ref = curves["items"][0]["ref"].as_str().unwrap().to_owned();
    let curve_modifier = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.fcurve.modifier.add",
        json!({"fcurve_ref":curve_ref,"type":"NOISE"}),
    )
    .await
    .unwrap();
    let modifier_ref = curve_modifier["ref"].as_str().unwrap().to_owned();
    let removed_curve_modifier = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.fcurve.modifier.remove",
        json!({"ref":modifier_ref}),
    )
    .await
    .unwrap();
    let curve_ref = removed_curve_modifier["fcurve_ref"]
        .as_str()
        .unwrap()
        .to_owned();
    let points = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":curve_ref,"property":"keyframe_points","limit":8}),
    )
    .await
    .unwrap();
    let point_ref = points["items"][0]["ref"].as_str().unwrap().to_owned();
    let removed_point = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.fcurve.keyframe.remove",
        json!({"ref":point_ref}),
    )
    .await
    .unwrap();
    let curve_ref = removed_point["fcurve_ref"].as_str().unwrap().to_owned();
    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.action.fcurve.remove",
        json!({"ref":curve_ref}),
    )
    .await
    .unwrap();

    let actions = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"actions","query":"SemwrightLayeredAction","limit":8}),
    )
    .await
    .unwrap();
    let action_ref = actions["items"][0]["ref"].as_str().unwrap().to_owned();
    let layers = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":action_ref,"property":"layers","limit":8}),
    )
    .await
    .unwrap();
    let layer_ref = layers["items"][0]["ref"].as_str().unwrap().to_owned();
    let strips = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":layer_ref,"property":"strips","limit":8}),
    )
    .await
    .unwrap();
    let strip_ref = strips["items"][0]["ref"].as_str().unwrap().to_owned();
    let removed_action_strip = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.action.strip.remove",
        json!({"ref":strip_ref}),
    )
    .await
    .unwrap();
    let layer_ref = removed_action_strip["layer_ref"]
        .as_str()
        .unwrap()
        .to_owned();
    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.action.layer.remove",
        json!({"ref":layer_ref}),
    )
    .await
    .unwrap();

    let actions = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"actions","query":"SemwrightLayeredAction","limit":8}),
    )
    .await
    .unwrap();
    let action_ref = actions["items"][0]["ref"].as_str().unwrap().to_owned();
    let slots = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":action_ref,"property":"slots","limit":8}),
    )
    .await
    .unwrap();
    let slot_ref = slots["items"][0]["ref"].as_str().unwrap().to_owned();
    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.action.slot.remove",
        json!({"ref":slot_ref}),
    )
    .await
    .unwrap();
    let actions = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"actions","query":"SemwrightLayeredAction","limit":8}),
    )
    .await
    .unwrap();
    let action_ref = actions["items"][0]["ref"].as_str().unwrap().to_owned();
    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.datablock.remove",
        json!({"ref":action_ref}),
    )
    .await
    .unwrap();

    let mask = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.datablock.create",
        json!({"root":"masks","name":"SemwrightMask"}),
    )
    .await
    .unwrap();
    let mask_ref = mask["ref"].as_str().unwrap().to_owned();
    let mask_layer = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.mask.layer.add",
        json!({"mask_ref":mask_ref,"name":"SemwrightMaskLayer"}),
    )
    .await
    .unwrap();
    let layer_ref = mask_layer["ref"].as_str().unwrap().to_owned();
    let mask_spline = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.mask.spline.add",
        json!({"layer_ref":layer_ref,"points":2}),
    )
    .await
    .unwrap();
    assert_eq!(mask_spline["points"], 2);
    let spline_ref = mask_spline["ref"].as_str().unwrap().to_owned();
    let mask_points = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":spline_ref,"property":"points","limit":8}),
    )
    .await
    .unwrap();
    assert_eq!(mask_points["items"].as_array().unwrap().len(), 2);
    let point_ref = mask_points["items"][0]["ref"].as_str().unwrap().to_owned();
    let moved_mask_point = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.property.set",
        json!({"ref":point_ref,"property":"co","value":[0.25,0.5]}),
    )
    .await
    .unwrap();
    assert_eq!(moved_mask_point["value"], json!([0.25, 0.5]));

    let masks = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"masks","query":"SemwrightMask","limit":8}),
    )
    .await
    .unwrap();
    let mask_ref = masks["items"][0]["ref"].as_str().unwrap().to_owned();
    let layers = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":mask_ref,"property":"layers","limit":8}),
    )
    .await
    .unwrap();
    let layer_ref = layers["items"][0]["ref"].as_str().unwrap().to_owned();
    let splines = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":layer_ref,"property":"splines","limit":8}),
    )
    .await
    .unwrap();
    let spline_ref = splines["items"][0]["ref"].as_str().unwrap().to_owned();
    let grown_mask = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.mask.points.add",
        json!({"spline_ref":spline_ref,"count":1}),
    )
    .await
    .unwrap();
    assert_eq!(grown_mask["point_count"], 3);
    let spline_ref = grown_mask["ref"].as_str().unwrap().to_owned();
    let points = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":spline_ref,"property":"points","limit":8}),
    )
    .await
    .unwrap();
    let point_ref = points["items"][2]["ref"].as_str().unwrap().to_owned();
    let removed_point = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.mask.point.remove",
        json!({"ref":point_ref}),
    )
    .await
    .unwrap();
    let spline_ref = removed_point["spline_ref"].as_str().unwrap().to_owned();
    let removed_spline = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.mask.spline.remove",
        json!({"ref":spline_ref}),
    )
    .await
    .unwrap();
    let layer_ref = removed_spline["layer_ref"].as_str().unwrap().to_owned();
    let removed_layer = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.mask.layer.remove",
        json!({"ref":layer_ref}),
    )
    .await
    .unwrap();
    let mask_ref = removed_layer["mask_ref"].as_str().unwrap().to_owned();
    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.datablock.remove",
        json!({"ref":mask_ref}),
    )
    .await
    .unwrap();

    let grease = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.datablock.create",
        json!({"root":"grease_pencils_v3","name":"SemwrightGrease"}),
    )
    .await
    .unwrap();
    assert_eq!(grease["rna_type"], "GreasePencilv3");
    let grease_ref = grease["ref"].as_str().unwrap().to_owned();
    let grease_layer = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.grease.layer.add",
        json!({"grease_ref":grease_ref,"name":"SemwrightLayer","set_active":true}),
    )
    .await
    .unwrap();
    let grease_layer_ref = grease_layer["ref"].as_str().unwrap().to_owned();
    let grease_frame = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.grease.frame.add",
        json!({"layer_ref":grease_layer_ref,"frame":1}),
    )
    .await
    .unwrap();
    let grease_frame_ref = grease_frame["ref"].as_str().unwrap().to_owned();
    let drawing = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":grease_frame_ref,"property":"drawing","limit":8}),
    )
    .await
    .unwrap();
    let drawing_ref = drawing["items"][0]["ref"].as_str().unwrap().to_owned();
    let strokes = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.grease.strokes.add",
        json!({"drawing_ref":drawing_ref,"sizes":[3,2]}),
    )
    .await
    .unwrap();
    assert_eq!(strokes["strokes_added"], 2);
    let drawing_ref = strokes["ref"].as_str().unwrap().to_owned();
    let resized_strokes = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.grease.strokes.resize",
        json!({"drawing_ref":drawing_ref,"sizes":[4],"indices":[0]}),
    )
    .await
    .unwrap();
    let drawing_ref = resized_strokes["ref"].as_str().unwrap().to_owned();
    let removed_strokes = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.grease.strokes.remove",
        json!({"drawing_ref":drawing_ref,"indices":[1]}),
    )
    .await
    .unwrap();
    assert_eq!(removed_strokes["stroke_count"], 1);

    let grease = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"grease_pencils_v3","query":"SemwrightGrease","limit":8}),
    )
    .await
    .unwrap();
    let grease_ref = grease["items"][0]["ref"].as_str().unwrap().to_owned();
    let layers = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":grease_ref,"property":"layers","limit":8}),
    )
    .await
    .unwrap();
    let layer_ref = layers["items"][0]["ref"].as_str().unwrap().to_owned();
    let frames = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":layer_ref,"property":"frames","limit":8}),
    )
    .await
    .unwrap();
    let frame_ref = frames["items"][0]["ref"].as_str().unwrap().to_owned();
    let removed_frame = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.grease.frame.remove",
        json!({"ref":frame_ref}),
    )
    .await
    .unwrap();
    let layer_ref = removed_frame["layer_ref"].as_str().unwrap().to_owned();
    let removed_layer = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.grease.layer.remove",
        json!({"ref":layer_ref}),
    )
    .await
    .unwrap();
    let grease_ref = removed_layer["grease_ref"].as_str().unwrap().to_owned();
    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.datablock.remove",
        json!({"ref":grease_ref}),
    )
    .await
    .unwrap();

    let hair = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.datablock.create",
        json!({"root":"hair_curves","name":"SemwrightHair"}),
    )
    .await
    .unwrap();
    let hair_ref = hair["ref"].as_str().unwrap().to_owned();
    let hair_added = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.hair_curves.add",
        json!({"curves_ref":hair_ref,"sizes":[2,3]}),
    )
    .await
    .unwrap();
    assert_eq!(hair_added["curve_count"], 2);
    let hair_ref = hair_added["ref"].as_str().unwrap().to_owned();
    let hair_typed = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.hair_curves.types.set",
        json!({"curves_ref":hair_ref,"type":"POLY","indices":[0,1]}),
    )
    .await
    .unwrap();
    let hair_ref = hair_typed["ref"].as_str().unwrap().to_owned();
    let hair_resized = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.hair_curves.resize",
        json!({"curves_ref":hair_ref,"sizes":[4],"indices":[0]}),
    )
    .await
    .unwrap();
    let hair_ref = hair_resized["ref"].as_str().unwrap().to_owned();
    let hair_reordered = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.hair_curves.reorder",
        json!({"curves_ref":hair_ref,"new_indices":[1,0]}),
    )
    .await
    .unwrap();
    let hair_ref = hair_reordered["ref"].as_str().unwrap().to_owned();
    let hair_removed = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.hair_curves.remove",
        json!({"curves_ref":hair_ref,"indices":[1]}),
    )
    .await
    .unwrap();
    assert_eq!(hair_removed["curve_count"], 1);
    let hair_ref = hair_removed["ref"].as_str().unwrap().to_owned();
    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.datablock.remove",
        json!({"ref":hair_ref}),
    )
    .await
    .unwrap();

    let metaball = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.datablock.create",
        json!({"root":"metaballs","name":"SemwrightMetaBall"}),
    )
    .await
    .unwrap();
    let metaball_ref = metaball["ref"].as_str().unwrap().to_owned();
    let element = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.metaball.element.add",
        json!({"metaball_ref":metaball_ref,"type":"BALL"}),
    )
    .await
    .unwrap();
    let element_ref = element["ref"].as_str().unwrap().to_owned();
    let resized_element = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.property.set",
        json!({"ref":element_ref,"property":"radius","value":1.25}),
    )
    .await
    .unwrap();
    assert_eq!(resized_element["value"], 1.25);
    let element_ref = resized_element["ref"].as_str().unwrap().to_owned();
    let removed_element = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.metaball.element.remove",
        json!({"ref":element_ref}),
    )
    .await
    .unwrap();
    let metaball_ref = removed_element["metaball_ref"].as_str().unwrap().to_owned();
    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.datablock.remove",
        json!({"ref":metaball_ref}),
    )
    .await
    .unwrap();

    let refreshed_objects = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"objects","query":"SemwrightCube","limit":8}),
    )
    .await
    .unwrap();
    authored_object_ref = refreshed_objects["items"][0]["ref"]
        .as_str()
        .unwrap()
        .to_owned();

    let basis = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.shape_key.add",
        json!({"object_ref":authored_object_ref,"name":"Basis","from_mix":false}),
    )
    .await
    .unwrap();
    let basis_ref = basis["ref"].as_str().unwrap().to_owned();
    let refreshed_objects = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"objects","query":"SemwrightCube","limit":8}),
    )
    .await
    .unwrap();
    authored_object_ref = refreshed_objects["items"][0]["ref"]
        .as_str()
        .unwrap()
        .to_owned();
    let target_key = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.shape_key.add",
        json!({"object_ref":authored_object_ref,"name":"SemwrightShape","from_mix":false}),
    )
    .await
    .unwrap();
    let target_key_ref = target_key["ref"].as_str().unwrap().to_owned();
    let key_value = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.property.set",
        json!({"ref":target_key_ref,"property":"value","value":0.5}),
    )
    .await
    .unwrap();
    let target_key_ref = key_value["ref"].as_str().unwrap().to_owned();
    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.shape_key.remove",
        json!({"ref":target_key_ref}),
    )
    .await
    .unwrap();
    let refreshed_objects = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"objects","query":"SemwrightCube","limit":8}),
    )
    .await
    .unwrap();
    authored_object_ref = refreshed_objects["items"][0]["ref"]
        .as_str()
        .unwrap()
        .to_owned();
    let key_blocks = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":authored_object_ref,"property":"data","limit":8}),
    )
    .await
    .unwrap();
    let cube_mesh_ref = key_blocks["items"][0]["ref"].as_str().unwrap().to_owned();
    let shape_keys = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":cube_mesh_ref,"property":"shape_keys","limit":8}),
    )
    .await
    .unwrap();
    let shape_keys_ref = shape_keys["items"][0]["ref"].as_str().unwrap().to_owned();
    let basis_keys = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":shape_keys_ref,"property":"key_blocks","limit":8}),
    )
    .await
    .unwrap();
    assert_eq!(basis_keys["items"].as_array().unwrap().len(), 1);
    assert_eq!(basis_keys["items"][0]["name"], "Basis");
    let _ = basis_ref;

    let custom = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.custom.set",
        json!({"ref":authored_object_ref,"key":"semwright.test","value":0.75}),
    )
    .await
    .unwrap();
    authored_object_ref = custom["ref"].as_str().unwrap().to_owned();
    let custom_read = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.custom.get",
        json!({"ref":authored_object_ref,"key":"semwright.test"}),
    )
    .await
    .unwrap();
    assert_eq!(custom_read["value"], 0.75);
    let custom_removed = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.custom.remove",
        json!({"ref":authored_object_ref,"key":"semwright.test"}),
    )
    .await
    .unwrap();
    assert_eq!(custom_removed["changed"], true);

    let refreshed_objects = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"objects","query":"SemwrightCube","limit":8}),
    )
    .await
    .unwrap();
    let cube_ref = refreshed_objects["items"][0]["ref"]
        .as_str()
        .unwrap()
        .to_owned();
    let vertex_group = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.vertex_group.add",
        json!({"object_ref":cube_ref,"name":"SemwrightWeights"}),
    )
    .await
    .unwrap();
    let group_ref = vertex_group["ref"].as_str().unwrap().to_owned();
    let weighted = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.vertex_group.weights.set",
        json!({"ref":group_ref,"indices":[0,1],"weight":0.75,"mode":"REPLACE"}),
    )
    .await
    .unwrap();
    assert_eq!(weighted["vertices"], 2);
    let group_ref = weighted["ref"].as_str().unwrap().to_owned();
    let cleared = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.vertex_group.weights.remove",
        json!({"ref":group_ref,"indices":[1]}),
    )
    .await
    .unwrap();
    let group_ref = cleared["ref"].as_str().unwrap().to_owned();
    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.vertex_group.remove",
        json!({"ref":group_ref}),
    )
    .await
    .unwrap();

    let mesh = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.datablock.create",
        json!({"root":"meshes","name":"SemwrightTriangle"}),
    )
    .await
    .unwrap();
    let mesh_ref = mesh["ref"].as_str().unwrap().to_owned();
    let initialized = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.mesh.geometry.initialize",
        json!({
            "mesh_ref":mesh_ref,
            "vertices":[[0.0,0.0,0.0],[1.0,0.0,0.0],[0.0,1.0,0.0]],
            "edges":[[0,1],[1,2],[2,0]],
            "faces":[[0,1,2]]
        }),
    )
    .await
    .unwrap();
    assert_eq!(initialized["vertices"], 3);
    assert_eq!(initialized["edges"], 3);
    assert_eq!(initialized["faces"], 1);
    let initialized_ref = initialized["ref"].as_str().unwrap().to_owned();
    let reinitialize = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.mesh.geometry.initialize",
        json!({
            "mesh_ref":initialized_ref.clone(),
            "vertices":[[0.0,0.0,0.0],[1.0,0.0,0.0],[0.0,1.0,0.0]],
            "edges":[[0,1],[1,2],[2,0]],
            "faces":[[0,1,2]]
        }),
    )
    .await
    .unwrap_err();
    assert_eq!(reinitialize.code, semwright_types::ErrorCode::Conflict);

    let replaced = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.mesh.geometry.replace",
        json!({
            "mesh_ref":initialized_ref,
            "vertices":[[0.0,0.0,0.0],[1.0,0.0,0.0],[0.0,1.0,0.0]],
            "edges":[[0,1],[1,2],[2,0]],
            "faces":[[0,1,2]]
        }),
    )
    .await
    .unwrap();
    assert_eq!(replaced["vertices"], 3);
    assert_eq!(replaced["edges"], 3);
    assert_eq!(replaced["faces"], 1);
    let mesh_ref = replaced["ref"].as_str().unwrap().to_owned();
    let mesh_summary = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.mesh.summary",
        json!({"mesh_ref":mesh_ref}),
    )
    .await
    .unwrap();
    assert_eq!(mesh_summary["faces"], 1);
    let attribute = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.mesh.attribute.add",
        json!({"mesh_ref":mesh_ref,"name":"semwright_weight","data_type":"FLOAT","domain":"POINT"}),
    )
    .await
    .unwrap();
    assert_eq!(attribute["domain"], "POINT");
    let attribute_ref = attribute["ref"].as_str().unwrap().to_owned();
    let attribute_data = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":attribute_ref,"property":"data","limit":8}),
    )
    .await
    .unwrap();
    assert_eq!(attribute_data["items"].as_array().unwrap().len(), 3);
    let removed_attribute = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.mesh.attribute.remove",
        json!({"ref":attribute_ref}),
    )
    .await
    .unwrap();
    let mesh_ref = removed_attribute["mesh_ref"].as_str().unwrap().to_owned();
    let uv_layer = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.mesh.uv_layer.add",
        json!({"mesh_ref":mesh_ref,"name":"SemwrightUV","do_init":true}),
    )
    .await
    .unwrap();
    let uv_ref = uv_layer["ref"].as_str().unwrap().to_owned();
    let removed_uv = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.mesh.uv_layer.remove",
        json!({"ref":uv_ref}),
    )
    .await
    .unwrap();
    let mesh_ref = removed_uv["mesh_ref"].as_str().unwrap().to_owned();
    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.datablock.remove",
        json!({"ref":mesh_ref}),
    )
    .await
    .unwrap();

    let curve = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.datablock.create",
        json!({"root":"curves","name":"SemwrightCurve","kind":"CURVE"}),
    )
    .await
    .unwrap();
    let curve_ref = curve["ref"].as_str().unwrap().to_owned();
    let spline = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.curve.spline.add",
        json!({"curve_ref":curve_ref,"type":"POLY","points":3}),
    )
    .await
    .unwrap();
    assert_eq!(spline["points"], 3);
    let spline_ref = spline["ref"].as_str().unwrap().to_owned();
    let points = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":spline_ref,"property":"points","limit":8}),
    )
    .await
    .unwrap();
    assert_eq!(points["items"].as_array().unwrap().len(), 3);
    let refreshed_curves = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"curves","query":"SemwrightCurve","limit":8}),
    )
    .await
    .unwrap();
    let curve_ref = refreshed_curves["items"][0]["ref"]
        .as_str()
        .unwrap()
        .to_owned();
    let splines = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":curve_ref,"property":"splines","limit":8}),
    )
    .await
    .unwrap();
    let spline_ref = splines["items"][0]["ref"].as_str().unwrap().to_owned();
    let removed_spline = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.curve.spline.remove",
        json!({"ref":spline_ref}),
    )
    .await
    .unwrap();
    let curve_ref = removed_spline["curve_ref"].as_str().unwrap().to_owned();
    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.datablock.remove",
        json!({"ref":curve_ref}),
    )
    .await
    .unwrap();

    let armature = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.datablock.create",
        json!({"root":"armatures","name":"SemwrightRigData"}),
    )
    .await
    .unwrap();
    let rig_created = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.object.create",
        json!({"name":"SemwrightRig","data_ref":armature["ref"]}),
    )
    .await
    .unwrap();
    assert_eq!(rig_created["object_type"], "ARMATURE");
    let mut rig_ref = rig_created["ref"].as_str().unwrap().to_owned();
    let root_bone = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.armature.bone.add",
        json!({"object_ref":rig_ref,"name":"Root","head":[0.0,0.0,0.0],"tail":[0.0,0.0,1.0]}),
    )
    .await
    .unwrap();
    assert_eq!(root_bone["name"], "Root");
    let rigs = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"objects","query":"SemwrightRig","limit":8}),
    )
    .await
    .unwrap();
    rig_ref = rigs["items"][0]["ref"].as_str().unwrap().to_owned();
    let child_bone = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.armature.bone.add",
        json!({"object_ref":rig_ref,"name":"Child","head":[0.0,0.0,1.0],"tail":[0.0,0.0,2.0],"parent_name":"Root","connected":true}),
    )
    .await
    .unwrap();
    assert_eq!(child_bone["name"], "Child");

    let rigs = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"objects","query":"SemwrightRig","limit":8}),
    )
    .await
    .unwrap();
    rig_ref = rigs["items"][0]["ref"].as_str().unwrap().to_owned();
    let rig_data = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":rig_ref,"property":"data","limit":8}),
    )
    .await
    .unwrap();
    let rig_data_ref = rig_data["items"][0]["ref"].as_str().unwrap().to_owned();
    let bones = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":rig_data_ref,"property":"bones","limit":16}),
    )
    .await
    .unwrap();
    assert!(
        bones["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["name"] == "Root")
    );
    let bone_collection = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.bone_collection.add",
        json!({"armature_ref":rig_data_ref,"name":"SemwrightControls"}),
    )
    .await
    .unwrap();
    let collection_ref = bone_collection["ref"].as_str().unwrap().to_owned();

    let rigs = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"objects","query":"SemwrightRig","limit":8}),
    )
    .await
    .unwrap();
    rig_ref = rigs["items"][0]["ref"].as_str().unwrap().to_owned();
    let rig_data = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":rig_ref,"property":"data","limit":8}),
    )
    .await
    .unwrap();
    let rig_data_ref = rig_data["items"][0]["ref"].as_str().unwrap().to_owned();
    let bones = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":rig_data_ref,"property":"bones","limit":16}),
    )
    .await
    .unwrap();
    let root_ref = bones["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "Root")
        .unwrap()["ref"]
        .as_str()
        .unwrap()
        .to_owned();
    let assigned = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.bone_collection.assign",
        json!({"collection_ref":collection_ref,"bone_ref":root_ref}),
    )
    .await
    .unwrap();
    assert_eq!(assigned["assigned"], true);
    assert_eq!(assigned["changed"], true);
    let collection_ref = assigned["collection_ref"].as_str().unwrap().to_owned();
    let root_ref = assigned["bone_ref"].as_str().unwrap().to_owned();
    let unassigned = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.bone_collection.unassign",
        json!({"collection_ref":collection_ref,"bone_ref":root_ref}),
    )
    .await
    .unwrap();
    assert_eq!(unassigned["assigned"], false);
    let collection_ref = unassigned["collection_ref"].as_str().unwrap().to_owned();
    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.bone_collection.remove",
        json!({"ref":collection_ref}),
    )
    .await
    .unwrap();

    let rigs = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"objects","query":"SemwrightRig","limit":8}),
    )
    .await
    .unwrap();
    rig_ref = rigs["items"][0]["ref"].as_str().unwrap().to_owned();
    let pose = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":rig_ref,"property":"pose","limit":8}),
    )
    .await
    .unwrap();
    let pose_ref = pose["items"][0]["ref"].as_str().unwrap().to_owned();
    let pose_bones = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":pose_ref,"property":"bones","limit":16}),
    )
    .await
    .unwrap();
    let child_pose_ref = pose_bones["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "Child")
        .unwrap()["ref"]
        .as_str()
        .unwrap()
        .to_owned();
    let pose_constraint = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.pose_constraint.add",
        json!({"pose_bone_ref":child_pose_ref,"name":"SemwrightPoseLimit","type":"LIMIT_LOCATION"}),
    )
    .await
    .unwrap();
    let constraint_ref = pose_constraint["ref"].as_str().unwrap().to_owned();
    let constraint_value = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.property.set",
        json!({"ref":constraint_ref,"property":"influence","value":0.5}),
    )
    .await
    .unwrap();
    let constraint_ref = constraint_value["ref"].as_str().unwrap().to_owned();
    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.pose_constraint.remove",
        json!({"ref":constraint_ref}),
    )
    .await
    .unwrap();

    let rigs = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"objects","query":"SemwrightRig","limit":8}),
    )
    .await
    .unwrap();
    rig_ref = rigs["items"][0]["ref"].as_str().unwrap().to_owned();
    let rig_data = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":rig_ref,"property":"data","limit":8}),
    )
    .await
    .unwrap();
    let rig_data_ref = rig_data["items"][0]["ref"].as_str().unwrap().to_owned();
    let bones = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":rig_data_ref,"property":"bones","limit":16}),
    )
    .await
    .unwrap();
    let child_ref = bones["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "Child")
        .unwrap()["ref"]
        .as_str()
        .unwrap()
        .to_owned();
    let detached_bone = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.armature.bone.parent.set",
        json!({"ref":child_ref,"parent_ref":null,"connected":false}),
    )
    .await
    .unwrap();
    assert_eq!(detached_bone["parent_name"], Value::Null);

    let rigs = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"objects","query":"SemwrightRig","limit":8}),
    )
    .await
    .unwrap();
    rig_ref = rigs["items"][0]["ref"].as_str().unwrap().to_owned();
    let rig_data = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":rig_ref,"property":"data","limit":8}),
    )
    .await
    .unwrap();
    let rig_data_ref = rig_data["items"][0]["ref"].as_str().unwrap().to_owned();
    let bones = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":rig_data_ref,"property":"bones","limit":16}),
    )
    .await
    .unwrap();
    let root_ref = bones["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "Root")
        .unwrap()["ref"]
        .as_str()
        .unwrap()
        .to_owned();
    let child_ref = bones["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "Child")
        .unwrap()["ref"]
        .as_str()
        .unwrap()
        .to_owned();
    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.armature.bone.parent.set",
        json!({"ref":child_ref,"parent_ref":root_ref,"connected":true}),
    )
    .await
    .unwrap();

    let rigs = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"objects","query":"SemwrightRig","limit":8}),
    )
    .await
    .unwrap();
    rig_ref = rigs["items"][0]["ref"].as_str().unwrap().to_owned();
    let rig_data = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":rig_ref,"property":"data","limit":8}),
    )
    .await
    .unwrap();
    let rig_data_ref = rig_data["items"][0]["ref"].as_str().unwrap().to_owned();
    let bones = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":rig_data_ref,"property":"bones","limit":16}),
    )
    .await
    .unwrap();
    let child_ref = bones["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "Child")
        .unwrap()["ref"]
        .as_str()
        .unwrap()
        .to_owned();
    let removed_child = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.armature.bone.remove",
        json!({"ref":child_ref}),
    )
    .await
    .unwrap();
    rig_ref = removed_child["object_ref"].as_str().unwrap().to_owned();

    let rig_data = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":rig_ref,"property":"data","limit":8}),
    )
    .await
    .unwrap();
    let rig_data_ref = rig_data["items"][0]["ref"].as_str().unwrap().to_owned();
    let bones = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":rig_data_ref,"property":"bones","limit":16}),
    )
    .await
    .unwrap();
    let root_ref = bones["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "Root")
        .unwrap()["ref"]
        .as_str()
        .unwrap()
        .to_owned();
    let removed_root = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.armature.bone.remove",
        json!({"ref":root_ref}),
    )
    .await
    .unwrap();
    rig_ref = removed_root["object_ref"].as_str().unwrap().to_owned();
    let removed_rig = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.datablock.remove",
        json!({"ref":rig_ref}),
    )
    .await
    .unwrap();
    assert_eq!(removed_rig["root"], "objects");
    let armatures = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"armatures","query":"SemwrightRigData","limit":8}),
    )
    .await
    .unwrap();
    let armature_ref = armatures["items"][0]["ref"].as_str().unwrap().to_owned();
    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.datablock.remove",
        json!({"ref":armature_ref}),
    )
    .await
    .unwrap();

    let camera = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.datablock.create",
        json!({"root":"cameras","name":"SemwrightCameraData"}),
    )
    .await
    .unwrap();
    assert_eq!(camera["rna_type"], "Camera");
    let camera_created = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.object.create",
        json!({"name":"SemwrightCameraObject","data_ref":camera["ref"]}),
    )
    .await
    .unwrap();
    assert_eq!(camera_created["object_type"], "CAMERA");
    let camera_object_ref = camera_created["ref"].as_str().unwrap().to_owned();
    let camera_object = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.object.get",
        json!({"name":"SemwrightCameraObject"}),
    )
    .await
    .unwrap();
    assert_eq!(camera_object["object"]["type"], "CAMERA");

    let scenes = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"scenes","limit":8}),
    )
    .await
    .unwrap();
    let scene_ref = scenes["items"][0]["ref"].as_str().unwrap().to_owned();
    let selected_camera = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relation.set",
        json!({"ref":scene_ref,"property":"camera","target_ref":camera_object_ref}),
    )
    .await
    .unwrap();
    assert!(selected_camera["target_ref"].as_str().is_some());
    let scene_ref = selected_camera["ref"].as_str().unwrap().to_owned();
    let cleared_camera = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relation.set",
        json!({"ref":scene_ref,"property":"camera","target_ref":null}),
    )
    .await
    .unwrap();
    assert_eq!(cleared_camera["target_ref"], Value::Null);

    // Restore a real camera before later render acceptance. relation.set(null)
    // is still exercised above, but the render smoke must retain Blender's
    // documented active-camera precondition.
    let scenes = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"scenes","limit":8}),
    )
    .await
    .unwrap();
    let scene_ref = scenes["items"][0]["ref"].as_str().unwrap().to_owned();
    let camera_candidates = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.query",
        json!({"root":"objects","property":"type","operator":"eq","value":"CAMERA","limit":16}),
    )
    .await
    .unwrap();
    let fallback_camera_ref = camera_candidates["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"].as_str() != Some("SemwrightCameraObject"))
        .and_then(|item| item["ref"].as_str())
        .expect("factory scene should retain a fallback camera")
        .to_owned();
    let restored_camera = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relation.set",
        json!({"ref":scene_ref,"property":"camera","target_ref":fallback_camera_ref}),
    )
    .await
    .unwrap();
    assert!(restored_camera["target_ref"].as_str().is_some());

    let scenes = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"scenes","limit":8}),
    )
    .await
    .unwrap();
    let scene_ref = scenes["items"][0]["ref"].as_str().unwrap().to_owned();
    let view_layer = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.scene.view_layer.add",
        json!({"scene_ref":scene_ref,"name":"SemwrightView"}),
    )
    .await
    .unwrap();
    let view_layer_ref = view_layer["ref"].as_str().unwrap().to_owned();
    let moved_layer = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.scene.view_layer.move",
        json!({"ref":view_layer_ref,"to_index":0}),
    )
    .await
    .unwrap();
    let view_layer_ref = moved_layer["ref"].as_str().unwrap().to_owned();
    let removed_layer = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.scene.view_layer.remove",
        json!({"ref":view_layer_ref}),
    )
    .await
    .unwrap();
    let scene_ref = removed_layer["scene_ref"].as_str().unwrap().to_owned();
    let marker = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.scene.marker.add",
        json!({"scene_ref":scene_ref,"name":"SemwrightMarker","frame":12}),
    )
    .await
    .unwrap();
    assert_eq!(marker["frame"], 12);
    let marker_ref = marker["ref"].as_str().unwrap().to_owned();
    let removed_marker = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.scene.marker.remove",
        json!({"ref":marker_ref}),
    )
    .await
    .unwrap();
    let scene_ref = removed_marker["scene_ref"].as_str().unwrap().to_owned();
    let sequence = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.sequence.ensure",
        json!({"scene_ref":scene_ref}),
    )
    .await
    .unwrap();
    let sequence_ref = sequence["ref"].as_str().unwrap().to_owned();
    let sequence_description = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.object.describe",
        json!({"ref":sequence_ref}),
    )
    .await
    .unwrap();
    assert_eq!(sequence_description["rna_type"], "SequenceEditor");

    let scenes = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"scenes","limit":8}),
    )
    .await
    .unwrap();
    let scene_ref = scenes["items"][0]["ref"].as_str().unwrap().to_owned();
    let meta = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.sequence.meta.add",
        json!({"scene_ref":scene_ref,"name":"SemwrightMeta","channel":1,"frame_start":1}),
    )
    .await
    .unwrap();
    let meta_ref = meta["ref"].as_str().unwrap().to_owned();
    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.sequence.strip.remove",
        json!({"ref":meta_ref}),
    )
    .await
    .unwrap();

    let scenes = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"scenes","limit":8}),
    )
    .await
    .unwrap();
    let scene_ref = scenes["items"][0]["ref"].as_str().unwrap().to_owned();
    let effect = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.sequence.effect.add",
        json!({"scene_ref":scene_ref,"name":"SemwrightColor","type":"COLOR","channel":1,"frame_start":1,"frame_end":20}),
    )
    .await
    .unwrap();
    let effect_ref = effect["ref"].as_str().unwrap().to_owned();
    let strip_modifier = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.sequence.modifier.add",
        json!({"strip_ref":effect_ref,"name":"SemwrightContrast","type":"BRIGHT_CONTRAST"}),
    )
    .await
    .unwrap();
    let strip_modifier_ref = strip_modifier["ref"].as_str().unwrap().to_owned();
    let removed_strip_modifier = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.sequence.modifier.remove",
        json!({"ref":strip_modifier_ref}),
    )
    .await
    .unwrap();
    let effect_ref = removed_strip_modifier["strip_ref"]
        .as_str()
        .unwrap()
        .to_owned();
    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.sequence.strip.remove",
        json!({"ref":effect_ref}),
    )
    .await
    .unwrap();

    let camera_objects = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"objects","query":"SemwrightCameraObject","limit":8}),
    )
    .await
    .unwrap();
    let camera_object_ref = camera_objects["items"][0]["ref"]
        .as_str()
        .unwrap()
        .to_owned();
    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.datablock.remove",
        json!({"ref":camera_object_ref}),
    )
    .await
    .unwrap();
    let cameras = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"cameras","query":"SemwrightCameraData","limit":8}),
    )
    .await
    .unwrap();
    let camera_data_ref = cameras["items"][0]["ref"].as_str().unwrap().to_owned();
    let removed_camera = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.datablock.remove",
        json!({"ref":camera_data_ref}),
    )
    .await
    .unwrap();
    assert_eq!(removed_camera["changed"], true);

    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.collection.create",
        json!({"name":"SemwrightCollection"}),
    )
    .await
    .unwrap();
    let collections = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"collections","query":"SemwrightCollection","limit":8}),
    )
    .await
    .unwrap();
    let objects = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"objects","query":"SemwrightCube","limit":8}),
    )
    .await
    .unwrap();
    let collection_ref = collections["items"][0]["ref"].as_str().unwrap().to_owned();
    let cube_ref = objects["items"][0]["ref"].as_str().unwrap().to_owned();
    let linked = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relation.link",
        json!({"ref":collection_ref,"property":"objects","target_ref":cube_ref}),
    )
    .await
    .unwrap();
    let collection_ref = linked["ref"].as_str().unwrap().to_owned();
    let cube_ref = linked["target_ref"].as_str().unwrap().to_owned();
    let linked_object = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.object.get",
        json!({"name":"SemwrightCube"}),
    )
    .await
    .unwrap();
    assert!(
        linked_object["object"]["collections"]
            .as_array()
            .unwrap()
            .iter()
            .any(|name| name == "SemwrightCollection")
    );
    let unlinked = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relation.unlink",
        json!({"ref":collection_ref,"property":"objects","target_ref":cube_ref}),
    )
    .await
    .unwrap();
    let collection_ref = unlinked["ref"].as_str().unwrap().to_owned();
    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.datablock.remove",
        json!({"ref":collection_ref}),
    )
    .await
    .unwrap();

    let node_group = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.datablock.create",
        json!({"root":"node_groups","name":"SemwrightInterfaceGroup","kind":"ShaderNodeTree"}),
    )
    .await
    .unwrap();
    let tree_ref = node_group["ref"].as_str().unwrap().to_owned();
    let interface = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":tree_ref,"property":"interface","limit":8}),
    )
    .await
    .unwrap();
    let interface_ref = interface["items"][0]["ref"].as_str().unwrap().to_owned();
    let panel = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.node.interface.panel.add",
        json!({"interface_ref":interface_ref,"name":"SemwrightPanel","default_closed":false}),
    )
    .await
    .unwrap();
    assert_eq!(panel["item_type"], "PANEL");

    let groups = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"node_groups","query":"SemwrightInterfaceGroup","limit":8}),
    )
    .await
    .unwrap();
    let tree_ref = groups["items"][0]["ref"].as_str().unwrap().to_owned();
    let interface = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":tree_ref,"property":"interface","limit":8}),
    )
    .await
    .unwrap();
    let interface_ref = interface["items"][0]["ref"].as_str().unwrap().to_owned();
    let socket = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.node.interface.socket.add",
        json!({"interface_ref":interface_ref,"name":"SemwrightValue","in_out":"INPUT","socket_type":"NodeSocketFloat"}),
    )
    .await
    .unwrap();
    assert_eq!(socket["item_type"], "SOCKET");

    let groups = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"node_groups","query":"SemwrightInterfaceGroup","limit":8}),
    )
    .await
    .unwrap();
    let tree_ref = groups["items"][0]["ref"].as_str().unwrap().to_owned();
    let interface = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":tree_ref,"property":"interface","limit":8}),
    )
    .await
    .unwrap();
    let interface_ref = interface["items"][0]["ref"].as_str().unwrap().to_owned();
    let items = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":interface_ref,"property":"items_tree","limit":16}),
    )
    .await
    .unwrap();
    let panel_ref = items["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "SemwrightPanel")
        .unwrap()["ref"]
        .as_str()
        .unwrap()
        .to_owned();
    let socket_ref = items["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "SemwrightValue")
        .unwrap()["ref"]
        .as_str()
        .unwrap()
        .to_owned();
    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.node.interface.item.move_to_parent",
        json!({"ref":socket_ref,"parent_ref":panel_ref,"to_position":0}),
    )
    .await
    .unwrap();

    let groups = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"node_groups","query":"SemwrightInterfaceGroup","limit":8}),
    )
    .await
    .unwrap();
    let tree_ref = groups["items"][0]["ref"].as_str().unwrap().to_owned();
    let interface = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":tree_ref,"property":"interface","limit":8}),
    )
    .await
    .unwrap();
    let interface_ref = interface["items"][0]["ref"].as_str().unwrap().to_owned();
    let items = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":interface_ref,"property":"items_tree","limit":16}),
    )
    .await
    .unwrap();
    let socket_ref = items["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "SemwrightValue")
        .unwrap()["ref"]
        .as_str()
        .unwrap()
        .to_owned();
    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.node.interface.item.remove",
        json!({"ref":socket_ref}),
    )
    .await
    .unwrap();

    let groups = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"node_groups","query":"SemwrightInterfaceGroup","limit":8}),
    )
    .await
    .unwrap();
    let tree_ref = groups["items"][0]["ref"].as_str().unwrap().to_owned();
    let interface = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":tree_ref,"property":"interface","limit":8}),
    )
    .await
    .unwrap();
    let interface_ref = interface["items"][0]["ref"].as_str().unwrap().to_owned();
    let items = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":interface_ref,"property":"items_tree","limit":16}),
    )
    .await
    .unwrap();
    let panel_ref = items["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "SemwrightPanel")
        .unwrap()["ref"]
        .as_str()
        .unwrap()
        .to_owned();
    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.node.interface.item.remove",
        json!({"ref":panel_ref}),
    )
    .await
    .unwrap();
    let groups = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"node_groups","query":"SemwrightInterfaceGroup","limit":8}),
    )
    .await
    .unwrap();
    let tree_ref = groups["items"][0]["ref"].as_str().unwrap().to_owned();
    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.datablock.remove",
        json!({"ref":tree_ref}),
    )
    .await
    .unwrap();

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

    let materials = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"materials","query":"SemwrightMaterial","limit":8}),
    )
    .await
    .unwrap();
    let material_ref = materials["items"][0]["ref"].as_str().unwrap().to_owned();
    let tree = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":material_ref,"property":"node_tree","limit":8}),
    )
    .await
    .unwrap();
    let tree_ref = tree["items"][0]["ref"].as_str().unwrap().to_owned();
    let node_types = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.node.types",
        json!({"query":"ShaderNodeValue","limit":16}),
    )
    .await
    .unwrap();
    assert!(
        node_types["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["id"] == "ShaderNodeValue" && item["status"] == "managed")
    );
    let node = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.node.add",
        json!({"tree_ref":tree_ref,"type":"ShaderNodeValue","name":"SemwrightValue"}),
    )
    .await
    .unwrap();
    assert_eq!(node["node_type"], "ShaderNodeValue");

    let materials = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"materials","query":"SemwrightMaterial","limit":8}),
    )
    .await
    .unwrap();
    let material_ref = materials["items"][0]["ref"].as_str().unwrap().to_owned();
    let tree = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":material_ref,"property":"node_tree","limit":8}),
    )
    .await
    .unwrap();
    let tree_ref = tree["items"][0]["ref"].as_str().unwrap().to_owned();
    let nodes = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":tree_ref,"property":"nodes","limit":64}),
    )
    .await
    .unwrap();
    let value_node_ref = nodes["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "SemwrightValue")
        .unwrap()["ref"]
        .as_str()
        .unwrap()
        .to_owned();
    let principled_ref = nodes["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "Principled BSDF")
        .unwrap()["ref"]
        .as_str()
        .unwrap()
        .to_owned();
    let outputs = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":value_node_ref,"property":"outputs","limit":32}),
    )
    .await
    .unwrap();
    let value_output = outputs["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "Value")
        .unwrap()["ref"]
        .as_str()
        .unwrap()
        .to_owned();
    let inputs = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":principled_ref,"property":"inputs","limit":64}),
    )
    .await
    .unwrap();
    let roughness_input = inputs["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "Roughness")
        .unwrap()["ref"]
        .as_str()
        .unwrap()
        .to_owned();
    let link = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.node.link",
        json!({"tree_ref":tree_ref,"from_socket_ref":value_output,"to_socket_ref":roughness_input}),
    )
    .await
    .unwrap();
    let link_ref = link["ref"].as_str().unwrap().to_owned();
    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.node.unlink",
        json!({"ref":link_ref}),
    )
    .await
    .unwrap();

    let materials = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"materials","query":"SemwrightMaterial","limit":8}),
    )
    .await
    .unwrap();
    let material_ref = materials["items"][0]["ref"].as_str().unwrap().to_owned();
    let tree = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":material_ref,"property":"node_tree","limit":8}),
    )
    .await
    .unwrap();
    let tree_ref = tree["items"][0]["ref"].as_str().unwrap().to_owned();
    let nodes = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":tree_ref,"property":"nodes","limit":64}),
    )
    .await
    .unwrap();
    let node_ref = nodes["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "SemwrightValue")
        .unwrap()["ref"]
        .as_str()
        .unwrap()
        .to_owned();
    let removed_node = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.node.remove",
        json!({"ref":node_ref}),
    )
    .await
    .unwrap();
    assert_eq!(removed_node["changed"], true);

    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.collection.create",
        json!({"name":"SemwrightExport"}),
    )
    .await
    .unwrap();
    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.collection.link",
        json!({"object":"SemwrightCube","collection":"SemwrightExport"}),
    )
    .await
    .unwrap();
    let exported = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.export.glb",
        json!({"collection":"SemwrightExport","path":"semwright-cube.glb","animations":false}),
    )
    .await
    .unwrap();
    assert_eq!(exported["changed"], true);
    assert_eq!(exported["format"], "glb");
    assert_eq!(exported["objects"], 1);
    assert_eq!(
        exported["sha256"],
        digest(&workspace_path.join("semwright-cube.glb"))
    );
    let second_export = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.export.glb",
        json!({"collection":"SemwrightExport","path":"semwright-cube.glb","animations":false}),
    )
    .await
    .unwrap_err();
    assert_eq!(second_export.code, semwright_types::ErrorCode::Conflict);

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

    let scenes = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"scenes","limit":8}),
    )
    .await
    .unwrap();
    let scene_ref = scenes["items"][0]["ref"].as_str().unwrap().to_owned();
    let media_strip = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.sequence.media.add",
        json!({"scene_ref":scene_ref,"kind":"IMAGE","name":"SemwrightPreviewStrip","path":"preview.png","channel":5,"frame_start":1,"fit_method":"ORIGINAL"}),
    )
    .await
    .unwrap();
    assert_eq!(media_strip["kind"], "IMAGE");
    assert_eq!(media_strip["path"], "preview.png");
    let media_ref = media_strip["ref"].as_str().unwrap().to_owned();
    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.sequence.strip.remove",
        json!({"ref":media_ref}),
    )
    .await
    .unwrap();

    let secondary_scene = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.datablock.create",
        json!({"root":"scenes","name":"SemwrightSequenceScene"}),
    )
    .await
    .unwrap();
    let secondary_scene_ref = secondary_scene["ref"].as_str().unwrap().to_owned();
    let scenes = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"scenes","limit":16}),
    )
    .await
    .unwrap();
    let scene_ref = scenes["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"].as_str() != Some("SemwrightSequenceScene"))
        .and_then(|item| item["ref"].as_str())
        .unwrap()
        .to_owned();
    let secondary_scene_ref = scenes["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "SemwrightSequenceScene")
        .and_then(|item| item["ref"].as_str())
        .unwrap_or(&secondary_scene_ref)
        .to_owned();
    let scene_strip = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.sequence.datablock.add",
        json!({"scene_ref":scene_ref,"kind":"SCENE","name":"SemwrightSceneStrip","source_ref":secondary_scene_ref,"channel":6,"frame_start":1}),
    )
    .await
    .unwrap();
    assert_eq!(scene_strip["kind"], "SCENE");
    let scene_strip_ref = scene_strip["ref"].as_str().unwrap().to_owned();
    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.sequence.strip.remove",
        json!({"ref":scene_strip_ref}),
    )
    .await
    .unwrap();
    let scenes = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"scenes","query":"SemwrightSequenceScene","limit":8}),
    )
    .await
    .unwrap();
    let secondary_scene_ref = scenes["items"][0]["ref"].as_str().unwrap().to_owned();
    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.datablock.remove",
        json!({"ref":secondary_scene_ref}),
    )
    .await
    .unwrap();

    let loaded_image = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.asset.load",
        json!({"root":"images","path":"preview.png","name":"SemwrightPreviewImage"}),
    )
    .await
    .unwrap();
    assert_eq!(loaded_image["root"], "images");
    assert!(loaded_image["bytes"].as_u64().unwrap() > 8);
    let image_ref = loaded_image["ref"].as_str().unwrap().to_owned();
    let image_description = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.object.describe",
        json!({"ref":image_ref}),
    )
    .await
    .unwrap();
    assert_eq!(image_description["rna_type"], "Image");
    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.datablock.remove",
        json!({"ref":image_ref}),
    )
    .await
    .unwrap();

    let clip = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.asset.load",
        json!({"root":"movieclips","path":"preview.png","name":"SemwrightPreviewClip"}),
    )
    .await
    .unwrap();
    assert_eq!(clip["root"], "movieclips");
    let clip_ref = clip["ref"].as_str().unwrap().to_owned();
    let tracking = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":clip_ref,"property":"tracking","limit":8}),
    )
    .await
    .unwrap();
    let tracking_ref = tracking["items"][0]["ref"].as_str().unwrap().to_owned();
    let tracking_object = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.tracking.object.add",
        json!({"tracking_ref":tracking_ref,"name":"SemwrightTrackingObject"}),
    )
    .await
    .unwrap();
    let tracking_object_ref = tracking_object["ref"].as_str().unwrap().to_owned();
    let track = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.tracking.track.add",
        json!({"object_ref":tracking_object_ref,"name":"SemwrightTrack","frame":1,"co":[0.25,0.5]}),
    )
    .await
    .unwrap();
    assert_eq!(track["frame"], 1);
    let track_ref = track["ref"].as_str().unwrap().to_owned();
    let marker = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.tracking.marker.add",
        json!({"track_ref":track_ref,"frame":2,"co":[0.3,0.55]}),
    )
    .await
    .unwrap();
    let marker_ref = marker["ref"].as_str().unwrap().to_owned();
    let marker_position = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.property.get",
        json!({"ref":marker_ref,"property":"co"}),
    )
    .await
    .unwrap();
    let marker_co = marker_position["value"].as_array().unwrap();
    assert!((marker_co[0].as_f64().unwrap() - 0.3).abs() < 1e-5);
    assert!((marker_co[1].as_f64().unwrap() - 0.55).abs() < 1e-5);
    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.tracking.marker.remove",
        json!({"ref":marker_ref}),
    )
    .await
    .unwrap();

    let clips = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"movieclips","query":"SemwrightPreviewClip","limit":8}),
    )
    .await
    .unwrap();
    let clip_ref = clips["items"][0]["ref"].as_str().unwrap().to_owned();
    let tracking = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":clip_ref,"property":"tracking","limit":8}),
    )
    .await
    .unwrap();
    let tracking_ref = tracking["items"][0]["ref"].as_str().unwrap().to_owned();
    let tracking_objects = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relations",
        json!({"ref":tracking_ref,"property":"objects","limit":16}),
    )
    .await
    .unwrap();
    let tracking_object_ref = tracking_objects["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "SemwrightTrackingObject")
        .unwrap()["ref"]
        .as_str()
        .unwrap()
        .to_owned();
    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.tracking.object.remove",
        json!({"ref":tracking_object_ref}),
    )
    .await
    .unwrap();
    let clips = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"movieclips","query":"SemwrightPreviewClip","limit":8}),
    )
    .await
    .unwrap();
    let clip_ref = clips["items"][0]["ref"].as_str().unwrap().to_owned();
    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.datablock.remove",
        json!({"ref":clip_ref}),
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
