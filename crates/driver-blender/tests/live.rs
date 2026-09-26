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
        secrets: vec![],
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
    assert!(capabilities.len() >= 64);
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
    let unkeyed = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.animation.keyframe.delete",
        json!({"ref":authored_object_ref,"property":"location","frame":1.0,"index":0}),
    )
    .await
    .unwrap();
    assert_eq!(unkeyed["changed"], true);
    authored_object_ref = unkeyed["ref"].as_str().unwrap().to_owned();

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
    let replaced = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.mesh.geometry.replace",
        json!({
            "mesh_ref":mesh_ref,
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
    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.object.create",
        json!({"name":"SemwrightRig","primitive":"empty"}),
    )
    .await
    .unwrap();
    let rig_objects = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"objects","query":"SemwrightRig","limit":8}),
    )
    .await
    .unwrap();
    let armatures = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"armatures","query":"SemwrightRigData","limit":8}),
    )
    .await
    .unwrap();
    let rig_ref = rig_objects["items"][0]["ref"].as_str().unwrap().to_owned();
    let armature_ref = armatures["items"][0]["ref"].as_str().unwrap().to_owned();
    let rig_attached = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relation.set",
        json!({"ref":rig_ref,"property":"data","target_ref":armature_ref}),
    )
    .await
    .unwrap();
    let mut rig_ref = rig_attached["ref"].as_str().unwrap().to_owned();
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
    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relation.set",
        json!({"ref":rig_ref,"property":"data","target_ref":null}),
    )
    .await
    .unwrap();
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
    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.object.delete",
        json!({"name":"SemwrightRig"}),
    )
    .await
    .unwrap();
    let _ = armature;

    let camera = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.datablock.create",
        json!({"root":"cameras","name":"SemwrightCameraData"}),
    )
    .await
    .unwrap();
    assert_eq!(camera["rna_type"], "Camera");
    call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.object.create",
        json!({"name":"SemwrightCameraObject","primitive":"empty"}),
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
    let camera_data = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.objects",
        json!({"root":"cameras","query":"SemwrightCameraData","limit":8}),
    )
    .await
    .unwrap();
    let camera_object_ref = camera_objects["items"][0]["ref"]
        .as_str()
        .unwrap()
        .to_owned();
    let camera_data_ref = camera_data["items"][0]["ref"].as_str().unwrap().to_owned();
    let attached = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relation.set",
        json!({"ref":camera_object_ref,"property":"data","target_ref":camera_data_ref}),
    )
    .await
    .unwrap();
    let camera_object_ref = attached["ref"].as_str().unwrap().to_owned();
    let camera_data_ref = attached["target_ref"].as_str().unwrap().to_owned();
    let camera_object = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.object.get",
        json!({"name":"SemwrightCameraObject"}),
    )
    .await
    .unwrap();
    assert_eq!(camera_object["object"]["type"], "CAMERA");
    let detached = call(
        provider.as_ref(),
        &capabilities,
        "driver.blender.semantic.relation.set",
        json!({"ref":camera_object_ref,"property":"data","target_ref":null}),
    )
    .await
    .unwrap();
    assert_eq!(detached["target_ref"], Value::Null);
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
