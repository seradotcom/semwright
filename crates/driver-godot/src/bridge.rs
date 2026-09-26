use crate::config::{ProjectConfig, is_hex};
use futures_util::{SinkExt, StreamExt};
use hmac::{Hmac, Mac};
use semwright_driver_sdk::DriverChildEvent;
use semwright_types::{Error, ErrorCode, Result};
use serde_json::{Value, json};
use sha2::Sha256;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{collections::HashMap, sync::Arc, time::Duration};
#[cfg(unix)]
use tokio::net::UnixListener;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpListener,
    sync::{RwLock, mpsc, oneshot},
};
use tokio_tungstenite::{accept_async, tungstenite::Message};
use tokio_util::sync::CancellationToken;

type HmacSha256 = Hmac<Sha256>;
const BRIDGE_VERSION: u64 = 1;
const MAX_WIRE_BYTES: usize = 512 * 1024;

#[derive(Clone, Debug, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionInfo {
    pub session: String,
    pub project: String,
    pub generation: String,
    pub plugin_version: String,
    pub engine_version: String,
}

struct Call {
    op: String,
    args: Value,
    reply: oneshot::Sender<Result<Value>>,
}

struct Session {
    info: SessionInfo,
    tx: mpsc::Sender<Call>,
}

#[derive(Clone)]
pub struct Bridge {
    sessions: Arc<RwLock<HashMap<String, Arc<Session>>>>,
    stop: CancellationToken,
}

impl Bridge {
    pub async fn start(
        port: u16,
        projects: Vec<ProjectConfig>,
        events: mpsc::UnboundedSender<DriverChildEvent>,
    ) -> Result<Self> {
        let sessions = Arc::new(RwLock::new(HashMap::new()));
        let stop = CancellationToken::new();
        let bridge = Self {
            sessions: sessions.clone(),
            stop: stop.clone(),
        };
        let projects = Arc::new(
            projects
                .into_iter()
                .map(|p| (p.project.clone(), p))
                .collect::<HashMap<_, _>>(),
        );

        #[cfg(unix)]
        if let Ok(socket) = std::env::var("SEMWRIGHT_DRIVER_LOOPBACK_SOCKET") {
            const EXPECTED: &str = "/workspace/semwright-internal-loopback/bridge.sock";
            if socket != EXPECTED {
                return Err(Error::new(
                    ErrorCode::PermissionDenied,
                    "Godot loopback socket path is not Host-controlled",
                ));
            }
            let listener = UnixListener::bind(&socket)?;
            std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = stop.cancelled() => break,
                        accepted = listener.accept() => {
                            let Ok((stream, _)) = accepted else { continue };
                            let sessions = sessions.clone();
                            let projects = projects.clone();
                            let events = events.clone();
                            tokio::spawn(async move {
                                let _ = serve_connection(stream, sessions, projects, events).await;
                            });
                        }
                    }
                }
            });
            return Ok(bridge);
        }

        let listener = TcpListener::bind(("127.0.0.1", port)).await?;
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = stop.cancelled() => break,
                    accepted = listener.accept() => {
                        let Ok((stream, addr)) = accepted else { continue };
                        if !addr.ip().is_loopback() { continue; }
                        let sessions = sessions.clone();
                        let projects = projects.clone();
                        let events = events.clone();
                        tokio::spawn(async move {
                            let _ = serve_connection(stream, sessions, projects, events).await;
                        });
                    }
                }
            }
        });
        Ok(bridge)
    }

    pub async fn list(&self) -> Vec<SessionInfo> {
        self.sessions
            .read()
            .await
            .values()
            .map(|s| s.info.clone())
            .collect()
    }

    pub async fn call(
        &self,
        session: &str,
        op: &str,
        args: Value,
        timeout: Duration,
    ) -> Result<Value> {
        let session = self
            .sessions
            .read()
            .await
            .get(session)
            .cloned()
            .ok_or_else(|| {
                Error::new(ErrorCode::NotFound, "Godot editor session is not connected")
            })?;
        let (tx, rx) = oneshot::channel();
        session
            .tx
            .send(Call {
                op: op.to_owned(),
                args,
                reply: tx,
            })
            .await
            .map_err(|_| Error::new(ErrorCode::Unavailable, "Godot editor session disconnected"))?;
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(Error::new(
                ErrorCode::Unavailable,
                "Godot editor session disconnected",
            )),
            Err(_) => {
                Err(Error::new(ErrorCode::Timeout, "Godot editor request timed out").uncertain())
            }
        }
    }

    pub fn shutdown(&self) {
        self.stop.cancel();
    }
}

async fn serve_connection<S>(
    stream: S,
    sessions: Arc<RwLock<HashMap<String, Arc<Session>>>>,
    projects: Arc<HashMap<String, ProjectConfig>>,
    events: mpsc::UnboundedSender<DriverChildEvent>,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut ws = accept_async(stream).await.map_err(protocol)?;
    let hello = recv_json(&mut ws).await?;
    require_type(&hello, "hello")?;
    let project = string_field(&hello, "project", 64)?;
    let client_nonce = string_field(&hello, "nonce", 32)?;
    if !is_hex(&project, 64) || !is_hex(&client_nonce, 32) {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Invalid Godot bridge identity",
        ));
    }
    let plugin_version = string_field(&hello, "plugin_version", 64)?;
    let engine_version = string_field(&hello, "engine_version", 64)?;
    let paired = projects
        .get(&project)
        .ok_or_else(|| Error::new(ErrorCode::PermissionDenied, "Unpaired Godot project"))?;
    let secret =
        hex::decode(&paired.secret).map_err(|_| Error::invalid("Invalid pairing secret"))?;
    let server_nonce = random_hex(16)?;
    let generation = random_hex(16)?;
    let session_id = random_hex(16)?;
    let server_transcript = transcript(
        "server",
        &project,
        &client_nonce,
        &server_nonce,
        &generation,
        &session_id,
    );
    send_json(
        &mut ws,
        &json!({
            "type": "challenge",
            "bridge": BRIDGE_VERSION,
            "nonce": server_nonce,
            "generation": generation,
            "session": session_id,
            "proof": proof(&secret, &server_transcript)?
        }),
    )
    .await?;

    let auth = recv_json(&mut ws).await?;
    require_type(&auth, "authenticate")?;
    let received = string_field(&auth, "proof", 64)?;
    let client_transcript = transcript(
        "client",
        &project,
        &client_nonce,
        &server_nonce,
        &generation,
        &session_id,
    );
    let expected = proof(&secret, &client_transcript)?;
    if !constant_time_eq(expected.as_bytes(), received.as_bytes()) {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Godot bridge authentication failed",
        ));
    }
    send_json(&mut ws, &json!({"type":"ready","bridge":BRIDGE_VERSION,"session":session_id,"generation":generation})).await?;

    let info = SessionInfo {
        session: session_id.clone(),
        project: project.clone(),
        generation: generation.clone(),
        plugin_version,
        engine_version,
    };
    let (tx, mut rx) = mpsc::channel::<Call>(32);
    let session = Arc::new(Session { info, tx });
    sessions.write().await.insert(session_id.clone(), session);
    let mut request_counter: u64 = 0;
    let result: Result<()> = async {
        loop {
            tokio::select! {
                maybe_call = rx.recv() => {
                    let Some(call) = maybe_call else { break; };
                    request_counter = request_counter.checked_add(1).ok_or_else(|| {
                        Error::new(
                            ErrorCode::ResourceExhausted,
                            "Godot bridge request counter exhausted",
                        )
                    })?;
                    let request_id = format!("{session_id}:{request_counter}");
                    let payload = json!({"type":"request","id":request_id,"op":call.op,"args":call.args});
                    if let Err(error) = send_json(&mut ws, &payload).await {
                        let _ = call.reply.send(Err(error));
                        break;
                    }
                    let response = loop {
                        let value = recv_json(&mut ws).await?;
                        match value.get("type").and_then(Value::as_str) {
                            Some("response")
                                if value.get("id").and_then(Value::as_str) == Some(request_id.as_str()) =>
                            {
                                break value;
                            }
                            Some("event") => {
                                forward_event(
                                    &events,
                                    &project,
                                    &session_id,
                                    &generation,
                                    &value,
                                )?;
                            }
                            _ => {
                                return Err(Error::new(
                                    ErrorCode::ProtocolMismatch,
                                    "Unexpected Godot bridge frame",
                                ));
                            }
                        }
                    };
                    let result = if response.get("ok").and_then(Value::as_bool) == Some(true) {
                        response.get("value").cloned().ok_or_else(|| {
                            Error::new(
                                ErrorCode::ProtocolMismatch,
                                "Godot response is missing value",
                            )
                        })
                    } else {
                        let code = response
                            .get("code")
                            .and_then(Value::as_str)
                            .unwrap_or("backend_failed");
                        let message = response
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("Godot operation failed");
                        Err(Error::new(map_code(code), bounded_message(message)))
                    };
                    let _ = call.reply.send(result);
                }
                incoming = recv_json(&mut ws) => {
                    let value = incoming?;
                    if value.get("type").and_then(Value::as_str) == Some("event") {
                        forward_event(
                            &events,
                            &project,
                            &session_id,
                            &generation,
                            &value,
                        )?;
                    } else {
                        return Err(Error::new(
                            ErrorCode::ProtocolMismatch,
                            "Unexpected idle Godot bridge frame",
                        ));
                    }
                }
            }
        }
        Ok(())
    }
    .await;
    sessions.write().await.remove(&session_id);
    result
}

fn forward_event(
    events: &mpsc::UnboundedSender<DriverChildEvent>,
    project: &str,
    session: &str,
    generation: &str,
    value: &Value,
) -> Result<()> {
    let kind = string_field(value, "kind", 64)?;
    if !kind
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-'))
    {
        return Err(Error::invalid("Godot event kind is invalid"));
    }
    let revision = value
        .get("revision")
        .and_then(Value::as_u64)
        .ok_or_else(|| Error::invalid("Godot event revision is missing"))?;
    let payload = value.get("payload").cloned().unwrap_or_else(|| json!({}));
    let wrapped = json!({
        "project": project,
        "session": session,
        "generation": generation,
        "revision": revision,
        "data": payload,
    });
    if serde_json::to_vec(&wrapped)?.len() > 16_384 {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Godot event payload exceeds limit",
        ));
    }
    events
        .send(DriverChildEvent::Event {
            kind: format!("godot.{kind}"),
            payload: wrapped,
        })
        .map_err(|_| Error::unavailable("Godot driver event receiver is closed"))
}

fn map_code(code: &str) -> ErrorCode {
    match code {
        "invalid_argument" => ErrorCode::InvalidArgument,
        "not_found" => ErrorCode::NotFound,
        "conflict" => ErrorCode::Conflict,
        "stale_reference" => ErrorCode::StaleReference,
        "permission_denied" => ErrorCode::PermissionDenied,
        "unsupported" => ErrorCode::Unsupported,
        "unavailable" => ErrorCode::Unavailable,
        _ => ErrorCode::BackendFailed,
    }
}

fn bounded_message(message: &str) -> String {
    message.chars().take(512).collect()
}

async fn recv_json<S>(ws: &mut tokio_tungstenite::WebSocketStream<S>) -> Result<Value>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let message = ws
        .next()
        .await
        .ok_or_else(|| Error::new(ErrorCode::Unavailable, "Godot bridge disconnected"))?
        .map_err(protocol)?;
    let bytes = match message {
        Message::Text(text) => text.as_bytes().to_vec(),
        Message::Binary(bytes) => bytes.to_vec(),
        Message::Close(_) => return Err(Error::new(ErrorCode::Unavailable, "Godot bridge closed")),
        _ => {
            return Err(Error::new(
                ErrorCode::ProtocolMismatch,
                "Unsupported Godot bridge frame",
            ));
        }
    };
    if bytes.len() > MAX_WIRE_BYTES {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Godot bridge frame exceeds limit",
        ));
    }
    let value: Value = serde_json::from_slice(&bytes)?;
    Ok(value)
}

async fn send_json<S>(ws: &mut tokio_tungstenite::WebSocketStream<S>, value: &Value) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > MAX_WIRE_BYTES {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Godot bridge frame exceeds limit",
        ));
    }
    let text = String::from_utf8(bytes)
        .map_err(|_| Error::new(ErrorCode::Internal, "JSON encoding was not UTF-8"))?;
    ws.send(Message::Text(text.into())).await.map_err(protocol)
}

fn require_type(value: &Value, expected: &str) -> Result<()> {
    if value.get("type").and_then(Value::as_str) != Some(expected) {
        return Err(Error::new(
            ErrorCode::ProtocolMismatch,
            "Unexpected Godot bridge message type",
        ));
    }
    Ok(())
}

fn string_field(value: &Value, key: &str, max: usize) -> Result<String> {
    let value = value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::invalid("Godot bridge string field missing"))?;
    if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(Error::invalid("Godot bridge string field exceeds bounds"));
    }
    Ok(value.to_owned())
}

pub fn transcript(
    role: &str,
    project: &str,
    client: &str,
    server: &str,
    generation: &str,
    session: &str,
) -> Vec<u8> {
    format!("SWG1\n{role}\n{project}\n{client}\n{server}\n{generation}\n{session}").into_bytes()
}

pub fn proof(secret: &[u8], bytes: &[u8]) -> Result<String> {
    let mut mac = HmacSha256::new_from_slice(secret)
        .map_err(|_| Error::new(ErrorCode::Internal, "HMAC initialization failed"))?;
    mac.update(bytes);
    Ok(hex::encode(mac.finalize().into_bytes()))
}

pub fn random_hex(bytes: usize) -> Result<String> {
    let mut out = vec![0u8; bytes];
    getrandom::getrandom(&mut out)
        .map_err(|_| Error::new(ErrorCode::Internal, "Secure random source unavailable"))?;
    Ok(hex::encode(out))
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

fn protocol<E: std::fmt::Display>(_: E) -> Error {
    Error::new(
        ErrorCode::ProtocolMismatch,
        "Godot bridge WebSocket protocol failure",
    )
}
