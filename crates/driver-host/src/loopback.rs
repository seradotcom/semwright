use semwright_types::{Error, ErrorCode, Result, unique_id};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    net::{TcpListener, TcpStream, UnixStream},
    sync::Semaphore,
};
use tokio_util::sync::CancellationToken;

pub(crate) const MOUNT_NAME: &str = "semwright-internal-loopback";
pub(crate) const SANDBOX_SOCKET: &str = "/workspace/semwright-internal-loopback/bridge.sock";

pub(crate) struct LoopbackProxy {
    stop: CancellationToken,
    directory: PathBuf,
}

impl LoopbackProxy {
    pub(crate) fn directory(&self) -> &Path {
        &self.directory
    }
}

impl Drop for LoopbackProxy {
    fn drop(&mut self) {
        self.stop.cancel();
        let _ = std::fs::remove_file(self.directory.join("bridge.sock"));
        let _ = std::fs::remove_dir(&self.directory);
    }
}

async fn connect_private_socket(
    socket_path: &Path,
    stop: &CancellationToken,
) -> Result<UnixStream> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if stop.is_cancelled() {
            return Err(Error::new(ErrorCode::Cancelled, "Loopback proxy stopped"));
        }
        match UnixStream::connect(socket_path).await {
            Ok(stream) => {
                semwright_protocol::validate_peer(&stream)?;
                return Ok(stream);
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound
                        | std::io::ErrorKind::ConnectionRefused
                        | std::io::ErrorKind::WouldBlock
                ) && tokio::time::Instant::now() < deadline =>
            {
                tokio::select! {
                    _ = stop.cancelled() => {
                        return Err(Error::new(ErrorCode::Cancelled, "Loopback proxy stopped"));
                    }
                    _ = tokio::time::sleep(Duration::from_millis(20)) => {}
                }
            }
            Err(error) => return Err(error.into()),
        }
    }
}

async fn proxy_connection(
    mut tcp: TcpStream,
    socket_path: PathBuf,
    stop: CancellationToken,
) -> Result<()> {
    let mut unix = connect_private_socket(&socket_path, &stop).await?;
    tokio::select! {
        result = tokio::io::copy_bidirectional(&mut tcp, &mut unix) => {
            result?;
        }
        _ = stop.cancelled() => {}
    }
    Ok(())
}

pub(crate) async fn start(state: &Path, port: u16) -> Result<Arc<LoopbackProxy>> {
    let listener = TcpListener::bind(("127.0.0.1", port)).await.map_err(|_| {
        Error::new(
            ErrorCode::Unavailable,
            "Driver loopback port is unavailable",
        )
    })?;
    let directory = state.join(format!("loopback-{}", unique_id()));
    semwright_protocol::private_directory(&directory)?;
    let socket_path = directory.join("bridge.sock");

    let stop = CancellationToken::new();
    let task_stop = stop.clone();
    let semaphore = Arc::new(Semaphore::new(16));

    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = task_stop.cancelled() => break,
                accepted = listener.accept() => {
                    let Ok((tcp, address)) = accepted else { continue };
                    if !address.ip().is_loopback() {
                        continue;
                    }
                    let Ok(permit) = semaphore.clone().try_acquire_owned() else {
                        continue;
                    };
                    let socket_path = socket_path.clone();
                    let connection_stop = task_stop.clone();
                    tokio::spawn(async move {
                        let _permit = permit;
                        let _ = proxy_connection(tcp, socket_path, connection_stop).await;
                    });
                }
            }
        }
    });

    Ok(Arc::new(LoopbackProxy { stop, directory }))
}
