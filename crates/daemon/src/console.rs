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
    // SAFETY: file owns the open /dev/tty descriptor and is moved into
    // registration. Once registered, the File is never extracted or replaced.
    // get_ref().as_raw_fd() only borrows its descriptor for read, write, and
    // tcflush; none closes it. The File keeps the descriptor open and unchanged
    // throughout the AsyncFd's lifetime.
    unsafe { AsyncFd::register(file) }
        .map_err(io::Error::from)
        .map_err(Into::into)
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
async fn wait_for_answer(fd: &AsyncFd<File>, cancellation: &CancellationToken) -> Result<String> {
    tokio::select! {
        _ = cancellation.cancelled() => Err(Error::new(
            ErrorCode::Cancelled,
            "Approval request cancelled before dispatch",
        )),
        value = tokio::time::timeout(std::time::Duration::from_secs(120), line(fd)) => {
            value.unwrap_or_else(|_| Err(Error::new(
                ErrorCode::ConsentRequired,
                "Approval expired before dispatch",
            )))
        },
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
        let answer = wait_for_answer(&fd, &cancellation).await;
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

#[cfg(all(test, unix))]
mod console_ownership_tests {
    use super::*;
    use std::{
        future::{Future, poll_fn},
        net::Shutdown,
        os::{fd::OwnedFd, unix::net::UnixStream},
        task::Poll,
        time::Duration,
    };

    const TEST_BOUND: Duration = Duration::from_secs(1);

    fn register_socket(stream: UnixStream) -> AsyncFd<File> {
        stream
            .set_nonblocking(true)
            .expect("test socket must become nonblocking");
        let owned: OwnedFd = stream.into();
        let file = File::from(owned);
        // SAFETY: UnixStream -> OwnedFd -> File transfers sole ownership of
        // the same open descriptor through safe conversions. Registration
        // takes this File; these tests only borrow it for I/O and close it by
        // dropping the AsyncFd. They never extract, replace, or independently
        // close its fd.
        unsafe { AsyncFd::register(file) }.expect("test socket must register with Tokio")
    }

    fn socket_pair() -> (AsyncFd<File>, UnixStream) {
        let (owner, peer) = UnixStream::pair().expect("test socket pair must open");
        (register_socket(owner), peer)
    }
    async fn bounded_line(fd: &AsyncFd<File>) -> Result<String> {
        tokio::time::timeout(TEST_BOUND, line(fd))
            .await
            .expect("test line read must finish before its deadline")
    }
    async fn bounded_write(fd: &AsyncFd<File>, bytes: &[u8]) {
        tokio::time::timeout(TEST_BOUND, write(fd, bytes))
            .await
            .expect("test write must finish before its deadline")
            .expect("test write must succeed");
    }

    #[tokio::test]
    async fn input_eof_denies_without_closing_the_owned_descriptor() {
        let (owner, peer) = socket_pair();
        peer.shutdown(Shutdown::Write)
            .expect("test peer must half-close its output");
        let peer = register_socket(peer);
        let error = bounded_line(&owner)
            .await
            .expect_err("EOF must deny an approval input");
        assert!(matches!(error.code, ErrorCode::ConsentRequired));
        // EOF is an input result, not a transfer or close of the File owner.
        bounded_write(&owner, b"owner-still-open\n").await;
        let observed = bounded_line(&peer)
            .await
            .expect("peer must still receive output from the retained owner");
        assert_eq!(observed, "owner-still-open\n");
    }

    #[tokio::test]
    async fn cancellation_drops_pending_answer_without_closing_or_retaining_a_reader() {
        let (owner, peer) = socket_pair();
        let peer = register_socket(peer);
        let cancellation = CancellationToken::new();
        let error = {
            let answer = wait_for_answer(&owner, &cancellation);
            tokio::pin!(answer);
            poll_fn(|cx| match answer.as_mut().poll(cx) {
                Poll::Pending => Poll::Ready(()),
                Poll::Ready(_) => panic!("empty live input must leave the answer pending"),
            })
            .await;
            cancellation.cancel();
            tokio::time::timeout(TEST_BOUND, answer.as_mut())
                .await
                .expect("cancelled answer must finish before its test deadline")
                .expect_err("cancellation must deny the answer")
        };
        assert!(matches!(error.code, ErrorCode::Cancelled));
        // Only write after the cancelled wait is gone. A new real line() must
        // receive all subsequent input through the same retained AsyncFd.
        bounded_write(&peer, b"input-after-cancellation\n").await;
        let observed = bounded_line(&owner)
            .await
            .expect("retained owner must remain readable after cancellation");
        assert_eq!(observed, "input-after-cancellation\n");
    }

    #[tokio::test]
    async fn dropping_the_async_fd_owner_exposes_eof_to_the_peer() {
        let (owner, peer) = socket_pair();
        let peer = register_socket(peer);
        let peer_line = line(&peer);
        tokio::pin!(peer_line);
        poll_fn(|cx| match peer_line.as_mut().poll(cx) {
            Poll::Pending => Poll::Ready(()),
            Poll::Ready(_) => panic!("live owner must leave the empty peer input pending"),
        })
        .await;
        drop(owner);
        let error = tokio::time::timeout(TEST_BOUND, peer_line.as_mut())
            .await
            .expect("owner drop must expose EOF before the test deadline")
            .expect_err("peer EOF must deny the line read");
        assert!(matches!(error.code, ErrorCode::ConsentRequired));
    }
}
