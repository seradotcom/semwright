//! Peer-authenticated local IPC, resumable bounded sessions and request cancellation.
use semwright_core::Broker;
use semwright_protocol::{
    ClientMessage, ServerMessage, current_uid, private_directory, read_frame, validate_peer,
    write_frame,
};
use semwright_types::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{
    net::{UnixListener, UnixStream},
    sync::{Semaphore, mpsc},
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
    ) -> Result<()> {
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
                "Request id was already used in this session; replaying mutations is forbidden",
            ));
        }
        if state.active.len() >= 16 || state.seen.len() >= 4096 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Session request budget exhausted",
            ));
        }
        state.touched = Instant::now();
        state.seen.insert(id.into());
        state.active.insert(
            id.into(),
            Active {
                token,
                connection: connection.into(),
            },
        );
        Ok(())
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
struct SocketFile {
    path: PathBuf,
    inode: u64,
}
impl Drop for SocketFile {
    fn drop(&mut self) {
        if std::fs::symlink_metadata(&self.path)
            .is_ok_and(|m| m.ino() == self.inode && m.uid() == current_uid())
        {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
async fn connection(
    mut stream: UnixStream,
    broker: Arc<Broker>,
    sessions: Arc<Sessions>,
    stop: CancellationToken,
    jobs: TaskTracker,
) -> Result<()> {
    validate_peer(&stream)?;
    let message = tokio::time::timeout(
        Duration::from_secs(5),
        read_frame::<_, ClientMessage>(&mut stream),
    )
    .await
    .map_err(|_| Error::new(ErrorCode::Timeout, "Broker handshake timed out"))??;
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
    let (read, write) = stream.into_split();
    let mut read = read;
    let mut write = write;
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
                let permit = match permits.clone().try_acquire_owned() {
                    Ok(permit) => permit,
                    Err(_) => {
                        let envelope = Envelope::finish(
                            request_id,
                            request.command,
                            "core".into(),
                            Duration::ZERO,
                            request.dry_run,
                            Err(Error::new(
                                ErrorCode::ResourceExhausted,
                                "Eight in-flight requests per connection are allowed",
                            )),
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
                let token = local_stop.child_token();
                if let Err(error) = sessions.start(&session, &request_id, &id, token.clone()) {
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
pub async fn serve(path: &Path, broker: Arc<Broker>, stop: CancellationToken) -> Result<()> {
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
                let(stream,_)=accepted?;if validate_peer(&stream).is_err(){continue;}
                let Ok(permit)=connections.clone().try_acquire_owned()else{continue};
                let broker=broker.clone();let sessions=sessions.clone();let stop=stop.clone();let jobs=jobs.clone();
                handlers.spawn(async move{let _permit=permit;let _=connection(stream,broker,sessions,stop,jobs).await;});
            },
        }
    }
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
        sessions
            .start(&s, &id, "c", CancellationToken::new())
            .unwrap();
        sessions.finish(&s, &id);
        assert_eq!(
            sessions
                .start(&s, &id, "c", CancellationToken::new())
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
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
        sessions.start(&s, &unique_id(), "a", a.clone()).unwrap();
        sessions.start(&s, &unique_id(), "b", b.clone()).unwrap();
        sessions.disconnect(&s, "a");
        assert!(a.is_cancelled());
        assert!(!b.is_cancelled());
    }
}
