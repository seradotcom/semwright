//! These tests exercise the PRODUCTION Rust client/driver against an independent Python fixture.
//! They must run with a real Rust toolchain; Python fixture self-tests are not substitutes.
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
            .expect("Python fixture dependencies must be installed");
        let mut reader = BufReader::new(child.stdout.take().expect("fixture stdout"));
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut line))
            .await
            .unwrap()
            .unwrap();
        let port = serde_json::from_str::<Value>(&line).unwrap()["port"]
            .as_u64()
            .unwrap() as u16;
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

#[tokio::test]
async fn connection_status_and_version() {
    let f = Fixture::start("normal", 1000).await;
    f.client.ready().await.unwrap();
    assert_eq!(f.client.health().await["connected"], true);
    assert!(f.read("GetVersion").await.unwrap()["availableRequests"].is_array());
    f.close().await;
}
#[tokio::test]
async fn authentication_real_rust_client() {
    let f = Fixture::start("auth_required", 1000).await;
    f.client.ready().await.unwrap();
    assert_eq!(f.client.health().await["authenticated"], true);
    f.close().await;
}
#[tokio::test]
async fn authentication_failure_is_permanent() {
    let f = Fixture::start("auth_fail", 1000).await;
    assert_eq!(
        f.client.ready().await.unwrap_err().kind,
        FaultKind::Authentication
    );
    assert_eq!(f.client.health().await["reconnect_attempts"], 1);
    f.close().await;
}
#[tokio::test]
async fn wrong_rpc_rejected() {
    let f = Fixture::start("wrong_rpc_version", 1000).await;
    assert_eq!(
        f.client.ready().await.unwrap_err().kind,
        FaultKind::Protocol
    );
    f.close().await;
}
#[tokio::test]
async fn malformed_hello_rejected() {
    let f = Fixture::start("malformed_hello", 1000).await;
    assert!(f.client.ready().await.is_err());
    f.close().await;
}
#[tokio::test]
async fn not_ready_is_not_authentication_failure() {
    let f = Fixture::start("not_ready", 1000).await;
    f.client.ready().await.unwrap();
    assert_eq!(
        f.read("GetStats").await.unwrap_err().kind,
        FaultKind::NotReady
    );
    f.close().await;
}
#[tokio::test]
async fn unsupported_request_not_sent() {
    let f = Fixture::start("unsupported", 1000).await;
    f.client.ready().await.unwrap();
    assert_eq!(
        f.read("GetStats").await.unwrap_err().kind,
        FaultKind::Unsupported
    );
    f.close().await;
}
#[tokio::test]
async fn error_text_does_not_leak() {
    let f = Fixture::start("request_error", 1000).await;
    f.client.ready().await.unwrap();
    let error = f.read("GetStats").await.unwrap_err();
    assert_eq!(error.kind, FaultKind::Application);
    assert!(!format!("{error:?}").contains("CREDENTIAL"));
    f.close().await;
}
#[tokio::test]
async fn fifty_concurrent_reads_are_correlated() {
    let f = Fixture::start("out_of_order_response", 3000).await;
    f.client.ready().await.unwrap();
    let mut tasks = Vec::new();
    for ordinal in 0..50 {
        let c = f.client.clone();
        tasks.push(tokio::spawn(async move {
            let v = c
                .request(
                    "GetStats",
                    json!({"fixtureOrdinal":ordinal}),
                    false,
                    None,
                    CancellationToken::new(),
                )
                .await
                .unwrap();
            assert_eq!(v["cpuUsage"].as_f64().unwrap(), ordinal as f64);
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    f.close().await;
}
#[tokio::test]
async fn unknown_response_id_cannot_complete_request() {
    let f = Fixture::start("unknown_request_id", 1000).await;
    f.client.ready().await.unwrap();
    let v = f.read("GetStats").await.unwrap();
    assert_ne!(v["cpuUsage"], 9999);
    assert!(
        f.client.health().await["event_metrics"]["unknown_responses"]
            .as_u64()
            .unwrap()
            >= 1
    );
    f.close().await;
}
#[tokio::test]
async fn duplicate_response_ignored() {
    let f = Fixture::start("duplicate_response", 1000).await;
    f.client.ready().await.unwrap();
    f.read("GetStats").await.unwrap();
    f.read("GetStats").await.unwrap();
    assert!(
        f.client.health().await["event_metrics"]["unknown_responses"]
            .as_u64()
            .unwrap()
            >= 1
    );
    f.close().await;
}
#[tokio::test]
async fn response_old_generation_ignored() {
    let f = Fixture::start("old_response", 1000).await;
    f.client.ready().await.unwrap();
    assert_ne!(f.read("GetStats").await.unwrap()["cpuUsage"], 9999);
    f.close().await;
}
#[tokio::test]
async fn wrong_response_type_is_error() {
    let f = Fixture::start("wrong_response_type", 1000).await;
    f.client.ready().await.unwrap();
    assert_eq!(
        f.read("GetStats").await.unwrap_err().kind,
        FaultKind::Protocol
    );
    f.close().await;
}
#[tokio::test]
async fn timeout_then_late_response_never_satisfies_new_request() {
    let f = Fixture::start("late_response", 100).await;
    f.client.ready().await.unwrap();
    assert_eq!(
        f.read("GetStats").await.unwrap_err().kind,
        FaultKind::Timeout
    );
    tokio::time::sleep(Duration::from_millis(350)).await;
    assert_eq!(
        f.read("GetStats").await.unwrap_err().kind,
        FaultKind::Timeout
    );
    assert!(
        f.client.health().await["event_metrics"]["unknown_responses"]
            .as_u64()
            .unwrap()
            >= 1
    );
    f.close().await;
}
#[tokio::test]
async fn cancellation_aborts_wait_not_application_operation() {
    let f = Fixture::start("delayed_response", 1000).await;
    f.client.ready().await.unwrap();
    let token = CancellationToken::new();
    let c = f.client.clone();
    let t = token.clone();
    let task = tokio::spawn(async move { c.request("GetStats", json!({}), false, None, t).await });
    tokio::time::sleep(Duration::from_millis(30)).await;
    token.cancel();
    assert_eq!(task.await.unwrap().unwrap_err().kind, FaultKind::Cancelled);
    f.close().await;
}
#[tokio::test]
async fn shutdown_cancels_pending_requests() {
    let f = Fixture::start("delayed_response", 1000).await;
    f.client.ready().await.unwrap();
    let c = f.client.clone();
    let task = tokio::spawn(async move {
        c.request("GetStats", json!({}), false, None, CancellationToken::new())
            .await
    });
    tokio::time::sleep(Duration::from_millis(30)).await;
    f.client.shutdown().await.unwrap();
    assert!(task.await.unwrap().is_err());
    f.client.shutdown().await.unwrap();
    f.close().await;
}
#[tokio::test]
async fn event_flood_is_bounded_and_marks_gap() {
    let f = Fixture::start("event_flood", 5000).await;
    f.client.ready().await.unwrap();
    f.read("GetStats").await.unwrap();
    let health = f.client.health().await;
    assert_eq!(health["event_queue_length"], 8);
    assert_eq!(health["event_metrics"]["received"], 1500);
    assert_eq!(health["event_metrics"]["dropped"], 1492);
    assert_eq!(health["cache_stale"], true);
    f.close().await;
}
#[tokio::test]
async fn malformed_event_disconnects_and_invalidates() {
    let f = Fixture::start("malformed_event", 1000).await;
    f.client.ready().await.unwrap();
    assert!(f.read("GetStats").await.is_err());
    assert!(
        f.client.health().await["event_metrics"]["malformed"]
            .as_u64()
            .unwrap()
            >= 1
    );
    f.close().await;
}
#[tokio::test]
async fn huge_websocket_message_rejected() {
    let f = Fixture::start("huge_response", 1000).await;
    f.client.ready().await.unwrap();
    assert!(f.read("GetStats").await.is_err());
    f.close().await;
}
#[tokio::test]
async fn binary_frame_rejected() {
    let f = Fixture::start("binary_frame", 1000).await;
    f.client.ready().await.unwrap();
    assert_eq!(
        f.read("GetStats").await.unwrap_err().kind,
        FaultKind::Protocol
    );
    f.close().await;
}
#[tokio::test]
async fn duplicate_json_keys_on_wire_rejected() {
    let f = Fixture::start("duplicate_json_key", 1000).await;
    f.client.ready().await.unwrap();
    assert!(f.read("GetStats").await.is_err());
    f.close().await;
}
#[tokio::test]
async fn deep_json_on_wire_rejected() {
    let f = Fixture::start("deep_json", 1000).await;
    f.client.ready().await.unwrap();
    assert!(f.read("GetStats").await.is_err());
    f.close().await;
}
#[tokio::test]
async fn mutation_disconnect_has_unknown_outcome() {
    let f = Fixture::start("disconnect_after_request", 1000).await;
    f.client.ready().await.unwrap();
    let result = f
        .client
        .request(
            "SetInputMute",
            json!({"inputUuid":"00000000-0000-0000-0000-000000000065","inputMuted":true}),
            true,
            None,
            CancellationToken::new(),
        )
        .await;
    assert!(!result.unwrap_err().outcome_known);
    f.close().await;
}
#[tokio::test]
async fn generation_increments_on_reconnect() {
    let f = Fixture::start("generation_change", 1000).await;
    f.client.ready().await.unwrap();
    let old = f.client.stamp().await;
    assert!(f.read("GetSceneList").await.is_err());
    tokio::time::sleep(Duration::from_millis(250)).await;
    f.client.ready().await.unwrap();
    assert!(f.client.stamp().await.generation > old.generation);
    f.read("GetSceneList").await.unwrap();
    f.close().await;
}
#[tokio::test]
async fn reconnect_storm_bounded() {
    let f = Fixture::start("reconnect_storm", 1000).await;
    assert!(f.client.ready().await.is_err());
    assert!(
        f.client.health().await["reconnect_attempts"]
            .as_u64()
            .unwrap()
            <= 2
    );
    f.close().await;
}
#[tokio::test]
async fn batch_halt_partial_failure() {
    let f = Fixture::start("batch_partial", 1000).await;
    f.client.ready().await.unwrap();
    let rows = vec![("GetStats".into(), json!({})); 3];
    let v = f
        .client
        .batch_read(rows, true, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(v["results"].as_array().unwrap().len(), 2);
    f.close().await;
}
#[tokio::test]
async fn batch_continue_partial_failure() {
    let f = Fixture::start("batch_partial", 1000).await;
    f.client.ready().await.unwrap();
    let rows = vec![("GetStats".into(), json!({})); 3];
    let v = f
        .client
        .batch_read(rows, false, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(v["results"].as_array().unwrap().len(), 3);
    f.close().await;
}
#[tokio::test]
async fn reidentify_updates_actual_subscription_state() {
    let f = Fixture::start("normal", 1000).await;
    f.client.ready().await.unwrap();
    f.client.reidentify(0).await.unwrap();
    assert_eq!(f.client.health().await["subscriptions"], 0);
    assert_eq!(
        f.client
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
    f.close().await;
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
async fn curated_scene_change_and_stale_ref_rejection() {
    let f = Fixture::start("normal", 1000).await;
    f.client.ready().await.unwrap();
    let d = ObsDriver::new(f.client.clone(), f.config.clone()).unwrap();
    let scenes = invoke(&d, "scene.list", json!({})).await.unwrap();
    let reference = scenes["data"]["scenes"][1]["ref"].clone();
    let generation = scenes["generation"].clone();
    invoke(
        &d,
        "scene.current.set",
        json!({"scene_ref":reference,"expected_generation":generation}),
    )
    .await
    .unwrap();
    let error = invoke(
        &d,
        "scene.remove",
        json!({"scene_ref":reference,"expected_generation":generation}),
    )
    .await
    .unwrap_err();
    assert_eq!(error.kind, FaultKind::StaleReference);
    f.close().await;
}
#[tokio::test]
async fn curated_input_mute_precondition() {
    let f = Fixture::start("normal", 1000).await;
    f.client.ready().await.unwrap();
    let d = ObsDriver::new(f.client.clone(), f.config.clone()).unwrap();
    let inputs = invoke(&d, "input.list", json!({})).await.unwrap();
    let reference = inputs["data"]["inputs"][0]["ref"].clone();
    let generation = inputs["generation"].clone();
    let error=invoke(&d,"input.mute.set",json!({"input_ref":reference,"muted":true,"expected_muted":true,"expected_generation":generation})).await.unwrap_err();
    assert_eq!(error.kind, FaultKind::Precondition);
    invoke(&d,"input.mute.set",json!({"input_ref":reference,"muted":true,"expected_muted":false,"expected_generation":generation})).await.unwrap();
    f.close().await;
}
#[tokio::test]
async fn curated_record_operation_event_before_ack() {
    let f = Fixture::start("event_before_response", 1000).await;
    f.client.ready().await.unwrap();
    let d = ObsDriver::new(f.client.clone(), f.config.clone()).unwrap();
    let generation = f.client.stamp().await.generation;
    let r = invoke(
        &d,
        "record.start",
        json!({"expected_generation":generation,"expected_active":false}),
    )
    .await
    .unwrap();
    assert_eq!(r["data"]["accepted"], true);
    assert_eq!(r["data"]["state"], "active");
    let op = invoke(
        &d,
        "operations.get",
        json!({"operation_ref":r["data"]["operation"]}),
    )
    .await
    .unwrap();
    assert_eq!(op["data"]["phase"], "active");
    invoke(
        &d,
        "record.stop",
        json!({"expected_generation":generation,"expected_active":true}),
    )
    .await
    .unwrap();
    f.close().await;
}
#[tokio::test]
async fn curated_stream_start_disabled_by_default() {
    let f = Fixture::start("normal", 1000).await;
    f.client.ready().await.unwrap();
    let d = ObsDriver::new(f.client.clone(), f.config.clone()).unwrap();
    let generation = f.client.stamp().await.generation;
    let error = invoke(
        &d,
        "stream.start",
        json!({"expected_generation":generation,"expected_active":false}),
    )
    .await
    .unwrap_err();
    assert_eq!(error.kind, FaultKind::Precondition);
    f.close().await;
}
#[tokio::test]
async fn curated_descriptor_digest_must_match() {
    let f = Fixture::start("normal", 1000).await;
    let d = ObsDriver::new(f.client.clone(), f.config.clone()).unwrap();
    assert_eq!(
        d.invoke("driver.obs.status", &"0".repeat(64), json!({}))
            .await
            .unwrap_err()
            .kind,
        FaultKind::StaleReference
    );
    f.close().await;
}
#[tokio::test]
async fn mixed_reads_and_mutations() {
    let f = Fixture::start("normal", 3000).await;
    f.client.ready().await.unwrap();
    let mut tasks = Vec::new();
    for i in 0..50 {
        let c = f.client.clone();
        tasks.push(tokio::spawn(async move {
            if i % 10 == 0 {
                c.request(
                    "SetInputMute",
                    json!({"inputUuid":"00000000-0000-0000-0000-000000000065","inputMuted":true}),
                    true,
                    None,
                    CancellationToken::new(),
                )
                .await
            } else {
                c.request("GetStats", json!({}), false, None, CancellationToken::new())
                    .await
            }
        }));
    }
    for t in tasks {
        t.await.unwrap().unwrap();
    }
    f.close().await;
}
#[tokio::test]
async fn disconnect_mid_concurrency_does_not_leave_waiters() {
    let f = Fixture::start("disconnect_mid_concurrency", 1000).await;
    f.client.ready().await.unwrap();
    let mut tasks = Vec::new();
    for _ in 0..50 {
        let c = f.client.clone();
        tasks.push(tokio::spawn(async move {
            c.request("GetStats", json!({}), false, None, CancellationToken::new())
                .await
        }));
    }
    for t in tasks {
        assert!(
            tokio::time::timeout(Duration::from_secs(2), t)
                .await
                .unwrap()
                .unwrap()
                .is_err()
        );
    }
    f.close().await;
}

#[tokio::test]
async fn anonymous_session_does_not_claim_password_authentication() {
    let f = Fixture::start("normal", 1000).await;
    f.client.ready().await.unwrap();
    assert_eq!(f.client.health().await["authenticated"], false);
    f.close().await;
}
#[tokio::test]
async fn disconnected_health_does_not_claim_negotiated_rpc() {
    let config = Config {
        port: 1,
        connect_timeout_ms: 100,
        reconnect_limit: 1,
        ..Config::default()
    };
    let c = Client::start(config, None).unwrap();
    c.shutdown().await.unwrap();
    let h = c.health().await;
    assert_eq!(h["rpc_version"], Value::Null);
    assert_eq!(h["authenticated"], false);
    assert_eq!(h["subscriptions"], 0);
}
