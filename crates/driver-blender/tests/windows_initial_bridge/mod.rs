//! Physical, owner-authorized diagnostic. This does not enable a production grant.
use super::*;
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::PathBuf};
const FRAME: usize = 256 * 1024;
const EXE_SHA: &str = "284f4041f98e113f3dc10654a7193ffaaa9bfdfec8b87fa116620a48b5f6d4cb";

fn mount_arg(mount: &str, relative: &str) -> String {
    encode_host_tool_arg_ref(&HostToolArgRef::MountPath {
        mount: mount.into(),
        relative: relative.into(),
    })
    .unwrap()
}
fn io_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

async fn exchange<I, O>(
    input: &mut I,
    output: &mut O,
    command: &str,
    args: Value,
) -> Result<Value, String>
where
    I: tokio::io::AsyncWrite + Unpin,
    O: tokio::io::AsyncRead + Unpin,
{
    let bytes = serde_json::to_vec(&json!({"command":command,"args":args})).map_err(io_error)?;
    if bytes.is_empty() || bytes.len() > FRAME {
        return Err("request bounds".into());
    }
    input
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .await
        .map_err(io_error)?;
    input.write_all(&bytes).await.map_err(io_error)?;
    input.flush().await.map_err(io_error)?;
    let length = output.read_u32().await.map_err(io_error)? as usize;
    if length == 0 || length > FRAME {
        return Err(format!("response bounds: {length}"));
    }
    let mut payload = vec![0; length];
    output.read_exact(&mut payload).await.map_err(io_error)?;
    let value: Value = serde_json::from_slice(&payload).map_err(io_error)?;
    if value["ok"] != true || !value["data"].is_object() {
        return Err(format!("native command {command}: {value}"));
    }
    Ok(value["data"].clone())
}
async fn exercise<I, O>(
    input: &mut I,
    output: &mut O,
    nonce: &str,
    trace: &mut Vec<Value>,
) -> Result<(), String>
where
    I: tokio::io::AsyncWrite + Unpin,
    O: tokio::io::AsyncRead + Unpin,
{
    let marker = format!("\0SEMWRIGHT-BLENDER-STDIO-v1:{nonce}\0").into_bytes();
    let mut prefix = Vec::new();
    let mut ready = false;
    for _ in 0..65_536 + marker.len() {
        let byte = output
            .read_u8()
            .await
            .map_err(|e| format!("startup: {e}; prefix={}", String::from_utf8_lossy(&prefix)))?;
        prefix.push(byte);
        if prefix.ends_with(&marker) {
            ready = true;
            break;
        }
    }
    if !ready {
        return Err("startup marker absent or over budget".into());
    }
    trace.push(
        json!({"phase":"nonce_bound_startup_marker","prefix_bytes":prefix.len(),"nonce":nonce}),
    );
    let summary = exchange(
        input,
        output,
        "driver.blender.introspect.summary",
        json!({}),
    )
    .await?;
    if summary["version"] != json!([5, 2, 1]) || summary["arbitrary_python"] != false {
        return Err(format!("unexpected summary: {summary}"));
    }
    trace.push(json!({"phase":"summary","data":summary}));
    let operations = [
        (
            "driver.blender.collection.create",
            json!({"name":"IsolatedProof"}),
        ),
        (
            "driver.blender.object.create",
            json!({"name":"IsolatedProofCube","primitive":"cube","location":[0,0,0]}),
        ),
        (
            "driver.blender.collection.link",
            json!({"object":"IsolatedProofCube","collection":"IsolatedProof"}),
        ),
        (
            "driver.blender.object.transform",
            json!({"name":"IsolatedProofCube","location":[3,4,5],"scale":[1,2,1]}),
        ),
        (
            "driver.blender.object.get",
            json!({"name":"IsolatedProofCube"}),
        ),
        (
            "driver.blender.file.save",
            json!({"path":"isolated-proof.blend"}),
        ),
        (
            "driver.blender.export.glb",
            json!({"collection":"IsolatedProof","path":"isolated-proof.glb","animations":false}),
        ),
    ];
    for (command, args) in operations {
        eprintln!("INITIAL_BRIDGE_PHASE {command}");
        let data = exchange(input, output, command, args.clone()).await?;
        if command == "driver.blender.object.get"
            && (data["object"]["location"] != json!([3.0, 4.0, 5.0])
                || data["object"]["scale"] != json!([1.0, 2.0, 1.0]))
        {
            return Err(format!("readback mismatch: {data}"));
        }
        trace.push(json!({"command":command,"args":args,"data":data}));
    }
    Ok(())
}

#[tokio::test]
#[ignore = "Requires explicit owner-authorized registryRead diagnostic and pinned Blender 5.2.1"]
async fn registry_read_initial_bridge_typed_edit_export() {
    let destination = PathBuf::from(
        std::env::var_os("SEMWRIGHT_TEST_RECEIPTS").expect("explicit receipt directory"),
    );
    assert!(destination.is_absolute() && destination.is_dir());
    let workspace = destination.join("workspace");
    let code = destination.join("sealed-code");
    for path in [&workspace, &code] {
        fs::create_dir(path).unwrap();
        harden_cpu_fixture_root(path);
    }
    fs::create_dir(code.join("semwright_blender_runtime")).unwrap();
    let sources = [
        ("bootstrap.py", include_str!("bootstrap.py")),
        ("bridge.py", include_str!("../../src/bridge.py")),
        ("semwright_blender_runtime/__init__.py", ""),
        (
            "semwright_blender_runtime/commands.py",
            include_str!("../../../../adapters/blender/semwright_blender/commands.py"),
        ),
        (
            "semwright_blender_runtime/commands.json",
            include_str!("../../../../adapters/blender/semwright_blender/commands.json"),
        ),
        (
            "semwright_blender_runtime/validation.py",
            include_str!("../../../../adapters/blender/semwright_blender/validation.py"),
        ),
        (
            "semwright_blender_runtime/semantic.py",
            include_str!("../../src/semantic.py"),
        ),
        (
            "semwright_blender_runtime/authoring_native.py",
            include_str!("../../src/authoring_native.py"),
        ),
        (
            "semwright_blender_runtime/export_scope.py",
            include_str!("../../../../adapters/blender/semwright_blender/export_scope.py"),
        ),
    ];
    let mut pins = BTreeMap::new();
    for (relative, source) in sources {
        fs::write(code.join(relative), source).unwrap();
        pins.insert(relative, hex::encode(Sha256::digest(source.as_bytes())));
    }
    fs::write(
        destination.join("sealed-code-pins.json"),
        serde_json::to_vec_pretty(&pins).unwrap(),
    )
    .unwrap();
    let runtime = PathBuf::from(
        std::env::var_os("SEMWRIGHT_TEST_BLENDER_RUNTIME").expect("explicit runtime"),
    );
    let executable = runtime.join("blender.exe");
    semwright_platform_services::verify_sealed_tool_executable(&executable, EXE_SHA)
        .expect("owner pinned image");
    let nonce = semwright_types::unique_id().replace('-', "");
    assert_eq!(nonce.len(), 32);
    let spec = SandboxSpec {
        kind: SandboxKind::Driver,
        staged_executable: executable,
        helper: std::env::current_exe().unwrap(),
        mounts: vec![
            Mount {
                source: workspace.clone(),
                class: MountClass::Workspace,
                logical_name: "work".into(),
                read_only: false,
                execute: false,
            },
            Mount {
                source: code.clone(),
                class: MountClass::Workspace,
                logical_name: "code".into(),
                read_only: true,
                execute: false,
            },
            Mount {
                source: runtime.clone(),
                class: MountClass::Workspace,
                logical_name: "runtime".into(),
                read_only: true,
                execute: true,
            },
        ],
        args: vec![
            "--background".into(),
            "--factory-startup".into(),
            "--disable-autoexec".into(),
            "--offline-mode".into(),
            "-q".into(),
            "--python-exit-code".into(),
            "1".into(),
            "--python".into(),
            "bootstrap.py".into(),
            "--".into(),
            format!("stdio:{nonce}"),
            mount_arg("work", ""),
            mount_arg("code", ""),
            mount_arg("runtime", ""),
        ],
        environment: vec![
            (SANDBOX_HOST_TOOL_CHILD_ENV.into(), "1".into()),
            (SANDBOX_HOST_TOOL_TYPED_ARGS_ENV.into(), "1".into()),
            (
                SANDBOX_HOST_TOOL_CWD_ENV.into(),
                code.to_string_lossy().into_owned(),
            ),
        ],
        sealed_tools: vec![],
        network: false,
        nvidia_gpu: false,
        limits: Some(ResourceLimits {
            open_files: 256,
            processes: 1,
            cpu_seconds: 60,
            address_space_bytes: 4 * 1024 * 1024 * 1024,
            file_size_bytes: 16 * 1024 * 1024,
        }),
    };
    let mut process = semwright_platform_services::sandbox_spawn_registry_read_diagnostic(&spec)
        .expect("isolated initial Blender");
    let mut input = process.take_stdin().unwrap();
    let mut output = process.take_stdout().unwrap();
    let mut trace = Vec::new();
    let result = tokio::time::timeout(
        Duration::from_secs(60),
        exercise(&mut input, &mut output, &nonce, &mut trace),
    )
    .await;
    let _ = input.shutdown().await;
    drop(input);
    let mut tail = Vec::new();
    let drain = tokio::time::timeout(
        Duration::from_secs(5),
        (&mut output).take(8193).read_to_end(&mut tail),
    )
    .await;
    let waited = tokio::time::timeout(Duration::from_secs(5), process.wait_exit_code()).await;
    let exit = waited
        .as_ref()
        .ok()
        .and_then(|r| r.as_ref().ok())
        .copied()
        .flatten();
    if exit.is_none() {
        kill_and_reap_owned(&mut process).await;
    }
    let source_unchanged = pins.iter().all(|(path, sha)| {
        fs::read(code.join(path)).is_ok_and(|data| hex::encode(Sha256::digest(data)) == *sha)
    });
    let runtime_unchanged = semwright_platform_services::verify_sealed_tool_executable(
        &spec.staged_executable,
        EXE_SHA,
    )
    .is_ok();
    let outcome = match result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(e),
        Err(e) => Err(e.to_string()),
    };
    let receipt = json!({"probe":"initial_blender_fixed_bridge","result":outcome,"exit":exit,"trace":trace,
        "sealed_code_unchanged":source_unchanged,"runtime_executable_unchanged":runtime_unchanged,
        "stdout_drain_complete":matches!(drain,Ok(Ok(_))) && tail.len()<=8192,
        "stdout_tail":String::from_utf8_lossy(&tail),"network":false,"process_limit":1,
        "registry_read_diagnostic":true,"broker_used":false,"A05":"BLOCKED","nonce":nonce});
    fs::write(
        destination.join("bridge-receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    eprintln!("INITIAL_BRIDGE_RECEIPT {receipt}");
    assert!(source_unchanged && runtime_unchanged);
    assert!(outcome.is_ok(), "native bridge: {outcome:?}");
    assert_eq!(exit, Some(0));
    assert!(matches!(drain, Ok(Ok(_))) && tail.len() <= 8192);
    let glb = fs::read(workspace.join("isolated-proof.glb")).unwrap();
    assert!(glb.len() >= 20 && glb.len() <= 16 * 1024 * 1024);
    assert_eq!(&glb[..4], b"glTF");
    assert_eq!(u32::from_le_bytes(glb[4..8].try_into().unwrap()), 2);
    assert_eq!(
        u32::from_le_bytes(glb[8..12].try_into().unwrap()) as usize,
        glb.len()
    );
    let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    assert_eq!(&glb[16..20], b"JSON");
    assert!(20 + json_len <= glb.len());
    let document: Value = serde_json::from_slice(&glb[20..20 + json_len]).unwrap();
    assert!(
        document["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["name"] == "IsolatedProofCube")
    );
    assert!(!document["meshes"].as_array().unwrap().is_empty());
    let node = document["nodes"].as_array().unwrap().iter()
        .find(|n| n["name"] == "IsolatedProofCube").unwrap();
    for (field, expected) in [("translation",[3.0,5.0,-4.0]),("scale",[1.0,1.0,2.0])] {
        let actual = node[field].as_array().expect("explicit glTF transform");
        assert_eq!(actual.len(),3);
        for (value,want) in actual.iter().zip(expected) {
            assert!((value.as_f64().unwrap()-want).abs()<0.00001,"GLB transform readback");
        }
    }
    for name in ["buffers","images"] {
        if let Some(items)=document[name].as_array() {
            assert!(items.iter().all(|item| item.get("uri").is_none()),"GLB must be self-contained");
        }
    }
    eprintln!(
        "INITIAL_BRIDGE_GLB {}",
        json!({"bytes":glb.len(),"sha256":hex::encode(Sha256::digest(&glb)),
        "node_count":document["nodes"].as_array().unwrap().len(),"mesh_count":document["meshes"].as_array().unwrap().len(),
        "broker_used":false,"Godot_import":"NOT_RUN","A05":"BLOCKED"})
    );
}
