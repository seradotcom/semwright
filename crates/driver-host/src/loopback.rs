use semwright_types::{Error, ErrorCode, Result, unique_id};
#[cfg(windows)]
use std::sync::Mutex as StdMutex;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
#[cfg(unix)]
use tokio::net::UnixStream;
#[cfg(windows)]
use tokio::net::windows::named_pipe::NamedPipeServer;
use tokio::{
    net::{TcpListener, TcpStream},
    sync::Semaphore,
};
use tokio_util::sync::CancellationToken;

#[cfg(unix)]
pub(crate) const MOUNT_NAME: &str = "semwright-internal-loopback";
#[cfg(unix)]
pub(crate) const SANDBOX_SOCKET: &str = "/workspace/semwright-internal-loopback/bridge.sock";
#[cfg(windows)]
pub(crate) const SANDBOX_PIPE_ENV: &str = "SEMWRIGHT_DRIVER_LOOPBACK_PIPE";

pub(crate) struct LoopbackProxy {
    stop: CancellationToken,
    #[cfg(unix)]
    directory: PathBuf,
    #[cfg(windows)]
    pipe_path: PathBuf,
    #[cfg(windows)]
    reserved_pipe: StdMutex<Option<NamedPipeServer>>,
    #[cfg(windows)]
    listener: StdMutex<Option<TcpListener>>,
}

#[cfg(windows)]
async fn connect_expected_pipe(
    server: &mut NamedPipeServer,
    expected_pid: u32,
    stop: &CancellationToken,
) -> Result<()> {
    let connected = tokio::select! {
        _ = stop.cancelled() => {
            return Err(Error::new(ErrorCode::Cancelled, "Loopback proxy stopped"));
        }
        result = tokio::time::timeout(Duration::from_secs(5), server.connect()) => result,
    };
    match connected {
        Ok(Ok(())) => {}
        Ok(Err(_)) => {
            return Err(Error::new(
                ErrorCode::Unavailable,
                "Driver loopback pipe connection failed",
            ));
        }
        Err(_) => {
            return Err(Error::new(
                ErrorCode::Timeout,
                "Driver loopback pipe connection timed out",
            ));
        }
    }
    semwright_platform_services::validate_windows_appcontainer_loopback_peer(server, expected_pid)
}

impl LoopbackProxy {
    pub(crate) fn shutdown(&self) {
        self.stop.cancel();
    }

    #[cfg(unix)]
    pub(crate) fn directory(&self) -> &Path {
        &self.directory
    }

    #[cfg(windows)]
    pub(crate) fn pipe_path(&self) -> &Path {
        &self.pipe_path
    }

    #[cfg(windows)]
    pub(crate) async fn activate(self: &Arc<Self>, expected_pid: u32) -> Result<()> {
        let listener = self
            .listener
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "Loopback listener lock poisoned"))?
            .take()
            .ok_or_else(|| Error::new(ErrorCode::Conflict, "Loopback proxy already activated"))?;
        let pipe_path = self.pipe_path.clone();
        let mut server = self
            .reserved_pipe
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "Reserved loopback pipe lock poisoned"))?
            .take()
            .ok_or_else(|| Error::new(ErrorCode::Conflict, "Loopback pipe already activated"))?;
        // The first Host-owned pipe instance is created before the LPAC child starts with an
        // owner/SYSTEM-only DACL. Once the kernel PID is known, grant only that child package
        // SID on the existing instance. The unqualified Win32 pipe namespace is intentional:
        // this LPAC is launched manually rather than from an MSIX package namespace.
        // Authorization still fails closed before any child traffic can connect.
        semwright_platform_services::windows_authorize_appcontainer_loopback_server(
            &server,
            expected_pid,
        )?;
        connect_expected_pipe(&mut server, expected_pid, &self.stop).await?;

        let stop = self.stop.clone();
        let semaphore = Arc::new(Semaphore::new(16));
        tokio::spawn(async move {
            loop {
                let accepted = tokio::select! {
                    _ = stop.cancelled() => break,
                    accepted = listener.accept() => accepted,
                };
                let Ok((tcp, address)) = accepted else {
                    continue;
                };
                if !address.ip().is_loopback() {
                    continue;
                }
                let Ok(permit) = semaphore.clone().try_acquire_owned() else {
                    continue;
                };

                let connected_pipe = server;
                let connection_stop = stop.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    let _ = proxy_pipe_connection(tcp, connected_pipe, connection_stop).await;
                });

                let mut next =
                    match semwright_platform_services::windows_appcontainer_loopback_server(
                        &pipe_path,
                        expected_pid,
                        false,
                    ) {
                        Ok(next) => next,
                        Err(_) => break,
                    };
                if connect_expected_pipe(&mut next, expected_pid, &stop)
                    .await
                    .is_err()
                {
                    break;
                }
                server = next;
            }
        });
        Ok(())
    }
}

impl Drop for LoopbackProxy {
    fn drop(&mut self) {
        self.stop.cancel();
        #[cfg(unix)]
        {
            let _ = std::fs::remove_file(self.directory.join("bridge.sock"));
            let _ = std::fs::remove_dir(&self.directory);
        }
    }
}

#[cfg(unix)]
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

#[cfg(unix)]
async fn proxy_unix_connection(
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

#[cfg(windows)]
async fn proxy_pipe_connection(
    mut tcp: TcpStream,
    mut pipe: NamedPipeServer,
    stop: CancellationToken,
) -> Result<()> {
    tokio::select! {
        result = tokio::io::copy_bidirectional(&mut tcp, &mut pipe) => {
            result?;
        }
        _ = stop.cancelled() => {}
    }
    Ok(())
}

#[cfg(unix)]
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
                    let Ok((tcp, address)) = accepted else { continue; };
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
                        let _ = proxy_unix_connection(tcp, socket_path, connection_stop).await;
                    });
                }
            }
        }
    });

    Ok(Arc::new(LoopbackProxy { stop, directory }))
}

#[cfg(windows)]
pub(crate) async fn start(_state: &Path, port: u16) -> Result<Arc<LoopbackProxy>> {
    let listener = TcpListener::bind(("127.0.0.1", port)).await.map_err(|_| {
        Error::new(
            ErrorCode::Unavailable,
            "Driver loopback port is unavailable",
        )
    })?;
    let pipe_path = PathBuf::from(format!(r"\\.\pipe\semwright-loopback-{}", unique_id()));
    // Reserve the first Host-owned instance before the sandbox process is created. The
    // initial DACL intentionally excludes AppContainer identities; activate() authorizes exactly
    // the kernel-observed child SID after spawn.
    let reserved_pipe =
        semwright_platform_services::windows_reserve_appcontainer_loopback_server(&pipe_path)?;
    Ok(Arc::new(LoopbackProxy {
        stop: CancellationToken::new(),
        pipe_path,
        reserved_pipe: StdMutex::new(Some(reserved_pipe)),
        listener: StdMutex::new(Some(listener)),
    }))
}
