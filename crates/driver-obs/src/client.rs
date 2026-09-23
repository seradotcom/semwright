//! Bounded OBS WebSocket actor.
//! A single task owns transport and request correlation. Calls are never replayed across reconnects.
use crate::{
    Fault, FaultKind, Result,
    auth::{Secret, proof},
    bounds,
    config::{Config, socket_secret},
    events::{EventEngine, SUBSCRIPTIONS},
    lifecycle::Operations,
    protocol::{self, Message as ObsMessage},
    refs::Registry,
    state::{ConnectionState, ReconnectBudget, RequestIds, Stamp},
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    net::TcpStream,
    sync::{Mutex, mpsc, oneshot, watch},
    task::JoinHandle,
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async_with_config,
    tungstenite::{Message, protocol::WebSocketConfig},
};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroize;

pub const MAX_PENDING: usize = 64;
type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;
type Reply = oneshot::Sender<Result<Value>>;

pub struct Shared {
    pub connection: ConnectionState,
    pub stamp: Stamp,
    pub obs_version: Option<String>,
    pub websocket_version: Option<String>,
    pub authenticated: bool,
    pub subscriptions: u32,
    pub available: BTreeSet<String>,
    pub last_error: Option<FaultKind>,
    pub reconnect_attempts: u64,
    pub events: EventEngine,
    pub refs: Registry,
    pub operations: Operations,
}
impl Shared {
    fn new(capacity: usize) -> Self {
        Self {
            connection: ConnectionState::Disconnected,
            stamp: Stamp {
                generation: 0,
                graph_revision: 0,
            },
            obs_version: None,
            websocket_version: None,
            authenticated: false,
            subscriptions: 0,
            available: BTreeSet::new(),
            last_error: None,
            reconnect_attempts: 0,
            events: EventEngine::new(capacity),
            refs: Registry::new(4096, Duration::from_secs(300)),
            operations: Operations::default(),
        }
    }
    pub fn health(&self) -> Value {
        json!({
            "driver_version":env!("CARGO_PKG_VERSION"),
            "connected":self.connection==ConnectionState::Ready,
            "authenticated":self.authenticated,
            "connection":self.connection,
            "obs_version":self.obs_version,
            "websocket_version":self.websocket_version,
            "rpc_version":if matches!(self.connection,ConnectionState::Ready|ConnectionState::Identified){Some(1u32)}else{None},
            "generation":self.stamp.generation,
            "graph_revision":self.stamp.graph_revision,
            "subscriptions":self.subscriptions,
            "event_queue_length":self.events.len(),
            "event_metrics":self.events.metrics,
            "cache_stale":self.events.cache_stale,
            "last_error":self.last_error,
            "reconnect_attempts":self.reconnect_attempts
        })
    }
    fn lost(&mut self) -> Result<()> {
        self.available.clear();
        self.authenticated = false;
        self.subscriptions = 0;
        self.obs_version = None;
        self.websocket_version = None;
        self.events
            .disconnected(&mut self.stamp, &mut self.refs, &mut self.operations)
    }
    fn event(&mut self, data: &Value, generation: u64) -> Result<()> {
        self.events.ingest(
            data,
            generation,
            &mut self.stamp,
            &mut self.refs,
            &mut self.operations,
        )
    }
}

#[derive(Clone)]
enum Wire {
    Single {
        kind: String,
        data: Value,
    },
    Batch {
        rows: Vec<(String, Value)>,
        halt: bool,
    },
    Subscriptions(u32),
}
struct Command {
    wire: Wire,
    generation: u64,
    stamp: Option<Stamp>,
    mutation: bool,
    deadline: Instant,
    cancel: CancellationToken,
    reply: Reply,
}
struct Pending {
    command: Command,
}
struct Inner {
    tx: mpsc::Sender<Command>,
    stop: CancellationToken,
    shared: Arc<Mutex<Shared>>,
    states: watch::Receiver<ConnectionState>,
    task: Mutex<Option<JoinHandle<()>>>,
    config: Config,
}
impl Drop for Inner {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}
#[derive(Clone)]
pub struct Client(Arc<Inner>);

impl Client {
    pub fn start(config: Config, fixture_secret: Option<Secret>) -> Result<Self> {
        config.validate()?;
        let (tx, rx) = mpsc::channel(MAX_PENDING);
        let (state_tx, states) = watch::channel(ConnectionState::Disconnected);
        let shared = Arc::new(Mutex::new(Shared::new(config.event_capacity)));
        let stop = CancellationToken::new();
        let task = tokio::spawn(run(
            config.clone(),
            fixture_secret,
            rx,
            shared.clone(),
            stop.clone(),
            state_tx,
        ));
        Ok(Self(Arc::new(Inner {
            tx,
            stop,
            shared,
            states,
            task: Mutex::new(Some(task)),
            config,
        })))
    }

    pub fn shared(&self) -> Arc<Mutex<Shared>> {
        self.0.shared.clone()
    }
    pub async fn health(&self) -> Value {
        self.0.shared.lock().await.health()
    }
    pub async fn stamp(&self) -> Stamp {
        self.0.shared.lock().await.stamp
    }

    pub async fn ready(&self) -> Result<()> {
        let mut states = self.0.states.clone();
        let wait = async {
            loop {
                let state = *states.borrow_and_update();
                match state {
                    ConnectionState::Ready => return Ok(()),
                    ConnectionState::Failed => {
                        let kind = self
                            .0
                            .shared
                            .lock()
                            .await
                            .last_error
                            .unwrap_or(FaultKind::Transport);
                        return Err(Fault::new(kind));
                    }
                    ConnectionState::Closed | ConnectionState::Closing => {
                        return Err(Fault::new(FaultKind::Closed));
                    }
                    _ => {}
                }
                states
                    .changed()
                    .await
                    .map_err(|_| Fault::new(FaultKind::Closed))?;
            }
        };
        tokio::select! {
            biased;
            _ = self.0.stop.cancelled() => Err(Fault::new(FaultKind::Closed)),
            answer = tokio::time::timeout(
                Duration::from_millis(self.0.config.connect_timeout_ms + 500),
                wait
            ) => answer.map_err(|_|Fault::new(FaultKind::Timeout))?,
        }
    }

    async fn submit(
        &self,
        wire: Wire,
        stamp: Option<Stamp>,
        mutation: bool,
        cancel: CancellationToken,
    ) -> Result<Value> {
        if self.0.stop.is_cancelled() {
            return Err(Fault::new(FaultKind::Closed));
        }
        if cancel.is_cancelled() {
            return Err(Fault::new(FaultKind::Cancelled));
        }
        let generation = {
            let shared = self.0.shared.lock().await;
            if shared.connection != ConnectionState::Ready {
                return Err(Fault::new(FaultKind::NotReady));
            }
            shared.stamp.generation
        };
        let (reply, wait) = oneshot::channel();
        let deadline = Instant::now() + self.0.config.timeout();
        let command = Command {
            wire,
            generation,
            stamp,
            mutation,
            deadline,
            cancel: cancel.clone(),
            reply,
        };
        self.0.tx.try_send(command).map_err(|error| match error {
            mpsc::error::TrySendError::Full(_) => Fault::new(FaultKind::ResourceLimit),
            mpsc::error::TrySendError::Closed(_) => Fault::new(FaultKind::Closed),
        })?;
        tokio::select! {
            biased;
            result = wait => result.map_err(|_|Fault::new(FaultKind::Closed).for_mutation(mutation))?,
            _ = tokio::time::sleep_until(
                tokio::time::Instant::from_std(deadline + Duration::from_millis(100))
            ) => {
                cancel.cancel();
                Err(Fault::new(FaultKind::Timeout).for_mutation(mutation))
            }
        }
    }

    pub async fn request(
        &self,
        kind: &str,
        data: Value,
        mutation: bool,
        stamp: Option<Stamp>,
        cancel: CancellationToken,
    ) -> Result<Value> {
        bounds::check(&data, 65_536)?;
        if !data.is_object() || kind.len() > 128 {
            return Err(Fault::new(FaultKind::Configuration));
        }
        self.submit(
            Wire::Single {
                kind: kind.into(),
                data,
            },
            stamp,
            mutation,
            cancel,
        )
        .await
    }

    pub async fn batch_read(
        &self,
        rows: Vec<(String, Value)>,
        halt: bool,
        cancel: CancellationToken,
    ) -> Result<Value> {
        if rows.is_empty()
            || rows.len() > 32
            || rows.iter().any(|(kind, _)| !kind.starts_with("Get"))
        {
            return Err(Fault::new(FaultKind::Configuration));
        }
        for (_, data) in &rows {
            bounds::check(data, 65_536)?;
            if !data.is_object() {
                return Err(Fault::new(FaultKind::Configuration));
            }
        }
        self.submit(Wire::Batch { rows, halt }, None, false, cancel)
            .await
    }

    pub async fn reidentify(&self, mask: u32) -> Result<Value> {
        if mask & !SUBSCRIPTIONS != 0 {
            return Err(Fault::new(FaultKind::Configuration));
        }
        self.submit(
            Wire::Subscriptions(mask),
            None,
            false,
            CancellationToken::new(),
        )
        .await
    }

    pub async fn shutdown(&self) -> Result<()> {
        self.0.stop.cancel();
        if let Some(mut task) = self.0.task.lock().await.take() {
            match tokio::time::timeout(Duration::from_secs(3), &mut task).await {
                Ok(Ok(())) => {}
                Ok(Err(_)) => return Err(Fault::new(FaultKind::Protocol)),
                Err(_) => {
                    task.abort();
                    let _ = task.await;
                    return Err(Fault::new(FaultKind::Timeout));
                }
            }
        }
        Ok(())
    }
}

async fn set_state(
    shared: &Arc<Mutex<Shared>>,
    states: &watch::Sender<ConnectionState>,
    next: ConnectionState,
) -> Result<()> {
    shared.lock().await.connection.transition(next)?;
    states.send_replace(next);
    Ok(())
}

async fn send(socket: &mut Socket, value: Value, stop: &CancellationToken) -> Result<()> {
    let text = serde_json::to_string(&value)?;
    if text.len() > bounds::MAX_FRAME {
        return Err(Fault::new(FaultKind::ResourceLimit));
    }
    tokio::select! {
        biased;
        _ = stop.cancelled() => Err(Fault::new(FaultKind::Cancelled)),
        result = tokio::time::timeout(
            Duration::from_secs(2),
            socket.send(Message::Text(text.into()))
        ) => {
            result
                .map_err(|_|Fault::new(FaultKind::Timeout))?
                .map_err(|_|Fault::new(FaultKind::Transport))
        }
    }
}

async fn receive(socket: &mut Socket) -> Result<ObsMessage> {
    loop {
        match socket.next().await {
            Some(Ok(Message::Text(text))) => return protocol::parse(text.as_bytes()),
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
            Some(Ok(Message::Close(frame))) => {
                return Err(protocol::close(
                    frame.map_or(1006, |frame| u16::from(frame.code)),
                ));
            }
            Some(Ok(_)) => return Err(Fault::new(FaultKind::Protocol)),
            _ => return Err(Fault::new(FaultKind::Transport)),
        }
    }
}

async fn connect(
    config: &Config,
    password: Option<&Secret>,
    shared: &Arc<Mutex<Shared>>,
    states: &watch::Sender<ConnectionState>,
    stop: &CancellationToken,
    ids: &mut RequestIds,
) -> Result<Socket> {
    let ws_config = WebSocketConfig::default()
        .read_buffer_size(8192)
        .write_buffer_size(8192)
        .max_write_buffer_size(524_288)
        .max_message_size(Some(bounds::MAX_FRAME))
        .max_frame_size(Some(bounds::MAX_FRAME));
    let uri = format!("ws://{}/", config.endpoint());
    let (mut socket, _) = connect_async_with_config(uri, Some(ws_config), true)
        .await
        .map_err(|_| Fault::new(FaultKind::Transport))?;
    set_state(shared, states, ConnectionState::Authenticating).await?;
    let hello = match receive(&mut socket).await? {
        ObsMessage::Hello(hello) => hello,
        _ => return Err(Fault::new(FaultKind::Protocol)),
    };

    let mut identify = json!({
        "op":1,
        "d":{
            "rpcVersion":1,
            "eventSubscriptions":SUBSCRIPTIONS
        }
    });
    let fetched = if config.secret_socket {
        Some(socket_secret().await?)
    } else {
        None
    };
    if let Some(authentication) = hello.get("authentication") {
        let secret = fetched
            .as_ref()
            .or(password)
            .ok_or_else(|| Fault::new(FaultKind::Authentication))?;
        let salt = authentication["salt"]
            .as_str()
            .ok_or_else(|| Fault::new(FaultKind::Authentication))?;
        let challenge = authentication["challenge"]
            .as_str()
            .ok_or_else(|| Fault::new(FaultKind::Authentication))?;
        let response = proof(secret, salt, challenge)?;
        identify["d"]["authentication"] = Value::String(response.to_string());
    } else if fetched.is_some() || password.is_some() {
        return Err(Fault::new(FaultKind::Authentication));
    }

    let sent = send(&mut socket, identify.clone(), stop).await;
    if let Some(Value::String(secret)) = identify["d"].get_mut("authentication") {
        secret.zeroize();
    }
    sent?;
    match receive(&mut socket).await? {
        ObsMessage::Identified(1) => {}
        _ => return Err(Fault::new(FaultKind::Protocol)),
    }
    shared.lock().await.authenticated = hello.get("authentication").is_some();
    set_state(shared, states, ConnectionState::Identified).await?;

    let generation = shared.lock().await.stamp.generation;
    let id = ids.next(generation, false)?;
    send(
        &mut socket,
        protocol::request(&id, "GetVersion", json!({}))?,
        stop,
    )
    .await?;
    loop {
        match receive(&mut socket).await? {
            ObsMessage::Response(response)
                if response.id == id && response.request_type == "GetVersion" =>
            {
                let version = response.result?;
                let obs = version["obsVersion"]
                    .as_str()
                    .filter(|value| value.len() <= 80 && !value.chars().any(char::is_control))
                    .ok_or_else(|| Fault::new(FaultKind::Protocol))?;
                let websocket = version["obsWebSocketVersion"]
                    .as_str()
                    .filter(|value| value.len() <= 80 && !value.chars().any(char::is_control))
                    .ok_or_else(|| Fault::new(FaultKind::Protocol))?;
                let requests = version["availableRequests"]
                    .as_array()
                    .filter(|value| value.len() <= 2048)
                    .ok_or_else(|| Fault::new(FaultKind::Protocol))?;
                let mut available = BTreeSet::new();
                for request in requests {
                    let request = request
                        .as_str()
                        .filter(|value| {
                            !value.is_empty()
                                && value.len() <= 128
                                && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
                        })
                        .ok_or_else(|| Fault::new(FaultKind::Protocol))?;
                    if !available.insert(request.to_string()) {
                        return Err(Fault::new(FaultKind::Protocol));
                    }
                }
                let mut state = shared.lock().await;
                state.obs_version = Some(obs.into());
                state.websocket_version = Some(websocket.into());
                state.available = available;
                state.last_error = None;
                state.subscriptions = SUBSCRIPTIONS;
                break;
            }
            ObsMessage::Event(data) => {
                shared.lock().await.event(&data, generation)?;
            }
            _ => return Err(Fault::new(FaultKind::Protocol)),
        }
    }
    set_state(shared, states, ConnectionState::Ready).await?;
    Ok(socket)
}

fn reject(command: Command, error: Fault) {
    let _ = command.reply.send(Err(error));
}

fn validate_batch(id: &str, rows: &[(String, Value)], halt: bool, results: &[Value]) -> Result<()> {
    if results.len() > rows.len() || (!halt && results.len() != rows.len()) || results.is_empty() {
        return Err(Fault::new(FaultKind::Protocol));
    }
    let mut failed = false;
    for (index, result) in results.iter().enumerate() {
        if failed && halt {
            return Err(Fault::new(FaultKind::Protocol));
        }
        let expected_id = format!("{id}:{index}");
        if result["requestType"] != rows[index].0
            || result
                .get("requestId")
                .is_some_and(|value| value != &expected_id)
        {
            return Err(Fault::new(FaultKind::Protocol));
        }
        let status = protocol::status(result);
        if status
            .as_ref()
            .err()
            .is_some_and(|error| error.kind == FaultKind::Protocol)
        {
            return Err(Fault::new(FaultKind::Protocol));
        }
        failed = status.is_err();
    }
    if results.len() < rows.len() && (!halt || !failed) {
        return Err(Fault::new(FaultKind::Protocol));
    }
    Ok(())
}

async fn session(
    socket: &mut Socket,
    rx: &mut mpsc::Receiver<Command>,
    shared: &Arc<Mutex<Shared>>,
    stop: &CancellationToken,
    ids: &mut RequestIds,
) -> Result<()> {
    let mut pending: BTreeMap<String, Pending> = BTreeMap::new();
    let mut subscription: Option<(String, Pending)> = None;
    let generation = shared.lock().await.stamp.generation;
    let mut timer = tokio::time::interval(Duration::from_millis(10));
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let failure = loop {
        tokio::select! {
            _ = stop.cancelled() => break Fault::new(FaultKind::Cancelled),
            _ = timer.tick() => {
                let now = Instant::now();
                let expired:Vec<_> = pending
                    .iter()
                    .filter(|(_,pending)| {
                        pending.command.deadline <= now
                            || pending.command.cancel.is_cancelled()
                            || pending.command.reply.is_closed()
                    })
                    .map(|(id,_)|id.clone())
                    .collect();
                for id in expired {
                    if let Some(pending) = pending.remove(&id) {
                        let kind = if pending.command.cancel.is_cancelled() {
                            FaultKind::Cancelled
                        } else {
                            FaultKind::Timeout
                        };
                        shared.lock().await.events.metrics.timeouts +=
                            u64::from(kind == FaultKind::Timeout);
                        let mutation = pending.command.mutation;
                        reject(
                            pending.command,
                            Fault::new(kind).for_mutation(mutation)
                        );
                    }
                }
                if subscription.as_ref().is_some_and(|(_,pending)| {
                    pending.command.deadline <= now || pending.command.cancel.is_cancelled()
                }) {
                    break Fault::new(FaultKind::Timeout);
                }
            },
            incoming = receive(socket) => match incoming {
                Err(error) => break error,
                Ok(ObsMessage::Event(data)) => {
                    if let Err(error) = shared.lock().await.event(&data,generation) {
                        break error;
                    }
                }
                Ok(ObsMessage::Response(response)) => {
                    if let Some(pending) = pending.remove(&response.id) {
                        let expected = match &pending.command.wire {
                            Wire::Single{kind,..} => Some(kind.as_str()),
                            _ => None,
                        };
                        let result = if expected == Some(response.request_type.as_str()) {
                            response.result.map_err(|error| {
                                error.for_mutation(pending.command.mutation)
                            })
                        } else {
                            Err(Fault::new(FaultKind::Protocol)
                                .for_mutation(pending.command.mutation))
                        };
                        let _ = pending.command.reply.send(result);
                    } else {
                        shared.lock().await.events.metrics.unknown_responses += 1;
                    }
                }
                Ok(ObsMessage::Batch{id,results}) => {
                    if let Some(pending) = pending.remove(&id) {
                        let result = match &pending.command.wire {
                            Wire::Batch{rows,halt} => {
                                validate_batch(&id,rows,*halt,&results)
                                    .map(|_|json!({"results":results}))
                            }
                            _ => Err(Fault::new(FaultKind::Protocol)),
                        };
                        let _ = pending.command.reply.send(result);
                    } else {
                        shared.lock().await.events.metrics.unknown_responses += 1;
                    }
                }
                Ok(ObsMessage::Identified(1)) => {
                    if let Some((_,pending)) = subscription.take() {
                        if let Wire::Subscriptions(mask) = pending.command.wire {
                            let mut state = shared.lock().await;
                            state.subscriptions = mask;
                            state.refs.invalidate();
                            state.stamp.invalidate()?;
                            let _ = pending.command.reply.send(
                                Ok(json!({"subscriptions":mask}))
                            );
                        }
                    } else {
                        break Fault::new(FaultKind::Protocol);
                    }
                }
                Ok(_) => break Fault::new(FaultKind::Protocol),
            },
            command = rx.recv() => {
                let Some(command) = command else {
                    break Fault::new(FaultKind::Closed);
                };
                if command.cancel.is_cancelled() || command.reply.is_closed() {
                    reject(command,Fault::new(FaultKind::Cancelled));
                    continue;
                }
                if command.deadline <= Instant::now() {
                    reject(command,Fault::new(FaultKind::Timeout));
                    continue;
                }
                if pending.len() + usize::from(subscription.is_some()) >= MAX_PENDING {
                    reject(command,Fault::new(FaultKind::ResourceLimit));
                    continue;
                }
                let state = shared.lock().await;
                if command.generation != state.stamp.generation
                    || command.stamp.is_some_and(|stamp| stamp != state.stamp)
                {
                    drop(state);
                    reject(command,Fault::new(FaultKind::StaleReference));
                    continue;
                }
                if command.mutation && state.subscriptions != SUBSCRIPTIONS {
                    drop(state);
                    reject(command,Fault::new(FaultKind::Precondition));
                    continue;
                }
                let supported = match &command.wire {
                    Wire::Single{kind,..} => state.available.contains(kind),
                    Wire::Batch{rows,..} => rows.iter().all(|(kind,_)|state.available.contains(kind)),
                    Wire::Subscriptions(_) => true,
                };
                drop(state);
                if !supported {
                    reject(command,Fault::new(FaultKind::Unsupported));
                    continue;
                }
                if matches!(&command.wire,Wire::Subscriptions(_)) && subscription.is_some() {
                    reject(command,Fault::new(FaultKind::Precondition));
                    continue;
                }
                let id = ids.next(generation,matches!(&command.wire,Wire::Batch{..}))?;
                let message = match &command.wire {
                    Wire::Single{kind,data} => protocol::request(&id,kind,data.clone()),
                    Wire::Batch{rows,halt} => protocol::batch(&id,rows,*halt),
                    Wire::Subscriptions(mask) => {
                        Ok(json!({"op":3,"d":{"eventSubscriptions":mask}}))
                    }
                };
                let message = match message {
                    Ok(value) => value,
                    Err(error) => {
                        reject(command,error);
                        continue;
                    }
                };
                if matches!(&command.wire,Wire::Subscriptions(_)) {
                    subscription = Some((id.clone(),Pending{command}));
                } else {
                    pending.insert(id,Pending{command});
                }
                if let Err(error) = send(socket,message,stop).await {
                    break error;
                }
            }
        }
    };

    for (_, pending) in pending {
        let mutation = pending.command.mutation;
        reject(pending.command, failure.clone().for_mutation(mutation));
    }
    if let Some((_, pending)) = subscription {
        reject(pending.command, failure.clone());
    }
    Err(failure)
}

async fn run(
    config: Config,
    password: Option<Secret>,
    mut rx: mpsc::Receiver<Command>,
    shared: Arc<Mutex<Shared>>,
    stop: CancellationToken,
    states: watch::Sender<ConnectionState>,
) {
    let mut budget = ReconnectBudget::new(config.reconnect_limit);
    let mut ids = RequestIds::default();

    loop {
        if stop.is_cancelled() {
            break;
        }
        let delay = match budget.reserve(Instant::now()) {
            Ok(delay) => delay,
            Err(error) => {
                shared.lock().await.last_error = Some(error.kind);
                let _ = set_state(&shared, &states, ConnectionState::Failed).await;
                break;
            }
        };
        tokio::select! {
            _ = stop.cancelled() => break,
            _ = tokio::time::sleep(delay) => {}
        }
        if set_state(&shared, &states, ConnectionState::Connecting)
            .await
            .is_err()
        {
            break;
        }
        {
            let mut state = shared.lock().await;
            state.reconnect_attempts += 1;
            if state.stamp.next_connection().is_err() {
                break;
            }
        }

        let connection = tokio::select! {
            biased;
            _ = stop.cancelled() => Err(Fault::new(FaultKind::Cancelled)),
            answer = tokio::time::timeout(
                Duration::from_millis(config.connect_timeout_ms),
                connect(&config,password.as_ref(),&shared,&states,&stop,&mut ids)
            ) => answer.unwrap_or_else(|_|Err(Fault::new(FaultKind::Timeout))),
        };

        let failure = match connection {
            Ok(mut socket) => {
                let error = session(&mut socket, &mut rx, &shared, &stop, &mut ids)
                    .await
                    .err()
                    .unwrap_or_else(|| Fault::new(FaultKind::Closed));
                let _ = tokio::time::timeout(Duration::from_millis(200), socket.close(None)).await;
                error
            }
            Err(error) => error,
        };

        {
            let mut state = shared.lock().await;
            state.last_error = Some(failure.kind);
            let _ = state.lost();
        }
        while let Ok(command) = rx.try_recv() {
            reject(command, Fault::new(FaultKind::NotReady));
        }
        if stop.is_cancelled() {
            break;
        }
        if !failure.retryable_connection() {
            let _ = set_state(&shared, &states, ConnectionState::Failed).await;
            break;
        }
        if set_state(&shared, &states, ConnectionState::Reconnecting)
            .await
            .is_err()
        {
            break;
        }
    }

    if shared.lock().await.connection == ConnectionState::Failed && !stop.is_cancelled() {
        loop {
            tokio::select! {
                _ = stop.cancelled() => break,
                command = rx.recv() => match command {
                    Some(command) => reject(command,Fault::new(FaultKind::Closed)),
                    None => break,
                }
            }
        }
    }

    let _ = set_state(&shared, &states, ConnectionState::Closing).await;
    rx.close();
    while let Some(command) = rx.recv().await {
        reject(command, Fault::new(FaultKind::Closed));
    }
    {
        let mut state = shared.lock().await;
        let _ = state.lost();
    }
    drop(password);
    let _ = set_state(&shared, &states, ConnectionState::Closed).await;
}
