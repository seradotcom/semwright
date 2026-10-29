//! Physical handoff of pinned Blender GLBs through the first-party Godot broker.
//! The Godot editor is an owner process for a trusted project, not a confined native runtime.
#![cfg(target_os = "windows")]
use semwright_protocol::Client;
use semwright_types::{ErrorCode, ExecuteRequest, unique_id};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
type ProofResult<T> = Result<T, String>;
fn check(value: bool, message: &str) -> ProofResult<()> {
    if value {
        Ok(())
    } else {
        Err(message.to_owned())
    }
}
fn digest(path: &Path) -> ProofResult<String> {
    Ok(hex::encode(Sha256::digest(
        fs::read(path).map_err(|e| e.to_string())?,
    )))
}
fn save(path: &Path, value: &Value) -> ProofResult<()> {
    let mut raw = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    raw.push(b'\n');
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    f.write_all(&raw)
        .and_then(|_| f.sync_all())
        .map_err(|e| e.to_string())
}
struct Proof {
    client: Client,
    dir: PathBuf,
    trace: Vec<Value>,
    session: String,
}
impl Proof {
    async fn call(
        &mut self,
        label: &str,
        op: &str,
        args: Value,
        expected: Option<ErrorCode>,
    ) -> ProofResult<Value> {
        let id = unique_id();
        let command = format!("driver.godot.{op}");
        let prefix = format!("{:02}-{label}", self.trace.len());
        save(
            &self.dir.join(format!("{prefix}.request.json")),
            &json!({"request_id":id,"command":command,"args":args,"dry_run":false,"backend":null}),
        )?;
        let index = self.trace.len();
        self.trace
            .push(json!({"label":label,"request_id":id,"command":command,"outcome":"PENDING"}));
        let started = Instant::now();
        let result = tokio::time::timeout(
            Duration::from_secs(25),
            self.client.execute(
                id.clone(),
                ExecuteRequest {
                    command,
                    args,
                    dry_run: false,
                    backend: None,
                },
            ),
        )
        .await;
        let envelope = match result {
            Ok(Ok(e)) => e,
            Ok(Err(_)) => {
                self.trace[index]["outcome"] = json!("TRANSPORT_ERROR_UNKNOWN");
                return Err(format!("{label}: transport error; no replay"));
            }
            Err(_) => {
                self.trace[index]["outcome"] = json!("TIMEOUT_UNKNOWN");
                return Err(format!("{label}: deadline; no replay"));
            }
        };
        let raw = serde_json::to_value(&envelope).map_err(|e| e.to_string())?;
        save(&self.dir.join(format!("{prefix}.envelope.json")), &raw)?;
        self.trace[index]["outcome"] = json!("RESPONSE_RECEIVED");
        self.trace[index]["elapsed_ms"] = json!(started.elapsed().as_millis());
        self.trace[index]["ok"] = json!(envelope.ok);
        self.trace[index]["error"] = raw.get("error").cloned().unwrap_or(Value::Null);
        check(envelope.request_id == id, "Request identity mismatch")?;
        check(
            raw["execution"]["backend"] == "driver:godot"
                && raw["execution"]["policy_decision"] == "allow"
                && raw["execution"]["provenance"]["source"] == "driver",
            "Unqualified Godot provider route",
        )?;
        match expected {
            Some(code) => check(
                !envelope.ok && envelope.error.as_ref().map(|e| &e.code) == Some(&code),
                "Unexpected rejection",
            )?,
            None => check(
                envelope.ok && envelope.error.is_none(),
                &format!("{label}: broker rejected operation"),
            )?,
        }
        eprintln!(
            "GODOT_BROKER_STEP {}",
            json!({"label":label,"request_id":id,"ok":envelope.ok})
        );
        Ok(envelope.data.unwrap_or(Value::Null))
    }
    async fn read(&mut self, label: &str, op: &str, mut args: Value) -> ProofResult<Value> {
        args["session"] = json!(self.session);
        self.call(label, op, args, None).await
    }
    async fn mutate(
        &mut self,
        label: &str,
        op: &str,
        mut args: Value,
        stamp: &Value,
    ) -> ProofResult<Value> {
        args["session"] = json!(self.session);
        args["expect"] = stamp.clone();
        args["dry_run"] = json!(false);
        let result = self.call(label, op, args, None).await?;
        check(result["applied"] == true, "Mutation did not report applied")?;
        Ok(result)
    }
    async fn idle(&mut self, label: &str) -> ProofResult<Value> {
        for n in 0..30 {
            let v = self
                .read(&format!("{label}-{n:02}"), "assets.status", json!({}))
                .await?;
            if v["data"]["scanning"] == false && v["data"]["importing"] == false {
                return Ok(v);
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        Err("Godot resource filesystem did not become idle".into())
    }
    async fn edited_scene(&mut self, path: &str, label: &str) -> ProofResult<Value> {
        for n in 0..30 {
            let v = self
                .read(&format!("{label}-{n:02}"), "project.inspect", json!({}))
                .await?;
            if v["data"]["scene"] == path {
                return Ok(v);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err("Edited scene did not match requested supporting scene".into())
    }
}
fn vec3(v: &Value) -> ProofResult<[f64; 3]> {
    let a = v.as_array().ok_or("Expected vector")?;
    check(a.len() == 3, "Vector length")?;
    let mut out = [0.0; 3];
    for i in 0..3 {
        out[i] = a[i].as_f64().ok_or("Expected finite scalar")?;
        check(out[i].is_finite(), "Non-finite bound")?;
    }
    Ok(out)
}
fn close_vec(a: &Value, b: &Value) -> ProofResult<bool> {
    let a = vec3(a)?;
    let b = vec3(b)?;
    Ok((0..3).all(|i| (a[i] - b[i]).abs() < 0.0002))
}
async fn exercise(p: &mut Proof, cfg: &Value) -> ProofResult<Value> {
    let root = PathBuf::from(cfg["project_root"].as_str().ok_or("Project root")?);
    let scene = cfg["supporting_scene"].as_str().ok_or("Supporting scene")?;
    check(
        scene.starts_with("res://proofs/") && scene.ends_with(".tscn") && !scene.contains(".."),
        "Supporting path scope",
    )?;
    let scene_file = root.join(scene.trim_start_matches("res://"));
    check(!scene_file.exists(), "Preserve prior supporting scene")?;
    let assets = cfg["assets"].as_array().ok_or("Assets")?;
    check(assets.len() == 2, "Expected two original Blender artifacts")?;
    for a in assets {
        let rel = a["path"].as_str().ok_or("Asset path")?;
        check(
            rel.starts_with("res://assets/") && !rel.contains("..") && rel.ends_with(".glb"),
            "Asset scope",
        )?;
        check(
            digest(&root.join(rel.trim_start_matches("res://")))? == a["sha256"],
            "Source asset digest",
        )?;
    }
    let doctor = p.call("doctor", "doctor", json!({}), None).await?;
    check(
        doctor["certification"] == "integration-under-test",
        "Unexpected platform claim",
    )?;
    let sessions = p.call("sessions", "session.list", json!({}), None).await?;
    let all = sessions["sessions"].as_array().ok_or("Sessions")?;
    check(
        all.len() == 1 && all[0]["project"] == cfg["project_id"],
        "Expected exactly the owned paired project",
    )?;
    check(
        all[0]["engine_version"] == "4.7.2-stable (official)",
        "Godot version",
    )?;
    p.session = all[0]["session"].as_str().ok_or("Session identity")?.into();
    let before = p
        .read("project-before", "project.inspect", json!({}))
        .await?;
    let original_editor = p.read("editor-before", "editor.state", json!({})).await?;
    let original_nodes = p
        .read("original-nodes-before", "scene.inspect", json!({}))
        .await?;
    check(
        original_editor["data"]["unsaved_scenes"] == cfg["initial_unsaved_scenes"]
            && original_nodes["data"]["nodes"] == cfg["initial_scene_nodes"]
            && before["data"]["scene"] == cfg["initial_scene"],
        "Initial editor state differs from reviewed read-only baseline",
    )?;
    let original_scene = before["data"]["scene"]
        .as_str()
        .ok_or("Original scene")?
        .to_owned();
    check(
        original_scene == "res://main.tscn"
            && original_nodes["data"]["nodes"]
                .as_array()
                .is_some_and(|n| n.len() == 1),
        "Unexpected original scene scope",
    )?;
    let idle = p.idle("initial-idle").await?;
    p.mutate(
        "supporting-create",
        "scene.create",
        json!({"path":scene,"name":"SemwrightHandoffProof","class":"Node3D"}),
        &idle["stamp"],
    )
    .await?;
    let open = p.edited_scene(scene, "supporting-opened").await?;
    let created = p
        .mutate(
            "camera-create",
            "node.create",
            json!({"parent":".","name":"ProofCamera","class":"Camera3D","properties":[]}),
            &open["stamp"],
        )
        .await?;
    p.mutate(
        "camera-configure",
        "camera.configure",
        json!({"target":"ProofCamera","fov":60.0,"near":0.2,"far":250.0}),
        &created["stamp"],
    )
    .await?;
    let camera_before = p
        .read(
            "camera-before",
            "camera.inspect",
            json!({"target":"ProofCamera"}),
        )
        .await?;
    let camera_node_before = p
        .read(
            "camera-transform-before",
            "node.inspect",
            json!({"path":"ProofCamera"}),
        )
        .await?;
    let mut observations = Vec::new();
    for (i, a) in assets.iter().enumerate() {
        p.idle(&format!("asset{i}-idle-before")).await?;
        let inspected = p
            .read(
                &format!("asset{i}-before"),
                "asset.inspect",
                json!({"path":a["path"]}),
            )
            .await?;
        check(
            inspected["data"]["resource_exists"] == true
                && inspected["data"]["type"] == "PackedScene"
                && inspected["data"]["bytes"] == a["bytes"],
            "Native asset identity/type/size",
        )?;
        let old = inspected["stamp"].clone();
        let reimport = p
            .mutate(
                &format!("asset{i}-reimport"),
                "asset.reimport",
                json!({"path":a["path"]}),
                &old,
            )
            .await?;
        check(
            reimport["affected"]
                .as_array()
                .is_some_and(|v| v == &vec![a["path"].clone()]),
            "Reimport target mismatch",
        )?;
        p.idle(&format!("asset{i}-idle-after")).await?;
        let after = p
            .read(
                &format!("asset{i}-after"),
                "asset.inspect",
                json!({"path":a["path"]}),
            )
            .await?;
        check(
            after["stamp"] != old
                && after["data"]["is_importing"] == false
                && after["data"]["resource_exists"] == true
                && after["data"]["import_sidecar"] == true,
            "Import readback incomplete",
        )?;
        p.call(
            &format!("asset{i}-stale-reimport"),
            "asset.reimport",
            json!({"session":p.session,"path":a["path"],"expect":old,"dry_run":false}),
            Some(ErrorCode::Conflict),
        )
        .await?;
        let rejected = p
            .read(
                &format!("asset{i}-after-stale"),
                "asset.inspect",
                json!({"path":a["path"]}),
            )
            .await?;
        check(
            rejected == after,
            "Rejected stale request changed editor or asset observation",
        )?;
        p.mutate(
            &format!("asset{i}-instantiate"),
            "scene.instantiate",
            json!({"scene":a["path"],"parent":".","name":a["instance"]}),
            &rejected["stamp"],
        )
        .await?;
        observations.push(
            json!({"asset":a,"source_already_imported_before_request":true,"reimport":reimport,
            "after":after,"stale_reimport_rejected_without_observed_effect":true}),
        );
    }
    let scene_state = p.read("scene-meshes", "scene.inspect", json!({})).await?;
    let nodes = scene_state["data"]["nodes"]
        .as_array()
        .ok_or("Scene nodes")?;
    check(
        nodes
            .iter()
            .filter(|n| n["class"] == "MeshInstance3D")
            .count()
            == 4,
        "Expected four imported meshes",
    )?;
    let mut meshes = Vec::new();
    for a in assets {
        for expected in a["meshes"].as_array().ok_or("Mesh expectations")? {
            let name = expected["name"].as_str().ok_or("Expected mesh name")?;
            let instance = a["instance"].as_str().ok_or("Instance")?;
            let matches: Vec<_> = nodes
                .iter()
                .filter(|n| {
                    n["class"] == "MeshInstance3D"
                        && n["name"] == name
                        && n["path"]
                            .as_str()
                            .is_some_and(|v| v.starts_with(&format!("{instance}/")))
                })
                .collect();
            check(matches.len() == 1, "Imported mesh identity not unique")?;
            let path = matches[0]["path"].clone();
            let observed = p
                .read(
                    &format!("mesh-{}", meshes.len()),
                    "node.inspect",
                    json!({"path":path,"include_vertex_bounds":true}),
                )
                .await?;
            let b = &observed["data"]["observed"]["vertex_bounds"];
            check(
                b["complete"] == true
                    && b["space"] == "global"
                    && b["geometry"] == "base_mesh_vertices"
                    && b["vertex_count"].as_u64().is_some_and(|v| v > 0)
                    && b["surface_count"] == expected["surface_count"],
                "Incomplete native vertex observation",
            )?;
            check(
                close_vec(&b["global_min"], &expected["min"])?
                    && close_vec(&b["global_max"], &expected["max"])?,
                "Native geometry bounds differ from GLB",
            )?;
            meshes.push(json!({"expected":expected,"native":observed}));
        }
    }
    let camera_after = p
        .read(
            "camera-after",
            "camera.inspect",
            json!({"target":"ProofCamera"}),
        )
        .await?;
    let camera_node_after = p
        .read(
            "camera-transform-after",
            "node.inspect",
            json!({"path":"ProofCamera"}),
        )
        .await?;
    check(
        camera_before["data"] == camera_after["data"]
            && camera_node_before["data"]["observed"] == camera_node_after["data"]["observed"],
        "Fixture camera changed",
    )?;
    p.mutate(
        "supporting-save",
        "scene.save",
        json!({"save_external_resources":false}),
        &camera_node_after["stamp"],
    )
    .await?;
    let saved = p
        .read("supporting-saved", "project.inspect", json!({}))
        .await?;
    let after_save = p
        .read("editor-after-save", "editor.state", json!({}))
        .await?;
    check(
        after_save["data"]["unsaved_scenes"] == original_editor["data"]["unsaved_scenes"]
            && scene_file.is_file(),
        "Supporting scene save incomplete or original unsaved state changed",
    )?;
    let scene_hash = digest(&scene_file)?;
    p.mutate(
        "restore-edited-scene",
        "scene.open",
        json!({"path":original_scene}),
        &saved["stamp"],
    )
    .await?;
    p.edited_scene(&original_scene, "restored").await?;
    let restored_nodes = p
        .read("original-nodes-after", "scene.inspect", json!({}))
        .await?;
    let restored_editor = p.read("editor-restored", "editor.state", json!({})).await?;
    check(
        restored_nodes["data"]["nodes"] == original_nodes["data"]["nodes"]
            && restored_editor["data"]["unsaved_scenes"]
                == original_editor["data"]["unsaved_scenes"],
        "Original scene content or unsaved status changed",
    )?;
    for a in assets {
        check(
            digest(&root.join(a["path"].as_str().unwrap().trim_start_matches("res://")))?
                == a["sha256"],
            "GLB source changed",
        )?;
    }
    Ok(
        json!({"doctor":doctor,"sessions":sessions,"original_project":before,"artifacts":observations,"meshes":meshes,
        "camera_before":camera_before,"camera_after":camera_after,"fixture_camera_preserved":true,
        "original_nodes_before":original_nodes,"original_nodes_after":restored_nodes,
        "original_editor_before":original_editor,"original_editor_after":restored_editor,
        "original_unsaved_scene_preserved":true,
        "supporting_scene":scene,"supporting_scene_sha256":scene_hash,"original_edited_scene_restored":!original_scene.is_empty(),
        "autoimport_exclusivity_claimed":false,"geometric_transition_A_to_B_claimed":false}),
    )
}
#[tokio::test]
#[ignore = "Physical owner-authorized first-party Godot handoff of pinned Blender exports on Windows"]
async fn windows_broker_godot_handoff() {
    let pipe = PathBuf::from(
        std::env::var_os("SEMWRIGHT_TEST_BROKER_PIPE").expect("Explicit broker pipe"),
    );
    let dir = PathBuf::from(std::env::var_os("SEMWRIGHT_TEST_RECEIPTS").expect("New receipts"));
    let input = PathBuf::from(
        std::env::var_os("SEMWRIGHT_GODOT_PROOF_CONFIG").expect("Bound proof config"),
    );
    let expected = std::env::var("SEMWRIGHT_GODOT_PROOF_CONFIG_SHA256").expect("Config hash");
    assert_eq!(digest(&input).unwrap(), expected);
    let cfg: Value = serde_json::from_slice(&fs::read(&input).unwrap()).unwrap();
    fs::create_dir(&dir).expect("Preserve existing evidence");
    let client = Client::connect(&pipe, None)
        .await
        .expect("Official authenticated memory-only client");
    let mut p = Proof {
        client,
        dir,
        trace: vec![],
        session: String::new(),
    };
    let work = match tokio::time::timeout(Duration::from_secs(180), exercise(&mut p, &cfg)).await {
        Ok(v) => v,
        Err(_) => Err("Overall deadline; unknown outcome; no replay".into()),
    };
    let passed = work.is_ok();
    save(&p.dir.join("receipt.json"),&json!({"schema":1,"passed":passed,"proof":"windows_godot_curated_editor_handoff",
        "config_sha256":expected,"requests":p.trace,"result":work.as_ref().ok(),"error":work.as_ref().err(),
        "driver_broker_used":true,"editor_isolated":false,"human_camera_claimed":false,"G6":"NOT_COMPLETE",
        "Linux_4_5_14":"NOT_RUN","art_status":"REVISE","unknown_mutations_replayed":false})).expect("Preserve receipt");
    assert!(passed, "Inspect preserved Godot handoff receipt");
}

async fn existing_import_readback(p: &mut Proof, cfg: &Value) -> ProofResult<Value> {
    let root = PathBuf::from(cfg["project_root"].as_str().ok_or("Project root")?);
    let sessions = p.call("sessions", "session.list", json!({}), None).await?;
    let rows = sessions["sessions"].as_array().ok_or("Sessions")?;
    check(
        rows.len() == 1 && rows[0]["project"] == cfg["project_id"],
        "Owned session mismatch",
    )?;
    p.session = rows[0]["session"].as_str().ok_or("Session")?.into();
    let initial = p
        .read("continuation-scene", "scene.inspect", json!({}))
        .await?;
    let nodes = initial["data"]["nodes"].as_array().ok_or("Nodes")?;
    check(
        initial["data"]["scene"] == cfg["supporting_scene"]
            && nodes.len() == 2
            && nodes
                .iter()
                .any(|n| n["name"] == "ProofCamera" && n["class"] == "Camera3D"),
        "Continuation fixture changed",
    )?;
    let camera = p
        .read(
            "camera-before",
            "camera.inspect",
            json!({"target":"ProofCamera"}),
        )
        .await?;
    check(
        camera["data"]["fov"] == 60.0 && camera["data"]["far"] == 250.0,
        "Camera fixture changed",
    )?;
    let camera_node = p
        .read(
            "camera-node-before",
            "node.inspect",
            json!({"path":"ProofCamera"}),
        )
        .await?;
    let assets = cfg["assets"].as_array().ok_or("Assets")?;
    check(assets.len() == 2, "Artifact scope")?;
    for (i, a) in assets.iter().enumerate() {
        let path = a["path"].as_str().ok_or("Asset path")?;
        check(
            path.starts_with("res://assets/") && !path.contains(".."),
            "Asset path scope",
        )?;
        check(
            digest(&root.join(&path[6..]))? == a["sha256"],
            "Existing source bytes changed",
        )?;
        let observed = p
            .read(
                &format!("asset{i}-existing"),
                "asset.inspect",
                json!({"path":path}),
            )
            .await?;
        check(
            observed["data"]["resource_exists"] == true
                && observed["data"]["type"] == "PackedScene"
                && observed["data"]["is_importing"] == false
                && observed["data"]["bytes"] == a["bytes"],
            "Existing import unavailable",
        )?;
        // This operation instantiates the already-imported resource. It never requests reimport.
        p.mutate(
            &format!("asset{i}-instantiate-existing"),
            "scene.instantiate",
            json!({"scene":path,"parent":".","name":a["instance"]}),
            &observed["stamp"],
        )
        .await?;
    }
    let native = p.read("scene-meshes", "scene.inspect", json!({})).await?;
    let nodes = native["data"]["nodes"].as_array().ok_or("Native nodes")?;
    check(
        nodes
            .iter()
            .filter(|n| n["class"] == "MeshInstance3D")
            .count()
            == 4,
        "Four native meshes required",
    )?;
    let mut meshes = Vec::new();
    for a in assets {
        for expected in a["meshes"].as_array().ok_or("Expected meshes")? {
            let prefix = format!("{}/", a["instance"].as_str().ok_or("Instance")?);
            let found: Vec<_> = nodes
                .iter()
                .filter(|n| {
                    n["name"] == expected["name"]
                        && n["class"] == "MeshInstance3D"
                        && n["path"].as_str().is_some_and(|v| v.starts_with(&prefix))
                })
                .collect();
            check(found.len() == 1, "Native mesh identity")?;
            let observed = p
                .read(
                    &format!("mesh-{}", meshes.len()),
                    "node.inspect",
                    json!({"path":found[0]["path"],"include_vertex_bounds":true}),
                )
                .await?;
            let b = &observed["data"]["observed"]["vertex_bounds"];
            check(
                b["complete"] == true
                    && b["space"] == "global"
                    && b["geometry"] == "base_mesh_vertices"
                    && b["surface_count"] == expected["surface_count"]
                    && b["vertex_count"].as_u64().is_some_and(|n| n > 0),
                "Complete bounded native geometry required",
            )?;
            check(
                close_vec(&b["global_min"], &expected["min"])?
                    && close_vec(&b["global_max"], &expected["max"])?,
                "Native imported bounds differ from exact GLB",
            )?;
            meshes.push(json!({"expected":expected,"native":observed}));
        }
    }
    let after = p
        .read(
            "camera-after",
            "camera.inspect",
            json!({"target":"ProofCamera"}),
        )
        .await?;
    let node_after = p
        .read(
            "camera-node-after",
            "node.inspect",
            json!({"path":"ProofCamera"}),
        )
        .await?;
    check(
        after["data"] == camera["data"]
            && node_after["data"]["observed"] == camera_node["data"]["observed"],
        "Fixture camera changed",
    )?;
    let saved = p
        .mutate(
            "save-supporting-only",
            "scene.save",
            json!({"save_external_resources":false}),
            &node_after["stamp"],
        )
        .await?;
    let editor = p
        .read("editor-after-save", "editor.state", json!({}))
        .await?;
    check(
        editor["data"]["unsaved_scenes"] == cfg["initial_unsaved_scenes"],
        "Original unsaved state changed",
    )?;
    p.mutate(
        "restore-main-tab",
        "scene.open",
        json!({"path":cfg["initial_scene"]}),
        &saved["stamp"],
    )
    .await?;
    p.edited_scene(cfg["initial_scene"].as_str().unwrap(), "main-restored")
        .await?;
    let restored = p
        .read("main-nodes-restored", "scene.inspect", json!({}))
        .await?;
    let restored_editor = p
        .read("main-editor-restored", "editor.state", json!({}))
        .await?;
    check(
        restored["data"]["nodes"] == cfg["initial_scene_nodes"]
            && restored_editor["data"]["unsaved_scenes"] == cfg["initial_unsaved_scenes"],
        "Original live scene changed",
    )?;
    for a in assets {
        check(
            digest(&root.join(&a["path"].as_str().unwrap()[6..]))? == a["sha256"],
            "Source bytes changed",
        )?;
    }
    let path = cfg["supporting_scene"].as_str().ok_or("Supporting path")?;
    Ok(
        json!({"sessions":sessions,"initial_fixture":initial,"meshes":meshes,"camera_before":camera,"camera_after":after,
        "fixture_camera_preserved":true,"original_live_scene_preserved":true,"main_nodes_restored":restored,
        "main_editor_restored":restored_editor,"supporting_scene_sha256":digest(&root.join(&path[6..]))?,
        "reimport":"BLOCKED_CONSENT_REQUIRED","autoimport_exclusivity_claimed":false,
        "geometric_transition_A_to_B_claimed":false}),
    )
}
#[tokio::test]
#[ignore = "Continue the known fixture after explicit reimport denial, observing existing imports only"]
async fn windows_broker_godot_existing_import_readback() {
    let pipe =
        PathBuf::from(std::env::var_os("SEMWRIGHT_TEST_BROKER_PIPE").expect("Explicit pipe"));
    let dir = PathBuf::from(std::env::var_os("SEMWRIGHT_TEST_RECEIPTS").expect("New receipts"));
    let input = PathBuf::from(std::env::var_os("SEMWRIGHT_GODOT_PROOF_CONFIG").expect("Config"));
    let expected = std::env::var("SEMWRIGHT_GODOT_PROOF_CONFIG_SHA256").expect("Config hash");
    assert_eq!(digest(&input).unwrap(), expected);
    let cfg: Value = serde_json::from_slice(&fs::read(&input).unwrap()).unwrap();
    fs::create_dir(&dir).expect("Preserve previous attempt");
    let client = Client::connect(&pipe, None)
        .await
        .expect("Official authenticated client");
    let mut p = Proof {
        client,
        dir,
        trace: vec![],
        session: String::new(),
    };
    let result = match tokio::time::timeout(
        Duration::from_secs(120),
        existing_import_readback(&mut p, &cfg),
    )
    .await
    {
        Ok(v) => v,
        Err(_) => Err("Unknown outcome; no replay".into()),
    };
    let passed = result.is_ok();
    save(&p.dir.join("receipt.json"),&json!({"schema":1,"passed":passed,"proof":"existing_import_native_geometry_readback",
        "requests":p.trace,"config_sha256":expected,"result":result.as_ref().ok(),"error":result.as_ref().err(),
        "reimport":"BLOCKED_CONSENT_REQUIRED","whole_P3":"PARTIAL","G6":"NOT_COMPLETE","editor_isolated":false,
        "human_camera_claimed":false,"art_status":"REVISE","Linux_4_5_14":"NOT_RUN"})).unwrap();
    assert!(passed, "Inspect preserved existing-import readback receipt");
}

async fn finalize_known_fixture(p: &mut Proof, cfg: &Value) -> ProofResult<Value> {
    let root = PathBuf::from(cfg["project_root"].as_str().ok_or("Project root")?);
    let scene = cfg["supporting_scene"].as_str().ok_or("Supporting scene")?;
    check(
        scene.starts_with("res://proofs/") && !scene.contains(".."),
        "Owned fixture scope",
    )?;
    let scene_file = root.join(&scene[6..]);
    check(
        digest(&scene_file)? == cfg["fixture_before_sha256"],
        "Fixture disk bytes changed after reconciliation",
    )?;
    let sessions = p.call("sessions", "session.list", json!({}), None).await?;
    let rows = sessions["sessions"].as_array().ok_or("Sessions")?;
    check(
        rows.len() == 1
            && rows[0]["project"] == cfg["project_id"]
            && rows[0]["session"] == cfg["editor_session"],
        "Owned Editor session changed",
    )?;
    p.session = rows[0]["session"].as_str().unwrap().into();
    let initial = p
        .read("reconciled-fixture", "scene.inspect", json!({}))
        .await?;
    check(
        initial["data"]["scene"] == scene && initial["stamp"] == cfg["reconciled_stamp"],
        "Live fixture differs from reconciled failed-save state",
    )?;
    let camera_before = p
        .read(
            "camera-before",
            "node.inspect",
            json!({"path":"ProofCamera"}),
        )
        .await?;
    check(
        camera_before["data"] == cfg["reconciled_camera_data"],
        "Fixture camera changed",
    )?;
    // Switching tabs preserves their live contents. Never reload or discard the original main.
    p.mutate(
        "observe-main-tab",
        "scene.open",
        json!({"path":cfg["initial_scene"]}),
        &camera_before["stamp"],
    )
    .await?;
    p.edited_scene(cfg["initial_scene"].as_str().unwrap(), "main-opened-before")
        .await?;
    let main_before = p
        .read("main-nodes-before", "scene.inspect", json!({}))
        .await?;
    let editor_before = p.read("editor-before", "editor.state", json!({})).await?;
    check(
        main_before["data"]["nodes"] == cfg["initial_scene_nodes"],
        "Original live scene changed before save",
    )?;
    let unsaved = editor_before["data"]["unsaved_scenes"]
        .as_array()
        .ok_or("Unsaved scenes")?;
    check(
        unsaved.len() == 2
            && unsaved.contains(&cfg["initial_scene"])
            && unsaved.contains(&cfg["supporting_scene"]),
        "Unexpected unsaved scene scope",
    )?;
    p.mutate(
        "return-fixture-tab",
        "scene.open",
        json!({"path":scene}),
        &editor_before["stamp"],
    )
    .await?;
    p.edited_scene(scene, "fixture-opened").await?;
    let native = p
        .read("fixture-before-save", "scene.inspect", json!({}))
        .await?;
    check(
        native["data"]["nodes"] == initial["data"]["nodes"],
        "Tab switching changed live fixture",
    )?;
    for a in cfg["assets"].as_array().ok_or("Assets")? {
        check(
            digest(&root.join(&a["path"].as_str().ok_or("Asset path")?[6..]))? == a["sha256"],
            "External GLB changed before save",
        )?;
    }
    // A new, supported save operation after read-only reconciliation; not a retry of scene-only mode.
    let save = p
        .mutate(
            "save-owned-fixture-with-external-mode",
            "scene.save",
            json!({"save_external_resources":true}),
            &native["stamp"],
        )
        .await?;
    let camera_after = p
        .read(
            "camera-after-save",
            "node.inspect",
            json!({"path":"ProofCamera"}),
        )
        .await?;
    check(
        camera_after["data"] == camera_before["data"],
        "Fixture camera changed during save",
    )?;
    let editor_after = p
        .read("editor-after-save", "editor.state", json!({}))
        .await?;
    check(
        editor_after["data"]["unsaved_scenes"] == cfg["initial_unsaved_scenes"],
        "Original unsaved state changed or fixture not saved",
    )?;
    check(
        digest(&scene_file)? != cfg["fixture_before_sha256"],
        "Fixture disk bytes did not change",
    )?;
    let stale = native["stamp"].clone();
    p.call(
        "reject-stale-camera-configure",
        "camera.configure",
        json!({"session":p.session,"target":"ProofCamera","fov":75.0,"near":0.2,"far":250.0,
               "expect":stale,"dry_run":false}),
        Some(ErrorCode::Conflict),
    )
    .await?;
    let camera_rejected = p
        .read(
            "camera-after-stale",
            "node.inspect",
            json!({"path":"ProofCamera"}),
        )
        .await?;
    check(
        camera_rejected == camera_after,
        "Stale camera request changed observed state",
    )?;
    p.mutate(
        "restore-main-tab",
        "scene.open",
        json!({"path":cfg["initial_scene"]}),
        &camera_rejected["stamp"],
    )
    .await?;
    p.edited_scene(cfg["initial_scene"].as_str().unwrap(), "main-restored")
        .await?;
    let main_after = p
        .read("main-nodes-after", "scene.inspect", json!({}))
        .await?;
    let restored_editor = p
        .read("main-editor-after", "editor.state", json!({}))
        .await?;
    check(
        main_after["data"]["nodes"] == main_before["data"]["nodes"]
            && restored_editor["data"]["unsaved_scenes"] == cfg["initial_unsaved_scenes"],
        "Original scene content or unsaved flag changed",
    )?;
    for a in cfg["assets"].as_array().unwrap() {
        check(
            digest(&root.join(&a["path"].as_str().unwrap()[6..]))? == a["sha256"],
            "External GLB changed",
        )?;
    }
    Ok(
        json!({"save":save,"camera_before":camera_before,"camera_after":camera_after,
        "stale_camera_rejected_without_observed_effect":true,"main_before":main_before,"main_after":main_after,
        "main_editor_after":restored_editor,"supporting_scene_sha256":digest(&scene_file)?,
        "original_live_scene_projection_preserved":true,"original_unsaved_flag_preserved":true,
        "fixture_camera_preserved":true,"external_resource_dirty_flag_introspection":false,
        "reimport":"BLOCKED_CONSENT_REQUIRED","prior_unknown_save_error_rewritten":false}),
    )
}
#[tokio::test]
#[ignore = "Save the reconciled owned supporting fixture with the existing supported save mode"]
async fn windows_broker_godot_finalize_known_fixture() {
    let pipe =
        PathBuf::from(std::env::var_os("SEMWRIGHT_TEST_BROKER_PIPE").expect("Explicit pipe"));
    let dir = PathBuf::from(std::env::var_os("SEMWRIGHT_TEST_RECEIPTS").expect("New receipts"));
    let input = PathBuf::from(std::env::var_os("SEMWRIGHT_GODOT_PROOF_CONFIG").expect("Config"));
    let expected = std::env::var("SEMWRIGHT_GODOT_PROOF_CONFIG_SHA256").expect("Config hash");
    assert_eq!(digest(&input).unwrap(), expected);
    let cfg: Value = serde_json::from_slice(&fs::read(&input).unwrap()).unwrap();
    fs::create_dir(&dir).expect("Preserve previous evidence");
    let client = Client::connect(&pipe, None)
        .await
        .expect("Official authenticated client");
    let mut p = Proof {
        client,
        dir,
        trace: vec![],
        session: String::new(),
    };
    let result = match tokio::time::timeout(
        Duration::from_secs(120),
        finalize_known_fixture(&mut p, &cfg),
    )
    .await
    {
        Ok(v) => v,
        Err(_) => Err("Unknown outcome; do not replay".into()),
    };
    let passed = result.is_ok();
    save(&p.dir.join("receipt.json"), &json!({"schema":1,"passed":passed,"proof":"reconciled_fixture_supported_save",
        "config_sha256":expected,"requests":p.trace,"result":result.as_ref().ok(),"error":result.as_ref().err(),
        "reimport":"BLOCKED_CONSENT_REQUIRED","whole_P3":"PARTIAL","G6":"NOT_COMPLETE",
        "editor_isolated":false,"human_camera_claimed":false,"art_status":"REVISE","Linux_4_5_14":"NOT_RUN"})).unwrap();
    assert!(passed, "Inspect preserved fixture-save receipt");
}
