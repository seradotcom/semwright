//! Bounded length-prefixed JSON protocol over peer-authenticated local IPC. TCP is deliberately absent.
use semwright_types::{
    Envelope, Error, ErrorCode, EventEnvelope, ExecuteRequest, MAX_FRAME, PROTOCOL_VERSION, Result,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
#[cfg(unix)]
use tokio::net::UnixStream;
#[cfg(target_os = "windows")]
use tokio::net::windows::named_pipe::NamedPipeClient;

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
#[cfg(unix)]
pub fn current_uid() -> u32 {
    semwright_platform_services::current_uid()
}
pub fn private_directory(path: &Path) -> Result<()> {
    semwright_platform_services::private_directory(path)
}
pub fn runtime_directory() -> Result<PathBuf> {
    semwright_platform_services::runtime_directory()
}
pub fn default_endpoint(kind: &str) -> Result<PathBuf> {
    if kind.is_empty()
        || kind.len() > 32
        || !kind.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(Error::invalid("Invalid local endpoint kind"));
    }
    #[cfg(unix)]
    {
        return Ok(runtime_directory()?.join(format!("{kind}.sock")));
    }
    #[cfg(target_os = "windows")]
    {
        return semwright_platform_services::windows_pipe_path(kind);
    }
    #[allow(unreachable_code)]
    Err(Error::new(
        ErrorCode::Unsupported,
        "Local IPC transport is unavailable on this platform",
    ))
}
pub fn default_socket() -> Result<PathBuf> {
    default_endpoint("broker")
}

/// Default CLI/TUI session persistence must use a real filesystem path.
/// Windows endpoints are named pipes, not directories that can hold tickets.
pub fn default_session_ticket(socket: &Path) -> Result<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        // A private ticket per broker endpoint avoids cross-broker stale
        // handshakes when several isolated applications are running.
        let name = socket
            .file_name()
            .and_then(|component| component.to_str())
            .filter(|value| {
                value.len() <= 96
                    && value
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
            })
            .unwrap_or("broker");
        Ok(runtime_directory()?.join(format!("cli-{name}.session")))
    }
    #[cfg(not(target_os = "windows"))]
    {
        Ok(socket.with_file_name("cli.session"))
    }
}

#[cfg(unix)]
type LocalClientStream = UnixStream;
#[cfg(target_os = "windows")]
type LocalClientStream = NamedPipeClient;

#[cfg(unix)]
pub fn validate_peer(stream: &UnixStream) -> Result<()> {
    semwright_platform_services::validate_peer(stream)
}
#[cfg(unix)]
fn validate_client_peer(stream: &UnixStream) -> Result<()> {
    validate_peer(stream)
}
#[cfg(target_os = "windows")]
fn validate_client_peer(stream: &NamedPipeClient) -> Result<()> {
    semwright_platform_services::validate_windows_client_peer(stream).map(|_| ())
}

pub struct Client {
    pub stream: LocalClientStream,
    pub session: String,
}
impl Client {
    async fn open_local(path: &Path) -> Result<LocalClientStream> {
        #[cfg(unix)]
        {
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
            return UnixStream::connect(path).await.map_err(Into::into);
        }
        #[cfg(target_os = "windows")]
        {
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                match semwright_platform_services::windows_pipe_client(path) {
                    Ok(stream) => return Ok(stream),
                    Err(error) if tokio::time::Instant::now() < deadline => {
                        let _ = error;
                        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        #[allow(unreachable_code)]
        Err(Error::new(
            ErrorCode::Unsupported,
            "Local IPC transport is unavailable on this platform",
        ))
    }

    pub async fn connect(path: &Path, session: Option<String>) -> Result<Self> {
        let mut stream = Self::open_local(path).await?;
        validate_client_peer(&stream)?;
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
#[cfg(unix)]
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
#[cfg(unix)]
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
#[cfg(unix)]
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

// Windows ticket and lock files must be protected *before* reading/writing
// bearer tokens. Opening the reparse point itself avoids following a hostile
// symlink; the owner-only directory and file ACLs are both enforced.
#[cfg(target_os = "windows")]
fn windows_ticket_file(path: &Path, create: bool) -> Result<Option<std::fs::File>> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    let mut options = std::fs::OpenOptions::new();
    options
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    if create {
        options.write(true).create(true);
    }
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if !create && error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(None);
        }
        Err(error) => return Err(error.into()),
    };
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || metadata.len() > 128
    {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Unsafe Windows session ticket or lock file",
        ));
    }
    semwright_platform_services::protect_owner_only_file(path)?;
    Ok(Some(file))
}

#[cfg(target_os = "windows")]
pub fn load_ticket(path: &Path) -> Result<Option<String>> {
    use std::io::Read;
    if !path.is_absolute() {
        return Err(Error::invalid("Session ticket must be an absolute path"));
    }
    let parent = path
        .parent()
        .ok_or_else(|| Error::invalid("Session ticket needs a parent directory"))?;
    semwright_platform_services::private_data_directory(parent)?;
    let Some(mut file) = windows_ticket_file(path, false)? else {
        return Ok(None);
    };
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    let ticket = text.trim();
    uuid::Uuid::parse_str(ticket).map_err(|_| Error::invalid("Invalid session ticket"))?;
    Ok(Some(ticket.to_owned()))
}

#[cfg(target_os = "windows")]
pub fn save_ticket(path: &Path, ticket: &str) -> Result<()> {
    use std::io::Write;
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    if !path.is_absolute() {
        return Err(Error::invalid("Session ticket must be an absolute path"));
    }
    uuid::Uuid::parse_str(ticket).map_err(|_| Error::invalid("Invalid session ticket"))?;
    let parent = path
        .parent()
        .ok_or_else(|| Error::invalid("Session ticket needs a parent directory"))?;
    semwright_platform_services::private_data_directory(parent)?;
    let temporary = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&temporary)?;
        // Enforce a protected DACL on the newly-created file before writing
        // the bearer token, even when the parent already has a strict DACL.
        semwright_platform_services::protect_owner_only_file(&temporary)?;
        file.write_all(ticket.as_bytes())?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[cfg(target_os = "windows")]
pub async fn connect_persistent(socket: &Path, ticket: &Path) -> Result<Client> {
    if !ticket.is_absolute() {
        return Err(Error::invalid("Session ticket must be an absolute path"));
    }
    let parent = ticket
        .parent()
        .ok_or_else(|| Error::invalid("Session ticket needs a parent directory"))?;
    semwright_platform_services::private_data_directory(parent)?;
    // File locks are OS-managed and automatically released after a crash.
    // Keep the lock separate from the ticket so atomic rename cannot bypass it.
    let lock_file = windows_ticket_file(&ticket.with_extension("lock"), true)?
        .ok_or_else(|| Error::unavailable("Windows session lock unavailable"))?;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if semwright_platform_services::try_lock_session_file(&lock_file)? {
            break;
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
        Ok(Err(error)) if error.code == ErrorCode::StaleReference => tokio::time::timeout(
            std::time::Duration::from_secs(5),
            Client::connect(socket, None),
        )
        .await
        .map_err(|_| Error::new(ErrorCode::Timeout, "Broker handshake timed out"))??,
        Ok(Err(error)) => return Err(error),
        Err(_) => return Err(Error::new(ErrorCode::Timeout, "Broker handshake timed out")),
    };
    save_ticket(ticket, &client.session)?;
    drop(lock_file);
    Ok(client)
}

#[cfg(all(test, target_os = "windows"))]
mod windows_ticket_tests {
    use super::*;

    #[test]
    fn default_session_file_is_a_distinct_private_path_per_windows_pipe() {
        let first = default_session_ticket(Path::new(r"\\.\pipe\semwright-test-broker")).unwrap();
        let second = default_session_ticket(Path::new(r"\\.\pipe\semwright-test-olevu")).unwrap();
        assert!(first.is_absolute());
        assert_ne!(first, second);
        assert!(first.starts_with(runtime_directory().unwrap()));
    }

    #[test]
    fn owner_only_ticket_roundtrip_replaces_prior_ticket() {
        let base = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let dir = tempfile::Builder::new()
            .prefix("semwright-win-ticket-")
            .tempdir_in(base)
            .unwrap();
        let path = dir.path().join("lab-mcp.session");
        assert_eq!(load_ticket(&path).unwrap(), None);
        let first = uuid::Uuid::new_v4().to_string();
        save_ticket(&path, &first).unwrap();
        assert_eq!(load_ticket(&path).unwrap(), Some(first));
        let replacement = uuid::Uuid::new_v4().to_string();
        save_ticket(&path, &replacement).unwrap();
        assert_eq!(load_ticket(&path).unwrap(), Some(replacement));
        assert!(save_ticket(&path, "not-a-session").is_err());
    }
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
