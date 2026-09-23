//! Production Rust client/driver against an independent Python obs-websocket fixture.
use semwright_obs_driver::{FaultKind, ObsDriver, auth::Secret, client::Client, config::Config};
use serde_json::{Value, json};
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
};
use tokio_util::sync::CancellationToken;

struct Fixture {
    child: Child,
    client: Client,
    config: Config,
}
impl Fixture {
    async fn start(mode: &str, request_ms: u64) -> Self {
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/fake-obs/server.py");
        let mut child = Command::new("python3")
            .arg(script)
            .args(["--mode", mode, "--delay", "0.3", "--event-count", "1500"])
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .expect("Python fake OBS must start");
        let mut reader = BufReader::new(child.stdout.take().expect("fixture stdout"));
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut line))
            .await
            .expect("fixture startup timeout")
            .expect("fixture startup I/O");
        let port = serde_json::from_str::<Value>(&line).expect("fixture JSON")["port"]
            .as_u64()
            .expect("fixture port") as u16;
        let config = Config {
            port,
            request_timeout_ms: request_ms,
            event_capacity: 8,
            reconnect_limit: 2,
            ..Config::default()
        };
        let secret = if matches!(mode, "auth_required" | "auth_fail") {
            Some(Secret::new(b"OBS-FIXTURE-ONLY-NOT-A-REAL-CREDENTIAL".to_vec()).unwrap())
        } else {
            None
        };
        let client = Client::start(config.clone(), secret).unwrap();
        Self {
            child,
            client,
            config,
        }
    }
    async fn read(&self, kind: &str) -> semwright_obs_driver::Result<Value> {
        self.client
            .request(kind, json!({}), false, None, CancellationToken::new())
            .await
    }
    async fn close(mut self) {
        let _ = self.client.shutdown().await;
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
    }
}
async fn invoke(
    driver: &ObsDriver,
    command: &str,
    args: Value,
) -> semwright_obs_driver::Result<Value> {
    let name = format!("driver.obs.{command}");
    let digest = driver.catalog().get(&name).unwrap().digest.clone();
    driver.invoke(&name, &digest, args).await
}

#[tokio::test]
#[ignore = "requires independent Python obs-websocket fixture"]
async fn connection_status_and_version() {
    let fixture = Fixture::start("normal", 1000).await;
    fixture.client.ready().await.unwrap();
    assert_eq!(fixture.client.health().await["connected"], true);
    assert!(fixture.read("GetVersion").await.unwrap()["availableRequests"].is_array());
    fixture.close().await;
}
#[tokio::test]
#[ignore = "requires independent Python obs-websocket fixture"]
async fn authentication_real_rust_client() {
    let fixture = Fixture::start("auth_required", 1000).await;
    fixture.client.ready().await.unwrap();
    assert_eq!(fixture.client.health().await["authenticated"], true);
    fixture.close().await;
}
#[tokio::test]
#[ignore = "requires independent Python obs-websocket fixture"]
async fn authentication_failure_is_permanent() {
    let fixture = Fixture::start("auth_fail", 1000).await;
    assert_eq!(
        fixture.client.ready().await.unwrap_err().kind,
        FaultKind::Authentication
    );
    assert_eq!(fixture.client.health().await["reconnect_attempts"], 1);
    fixture.close().await;
}
#[tokio::test]
#[ignore = "requires independent Python obs-websocket fixture"]
async fn malformed_or_wrong_rpc_hello_is_rejected() {
    for mode in ["wrong_rpc_version", "malformed_hello"] {
        let fixture = Fixture::start(mode, 1000).await;
        assert!(fixture.client.ready().await.is_err());
        fixture.close().await;
    }
}
#[tokio::test]
#[ignore = "requires independent Python obs-websocket fixture"]
async fn unsupported_is_rejected_before_send() {
    let fixture = Fixture::start("unsupported", 1000).await;
    fixture.client.ready().await.unwrap();
    assert_eq!(
        fixture.read("GetStats").await.unwrap_err().kind,
        FaultKind::Unsupported
    );
    fixture.close().await;
}
#[tokio::test]
#[ignore = "requires independent Python obs-websocket fixture"]
async fn upstream_error_text_does_not_leak() {
    let fixture = Fixture::start("request_error", 1000).await;
    fixture.client.ready().await.unwrap();
    let error = fixture.read("GetStats").await.unwrap_err();
    assert_eq!(error.kind, FaultKind::Application);
    assert!(!format!("{error:?}").contains("CREDENTIAL"));
    fixture.close().await;
}
#[tokio::test]
#[ignore = "requires independent Python obs-websocket fixture"]
async fn fifty_concurrent_reads_remain_correlated() {
    let fixture = Fixture::start("out_of_order_response", 3000).await;
    fixture.client.ready().await.unwrap();
    let mut tasks = Vec::new();
    for ordinal in 0..50 {
        let client = fixture.client.clone();
        tasks.push(tokio::spawn(async move {
            let value = client
                .request(
                    "GetStats",
                    json!({"fixtureOrdinal":ordinal}),
                    false,
                    None,
                    CancellationToken::new(),
                )
                .await
                .unwrap();
            assert_eq!(value["cpuUsage"].as_f64().unwrap(), ordinal as f64);
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    fixture.close().await;
}
#[tokio::test]
#[ignore = "requires independent Python obs-websocket fixture"]
async fn unknown_and_duplicate_responses_do_not_complete_future_calls() {
    for mode in ["unknown_request_id", "duplicate_response", "old_response"] {
        let fixture = Fixture::start(mode, 1000).await;
        fixture.client.ready().await.unwrap();
        let value = fixture.read("GetStats").await.unwrap();
        assert_ne!(value["cpuUsage"], 9999);
        if mode != "old_response" {
            let mut observed = false;
            for _ in 0..20 {
                if fixture.client.health().await["event_metrics"]["unknown_responses"]
                    .as_u64()
                    .unwrap()
                    >= 1
                {
                    observed = true;
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            assert!(observed, "{mode} response was not accounted as unknown");
        }
        fixture.close().await;
    }
}
#[tokio::test]
#[ignore = "requires independent Python obs-websocket fixture"]
async fn wrong_response_type_is_protocol_error() {
    let fixture = Fixture::start("wrong_response_type", 1000).await;
    fixture.client.ready().await.unwrap();
    assert_eq!(
        fixture.read("GetStats").await.unwrap_err().kind,
        FaultKind::Protocol
    );
    fixture.close().await;
}
#[tokio::test]
#[ignore = "requires independent Python obs-websocket fixture"]
async fn timeout_then_late_response_stays_isolated() {
    let fixture = Fixture::start("late_response", 100).await;
    fixture.client.ready().await.unwrap();
    assert_eq!(
        fixture.read("GetStats").await.unwrap_err().kind,
        FaultKind::Timeout
    );
    tokio::time::sleep(Duration::from_millis(350)).await;
    assert_eq!(
        fixture.read("GetStats").await.unwrap_err().kind,
        FaultKind::Timeout
    );
    assert!(
        fixture.client.health().await["event_metrics"]["unknown_responses"]
            .as_u64()
            .unwrap()
            >= 1
    );
    fixture.close().await;
}
#[tokio::test]
#[ignore = "requires independent Python obs-websocket fixture"]
async fn cooperative_internal_cancellation_releases_waiter() {
    let fixture = Fixture::start("delayed_response", 1000).await;
    fixture.client.ready().await.unwrap();
    let token = CancellationToken::new();
    let client = fixture.client.clone();
    let child_token = token.clone();
    let task = tokio::spawn(async move {
        client
            .request("GetStats", json!({}), false, None, child_token)
            .await
    });
    tokio::time::sleep(Duration::from_millis(30)).await;
    token.cancel();
    assert_eq!(task.await.unwrap().unwrap_err().kind, FaultKind::Cancelled);
    fixture.close().await;
}
#[tokio::test]
#[ignore = "requires independent Python obs-websocket fixture"]
async fn shutdown_cancels_pending_requests() {
    let fixture = Fixture::start("delayed_response", 1000).await;
    fixture.client.ready().await.unwrap();
    let client = fixture.client.clone();
    let task = tokio::spawn(async move {
        client
            .request("GetStats", json!({}), false, None, CancellationToken::new())
            .await
    });
    tokio::time::sleep(Duration::from_millis(30)).await;
    fixture.client.shutdown().await.unwrap();
    assert!(task.await.unwrap().is_err());
    fixture.client.shutdown().await.unwrap();
    fixture.close().await;
}
#[tokio::test]
#[ignore = "requires independent Python obs-websocket fixture"]
async fn event_flood_is_bounded_and_marks_gap() {
    let fixture = Fixture::start("event_flood", 5000).await;
    fixture.client.ready().await.unwrap();
    fixture.read("GetStats").await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let health = fixture.client.health().await;
    assert_eq!(health["event_queue_length"], 8);
    assert_eq!(health["event_metrics"]["received"], 1500);
    assert_eq!(health["event_metrics"]["dropped"], 1492);
    assert_eq!(health["cache_stale"], true);
    fixture.close().await;
}
#[tokio::test]
#[ignore = "requires independent Python obs-websocket fixture"]
async fn malformed_and_oversized_wire_data_fail_closed() {
    for mode in [
        "malformed_event",
        "huge_response",
        "binary_frame",
        "duplicate_json_key",
        "deep_json",
    ] {
        let fixture = Fixture::start(mode, 1000).await;
        fixture.client.ready().await.unwrap();
        assert!(fixture.read("GetStats").await.is_err(), "{mode}");
        fixture.close().await;
    }
}
#[tokio::test]
#[ignore = "requires independent Python obs-websocket fixture"]
async fn mutation_disconnect_has_unknown_outcome() {
    let fixture = Fixture::start("disconnect_after_request", 1000).await;
    fixture.client.ready().await.unwrap();
    let result = fixture
        .client
        .request(
            "SetInputMute",
            json!({
                "inputUuid":"00000000-0000-0000-0000-000000000065",
                "inputMuted":true
            }),
            true,
            None,
            CancellationToken::new(),
        )
        .await;
    assert!(!result.unwrap_err().outcome_known);
    fixture.close().await;
}
#[tokio::test]
#[ignore = "requires independent Python obs-websocket fixture"]
async fn generation_increments_on_reconnect() {
    let fixture = Fixture::start("generation_change", 1000).await;
    fixture.client.ready().await.unwrap();
    let old = fixture.client.stamp().await;
    assert!(fixture.read("GetSceneList").await.is_err());
    tokio::time::sleep(Duration::from_millis(250)).await;
    fixture.client.ready().await.unwrap();
    assert!(fixture.client.stamp().await.generation > old.generation);
    fixture.read("GetSceneList").await.unwrap();
    fixture.close().await;
}
#[tokio::test]
#[ignore = "requires independent Python obs-websocket fixture"]
async fn batches_preserve_declared_partial_failure_semantics() {
    for (halt, expected) in [(true, 2), (false, 3)] {
        let fixture = Fixture::start("batch_partial", 1000).await;
        fixture.client.ready().await.unwrap();
        let rows = vec![("GetStats".into(), json!({})); 3];
        let value = fixture
            .client
            .batch_read(rows, halt, CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(value["results"].as_array().unwrap().len(), expected);
        fixture.close().await;
    }
}
#[tokio::test]
#[ignore = "requires independent Python obs-websocket fixture"]
async fn reidentify_updates_subscription_and_mutation_precondition() {
    let fixture = Fixture::start("normal", 1000).await;
    fixture.client.ready().await.unwrap();
    fixture.client.reidentify(0).await.unwrap();
    assert_eq!(fixture.client.health().await["subscriptions"], 0);
    assert_eq!(
        fixture
            .client
            .request(
                "StartRecord",
                json!({}),
                true,
                None,
                CancellationToken::new()
            )
            .await
            .unwrap_err()
            .kind,
        FaultKind::Precondition
    );
    fixture.close().await;
}

#[tokio::test]
#[ignore = "requires independent Python obs-websocket fixture"]
async fn curated_scene_change_invalidates_old_ref() {
    let fixture = Fixture::start("normal", 1000).await;
    fixture.client.ready().await.unwrap();
    let driver = ObsDriver::new(fixture.client.clone(), fixture.config.clone()).unwrap();
    let scenes = invoke(&driver, "scene.list", json!({})).await.unwrap();
    let reference = scenes["data"]["scenes"][1]["ref"].clone();
    let generation = scenes["generation"].clone();
    invoke(
        &driver,
        "scene.current.set",
        json!({"scene_ref":reference,"expected_generation":generation}),
    )
    .await
    .unwrap();
    let error = invoke(
        &driver,
        "scene.remove",
        json!({"scene_ref":reference,"expected_generation":generation}),
    )
    .await
    .unwrap_err();
    assert_eq!(error.kind, FaultKind::StaleReference);
    fixture.close().await;
}
#[tokio::test]
#[ignore = "requires independent Python obs-websocket fixture"]
async fn curated_input_mute_uses_precondition() {
    let fixture = Fixture::start("normal", 1000).await;
    fixture.client.ready().await.unwrap();
    let driver = ObsDriver::new(fixture.client.clone(), fixture.config.clone()).unwrap();
    let inputs = invoke(&driver, "input.list", json!({})).await.unwrap();
    let reference = inputs["data"]["inputs"][0]["ref"].clone();
    let generation = inputs["generation"].clone();
    assert_eq!(
        invoke(
            &driver,
            "input.mute.set",
            json!({
                "input_ref":reference,
                "muted":true,
                "expected_muted":true,
                "expected_generation":generation
            }),
        )
        .await
        .unwrap_err()
        .kind,
        FaultKind::Precondition
    );
    invoke(
        &driver,
        "input.mute.set",
        json!({
            "input_ref":reference,
            "muted":true,
            "expected_muted":false,
            "expected_generation":generation
        }),
    )
    .await
    .unwrap();
    fixture.close().await;
}
#[tokio::test]
#[ignore = "requires independent Python obs-websocket fixture"]
async fn curated_record_tracks_event_before_ack() {
    let fixture = Fixture::start("event_before_response", 1000).await;
    fixture.client.ready().await.unwrap();
    let driver = ObsDriver::new(fixture.client.clone(), fixture.config.clone()).unwrap();
    let generation = fixture.client.stamp().await.generation;
    let result = invoke(
        &driver,
        "record.start",
        json!({"expected_generation":generation,"expected_active":false}),
    )
    .await
    .unwrap();
    assert_eq!(result["data"]["accepted"], true);
    assert_eq!(result["data"]["state"], "active");
    let operation = invoke(
        &driver,
        "operations.get",
        json!({"operation_ref":result["data"]["operation"]}),
    )
    .await
    .unwrap();
    assert_eq!(operation["data"]["phase"], "active");
    fixture.close().await;
}
#[tokio::test]
#[ignore = "requires independent Python obs-websocket fixture"]
async fn curated_stream_start_is_disabled_by_default() {
    let fixture = Fixture::start("normal", 1000).await;
    fixture.client.ready().await.unwrap();
    let driver = ObsDriver::new(fixture.client.clone(), fixture.config.clone()).unwrap();
    let generation = fixture.client.stamp().await.generation;
    assert_eq!(
        invoke(
            &driver,
            "stream.start",
            json!({"expected_generation":generation,"expected_active":false}),
        )
        .await
        .unwrap_err()
        .kind,
        FaultKind::Precondition
    );
    fixture.close().await;
}
#[tokio::test]
#[ignore = "requires independent Python obs-websocket fixture"]
async fn descriptor_digest_must_match() {
    let fixture = Fixture::start("normal", 1000).await;
    let driver = ObsDriver::new(fixture.client.clone(), fixture.config.clone()).unwrap();
    assert_eq!(
        driver
            .invoke("driver.obs.status", &"0".repeat(64), json!({}))
            .await
            .unwrap_err()
            .kind,
        FaultKind::StaleReference
    );
    fixture.close().await;
}
