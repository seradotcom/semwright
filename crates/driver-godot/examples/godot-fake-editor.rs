use futures_util::{SinkExt, StreamExt};
use semwright_godot_driver::bridge::{proof, transcript};
use serde_json::{Value, json};
use std::{error::Error as StdError, time::Duration};
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

fn required_arg(name: &str) -> Result<String, Box<dyn StdError>> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == name {
            return args
                .next()
                .ok_or_else(|| format!("missing value for {name}").into());
        }
    }
    Err(format!("missing argument {name}").into())
}

async fn send(ws: &mut Ws, value: Value) -> Result<(), Box<dyn StdError>> {
    ws.send(Message::Text(value.to_string().into())).await?;
    Ok(())
}

async fn recv(ws: &mut Ws) -> Result<Value, Box<dyn StdError>> {
    loop {
        match ws.next().await.ok_or("bridge closed")?? {
            Message::Text(text) => return Ok(serde_json::from_str(&text)?),
            Message::Binary(bytes) => return Ok(serde_json::from_slice(&bytes)?),
            Message::Ping(_) | Message::Pong(_) => continue,
            Message::Close(_) => return Err("bridge closed".into()),
            _ => continue,
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn StdError>> {
    let port: u16 = required_arg("--port")?.parse()?;
    let project = required_arg("--project")?;
    let secret_hex = required_arg("--secret")?;
    let secret = hex::decode(&secret_hex)?;
    let client_nonce = "c".repeat(32);

    let url = format!("ws://127.0.0.1:{port}");
    let mut last_error = None;
    let mut socket = None;
    for _ in 0..200 {
        match connect_async(&url).await {
            Ok((ws, _)) => {
                socket = Some(ws);
                break;
            }
            Err(error) => {
                last_error = Some(error);
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        }
    }
    let mut ws = socket.ok_or_else(|| {
        format!(
            "failed to connect to Godot bridge: {}",
            last_error
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| "unknown error".into())
        )
    })?;

    send(
        &mut ws,
        json!({
            "type":"hello",
            "project":project,
            "nonce":client_nonce,
            "plugin_version":"0.1.0-broker-smoke",
            "engine_version":"4.7.2-stable (broker-fixture)"
        }),
    )
    .await?;

    let challenge = recv(&mut ws).await?;
    if challenge["type"] != "challenge" {
        return Err("expected challenge".into());
    }
    let server_nonce = challenge["nonce"].as_str().ok_or("missing server nonce")?;
    let generation = challenge["generation"]
        .as_str()
        .ok_or("missing generation")?;
    let session = challenge["session"].as_str().ok_or("missing session")?;
    let server_transcript = transcript(
        "server",
        &project,
        &client_nonce,
        server_nonce,
        generation,
        session,
    );
    let expected_server = proof(&secret, &server_transcript)
        .map_err(|error| format!("server proof failed: {:?}", error.code))?;
    if challenge["proof"].as_str() != Some(expected_server.as_str()) {
        return Err("server bridge proof mismatch".into());
    }

    let client_transcript = transcript(
        "client",
        &project,
        &client_nonce,
        server_nonce,
        generation,
        session,
    );
    let client_proof = proof(&secret, &client_transcript)
        .map_err(|error| format!("client proof failed: {:?}", error.code))?;
    send(&mut ws, json!({"type":"authenticate","proof":client_proof})).await?;
    if recv(&mut ws).await?["type"] != "ready" {
        return Err("expected ready".into());
    }

    send(
        &mut ws,
        json!({
            "type":"event",
            "kind":"scene_changed",
            "revision":1,
            "payload":{"source":"broker-smoke"}
        }),
    )
    .await?;

    loop {
        let frame = match recv(&mut ws).await {
            Ok(frame) => frame,
            Err(_) => break,
        };
        if frame["type"] != "request" {
            continue;
        }
        let id = frame["id"].clone();
        let op = frame["op"].as_str().unwrap_or("");
        match op {
            "project.inspect" => {
                send(
                    &mut ws,
                    json!({
                        "type":"response",
                        "id":id,
                        "ok":true,
                        "value":{
                            "stamp":{"revision":1,"fingerprint":"d".repeat(64)},
                            "data":{
                                "name":"Broker fixture",
                                "engine":"4.7.2-stable (broker-fixture)",
                                "scene":"",
                                "unsaved":false
                            }
                        }
                    }),
                )
                .await?;
            }
            _ => {
                send(
                    &mut ws,
                    json!({
                        "type":"response",
                        "id":id,
                        "ok":false,
                        "error":{
                            "code":"Unsupported",
                            "message":"broker smoke fake supports project.inspect only"
                        }
                    }),
                )
                .await?;
            }
        }
    }
    Ok(())
}
