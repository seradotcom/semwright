//! Physical Windows proof through one official, memory-only broker client.
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
fn check(condition: bool, message: &str) -> ProofResult<()> {
    if condition {
        Ok(())
    } else {
        Err(message.to_owned())
    }
}
fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn read(path: &Path) -> ProofResult<Vec<u8>> {
    fs::read(path).map_err(|e| e.to_string())
}
fn require_absent(path: &Path) -> ProofResult<()> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(_) => Err(format!(
            "Preserve existing fixture target: {}",
            path.display()
        )),
        Err(e) => Err(e.to_string()),
    }
}
fn save_json(path: &Path, value: &Value) -> ProofResult<String> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(&bytes).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    Ok(sha256(&bytes))
}
struct SessionProof {
    client: Client,
    receipts: PathBuf,
    trace: Vec<Value>,
}
impl SessionProof {
    async fn exchange(
        &mut self,
        label: &str,
        command: &str,
        args: Value,
        expected_error: Option<ErrorCode>,
        budget: Duration,
    ) -> ProofResult<Value> {
        let id = unique_id();
        let index = self.trace.len();
        let prefix = format!("{index:02}-{label}");
        let request = json!({"request_id":id,"command":command,"args":args,
            "dry_run":false,"backend":null});
        let request_sha = save_json(
            &self.receipts.join(format!("{prefix}.request.json")),
            &request,
        )?;
        self.trace
            .push(json!({"label":label,"request_id":id,"command":command,
            "request_sha256":request_sha,"outcome":"REQUEST_RECORDED_RESPONSE_PENDING"}));
        let started = Instant::now();
        let result = tokio::time::timeout(
            budget,
            self.client.execute(
                id.clone(),
                ExecuteRequest {
                    command: command.into(),
                    args,
                    dry_run: false,
                    backend: None,
                },
            ),
        )
        .await;
        let envelope = match result {
            Ok(Ok(value)) => value,
            Ok(Err(error)) => {
                self.trace[index]["outcome"] = json!("TRANSPORT_ERROR_UNKNOWN");
                self.trace[index]["error"] = json!(error.to_string());
                return Err(format!("{label}: transport error; no mutation retry"));
            }
            Err(_) => {
                self.trace[index]["outcome"] = json!("TIMEOUT_UNKNOWN");
                return Err(format!("{label}: timeout; no mutation retry"));
            }
        };
        let value = serde_json::to_value(&envelope).map_err(|e| e.to_string())?;
        let envelope_sha = save_json(
            &self.receipts.join(format!("{prefix}.envelope.json")),
            &value,
        )?;
        self.trace[index]["outcome"] = json!("RESPONSE_RECEIVED");
        self.trace[index]["envelope_sha256"] = json!(envelope_sha);
        self.trace[index]["elapsed_ms"] = json!(started.elapsed().as_millis());
        self.trace[index]["ok"] = json!(envelope.ok);
        self.trace[index]["error"] = value.get("error").cloned().unwrap_or(Value::Null);
        eprintln!(
            "BROKER_PROOF_STEP {}",
            json!({"label":label,"request_id":id,"ok":envelope.ok})
        );
        check(
            envelope.request_id == id,
            "Response request identity mismatch",
        )?;
        match expected_error {
            Some(expected) => {
                check(!envelope.ok, "Expected operation rejection")?;
                check(
                    envelope.error.as_ref().map(|e| &e.code) == Some(&expected),
                    "Unexpected rejection code",
                )?;
            }
            None => check(
                envelope.ok && envelope.error.is_none(),
                &format!("{label}: broker operation failed"),
            )?,
        }
        Ok(envelope.data.unwrap_or(Value::Null))
    }
    async fn op(&mut self, label: &str, command: &str, args: Value) -> ProofResult<Value> {
        self.exchange(label, command, args, None, Duration::from_secs(100))
            .await
    }
}

fn static_spec() -> Value {
    json!({
        "version":1,"collection":"native_authoring_proof","meters_per_unit":1.0,
        "textures":[],
        "materials":[
            {"id":"earth","base_color":[0.45,0.24,0.10,1.0],"roughness":0.7,"metallic":0.0},
            {"id":"sea","base_color":[0.08,0.32,0.45,1.0],"roughness":0.5,"metallic":0.0}
        ],
        "entities":[
            {"id":"tetra","name":"Proof Tetrahedron",
             "shape":{"kind":"mesh","vertices":[[0,0,0],[2,0,0],[0,1,0],[0,0,3]],
                      "faces":[[0,2,1],[0,1,3],[0,3,2],[1,2,3]],"uv":null},
             "transform":{"translation":[0,0,0],"rotation":[0,0,0],"scale":[1,1,1]},
             "materials":["earth"],"modifiers":[],"shade_smooth":false},
            {"id":"wedge","name":"Proof Triangular Prism",
             "shape":{"kind":"mesh","vertices":[[0,0,0],[2,0,0],[0,2,0],[0,0,1],[2,0,1],[0,2,1]],
                      "faces":[[0,2,1],[3,4,5],[0,1,4,3],[1,2,5,4],[2,0,3,5]],"uv":null},
             "transform":{"translation":[4,0,0],"rotation":[0,0,0],"scale":[1,1,1]},
             "materials":["sea"],"modifiers":[],"shade_smooth":false}
        ],
        "relations":[],"animation":null
    })
}
fn rows(snapshot: &Value) -> ProofResult<&Vec<Value>> {
    snapshot["items"]
        .as_array()
        .ok_or_else(|| "Snapshot lacks rows".into())
}
fn entity<'a>(snapshot: &'a Value, id: &str) -> ProofResult<&'a Value> {
    let matches: Vec<_> = rows(snapshot)?
        .iter()
        .filter(|v| v["entity"] == id)
        .collect();
    check(matches.len() == 1, "Managed entity identity is not unique")?;
    Ok(matches[0])
}
fn verify_report(result: &Value) -> ProofResult<()> {
    let report: semwright_semantic_composition::VerificationReport =
        serde_json::from_value(result["report"].clone()).map_err(|e| e.to_string())?;
    check(
        result["report"]["execution_status"] == "completed",
        "Apply incomplete",
    )?;
    check(
        report.verdict().map_err(|e| e.to_string())?
            == semwright_semantic_composition::Verdict::Pass,
        "Trusted Effect Conformance did not pass",
    )
}
fn verify_geometry(snapshot: &Value) -> ProofResult<()> {
    check(
        snapshot["source_only"] == true
            && snapshot["exhaustive"] == true
            && snapshot["total"] == 2
            && snapshot["drift"] == false,
        "Incomplete/drifted island",
    )?;
    for (id, vertices, polygons, material) in [("tetra", 4, 4, "earth"), ("wedge", 6, 5, "sea")] {
        let row = entity(snapshot, id)?;
        check(
            row["type"] == "MESH" && row["vertices"] == vertices && row["polygons"] == polygons,
            "Native topology counts differ",
        )?;
        check(
            row["geometry_digest"]
                .as_str()
                .is_some_and(|s| s.len() == 64)
                && row["normal_digest"].as_str().is_some_and(|s| s.len() == 64),
            "Geometry readback digest missing",
        )?;
        check(
            row["materials"].as_array().is_some_and(|a| a.len() == 1)
                && row["materials"][0]["id"] == material,
            "Native material assignment differs",
        )?;
    }
    Ok(())
}
fn inspect_glb(path: &Path) -> ProofResult<Value> {
    let bytes = read(path)?;
    check(
        (20..=16 * 1024 * 1024).contains(&bytes.len()),
        "GLB size budget",
    )?;
    let word = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
    check(
        &bytes[..4] == b"glTF" && word(4) == 2 && word(8) as usize == bytes.len(),
        "GLB header",
    )?;
    let end = 20usize
        .checked_add(word(12) as usize)
        .ok_or("GLB length overflow")?;
    check(
        end <= bytes.len() && word(16) == 0x4e4f534a,
        "GLB JSON chunk",
    )?;
    let doc: Value = serde_json::from_slice(&bytes[20..end]).map_err(|e| e.to_string())?;
    check(
        doc["meshes"].as_array().is_some_and(|v| v.len() == 2),
        "GLB mesh count",
    )?;
    check(
        doc["materials"].as_array().is_some_and(|v| v.len() == 2),
        "GLB material count",
    )?;
    for field in ["buffers", "images"] {
        if let Some(values) = doc[field].as_array() {
            check(
                values.iter().all(|v| v.get("uri").is_none()),
                "GLB external URI",
            )?;
        }
    }
    Ok(
        json!({"path":path,"bytes":bytes.len(),"sha256":sha256(&bytes),
              "document":doc,"self_contained":true}),
    )
}
async fn authoring_exercise(
    proof: &mut SessionProof,
    workspace: &Path,
    prefix: &str,
) -> ProofResult<Value> {
    let artifact = |name: &str| format!("{prefix}/{name}");
    for name in [
        "authoring.glb",
        "authoring-scene.blend",
        "authoring-library.blend",
        "final-recovery.blend",
    ] {
        require_absent(&workspace.join(artifact(name)))?;
    }
    let summary = proof
        .op("version", "driver.blender.introspect.summary", json!({}))
        .await?;
    check(
        summary["version"] == json!([5, 2, 1]) && summary["arbitrary_python"] == false,
        "Unexpected native version/authority",
    )?;
    proof
        .op(
            "sentinel-create",
            "driver.blender.object.create",
            json!({"name":"AuthoringSentinel","primitive":"empty","location":[1,2,3]}),
        )
        .await?;
    let initial = proof
        .op(
            "initial-inspect",
            "driver.blender.composition.inspect",
            json!({}),
        )
        .await?;
    let stale = proof
        .op(
            "stale-plan",
            "driver.blender.composition.plan",
            json!({"intent":{"kind":"create","spec":static_spec()}}),
        )
        .await?;
    check(
        proof
            .op(
                "plan-readonly",
                "driver.blender.composition.inspect",
                json!({}),
            )
            .await?
            == initial,
        "Planning mutated the scene",
    )?;
    proof
        .op(
            "controlled-intervening-edit",
            "driver.blender.object.transform",
            json!({"name":"AuthoringSentinel","location":[2,2,3]}),
        )
        .await?;
    let baseline = proof
        .op(
            "intervening-readback",
            "driver.blender.composition.inspect",
            json!({}),
        )
        .await?;
    check(
        baseline["fingerprint"] != initial["fingerprint"],
        "Intervening edit did not change plan scope",
    )?;
    proof
        .exchange(
            "deny-stale-plan",
            "driver.blender.composition.apply",
            json!({"plan_ref":stale["plan_ref"]}),
            Some(ErrorCode::StaleReference),
            Duration::from_secs(100),
        )
        .await?;
    check(
        proof
            .op(
                "stale-no-effects",
                "driver.blender.composition.inspect",
                json!({}),
            )
            .await?
            == baseline,
        "Stale rejection changed current scene",
    )?;
    let planned = proof
        .op(
            "create-plan",
            "driver.blender.composition.plan",
            json!({"intent":{"kind":"create","spec":static_spec()}}),
        )
        .await?;
    let applied = proof
        .op(
            "create-apply",
            "driver.blender.composition.apply",
            json!({"plan_ref":planned["plan_ref"]}),
        )
        .await?;
    verify_report(&applied)?;
    let island = applied["island"]
        .as_str()
        .ok_or("Missing island")?
        .to_owned();
    let observed = proof
        .op(
            "independent-island-readback",
            "driver.blender.composition.inspect",
            json!({"island":island}),
        )
        .await?;
    check(
        observed == applied["snapshot"],
        "Apply and independent readback differ",
    )?;
    verify_geometry(&observed)?;
    let whole = proof
        .op(
            "unmanaged-preservation",
            "driver.blender.composition.inspect",
            json!({}),
        )
        .await?;
    let unmanaged: Vec<_> = rows(&whole)?
        .iter()
        .filter(|v| v["entity"].is_null())
        .cloned()
        .collect();
    check(
        Value::Array(unmanaged) == baseline["items"],
        "Unmanaged native objects changed",
    )?;
    proof
        .exchange(
            "deny-replay",
            "driver.blender.composition.apply",
            json!({"plan_ref":planned["plan_ref"]}),
            Some(ErrorCode::StaleReference),
            Duration::from_secs(100),
        )
        .await?;
    check(
        proof
            .op(
                "replay-no-effects",
                "driver.blender.composition.inspect",
                json!({}),
            )
            .await?
            == whole,
        "Replay rejection changed the scene",
    )?;
    let transformed = proof
        .op(
            "transform-plan",
            "driver.blender.composition.plan",
            json!({"intent":{"kind":"transform","island":island,"entity":"tetra",
            "transform":{"translation":[1,2,0.5],"rotation":[0,0,0],"scale":[1,1,1]},
            "meters_per_unit":1.0,"expected_fingerprint":observed["fingerprint"]}}),
        )
        .await?;
    let edited = proof
        .op(
            "transform-apply",
            "driver.blender.composition.apply",
            json!({"plan_ref":transformed["plan_ref"]}),
        )
        .await?;
    verify_report(&edited)?;
    let edited_readback = proof
        .op(
            "transform-readback",
            "driver.blender.composition.inspect",
            json!({"island":island}),
        )
        .await?;
    verify_geometry(&edited_readback)?;
    check(
        entity(&edited_readback, "tetra")?["translation"] == json!([1.0, 2.0, 0.5]),
        "Incremental transform differs",
    )?;
    check(
        entity(&edited_readback, "wedge")? == entity(&observed, "wedge")?,
        "Other managed entity changed",
    )?;
    let library = proof
        .op(
            "persist-library",
            "driver.blender.composition.persist",
            json!({"island":island,"path":artifact("authoring-library.blend")}),
        )
        .await?;
    let library_bytes = read(&workspace.join(artifact("authoring-library.blend")))?;
    check(
        library["sha256"] == sha256(&library_bytes)
            && library["bytes"].as_u64() == Some(library_bytes.len() as u64),
        "Library hash differs",
    )?;
    let static_before = static_structure_signature(&checkpoint_surface(proof).await?)?;
    let inspection = proof
        .op(
            "checkpoint-inspect",
            "driver.blender.session.checkpoint.inspect",
            json!({}),
        )
        .await?;
    check(
        inspection["supported"] == true,
        "Authored static scene checkpoint unsupported",
    )?;
    let closed = proof
        .op(
            "checkpoint-close",
            "driver.blender.session.checkpoint_close",
            json!({"checkpoint_path":artifact("authoring-scene.blend")}),
        )
        .await?;
    check(
        closed["closed"] == true && closed["close_state"] == "closed",
        "Source session not closed",
    )?;
    let blend_bytes = read(&workspace.join(artifact("authoring-scene.blend")))?;
    let blend_sha = sha256(&blend_bytes);
    check(
        closed["checkpoint"]["sha256"] == blend_sha,
        "Scene checkpoint hash differs",
    )?;
    let reopened = proof
        .op(
            "fresh-reopen",
            "driver.blender.session.open",
            json!({"path":artifact("authoring-scene.blend"),"expected_sha256":blend_sha}),
        )
        .await?;
    check(
        reopened["fresh_session"] == true && reopened["use_scripts"] == false,
        "Unsafe fresh reopen",
    )?;
    let static_after = static_structure_signature(&checkpoint_surface(proof).await?)?;
    check(
        static_before == static_after,
        "Recovery changed static sequencer/shader structure",
    )?;
    let recovered = proof
        .op(
            "recovered-island",
            "driver.blender.composition.inspect",
            json!({"island":island}),
        )
        .await?;
    verify_geometry(&recovered)?;
    check(
        recovered["native_session"] != edited_readback["native_session"],
        "Native session was not fresh",
    )?;
    check(
        recovered["items"] == edited_readback["items"]
            && recovered["fingerprint"] == edited_readback["fingerprint"],
        "Recovery lost authored projection",
    )?;
    proof
        .exchange(
            "deny-old-generation-plan",
            "driver.blender.composition.apply",
            json!({"plan_ref":transformed["plan_ref"]}),
            Some(ErrorCode::PolicyDenied),
            Duration::from_secs(100),
        )
        .await?;
    let collection = entity(&recovered, "tetra")?["collections"][0]
        .as_str()
        .ok_or("Native collection absent")?
        .to_owned();
    let exported = proof
        .op(
            "export",
            "driver.blender.export.glb",
            json!({"collection":collection,"path":artifact("authoring.glb"),"animations":false}),
        )
        .await?;
    let glb = inspect_glb(&workspace.join(artifact("authoring.glb")))?;
    let final_whole = proof
        .op(
            "recovered-unmanaged",
            "driver.blender.composition.inspect",
            json!({}),
        )
        .await?;
    let final_unmanaged: Vec<_> = rows(&final_whole)?
        .iter()
        .filter(|v| v["entity"].is_null())
        .cloned()
        .collect();
    check(
        Value::Array(final_unmanaged) == baseline["items"],
        "Recovered unmanaged objects differ",
    )?;
    Ok(
        json!({"native_version":summary,"island":island,"plan":planned,"apply":applied,
        "independent_readback":observed,"incremental_apply":edited,"recovered_readback":recovered,
        "checkpoint":closed,"reopen":reopened,"library":library,"export":exported,"glb":glb,
        "stale_plan":"DENIED_NO_EFFECT","replay":"DENIED_NO_EFFECT","old_generation_plan":"DENIED",
        "static_structure_preservation":{"before":static_before,"after":static_after,"passed":true,
            "scope":"RNA relation names/types and channel names/counts; not full shader socket-state equivalence"},
        "unmanaged_preservation":"PASS","human_edit_simulated":false,
        "intervening_edit_source":"authorized typed fixture operation; not human review"}),
    )
}
#[tokio::test]
#[ignore = "Requires owner-authorized isolated Windows broker and new artifact directory"]
async fn windows_production_broker_typed_authoring() {
    let pipe =
        PathBuf::from(std::env::var_os("SEMWRIGHT_TEST_BROKER_PIPE").expect("Explicit pipe"));
    let receipts =
        PathBuf::from(std::env::var_os("SEMWRIGHT_TEST_RECEIPTS").expect("New receipts"));
    let workspace = PathBuf::from(std::env::var_os("SEMWRIGHT_TEST_WORKSPACE").expect("Workspace"));
    let prefix = std::env::var("SEMWRIGHT_TEST_ARTIFACT_PREFIX").expect("New artifact prefix");
    assert!(
        prefix.len() <= 48
            && !prefix.is_empty()
            && prefix
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    );
    assert!(workspace.is_absolute() && receipts.is_absolute() && workspace.join(&prefix).is_dir());
    fs::create_dir(&receipts).expect("Preserve earlier receipts");
    let started = Instant::now();
    let client = tokio::time::timeout(Duration::from_secs(5), Client::connect(&pipe, None))
        .await
        .expect("Connect deadline")
        .expect("Authenticated connection");
    let mut proof = SessionProof {
        client,
        receipts,
        trace: Vec::new(),
    };
    let work = match tokio::time::timeout(
        Duration::from_secs(210),
        authoring_exercise(&mut proof, &workspace, &prefix),
    )
    .await
    {
        Ok(value) => value,
        Err(_) => Err("Overall deadline; unknown outcome, no replay".into()),
    };
    let inspection = proof
        .exchange(
            "cleanup-inspect",
            "driver.blender.session.checkpoint.inspect",
            json!({}),
            None,
            Duration::from_secs(10),
        )
        .await;
    let close = if inspection.as_ref().is_ok_and(|v| v["supported"] == true) {
        proof
            .exchange(
                "final-close",
                "driver.blender.session.checkpoint_close",
                json!({"checkpoint_path":format!("{prefix}/final-recovery.blend")}),
                None,
                Duration::from_secs(15),
            )
            .await
    } else {
        Err(format!(
            "Cleanup declined after read-only inspection: {inspection:?}"
        ))
    };
    let close_ack = close
        .as_ref()
        .is_ok_and(|v| v["closed"] == true && v["close_state"] == "closed");
    let passed = work.is_ok() && close_ack;
    let result = json!({"schema":1,"proof_kind":"windows_broker_typed_authoring",
        "broker_used":true,"passed":passed,"elapsed_ms":started.elapsed().as_millis(),
        "requests":proof.trace,"result":work.as_ref().ok(),"error":work.as_ref().err(),
        "close":close.as_ref().ok(),"close_error":close.as_ref().err(),"close_acknowledged":close_ack,
        "physical_reap":"VERIFY_CORRELATED_HOST_LOG_AND_PID","Linux_4_5_14":"NOT_RUN",
        "art_approval":false,"G6":"NOT_COMPLETE","Godot_import":"NOT_RUN"});
    let digest = save_json(&proof.receipts.join("receipt.json"), &result).expect("Save receipt");
    eprintln!(
        "AUTHORING_PROOF_RECEIPT {}",
        json!({"passed":passed,"sha256":digest})
    );
    assert!(passed, "Inspect preserved physical authoring receipts");
}

fn static_structure_signature(surface: &Value) -> ProofResult<Value> {
    let mut signature = serde_json::Map::new();
    for observation in surface["observations"]
        .as_array()
        .ok_or("Missing surface observations")?
    {
        let label = observation["label"]
            .as_str()
            .ok_or("Missing surface label")?;
        let mut fields = serde_json::Map::new();
        for (property, value) in observation["observed"]
            .as_object()
            .ok_or("Missing observed fields")?
        {
            if let Some(rows) = value["items"].as_array() {
                check(value["truncated"] == false, "Incomplete recovery relation")?;
                let mut stable: Vec<Value> = rows
                    .iter()
                    .map(|row| json!({"name":row["name"],"rna_type":row["rna_type"]}))
                    .collect();
                stable.sort_by_key(|row| row.to_string());
                fields.insert(property.clone(), Value::Array(stable));
            } else {
                fields.insert(property.clone(), value["value"].clone());
            }
        }
        signature.insert(label.into(), Value::Object(fields));
    }
    Ok(Value::Object(signature))
}

async fn checkpoint_surface(proof: &mut SessionProof) -> ProofResult<Value> {
    let mut queue = std::collections::VecDeque::new();
    let mut observations = Vec::new();
    for root in ["scenes", "materials", "worlds", "lights"] {
        let listed = proof
            .op(
                &format!("list-{root}"),
                "driver.blender.semantic.objects",
                json!({"root":root,"limit":8}),
            )
            .await?;
        check(
            listed["truncated"] == false,
            "Diagnostic root exceeds bound",
        )?;
        for item in listed["items"]
            .as_array()
            .ok_or("Missing diagnostic rows")?
        {
            queue.push_back((
                format!("{root}:{}", item["name"]),
                item["ref"].clone(),
                0u32,
            ));
        }
    }
    let mut visited = 0;
    while let Some((label, reference, depth)) = queue.pop_front() {
        visited += 1;
        check(visited <= 64, "Diagnostic relation budget")?;
        let described = proof
            .op(
                "describe",
                "driver.blender.semantic.object.describe",
                json!({"ref":reference}),
            )
            .await?;
        check(
            described["truncated"] == false,
            "Diagnostic property enumeration incomplete",
        )?;
        let props = described["properties"]
            .as_array()
            .ok_or("Missing property descriptors")?;
        let mut relations = serde_json::Map::new();
        for name in [
            "use_nodes",
            "sequence_editor",
            "node_tree",
            "compositing_node_group",
            "rigidbody_world",
            "strips",
            "strips_all",
            "sequences",
            "sequences_all",
            "meta_stack",
            "active_strip",
            "nodes",
            "channels",
            "collection",
            "constraints",
            "point_cache",
        ] {
            let Some(property) = props.iter().find(|p| p["id"] == name) else {
                continue;
            };
            if name == "use_nodes" {
                let value = proof
                    .op(
                        "scalar",
                        "driver.blender.semantic.property.get",
                        json!({"ref":reference,"property":name}),
                    )
                    .await?;
                relations.insert(name.into(), value);
                continue;
            }
            if property["status"] != "relation" {
                continue;
            }
            let relation = proof
                .op(
                    "relation",
                    "driver.blender.semantic.relations",
                    json!({"ref":reference,"property":name,"limit":256}),
                )
                .await?;
            check(
                relation["truncated"] == false,
                "Diagnostic relation exceeds bound",
            )?;
            if depth < 1
                && [
                    "sequence_editor",
                    "node_tree",
                    "compositing_node_group",
                    "rigidbody_world",
                ]
                .contains(&name)
            {
                for child in relation["items"].as_array().ok_or("Missing child rows")? {
                    queue.push_back((format!("{label}/{name}"), child["ref"].clone(), depth + 1));
                }
            }
            relations.insert(name.into(), relation);
        }
        observations.push(json!({"label":label,"description":described,"observed":relations}));
    }
    let inspection = proof
        .op(
            "checkpoint-inspect",
            "driver.blender.session.checkpoint.inspect",
            json!({}),
        )
        .await?;
    Ok(json!({"observations":observations,"inspection":inspection}))
}
#[tokio::test]
#[ignore = "Owner-authorized read-only Windows checkpoint compatibility diagnosis"]
async fn windows_production_broker_checkpoint_surface() {
    let pipe =
        PathBuf::from(std::env::var_os("SEMWRIGHT_TEST_BROKER_PIPE").expect("Explicit pipe"));
    let receipts =
        PathBuf::from(std::env::var_os("SEMWRIGHT_TEST_RECEIPTS").expect("New receipts"));
    assert!(receipts.is_absolute());
    fs::create_dir(&receipts).expect("Preserve receipts");
    let client = Client::connect(&pipe, None)
        .await
        .expect("Authenticated client");
    let mut proof = SessionProof {
        client,
        receipts,
        trace: Vec::new(),
    };
    let result = match tokio::time::timeout(
        Duration::from_secs(120),
        checkpoint_surface(&mut proof),
    )
    .await
    {
        Ok(value) => value,
        Err(_) => Err("Diagnostic deadline".into()),
    };
    let passed = result.is_ok();
    save_json(&proof.receipts.join("receipt.json"),&json!({"schema":1,"passed":passed,
        "read_only":true,"result":result.as_ref().ok(),"error":result.as_ref().err(),"requests":proof.trace,
        "normal_close":false,"cleanup":"Owned technical fixture retirement required; no human scene loaded",
        "save_qualified":false})).expect("Save diagnostic");
    assert!(passed, "Inspect read-only diagnostic receipts");
}

async fn static_asset_exercise(
    proof: &mut SessionProof,
    workspace: &Path,
    prefix: &str,
    spec: &Value,
) -> ProofResult<Value> {
    let artifact = |name: &str| format!("{prefix}/{name}");
    let entities = spec["entities"].as_array().ok_or("Missing entity list")?;
    check((1..=128).contains(&entities.len()), "Static entity budget")?;
    check(
        spec["textures"] == json!([])
            && spec["relations"] == json!([])
            && spec["animation"].is_null(),
        "Static asset contains unsupported dependencies",
    )?;
    for item in entities {
        check(
            item["shape"]["kind"] == "mesh" && item["modifiers"] == json!([]),
            "Static asset requires concrete meshes without modifiers",
        )?;
    }
    for name in [
        "asset-library.blend",
        "asset-scene.blend",
        "asset.glb",
        "final-recovery.blend",
    ] {
        require_absent(&workspace.join(artifact(name)))?;
    }
    let version = proof
        .op("version", "driver.blender.introspect.summary", json!({}))
        .await?;
    check(
        version["version"] == json!([5, 2, 1]) && version["arbitrary_python"] == false,
        "Unexpected runtime authority",
    )?;
    let baseline = proof
        .op("baseline", "driver.blender.composition.inspect", json!({}))
        .await?;
    let planned = proof
        .op(
            "plan",
            "driver.blender.composition.plan",
            json!({"intent":{"kind":"create","spec":spec}}),
        )
        .await?;
    check(
        proof
            .op(
                "plan-readonly",
                "driver.blender.composition.inspect",
                json!({}),
            )
            .await?
            == baseline,
        "Planning changed source state",
    )?;
    let applied = proof
        .op(
            "apply",
            "driver.blender.composition.apply",
            json!({"plan_ref":planned["plan_ref"]}),
        )
        .await?;
    verify_report(&applied)?;
    let island = applied["island"]
        .as_str()
        .ok_or("Missing island")?
        .to_owned();
    let observed = proof
        .op(
            "readback",
            "driver.blender.composition.inspect",
            json!({"island":island}),
        )
        .await?;
    check(
        observed == applied["snapshot"]
            && observed["exhaustive"] == true
            && observed["drift"] == false
            && rows(&observed)?.len() == entities.len(),
        "Independent asset readback differs",
    )?;
    for authored in entities {
        let id = authored["id"].as_str().ok_or("Entity identity")?;
        let row = entity(&observed, id)?;
        check(
            row["vertices"].as_u64()
                == authored["shape"]["vertices"]
                    .as_array()
                    .map(|v| v.len() as u64)
                && row["polygons"].as_u64()
                    == authored["shape"]["faces"]
                        .as_array()
                        .map(|v| v.len() as u64),
            "Native asset topology differs from source specification",
        )?;
    }
    let whole = proof
        .op(
            "unmanaged-readback",
            "driver.blender.composition.inspect",
            json!({}),
        )
        .await?;
    let unmanaged: Vec<_> = rows(&whole)?
        .iter()
        .filter(|v| v["entity"].is_null())
        .cloned()
        .collect();
    check(
        Value::Array(unmanaged) == baseline["items"],
        "Unmanaged factory state changed",
    )?;
    let library = proof
        .op(
            "persist-library",
            "driver.blender.composition.persist",
            json!({"island":island,"path":artifact("asset-library.blend")}),
        )
        .await?;
    let library_bytes = read(&workspace.join(artifact("asset-library.blend")))?;
    check(
        library["sha256"] == sha256(&library_bytes),
        "Library bytes disagree",
    )?;
    let closed = proof
        .op(
            "checkpoint-close",
            "driver.blender.session.checkpoint_close",
            json!({"checkpoint_path":artifact("asset-scene.blend")}),
        )
        .await?;
    check(
        closed["closed"] == true && closed["close_state"] == "closed",
        "Asset checkpoint close failed",
    )?;
    let saved = read(&workspace.join(artifact("asset-scene.blend")))?;
    let saved_hash = sha256(&saved);
    check(
        closed["checkpoint"]["sha256"] == saved_hash,
        "Scene checkpoint hash differs",
    )?;
    let reopened = proof
        .op(
            "reopen",
            "driver.blender.session.open",
            json!({"path":artifact("asset-scene.blend"),"expected_sha256":saved_hash}),
        )
        .await?;
    check(
        reopened["fresh_session"] == true && reopened["use_scripts"] == false,
        "Unsafe asset reopen",
    )?;
    let recovered = proof
        .op(
            "reopened-readback",
            "driver.blender.composition.inspect",
            json!({"island":island}),
        )
        .await?;
    check(
        recovered["native_session"] != observed["native_session"]
            && recovered["items"] == observed["items"]
            && recovered["fingerprint"] == observed["fingerprint"],
        "Saved asset changed across restart",
    )?;
    let collection = rows(&recovered)?[0]["collections"][0]
        .as_str()
        .ok_or("Asset collection absent")?;
    let export = proof
        .op(
            "export",
            "driver.blender.export.glb",
            json!({"collection":collection,"path":artifact("asset.glb"),"animations":false}),
        )
        .await?;
    let bytes = read(&workspace.join(artifact("asset.glb")))?;
    check(
        (20..=16 * 1024 * 1024).contains(&bytes.len()),
        "Asset GLB size budget",
    )?;
    let word = |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    check(
        &bytes[..4] == b"glTF"
            && word(4) == 2
            && word(8) as usize == bytes.len()
            && word(16) == 0x4e4f534a,
        "Asset GLB header",
    )?;
    let end = 20usize
        .checked_add(word(12) as usize)
        .ok_or("Asset GLB length overflow")?;
    check(end <= bytes.len(), "Asset GLB JSON bounds")?;
    let document: Value = serde_json::from_slice(&bytes[20..end]).map_err(|e| e.to_string())?;
    check(
        document["meshes"]
            .as_array()
            .is_some_and(|v| v.len() == entities.len()),
        "Asset GLB mesh count",
    )?;
    for field in ["buffers", "images"] {
        if let Some(items) = document[field].as_array() {
            check(
                items.iter().all(|item| item.get("uri").is_none()),
                "Asset external GLB URI",
            )?;
        }
    }
    check(
        export["sha256"] == sha256(&bytes) && export["bytes"].as_u64() == Some(bytes.len() as u64),
        "Export receipt disagrees with independent GLB bytes",
    )?;
    Ok(
        json!({"version":version,"island":island,"plan":planned,"apply":applied,
        "independent_readback":observed,"recovered_readback":recovered,"library":library,
        "checkpoint":closed,"reopen":reopened,"export":export,
        "glb":{"sha256":sha256(&bytes),"bytes":bytes.len(),"document":document},
        "unmanaged_fixture_preservation":"PASS","human_edit_preservation_claimed":false}),
    )
}
#[tokio::test]
#[ignore = "Owner-authorized static original asset production through typed Windows broker operations"]
async fn windows_production_broker_static_asset() {
    let pipe =
        PathBuf::from(std::env::var_os("SEMWRIGHT_TEST_BROKER_PIPE").expect("Explicit pipe"));
    let receipts =
        PathBuf::from(std::env::var_os("SEMWRIGHT_TEST_RECEIPTS").expect("New receipts"));
    let workspace = PathBuf::from(std::env::var_os("SEMWRIGHT_TEST_WORKSPACE").expect("Workspace"));
    let prefix =
        std::env::var("SEMWRIGHT_TEST_ARTIFACT_PREFIX").expect("Relative artifact directory");
    assert!(
        !prefix.is_empty()
            && prefix.len() <= 48
            && prefix
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
    );
    let spec_path =
        PathBuf::from(std::env::var_os("SEMWRIGHT_ASSET_SPEC").expect("Explicit typed asset spec"));
    let expected = std::env::var("SEMWRIGHT_ASSET_SPEC_SHA256").expect("Bound input hash");
    let bytes = read(&spec_path).expect("Read typed spec");
    assert!(spec_path.is_absolute() && bytes.len() <= 786432 && sha256(&bytes) == expected);
    let spec: Value = serde_json::from_slice(&bytes).expect("Typed specification JSON");
    fs::create_dir(&receipts).expect("Preserve prior receipts");
    let client = Client::connect(&pipe, None)
        .await
        .expect("Authenticated client");
    let mut proof = SessionProof {
        client,
        receipts,
        trace: Vec::new(),
    };
    let work = match tokio::time::timeout(
        Duration::from_secs(210),
        static_asset_exercise(&mut proof, &workspace, &prefix, &spec),
    )
    .await
    {
        Ok(value) => value,
        Err(_) => Err("Asset deadline; unknown outcome, no replay".into()),
    };
    let inspection = proof
        .exchange(
            "cleanup-inspect",
            "driver.blender.session.checkpoint.inspect",
            json!({}),
            None,
            Duration::from_secs(10),
        )
        .await;
    let close = if inspection.as_ref().is_ok_and(|v| v["supported"] == true) {
        proof
            .exchange(
                "final-close",
                "driver.blender.session.checkpoint_close",
                json!({"checkpoint_path":format!("{prefix}/final-recovery.blend")}),
                None,
                Duration::from_secs(15),
            )
            .await
    } else {
        Err(format!(
            "No close after unsupported/unknown inspection: {inspection:?}"
        ))
    };
    let close_ack = close
        .as_ref()
        .is_ok_and(|v| v["closed"] == true && v["close_state"] == "closed");
    let passed = work.is_ok() && close_ack;
    save_json(&proof.receipts.join("receipt.json"),&json!({"schema":1,
        "proof_kind":"original_static_asset_broker_production","broker_used":true,"passed":passed,
        "spec_path":spec_path,"spec_sha256":expected,"requests":proof.trace,
        "result":work.as_ref().ok(),"error":work.as_ref().err(),
        "close":close.as_ref().ok(),"close_error":close.as_ref().err(),"close_acknowledged":close_ack,
        "art_approval":false,"human_edit_preservation_claimed":false,"G6":"NOT_COMPLETE",
        "Linux_4_5_14":"NOT_RUN","Godot_import":"NOT_RUN"})).expect("Preserve asset receipt");
    assert!(passed, "Inspect preserved static asset production receipt");
}

async fn plan_output_budget_exercise(
    proof: &mut SessionProof,
    workspace: &Path,
    prefix: &str,
) -> ProofResult<Value> {
    let version = proof
        .op(
            "version-before-rejection",
            "driver.blender.introspect.summary",
            json!({}),
        )
        .await?;
    check(
        version["version"] == json!([5, 2, 1]) && version["arbitrary_python"] == false,
        "Unexpected native runtime",
    )?;
    let before = proof
        .op(
            "native-before-rejection",
            "driver.blender.composition.inspect",
            json!({}),
        )
        .await?;
    let spec: Value =
        serde_json::from_str(include_str!("fixtures/authoring_plan_output_budget.json"))
            .map_err(|e| e.to_string())?;
    let args = json!({"intent":{"kind":"create","spec":spec}});
    semwright_types::json_budget::value_budget(&args).map_err(|e| e.to_string())?;
    let rejected = proof
        .exchange(
            "oversized-plan",
            "driver.blender.composition.plan",
            args,
            Some(ErrorCode::ResourceExhausted),
            Duration::from_secs(30),
        )
        .await?;
    check(rejected.is_null(), "Rejected plan exposed result authority")?;
    let envelope: Value = serde_json::from_slice(&read(
        &proof.receipts.join("02-oversized-plan.envelope.json"),
    )?)
    .map_err(|e| e.to_string())?;
    check(
        envelope["execution"]["backend"] == "driver:blender"
            && envelope["execution"]["policy_decision"] == "allow"
            && envelope["execution"]["provenance"]["source"] == "driver",
        "Rejection did not traverse the qualified first-party driver",
    )?;
    check(
        envelope["error"]["message"]
            .as_str()
            .is_some_and(|m| m.starts_with("Driver reported an error;")),
        "Rejection must originate in the driver, not the post-dispatch registry budget",
    )?;
    let after = proof
        .op(
            "native-after-rejection",
            "driver.blender.composition.inspect",
            json!({}),
        )
        .await?;
    check(
        after == before,
        "Rejected plan changed native scene or native session",
    )?;
    // Same client and driver: a valid typed plan must still apply and read back before any restart.
    let valid = static_asset_exercise(proof, workspace, prefix, &static_spec()).await?;
    check(
        valid["independent_readback"]["native_session"] == before["native_session"],
        "Valid follow-up did not execute in the native session that rejected the oversized output",
    )?;
    Ok(
        json!({"version":version,"rejected_request":envelope["request_id"],
        "driver_origin_rejection":true,"rejected_result":rejected,"native_before":before,"native_after":after,
        "valid_followup":valid,"same_native_session_until_apply_and_readback":true,
        "zero_vault_or_cache_publication":"Covered by runtime unit regression; not directly observed through broker"}),
    )
}
#[tokio::test]
#[ignore = "Physical bounded-plan preflight regression in the production isolated Windows broker"]
async fn windows_production_broker_plan_output_budget() {
    let pipe =
        PathBuf::from(std::env::var_os("SEMWRIGHT_TEST_BROKER_PIPE").expect("Explicit pipe"));
    let receipts =
        PathBuf::from(std::env::var_os("SEMWRIGHT_TEST_RECEIPTS").expect("New receipts"));
    let workspace = PathBuf::from(std::env::var_os("SEMWRIGHT_TEST_WORKSPACE").expect("Workspace"));
    let prefix = std::env::var("SEMWRIGHT_TEST_ARTIFACT_PREFIX").expect("Artifact prefix");
    assert!(
        !prefix.is_empty()
            && prefix.len() <= 48
            && prefix
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
    );
    fs::create_dir(&receipts).expect("Preserve previous evidence");
    let client = Client::connect(&pipe, None)
        .await
        .expect("Official authenticated memory-only client");
    let mut proof = SessionProof {
        client,
        receipts,
        trace: vec![],
    };
    let work = match tokio::time::timeout(
        Duration::from_secs(210),
        plan_output_budget_exercise(&mut proof, &workspace, &prefix),
    )
    .await
    {
        Ok(v) => v,
        Err(_) => Err("Unknown outcome; no automatic replay".into()),
    };
    let inspection = proof
        .exchange(
            "cleanup-inspect",
            "driver.blender.session.checkpoint.inspect",
            json!({}),
            None,
            Duration::from_secs(10),
        )
        .await;
    let close = if inspection.as_ref().is_ok_and(|v| v["supported"] == true) {
        proof
            .exchange(
                "final-close",
                "driver.blender.session.checkpoint_close",
                json!({"checkpoint_path":format!("{prefix}/final-recovery.blend")}),
                None,
                Duration::from_secs(15),
            )
            .await
    } else {
        Err("No close after unsupported/unknown checkpoint inspection".into())
    };
    let close_ack = close
        .as_ref()
        .is_ok_and(|v| v["closed"] == true && v["close_state"] == "closed");
    let passed = work.is_ok() && close_ack;
    save_json(
        &proof.receipts.join("receipt.json"),
        &json!({"schema":1,"passed":passed,
        "proof_kind":"native_plan_output_preflight_and_valid_continuation","requests":proof.trace,
        "result":work.as_ref().ok(),"error":work.as_ref().err(),"close":close.as_ref().ok(),
        "close_error":close.as_ref().err(),"close_acknowledged":close_ack,
        "broker_used":true,"human_edit_preservation_claimed":false,"art_status":"REVISE",
        "G6":"NOT_COMPLETE","Linux_4_5_14":"NOT_RUN"}),
    )
    .expect("Preserve receipt");
    assert!(passed, "Inspect preserved plan-output-budget receipt");
}
