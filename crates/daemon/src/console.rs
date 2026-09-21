//! Trusted human confirmation lives on the broker's own terminal, never on IPC/MCP.
use async_trait::async_trait;
use semwright_core::{Approval, Approver};
use semwright_types::*;
use std::{
    fs::{File, OpenOptions},
    io,
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
};
use tokio::io::unix::AsyncFd;
use tokio_util::sync::CancellationToken;
pub struct Console;
fn terminal() -> Result<AsyncFd<File>> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOCTTY | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open("/dev/tty")
        .map_err(|_| {
            Error::new(
                ErrorCode::ConsentRequired,
                "Foreground approval requires the broker's controlling terminal",
            )
        })?;
    // SAFETY: isatty only inspects this live descriptor.
    if unsafe { libc::isatty(file.as_raw_fd()) } != 1 {
        return Err(Error::new(
            ErrorCode::ConsentRequired,
            "Approval descriptor is not a terminal",
        ));
    }
    AsyncFd::new(file).map_err(Into::into)
}
async fn write(fd: &AsyncFd<File>, mut bytes: &[u8]) -> Result<()> {
    while !bytes.is_empty() {
        let mut guard = fd.writable().await?;
        match guard.try_io(|inner| {
            // SAFETY: byte slice is valid for its length and descriptor remains owned by AsyncFd.
            let n = unsafe {
                libc::write(
                    inner.get_ref().as_raw_fd(),
                    bytes.as_ptr().cast(),
                    bytes.len(),
                )
            };
            if n < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(n as usize)
            }
        }) {
            Ok(Ok(0)) => {
                return Err(Error::new(
                    ErrorCode::Unavailable,
                    "Approval terminal closed",
                ));
            }
            Ok(Ok(n)) => bytes = &bytes[n..],
            Ok(Err(e)) => return Err(e.into()),
            Err(_) => continue,
        }
    }
    Ok(())
}
async fn line(fd: &AsyncFd<File>) -> Result<String> {
    let mut bytes = vec![];
    loop {
        let mut buf = [0u8; 128];
        let mut guard = fd.readable().await?;
        match guard.try_io(|inner| {
            // SAFETY: buf is writable for its entire length and fd is live.
            let n = unsafe {
                libc::read(
                    inner.get_ref().as_raw_fd(),
                    buf.as_mut_ptr().cast(),
                    buf.len(),
                )
            };
            if n < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(n as usize)
            }
        }) {
            Ok(Ok(0)) => {
                return Err(Error::new(
                    ErrorCode::ConsentRequired,
                    "Approval terminal input ended",
                ));
            }
            Ok(Ok(n)) => {
                bytes.extend_from_slice(&buf[..n]);
                if bytes.len() > 256 {
                    return Err(Error::new(
                        ErrorCode::PolicyDenied,
                        "Approval response exceeds the limit",
                    ));
                }
                if bytes.contains(&b'\n') {
                    return String::from_utf8(bytes)
                        .map_err(|_| Error::invalid("Approval must be ASCII"));
                }
            }
            Ok(Err(e)) => return Err(e.into()),
            Err(_) => continue,
        }
    }
}
#[async_trait]
impl Approver for Console {
    async fn approve(&self, approval: Approval, cancellation: CancellationToken) -> Result<bool> {
        let fd = terminal()?;
        let challenge = unique_id()[..12].to_owned();
        // Escape every non-ASCII/control character: application text cannot alter terminal state.
        let summary: String = approval
            .summary
            .chars()
            .flat_map(char::escape_default)
            .take(8192)
            .collect();
        let message = format!(
            "\nSEMWRIGHT HUMAN APPROVAL\nCommand: {}\nBackend: {}\nRisk: {:?}\n{}\nBroker desktop I/O is locked while this question is pending.\nType exactly: approve {}\n> ",
            approval.command, approval.backend, approval.risk, summary, challenge
        );
        write(&fd, message.as_bytes()).await?;
        let answer = tokio::select! {
            _=cancellation.cancelled()=>Err(Error::new(ErrorCode::Cancelled,"Approval request cancelled before dispatch")),
            value=tokio::time::timeout(std::time::Duration::from_secs(120),line(&fd))=>value.unwrap_or_else(|_|Err(Error::new(ErrorCode::ConsentRequired,"Approval expired before dispatch"))),
        };
        // SAFETY: flushing unread input on this controlling terminal prevents an expired response
        // from being interpreted as approval for a later, different operation.
        let _ = unsafe { libc::tcflush(fd.get_ref().as_raw_fd(), libc::TCIFLUSH) };
        let granted = answer?.trim() == format!("approve {challenge}");
        write(
            &fd,
            if granted {
                b"Approved once.\n"
            } else {
                b"Declined.\n"
            },
        )
        .await?;
        Ok(granted)
    }
}
