use futures_util::{SinkExt, StreamExt};
use semwright_driver_sdk::{Driver, DriverChildEvent};
use semwright_godot_driver::{
    GodotDriver,
    bridge::{proof, transcript},
    catalog::Catalog,
    config::{Config, ProjectConfig},
};
use serde_json::{Value, json};
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[tokio::test]
async fn production_execute_crosses_authenticated_websocket() {
    let dir = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    std::fs::write(dir.path().join("project.godot"), "config_version=5\n").unwrap();

    let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = socket.local_addr().unwrap().port();
    drop(socket);

    let project = "a".repeat(64);
    let secret_hex = "b".repeat(64);
    let config = Config {
        port,
        development_mode: true,
        projects: vec![ProjectConfig {
            project: project.clone(),
            root: dir.path().canonicalize().unwrap(),
            secret: secret_hex.clone(),
        }],
        runner: None,
    };
    let mut driver = GodotDriver::new(config).await.unwrap();
    let mut events = driver.take_events().expect("Godot event receiver");
    let fake = tokio::spawn(fake_editor(port, project.clone(), secret_hex));

    let sessions = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let sessions = driver.bridge.list().await;
            if !sessions.is_empty() {
                break sessions;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let session = sessions[0].session.clone();
    let event = tokio::time::timeout(std::time::Duration::from_secs(2), events.recv())
        .await
        .expect("Godot child event timeout")
        .expect("Godot child event channel closed");
    match event {
        DriverChildEvent::Event { kind, payload } => {
            assert_eq!(kind, "godot.scene_changed");
            assert_eq!(payload["project"], project);
            assert_eq!(payload["session"], session);
            assert_eq!(payload["revision"], 1);
            assert_eq!(payload["data"]["path"], "res://Main.tscn");
        }
        other => panic!("unexpected Godot child event: {other:?}"),
    }
    let catalog = Catalog::load().unwrap();

    let inspect = catalog.get("driver.godot.project.inspect").unwrap();
    let before = driver
        .execute(
            &inspect.capability.descriptor.name,
            &inspect.digest,
            json!({"session": session}),
        )
        .await
        .unwrap();
    assert_eq!(before["data"]["engine"], "4.7.2");
    assert_eq!(before["stamp"]["revision"], 1);

    let create = catalog.get("driver.godot.scene.create").unwrap();
    let after = driver
        .execute(
            &create.capability.descriptor.name,
            &create.digest,
            json!({
                "session": sessions[0].session,
                "path": "res://Lab.tscn",
                "name": "Lab",
                "class": "Node3D",
                "expect": before["stamp"],
                "dry_run": false
            }),
        )
        .await
        .unwrap();
    assert_eq!(after["applied"], true);
    assert_eq!(after["stamp"]["revision"], 2);
    fake.await.unwrap();
}

async fn fake_editor(port: u16, project: String, secret_hex: String) {
    let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}"))
        .await
        .unwrap();
    let client_nonce = "c".repeat(32);
    send(
        &mut ws,
        json!({
            "type":"hello",
            "project":project,
            "nonce":client_nonce,
            "plugin_version":"0.1.0",
            "engine_version":"4.7.2"
        }),
    )
    .await;
    let challenge = recv(&mut ws).await;
    assert_eq!(challenge["type"], "challenge");
    let server_nonce = challenge["nonce"].as_str().unwrap();
    let generation = challenge["generation"].as_str().unwrap();
    let session = challenge["session"].as_str().unwrap();
    let secret = hex::decode(secret_hex).unwrap();

    let server = transcript(
        "server",
        &project,
        &client_nonce,
        server_nonce,
        generation,
        session,
    );
    assert_eq!(challenge["proof"], proof(&secret, &server).unwrap());
    let client = transcript(
        "client",
        &project,
        &client_nonce,
        server_nonce,
        generation,
        session,
    );
    send(
        &mut ws,
        json!({"type":"authenticate","proof":proof(&secret,&client).unwrap()}),
    )
    .await;
    assert_eq!(recv(&mut ws).await["type"], "ready");
    send(
        &mut ws,
        json!({
            "type":"event",
            "kind":"scene_changed",
            "revision":1,
            "payload":{"path":"res://Main.tscn"}
        }),
    )
    .await;

    let first = recv(&mut ws).await;
    assert_eq!(first["op"], "project.inspect");
    send(
        &mut ws,
        json!({
            "type":"response","id":first["id"],"ok":true,
            "value":{
                "stamp":{"revision":1,"fingerprint":"d".repeat(64)},
                "data":{"name":"Fixture","engine":"4.7.2","scene":"res://Main.tscn","unsaved":false}
            }
        }),
    )
    .await;

    let second = recv(&mut ws).await;
    assert_eq!(second["op"], "scene.create");
    assert_eq!(second["args"]["expect"]["revision"], 1);
    send(
        &mut ws,
        json!({
            "type":"response","id":second["id"],"ok":true,
            "value":{
                "applied":true,
                "stamp":{"revision":2,"fingerprint":"e".repeat(64)},
                "affected":["res://Lab.tscn"],
                "undo":"Create scene"
            }
        }),
    )
    .await;
}

#[tokio::test]
async fn logical_session_resumes_and_waiting_call_crosses_reconnect_gap() {
    let dir = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    std::fs::write(
        dir.path().join("project.godot"),
        "config_version=5
",
    )
    .unwrap();

    let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = socket.local_addr().unwrap().port();
    drop(socket);

    let project = "1".repeat(64);
    let secret_hex = "2".repeat(64);
    let config = Config {
        port,
        development_mode: true,
        projects: vec![ProjectConfig {
            project: project.clone(),
            root: dir.path().canonicalize().unwrap(),
            secret: secret_hex.clone(),
        }],
        runner: None,
    };
    let driver = GodotDriver::new(config).await.unwrap();

    let (mut first, session, generation_one) =
        authenticate_editor(port, &project, &secret_hex, None).await;
    let active = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let sessions = driver.bridge.list().await;
            if sessions.len() == 1 {
                break sessions;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(active[0].session, session);

    first.close(None).await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let continuity = driver.bridge.continuity().await;
            if continuity.connected_sessions == 0 && continuity.reconnecting_sessions == 1 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();

    let bridge = driver.bridge.clone();
    let waiting_session = session.clone();
    let waiting = tokio::spawn(async move {
        bridge
            .call(
                &waiting_session,
                "project.inspect",
                json!({}),
                std::time::Duration::from_secs(3),
            )
            .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let (mut resumed, resumed_session, generation_two) =
        authenticate_editor(port, &project, &secret_hex, Some(&session)).await;
    assert_eq!(resumed_session, session);
    assert_ne!(generation_two, generation_one);

    let request = recv(&mut resumed).await;
    assert_eq!(request["type"], "request");
    assert_eq!(request["op"], "project.inspect");
    send(
        &mut resumed,
        json!({
            "type":"response",
            "id":request["id"],
            "ok":true,
            "value":{"resumed":true}
        }),
    )
    .await;

    let result = tokio::time::timeout(std::time::Duration::from_secs(2), waiting)
        .await
        .expect("waiting Godot request did not wake after reconnect")
        .unwrap()
        .unwrap();
    assert_eq!(result["resumed"], true);

    let continuity = driver.bridge.continuity().await;
    assert_eq!(continuity.connected_sessions, 1);
    assert_eq!(continuity.reconnecting_sessions, 0);
    assert_eq!(continuity.reconnects, 1);

    tokio::time::sleep(
        semwright_godot_driver::bridge::RECONNECT_GRACE + std::time::Duration::from_millis(150),
    )
    .await;
    let sessions = driver.bridge.list().await;
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].session, session);
    assert_eq!(sessions[0].generation, generation_two);

    resumed.close(None).await.unwrap();
}

async fn authenticate_editor(
    port: u16,
    project: &str,
    secret_hex: &str,
    resume_session: Option<&str>,
) -> (
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    String,
    String,
) {
    let (mut ws, _) = connect_async(format!("ws://127.0.0.1:{port}"))
        .await
        .unwrap();
    let client_nonce = "3".repeat(32);
    let mut hello = json!({
        "type":"hello",
        "project":project,
        "nonce":client_nonce,
        "plugin_version":"0.2.0",
        "engine_version":"4.7.2"
    });
    if let Some(session) = resume_session {
        hello["resume_session"] = Value::String(session.to_owned());
    }
    send(&mut ws, hello).await;
    let challenge = recv(&mut ws).await;
    assert_eq!(challenge["type"], "challenge");
    let server_nonce = challenge["nonce"].as_str().unwrap();
    let generation = challenge["generation"].as_str().unwrap().to_owned();
    let session = challenge["session"].as_str().unwrap().to_owned();
    let secret = hex::decode(secret_hex).unwrap();
    let server = transcript(
        "server",
        project,
        &client_nonce,
        server_nonce,
        &generation,
        &session,
    );
    assert_eq!(challenge["proof"], proof(&secret, &server).unwrap());
    let client = transcript(
        "client",
        project,
        &client_nonce,
        server_nonce,
        &generation,
        &session,
    );
    send(
        &mut ws,
        json!({"type":"authenticate","proof":proof(&secret,&client).unwrap()}),
    )
    .await;
    let ready = recv(&mut ws).await;
    assert_eq!(ready["type"], "ready");
    (ws, session, generation)
}

async fn send(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    value: Value,
) {
    ws.send(Message::Text(value.to_string().into()))
        .await
        .unwrap();
}

async fn recv(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> Value {
    let msg = ws.next().await.unwrap().unwrap();
    serde_json::from_slice(&msg.into_data()).unwrap()
}
