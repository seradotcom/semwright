//! Peer-authenticated local IPC, resumable bounded sessions and request cancellation.
use semwright_core::Broker;
use semwright_protocol::{ClientMessage, ServerMessage, read_frame, write_frame};
#[cfg(unix)]
use semwright_protocol::{current_uid, private_directory};
use semwright_types::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
#[cfg(unix)]
use std::{
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::PathBuf,
};
#[cfg(unix)]
use tokio::net::UnixListener;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::{OwnedSemaphorePermit, Semaphore, mpsc},
    task::JoinSet,
};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
struct Active {
    token: CancellationToken,
    connection: String,
}
struct Session {
    touched: Instant,
    seen: BTreeSet<String>,
    active: BTreeMap<String, Active>,
}
#[derive(Default)]
struct Sessions {
    entries: Mutex<BTreeMap<String, Session>>,
}
impl Sessions {
    fn handshake(&self, ticket: Option<String>, broker: &Broker) -> Result<String> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "Session store lock poisoned"))?;
        let expired: Vec<_> = entries
            .iter()
            .filter(|(_, s)| s.touched.elapsed() > Duration::from_secs(1800) && s.active.is_empty())
            .map(|(id, _)| id.clone())
            .collect();
        for id in expired {
            entries.remove(&id);
            broker.revoke_session(&id);
        }
        if let Some(ticket) = ticket {
            valid_id(&ticket)?;
            let session=entries.get_mut(&ticket).ok_or_else(||Error::new(ErrorCode::StaleReference,"Broker session expired or belongs to an earlier broker process; reconnect with a fresh session"))?;
            session.touched = Instant::now();
            return Ok(ticket);
        }
        if entries.len() >= 32 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Broker session limit reached; reuse an existing session or wait for idle expiry",
            ));
        }
        let id = unique_id();
        entries.insert(
            id.clone(),
            Session {
                touched: Instant::now(),
                seen: BTreeSet::new(),
                active: BTreeMap::new(),
            },
        );
        Ok(id)
    }
    fn start(
        &self,
        session: &str,
        id: &str,
        connection: &str,
        token: CancellationToken,
        permits: &Arc<Semaphore>,
    ) -> Result<OwnedSemaphorePermit> {
        valid_id(id)?;
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "Session lock poisoned"))?;
        let state = entries
            .get_mut(session)
            .ok_or_else(|| Error::new(ErrorCode::StaleReference, "Session is no longer active"))?;
        if state.seen.contains(id) {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Request id was already used in this session; reconcile the original outcome before any new effect",
            )
            .uncertain());
        }
        if state.active.len() >= 16 || state.seen.len() >= 4096 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Session request budget exhausted",
            ));
        }
        // Admission and replay detection share the session lock. Exhaustion
        // must not disguise a previously submitted ID as a known non-effect.
        let permit = permits.clone().try_acquire_owned().map_err(|_| {
            Error::new(
                ErrorCode::ResourceExhausted,
                "Eight in-flight requests per connection are allowed",
            )
        })?;
        state.touched = Instant::now();
        state.seen.insert(id.into());
        state.active.insert(
            id.into(),
            Active {
                token,
                connection: connection.into(),
            },
        );
        Ok(permit)
    }
    fn finish(&self, session: &str, id: &str) {
        if let Ok(mut entries) = self.entries.lock()
            && let Some(state) = entries.get_mut(session)
        {
            state.active.remove(id);
            state.touched = Instant::now();
        }
    }
    fn cancel(&self, session: &str, id: &str) {
        if let Ok(entries) = self.entries.lock()
            && let Some(active) = entries.get(session).and_then(|state| state.active.get(id))
        {
            active.token.cancel();
        }
    }
    fn disconnect(&self, session: &str, connection: &str) {
        if let Ok(entries) = self.entries.lock()
            && let Some(state) = entries.get(session)
        {
            for active in state.active.values().filter(|a| a.connection == connection) {
                active.token.cancel();
            }
        }
    }
    fn cancel_all(&self) {
        if let Ok(entries) = self.entries.lock() {
            for session in entries.values() {
                for active in session.active.values() {
                    active.token.cancel();
                }
            }
        }
    }
}
fn valid_id(id: &str) -> Result<()> {
    if id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        Ok(())
    } else {
        Err(Error::invalid(
            "Request and session ids must contain 32 lowercase hexadecimal characters",
        ))
    }
}
struct Running {
    sessions: Arc<Sessions>,
    session: String,
    id: String,
}
impl Drop for Running {
    fn drop(&mut self) {
        self.sessions.finish(&self.session, &self.id);
    }
}
#[cfg(unix)]
struct SocketFile {
    path: PathBuf,
    inode: u64,
}
#[cfg(unix)]
impl Drop for SocketFile {
    fn drop(&mut self) {
        if std::fs::symlink_metadata(&self.path)
            .is_ok_and(|m| m.ino() == self.inode && m.uid() == current_uid())
        {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
async fn connection<S>(
    mut stream: S,
    authenticate_after_hello: fn(&S) -> Result<()>,
    broker: Arc<Broker>,
    sessions: Arc<Sessions>,
    stop: CancellationToken,
    jobs: TaskTracker,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let message = tokio::time::timeout(
        Duration::from_secs(5),
        read_frame::<_, ClientMessage>(&mut stream),
    )
    .await
    .map_err(|_| Error::new(ErrorCode::Timeout, "Broker handshake timed out"))??;
    // Windows pipe impersonation needs the last client message to have been read.
    // Authenticate the OS peer before interpreting any bearer ticket, sending a
    // response or installing a session. A failed check is silent and fail-closed.
    authenticate_after_hello(&stream)?;
    let session = match message {
        ClientMessage::Hello {
            version: PROTOCOL_VERSION,
            session,
        } => match sessions.handshake(session, &broker) {
            Ok(s) => s,
            Err(error) => {
                write_frame(&mut stream, &ServerMessage::Error { error }).await?;
                return Ok(());
            }
        },
        _ => {
            write_frame(
                &mut stream,
                &ServerMessage::Error {
                    error: Error::new(
                        ErrorCode::ProtocolMismatch,
                        "A compatible Hello frame must be first",
                    ),
                },
            )
            .await?;
            return Ok(());
        }
    };
    write_frame(
        &mut stream,
        &ServerMessage::Welcome {
            version: PROTOCOL_VERSION,
            session: session.clone(),
        },
    )
    .await?;
    let id = unique_id();
    let local_stop = stop.child_token();
    let (mut read, mut write) = tokio::io::split(stream);
    let (tx, mut rx) = mpsc::channel::<ServerMessage>(32);
    let writer_stop = local_stop.clone();
    let writer = tokio::spawn(async move {
        loop {
            let message =
                tokio::select! {_ = writer_stop.cancelled()=>break,message=rx.recv()=>message};
            let Some(message) = message else { break };
            if !matches!(
                tokio::time::timeout(Duration::from_secs(10), write_frame(&mut write, &message))
                    .await,
                Ok(Ok(()))
            ) {
                writer_stop.cancel();
                break;
            }
        }
    });
    let mut subscription = None;
    let permits = Arc::new(Semaphore::new(8));
    loop {
        let message = tokio::select! {_ = local_stop.cancelled()=>break,message=tokio::time::timeout(Duration::from_secs(300),read_frame::<_,ClientMessage>(&mut read))=>match message{Ok(Ok(message))=>message,_=>break}};
        match message {
            ClientMessage::Execute {
                id: request_id,
                request,
            } => {
                let token = local_stop.child_token();
                let permit =
                    match sessions.start(&session, &request_id, &id, token.clone(), &permits) {
                        Ok(permit) => permit,
                        Err(error) => {
                            let envelope = Envelope::finish(
                                request_id,
                                request.command,
                                "core".into(),
                                Duration::ZERO,
                                request.dry_run,
                                Err(error),
                            );
                            if tx
                                .send(ServerMessage::Result {
                                    envelope: Box::new(envelope),
                                })
                                .await
                                .is_err()
                            {
                                break;
                            }
                            continue;
                        }
                    };
                let running = Running {
                    sessions: sessions.clone(),
                    session: session.clone(),
                    id: request_id.clone(),
                };
                let broker = broker.clone();
                let session = session.clone();
                let tx = tx.clone();
                jobs.spawn(async move{let _permit=permit;let _running=running;
                    let deadline=token.clone();
                    let envelope=tokio::select!{
                        result=broker.execute(session,request_id.clone(),request.clone(),token)=>result,
                        _=tokio::time::sleep(Duration::from_secs(300))=>{deadline.cancel();Envelope::finish(request_id,request.command,"core".into(),Duration::from_secs(300),request.dry_run,Err(Error::new(ErrorCode::Timeout,"Broker request lifetime exceeded five minutes").uncertain()))},
                    };
                    let _=tokio::time::timeout(Duration::from_secs(5),tx.send(ServerMessage::Result{envelope:Box::new(envelope)})).await;
                });
            }
            ClientMessage::Cancel { id: request_id } => {
                if valid_id(&request_id).is_ok() {
                    sessions.cancel(&session, &request_id);
                }
            }
            ClientMessage::Ping {} => {
                if tx.send(ServerMessage::Pong).await.is_err() {
                    break;
                }
            }
            ClientMessage::Subscribe { after } => {
                if let Some(old) = subscription.take() {
                    let old: tokio::task::JoinHandle<()> = old;
                    old.abort();
                }
                // Subscribe before replay so events produced during replay are not lost; dedupe by sequence.
                let mut receiver = broker.subscribe();
                let replay = match broker.replay_for(&session, after) {
                    Ok(replay) => replay,
                    Err(error) => {
                        if tx.send(ServerMessage::Error { error }).await.is_err() {
                            break;
                        }
                        continue;
                    }
                };
                let tx = tx.clone();
                let cancel = local_stop.clone();
                let event_session = session.clone();
                subscription = Some(tokio::spawn(async move {
                    let mut last = after;
                    for event in replay {
                        last = event.sequence;
                        if tx
                            .send(ServerMessage::Event {
                                sequence: event.sequence,
                                event: event.event,
                            })
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                    loop {
                        let result = tokio::select! {_ = cancel.cancelled()=>return,result=receiver.recv()=>result};
                        match result {
                            Ok(event)
                                if event.sequence > last
                                    && event
                                        .audience
                                        .as_deref()
                                        .is_none_or(|audience| audience == event_session) =>
                            {
                                last = event.sequence;
                                if tx
                                    .send(ServerMessage::Event {
                                        sequence: event.sequence,
                                        event: event.event,
                                    })
                                    .await
                                    .is_err()
                                {
                                    return;
                                }
                            }
                            Ok(_) => (),
                            Err(_) => {
                                let _ = tx
                                    .send(ServerMessage::Error {
                                        error: Error::new(
                                            ErrorCode::Conflict,
                                            "Event subscriber lagged; re-snapshot and resubscribe",
                                        ),
                                    })
                                    .await;
                                return;
                            }
                        }
                    }
                }));
            }
            ClientMessage::Hello { .. } => {
                let _ = tx
                    .send(ServerMessage::Error {
                        error: Error::new(
                            ErrorCode::ProtocolMismatch,
                            "Hello cannot be repeated on an established connection",
                        ),
                    })
                    .await;
                break;
            }
        }
    }
    sessions.disconnect(&session, &id);
    local_stop.cancel();
    if let Some(task) = subscription {
        task.abort();
    }
    drop(tx);
    let _ = writer.await;
    Ok(())
}

#[cfg(unix)]
pub async fn serve(path: &Path, broker: Arc<Broker>, stop: CancellationToken) -> Result<()> {
    use semwright_platform_services::validate_peer;
    let parent = path
        .parent()
        .ok_or_else(|| Error::invalid("Broker socket requires an absolute parent directory"))?;
    private_directory(parent)?;
    if !path.is_absolute() {
        return Err(Error::invalid("Socket path must be absolute"));
    }
    if std::fs::symlink_metadata(path).is_ok() {
        return Err(Error::new(
            ErrorCode::Conflict,
            "Socket path already exists. Never replace it blindly; inspect its owner before removing a stale socket",
        ));
    }
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    let _socket = SocketFile {
        path: path.into(),
        inode: std::fs::symlink_metadata(path)?.ino(),
    };
    let sessions = Arc::new(Sessions::default());
    let connections = Arc::new(Semaphore::new(64));
    let jobs = TaskTracker::new();
    let mut handlers = JoinSet::new();
    loop {
        tokio::select! {
            _=stop.cancelled()=>break,
            joined=handlers.join_next(),if !handlers.is_empty()=>{let _=joined;},
            accepted=listener.accept()=>{
                let(stream,_)=accepted?;
                if validate_peer(&stream).is_err(){continue;}
                let Ok(permit)=connections.clone().try_acquire_owned()else{continue};
                let broker=broker.clone();let sessions=sessions.clone();let stop=stop.clone();let jobs=jobs.clone();
                handlers.spawn(async move{let _permit=permit;let _=connection(stream, |_| Ok(()), broker,sessions,stop,jobs).await;});
            },
        }
    }
    finish_server(sessions, handlers, jobs, broker).await
}

#[cfg(target_os = "windows")]
pub async fn serve(path: &Path, broker: Arc<Broker>, stop: CancellationToken) -> Result<()> {
    let sessions = Arc::new(Sessions::default());
    let connections = Arc::new(Semaphore::new(64));
    let jobs = TaskTracker::new();
    let mut handlers = JoinSet::new();
    let mut first = true;
    let mut listener = semwright_platform_services::windows_pipe_server(path, first)?;
    first = false;

    loop {
        tokio::select! {
            _=stop.cancelled()=>break,
            joined=handlers.join_next(),if !handlers.is_empty()=>{let _=joined;},
            connected=listener.connect()=>{
                connected?;
                // No impersonation at accept: the new client has not written Hello.
                let connected = listener;
                listener = semwright_platform_services::windows_pipe_server(path, first)?;
                let Ok(permit)=connections.clone().try_acquire_owned()else{continue};
                let broker=broker.clone();let sessions=sessions.clone();let stop=stop.clone();let jobs=jobs.clone();
                handlers.spawn(async move{
                    let _permit=permit;
                    let _=connection(
                        connected,
                        |pipe| semwright_platform_services::validate_windows_server_peer(pipe).map(|_| ()),
                        broker,sessions,stop,jobs
                    ).await;
                });
            },
        }
    }
    finish_server(sessions, handlers, jobs, broker).await
}

async fn finish_server(
    sessions: Arc<Sessions>,
    mut handlers: JoinSet<()>,
    jobs: TaskTracker,
    broker: Arc<Broker>,
) -> Result<()> {
    sessions.cancel_all();
    while handlers.join_next().await.is_some() {}
    jobs.close();
    let _ = tokio::time::timeout(Duration::from_secs(8), jobs.wait()).await;
    broker.shutdown().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_request_ids_fail() {
        for id in [
            "x",
            "",
            "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            "ABCDEFABCDEFABCDEFABCDEFABCDEFABCD",
        ] {
            assert!(valid_id(id).is_err());
        }
        assert!(valid_id(&unique_id()).is_ok());
    }
    #[test]
    fn replay_registration_fails() {
        let sessions = Sessions::default();
        let s = unique_id();
        sessions.entries.lock().unwrap().insert(
            s.clone(),
            Session {
                touched: Instant::now(),
                seen: BTreeSet::new(),
                active: BTreeMap::new(),
            },
        );
        let id = unique_id();
        let permits = Arc::new(Semaphore::new(8));
        let original = CancellationToken::new();
        let held = sessions
            .start(&s, &id, "c", original.clone(), &permits)
            .unwrap();
        for connection in ["c", "reconnected"] {
            let error = sessions
                .start(&s, &id, connection, CancellationToken::new(), &permits)
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::Conflict);
            assert!(
                !error.outcome_known,
                "The original effect may have happened"
            );
        }
        assert!(
            !original.is_cancelled(),
            "A duplicate must not cancel the original"
        );
        assert_eq!(permits.available_permits(), 7);
        sessions.finish(&s, &id);
        drop(held);
        let error = sessions
            .start(&s, &id, "reconnected", CancellationToken::new(), &permits)
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Conflict);
        assert!(!error.outcome_known);
        assert_eq!(permits.available_permits(), 8);
    }

    #[test]
    fn replay_registration_precedes_connection_budget() {
        let sessions = Sessions::default();
        let s = unique_id();
        sessions.entries.lock().unwrap().insert(
            s.clone(),
            Session {
                touched: Instant::now(),
                seen: BTreeSet::new(),
                active: BTreeMap::new(),
            },
        );
        let permits = Arc::new(Semaphore::new(8));
        let mut held = Vec::new();
        for _ in 0..8 {
            let id = unique_id();
            let permit = sessions
                .start(&s, &id, "c", CancellationToken::new(), &permits)
                .unwrap();
            held.push((id, permit));
        }
        assert_eq!(permits.available_permits(), 0);
        let duplicate = sessions
            .start(&s, &held[0].0, "c", CancellationToken::new(), &permits)
            .unwrap_err();
        assert_eq!(duplicate.code, ErrorCode::Conflict);
        assert!(!duplicate.outcome_known);

        let fresh = unique_id();
        let exhausted = sessions
            .start(&s, &fresh, "c", CancellationToken::new(), &permits)
            .unwrap_err();
        assert_eq!(exhausted.code, ErrorCode::ResourceExhausted);
        assert!(exhausted.outcome_known);
        {
            let entries = sessions.entries.lock().unwrap();
            let state = &entries[&s];
            assert_eq!(state.seen.len(), 8);
            assert_eq!(state.active.len(), 8);
            assert!(!state.seen.contains(&fresh));
        }
        let (completed, permit) = held.pop().unwrap();
        sessions.finish(&s, &completed);
        drop(permit);
        let accepted = sessions
            .start(&s, &fresh, "c", CancellationToken::new(), &permits)
            .unwrap();
        assert_eq!(permits.available_permits(), 0);
        sessions.finish(&s, &fresh);
        drop(accepted);
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    #[tokio::test]
    async fn replay_registration_wire_reconnect_preserves_original_file() {
        use semwright_core::{NoApprover, audit::Audit};
        use semwright_platform_common::filesystem::Filesystem;
        use semwright_policy::{FilesystemGrant, Policy, PolicyConfig};
        use serde_json::json;
        use tokio::io::DuplexStream;

        async fn connect_test(
            broker: Arc<Broker>,
            sessions: Arc<Sessions>,
            stop: CancellationToken,
            jobs: TaskTracker,
            ticket: Option<String>,
        ) -> (DuplexStream, String, tokio::task::JoinHandle<Result<()>>) {
            let (mut client, server) = tokio::io::duplex(65536);
            // In-memory transport only: real Windows peer checks are covered
            // by the separate native IPC certification.
            let handler =
                tokio::spawn(connection(server, |_| Ok(()), broker, sessions, stop, jobs));
            write_frame(
                &mut client,
                &ClientMessage::Hello {
                    version: PROTOCOL_VERSION,
                    session: ticket,
                },
            )
            .await
            .unwrap();
            let response = tokio::time::timeout(
                Duration::from_secs(5),
                read_frame::<_, ServerMessage>(&mut client),
            )
            .await
            .unwrap()
            .unwrap();
            let ServerMessage::Welcome { session, .. } = response else {
                panic!("Unexpected handshake response: {response:?}");
            };
            (client, session, handler)
        }

        async fn write_request(client: &mut DuplexStream, id: &str, content: &str) -> Envelope {
            write_frame(
                client,
                &ClientMessage::Execute {
                    id: id.into(),
                    request: ExecuteRequest {
                        command: "filesystem.write".into(),
                        args: json!({"root":"workspace","path":"effect.txt","text":content}),
                        dry_run: false,
                        backend: None,
                    },
                },
            )
            .await
            .unwrap();
            let response = tokio::time::timeout(
                Duration::from_secs(5),
                read_frame::<_, ServerMessage>(client),
            )
            .await
            .unwrap()
            .unwrap();
            let ServerMessage::Result { envelope } = response else {
                panic!("Unexpected execution response: {response:?}");
            };
            assert_eq!(envelope.request_id, id);
            *envelope
        }

        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let grant = FilesystemGrant {
            name: "workspace".into(),
            path: workspace.clone(),
            read: true,
            write: true,
        };
        let backend = Arc::new(Filesystem::new(std::slice::from_ref(&grant)).unwrap());
        let policy = Policy::new(PolicyConfig {
            filesystem: vec![grant],
            ..Default::default()
        })
        .unwrap();
        let broker = Broker::new(
            policy,
            vec![backend],
            Audit::open(&dir.path().join("audit"), 65536, 2).unwrap(),
            Arc::new(NoApprover),
            None,
            json!({}),
            false,
        )
        .unwrap();
        let sessions = Arc::new(Sessions::default());
        let stop = CancellationToken::new();
        let jobs = TaskTracker::new();
        let (mut client, session, handler) = connect_test(
            broker.clone(),
            sessions.clone(),
            stop.clone(),
            jobs.clone(),
            None,
        )
        .await;
        let request_id = unique_id();
        let first = write_request(&mut client, &request_id, "original").await;
        assert!(first.ok, "{first:?}");
        let path = workspace.join("effect.txt");
        assert_eq!(std::fs::read(&path).unwrap(), b"original");

        drop(client);
        tokio::time::timeout(Duration::from_secs(5), handler)
            .await
            .unwrap()
            .unwrap()
            .unwrap();

        let (mut client, resumed, handler) = connect_test(
            broker.clone(),
            sessions,
            stop.clone(),
            jobs.clone(),
            Some(session.clone()),
        )
        .await;
        assert_eq!(resumed, session);
        let replay = write_request(&mut client, &request_id, "duplicate effect").await;
        assert!(!replay.ok, "{replay:?}");
        let error = replay.error.unwrap();
        assert_eq!(error.code, ErrorCode::Conflict);
        assert!(!error.outcome_known);
        assert!(replay.execution.fallbacks_attempted.is_empty());
        assert_eq!(std::fs::read(&path).unwrap(), b"original");

        let fresh = write_request(&mut client, &unique_id(), "new authorized work").await;
        assert!(fresh.ok, "{fresh:?}");
        assert_eq!(std::fs::read(&path).unwrap(), b"new authorized work");
        drop(client);
        tokio::time::timeout(Duration::from_secs(5), handler)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        jobs.close();
        tokio::time::timeout(Duration::from_secs(5), jobs.wait())
            .await
            .unwrap();
        broker.shutdown().await;
    }
    #[test]
    fn disconnect_cancels_only_its_own_connection() {
        let sessions = Sessions::default();
        let s = unique_id();
        sessions.entries.lock().unwrap().insert(
            s.clone(),
            Session {
                touched: Instant::now(),
                seen: BTreeSet::new(),
                active: BTreeMap::new(),
            },
        );
        let a = CancellationToken::new();
        let b = CancellationToken::new();
        let permits_a = Arc::new(Semaphore::new(8));
        let permits_b = Arc::new(Semaphore::new(8));
        let _a = sessions
            .start(&s, &unique_id(), "a", a.clone(), &permits_a)
            .unwrap();
        let _b = sessions
            .start(&s, &unique_id(), "b", b.clone(), &permits_b)
            .unwrap();
        sessions.disconnect(&s, "a");
        assert!(a.is_cancelled());
        assert!(!b.is_cancelled());
    }
}
