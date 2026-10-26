//! Private owner opt-in for custody of a validated V7 Host-tool stdin request.
//!
//! This channel is an observation sink, never execution permission. It is not
//! serialized, exposed through the driver wire protocol, or enabled by an
//! environment variable. The owner must keep its receiver and any persisted
//! bytes private. Nothing in this module writes a file or reads process stdin.

use semwright_driver_sdk::{Manifest, RuntimeToolJob, validate_runtime_tool_job_start};
use semwright_types::{Error, ProviderIdentity, Result};
use sha2::{Digest, Sha256};
use std::io;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
#[cfg(any(target_os = "linux", test))]
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot};

// Matches driver-sdk's currently private MAX_TOOL_STDIN_BYTES. A future SDK
// increase must not silently increase custody memory: larger records are lost
// explicitly until this opt-in boundary is reviewed too.
const MAX_CAPTURE_BYTES: usize = 64 * 1024;
const MAX_CAPTURE_QUEUE: usize = 8;

/// Loss counters are evidence that custody is incomplete, not native failures.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HostStdinCustodyLosses {
    pub capture_full: u64,
    pub capture_closed: u64,
    pub capture_oversized: u64,
    pub terminal_receiver_lost: u64,
}

#[derive(Default)]
struct LossCounters {
    capture_full: AtomicU64,
    capture_closed: AtomicU64,
    capture_oversized: AtomicU64,
    terminal_receiver_lost: AtomicU64,
}

fn increment(counter: &AtomicU64) {
    let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
        Some(value.saturating_add(1))
    });
}

impl LossCounters {
    fn snapshot(&self) -> HostStdinCustodyLosses {
        HostStdinCustodyLosses {
            capture_full: self.capture_full.load(Ordering::Relaxed),
            capture_closed: self.capture_closed.load(Ordering::Relaxed),
            capture_oversized: self.capture_oversized.load(Ordering::Relaxed),
            terminal_receiver_lost: self.terminal_receiver_lost.load(Ordering::Relaxed),
        }
    }
}

/// Rust-only capability retained by the trusted owner of one real DriverProvider.
/// Cloning it does not enable capture unless passed to the explicit constructor.
#[derive(Clone)]
pub struct HostStdinCustody {
    tool: Arc<str>,
    sender: mpsc::Sender<HostStdinCapture>,
    losses: Arc<LossCounters>,
}

pub struct HostStdinCustodyReceiver {
    receiver: mpsc::Receiver<HostStdinCapture>,
    losses: Arc<LossCounters>,
}

/// Contains private plaintext by explicit owner opt-in. Deliberately has no
/// Debug, Serialize, Deserialize, Clone, or public field constructor.
pub struct HostStdinCapture {
    provider: ProviderIdentity,
    provider_process_id: u32,
    job_id: String,
    request_id: String,
    parent_request_id: String,
    session_id: String,
    tool: String,
    original_stdin: Vec<u8>,
    sha256: String,
    terminal: oneshot::Receiver<HostStdinWriteTerminal>,
}

/// Only the pipe-write boundary is observed. None of these variants proves that
/// the child consumed, parsed, or executed the supplied bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostStdinWriteTerminal {
    /// The instrumented Linux broker was never observed. In particular, this
    /// must not claim zero writes on another platform.
    NotObserved,
    /// Linux broker entered, but the observed write_all was never entered.
    NotAttempted,
    /// Original input was empty; the original code makes no write_all call.
    EmptyInputNoWrite,
    /// The one original write_all returned Ok. Child consumption is unknown.
    WriteAllCompleted,
    /// The one original write_all returned this I/O classification. A prefix
    /// may have been delivered; no byte count is inferred from the error.
    WriteAllFailed {
        kind: io::ErrorKind,
        raw_os_error: Option<i32>,
    },
    /// write_all was entered, but its future was dropped without a result (for
    /// example cancellation or timeout). Delivery may be partial or complete.
    /// Join with the unchanged native outcome to identify the abort cause.
    WriteAllInterrupted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostStdinTerminalUnavailable;

impl HostStdinCustody {
    /// Queue capacity is 1..=8, and each captured payload is at most 64 KiB.
    /// The selection names an existing Host tool; it grants no tool authority.
    pub fn channel_for_tool(
        tool: impl Into<String>,
        capacity: usize,
    ) -> Result<(Self, HostStdinCustodyReceiver)> {
        let tool = tool.into();
        validate_runtime_tool_job_start(&tool, &[], &[], 1, None)?;
        if !(1..=MAX_CAPTURE_QUEUE).contains(&capacity) {
            return Err(Error::invalid(
                "Host stdin custody queue must contain 1 through 8 records",
            ));
        }
        let (sender, receiver) = mpsc::channel(capacity);
        let losses = Arc::new(LossCounters::default());
        Ok((
            Self {
                tool: tool.into(),
                sender,
                losses: losses.clone(),
            },
            HostStdinCustodyReceiver { receiver, losses },
        ))
    }

    pub fn losses(&self) -> HostStdinCustodyLosses {
        self.losses.snapshot()
    }

    pub(super) fn validate_manifest(&self, manifest: &Manifest) -> Result<()> {
        if manifest.protocol < 7
            || !manifest.interfaces.host_tools
            || !manifest
                .tools
                .iter()
                .any(|tool| tool.name == self.tool.as_ref())
        {
            return Err(Error::invalid(
                "Host stdin custody must select an existing protocol-v7 Host tool",
            ));
        }
        Ok(())
    }

    pub(super) fn bind(
        &self,
        provider: ProviderIdentity,
        provider_process_id: u32,
    ) -> BoundHostStdinCustody {
        BoundHostStdinCustody {
            sink: self.clone(),
            provider,
            provider_process_id,
        }
    }
}

impl HostStdinCustodyReceiver {
    pub async fn recv(&mut self) -> Option<HostStdinCapture> {
        self.receiver.recv().await
    }

    pub fn close(&mut self) {
        self.receiver.close();
    }

    pub fn losses(&self) -> HostStdinCustodyLosses {
        self.losses.snapshot()
    }
}

impl HostStdinCapture {
    pub fn provider(&self) -> &ProviderIdentity {
        &self.provider
    }
    pub fn provider_process_id(&self) -> u32 {
        self.provider_process_id
    }
    pub fn job_id(&self) -> &str {
        &self.job_id
    }
    pub fn request_id(&self) -> &str {
        &self.request_id
    }
    pub fn parent_request_id(&self) -> &str {
        &self.parent_request_id
    }
    pub fn session_id(&self) -> &str {
        &self.session_id
    }
    pub fn tool(&self) -> &str {
        &self.tool
    }
    pub fn original_stdin(&self) -> &[u8] {
        &self.original_stdin
    }
    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    /// Consume the terminal receiver exactly once. Inspect or privately retain
    /// the capture beforehand; this method does not copy or persist its bytes.
    pub async fn terminal(
        self,
    ) -> std::result::Result<HostStdinWriteTerminal, HostStdinTerminalUnavailable> {
        self.terminal
            .await
            .map_err(|_| HostStdinTerminalUnavailable)
    }
}

#[derive(Clone)]
pub(super) struct BoundHostStdinCustody {
    sink: HostStdinCustody,
    provider: ProviderIdentity,
    provider_process_id: u32,
}

impl BoundHostStdinCustody {
    /// Called only after the real V7 parent/session validation and job insertion.
    pub(super) fn capture(
        &self,
        job: &RuntimeToolJob,
        request_id: &str,
        parent_request_id: &str,
        session_id: &str,
        tool: &str,
        original_stdin: &[u8],
    ) -> Option<StdinWriteTicket> {
        if tool != self.sink.tool.as_ref() {
            return None;
        }
        if original_stdin.len() > MAX_CAPTURE_BYTES {
            increment(&self.sink.losses.capture_oversized);
            return None;
        }
        // Reserve before cloning/hash work. Sink pressure cannot block a tool.
        let permit = match self.sink.sender.try_reserve() {
            Ok(permit) => permit,
            Err(mpsc::error::TrySendError::Full(_)) => {
                increment(&self.sink.losses.capture_full);
                return None;
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                increment(&self.sink.losses.capture_closed);
                return None;
            }
        };
        let (sender, terminal) = oneshot::channel();
        permit.send(HostStdinCapture {
            provider: self.provider.clone(),
            provider_process_id: self.provider_process_id,
            job_id: job.id.clone(),
            request_id: request_id.into(),
            parent_request_id: parent_request_id.into(),
            session_id: session_id.into(),
            tool: tool.into(),
            original_stdin: original_stdin.to_vec(),
            sha256: hex::encode(Sha256::digest(original_stdin)),
            terminal,
        });
        Some(StdinWriteTicket {
            sender: Some(sender),
            fallback: HostStdinWriteTerminal::NotObserved,
            losses: self.sink.losses.clone(),
        })
    }
}

pub(super) struct StdinWriteTicket {
    sender: Option<oneshot::Sender<HostStdinWriteTerminal>>,
    fallback: HostStdinWriteTerminal,
    losses: Arc<LossCounters>,
}

impl StdinWriteTicket {
    #[cfg(any(target_os = "linux", test))]
    pub(super) fn linux_broker_entered(&mut self) {
        self.fallback = HostStdinWriteTerminal::NotAttempted;
    }

    fn finish(&mut self, terminal: HostStdinWriteTerminal) {
        if let Some(sender) = self.sender.take()
            && sender.send(terminal).is_err()
        {
            increment(&self.losses.terminal_receiver_lost);
        }
    }
}

impl Drop for StdinWriteTicket {
    fn drop(&mut self) {
        self.finish(self.fallback);
    }
}

/// The real Linux broker calls this at its existing single write_all boundary.
/// No retry, error reconstruction, shutdown, stdout read, or fd0 access is added.
#[cfg(any(target_os = "linux", test))]
pub(super) async fn write_all_observed<W: AsyncWrite + Unpin>(
    writer: &mut W,
    original_stdin: &[u8],
    mut custody: Option<StdinWriteTicket>,
) -> io::Result<()> {
    if original_stdin.is_empty() {
        if let Some(ticket) = custody.as_mut() {
            ticket.finish(HostStdinWriteTerminal::EmptyInputNoWrite);
        }
        return Ok(());
    }
    if let Some(ticket) = custody.as_mut() {
        ticket.fallback = HostStdinWriteTerminal::WriteAllInterrupted;
    }
    let result = writer.write_all(original_stdin).await;
    if let Some(ticket) = custody.as_mut() {
        let terminal = match &result {
            Ok(()) => HostStdinWriteTerminal::WriteAllCompleted,
            Err(error) => HostStdinWriteTerminal::WriteAllFailed {
                kind: error.kind(),
                raw_os_error: error.raw_os_error(),
            },
        };
        ticket.finish(terminal);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use semwright_types::SourceKind;
    use std::future::{Future, poll_fn};
    use std::pin::Pin;
    use std::task::{Context, Poll};

    fn sink(
        capacity: usize,
    ) -> (
        HostStdinCustody,
        HostStdinCustodyReceiver,
        BoundHostStdinCustody,
    ) {
        let (sink, receiver) =
            HostStdinCustody::channel_for_tool("fixture-tool", capacity).unwrap();
        let provider = ProviderIdentity::external(SourceKind::Driver, "fixture", "1").unwrap();
        let bound = sink.bind(provider, 41);
        (sink, receiver, bound)
    }

    fn capture(bound: &BoundHostStdinCustody, input: &[u8]) -> StdinWriteTicket {
        bound
            .capture(
                &RuntimeToolJob {
                    id: "tool-job-issued".into(),
                },
                "call-validated",
                "parent-validated",
                "session-owned",
                "fixture-tool",
                input,
            )
            .unwrap()
    }

    #[derive(Default)]
    struct Writer {
        delivered: Vec<u8>,
        writes: usize,
        prefix_then_error: Option<io::Error>,
        block_after_prefix: bool,
    }

    impl AsyncWrite for Writer {
        fn poll_write(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
            bytes: &[u8],
        ) -> Poll<io::Result<usize>> {
            self.writes += 1;
            if !self.delivered.is_empty() {
                if let Some(error) = self.prefix_then_error.take() {
                    return Poll::Ready(Err(error));
                }
                if self.block_after_prefix {
                    return Poll::Pending;
                }
            }
            let count = if self.prefix_then_error.is_some() || self.block_after_prefix {
                bytes.len().min(2)
            } else {
                bytes.len()
            };
            self.delivered.extend_from_slice(&bytes[..count]);
            Poll::Ready(Ok(count))
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    #[test]
    fn configuration_and_capture_limit_are_bounded() {
        assert!(HostStdinCustody::channel_for_tool("fixture-tool", 0).is_err());
        assert!(HostStdinCustody::channel_for_tool("fixture-tool", MAX_CAPTURE_QUEUE + 1).is_err());
        assert!(HostStdinCustody::channel_for_tool("", 1).is_err());
        assert!(
            validate_runtime_tool_job_start(
                "fixture-tool",
                &[],
                &vec![0; MAX_CAPTURE_BYTES],
                1,
                None
            )
            .is_ok()
        );
        assert!(
            validate_runtime_tool_job_start(
                "fixture-tool",
                &[],
                &vec![0; MAX_CAPTURE_BYTES + 1],
                1,
                None
            )
            .is_err()
        );
        let (sink, mut receiver, bound) = sink(1);
        assert!(
            bound
                .capture(
                    &RuntimeToolJob { id: "job".into() },
                    "call",
                    "parent",
                    "session",
                    "fixture-tool",
                    &vec![0; MAX_CAPTURE_BYTES + 1]
                )
                .is_none()
        );
        assert_eq!(sink.losses().capture_oversized, 1);
        assert!(receiver.receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn captures_original_bytes_and_host_correlation_before_unchanged_write() {
        let (sink, mut receiver, bound) = sink(1);
        let original = b"a\0b\r\n\xff".to_vec();
        let pointer = original.as_ptr();
        let mut ticket = capture(&bound, &original);
        ticket.linux_broker_entered();
        let record = receiver.recv().await.unwrap();
        assert_eq!(record.provider().id, "driver:fixture");
        assert_eq!(record.provider_process_id(), 41);
        assert_eq!(record.job_id(), "tool-job-issued");
        assert_eq!(record.request_id(), "call-validated");
        assert_eq!(record.parent_request_id(), "parent-validated");
        assert_eq!(record.session_id(), "session-owned");
        assert_eq!(record.tool(), "fixture-tool");
        assert_eq!(record.original_stdin(), original);
        let mut writer = Writer::default();
        write_all_observed(&mut writer, &original, Some(ticket))
            .await
            .unwrap();
        assert_eq!(original.as_ptr(), pointer);
        assert_eq!(original, b"a\0b\r\n\xff");
        assert_eq!(writer.delivered, original);
        assert_eq!(writer.writes, 1);
        assert_eq!(
            record.terminal().await.unwrap(),
            HostStdinWriteTerminal::WriteAllCompleted
        );
        assert_eq!(sink.losses(), HostStdinCustodyLosses::default());
    }

    #[tokio::test]
    async fn known_digest_and_platform_or_prewrite_drop_are_explicit() {
        let (_, mut receiver, bound) = sink(1);
        let ticket = capture(&bound, b"abc");
        let record = receiver.recv().await.unwrap();
        assert_eq!(
            record.sha256(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        drop(ticket);
        assert_eq!(
            record.terminal().await.unwrap(),
            HostStdinWriteTerminal::NotObserved
        );
        let mut ticket = capture(&bound, b"abc");
        ticket.linux_broker_entered();
        let record = receiver.recv().await.unwrap();
        drop(ticket);
        assert_eq!(
            record.terminal().await.unwrap(),
            HostStdinWriteTerminal::NotAttempted
        );
    }

    #[tokio::test]
    async fn empty_input_does_not_invent_a_write() {
        let (_, mut receiver, bound) = sink(1);
        let mut ticket = capture(&bound, b"");
        ticket.linux_broker_entered();
        let record = receiver.recv().await.unwrap();
        let mut writer = Writer::default();
        write_all_observed(&mut writer, b"", Some(ticket))
            .await
            .unwrap();
        assert_eq!(writer.writes, 0);
        assert_eq!(
            record.terminal().await.unwrap(),
            HostStdinWriteTerminal::EmptyInputNoWrite
        );
    }

    #[tokio::test]
    async fn partial_write_preserves_the_same_original_error_without_replay() {
        let (_, mut receiver, bound) = sink(1);
        let mut ticket = capture(&bound, b"abcdef");
        ticket.linux_broker_entered();
        let record = receiver.recv().await.unwrap();
        let original_error = io::Error::from_raw_os_error(5);
        let kind = original_error.kind();
        let mut writer = Writer {
            prefix_then_error: Some(original_error),
            ..Writer::default()
        };
        let error = write_all_observed(&mut writer, b"abcdef", Some(ticket))
            .await
            .unwrap_err();
        assert_eq!(error.raw_os_error(), Some(5));
        assert_eq!(error.kind(), kind);
        assert_eq!(writer.delivered, b"ab");
        assert_eq!(writer.writes, 2);
        assert_eq!(
            record.terminal().await.unwrap(),
            HostStdinWriteTerminal::WriteAllFailed {
                kind,
                raw_os_error: Some(5)
            }
        );
    }

    #[tokio::test]
    async fn dropped_write_future_reports_partial_possible_without_retry() {
        let (_, mut receiver, bound) = sink(1);
        let mut ticket = capture(&bound, b"abcdef");
        ticket.linux_broker_entered();
        let record = receiver.recv().await.unwrap();
        let mut writer = Writer {
            block_after_prefix: true,
            ..Writer::default()
        };
        let mut future = Box::pin(write_all_observed(&mut writer, b"abcdef", Some(ticket)));
        poll_fn(|context| {
            assert!(future.as_mut().poll(context).is_pending());
            Poll::Ready(())
        })
        .await;
        drop(future);
        assert_eq!(writer.delivered, b"ab");
        assert_eq!(writer.writes, 2);
        assert_eq!(
            record.terminal().await.unwrap(),
            HostStdinWriteTerminal::WriteAllInterrupted
        );
    }

    #[tokio::test]
    async fn full_closed_and_unselected_sinks_leave_native_input_unchanged() {
        let (sink, mut receiver, bound) = sink(1);
        assert!(
            bound
                .capture(
                    &RuntimeToolJob { id: "job".into() },
                    "call",
                    "parent",
                    "session",
                    "other-tool",
                    b"ignored"
                )
                .is_none()
        );
        let queued = capture(&bound, b"queued");
        let dropped = bound.capture(
            &RuntimeToolJob {
                id: "other-job".into(),
            },
            "call",
            "parent",
            "session",
            "fixture-tool",
            b"original",
        );
        assert!(dropped.is_none());
        assert_eq!(sink.losses().capture_full, 1);
        let mut writer = Writer::default();
        write_all_observed(&mut writer, b"original", dropped)
            .await
            .unwrap();
        assert_eq!(writer.delivered, b"original");
        assert_eq!(writer.writes, 1);
        let record = receiver.recv().await.unwrap();
        drop(queued);
        assert_eq!(
            record.terminal().await.unwrap(),
            HostStdinWriteTerminal::NotObserved
        );
        receiver.close();
        assert!(
            bound
                .capture(
                    &RuntimeToolJob {
                        id: "closed-job".into()
                    },
                    "call",
                    "parent",
                    "session",
                    "fixture-tool",
                    b"original"
                )
                .is_none()
        );
        assert_eq!(sink.losses().capture_closed, 1);
        assert_eq!(receiver.losses(), sink.losses());
    }

    #[tokio::test]
    async fn loss_of_terminal_receiver_is_counted_and_cannot_fail_the_native_write() {
        let (sink, mut receiver, bound) = sink(1);
        let mut ticket = capture(&bound, b"original");
        ticket.linux_broker_entered();
        drop(receiver.recv().await.unwrap());
        let mut writer = Writer::default();
        write_all_observed(&mut writer, b"original", Some(ticket))
            .await
            .unwrap();
        assert_eq!(writer.delivered, b"original");
        assert_eq!(writer.writes, 1);
        assert_eq!(sink.losses().terminal_receiver_lost, 1);
    }
}
