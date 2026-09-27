use crate::config::{ProjectConfig, is_hex};
use futures_util::{SinkExt, StreamExt};
use hmac::{Hmac, Mac};
use semwright_driver_sdk::{
    DriverChildEvent,
    continuity::{ConnectionState, ContinuityStamp},
};
use semwright_types::{Error, ErrorCode, Result};
use serde_json::{Value, json};
use sha2::Sha256;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
#[cfg(unix)]
use tokio::net::UnixListener;
#[cfg(windows)]
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpListener,
    sync::{Mutex, Notify, RwLock, mpsc, oneshot},
    time::Instant,
};
use tokio_tungstenite::{accept_async, tungstenite::Message};
use tokio_util::sync::CancellationToken;

type HmacSha256 = Hmac<Sha256>;
const BRIDGE_VERSION: u64 = 1;
const MAX_WIRE_BYTES: usize = 512 * 1024;
const MAX_SESSIONS: usize = 8;
pub const RECONNECT_GRACE: Duration = Duration::from_secs(5);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);
const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(25);

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

#[derive(Clone)]
struct ActiveSession {
    generation: String,
    tx: mpsc::Sender<Call>,
    stop: CancellationToken,
}

struct SessionSlot {
    project: String,
    info: RwLock<SessionInfo>,
    active: RwLock<Option<ActiveSession>>,
    state: RwLock<ConnectionState>,
    notify: Notify,
    stamp: Mutex<ContinuityStamp>,
    reconnects: AtomicU64,
}

impl SessionSlot {
    fn new(project: String, info: SessionInfo) -> Self {
        Self {
            project,
            info: RwLock::new(info),
            active: RwLock::new(None),
            state: RwLock::new(ConnectionState::Disconnected),
            notify: Notify::new(),
            stamp: Mutex::new(ContinuityStamp::default()),
            reconnects: AtomicU64::new(0),
        }
    }

    async fn activate(
        &self,
        info: SessionInfo,
        tx: mpsc::Sender<Call>,
        stop: CancellationToken,
    ) -> Result<Option<ActiveSession>> {
        {
            let mut state = self.state.write().await;
            match *state {
                ConnectionState::Disconnected | ConnectionState::Reconnecting => {}
                ConnectionState::Ready => {
                    state.transition(ConnectionState::Reconnecting)?;
                }
                _ => {
                    return Err(Error::new(
                        ErrorCode::ProtocolMismatch,
                        "Godot logical session cannot activate from its current continuity state",
                    ));
                }
            }
            state.transition(ConnectionState::Connecting)?;
            state.transition(ConnectionState::Authenticating)?;
            state.transition(ConnectionState::Ready)?;
        }

        let old = self.active.write().await.replace(ActiveSession {
            generation: info.generation.clone(),
            tx,
            stop,
        });
        *self.info.write().await = info;
        let mut stamp = self.stamp.lock().await;
        stamp.next_connection()?;
        if stamp.generation > 1 {
            self.reconnects.fetch_add(1, Ordering::Relaxed);
        }
        drop(stamp);
        self.notify.notify_waiters();
        Ok(old)
    }

    async fn deactivate_if_generation(&self, generation: &str) -> Result<bool> {
        let mut active = self.active.write().await;
        let matches = active
            .as_ref()
            .is_some_and(|current| current.generation == generation);
        if matches {
            let mut state = self.state.write().await;
            state.transition(ConnectionState::Reconnecting)?;
            *active = None;
            drop(state);
            self.notify.notify_waiters();
        }
        Ok(matches)
    }

    async fn close_for_cleanup(&self) -> Result<()> {
        let mut state = self.state.write().await;
        state.transition(ConnectionState::Closing)?;
        state.transition(ConnectionState::Closed)?;
        Ok(())
    }

    async fn sender_until(&self, deadline: Instant) -> Result<mpsc::Sender<Call>> {
        loop {
            let notified = self.notify.notified();
            if let Some(tx) = self
                .active
                .read()
                .await
                .as_ref()
                .map(|current| current.tx.clone())
            {
                return Ok(tx);
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                return Err(Error::new(
                    ErrorCode::Unavailable,
                    "Godot editor session did not reconnect within the grace window",
                ));
            }
        }
    }

    async fn active_info(&self) -> Option<SessionInfo> {
        if self.active.read().await.is_some() {
            Some(self.info.read().await.clone())
        } else {
            None
        }
    }

    async fn should_cleanup(&self, generation: &str) -> bool {
        self.active.read().await.is_none() && self.info.read().await.generation == generation
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct BridgeContinuity {
    pub connected_sessions: usize,
    pub reconnecting_sessions: usize,
    pub reconnects: u64,
    pub reconnect_grace_ms: u64,
}

#[derive(Clone)]
pub struct Bridge {
    sessions: Arc<RwLock<HashMap<String, Arc<SessionSlot>>>>,
    stop: CancellationToken,
}

#[cfg(windows)]
async fn connect_loopback_pipe(path: &str, stop: &CancellationToken) -> Result<NamedPipeClient> {
    if !path.starts_with(r"\\.\pipe\semwright-loopback-") || path.len() > 256 {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Godot loopback pipe path is not Host-controlled",
        ));
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if stop.is_cancelled() {
            return Err(Error::new(ErrorCode::Cancelled, "Godot loopback stopped"));
        }
        match ClientOptions::new().open(path) {
            Ok(pipe) => return Ok(pipe),
            Err(error)
                if (matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound
                        | std::io::ErrorKind::PermissionDenied
                        | std::io::ErrorKind::WouldBlock
                ) || error.raw_os_error() == Some(231))
                    && tokio::time::Instant::now() < deadline =>
            {
                tokio::select! {
                    _ = stop.cancelled() => {
                        return Err(Error::new(ErrorCode::Cancelled, "Godot loopback stopped"));
                    }
                    _ = tokio::time::sleep(Duration::from_millis(20)) => {}
                }
            }
            Err(error) => return Err(error.into()),
        }
    }
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

        #[cfg(windows)]
        if let Ok(pipe_path) = std::env::var("SEMWRIGHT_DRIVER_LOOPBACK_PIPE") {
            if !pipe_path.starts_with(r"\\.\pipe\semwright-loopback-") || pipe_path.len() > 256 {
                return Err(Error::new(
                    ErrorCode::PermissionDenied,
                    "Godot loopback pipe path is not Host-controlled",
                ));
            }
            let pipe_stop = stop.clone();
            let pipe_sessions = sessions.clone();
            let pipe_projects = projects.clone();
            let pipe_events = events.clone();
            tokio::spawn(async move {
                loop {
                    let pipe = tokio::select! {
                        _ = pipe_stop.cancelled() => break,
                        result = connect_loopback_pipe(&pipe_path, &pipe_stop) => {
                            let Ok(pipe) = result else {
                                if pipe_stop.is_cancelled() {
                                    break;
                                }
                                tokio::time::sleep(Duration::from_millis(50)).await;
                                continue;
                            };
                            pipe
                        }
                    };
                    let sessions = pipe_sessions.clone();
                    let projects = pipe_projects.clone();
                    let events = pipe_events.clone();
                    tokio::spawn(async move {
                        let _ = serve_connection(pipe, sessions, projects, events).await;
                    });
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
        let slots = self
            .sessions
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let mut active = Vec::with_capacity(slots.len());
        for slot in slots {
            if let Some(info) = slot.active_info().await {
                active.push(info);
            }
        }
        active
    }

    pub async fn continuity(&self) -> BridgeContinuity {
        let slots = self
            .sessions
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let mut connected_sessions = 0usize;
        let mut reconnecting_sessions = 0usize;
        let mut reconnects = 0u64;
        for slot in slots {
            match *slot.state.read().await {
                ConnectionState::Ready => connected_sessions += 1,
                ConnectionState::Reconnecting => reconnecting_sessions += 1,
                _ => {}
            }
            reconnects = reconnects.saturating_add(slot.reconnects.load(Ordering::Relaxed));
        }
        BridgeContinuity {
            connected_sessions,
            reconnecting_sessions,
            reconnects,
            reconnect_grace_ms: RECONNECT_GRACE.as_millis() as u64,
        }
    }

    pub async fn call(
        &self,
        session: &str,
        op: &str,
        args: Value,
        timeout: Duration,
    ) -> Result<Value> {
        let slot = self
            .sessions
            .read()
            .await
            .get(session)
            .cloned()
            .ok_or_else(|| {
                Error::new(ErrorCode::NotFound, "Godot editor session is not connected")
            })?;
        let now = Instant::now();
        let dispatch_deadline = now + timeout.min(RECONNECT_GRACE);
        let response_deadline = now + timeout;
        let (tx, rx) = oneshot::channel();
        let mut call = Call {
            op: op.to_owned(),
            args,
            reply: tx,
        };
        loop {
            let sender = slot.sender_until(dispatch_deadline).await?;
            match sender.send(call).await {
                Ok(()) => break,
                Err(error) => {
                    call = error.0;
                    if Instant::now() >= dispatch_deadline {
                        return Err(Error::new(
                            ErrorCode::Unavailable,
                            "Godot editor session disconnected before request dispatch",
                        ));
                    }
                }
            }
        }
        match tokio::time::timeout_at(response_deadline, rx).await {
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
    sessions: Arc<RwLock<HashMap<String, Arc<SessionSlot>>>>,
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
    let requested_resume = match hello.get("resume_session") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) if is_hex(value, 32) => Some(value.clone()),
        Some(_) => {
            return Err(Error::invalid(
                "Godot resume_session must be a 32-character hexadecimal session ID",
            ));
        }
    };
    let resumed_slot = if let Some(session) = requested_resume.as_deref() {
        let candidate = sessions.read().await.get(session).cloned();
        candidate.filter(|slot| slot.project == project)
    } else {
        None
    };
    if resumed_slot.is_none() && sessions.read().await.len() >= MAX_SESSIONS {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Godot bridge session limit reached",
        ));
    }
    let server_nonce = random_hex(16)?;
    let generation = random_hex(16)?;
    let session_id = if let Some(slot) = resumed_slot.as_ref() {
        slot.info.read().await.session.clone()
    } else {
        random_hex(16)?
    };
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
    send_json(
        &mut ws,
        &json!({
            "type":"ready",
            "bridge":BRIDGE_VERSION,
            "session":session_id,
            "generation":generation,
            "resumed":resumed_slot.is_some()
        }),
    )
    .await?;

    let info = SessionInfo {
        session: session_id.clone(),
        project: project.clone(),
        generation: generation.clone(),
        plugin_version,
        engine_version,
    };
    let (tx, mut rx) = mpsc::channel::<Call>(32);
    let connection_stop = CancellationToken::new();
    let slot = if let Some(slot) = resumed_slot {
        let mut guard = sessions.write().await;
        match guard.get(&session_id) {
            Some(current) if Arc::ptr_eq(current, &slot) => {}
            Some(_) => {
                return Err(Error::new(
                    ErrorCode::Conflict,
                    "Godot logical session was replaced during authentication",
                ));
            }
            None => {
                guard.insert(session_id.clone(), slot.clone());
            }
        }
        drop(guard);
        slot
    } else {
        let slot = Arc::new(SessionSlot::new(project.clone(), info.clone()));
        let mut guard = sessions.write().await;
        if guard.len() >= MAX_SESSIONS {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Godot bridge session limit reached",
            ));
        }
        guard.insert(session_id.clone(), slot.clone());
        slot
    };
    if let Some(old) = slot.activate(info, tx, connection_stop.clone()).await? {
        old.stop.cancel();
    }
    let mut request_counter: u64 = 0;
    let mut heartbeat = tokio::time::interval(HEARTBEAT_INTERVAL);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    heartbeat.tick().await;
    let mut pending_ping: Option<(String, Instant)> = None;
    let result: Result<()> = async {
        loop {
            tokio::select! {
                _ = connection_stop.cancelled() => {
                    return Err(Error::new(
                        ErrorCode::Unavailable,
                        "Godot session generation was superseded",
                    ));
                }
                _ = heartbeat.tick() => {
                    if let Some((_, sent_at)) = pending_ping.as_ref()
                        && Instant::now().duration_since(*sent_at) >= HEARTBEAT_TIMEOUT
                    {
                        return Err(Error::new(
                            ErrorCode::Unavailable,
                            "Godot bridge heartbeat timed out",
                        ));
                    }
                    if pending_ping.is_none() {
                        let nonce = random_hex(8)?;
                        send_json(&mut ws, &json!({"type":"ping","nonce":nonce})).await?;
                        pending_ping = Some((nonce, Instant::now()));
                    }
                }
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
                        let value = tokio::select! {
                            _ = connection_stop.cancelled() => {
                                return Err(Error::new(
                                    ErrorCode::Unavailable,
                                    "Godot session generation was superseded",
                                ));
                            }
                            value = recv_json(&mut ws) => value?,
                        };
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
                            Some("pong") => {
                                let nonce = value
                                    .get("nonce")
                                    .and_then(Value::as_str)
                                    .ok_or_else(|| Error::invalid("Godot heartbeat pong is missing nonce"))?;
                                if !pending_ping
                                    .as_ref()
                                    .is_some_and(|(expected, _)| expected == nonce)
                                {
                                    return Err(Error::new(
                                        ErrorCode::ProtocolMismatch,
                                        "Godot heartbeat pong did not match",
                                    ));
                                }
                                pending_ping = None;
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
                    match value.get("type").and_then(Value::as_str) {
                        Some("event") => {
                            forward_event(
                                &events,
                                &project,
                                &session_id,
                                &generation,
                                &value,
                            )?;
                        }
                        Some("pong") => {
                            let nonce = value
                                .get("nonce")
                                .and_then(Value::as_str)
                                .ok_or_else(|| Error::invalid("Godot heartbeat pong is missing nonce"))?;
                            if !pending_ping
                                .as_ref()
                                .is_some_and(|(expected, _)| expected == nonce)
                            {
                                return Err(Error::new(
                                    ErrorCode::ProtocolMismatch,
                                    "Godot heartbeat pong did not match",
                                ));
                            }
                            pending_ping = None;
                        }
                        _ => {
                            return Err(Error::new(
                                ErrorCode::ProtocolMismatch,
                                "Unexpected idle Godot bridge frame",
                            ));
                        }
                    }
                }
            }
        }
        Ok(())
    }
    .await;
    if slot.deactivate_if_generation(&generation).await? {
        schedule_cleanup(
            sessions.clone(),
            session_id.clone(),
            generation.clone(),
            slot.clone(),
        );
    }
    result
}

fn schedule_cleanup(
    sessions: Arc<RwLock<HashMap<String, Arc<SessionSlot>>>>,
    session_id: String,
    generation: String,
    slot: Arc<SessionSlot>,
) {
    tokio::spawn(async move {
        tokio::time::sleep(RECONNECT_GRACE).await;
        if !slot.should_cleanup(&generation).await {
            return;
        }
        let mut guard = sessions.write().await;
        if guard
            .get(&session_id)
            .is_some_and(|current| Arc::ptr_eq(current, &slot))
            && slot.should_cleanup(&generation).await
        {
            if slot.close_for_cleanup().await.is_err() {
                return;
            }
            guard.remove(&session_id);
            slot.notify.notify_waiters();
        }
    });
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
