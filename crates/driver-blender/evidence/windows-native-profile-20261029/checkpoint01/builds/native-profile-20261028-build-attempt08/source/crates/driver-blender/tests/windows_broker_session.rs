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
fn verify_glb(path: &Path) -> ProofResult<Value> {
    let bytes = read(path)?;
    check(
        (20..=16 * 1024 * 1024).contains(&bytes.len()),
        "GLB size bounds",
    )?;
    let word = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
    check(&bytes[..4] == b"glTF" && word(4) == 2, "GLB 2 header")?;
    check(word(8) as usize == bytes.len(), "GLB declared length")?;
    let end = 20usize
        .checked_add(word(12) as usize)
        .ok_or("GLB length overflow")?;
    check(
        &bytes[16..20] == b"JSON" && end <= bytes.len(),
        "GLB JSON bounds",
    )?;
    let document: Value = serde_json::from_slice(&bytes[20..end]).map_err(|e| e.to_string())?;
    let nodes = document["nodes"].as_array().ok_or("GLB nodes absent")?;
    let meshes = document["meshes"].as_array().ok_or("GLB meshes absent")?;
    check(
        nodes.len() == 1 && meshes.len() == 1,
        "Expected one node and mesh",
    )?;
    let node = &nodes[0];
    check(
        node["name"] == "BrokerProofCube" && node["mesh"] == 0,
        "GLB object/mesh identity",
    )?;
    for (field, expected) in [
        ("translation", [3.0, 5.0, -4.0]),
        ("scale", [1.0, 1.0, 2.0]),
    ] {
        let actual = node[field].as_array().ok_or("GLB explicit TRS absent")?;
        check(actual.len() == 3, "GLB TRS shape")?;
        for (value, expected) in actual.iter().zip(expected) {
            let actual = value.as_f64().ok_or("GLB TRS is not numeric")?;
            check((actual - expected).abs() < 1e-5, "GLB TRS mismatch")?;
        }
    }
    for field in ["buffers", "images"] {
        if let Some(items) = document[field].as_array() {
            check(
                items.iter().all(|item| item.get("uri").is_none()),
                "Unexpected GLB URI",
            )?;
        }
    }
    Ok(
        json!({"bytes":bytes.len(),"sha256":sha256(&bytes),"nodes":1,"meshes":1,
        "translation":node["translation"],"scale":node["scale"],"self_contained":true}),
    )
}
async fn exercise(proof: &mut SessionProof, workspace: &Path) -> ProofResult<Value> {
    let glb = workspace.join("broker-proof.glb");
    let blend = workspace.join("broker-proof.blend");
    let outside = workspace
        .parent()
        .ok_or("Workspace parent missing")?
        .join("forbidden-native-proof.glb");
    for path in [
        &glb,
        &blend,
        &outside,
        &workspace.join("broker-final-recovery.blend"),
    ] {
        require_absent(path)?;
    }
    let summary = proof
        .op("summary", "driver.blender.introspect.summary", json!({}))
        .await?;
    check(
        summary["version"] == json!([5, 2, 1]) && summary["arbitrary_python"] == false,
        "Unexpected native Blender profile",
    )?;
    for (label, command, args) in [
        (
            "collection",
            "driver.blender.collection.create",
            json!({"name":"BrokerProof"}),
        ),
        (
            "object",
            "driver.blender.object.create",
            json!({"name":"BrokerProofCube","primitive":"cube","location":[0,0,0]}),
        ),
        (
            "link",
            "driver.blender.collection.link",
            json!({"object":"BrokerProofCube","collection":"BrokerProof"}),
        ),
        (
            "transform",
            "driver.blender.object.transform",
            json!({"name":"BrokerProofCube","location":[3,4,5],"scale":[1,2,1]}),
        ),
    ] {
        proof.op(label, command, args).await?;
    }
    let observed = proof
        .op(
            "readback",
            "driver.blender.object.get",
            json!({"name":"BrokerProofCube"}),
        )
        .await?;
    check(
        observed["object"]["location"] == json!([3.0, 4.0, 5.0])
            && observed["object"]["scale"] == json!([1.0, 2.0, 1.0]),
        "Independent typed readback mismatch",
    )?;
    let export_args =
        json!({"collection":"BrokerProof","path":"broker-proof.glb","animations":false});
    proof
        .op("export", "driver.blender.export.glb", export_args.clone())
        .await?;
    let glb_receipt = verify_glb(&glb)?;
    let before = sha256(&read(&glb)?);
    proof
        .exchange(
            "deny-overwrite",
            "driver.blender.export.glb",
            export_args,
            Some(ErrorCode::Conflict),
            Duration::from_secs(100),
        )
        .await?;
    check(
        sha256(&read(&glb)?) == before,
        "Rejected export changed GLB",
    )?;
    proof.exchange("deny-traversal","driver.blender.export.glb",
        json!({"collection":"BrokerProof","path":"../forbidden-native-proof.glb","animations":false}),
        Some(ErrorCode::PolicyDenied),Duration::from_secs(100)).await?;
    require_absent(&outside)?;
    let preflight = proof
        .op(
            "checkpoint-inspect",
            "driver.blender.session.checkpoint.inspect",
            json!({}),
        )
        .await?;
    check(
        preflight["supported"] == true,
        &format!("Checkpoint closure unsupported: {preflight}"),
    )?;
    let source_copy = proof
        .op(
            "checkpoint-copy",
            "driver.blender.file.checkpoint",
            json!({"path":"broker-source-copy.blend"}),
        )
        .await?;
    check(
        source_copy["source_fake_users_restored"] == true,
        "Source retention flags not restored",
    )?;
    let original_orphan = proof
        .op(
            "source-orphan",
            "driver.blender.semantic.objects",
            json!({"root":"materials","query":"Dots Stroke","limit":8}),
        )
        .await?;
    let orphan_ref = original_orphan["items"][0]["ref"].clone();
    check(!orphan_ref.is_null(), "Factory orphan material absent")?;
    let original_flag = proof
        .op(
            "source-orphan-flag",
            "driver.blender.semantic.property.get",
            json!({"ref":orphan_ref,"property":"use_fake_user"}),
        )
        .await?;
    check(
        original_flag["value"] == false,
        "Source orphan flag changed by recovery copy",
    )?;
    let closed = proof
        .op(
            "checkpoint-close",
            "driver.blender.session.checkpoint_close",
            json!({"checkpoint_path":"broker-proof.blend"}),
        )
        .await?;
    check(
        closed["closed"] == true && closed["close_state"] == "closed",
        "Checkpoint close did not complete",
    )?;
    let blend_bytes = read(&blend)?;
    let blend_sha = sha256(&blend_bytes);
    check(
        closed["checkpoint"]["sha256"] == blend_sha
            && closed["checkpoint"]["bytes"].as_u64() == Some(blend_bytes.len() as u64),
        "Checkpoint receipt disagrees with physical bytes",
    )?;
    let reopen_args = json!({"path":"broker-proof.blend","expected_sha256":blend_sha});
    let reopened = proof
        .op(
            "reopen-fresh",
            "driver.blender.session.open",
            reopen_args.clone(),
        )
        .await?;
    check(
        reopened["fresh_session"] == true && reopened["use_scripts"] == false,
        "Unsafe reopen receipt",
    )?;
    let recovered_orphan = proof
        .op(
            "recovered-orphan",
            "driver.blender.semantic.objects",
            json!({"root":"materials","query":"Dots Stroke","limit":8}),
        )
        .await?;
    let recovered_ref = recovered_orphan["items"][0]["ref"].clone();
    check(
        !recovered_ref.is_null(),
        "Recovery copy lost the orphan material",
    )?;
    let retained_flag = proof
        .op(
            "recovered-orphan-flag",
            "driver.blender.semantic.property.get",
            json!({"ref":recovered_ref,"property":"use_fake_user"}),
        )
        .await?;
    check(
        retained_flag["value"] == true,
        "Recovery orphan retention flag absent",
    )?;
    let reobserved = proof
        .op(
            "reopen-readback",
            "driver.blender.object.get",
            json!({"name":"BrokerProofCube"}),
        )
        .await?;
    check(
        reobserved == observed,
        "Recovered object state differs after native restart",
    )?;
    proof
        .exchange(
            "deny-replace-live-session",
            "driver.blender.session.open",
            reopen_args,
            Some(ErrorCode::Conflict),
            Duration::from_secs(100),
        )
        .await?;
    proof
        .exchange(
            "deny-checkpoint-overwrite",
            "driver.blender.session.checkpoint_close",
            json!({"checkpoint_path":"broker-proof.blend"}),
            Some(ErrorCode::Conflict),
            Duration::from_secs(100),
        )
        .await?;
    check(
        sha256(&read(&blend)?) == blend_sha,
        "Rejected checkpoint changed original file",
    )?;
    let retained = proof
        .op(
            "retained-readback",
            "driver.blender.object.get",
            json!({"name":"BrokerProofCube"}),
        )
        .await?;
    check(
        retained == observed,
        "Rejected lifecycle operation lost current scene",
    )?;
    Ok(
        json!({"summary":summary,"typed_readback":observed,"glb":glb_receipt,
        "blend":{"bytes":blend_bytes.len(),"sha256":sha256(&blend_bytes)},
        "no_overwrite":"PASS","outside_workspace":"DENIED",
        "recovered_readback":reobserved,"checkpoint":closed["checkpoint"],
        "reopen":reopened,"replace_live_session":"DENIED","checkpoint_overwrite":"DENIED"}),
    )
}

#[tokio::test]
#[ignore = "Requires owner-authorized production Windows broker and fresh workspace"]
async fn windows_production_broker_persistent_typed_fixture() {
    let pipe =
        PathBuf::from(std::env::var_os("SEMWRIGHT_TEST_BROKER_PIPE").expect("Explicit pipe"));
    let receipts =
        PathBuf::from(std::env::var_os("SEMWRIGHT_TEST_RECEIPTS").expect("New receipts"));
    let workspace = PathBuf::from(std::env::var_os("SEMWRIGHT_TEST_WORKSPACE").expect("Workspace"));
    assert!(receipts.is_absolute() && workspace.is_absolute() && workspace.is_dir());
    fs::create_dir(&receipts).expect("Preserve previous receipt directory");
    let started = Instant::now();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(180);
    let client = tokio::time::timeout(Duration::from_secs(5), Client::connect(&pipe, None))
        .await
        .expect("Connect timeout")
        .expect("Authenticated broker connection");
    let mut proof = SessionProof {
        client,
        receipts,
        trace: Vec::new(),
    };
    let budget = deadline
        .saturating_duration_since(tokio::time::Instant::now())
        .saturating_sub(Duration::from_secs(15));
    let work = match tokio::time::timeout(budget, exercise(&mut proof, &workspace)).await {
        Ok(result) => result,
        Err(_) => Err("Overall deadline; outcome UNKNOWN, no retry".into()),
    };
    // Explicit cleanup is attempted once; a transport failure never counts as a close ACK.
    let close_budget = deadline
        .saturating_duration_since(tokio::time::Instant::now())
        .min(Duration::from_secs(15));
    // Observe current closure after any failure before attempting a distinct recovery save.
    // Unsupported/unknown state never triggers a blind second checkpoint mutation.
    let cleanup_inspection = proof
        .exchange(
            "cleanup-inspect",
            "driver.blender.session.checkpoint.inspect",
            json!({}),
            None,
            close_budget,
        )
        .await;
    let close = if cleanup_inspection
        .as_ref()
        .is_ok_and(|v| v["supported"] == true)
    {
        proof
            .exchange(
                "close",
                "driver.blender.session.checkpoint_close",
                json!({"checkpoint_path":"broker-final-recovery.blend"}),
                None,
                close_budget,
            )
            .await
    } else {
        Err(format!(
            "Cleanup declined after read-only inspection: {cleanup_inspection:?}"
        ))
    };
    let close_ack = close
        .as_ref()
        .is_ok_and(|data| data["closed"] == true && data["close_state"] == "closed");
    let passed = work.is_ok() && close_ack;
    let report = json!({"schema":1,"broker_used":true,"client":"semwright_protocol::Client",
        "connection":"single authenticated connection; ticket in memory only",
        "typed_fixture":if work.is_ok() {"PASS"} else {"FAIL"},
        "proof":work.as_ref().ok(),"error":work.as_ref().err(),
        "close_acknowledged":close_ack,"close_error":close.as_ref().err(),
        "elapsed_ms":started.elapsed().as_millis(),"requests":proof.trace,
        "native_attestation":"VERIFY_CORRELATED_HOST_LOG","physical_reap":"VERIFY_HOST_PID",
        "Godot_import":"NOT_RUN","art_approval":false,
        "A05":"BLOCKED_PENDING_GODOT_AND_LIFECYCLE_EVIDENCE"});
    let receipt_sha = save_json(&proof.receipts.join("receipt.json"), &report).expect("Receipt");
    eprintln!(
        "BROKER_PROOF_RECEIPT {}",
        json!({"passed":passed,"receipt_sha256":receipt_sha,"close_ack":close_ack})
    );
    assert!(
        passed,
        "Physical broker fixture failed; inspect preserved receipts"
    );
}

#[tokio::test]
#[ignore = "Read-only native inventory for an owner-authorized production Windows broker"]
async fn windows_production_broker_checkpoint_inventory() {
    let pipe =
        PathBuf::from(std::env::var_os("SEMWRIGHT_TEST_BROKER_PIPE").expect("Explicit pipe"));
    let receipts =
        PathBuf::from(std::env::var_os("SEMWRIGHT_TEST_RECEIPTS").expect("New receipts"));
    fs::create_dir(&receipts).expect("Preserve receipts");
    let client = Client::connect(&pipe, None)
        .await
        .expect("Authenticated broker");
    let mut proof = SessionProof {
        client,
        receipts,
        trace: Vec::new(),
    };
    let result: ProofResult<Vec<Value>> = async {
        let summary = proof
            .op(
                "semantic-summary",
                "driver.blender.semantic.summary",
                json!({}),
            )
            .await?;
        let roots = summary["roots"].as_array().ok_or("Missing roots")?;
        check(roots.len() <= 64, "Root budget")?;
        let mut inventory = Vec::new();
        for entry in roots {
            let root = entry["root"].as_str().ok_or("Missing root")?;
            let data = proof
                .op(
                    &format!("root-{root}"),
                    "driver.blender.semantic.objects",
                    json!({"root":root,"limit":64}),
                )
                .await?;
            check(data["truncated"] == false, "Factory inventory truncated")?;
            let mut items = Vec::new();
            for item in data["items"].as_array().ok_or("Missing items")? {
                let mut row = json!({"name":item["name"],"rna_type":item["rna_type"]});
                for property in ["users", "use_fake_user", "is_embedded_data"] {
                    let value = proof
                        .op(
                            &format!("property-{}", proof.trace.len()),
                            "driver.blender.semantic.property.get",
                            json!({"ref":item["ref"],"property":property}),
                        )
                        .await?;
                    row[property] = value["value"].clone();
                }
                items.push(row);
            }
            if !items.is_empty() {
                eprintln!("FACTORY_INVENTORY {}", json!({"root":root,"items":items}));
            }
            inventory.push(json!({"root":root,"items":items}));
        }
        Ok(inventory)
    }
    .await;
    let report = json!({"read_only":true,"inventory":result.as_ref().ok(),"error":result.as_ref().err(),
        "requests":proof.trace,"native_close":"NOT_ATTEMPTED_UNSUPPORTED_CHECKPOINT_DIAGNOSIS",
        "cleanup":"OWNED_FIXTURE_BROKER_TEARDOWN_REQUIRED","A05":"PARTIAL"});
    save_json(&proof.receipts.join("receipt.json"), &report).expect("Receipt");
    assert!(result.is_ok(), "Read-only inventory incomplete");
}
