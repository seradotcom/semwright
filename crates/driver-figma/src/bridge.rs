use crate::model::{BRIDGE_PROTOCOL_VERSION, MAX_MESSAGE_BYTES};
use futures_util::{Sink, SinkExt, Stream, StreamExt};
use hmac::{Hmac, Mac};
use rand::RngCore;
use semwright_driver_sdk::DriverChildEvent;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::Sha256;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::ErrorKind,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};
use thiserror::Error;
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{Mutex, RwLock, mpsc, oneshot},
    task::JoinHandle,
    time::{Instant, MissedTickBehavior, interval, timeout},
};
use tokio_tungstenite::{accept_async, tungstenite::Message as WsMessage};
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;
const DEFAULT_PORT: u16 = 38_471;
const MAX_PENDING: usize = 128;
const MAX_EXPIRED_REQUESTS: usize = 256;
#[cfg(not(test))]
const REQUEST_TIMEOUT: Duration = Duration::from_secs(25);
#[cfg(test)]
const REQUEST_TIMEOUT: Duration = Duration::from_millis(150);
const RECONNECT_GRACE: Duration = Duration::from_secs(2);
const RECONNECT_POLL: Duration = Duration::from_millis(25);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);
const HEARTBEAT_DEADLINE: Duration = Duration::from_secs(35);
const MAX_RESUME_CREDENTIALS: usize = 16;
const RESUME_CREDENTIAL_TTL: Duration = Duration::from_secs(12 * 60 * 60);

#[derive(Debug, Error)]
pub enum BridgeError {
    #[error("authentication failed")]
    Auth,
    #[error("protocol mismatch")]
    Protocol,
    #[error("stale generation")]
    Stale,
    #[error("duplicate request")]
    Duplicate,
    #[error("resource limit")]
    Limit,
    #[error("session unavailable")]
    Unavailable,
    #[error("request timed out")]
    Timeout,
    #[error("bridge I/O failed")]
    Io,
    #[error("remote operation failed: {0}")]
    Remote(String, bool),
}

#[derive(Debug, Clone)]
pub struct PairingSecret([u8; 32]);
impl PairingSecret {
    pub fn random() -> Self {
        let mut b = [0u8; 32];
        rand::rng().fill_bytes(&mut b);
        Self(b)
    }
    pub fn code(&self) -> String {
        hex::encode(self.0)
    }
    pub fn proof(&self, nonce: &str, session: &str, generation: u64) -> String {
        let mut mac = HmacSha256::new_from_slice(&self.0).expect("HMAC accepts any key length");
        mac.update(nonce.as_bytes());
        mac.update(b"\0");
        mac.update(session.as_bytes());
        mac.update(b"\0");
        mac.update(&generation.to_be_bytes());
        hex::encode(mac.finalize().into_bytes())
    }
    pub fn verify(&self, nonce: &str, session: &str, generation: u64, proof: &str) -> bool {
        let Ok(bytes) = hex::decode(proof) else {
            return false;
        };
        let Ok(mut mac) = HmacSha256::new_from_slice(&self.0) else {
            return false;
        };
        mac.update(nonce.as_bytes());
        mac.update(b"\0");
        mac.update(session.as_bytes());
        mac.update(b"\0");
        mac.update(&generation.to_be_bytes());
        mac.verify_slice(&bytes).is_ok()
    }
}

#[derive(Clone)]
struct ResumeCredential {
    token: [u8; 32],
    session_id: String,
    document_id: String,
    generation: u64,
    expires_at: Instant,
}

#[cfg(test)]
fn resume_proof_bytes(token: &[u8; 32], nonce: &str, session: &str, generation: u64) -> String {
    let mut mac = HmacSha256::new_from_slice(token).expect("HMAC accepts any key length");
    mac.update(b"figma-resume-v1");
    mac.update(b"\0");
    mac.update(nonce.as_bytes());
    mac.update(b"\0");
    mac.update(session.as_bytes());
    mac.update(b"\0");
    mac.update(&generation.to_be_bytes());
    hex::encode(mac.finalize().into_bytes())
}

fn verify_resume_proof(
    token: &[u8; 32],
    nonce: &str,
    session: &str,
    generation: u64,
    proof: &str,
) -> bool {
    let Ok(bytes) = hex::decode(proof) else {
        return false;
    };
    let Ok(mut mac) = HmacSha256::new_from_slice(token) else {
        return false;
    };
    mac.update(b"figma-resume-v1");
    mac.update(b"\0");
    mac.update(nonce.as_bytes());
    mac.update(b"\0");
    mac.update(session.as_bytes());
    mac.update(b"\0");
    mac.update(&generation.to_be_bytes());
    mac.verify_slice(&bytes).is_ok()
}

fn random_hex<const N: usize>() -> (String, [u8; N]) {
    let mut bytes = [0u8; N];
    rand::rng().fill_bytes(&mut bytes);
    (hex::encode(bytes), bytes)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Message {
    Hello {
        protocol: u32,
        plugin_build: String,
        figma_api: String,
        editor_type: String,
        session_id: String,
        document_id: String,
        generation: u64,
        #[serde(default)]
        revision: u64,
        capabilities: Vec<String>,
    },
    ResumeHello {
        protocol: u32,
        plugin_build: String,
        figma_api: String,
        editor_type: String,
        document_id: String,
        #[serde(default)]
        revision: u64,
        capabilities: Vec<String>,
        resume_id: String,
    },
    Challenge {
        session_id: String,
        generation: u64,
        nonce: String,
    },
    ResumeChallenge {
        session_id: String,
        generation: u64,
        nonce: String,
        resume_id: String,
    },
    Authenticate {
        session_id: String,
        generation: u64,
        proof: String,
    },
    ResumeAuthenticate {
        session_id: String,
        generation: u64,
        resume_id: String,
        proof: String,
    },
    Ready {
        session_id: String,
        generation: u64,
        revision: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resume_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resume_token: Option<String>,
    },
    Request {
        id: String,
        session_id: String,
        generation: u64,
        expected_revision: Option<u64>,
        operation: String,
        args: Value,
    },
    Response {
        id: String,
        session_id: String,
        generation: u64,
        revision: u64,
        ok: bool,
        value: Option<Value>,
        error: Option<BridgeFailure>,
    },
    Event {
        session_id: String,
        generation: u64,
        revision: u64,
        kind: String,
        payload: Value,
    },
    Ping {
        nonce: String,
    },
    Pong {
        nonce: String,
    },
    RevokeResume {
        resume_id: String,
    },
    Close {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BridgeFailure {
    pub code: String,
    pub message: String,
    #[serde(alias = "outcomeKnown")]
    pub outcome_known: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub document_id: String,
    pub generation: u64,
    pub revision: u64,
    pub editor_type: String,
    pub capabilities: Vec<String>,
}

struct SessionHandle {
    info: SessionInfo,
    outbound: mpsc::Sender<Message>,
    pending: Arc<Mutex<BTreeMap<String, oneshot::Sender<Message>>>>,
    expired: Arc<Mutex<BTreeSet<String>>>,
}

#[derive(Default)]
struct HubState {
    sessions: BTreeMap<String, SessionHandle>,
    resume: BTreeMap<String, ResumeCredential>,
}

fn issue_resume_credential(
    state: &mut HubState,
    session_id: &str,
    document_id: &str,
    generation: u64,
) -> (String, String) {
    let now = Instant::now();
    state.resume.retain(|_, record| record.expires_at > now);
    let expiring_first = (state.resume.len() >= MAX_RESUME_CREDENTIALS)
        .then(|| {
            state
                .resume
                .iter()
                .min_by_key(|(_, record)| record.expires_at)
                .map(|(id, _)| id.clone())
        })
        .flatten();
    if let Some(expiring_first) = expiring_first {
        state.resume.remove(&expiring_first);
    }
    loop {
        let (resume_id, _) = random_hex::<16>();
        if state.resume.contains_key(&resume_id) {
            continue;
        }
        let (resume_token, token) = random_hex::<32>();
        state.resume.insert(
            resume_id.clone(),
            ResumeCredential {
                token,
                session_id: session_id.to_owned(),
                document_id: document_id.to_owned(),
                generation,
                expires_at: now + RESUME_CREDENTIAL_TTL,
            },
        );
        return (resume_id, resume_token);
    }
}

async fn take_session_generation(
    state: &Arc<RwLock<HubState>>,
    session_id: &str,
    generation: u64,
) -> Option<SessionHandle> {
    let mut guard = state.write().await;
    if guard
        .sessions
        .get(session_id)
        .is_some_and(|active| active.info.generation == generation)
    {
        guard.sessions.remove(session_id)
    } else {
        None
    }
}

async fn retire_session(handle: SessionHandle, reason: Option<&str>) {
    if let Some(reason) = reason {
        let _ = handle.outbound.try_send(Message::Close {
            reason: reason.to_owned(),
        });
    }
    handle.pending.lock().await.clear();
    handle.expired.lock().await.clear();
}

async fn remember_expired(expired: &Arc<Mutex<BTreeSet<String>>>, id: String) {
    let mut expired = expired.lock().await;
    if expired.len() >= MAX_EXPIRED_REQUESTS
        && let Some(oldest) = expired.iter().next().cloned()
    {
        expired.remove(&oldest);
    }
    expired.insert(id);
}

fn bridge_log(event: &str, mut fields: serde_json::Map<String, Value>) {
    if std::env::var_os("SEMWRIGHT_FIGMA_BRIDGE_LOG").is_none() {
        return;
    }
    fields.insert("component".into(), Value::String("figma_bridge".into()));
    fields.insert("event".into(), Value::String(event.into()));
    eprintln!("{}", Value::Object(fields));
}

async fn bind_loopback_listeners(
    port: u16,
) -> Result<(SocketAddr, Vec<TcpListener>, Vec<IpAddr>), BridgeError> {
    let ipv4 = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port))
        .await
        .map_err(|_| BridgeError::Io)?;
    let addr = ipv4.local_addr().map_err(|_| BridgeError::Io)?;
    let port = addr.port();
    let mut listeners = vec![ipv4];
    let mut hosts = vec![IpAddr::V4(Ipv4Addr::LOCALHOST)];

    match TcpListener::bind(SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), port)).await {
        Ok(ipv6) => {
            listeners.push(ipv6);
            hosts.push(IpAddr::V6(Ipv6Addr::LOCALHOST));
        }
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::AddrNotAvailable | ErrorKind::Unsupported
            ) => {}
        Err(_) => return Err(BridgeError::Io),
    }

    Ok((addr, listeners, hosts))
}

pub struct BridgeHub {
    addr: SocketAddr,
    listen_hosts: Vec<IpAddr>,
    secret: PairingSecret,
    state: Arc<RwLock<HubState>>,
    tasks: Vec<JoinHandle<()>>,
}

impl BridgeHub {
    pub async fn start(
        events: mpsc::UnboundedSender<DriverChildEvent>,
    ) -> Result<Self, BridgeError> {
        let port = std::env::var("SEMWRIGHT_FIGMA_BRIDGE_PORT")
            .ok()
            .map(|raw| raw.parse::<u16>().map_err(|_| BridgeError::Protocol))
            .transpose()?
            .unwrap_or(DEFAULT_PORT);
        Self::start_on(port, Some(events)).await
    }

    async fn start_on(
        port: u16,
        events: Option<mpsc::UnboundedSender<DriverChildEvent>>,
    ) -> Result<Self, BridgeError> {
        let (addr, listeners, listen_hosts) = bind_loopback_listeners(port).await?;
        let secret = PairingSecret::random();
        let state = Arc::new(RwLock::new(HubState::default()));
        let mut tasks = Vec::with_capacity(listeners.len());
        for listener in listeners {
            let accept_state = Arc::clone(&state);
            let accept_secret = secret.clone();
            let accept_events = events.clone();
            tasks.push(tokio::spawn(async move {
                while let Ok((stream, peer)) = listener.accept().await {
                    if !peer.ip().is_loopback() {
                        continue;
                    }
                    bridge_log(
                        "tcp_accepted",
                        serde_json::Map::from_iter([("peer_addr".into(), json!(peer.to_string()))]),
                    );
                    let state = Arc::clone(&accept_state);
                    let secret = accept_secret.clone();
                    let events = accept_events.clone();
                    tokio::spawn(async move {
                        let _ = serve_connection(stream, state, secret, events).await;
                    });
                }
            }));
        }
        bridge_log(
            "listening",
            serde_json::Map::from_iter([
                ("listen_port".into(), json!(addr.port())),
                (
                    "listen_hosts".into(),
                    json!(
                        listen_hosts
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                    ),
                ),
            ]),
        );
        Ok(Self {
            addr,
            listen_hosts,
            secret,
            state,
            tasks,
        })
    }

    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    pub fn listen_hosts(&self) -> Vec<String> {
        self.listen_hosts.iter().map(ToString::to_string).collect()
    }

    pub fn pairing_code(&self) -> String {
        self.secret.code()
    }

    pub async fn sessions(&self) -> Vec<SessionInfo> {
        self.state
            .read()
            .await
            .sessions
            .values()
            .map(|s| s.info.clone())
            .collect()
    }

    async fn invalidate_generation(&self, session_id: &str, generation: u64, reason: &str) {
        if let Some(handle) = take_session_generation(&self.state, session_id, generation).await {
            retire_session(handle, Some(reason)).await;
        }
    }

    pub async fn execute(
        &self,
        session_id: Option<&str>,
        operation: &str,
        expected_revision: Option<u64>,
        args: Value,
    ) -> Result<Value, BridgeError> {
        if operation.len() > 128 || !args.is_object() {
            return Err(BridgeError::Protocol);
        }
        let reconnect_deadline = Instant::now() + RECONNECT_GRACE;
        let (chosen_id, generation, revision, tx, pending, expired) = loop {
            let selected = {
                let state = self.state.read().await;
                let session = match session_id {
                    Some(id) => state.sessions.get(id),
                    None if state.sessions.len() == 1 => state.sessions.values().next(),
                    None if state.sessions.len() > 1 => return Err(BridgeError::Unavailable),
                    None => None,
                };
                session.map(|session| {
                    (
                        session.info.session_id.clone(),
                        session.info.generation,
                        session.info.revision,
                        session.outbound.clone(),
                        Arc::clone(&session.pending),
                        Arc::clone(&session.expired),
                    )
                })
            };
            if let Some(selected) = selected {
                break selected;
            }
            if Instant::now() >= reconnect_deadline {
                return Err(BridgeError::Unavailable);
            }
            tokio::time::sleep(RECONNECT_POLL).await;
        };
        if expected_revision.is_some_and(|rev| rev != revision) {
            return Err(BridgeError::Stale);
        }

        let id = request_id();
        let request_started = Instant::now();
        let (reply_tx, reply_rx) = oneshot::channel();
        {
            let mut map = pending.lock().await;
            if map.len() >= MAX_PENDING || map.contains_key(&id) {
                return Err(BridgeError::Limit);
            }
            map.insert(id.clone(), reply_tx);
        }

        let implicit_revision_guard = !matches!(
            operation,
            "artifact.upload.begin" | "artifact.upload.append" | "artifact.status"
        );
        let request = Message::Request {
            id: id.clone(),
            session_id: chosen_id.clone(),
            generation,
            expected_revision: expected_revision
                .or_else(|| implicit_revision_guard.then_some(revision)),
            operation: operation.to_owned(),
            args,
        };
        if tx.send(request).await.is_err() {
            pending.lock().await.remove(&id);
            bridge_log(
                "request_failed",
                serde_json::Map::from_iter([
                    ("request_id".into(), json!(id)),
                    ("session_id".into(), json!(chosen_id)),
                    ("generation".into(), json!(generation)),
                    ("operation".into(), json!(operation)),
                    (
                        "latency_ms".into(),
                        json!(request_started.elapsed().as_millis()),
                    ),
                    ("error".into(), json!("writer_closed")),
                ]),
            );
            self.invalidate_generation(
                &chosen_id,
                generation,
                "Figma plugin session writer closed",
            )
            .await;
            return Err(BridgeError::Unavailable);
        }

        let response = match timeout(REQUEST_TIMEOUT, reply_rx).await {
            Ok(Ok(message)) => {
                bridge_log(
                    "request_complete",
                    serde_json::Map::from_iter([
                        ("request_id".into(), json!(id)),
                        ("session_id".into(), json!(chosen_id)),
                        ("generation".into(), json!(generation)),
                        ("operation".into(), json!(operation)),
                        (
                            "latency_ms".into(),
                            json!(request_started.elapsed().as_millis()),
                        ),
                    ]),
                );
                message
            }
            Ok(Err(_)) => {
                bridge_log(
                    "request_failed",
                    serde_json::Map::from_iter([
                        ("request_id".into(), json!(id)),
                        ("session_id".into(), json!(chosen_id)),
                        ("generation".into(), json!(generation)),
                        ("operation".into(), json!(operation)),
                        (
                            "latency_ms".into(),
                            json!(request_started.elapsed().as_millis()),
                        ),
                        ("error".into(), json!("session_disconnected")),
                    ]),
                );
                self.invalidate_generation(
                    &chosen_id,
                    generation,
                    "Figma plugin session disconnected",
                )
                .await;
                return Err(BridgeError::Unavailable);
            }
            Err(_) => {
                pending.lock().await.remove(&id);
                bridge_log(
                    "request_failed",
                    serde_json::Map::from_iter([
                        ("request_id".into(), json!(id)),
                        ("session_id".into(), json!(chosen_id)),
                        ("generation".into(), json!(generation)),
                        ("operation".into(), json!(operation)),
                        (
                            "latency_ms".into(),
                            json!(request_started.elapsed().as_millis()),
                        ),
                        ("error".into(), json!("timeout")),
                    ]),
                );
                remember_expired(&expired, id).await;
                return Err(BridgeError::Timeout);
            }
        };

        match response {
            Message::Response {
                session_id: response_session,
                generation: response_generation,
                revision: response_revision,
                ok,
                value,
                error,
                ..
            } => {
                if response_session != chosen_id || response_generation != generation {
                    return Err(BridgeError::Stale);
                }
                {
                    let mut state = self.state.write().await;
                    let Some(session) = state.sessions.get_mut(&chosen_id) else {
                        return Err(BridgeError::Unavailable);
                    };
                    if session.info.generation != generation {
                        return Err(BridgeError::Stale);
                    }
                    session.info.revision = response_revision;
                }
                if ok {
                    Ok(value.unwrap_or(Value::Null))
                } else {
                    let failure = error.unwrap_or(BridgeFailure {
                        code: "plugin_error".into(),
                        message: "Figma plugin reported failure".into(),
                        outcome_known: true,
                    });
                    Err(BridgeError::Remote(failure.code, failure.outcome_known))
                }
            }
            _ => Err(BridgeError::Protocol),
        }
    }
}

impl Drop for BridgeHub {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

enum AuthMode {
    Pairing,
    Resume { resume_id: String, token: [u8; 32] },
}

async fn serve_connection(
    stream: TcpStream,
    state: Arc<RwLock<HubState>>,
    secret: PairingSecret,
    events: Option<mpsc::UnboundedSender<DriverChildEvent>>,
) -> Result<(), BridgeError> {
    let peer_addr = stream.peer_addr().ok().map(|peer| peer.to_string());
    let ws = match accept_async(stream).await {
        Ok(ws) => ws,
        Err(error) => {
            bridge_log(
                "websocket_handshake_failed",
                serde_json::Map::from_iter([
                    ("peer_addr".into(), json!(peer_addr)),
                    ("error".into(), json!(error.to_string())),
                ]),
            );
            return Err(BridgeError::Protocol);
        }
    };
    bridge_log(
        "websocket_open",
        serde_json::Map::from_iter([("peer_addr".into(), json!(peer_addr))]),
    );
    let (mut sink, mut source) = ws.split();

    let hello = read_message(&mut source).await?;
    bridge_log(
        "hello_received",
        serde_json::Map::from_iter([("peer_addr".into(), json!(peer_addr))]),
    );
    let (session_id, document_id, generation, revision, editor_type, capabilities, auth_mode) =
        match hello {
            Message::Hello {
                protocol,
                plugin_build,
                figma_api,
                editor_type,
                session_id,
                document_id,
                generation,
                revision,
                capabilities,
            } if protocol == BRIDGE_PROTOCOL_VERSION
                && !plugin_build.is_empty()
                && plugin_build.len() <= 128
                && !figma_api.is_empty()
                && figma_api.len() <= 64
                && !session_id.is_empty()
                && session_id.len() <= 128
                && !document_id.is_empty()
                && document_id.len() <= 256
                && generation > 0
                && capabilities.len() <= 256 =>
            {
                (
                    session_id,
                    document_id,
                    generation,
                    revision,
                    editor_type,
                    capabilities,
                    AuthMode::Pairing,
                )
            }
            Message::ResumeHello {
                protocol,
                plugin_build,
                figma_api,
                editor_type,
                document_id,
                revision,
                capabilities,
                resume_id,
            } if protocol == BRIDGE_PROTOCOL_VERSION
                && !plugin_build.is_empty()
                && plugin_build.len() <= 128
                && !figma_api.is_empty()
                && figma_api.len() <= 64
                && !document_id.is_empty()
                && document_id.len() <= 256
                && resume_id.len() == 32
                && resume_id.bytes().all(|b| b.is_ascii_hexdigit())
                && capabilities.len() <= 256 =>
            {
                let record = {
                    let mut guard = state.write().await;
                    let expired = guard
                        .resume
                        .get(&resume_id)
                        .is_some_and(|record| record.expires_at <= Instant::now());
                    if expired {
                        guard.resume.remove(&resume_id);
                        None
                    } else {
                        guard
                            .resume
                            .get(&resume_id)
                            .cloned()
                            .filter(|record| record.document_id == document_id)
                    }
                };
                let Some(record) = record else {
                    let _ = send_ws(
                        &mut sink,
                        &Message::Close {
                            reason: "resume_unavailable".into(),
                        },
                    )
                    .await;
                    return Err(BridgeError::Auth);
                };
                let Some(generation) = record.generation.checked_add(1) else {
                    return Err(BridgeError::Limit);
                };
                (
                    record.session_id,
                    document_id,
                    generation,
                    revision,
                    editor_type,
                    capabilities,
                    AuthMode::Resume {
                        resume_id,
                        token: record.token,
                    },
                )
            }
            _ => return Err(BridgeError::Protocol),
        };

    let mut nonce_bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut nonce_bytes);
    let nonce = hex::encode(nonce_bytes);
    match &auth_mode {
        AuthMode::Pairing => {
            send_ws(
                &mut sink,
                &Message::Challenge {
                    session_id: session_id.clone(),
                    generation,
                    nonce: nonce.clone(),
                },
            )
            .await?;
        }
        AuthMode::Resume { resume_id, .. } => {
            send_ws(
                &mut sink,
                &Message::ResumeChallenge {
                    session_id: session_id.clone(),
                    generation,
                    nonce: nonce.clone(),
                    resume_id: resume_id.clone(),
                },
            )
            .await?;
        }
    }

    let auth = read_message(&mut source).await?;
    let authenticated = match (&auth_mode, auth) {
        (
            AuthMode::Pairing,
            Message::Authenticate {
                session_id: auth_session,
                generation: auth_generation,
                proof,
            },
        ) => {
            auth_session == session_id
                && auth_generation == generation
                && secret.verify(&nonce, &session_id, generation, &proof)
        }
        (
            AuthMode::Resume { resume_id, token },
            Message::ResumeAuthenticate {
                session_id: auth_session,
                generation: auth_generation,
                resume_id: auth_resume_id,
                proof,
            },
        ) => {
            auth_session == session_id
                && auth_generation == generation
                && auth_resume_id == *resume_id
                && verify_resume_proof(token, &nonce, &session_id, generation, &proof)
        }
        _ => false,
    };
    if !authenticated {
        bridge_log(
            "authentication_failed",
            serde_json::Map::from_iter([
                ("session_id".into(), json!(session_id)),
                ("generation".into(), json!(generation)),
                ("peer_addr".into(), json!(peer_addr)),
            ]),
        );
        let _ = send_ws(
            &mut sink,
            &Message::Close {
                reason: "authentication_failed".into(),
            },
        )
        .await;
        return Err(BridgeError::Auth);
    }

    if let AuthMode::Resume { resume_id, token } = &auth_mode {
        let mut guard = state.write().await;
        let now = Instant::now();
        let still_current = guard.resume.get(resume_id).is_some_and(|record| {
            record.token == *token
                && record.session_id == session_id
                && record.document_id == document_id
                && record.expires_at > now
                && record.generation.checked_add(1) == Some(generation)
        });
        if !still_current {
            drop(guard);
            let _ = send_ws(
                &mut sink,
                &Message::Close {
                    reason: "resume_replayed".into(),
                },
            )
            .await;
            return Err(BridgeError::Auth);
        }
        let record = guard
            .resume
            .get_mut(resume_id)
            .expect("validated resume credential");
        record.generation = generation;
        record.expires_at = now + RESUME_CREDENTIAL_TTL;
    }

    let (outbound_tx, mut outbound_rx) = mpsc::channel::<Message>(MAX_PENDING);
    let pending = Arc::new(Mutex::new(BTreeMap::new()));
    let expired = Arc::new(Mutex::new(BTreeSet::new()));
    let superseded = {
        let mut guard = state.write().await;
        if guard
            .sessions
            .get(&session_id)
            .is_some_and(|old| generation <= old.info.generation)
        {
            return Err(BridgeError::Stale);
        }
        guard.sessions.insert(
            session_id.clone(),
            SessionHandle {
                info: SessionInfo {
                    session_id: session_id.clone(),
                    document_id: document_id.clone(),
                    generation,
                    revision,
                    editor_type,
                    capabilities: capabilities
                        .into_iter()
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect(),
                },
                outbound: outbound_tx,
                pending: Arc::clone(&pending),
                expired: Arc::clone(&expired),
            },
        )
    };
    if let Some(old) = superseded {
        bridge_log(
            "supersession",
            serde_json::Map::from_iter([
                ("session_id".into(), json!(session_id)),
                ("generation".into(), json!(generation)),
                ("superseded_generation".into(), json!(old.info.generation)),
            ]),
        );
        retire_session(old, Some("Superseded by a newer Figma plugin generation")).await;
    }

    let (next_resume_id, next_resume_token) = match &auth_mode {
        AuthMode::Pairing => {
            let mut guard = state.write().await;
            issue_resume_credential(&mut guard, &session_id, &document_id, generation)
        }
        AuthMode::Resume { resume_id, token } => (resume_id.clone(), hex::encode(token)),
    };

    bridge_log(
        "authenticated",
        serde_json::Map::from_iter([
            ("session_id".into(), json!(session_id)),
            ("generation".into(), json!(generation)),
            ("peer_addr".into(), json!(peer_addr)),
        ]),
    );

    let connection_result = async {
        send_ws(
            &mut sink,
            &Message::Ready {
                session_id: session_id.clone(),
                generation,
                revision,
                resume_id: Some(next_resume_id.clone()),
                resume_token: Some(next_resume_token.clone()),
            },
        )
        .await?;

        let mut heartbeat = interval(HEARTBEAT_INTERVAL);
        heartbeat.set_missed_tick_behavior(MissedTickBehavior::Skip);
        heartbeat.tick().await;
        let mut last_seen = Instant::now();
        let mut heartbeat_seq = 0u64;

        loop {
            tokio::select! {
                outbound = outbound_rx.recv() => {
                    let Some(message) = outbound else { break; };
                    send_ws(&mut sink, &message).await?;
                }
                _ = heartbeat.tick() => {
                    if last_seen.elapsed() >= HEARTBEAT_DEADLINE {
                        return Err(BridgeError::Unavailable);
                    }
                    heartbeat_seq = heartbeat_seq.wrapping_add(1);
                    send_ws(
                        &mut sink,
                        &Message::Ping {
                            nonce: format!("{generation:x}-{heartbeat_seq:x}"),
                        },
                    )
                    .await?;
                }
                inbound = source.next() => {
                    let Some(frame) = inbound else { break; };
                    let frame = frame.map_err(|_| BridgeError::Io)?;
                    if frame.is_close() {
                        break;
                    }
                    if !frame.is_text() {
                        continue;
                    }
                    let message = parse_message(frame.into_data().as_ref())?;
                    last_seen = Instant::now();
                    match &message {
                        Message::Response {
                            id,
                            session_id: response_session,
                            generation: response_generation,
                            revision,
                            ..
                        } => {
                            if response_session != &session_id || *response_generation != generation {
                                return Err(BridgeError::Stale);
                            }
                            let sender = pending.lock().await.remove(id);
                            let was_expired = if sender.is_none() {
                                expired.lock().await.remove(id)
                            } else {
                                false
                            };
                            if sender.is_none() && !was_expired {
                                return Err(BridgeError::Duplicate);
                            }
                            {
                                let mut guard = state.write().await;
                                if let Some(active) = guard.sessions.get_mut(&session_id)
                                    && active.info.generation == generation
                                {
                                    active.info.revision = *revision;
                                }
                            }
                            if let Some(sender) = sender {
                                let _ = sender.send(message);
                            }
                        }
                        Message::Event {
                            session_id: event_session,
                            generation: event_generation,
                            revision,
                            kind,
                            payload,
                        } => {
                            if event_session != &session_id || *event_generation != generation {
                                return Err(BridgeError::Stale);
                            }
                            if !matches!(
                                kind.as_str(),
                                "selectionchange"
                                    | "currentpagechange"
                                    | "documentchange"
                                    | "documentchange_unavailable"
                            ) {
                                return Err(BridgeError::Protocol);
                            }
                            {
                                let mut guard = state.write().await;
                                if let Some(active) = guard.sessions.get_mut(&session_id)
                                    && active.info.generation == generation
                                {
                                    active.info.revision = (*revision).max(active.info.revision);
                                }
                            }
                            if let Some(events) = &events {
                                let _ = events.send(DriverChildEvent::Event {
                                    kind: format!("figma.{kind}"),
                                    payload: json!({
                                        "session_id": session_id,
                                        "generation": generation,
                                        "revision": revision,
                                        "data": payload,
                                    }),
                                });
                            }
                        }
                        Message::Pong { .. } => {}
                        Message::RevokeResume { resume_id } => {
                            let mut guard = state.write().await;
                            if let Some(record) = guard.resume.get(resume_id) {
                                if record.session_id != session_id {
                                    return Err(BridgeError::Protocol);
                                }
                                guard.resume.remove(resume_id);
                            }
                        }
                        Message::Close { reason } => {
                            if reason == "manual_disconnect" {
                                let mut guard = state.write().await;
                                guard.resume.retain(|_, record| record.session_id != session_id);
                            }
                            break;
                        }
                        _ => return Err(BridgeError::Protocol),
                    }
                }
            }
        }
        Ok(())
    }
    .await;

    let pending_count = pending.lock().await.len();
    if let Some(active) = take_session_generation(&state, &session_id, generation).await {
        retire_session(active, None).await;
    }
    // The connection owns this pending map even if a newer generation replaced it.
    // Clearing it guarantees every waiter wakes immediately on all exit paths.
    pending.lock().await.clear();
    expired.lock().await.clear();
    bridge_log(
        "disconnect",
        serde_json::Map::from_iter([
            ("session_id".into(), json!(session_id)),
            ("generation".into(), json!(generation)),
            ("peer_addr".into(), json!(peer_addr)),
            ("pending_requests".into(), json!(pending_count)),
            (
                "result".into(),
                json!(connection_result.as_ref().err().map(ToString::to_string)),
            ),
        ]),
    );
    connection_result
}

async fn read_message<S>(source: &mut S) -> Result<Message, BridgeError>
where
    S: Stream<Item = Result<WsMessage, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    let frame = source
        .next()
        .await
        .ok_or(BridgeError::Unavailable)?
        .map_err(|_| BridgeError::Io)?;
    if !frame.is_text() {
        return Err(BridgeError::Protocol);
    }
    parse_message(frame.into_data().as_ref())
}

async fn send_ws<S>(sink: &mut S, message: &Message) -> Result<(), BridgeError>
where
    S: Sink<WsMessage, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    let bytes = serde_json::to_vec(message).map_err(|_| BridgeError::Protocol)?;
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err(BridgeError::Limit);
    }
    let text = String::from_utf8(bytes).map_err(|_| BridgeError::Protocol)?;
    sink.send(WsMessage::Text(text.into()))
        .await
        .map_err(|_| BridgeError::Io)
}

pub fn parse_message(bytes: &[u8]) -> Result<Message, BridgeError> {
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err(BridgeError::Limit);
    }
    let mut de = serde_json::Deserializer::from_slice(bytes);
    let msg = Message::deserialize(&mut de).map_err(|_| BridgeError::Protocol)?;
    de.end().map_err(|_| BridgeError::Protocol)?;
    Ok(msg)
}

pub fn request_id() -> String {
    Uuid::new_v4().simple().to_string()
}

pub fn pairing_status(
    port: u16,
    code: &str,
    sessions: &[SessionInfo],
    listen_hosts: &[String],
) -> Value {
    json!({
        "bridge_protocol": BRIDGE_PROTOCOL_VERSION,
        "listen_host": "127.0.0.1",
        "listen_hosts": listen_hosts,
        "listen_port": port,
        "pairing_code": code,
        "pairing_code_ephemeral": true,
        "sessions": sessions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_tungstenite::MaybeTlsStream;
    use tokio_tungstenite::{WebSocketStream, connect_async};

    type Client = WebSocketStream<MaybeTlsStream<TcpStream>>;

    async fn send_client(ws: &mut Client, message: &Message) {
        let text = serde_json::to_string(message).expect("serialize client message");
        ws.send(WsMessage::Text(text.into()))
            .await
            .expect("send client message");
    }

    async fn recv_client(ws: &mut Client) -> Message {
        let frame = timeout(Duration::from_secs(2), ws.next())
            .await
            .expect("server response timeout")
            .expect("server closed before response")
            .expect("websocket response");
        assert!(frame.is_text(), "expected text bridge frame");
        parse_message(frame.into_data().as_ref()).expect("parse server message")
    }

    async fn hello_at(
        hub: &BridgeHub,
        host: &str,
        session: &str,
        generation: u64,
    ) -> (Client, String) {
        let url = if host.contains(':') {
            format!("ws://[{host}]:{}", hub.port())
        } else {
            format!("ws://{host}:{}", hub.port())
        };
        let (mut ws, _) = connect_async(url).await.expect("connect loopback bridge");
        send_client(
            &mut ws,
            &Message::Hello {
                protocol: BRIDGE_PROTOCOL_VERSION,
                plugin_build: "test-plugin".into(),
                figma_api: "1.138.0".into(),
                editor_type: "figma".into(),
                session_id: session.into(),
                document_id: "test-document".into(),
                generation,
                revision: 0,
                capabilities: vec!["design".into()],
            },
        )
        .await;
        let challenge = recv_client(&mut ws).await;
        let nonce = match challenge {
            Message::Challenge {
                session_id,
                generation: received_generation,
                nonce,
            } if session_id == session && received_generation == generation => nonce,
            other => panic!("expected server challenge, got {other:?}"),
        };
        assert_eq!(nonce.len(), 32);
        (ws, nonce)
    }

    async fn hello(hub: &BridgeHub, session: &str, generation: u64) -> (Client, String) {
        hello_at(hub, "127.0.0.1", session, generation).await
    }

    async fn authenticate_at(
        hub: &BridgeHub,
        host: &str,
        session: &str,
        generation: u64,
    ) -> Client {
        let (mut ws, nonce) = hello_at(hub, host, session, generation).await;
        let proof = hub.secret.proof(&nonce, session, generation);
        send_client(
            &mut ws,
            &Message::Authenticate {
                session_id: session.into(),
                generation,
                proof,
            },
        )
        .await;
        match recv_client(&mut ws).await {
            Message::Ready {
                session_id,
                generation: ready_generation,
                revision,
                ..
            } => {
                assert_eq!(session_id, session);
                assert_eq!(ready_generation, generation);
                assert_eq!(revision, 0);
            }
            other => panic!("expected Ready, got {other:?}"),
        }
        ws
    }

    async fn authenticate(hub: &BridgeHub, session: &str, generation: u64) -> Client {
        authenticate_at(hub, "127.0.0.1", session, generation).await
    }

    async fn wait_for_no_sessions(hub: &BridgeHub) {
        timeout(Duration::from_secs(2), async {
            loop {
                if hub.sessions().await.is_empty() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("session cleanup timed out");
    }

    #[tokio::test]
    async fn accepts_ipv4_and_ipv6_loopback_when_available() {
        let hub = BridgeHub::start_on(0, None).await.expect("start bridge");
        assert!(hub.listen_hosts().iter().any(|host| host == "127.0.0.1"));

        let mut ipv4 = authenticate_at(&hub, "127.0.0.1", "ipv4", 1).await;
        ipv4.close(None).await.expect("close ipv4 websocket");
        wait_for_no_sessions(&hub).await;

        if hub.listen_hosts().iter().any(|host| host == "::1") {
            let mut ipv6 = authenticate_at(&hub, "::1", "ipv6", 1).await;
            ipv6.close(None).await.expect("close ipv6 websocket");
            wait_for_no_sessions(&hub).await;
        }
    }

    #[tokio::test]
    async fn forced_disconnect_wakes_request_and_new_generation_recovers() {
        let hub = BridgeHub::start_on(0, None).await.expect("start bridge");
        let mut first = authenticate(&hub, "forced", 1).await;

        let execute = hub.execute(Some("forced"), "document.status", None, json!({}));
        let close = async {
            let request = recv_client(&mut first).await;
            assert!(matches!(request, Message::Request { .. }));
            first.close(None).await.expect("force close websocket");
        };
        let (result, ()) = tokio::join!(execute, close);
        assert!(matches!(result, Err(BridgeError::Unavailable)));
        wait_for_no_sessions(&hub).await;

        let mut second = authenticate(&hub, "forced", 2).await;
        let execute = hub.execute(Some("forced"), "document.status", None, json!({}));
        let respond = async {
            let request = recv_client(&mut second).await;
            let (id, session_id, generation) = match request {
                Message::Request {
                    id,
                    session_id,
                    generation,
                    ..
                } => (id, session_id, generation),
                other => panic!("expected request, got {other:?}"),
            };
            send_client(
                &mut second,
                &Message::Response {
                    id,
                    session_id,
                    generation,
                    revision: 1,
                    ok: true,
                    value: Some(json!({"status":"ok"})),
                    error: None,
                },
            )
            .await;
        };
        let (result, ()) = tokio::join!(execute, respond);
        assert_eq!(
            result.expect("request after reconnect"),
            json!({"status":"ok"})
        );
        let sessions = hub.sessions().await;
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].generation, 2);
        second
            .close(None)
            .await
            .expect("close reconnected websocket");
        wait_for_no_sessions(&hub).await;
    }

    #[test]
    fn legacy_camel_case_failure_envelope_is_accepted() {
        let raw = br#"{"type":"response","id":"req","session_id":"session","generation":1,"revision":0,"ok":false,"value":null,"error":{"code":"plugin_error","message":"boom","outcomeKnown":true}}"#;
        match parse_message(raw).expect("legacy failure envelope") {
            Message::Response {
                error: Some(error), ..
            } => {
                assert_eq!(error.code, "plugin_error");
                assert_eq!(error.message, "boom");
                assert!(error.outcome_known);
            }
            other => panic!("expected failure response, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn hello_revision_seeds_session_and_ready_revision() {
        let hub = BridgeHub::start_on(0, None).await.expect("start bridge");
        let url = format!("ws://127.0.0.1:{}", hub.port());
        let (mut ws, _) = connect_async(url).await.expect("connect loopback bridge");
        send_client(
            &mut ws,
            &Message::Hello {
                protocol: BRIDGE_PROTOCOL_VERSION,
                plugin_build: "revision-test".into(),
                figma_api: "1.139.0".into(),
                editor_type: "figma".into(),
                session_id: "revision-session".into(),
                document_id: "test-document".into(),
                generation: 1,
                revision: 37,
                capabilities: vec!["design".into()],
            },
        )
        .await;
        let nonce = match recv_client(&mut ws).await {
            Message::Challenge { nonce, .. } => nonce,
            other => panic!("expected challenge, got {other:?}"),
        };
        send_client(
            &mut ws,
            &Message::Authenticate {
                session_id: "revision-session".into(),
                generation: 1,
                proof: hub.secret.proof(&nonce, "revision-session", 1),
            },
        )
        .await;
        match recv_client(&mut ws).await {
            Message::Ready { revision, .. } => assert_eq!(revision, 37),
            other => panic!("expected ready, got {other:?}"),
        }
        let sessions = hub.sessions().await;
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].revision, 37);
        ws.close(None).await.expect("close websocket");
        wait_for_no_sessions(&hub).await;
    }

    #[tokio::test]
    async fn artifact_memory_ops_skip_implicit_revision_guard() {
        let hub = BridgeHub::start_on(0, None).await.expect("start bridge");
        let mut ws = authenticate(&hub, "artifact-revision", 1).await;
        {
            let mut state = hub.state.write().await;
            state
                .sessions
                .get_mut("artifact-revision")
                .unwrap()
                .info
                .revision = 7;
        }

        let execute = hub.execute(
            Some("artifact-revision"),
            "artifact.upload.begin",
            None,
            json!({"name":"fixture.bin","mediaType":"application/octet-stream"}),
        );
        let respond = async {
            let request = recv_client(&mut ws).await;
            let (id, session_id, generation, expected_revision) = match request {
                Message::Request {
                    id,
                    session_id,
                    generation,
                    expected_revision,
                    ..
                } => (id, session_id, generation, expected_revision),
                other => panic!("expected request, got {other:?}"),
            };
            assert_eq!(expected_revision, None);
            send_client(
                &mut ws,
                &Message::Response {
                    id,
                    session_id,
                    generation,
                    revision: 8,
                    ok: true,
                    value: Some(json!({"token":"fixture","bytes":0,"mediaType":"application/octet-stream","name":"fixture.bin"})),
                    error: None,
                },
            )
            .await;
        };
        let (result, ()) = tokio::join!(execute, respond);
        assert!(result.is_ok());
        ws.close(None).await.expect("close websocket");
    }

    #[tokio::test]
    async fn document_ops_keep_implicit_revision_guard() {
        let hub = BridgeHub::start_on(0, None).await.expect("start bridge");
        let mut ws = authenticate(&hub, "document-revision", 1).await;
        {
            let mut state = hub.state.write().await;
            state
                .sessions
                .get_mut("document-revision")
                .unwrap()
                .info
                .revision = 7;
        }

        let execute = hub.execute(
            Some("document-revision"),
            "document.status",
            None,
            json!({}),
        );
        let respond = async {
            let request = recv_client(&mut ws).await;
            let (id, session_id, generation, expected_revision) = match request {
                Message::Request {
                    id,
                    session_id,
                    generation,
                    expected_revision,
                    ..
                } => (id, session_id, generation, expected_revision),
                other => panic!("expected request, got {other:?}"),
            };
            assert_eq!(expected_revision, Some(7));
            send_client(
                &mut ws,
                &Message::Response {
                    id,
                    session_id,
                    generation,
                    revision: 7,
                    ok: true,
                    value: Some(json!({"revision":7})),
                    error: None,
                },
            )
            .await;
        };
        let (result, ()) = tokio::join!(execute, respond);
        assert!(result.is_ok());
        ws.close(None).await.expect("close websocket");
    }

    async fn initial_resume_credential(
        hub: &BridgeHub,
        session: &str,
        generation: u64,
    ) -> (Client, String, String) {
        let (mut ws, nonce) = hello(hub, session, generation).await;
        let proof = hub.secret.proof(&nonce, session, generation);
        send_client(
            &mut ws,
            &Message::Authenticate {
                session_id: session.into(),
                generation,
                proof,
            },
        )
        .await;
        match recv_client(&mut ws).await {
            Message::Ready {
                session_id,
                generation: ready_generation,
                resume_id: Some(resume_id),
                resume_token: Some(resume_token),
                ..
            } => {
                assert_eq!(session_id, session);
                assert_eq!(ready_generation, generation);
                assert_eq!(resume_id.len(), 32);
                assert_eq!(resume_token.len(), 64);
                (ws, resume_id, resume_token)
            }
            other => panic!("expected Ready with resume credential, got {other:?}"),
        }
    }

    async fn resume_with_credential(
        hub: &BridgeHub,
        resume_id: &str,
        resume_token: &str,
    ) -> (Client, String, u64, String, String) {
        let url = format!("ws://127.0.0.1:{}", hub.port());
        let (mut ws, _) = connect_async(url).await.expect("connect loopback bridge");
        send_client(
            &mut ws,
            &Message::ResumeHello {
                protocol: BRIDGE_PROTOCOL_VERSION,
                plugin_build: "test-plugin-resume".into(),
                figma_api: "1.138.0".into(),
                editor_type: "figma".into(),
                document_id: "test-document".into(),
                revision: 0,
                capabilities: vec!["design".into()],
                resume_id: resume_id.into(),
            },
        )
        .await;
        let (session_id, generation, nonce) = match recv_client(&mut ws).await {
            Message::ResumeChallenge {
                session_id,
                generation,
                nonce,
                resume_id: challenged_id,
            } => {
                assert_eq!(challenged_id, resume_id);
                (session_id, generation, nonce)
            }
            other => panic!("expected ResumeChallenge, got {other:?}"),
        };
        let token_vec = hex::decode(resume_token).expect("hex resume token");
        let token: [u8; 32] = token_vec.try_into().expect("32-byte resume token");
        let proof = resume_proof_bytes(&token, &nonce, &session_id, generation);
        send_client(
            &mut ws,
            &Message::ResumeAuthenticate {
                session_id: session_id.clone(),
                generation,
                resume_id: resume_id.into(),
                proof,
            },
        )
        .await;
        match recv_client(&mut ws).await {
            Message::Ready {
                session_id: ready_session,
                generation: ready_generation,
                resume_id: Some(next_id),
                resume_token: Some(next_token),
                ..
            } => {
                assert_eq!(ready_session, session_id);
                assert_eq!(ready_generation, generation);
                (ws, session_id, generation, next_id, next_token)
            }
            other => panic!("expected resumed Ready, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn trusted_resume_survives_multiple_plugin_restarts_without_storage_handoff() {
        let hub = BridgeHub::start_on(0, None).await.expect("start bridge");
        let (mut first, resume_id, resume_token) =
            initial_resume_credential(&hub, "trusted-resume", 1).await;
        first.close(None).await.expect("close initial plugin");
        wait_for_no_sessions(&hub).await;

        let (mut resumed, session_id, generation, next_id, next_token) =
            resume_with_credential(&hub, &resume_id, &resume_token).await;
        assert_eq!(session_id, "trusted-resume");
        assert_eq!(generation, 2);
        assert_eq!(next_id, resume_id);
        assert_eq!(next_token, resume_token);
        assert_eq!(hub.sessions().await[0].generation, 2);

        resumed
            .close(None)
            .await
            .expect("close first resumed plugin");
        wait_for_no_sessions(&hub).await;

        let (mut resumed_again, session_id, generation, next_id, next_token) =
            resume_with_credential(&hub, &resume_id, &resume_token).await;
        assert_eq!(session_id, "trusted-resume");
        assert_eq!(generation, 3);
        assert_eq!(next_id, resume_id);
        assert_eq!(next_token, resume_token);

        let sessions = hub.sessions().await;
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "trusted-resume");
        assert_eq!(sessions[0].generation, 3);

        resumed_again
            .close(None)
            .await
            .expect("close second resumed plugin");
    }

    #[tokio::test]
    async fn concurrent_resume_attempts_serialize_on_generation() {
        let hub = BridgeHub::start_on(0, None).await.expect("start bridge");
        let (mut first, resume_id, resume_token) =
            initial_resume_credential(&hub, "resume-concurrent", 1).await;
        first.close(None).await.expect("close initial plugin");
        wait_for_no_sessions(&hub).await;

        let url = format!("ws://127.0.0.1:{}", hub.port());
        let token_vec = hex::decode(&resume_token).expect("hex resume token");
        let token: [u8; 32] = token_vec.try_into().expect("32-byte resume token");

        let (mut a, _) = connect_async(&url).await.expect("connect resume a");
        let (mut b, _) = connect_async(&url).await.expect("connect resume b");
        for client in [&mut a, &mut b] {
            send_client(
                client,
                &Message::ResumeHello {
                    protocol: BRIDGE_PROTOCOL_VERSION,
                    plugin_build: "test-plugin-concurrent".into(),
                    figma_api: "1.138.0".into(),
                    editor_type: "figma".into(),
                    document_id: "test-document".into(),
                    revision: 0,
                    capabilities: vec!["design".into()],
                    resume_id: resume_id.clone(),
                },
            )
            .await;
        }

        let parse_challenge = |message: Message| match message {
            Message::ResumeChallenge {
                session_id,
                generation,
                nonce,
                resume_id: challenged_id,
            } => {
                assert_eq!(challenged_id, resume_id);
                (session_id, generation, nonce)
            }
            other => panic!("expected ResumeChallenge, got {other:?}"),
        };
        let ca = parse_challenge(recv_client(&mut a).await);
        let cb = parse_challenge(recv_client(&mut b).await);
        assert_eq!(ca.0, cb.0);
        assert_eq!(ca.1, 2);
        assert_eq!(cb.1, 2);
        assert_ne!(ca.2, cb.2, "concurrent resume nonces must be fresh");

        let proof_a = resume_proof_bytes(&token, &ca.2, &ca.0, ca.1);
        send_client(
            &mut a,
            &Message::ResumeAuthenticate {
                session_id: ca.0.clone(),
                generation: ca.1,
                resume_id: resume_id.clone(),
                proof: proof_a,
            },
        )
        .await;
        assert!(matches!(
            recv_client(&mut a).await,
            Message::Ready { generation: 2, .. }
        ));

        let proof_b = resume_proof_bytes(&token, &cb.2, &cb.0, cb.1);
        send_client(
            &mut b,
            &Message::ResumeAuthenticate {
                session_id: cb.0,
                generation: cb.1,
                resume_id,
                proof: proof_b,
            },
        )
        .await;
        match recv_client(&mut b).await {
            Message::Close { reason } => assert_eq!(reason, "resume_replayed"),
            other => panic!("second concurrent resume must be rejected, got {other:?}"),
        }

        let sessions = hub.sessions().await;
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "resume-concurrent");
        assert_eq!(sessions[0].generation, 2);
        a.close(None).await.expect("close winning resume");
    }

    #[tokio::test]
    async fn captured_resume_proof_cannot_authenticate_a_fresh_challenge() {
        let hub = BridgeHub::start_on(0, None).await.expect("start bridge");
        let (mut first, resume_id, resume_token) =
            initial_resume_credential(&hub, "resume-replay", 1).await;
        first.close(None).await.expect("close initial plugin");
        wait_for_no_sessions(&hub).await;

        let url = format!("ws://127.0.0.1:{}", hub.port());
        let token_vec = hex::decode(&resume_token).expect("hex resume token");
        let token: [u8; 32] = token_vec.try_into().expect("32-byte resume token");

        let (mut captured, _) = connect_async(&url).await.expect("connect captured resume");
        send_client(
            &mut captured,
            &Message::ResumeHello {
                protocol: BRIDGE_PROTOCOL_VERSION,
                plugin_build: "test-plugin-capture".into(),
                figma_api: "1.138.0".into(),
                editor_type: "figma".into(),
                document_id: "test-document".into(),
                revision: 0,
                capabilities: vec!["design".into()],
                resume_id: resume_id.clone(),
            },
        )
        .await;
        let (session_id, generation, nonce_one) = match recv_client(&mut captured).await {
            Message::ResumeChallenge {
                session_id,
                generation,
                nonce,
                ..
            } => (session_id, generation, nonce),
            other => panic!("expected first ResumeChallenge, got {other:?}"),
        };
        let captured_proof = resume_proof_bytes(&token, &nonce_one, &session_id, generation);
        drop(captured);

        let (mut replay, _) = connect_async(&url).await.expect("connect replay resume");
        send_client(
            &mut replay,
            &Message::ResumeHello {
                protocol: BRIDGE_PROTOCOL_VERSION,
                plugin_build: "test-plugin-replay".into(),
                figma_api: "1.138.0".into(),
                editor_type: "figma".into(),
                document_id: "test-document".into(),
                revision: 0,
                capabilities: vec!["design".into()],
                resume_id: resume_id.clone(),
            },
        )
        .await;
        let (replay_session, replay_generation, nonce_two) = match recv_client(&mut replay).await {
            Message::ResumeChallenge {
                session_id,
                generation,
                nonce,
                ..
            } => (session_id, generation, nonce),
            other => panic!("expected second ResumeChallenge, got {other:?}"),
        };
        assert_eq!(replay_session, session_id);
        assert_eq!(replay_generation, generation);
        assert_ne!(nonce_two, nonce_one, "resume challenge nonce must be fresh");

        send_client(
            &mut replay,
            &Message::ResumeAuthenticate {
                session_id: replay_session,
                generation: replay_generation,
                resume_id,
                proof: captured_proof,
            },
        )
        .await;
        match recv_client(&mut replay).await {
            Message::Close { reason } => assert_eq!(reason, "authentication_failed"),
            other => panic!("captured resume proof must be rejected, got {other:?}"),
        }
        assert!(hub.sessions().await.is_empty());
    }

    #[tokio::test]
    async fn expired_resume_is_removed_without_cross_document_revocation() {
        let hub = BridgeHub::start_on(0, None).await.expect("start bridge");
        let (mut ws, resume_id, _resume_token) = initial_resume_credential(&hub, "expiry", 1).await;
        ws.close(None).await.expect("close initial plugin");
        wait_for_no_sessions(&hub).await;

        let url = format!("ws://127.0.0.1:{}", hub.port());
        let (mut wrong_document, _) = connect_async(&url)
            .await
            .expect("connect wrong-document resume");
        send_client(
            &mut wrong_document,
            &Message::ResumeHello {
                protocol: BRIDGE_PROTOCOL_VERSION,
                plugin_build: "test-plugin-wrong-document".into(),
                figma_api: "1.138.0".into(),
                editor_type: "figma".into(),
                document_id: "other-document".into(),
                revision: 0,
                capabilities: vec!["design".into()],
                resume_id: resume_id.clone(),
            },
        )
        .await;
        match recv_client(&mut wrong_document).await {
            Message::Close { reason } => assert_eq!(reason, "resume_unavailable"),
            other => panic!("wrong-document resume must be unavailable, got {other:?}"),
        }
        assert!(
            hub.state.read().await.resume.contains_key(&resume_id),
            "wrong-document attempt must not revoke a valid credential"
        );

        hub.state
            .write()
            .await
            .resume
            .get_mut(&resume_id)
            .expect("resume credential")
            .expires_at = Instant::now() - Duration::from_secs(1);

        let (mut expired, _) = connect_async(url).await.expect("connect expired resume");
        send_client(
            &mut expired,
            &Message::ResumeHello {
                protocol: BRIDGE_PROTOCOL_VERSION,
                plugin_build: "test-plugin-expired".into(),
                figma_api: "1.138.0".into(),
                editor_type: "figma".into(),
                document_id: "test-document".into(),
                revision: 0,
                capabilities: vec!["design".into()],
                resume_id: resume_id.clone(),
            },
        )
        .await;
        match recv_client(&mut expired).await {
            Message::Close { reason } => assert_eq!(reason, "resume_unavailable"),
            other => panic!("expired resume must be unavailable, got {other:?}"),
        }
        assert!(
            !hub.state.read().await.resume.contains_key(&resume_id),
            "expired credential should be pruned when observed"
        );
    }

    #[tokio::test]
    async fn driver_restart_invalidates_trusted_resume_credential() {
        let hub = BridgeHub::start_on(0, None)
            .await
            .expect("start first bridge");
        let (mut ws, resume_id, _resume_token) =
            initial_resume_credential(&hub, "driver-restart", 1).await;
        ws.close(None).await.expect("close initial plugin");
        wait_for_no_sessions(&hub).await;
        drop(hub);

        let restarted = BridgeHub::start_on(0, None)
            .await
            .expect("start restarted bridge");
        let url = format!("ws://127.0.0.1:{}", restarted.port());
        let (mut client, _) = connect_async(url).await.expect("connect restarted bridge");
        send_client(
            &mut client,
            &Message::ResumeHello {
                protocol: BRIDGE_PROTOCOL_VERSION,
                plugin_build: "test-plugin-restart".into(),
                figma_api: "1.138.0".into(),
                editor_type: "figma".into(),
                document_id: "test-document".into(),
                revision: 0,
                capabilities: vec!["design".into()],
                resume_id,
            },
        )
        .await;
        match recv_client(&mut client).await {
            Message::Close { reason } => assert_eq!(reason, "resume_unavailable"),
            other => panic!("driver restart must invalidate resume credential, got {other:?}"),
        }
        assert!(restarted.sessions().await.is_empty());
    }

    #[tokio::test]
    async fn manual_disconnect_revokes_trusted_resume_credential() {
        let hub = BridgeHub::start_on(0, None).await.expect("start bridge");
        let (mut ws, resume_id, _) = initial_resume_credential(&hub, "trusted-revoke", 1).await;
        send_client(
            &mut ws,
            &Message::RevokeResume {
                resume_id: resume_id.clone(),
            },
        )
        .await;
        timeout(Duration::from_secs(2), async {
            loop {
                if !hub.state.read().await.resume.contains_key(&resume_id) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("resume credential revocation timed out");

        let url = format!("ws://127.0.0.1:{}", hub.port());
        let (mut rejected, _) = connect_async(url).await.expect("connect revoked resume");
        send_client(
            &mut rejected,
            &Message::ResumeHello {
                protocol: BRIDGE_PROTOCOL_VERSION,
                plugin_build: "test-plugin-revoked".into(),
                figma_api: "1.138.0".into(),
                editor_type: "figma".into(),
                document_id: "test-document".into(),
                revision: 0,
                capabilities: vec!["design".into()],
                resume_id,
            },
        )
        .await;
        match recv_client(&mut rejected).await {
            Message::Close { reason } => assert_eq!(reason, "resume_unavailable"),
            other => panic!("revoked credential must be unavailable, got {other:?}"),
        }
        ws.close(None).await.expect("close original session");
    }

    #[tokio::test]
    async fn wrong_hmac_never_registers_a_session() {
        let hub = BridgeHub::start_on(0, None).await.expect("start bridge");
        let (mut ws, _) = hello(&hub, "wrong-auth", 1).await;
        send_client(
            &mut ws,
            &Message::Authenticate {
                session_id: "wrong-auth".into(),
                generation: 1,
                proof: "00".repeat(32),
            },
        )
        .await;

        let result = timeout(Duration::from_secs(2), ws.next())
            .await
            .expect("authentication rejection timeout")
            .expect("server closed before authentication rejection")
            .expect("authentication rejection websocket frame");
        assert!(result.is_text(), "authentication rejection must be textual");
        match parse_message(result.into_data().as_ref()).expect("parse authentication rejection") {
            Message::Close { reason } => assert_eq!(reason, "authentication_failed"),
            other => panic!("invalid proof must receive Close, got {other:?}"),
        }
        assert!(hub.sessions().await.is_empty());
    }

    #[tokio::test]
    async fn valid_server_challenge_registers_the_session() {
        let hub = BridgeHub::start_on(0, None).await.expect("start bridge");
        let (mut ws, nonce) = hello(&hub, "valid-auth", 4).await;
        let proof = hub.secret.proof(&nonce, "valid-auth", 4);
        send_client(
            &mut ws,
            &Message::Authenticate {
                session_id: "valid-auth".into(),
                generation: 4,
                proof,
            },
        )
        .await;
        match recv_client(&mut ws).await {
            Message::Ready {
                session_id,
                generation,
                revision,
                ..
            } => {
                assert_eq!(session_id, "valid-auth");
                assert_eq!(generation, 4);
                assert_eq!(revision, 0);
            }
            other => panic!("expected Ready, got {other:?}"),
        }
        let sessions = hub.sessions().await;
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "valid-auth");
        ws.close(None).await.expect("close websocket");
    }

    #[tokio::test]
    async fn plugin_events_are_forwarded_as_driver_child_events() {
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let hub = BridgeHub::start_on(0, Some(event_tx))
            .await
            .expect("start bridge");
        let (mut ws, nonce) = hello(&hub, "events", 2).await;
        let proof = hub.secret.proof(&nonce, "events", 2);
        send_client(
            &mut ws,
            &Message::Authenticate {
                session_id: "events".into(),
                generation: 2,
                proof,
            },
        )
        .await;
        assert!(matches!(recv_client(&mut ws).await, Message::Ready { .. }));

        send_client(
            &mut ws,
            &Message::Event {
                session_id: "events".into(),
                generation: 2,
                revision: 3,
                kind: "selectionchange".into(),
                payload: serde_json::json!({"selected": 2}),
            },
        )
        .await;

        let event = timeout(Duration::from_secs(2), event_rx.recv())
            .await
            .expect("driver event timeout")
            .expect("driver event channel closed");
        match event {
            DriverChildEvent::Event { kind, payload } => {
                assert_eq!(kind, "figma.selectionchange");
                assert_eq!(payload["session_id"], "events");
                assert_eq!(payload["generation"], 2);
                assert_eq!(payload["revision"], 3);
                assert_eq!(payload["data"]["selected"], 2);
            }
            other => panic!("unexpected child event: {other:?}"),
        }
        assert_eq!(hub.sessions().await[0].revision, 3);
    }

    #[tokio::test]
    async fn captured_proof_cannot_be_replayed_on_a_new_connection() {
        let hub = BridgeHub::start_on(0, None).await.expect("start bridge");
        let (ws1, nonce1) = hello(&hub, "replay", 1).await;
        let captured = hub.secret.proof(&nonce1, "replay", 1);
        drop(ws1);

        let (mut ws2, nonce2) = hello(&hub, "replay", 1).await;
        assert_ne!(nonce1, nonce2, "server challenge must be fresh");
        send_client(
            &mut ws2,
            &Message::Authenticate {
                session_id: "replay".into(),
                generation: 1,
                proof: captured,
            },
        )
        .await;

        let result = timeout(Duration::from_secs(2), ws2.next()).await;
        match result {
            Ok(Some(Ok(frame))) if frame.is_text() => {
                let parsed = parse_message(frame.into_data().as_ref());
                assert!(
                    !matches!(parsed, Ok(Message::Ready { .. })),
                    "captured proof must not authenticate a fresh challenge"
                );
            }
            _ => {}
        }
        assert!(hub.sessions().await.is_empty());
    }

    #[tokio::test]
    async fn request_waits_through_short_reconnect_gap() {
        let hub = BridgeHub::start_on(0, None).await.expect("start bridge");

        let (result, mut ws) = tokio::join!(
            async {
                timeout(
                    Duration::from_secs(3),
                    hub.execute(
                        Some("late"),
                        "node.query",
                        None,
                        serde_json::json!({"nameEquals":"after-gap"}),
                    ),
                )
                .await
                .expect("request exceeded reconnect grace")
            },
            async {
                tokio::time::sleep(Duration::from_millis(100)).await;
                let mut ws = authenticate(&hub, "late", 1).await;
                let request = recv_client(&mut ws).await;
                let (id, session_id, generation) = match request {
                    Message::Request {
                        id,
                        session_id,
                        generation,
                        ..
                    } => (id, session_id, generation),
                    other => panic!("expected request, got {other:?}"),
                };
                send_client(
                    &mut ws,
                    &Message::Response {
                        id,
                        session_id,
                        generation,
                        revision: 1,
                        ok: true,
                        value: Some(serde_json::json!({"gap_hidden":true})),
                        error: None,
                    },
                )
                .await;
                ws
            }
        );

        assert_eq!(
            result.expect("execute through reconnect gap"),
            serde_json::json!({"gap_hidden":true})
        );
        ws.close(None).await.expect("close websocket");
        wait_for_no_sessions(&hub).await;
    }

    #[tokio::test]
    async fn late_response_after_timeout_keeps_session_usable() {
        let hub = BridgeHub::start_on(0, None).await.expect("start bridge");
        let mut ws = authenticate(&hub, "slow", 1).await;

        let (first_result, ()) = tokio::join!(
            async {
                hub.execute(
                    Some("slow"),
                    "node.query",
                    None,
                    serde_json::json!({"nameEquals":"slow"}),
                )
                .await
            },
            async {
                let request = recv_client(&mut ws).await;
                let (id, session_id, generation) = match request {
                    Message::Request {
                        id,
                        session_id,
                        generation,
                        ..
                    } => (id, session_id, generation),
                    other => panic!("expected request, got {other:?}"),
                };
                tokio::time::sleep(Duration::from_millis(250)).await;
                send_client(
                    &mut ws,
                    &Message::Response {
                        id,
                        session_id,
                        generation,
                        revision: 5,
                        ok: true,
                        value: Some(serde_json::json!({"late":true})),
                        error: None,
                    },
                )
                .await;
            }
        );
        assert!(matches!(first_result, Err(BridgeError::Timeout)));

        timeout(Duration::from_secs(1), async {
            loop {
                let sessions = hub.sessions().await;
                if sessions.len() == 1 && sessions[0].revision == 5 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("late response did not settle without disconnecting");

        let (second_result, ()) = tokio::join!(
            async {
                hub.execute(
                    Some("slow"),
                    "node.query",
                    None,
                    serde_json::json!({"nameEquals":"after-timeout"}),
                )
                .await
            },
            async {
                let request = recv_client(&mut ws).await;
                let (id, session_id, generation) = match request {
                    Message::Request {
                        id,
                        session_id,
                        generation,
                        ..
                    } => (id, session_id, generation),
                    other => panic!("expected request, got {other:?}"),
                };
                send_client(
                    &mut ws,
                    &Message::Response {
                        id,
                        session_id,
                        generation,
                        revision: 6,
                        ok: true,
                        value: Some(serde_json::json!({"still_connected":true})),
                        error: None,
                    },
                )
                .await;
            }
        );
        assert_eq!(
            second_result.expect("session should remain usable after late response"),
            serde_json::json!({"still_connected":true})
        );

        ws.close(None).await.expect("close websocket");
        wait_for_no_sessions(&hub).await;
    }

    #[tokio::test]
    async fn protocol_failure_retires_session_and_wakes_pending_request() {
        let hub = BridgeHub::start_on(0, None).await.expect("start bridge");
        let mut ws = authenticate(&hub, "broken", 1).await;
        assert_eq!(hub.sessions().await.len(), 1);

        let (result, ()) = tokio::join!(
            async {
                timeout(
                    Duration::from_secs(2),
                    hub.execute(
                        Some("broken"),
                        "node.query",
                        None,
                        serde_json::json!({"nameEquals":"x"}),
                    ),
                )
                .await
                .expect("pending request did not wake after protocol failure")
            },
            async {
                assert!(matches!(
                    recv_client(&mut ws).await,
                    Message::Request { .. }
                ));
                ws.send(WsMessage::Text("{not-json".into()))
                    .await
                    .expect("send malformed frame");
            }
        );

        assert!(matches!(result, Err(BridgeError::Unavailable)));
        wait_for_no_sessions(&hub).await;
    }

    #[tokio::test]
    async fn newer_generation_supersedes_pending_request_and_recovers() {
        let hub = BridgeHub::start_on(0, None).await.expect("start bridge");
        let mut ws1 = authenticate(&hub, "reconnect", 1).await;

        let (old_result, mut ws2) = tokio::join!(
            async {
                timeout(
                    Duration::from_secs(2),
                    hub.execute(
                        Some("reconnect"),
                        "node.query",
                        None,
                        serde_json::json!({"nameEquals":"before"}),
                    ),
                )
                .await
                .expect("superseded request did not wake")
            },
            async {
                assert!(matches!(
                    recv_client(&mut ws1).await,
                    Message::Request { .. }
                ));
                authenticate(&hub, "reconnect", 2).await
            }
        );
        assert!(matches!(old_result, Err(BridgeError::Unavailable)));

        let sessions = hub.sessions().await;
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "reconnect");
        assert_eq!(sessions[0].generation, 2);

        let (new_result, ()) = tokio::join!(
            async {
                timeout(
                    Duration::from_secs(2),
                    hub.execute(
                        Some("reconnect"),
                        "node.query",
                        None,
                        serde_json::json!({"nameEquals":"after"}),
                    ),
                )
                .await
                .expect("reconnected request timed out")
            },
            async {
                let request = recv_client(&mut ws2).await;
                let (id, session_id, generation) = match request {
                    Message::Request {
                        id,
                        session_id,
                        generation,
                        ..
                    } => (id, session_id, generation),
                    other => panic!("expected request, got {other:?}"),
                };
                send_client(
                    &mut ws2,
                    &Message::Response {
                        id,
                        session_id,
                        generation,
                        revision: 1,
                        ok: true,
                        value: Some(serde_json::json!({"recovered":true})),
                        error: None,
                    },
                )
                .await;
            }
        );
        assert_eq!(
            new_result.expect("reconnected execute"),
            serde_json::json!({"recovered":true})
        );
        ws2.close(None).await.expect("close reconnected websocket");
        wait_for_no_sessions(&hub).await;
    }
}
