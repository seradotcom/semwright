//! Physical cancel-before-publication proof. No test delays are injected into Blender.
#![cfg(target_os = "windows")]
use semwright_protocol::{Client, ClientMessage, ServerMessage, read_frame, write_frame};
use semwright_types::{ErrorCode, ExecuteRequest, unique_id};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::windows::fs::MetadataExt,
    path::PathBuf,
    time::{Duration, Instant},
};
fn save(path: &std::path::Path, value: &Value) {
    let mut out = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    out.write_all(&serde_json::to_vec_pretty(value).unwrap())
        .unwrap();
    out.sync_all().unwrap();
}
fn tail(
    path: &std::path::Path,
    offset: u64,
    creation: u64,
) -> Result<(Vec<u8>, Vec<Value>), String> {
    let meta = fs::metadata(path).map_err(|e| e.to_string())?;
    if meta.creation_time() != creation || meta.len() < offset || meta.len() - offset > 1024 * 1024
    {
        return Err("Host log rotated, truncated or exceeded bound".into());
    }
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    let bytes = bytes
        .get(offset as usize..)
        .ok_or("Host log shrank")?
        .to_vec();
    let rows = bytes
        .split(|b| *b == b'\n')
        .filter_map(|line| serde_json::from_slice(line).ok())
        .collect();
    Ok((bytes, rows))
}
#[tokio::test]
#[ignore = "Requires owner-authorized production broker; no native session should be busy"]
async fn windows_production_broker_cancel_during_readiness() {
    let pipe = PathBuf::from(std::env::var_os("SEMWRIGHT_TEST_BROKER_PIPE").unwrap());
    let receipts = PathBuf::from(std::env::var_os("SEMWRIGHT_TEST_RECEIPTS").unwrap());
    let log = PathBuf::from(std::env::var_os("SEMWRIGHT_TEST_BROKER_LOG").unwrap());
    fs::create_dir(&receipts).expect("Preserve prior cancellation receipts");
    let meta = fs::metadata(&log).unwrap();
    let offset = meta.len();
    let creation = meta.creation_time();
    let client = tokio::time::timeout(Duration::from_secs(5), Client::connect(&pipe, None))
        .await
        .unwrap()
        .unwrap();
    let ticket = client.session;
    let (mut input, mut output) = tokio::io::split(client.stream);
    let id = unique_id();
    save(
        &receipts.join("request.json"),
        &json!({"request_id":id,"command":"driver.blender.introspect.summary","args":{}}),
    );
    write_frame(
        &mut output,
        &ClientMessage::Execute {
            id: id.clone(),
            request: ExecuteRequest {
                command: "driver.blender.introspect.summary".into(),
                args: json!({}),
                dry_run: false,
                backend: None,
            },
        },
    )
    .await
    .unwrap();
    let started = Instant::now();
    let reader = async {
        tokio::time::timeout(Duration::from_secs(80), async {
            loop {
                match read_frame::<_, ServerMessage>(&mut input)
                    .await
                    .map_err(|e| e.to_string())?
                {
                    ServerMessage::Result { envelope } if envelope.request_id == id => {
                        return Ok(*envelope);
                    }
                    ServerMessage::Event { .. } => {}
                    ServerMessage::Error { error } => return Err(error.to_string()),
                    _ => return Err("Unexpected result while awaiting cancellation".to_owned()),
                }
            }
        })
        .await
        .map_err(|_| "Cancellation result timeout".to_owned())?
    };
    let canceller = async {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(35);
        loop {
            let (_, rows) = tail(&log, offset, creation)?;
            if let Some(wait) = rows
                .iter()
                .find(|row| row["phase"] == "native_readiness_wait_started")
            {
                let nonce = wait["nonce"]
                    .as_str()
                    .ok_or("Readiness nonce absent")?
                    .to_owned();
                let pid = wait["pid"].as_u64().ok_or("Readiness PID absent")?;
                let ready_already = rows
                    .iter()
                    .any(|row| row["phase"] == "native_blender_ready" && row["nonce"] == nonce);
                write_frame(&mut output, &ClientMessage::Cancel { id: id.clone() })
                    .await
                    .map_err(|e| e.to_string())?;
                return Ok::<Value, String>(json!({"pid":pid,"nonce":nonce,"wait_event":wait,
                    "ready_already_observed":ready_already,"cancel_sent_ms":started.elapsed().as_millis()}));
            }
            if tokio::time::Instant::now() >= deadline {
                write_frame(&mut output, &ClientMessage::Cancel { id: id.clone() })
                    .await
                    .map_err(|e| e.to_string())?;
                return Err("Readiness window not observed; cancellation sent for cleanup".into());
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    };
    let (response, cancel) = tokio::join!(reader, canceller);
    if let Ok(envelope) = &response {
        save(
            &receipts.join("envelope.json"),
            &serde_json::to_value(envelope).unwrap(),
        );
    }
    let mut client = Client {
        stream: input.unsplit(output),
        session: ticket,
    };
    let cleanup = tokio::time::timeout(
        Duration::from_secs(15),
        client.execute(
            unique_id(),
            ExecuteRequest {
                command: "driver.blender.session.checkpoint_close".into(),
                args: json!({"checkpoint_path":"cancel-recovery.blend"}),
                dry_run: false,
                backend: None,
            },
        ),
    )
    .await;
    let cleanup_value = match &cleanup {
        Ok(Ok(envelope)) => serde_json::to_value(envelope).unwrap(),
        _ => json!({"close_acknowledged":false}),
    };
    save(&receipts.join("cleanup-envelope.json"), &cleanup_value);
    let tail_deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let (bytes, rows) = loop {
        let result = tail(&log, offset, creation).expect("Stable Host log");
        let terminal = cancel.as_ref().ok().is_some_and(|c| {
            result.1.iter().any(|r| {
                r["phase"] == "native_readiness_failed"
                    && r["nonce"] == c["nonce"]
                    && r["pid"] == c["pid"]
            })
        });
        if terminal || tokio::time::Instant::now() >= tail_deadline {
            break result;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    fs::write(receipts.join("host-tail.log"), &bytes).unwrap();
    let cancelled_result = response.as_ref().is_ok_and(|e| {
        !e.ok
            && e.error
                .as_ref()
                .is_some_and(|error| error.code == ErrorCode::Cancelled)
    });
    let (entered, terminal, reaped, ready) = if let Ok(c) = &cancel {
        (
            c["wait_event"]["cancelled_at_entry"] == false && c["ready_already_observed"] == false,
            rows.iter().any(|r| {
                r["phase"] == "native_readiness_terminal"
                    && r["nonce"] == c["nonce"]
                    && r["pid"] == c["pid"]
                    && r["reason"] == "Cancelled"
                    && r["parent_cancelled"] == true
            }),
            rows.iter().any(|r| {
                r["phase"] == "native_readiness_failed"
                    && r["nonce"] == c["nonce"]
                    && r["pid"] == c["pid"]
                    && r["exit_observed"] == true
            }),
            rows.iter()
                .any(|r| r["phase"] == "native_blender_ready" && r["nonce"] == c["nonce"]),
        )
    } else {
        (false, false, false, false)
    };
    let no_adoption = cleanup_value["ok"] == true
        && cleanup_value["data"]["closed"] == false
        && cleanup_value["data"]["close_state"] == "no_session"
        && cleanup_value["data"]["checkpoint"].is_null();
    let passed = cancelled_result && entered && terminal && reaped && !ready && no_adoption;
    let report = json!({"schema":1,"passed":passed,"request_id":id,"cancel":cancel.as_ref().ok(),
        "cancel_error":cancel.as_ref().err(),"response_error":response.as_ref().err(),
        "cancelled_result":cancelled_result,"observed_active_wait":entered,
        "host_observed_cancellation":terminal,"host_observed_exit":reaped,"session_not_adopted":no_adoption,
        "ready_after_cancel":ready,"host_log_offset":offset,
        "host_tail_sha256":hex::encode(Sha256::digest(&bytes)),
        "elapsed_ms":started.elapsed().as_millis(),"injected_native_delay":false,
        "interpretation":if passed {"CANCELLED_DURING_READINESS_AND_REAPED"} else {"FAIL_OR_INCONCLUSIVE_REVIEW_LOGS"}});
    save(&receipts.join("receipt.json"), &report);
    eprintln!("BROKER_CANCEL_RECEIPT {}", report);
    assert!(
        passed,
        "Cancellation criterion not met; do not promote inconclusive timing to PASS"
    );
}
