//! Bounded length-prefixed JSON protocol. Unix only; TCP is deliberately absent.
use semwright_types::{
    Envelope, Error, ErrorCode, EventEnvelope, ExecuteRequest, MAX_FRAME, PROTOCOL_VERSION, Result,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::UnixStream;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClientMessage {
    Hello {
        version: u32,
        session: Option<String>,
    },
    Execute {
        id: String,
        request: ExecuteRequest,
    },
    Cancel {
        id: String,
    },
    Subscribe {
        after: u64,
    },
    Ping {},
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Welcome { version: u32, session: String },
    Result { envelope: Box<Envelope> },
    Event { sequence: u64, event: EventEnvelope },
    Error { error: Error },
    Pong,
}
pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let body = serde_json::to_vec(value)?;
    if body.is_empty() || body.len() > MAX_FRAME {
        return Err(Error::invalid("Frame size exceeds protocol budget"));
    }
    let mut bytes = Vec::with_capacity(4 + body.len());
    bytes.extend_from_slice(&(body.len() as u32).to_be_bytes());
    bytes.extend_from_slice(&body);
    Ok(bytes)
}
pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    if bytes.len() < 4 {
        return Err(Error::invalid("Truncated frame header"));
    }
    let size = u32::from_be_bytes(
        bytes[..4]
            .try_into()
            .map_err(|_| Error::invalid("Invalid header"))?,
    ) as usize;
    if size == 0 || size > MAX_FRAME || bytes.len() != 4 + size {
        return Err(Error::invalid("Invalid frame length"));
    }
    serde_json::from_slice(&bytes[4..]).map_err(Into::into)
}
pub async fn read_frame<R: AsyncRead + Unpin, T: DeserializeOwned>(r: &mut R) -> Result<T> {
    let size = r.read_u32().await? as usize;
    if size == 0 || size > MAX_FRAME {
        return Err(Error::invalid("Frame length exceeds budget"));
    }
    let mut body = vec![0; size];
    r.read_exact(&mut body).await?;
    serde_json::from_slice(&body).map_err(Into::into)
}
pub async fn write_frame<W: AsyncWrite + Unpin, T: Serialize>(
    w: &mut W,
    message: &T,
) -> Result<()> {
    w.write_all(&encode(message)?).await?;
    w.flush().await?;
    Ok(())
}
pub fn current_uid() -> u32 {
    // SAFETY: getuid has no pointer arguments and no memory/lifetime preconditions.
    unsafe { libc::getuid() }
}
pub fn private_directory(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(m) => {
            if !m.is_dir()
                || m.file_type().is_symlink()
                || m.uid() != current_uid()
                || m.permissions().mode() & 0o777 != 0o700
            {
                return Err(Error::new(
                    ErrorCode::PermissionDenied,
                    "Directory must be owned by the current user with mode 0700",
                ));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            use std::os::unix::fs::DirBuilderExt;
            std::fs::DirBuilder::new().mode(0o700).create(path)?;
        }
        Err(e) => return Err(e.into()),
    }
    Ok(())
}
pub fn runtime_directory() -> Result<PathBuf> {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| {
            Error::unavailable(
                "XDG_RUNTIME_DIR is not set; use a private login-session runtime directory",
            )
        })?;
    private_directory(&base)?;
    let path = base.join("semwright");
    private_directory(&path)?;
    Ok(path)
}
pub fn default_socket() -> Result<PathBuf> {
    Ok(runtime_directory()?.join("broker.sock"))
}
pub fn validate_peer(stream: &UnixStream) -> Result<()> {
    if stream.peer_cred()?.uid() != current_uid() {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Unix peer UID does not match broker owner",
        ));
    }
    Ok(())
}
pub struct Client {
    pub stream: UnixStream,
    pub session: String,
}
impl Client {
    pub async fn connect(path: &Path, session: Option<String>) -> Result<Self> {
        let meta = std::fs::symlink_metadata(path)?;
        use std::os::unix::fs::FileTypeExt;
        if !meta.file_type().is_socket()
            || meta.uid() != current_uid()
            || meta.permissions().mode() & 0o777 != 0o600
        {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "Broker socket must be user-owned with mode 0600",
            ));
        }
        let mut stream = UnixStream::connect(path).await?;
        validate_peer(&stream)?;
        write_frame(
            &mut stream,
            &ClientMessage::Hello {
                version: PROTOCOL_VERSION,
                session,
            },
        )
        .await?;
        let reply: ServerMessage = read_frame(&mut stream).await?;
        match reply {
            ServerMessage::Welcome { version, session } if version == PROTOCOL_VERSION => {
                Ok(Self { stream, session })
            }
            ServerMessage::Error { error } => Err(error),
            _ => Err(Error::new(
                ErrorCode::ProtocolMismatch,
                "Broker handshake failed",
            )),
        }
    }
    pub async fn execute(&mut self, id: String, request: ExecuteRequest) -> Result<Envelope> {
        write_frame(
            &mut self.stream,
            &ClientMessage::Execute {
                id: id.clone(),
                request,
            },
        )
        .await?;
        loop {
            match read_frame::<_, ServerMessage>(&mut self.stream).await? {
                ServerMessage::Result { envelope } if envelope.request_id == id => {
                    return Ok(*envelope);
                }
                ServerMessage::Error { error } => return Err(error),
                ServerMessage::Event { .. } => (),
                _ => {
                    return Err(Error::new(
                        ErrorCode::ProtocolMismatch,
                        "Unexpected broker response",
                    ));
                }
            }
        }
    }
    pub async fn cancel(&mut self, id: String) -> Result<()> {
        write_frame(&mut self.stream, &ClientMessage::Cancel { id }).await
    }
}
/// CLI persistence is a bearer session ticket, not an authorization upgrade.
pub fn load_ticket(path: &Path) -> Result<Option<String>> {
    use std::os::unix::fs::OpenOptionsExt;
    match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
    {
        Ok(mut f) => {
            use std::io::Read;
            let m = f.metadata()?;
            if !m.is_file()
                || m.uid() != current_uid()
                || m.mode() & 0o777 != 0o600
                || m.nlink() != 1
                || m.len() > 128
            {
                return Err(Error::new(
                    ErrorCode::PermissionDenied,
                    "Unsafe session ticket",
                ));
            }
            let mut text = String::new();
            f.read_to_string(&mut text)?;
            uuid::Uuid::parse_str(text.trim())
                .map_err(|_| Error::invalid("Invalid session ticket"))?;
            Ok(Some(text.trim().to_owned()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
pub fn save_ticket(path: &Path, ticket: &str) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    uuid::Uuid::parse_str(ticket).map_err(|_| Error::invalid("Invalid session ticket"))?;
    let temp = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4()));
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)?;
    f.write_all(ticket.as_bytes())?;
    f.sync_all()?;
    std::fs::rename(&temp, path)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    #[test]
    fn ping_rejects_unknown_fields_without_changing_wire_encoding() {
        let valid = serde_json::json!({"type":"ping"});
        assert_eq!(serde_json::to_value(ClientMessage::Ping {}).unwrap(), valid);
        assert!(serde_json::from_value::<ClientMessage>(valid).is_ok());
        assert!(
            serde_json::from_value::<ClientMessage>(
                serde_json::json!({"type":"ping", "execute":"unexpected"})
            )
            .is_err()
        );
    }
    #[test]
    fn oversized_rejected_before_allocation() {
        assert!(decode::<serde_json::Value>(&u32::MAX.to_be_bytes()).is_err());
    }
    #[test]
    fn versioned_roundtrip() {
        let msg = ClientMessage::Hello {
            version: 1,
            session: None,
        };
        assert!(matches!(
            decode::<ClientMessage>(&encode(&msg).unwrap()).unwrap(),
            ClientMessage::Hello { version: 1, .. }
        ));
    }
    #[tokio::test]
    async fn framing_handles_short_reads() {
        let (mut a, mut b) = tokio::io::duplex(3);
        let handle = tokio::spawn(async move {
            write_frame(&mut a, &ClientMessage::Ping {}).await.unwrap();
        });
        assert!(matches!(
            read_frame::<_, ClientMessage>(&mut b).await.unwrap(),
            ClientMessage::Ping {}
        ));
        handle.await.unwrap();
    }
    proptest! {
        #[test] fn roundtrip_string(s in ".{0,2048}") {
            let v = serde_json::json!({"s":s}); let decoded:serde_json::Value = decode(&encode(&v).unwrap()).unwrap(); prop_assert_eq!(v,decoded);
        }
        #[test] fn arbitrary_frames_do_not_panic(bytes in proptest::collection::vec(any::<u8>(),0..5000)) {
            let _ = decode::<ClientMessage>(&bytes);
        }
    }
}

/// Serialize ticket creation only, not commands. A stale ticket starts a new session;
/// old refs still fail because they are never translated between sessions.
pub async fn connect_persistent(socket: &Path, ticket: &Path) -> Result<Client> {
    use std::os::{fd::AsRawFd, unix::fs::OpenOptionsExt};
    let parent = ticket
        .parent()
        .ok_or_else(|| Error::invalid("Session ticket needs a parent directory"))?;
    if !ticket.is_absolute() {
        return Err(Error::invalid("Session ticket must be an absolute path"));
    }
    private_directory(parent)?;
    let lock_path = ticket.with_extension("lock");
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(lock_path)?;
    let m = lock.metadata()?;
    if !m.is_file() || m.uid() != current_uid() || m.nlink() != 1 || m.mode() & 0o777 != 0o600 {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Unsafe session lock file",
        ));
    }
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        // SAFETY: flock only operates on this live owned descriptor. Closing releases it.
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            break;
        }
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::WouldBlock {
            return Err(error.into());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(Error::new(ErrorCode::Timeout, "Session ticket is busy"));
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let session = load_ticket(ticket)?;
    let client = match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        Client::connect(socket, session),
    )
    .await
    {
        Ok(Ok(client)) => client,
        Ok(Err(e)) if e.code == ErrorCode::StaleReference => tokio::time::timeout(
            std::time::Duration::from_secs(5),
            Client::connect(socket, None),
        )
        .await
        .map_err(|_| Error::new(ErrorCode::Timeout, "Broker handshake timed out"))??,
        Ok(Err(e)) => return Err(e),
        Err(_) => return Err(Error::new(ErrorCode::Timeout, "Broker handshake timed out")),
    };
    save_ticket(ticket, &client.session)?;
    drop(lock);
    Ok(client)
}

#[cfg(test)]
mod golden_contract {
    use super::*;
    #[test]
    fn hello_frame_matches_checked_in_golden() {
        let wire = encode(&ClientMessage::Hello {
            version: 1,
            session: None,
        })
        .unwrap();
        let actual: String = wire.iter().map(|byte| format!("{byte:02x}")).collect();
        assert_eq!(
            actual,
            include_str!("../../../tests/golden/hello-v1.frame.hex").trim()
        );
    }
}
