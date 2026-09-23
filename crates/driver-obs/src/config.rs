use crate::{Fault, FaultKind, Result, auth::Secret, bounds};
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    net::{IpAddr, SocketAddr},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
    time::Duration,
};
use tokio::io::AsyncReadExt;

pub const CONFIG_PATH: &str = "/etc/semwright-obs/config.json";
pub const SECRET_SOCKET: &str = "/workspace/obs-credential/secret.sock";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    pub address: IpAddr,
    pub port: u16,
    pub connect_timeout_ms: u64,
    pub request_timeout_ms: u64,
    pub reconnect_limit: usize,
    pub event_capacity: usize,
    pub secret_socket: bool,
    pub allow_stream_start: bool,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            address: IpAddr::from([127, 0, 0, 1]),
            port: 4455,
            connect_timeout_ms: 2000,
            request_timeout_ms: 3000,
            reconnect_limit: 5,
            event_capacity: 256,
            secret_socket: false,
            allow_stream_start: false,
        }
    }
}
impl Config {
    pub fn validate(&self) -> Result<()> {
        if !self.address.is_loopback()
            || self.port == 0
            || !(100..=5000).contains(&self.connect_timeout_ms)
            || !(100..=5000).contains(&self.request_timeout_ms)
            || !(1..=5).contains(&self.reconnect_limit)
            || !(8..=512).contains(&self.event_capacity)
        {
            return Err(Fault::new(FaultKind::Configuration));
        }
        Ok(())
    }
    pub fn endpoint(&self) -> SocketAddr {
        SocketAddr::new(self.address, self.port)
    }
    pub fn timeout(&self) -> Duration {
        Duration::from_millis(self.request_timeout_ms)
    }
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let value = bounds::parse(bytes, 8192)?;
        let config: Self =
            serde_json::from_value(value).map_err(|_| Fault::new(FaultKind::Configuration))?;
        config.validate()?;
        Ok(config)
    }
    pub fn load() -> Result<Self> {
        match std::fs::symlink_metadata(CONFIG_PATH) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(_) => Err(Fault::new(FaultKind::Configuration)),
            Ok(_) => Self::read_protected(Path::new(CONFIG_PATH)),
        }
    }
    pub fn read_protected(path: &Path) -> Result<Self> {
        let parent = path
            .parent()
            .ok_or_else(|| Fault::new(FaultKind::Configuration))?;
        if std::fs::canonicalize(parent).map_err(|_| Fault::new(FaultKind::Configuration))?
            != parent
        {
            return Err(Fault::new(FaultKind::Configuration));
        }
        let metadata = std::fs::metadata(parent)?;
        // SAFETY: getuid takes no pointers and has no preconditions.
        let uid = unsafe { libc::getuid() };
        if !metadata.is_dir() || ![0, uid].contains(&metadata.uid()) || metadata.mode() & 0o022 != 0
        {
            return Err(Fault::new(FaultKind::Configuration));
        }
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
            .open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || metadata.nlink() != 1
            || ![0, uid].contains(&metadata.uid())
            || metadata.mode() & 0o022 != 0
            || metadata.len() > 8192
        {
            return Err(Fault::new(FaultKind::Configuration));
        }
        let mut bytes = Vec::new();
        file.take(8193).read_to_end(&mut bytes)?;
        Self::parse(&bytes)
    }
}

pub async fn socket_secret() -> Result<Secret> {
    let action = async {
        let path = Path::new(SECRET_SOCKET);
        let parent = path
            .parent()
            .ok_or_else(|| Fault::new(FaultKind::Authentication))?;
        let pm = std::fs::symlink_metadata(parent)?;
        // SAFETY: getuid takes no pointers and has no preconditions.
        let uid = unsafe { libc::getuid() };
        if !pm.is_dir() || pm.uid() != uid || pm.mode() & 0o777 != 0o700 {
            return Err(Fault::new(FaultKind::Authentication));
        }
        use std::os::unix::fs::FileTypeExt;
        let sm = std::fs::symlink_metadata(path)?;
        if !sm.file_type().is_socket() || sm.uid() != uid || sm.mode() & 0o777 != 0o600 {
            return Err(Fault::new(FaultKind::Authentication));
        }
        let mut socket = tokio::net::UnixStream::connect(path).await?;
        if socket.peer_cred()?.uid() != uid {
            return Err(Fault::new(FaultKind::Authentication));
        }
        let size = socket.read_u32().await? as usize;
        if size == 0 || size > 1024 {
            return Err(Fault::new(FaultKind::Authentication));
        }
        let mut bytes = zeroize::Zeroizing::new(vec![0u8; size]);
        socket.read_exact(&mut bytes).await?;
        Secret::new(std::mem::take(&mut *bytes))
    };
    tokio::time::timeout(Duration::from_secs(2), action)
        .await
        .map_err(|_| Fault::new(FaultKind::Timeout))?
}
