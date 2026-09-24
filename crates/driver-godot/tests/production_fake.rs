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
