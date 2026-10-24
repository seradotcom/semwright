//! Protocol regressions against the real compiled Driver and SDK dispatcher.
//! This peer simulates Host replies only. It never starts Blender or a runtime tool.
use semwright_driver_blender::authoring::NativeSnapshot;
use semwright_driver_sdk::{
    Capability, DriverRequestContext, Request, Response, RuntimeToolSession, descriptor_digest,
};
use semwright_protocol::{read_frame, write_frame};
use semwright_semantic_composition::Digest;
use semwright_types::{Error, ErrorCode, Result};
use serde_json::{Value, json};
use std::{collections::BTreeMap, process::Stdio, time::Duration};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReplyMode {
    Normal,
    MalformedVersion,
    HostTimeoutDuringInspect,
    NativeTimeoutDuringInspect,
    HostTimeoutAfterFirstApply,
}

struct ProtocolFixture {
    child: Child,
    input: ChildStdin,
    output: ChildStdout,
    capabilities: BTreeMap<String, Capability>,
    mode: ReplyMode,
    request_count: usize,
    active: Option<RuntimeToolSession>,
    starts: usize,
    closes: usize,
    apply_calls: usize,
    confirmed_synthetic_operations: usize,
    finish_calls: usize,
}

impl ProtocolFixture {
    async fn start(mode: ReplyMode) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_semwright-blender-driver"))
            .env("SEMWRIGHT_DRIVER_HOST_TOOLS", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut fixture = Self {
            input: child.stdin.take().unwrap(),
            output: child.stdout.take().unwrap(),
            child,
            capabilities: BTreeMap::new(),
            mode,
            request_count: 0,
            active: None,
            starts: 0,
            closes: 0,
            apply_calls: 0,
            confirmed_synthetic_operations: 0,
            finish_calls: 0,
        };
        let hello: Request = serde_json::from_value(json!({
            "type": "hello",
            "protocol": 8,
            "provider": {
                "id": "driver:blender", "kind": "driver",
                "version": env!("CARGO_PKG_VERSION"),
                "namespace": "driver.blender.", "application": "org.blender.Blender",
                "origin": "driver-manifest:deadline-protocol-fixture"
            },
            "executable_sha256": "a".repeat(64)
        }))
        .unwrap();
        fixture.send(hello).await;
        assert!(matches!(
            fixture.receive().await,
            Response::Ready { protocol: 8, .. }
        ));
        fixture
            .send(Request::Capabilities {
                id: "catalog".into(),
            })
            .await;
        let Response::Capabilities {
            id, capabilities, ..
        } = fixture.receive().await
        else {
            panic!("Driver did not return its real catalog");
        };
        assert_eq!(id, "catalog");
        fixture.capabilities = capabilities
            .into_iter()
            .map(|capability| (capability.descriptor.name.clone(), capability))
            .collect();
        fixture
    }

    async fn send(&mut self, request: Request) {
        tokio::time::timeout(
            Duration::from_secs(5),
            write_frame(&mut self.input, &request),
        )
        .await
        .expect("bounded protocol fixture write")
        .unwrap();
    }

    async fn receive(&mut self) -> Response {
        tokio::time::timeout(Duration::from_secs(5), read_frame(&mut self.output))
            .await
            .expect("bounded protocol fixture read")
            .unwrap()
    }

    async fn frame(&mut self, id: String, session: RuntimeToolSession, value: Value) {
        self.send(Request::ToolSessionFrame {
            id,
            session,
            payload: serde_json::to_vec(&value).unwrap(),
        })
        .await;
    }

    fn snapshot() -> Value {
        serde_json::to_value(NativeSnapshot {
            native_session: "synthetic-native-boot".into(),
            island: None,
            fingerprint: Digest::of_bytes(b"synthetic-before"),
            drift: false,
            total: 0,
            items: vec![],
            source_only: true,
            exhaustive: true,
        })
        .unwrap()
    }

    async fn execute(&mut self, suffix: &str, args: Value) -> Result<Value> {
        let command = format!("driver.blender.composition.{suffix}");
        let digest = descriptor_digest(&self.capabilities[&command].descriptor).unwrap();
        self.request_count += 1;
        let request_id = format!("deadline-fixture-{}", self.request_count);
        self.send(Request::Execute {
            id: request_id.clone(),
            command,
            descriptor_sha256: digest,
            args,
            context: Some(DriverRequestContext {
                session: "deadline-fixture-owner".into(),
                native_target: None,
            }),
        })
        .await;
        for _ in 0..128 {
            match self.receive().await {
                Response::ToolSessionStartV8 {
                    id, parent, name, ..
                } => {
                    assert_eq!(parent, request_id);
                    assert_eq!(name, "blender-session-runner");
                    assert!(self.active.is_none(), "unexpected second live session");
                    self.starts += 1;
                    let session = RuntimeToolSession {
                        id: format!("fixture-session-{}", self.starts),
                    };
                    self.active = Some(session.clone());
                    self.send(Request::ToolSessionStarted { id, session }).await;
                }
                Response::ToolSessionRequest {
                    id,
                    parent,
                    session,
                    payload,
                    timeout_ms,
                } => {
                    assert_eq!(parent, request_id);
                    assert!((1..=52_000).contains(&timeout_ms));
                    assert_eq!(self.active.as_ref(), Some(&session));
                    let packet: Value = serde_json::from_slice(&payload).unwrap();
                    assert_eq!(packet.as_object().unwrap().len(), 2);
                    let operation = packet["command"].as_str().unwrap();
                    let result = match operation {
                        "driver.blender.introspect.summary" => {
                            let version = if self.mode == ReplyMode::MalformedVersion {
                                json!([4, 5])
                            } else {
                                json!([4, 5, 14])
                            };
                            json!({"version": version, "version_string": "4.5.14 LTS"})
                        }
                        "driver.blender._authoring.snapshot" => {
                            if self.mode == ReplyMode::HostTimeoutDuringInspect {
                                self.mode = ReplyMode::Normal;
                                self.active = None;
                                self.send(Request::ToolSessionFailure {
                                    id,
                                    error: Error::new(ErrorCode::Timeout, "synthetic Host timeout"),
                                })
                                .await;
                                continue;
                            }
                            if self.mode == ReplyMode::NativeTimeoutDuringInspect {
                                self.mode = ReplyMode::Normal;
                                self.frame(
                                    id,
                                    session,
                                    json!({
                                        "ok": false, "error": {"code": "Timeout"}
                                    }),
                                )
                                .await;
                                continue;
                            }
                            Self::snapshot()
                        }
                        "driver.blender._authoring.begin" => json!({}),
                        "driver.blender._authoring.apply" => {
                            self.apply_calls += 1;
                            if self.mode == ReplyMode::HostTimeoutAfterFirstApply
                                && self.apply_calls == 2
                            {
                                self.active = None;
                                self.send(Request::ToolSessionFailure {
                                    id,
                                    error: Error::new(ErrorCode::Timeout, "synthetic Host timeout"),
                                })
                                .await;
                                continue;
                            }
                            self.confirmed_synthetic_operations += 1;
                            json!({})
                        }
                        "driver.blender._authoring.finish" => {
                            self.finish_calls += 1;
                            panic!("fixture must stop after the injected second-operation failure");
                        }
                        _ => panic!("unexpected bounded native command: {operation}"),
                    };
                    self.frame(id, session, json!({"ok": true, "data": result}))
                        .await;
                }
                Response::ToolSessionClose {
                    id,
                    parent,
                    session,
                } => {
                    assert_eq!(parent, request_id);
                    self.closes += 1;
                    if self.active.as_ref() == Some(&session) {
                        self.active = None;
                        self.send(Request::ToolSessionClosed { id, session }).await;
                    } else {
                        // Real Host already reaps a failed exchange. A later close can
                        // report NotFound; the Driver must not retry that cleanup.
                        self.send(Request::ToolSessionFailure {
                            id,
                            error: Error::new(
                                ErrorCode::NotFound,
                                "synthetic already-reaped handle",
                            ),
                        })
                        .await;
                    }
                }
                Response::Progress { id, .. } => assert_eq!(id, request_id),
                Response::Result { id, value } => {
                    assert_eq!(id, request_id);
                    return Ok(value);
                }
                Response::Failure { id, error } => {
                    assert_eq!(id, request_id);
                    return Err(error);
                }
                other => panic!("unexpected Driver protocol response: {other:?}"),
            }
        }
        panic!("fixture exceeded its bounded response count");
    }

    async fn session_count(&mut self) -> u64 {
        self.send(Request::Health {
            id: "health".into(),
        })
        .await;
        let Response::Healthy { id, details } = self.receive().await else {
            panic!("expected health response");
        };
        assert_eq!(id, "health");
        details["sessions"].as_u64().unwrap()
    }

    async fn shutdown(mut self) {
        self.send(Request::Shutdown {
            id: "shutdown".into(),
        })
        .await;
        assert!(matches!(self.receive().await, Response::Shutdown { .. }));
        let status = tokio::time::timeout(Duration::from_secs(5), self.child.wait())
            .await
            .expect("bounded protocol fixture shutdown")
            .unwrap();
        assert!(status.success());
    }
}

#[tokio::test]
async fn malformed_summary_closes_once_before_failure_and_is_never_cached() {
    let mut fixture = ProtocolFixture::start(ReplyMode::MalformedVersion).await;
    let error = fixture.execute("inspect", json!({})).await.unwrap_err();
    assert_eq!(error.code, ErrorCode::ProtocolMismatch);
    assert!(error.outcome_known);
    assert_eq!(fixture.starts, 1);
    assert_eq!(fixture.closes, 1);
    assert_eq!(fixture.apply_calls, 0);
    assert_eq!(fixture.session_count().await, 0);
    fixture.shutdown().await;
}

#[tokio::test]
async fn host_timeout_discards_handle_and_a_new_read_gets_a_new_session() {
    let mut fixture = ProtocolFixture::start(ReplyMode::HostTimeoutDuringInspect).await;
    let error = fixture.execute("inspect", json!({})).await.unwrap_err();
    assert_eq!(error.code, ErrorCode::Timeout);
    assert_eq!(fixture.closes, 1);
    assert_eq!(fixture.session_count().await, 0);
    fixture.execute("inspect", json!({})).await.unwrap();
    assert_eq!(fixture.starts, 2);
    assert_eq!(fixture.closes, 1);
    assert_eq!(fixture.apply_calls, 0);
    assert_eq!(fixture.session_count().await, 1);
    fixture.shutdown().await;
}

#[tokio::test]
async fn typed_native_timeout_preserves_the_live_transport_handle() {
    let mut fixture = ProtocolFixture::start(ReplyMode::NativeTimeoutDuringInspect).await;
    let error = fixture.execute("inspect", json!({})).await.unwrap_err();
    assert_eq!(error.code, ErrorCode::Timeout);
    assert_eq!(fixture.closes, 0);
    assert_eq!(fixture.session_count().await, 1);
    fixture.execute("inspect", json!({})).await.unwrap();
    assert_eq!(fixture.starts, 1);
    assert_eq!(fixture.closes, 0);
    fixture.shutdown().await;
}

#[tokio::test]
async fn failure_after_one_applied_operation_returns_unknown_and_blocks_redispatch() {
    let mut fixture = ProtocolFixture::start(ReplyMode::HostTimeoutAfterFirstApply).await;
    let spec: Value = serde_json::from_str(include_str!(
        "../../../fixtures/blender-authoring/hard_surface.json"
    ))
    .unwrap();
    let plan = fixture
        .execute("plan", json!({"intent": {"kind": "create", "spec": spec}}))
        .await
        .unwrap();
    assert!(
        plan["plan"]["body"]["changes"]["operations"]
            .as_array()
            .unwrap()
            .len()
            >= 2
    );
    let apply_args = json!({"plan_ref": plan["plan_ref"]});
    let error = fixture
        .execute("apply", apply_args.clone())
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Timeout);
    assert!(!error.outcome_known);
    assert_eq!(fixture.confirmed_synthetic_operations, 1);
    assert_eq!(fixture.apply_calls, 2);
    assert_eq!(fixture.finish_calls, 0);
    assert_eq!(fixture.starts, 1);
    assert_eq!(fixture.closes, 1);
    assert_eq!(fixture.session_count().await, 0);

    // Negative fixture probe only: the poisoned Driver must reject before it can
    // send any new Host request. There is no actual native operation to replay.
    let rejected = fixture.execute("apply", apply_args).await.unwrap_err();
    assert_eq!(rejected.code, ErrorCode::Unavailable);
    assert_eq!(fixture.starts, 1);
    assert_eq!(fixture.closes, 1);
    assert_eq!(fixture.apply_calls, 2);
    fixture.shutdown().await;
}
