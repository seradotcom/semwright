use crate::model::{BRIDGE_PROTOCOL_VERSION, MAX_MESSAGE_BYTES};
use futures_util::{Sink, SinkExt, Stream, StreamExt};
use hmac::{Hmac, Mac};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::Sha256;
use std::{
    collections::{BTreeMap, BTreeSet},
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};
use thiserror::Error;
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{Mutex, RwLock, mpsc, oneshot},
    task::JoinHandle,
    time::timeout,
};
use tokio_tungstenite::{accept_async, tungstenite::Message as WsMessage};
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;
const DEFAULT_PORT: u16 = 38_471;
const MAX_PENDING: usize = 128;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(25);

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
        capabilities: Vec<String>,
    },
    Challenge {
        session_id: String,
        generation: u64,
        nonce: String,
    },
    Authenticate {
        session_id: String,
        generation: u64,
        proof: String,
    },
    Ready {
        session_id: String,
        generation: u64,
        revision: u64,
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
    Close {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BridgeFailure {
    pub code: String,
    pub message: String,
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
}

#[derive(Default)]
struct HubState {
    sessions: BTreeMap<String, SessionHandle>,
}

pub struct BridgeHub {
    addr: SocketAddr,
    secret: PairingSecret,
    state: Arc<RwLock<HubState>>,
    task: JoinHandle<()>,
}

impl BridgeHub {
    pub async fn start() -> Result<Self, BridgeError> {
        let port = std::env::var("SEMWRIGHT_FIGMA_BRIDGE_PORT")
            .ok()
            .map(|raw| raw.parse::<u16>().map_err(|_| BridgeError::Protocol))
            .transpose()?
            .unwrap_or(DEFAULT_PORT);
        Self::start_on(port).await
    }

    async fn start_on(port: u16) -> Result<Self, BridgeError> {
        let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port))
            .await
            .map_err(|_| BridgeError::Io)?;
        let addr = listener.local_addr().map_err(|_| BridgeError::Io)?;
        let secret = PairingSecret::random();
        let state = Arc::new(RwLock::new(HubState::default()));
        let accept_state = Arc::clone(&state);
        let accept_secret = secret.clone();
        let task = tokio::spawn(async move {
            while let Ok((stream, peer)) = listener.accept().await {
                if !peer.ip().is_loopback() {
                    continue;
                }
                let state = Arc::clone(&accept_state);
                let secret = accept_secret.clone();
                tokio::spawn(async move {
                    let _ = serve_connection(stream, state, secret).await;
                });
            }
        });
        Ok(Self {
            addr,
            secret,
            state,
            task,
        })
    }

    pub fn port(&self) -> u16 {
        self.addr.port()
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
        let (chosen_id, generation, revision, tx, pending) = {
            let state = self.state.read().await;
            let session = match session_id {
                Some(id) => state.sessions.get(id).ok_or(BridgeError::Unavailable)?,
                None if state.sessions.len() == 1 => {
                    state.sessions.values().next().expect("len checked")
                }
                None => return Err(BridgeError::Unavailable),
            };
            if expected_revision.is_some_and(|rev| rev != session.info.revision) {
                return Err(BridgeError::Stale);
            }
            (
                session.info.session_id.clone(),
                session.info.generation,
                session.info.revision,
                session.outbound.clone(),
                Arc::clone(&session.pending),
            )
        };

        let id = request_id();
        let (reply_tx, reply_rx) = oneshot::channel();
        {
            let mut map = pending.lock().await;
            if map.len() >= MAX_PENDING || map.contains_key(&id) {
                return Err(BridgeError::Limit);
            }
            map.insert(id.clone(), reply_tx);
        }

        let request = Message::Request {
            id: id.clone(),
            session_id: chosen_id.clone(),
            generation,
            expected_revision: expected_revision.or(Some(revision)),
            operation: operation.to_owned(),
            args,
        };
        if tx.send(request).await.is_err() {
            pending.lock().await.remove(&id);
            return Err(BridgeError::Unavailable);
        }

        let response = match timeout(REQUEST_TIMEOUT, reply_rx).await {
            Ok(Ok(message)) => message,
            Ok(Err(_)) => return Err(BridgeError::Unavailable),
            Err(_) => {
                pending.lock().await.remove(&id);
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
        self.task.abort();
    }
}

async fn serve_connection(
    stream: TcpStream,
    state: Arc<RwLock<HubState>>,
    secret: PairingSecret,
) -> Result<(), BridgeError> {
    let ws = accept_async(stream)
        .await
        .map_err(|_| BridgeError::Protocol)?;
    let (mut sink, mut source) = ws.split();

    let hello = read_message(&mut source).await?;
    let (session_id, document_id, generation, editor_type, capabilities) = match hello {
        Message::Hello {
            protocol,
            plugin_build,
            figma_api,
            editor_type,
            session_id,
            document_id,
            generation,
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
                editor_type,
                capabilities,
            )
        }
        _ => return Err(BridgeError::Protocol),
    };

    let mut nonce_bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut nonce_bytes);
    let nonce = hex::encode(nonce_bytes);
    send_ws(
        &mut sink,
        &Message::Challenge {
            session_id: session_id.clone(),
            generation,
            nonce: nonce.clone(),
        },
    )
    .await?;

    let auth = read_message(&mut source).await?;
    match auth {
        Message::Authenticate {
            session_id: auth_session,
            generation: auth_generation,
            proof,
        } if auth_session == session_id
            && auth_generation == generation
            && secret.verify(&nonce, &session_id, generation, &proof) => {}
        _ => return Err(BridgeError::Auth),
    }

    let (outbound_tx, mut outbound_rx) = mpsc::channel::<Message>(MAX_PENDING);
    let pending = Arc::new(Mutex::new(BTreeMap::new()));
    {
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
                    document_id,
                    generation,
                    revision: 0,
                    editor_type,
                    capabilities: capabilities
                        .into_iter()
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect(),
                },
                outbound: outbound_tx,
                pending: Arc::clone(&pending),
            },
        );
    }

    send_ws(
        &mut sink,
        &Message::Ready {
            session_id: session_id.clone(),
            generation,
            revision: 0,
        },
    )
    .await?;

    loop {
        tokio::select! {
            outbound = outbound_rx.recv() => {
                let Some(message) = outbound else { break; };
                send_ws(&mut sink, &message).await?;
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
                        let sender = pending
                            .lock()
                            .await
                            .remove(id)
                            .ok_or(BridgeError::Duplicate)?;
                        {
                            let mut guard = state.write().await;
                            if let Some(active) = guard.sessions.get_mut(&session_id)
                                && active.info.generation == generation
                            {
                                active.info.revision = *revision;
                            }
                        }
                        let _ = sender.send(message);
                    }
                    Message::Event {
                        session_id: event_session,
                        generation: event_generation,
                        revision,
                        ..
                    } => {
                        if event_session != &session_id || *event_generation != generation {
                            return Err(BridgeError::Stale);
                        }
                        let mut guard = state.write().await;
                        if let Some(active) = guard.sessions.get_mut(&session_id) {
                            active.info.revision = (*revision).max(active.info.revision);
                        }
                    }
                    Message::Pong { .. } => {}
                    Message::Close { .. } => break,
                    _ => return Err(BridgeError::Protocol),
                }
            }
        }
    }

    {
        let mut guard = state.write().await;
        if guard
            .sessions
            .get(&session_id)
            .is_some_and(|active| active.info.generation == generation)
        {
            guard.sessions.remove(&session_id);
        }
    }
    pending.lock().await.clear();
    Ok(())
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

pub fn pairing_status(port: u16, code: &str, sessions: &[SessionInfo]) -> Value {
    json!({
        "bridge_protocol": BRIDGE_PROTOCOL_VERSION,
        "listen_host": "127.0.0.1",
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

    async fn hello(hub: &BridgeHub, session: &str, generation: u64) -> (Client, String) {
        let url = format!("ws://127.0.0.1:{}", hub.port());
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

    #[tokio::test]
    async fn wrong_hmac_never_registers_a_session() {
        let hub = BridgeHub::start_on(0).await.expect("start bridge");
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

        let result = timeout(Duration::from_secs(2), ws.next()).await;
        match result {
            Ok(Some(Ok(frame))) if frame.is_text() => {
                let parsed = parse_message(frame.into_data().as_ref());
                assert!(
                    !matches!(parsed, Ok(Message::Ready { .. })),
                    "invalid proof must never receive Ready"
                );
            }
            _ => {}
        }
        assert!(hub.sessions().await.is_empty());
    }

    #[tokio::test]
    async fn valid_server_challenge_registers_the_session() {
        let hub = BridgeHub::start_on(0).await.expect("start bridge");
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
    async fn captured_proof_cannot_be_replayed_on_a_new_connection() {
        let hub = BridgeHub::start_on(0).await.expect("start bridge");
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
}
