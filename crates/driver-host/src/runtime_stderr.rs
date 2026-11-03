//! Bounded stderr for one Linux Host-mediated runtime-tool invocation.
//! Raw bytes return only in that caller's existing ToolExecutionOutput.
//! No logs, files, grants, public diagnostics, or shared payload queues.

use crate::MAX_HOST_TOOL_OUTPUT_BYTES;
use semwright_driver_sdk::{Request, RuntimeToolJobStatus};
use semwright_platform_api::launch::{SandboxStderrObserver, SandboxStderrTerminal};
use semwright_types::{Error, ErrorCode, Result};
use std::sync::{Mutex, MutexGuard};
use tokio_util::sync::CancellationToken;

/// Enforce the independent JSON frame budget using the real encoder, before
/// consuming a detached result. No truncation or unbounded queue is introduced.
pub(super) fn bounded_reply(request: Request) -> Result<Request> {
    let output = match &request {
        Request::ToolResult { output, .. }
        | Request::ToolJobState {
            status: RuntimeToolJobStatus::Succeeded { output },
            ..
        } => output,
        _ => {
            semwright_protocol::encode(&request)?;
            return Ok(request);
        }
    };
    if output.validate().is_ok() && semwright_protocol::encode(&request).is_ok() {
        return Ok(request);
    }
    let error = Error::new(
        ErrorCode::ResourceExhausted,
        "Linux Host-mediated sealed tool result exceeds the protocol frame budget",
    )
    .uncertain();
    let bounded = match request {
        Request::ToolResult { id, .. } => Request::ToolFailure { id, error },
        Request::ToolJobState { id, job, .. } => Request::ToolJobState {
            id,
            job,
            status: RuntimeToolJobStatus::Failed { error },
        },
        _ => unreachable!("only tool outputs enter frame-budget replacement"),
    };
    // A malformed caller identifier must not consume a job if even its bounded
    // failure cannot fit. The existing transport rejection then remains active.
    semwright_protocol::encode(&bounded)?;
    Ok(bounded)
}

#[derive(Clone, Copy)]
enum CaptureFailure {
    Overflow,
    ReadFailed,
    Incomplete,
}

impl CaptureFailure {
    fn error(self) -> Error {
        let (code, message) = match self {
            Self::Overflow => (
                ErrorCode::ResourceExhausted,
                "Linux Host-mediated sealed tool stderr exceeded its bound",
            ),
            Self::ReadFailed => (
                ErrorCode::BackendFailed,
                "Linux Host-mediated sealed tool stderr read failed",
            ),
            Self::Incomplete => (
                ErrorCode::BackendFailed,
                "Linux Host-mediated sealed tool stderr capture is incomplete",
            ),
        };
        Error::new(code, message).uncertain()
    }
}

struct CaptureState {
    bytes: Vec<u8>,
    started: bool,
    terminal: Option<SandboxStderrTerminal>,
    failure: Option<CaptureFailure>,
    collected: bool,
}

impl Default for CaptureState {
    fn default() -> Self {
        Self {
            bytes: Vec::with_capacity(MAX_HOST_TOOL_OUTPUT_BYTES),
            started: false,
            terminal: None,
            failure: None,
            collected: false,
        }
    }
}

#[derive(Default)]
pub(super) struct RuntimeToolStderr {
    state: Mutex<CaptureState>,
    finished: CancellationToken,
    failed: CancellationToken,
}

impl RuntimeToolStderr {
    fn state(&self) -> MutexGuard<'_, CaptureState> {
        match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => {
                let mut state = poisoned.into_inner();
                self.fail(&mut state, CaptureFailure::Incomplete);
                state
            }
        }
    }

    fn fail(&self, state: &mut CaptureState, failure: CaptureFailure) {
        state.failure.get_or_insert(failure);
        self.failed.cancel();
    }

    pub(super) async fn failure(&self) -> Error {
        self.failed.cancelled().await;
        self.state()
            .failure
            .unwrap_or(CaptureFailure::Incomplete)
            .error()
    }

    /// Called within the invocation's original deadline, after process exit.
    /// Missing EOF remains pending; partial bytes are never returned as complete.
    pub(super) async fn finish(&self) -> Result<Vec<u8>> {
        self.finished.cancelled().await;
        let mut state = self.state();
        if let Some(failure) = state.failure {
            return Err(failure.error());
        }
        if !state.started || state.terminal != Some(SandboxStderrTerminal::Eof) || state.collected {
            return Err(CaptureFailure::Incomplete.error());
        }
        state.collected = true;
        Ok(std::mem::take(&mut state.bytes))
    }
}

impl SandboxStderrObserver for RuntimeToolStderr {
    fn started(&self) {
        let mut state = self.state();
        if state.started || state.terminal.is_some() {
            self.fail(&mut state, CaptureFailure::Incomplete);
        } else {
            state.started = true;
        }
    }

    fn bytes(&self, bytes: &[u8]) {
        let mut state = self.state();
        if !state.started || state.terminal.is_some() || state.collected {
            self.fail(&mut state, CaptureFailure::Incomplete);
            return;
        }
        if state.failure.is_some() {
            return;
        }
        let remaining = MAX_HOST_TOOL_OUTPUT_BYTES - state.bytes.len();
        let retain = remaining.min(bytes.len());
        state.bytes.extend_from_slice(&bytes[..retain]);
        if bytes.len() > remaining {
            self.fail(&mut state, CaptureFailure::Overflow);
        }
    }

    fn terminal(&self, terminal: SandboxStderrTerminal) {
        let mut state = self.state();
        if state.terminal.is_some() {
            self.fail(&mut state, CaptureFailure::Incomplete);
        } else {
            state.terminal = Some(terminal);
            match terminal {
                SandboxStderrTerminal::Eof if state.started => {}
                SandboxStderrTerminal::ReadFailed { .. } => {
                    self.fail(&mut state, CaptureFailure::ReadFailed);
                }
                _ => self.fail(&mut state, CaptureFailure::Incomplete),
            }
        }
        drop(state);
        self.finished.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn stderr_bytes_require_eof_and_can_be_collected_only_once() {
        let capture = RuntimeToolStderr::default();
        capture.started();
        capture.bytes(b"original\0stderr\xff");
        assert!(
            tokio::time::timeout(Duration::from_millis(10), capture.finish())
                .await
                .is_err()
        );
        capture.terminal(SandboxStderrTerminal::Eof);
        assert_eq!(capture.finish().await.unwrap(), b"original\0stderr\xff");
        assert!(!capture.finish().await.unwrap_err().outcome_known);
    }

    #[tokio::test]
    async fn empty_stderr_is_valid_only_after_an_observed_start_and_eof() {
        let capture = RuntimeToolStderr::default();
        assert!(
            tokio::time::timeout(Duration::from_millis(10), capture.finish())
                .await
                .is_err()
        );
        capture.started();
        capture.terminal(SandboxStderrTerminal::Eof);
        assert!(capture.finish().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn stderr_overflow_is_bounded_and_never_returns_a_successful_prefix() {
        let capture = RuntimeToolStderr::default();
        capture.started();
        capture.bytes(&vec![b'x'; MAX_HOST_TOOL_OUTPUT_BYTES]);
        assert!(!capture.failed.is_cancelled());
        capture.bytes(b"x");
        capture.bytes(&vec![b'y'; MAX_HOST_TOOL_OUTPUT_BYTES]);
        assert_eq!(capture.state().bytes.len(), MAX_HOST_TOOL_OUTPUT_BYTES);
        assert_eq!(capture.state().bytes.capacity(), MAX_HOST_TOOL_OUTPUT_BYTES);
        let error = capture.failure().await;
        assert_eq!(error.code, ErrorCode::ResourceExhausted);
        assert!(!error.outcome_known);
        capture.terminal(SandboxStderrTerminal::Eof);
        assert_eq!(
            capture.finish().await.unwrap_err().code,
            ErrorCode::ResourceExhausted
        );
    }

    #[tokio::test]
    async fn stderr_read_failure_and_interruption_are_uncertain_without_raw_messages() {
        for terminal in [
            SandboxStderrTerminal::ReadFailed {
                kind: std::io::ErrorKind::Other,
                raw_os_error: Some(5),
            },
            SandboxStderrTerminal::Interrupted,
        ] {
            let capture = RuntimeToolStderr::default();
            capture.started();
            capture.bytes(b"private diagnostic sentinel");
            capture.terminal(terminal);
            let error = capture.failure().await;
            assert_eq!(error.code, ErrorCode::BackendFailed);
            assert!(!error.outcome_known);
            assert!(!error.message.contains("private diagnostic sentinel"));
            assert!(!capture.finish().await.unwrap_err().outcome_known);
        }
    }

    #[tokio::test]
    async fn stderr_missing_start_or_duplicate_terminal_never_becomes_complete() {
        let missing_start = RuntimeToolStderr::default();
        missing_start.terminal(SandboxStderrTerminal::Eof);
        assert!(!missing_start.finish().await.unwrap_err().outcome_known);

        let duplicate_terminal = RuntimeToolStderr::default();
        duplicate_terminal.started();
        duplicate_terminal.terminal(SandboxStderrTerminal::Eof);
        duplicate_terminal.terminal(SandboxStderrTerminal::Eof);
        assert!(!duplicate_terminal.finish().await.unwrap_err().outcome_known);
    }

    #[test]
    fn host_tool_redaction_preserves_uncertainty_and_drops_private_payloads() {
        for outcome_known in [false, true] {
            let mut original = Error::new(ErrorCode::BackendFailed, "private diagnostic sentinel");
            original.outcome_known = outcome_known;
            original.candidates.push("private candidate".into());
            let redacted = crate::redact_host_tool_error(original, "Host-mediated tool failed");
            assert_eq!(redacted.code, ErrorCode::BackendFailed);
            assert_eq!(redacted.outcome_known, outcome_known);
            assert_eq!(redacted.message, "Host-mediated tool failed");
            assert!(redacted.candidates.is_empty());
            assert!(redacted.progress.is_none());
            assert!(redacted.native_diagnostic.is_none());
        }
    }

    fn job_reply(stdout: Vec<u8>, stderr: Vec<u8>) -> Request {
        Request::ToolJobState {
            id: "stderr-status-request".into(),
            job: semwright_driver_sdk::RuntimeToolJob {
                id: "stderr-original-job".into(),
            },
            status: RuntimeToolJobStatus::Succeeded {
                output: semwright_driver_sdk::ToolExecutionOutput {
                    exit_code: 2,
                    stdout,
                    stderr,
                },
            },
        }
    }

    #[test]
    fn raw_and_job_replies_reject_real_oversized_json_as_uncertain_failures() {
        let output = semwright_driver_sdk::ToolExecutionOutput {
            exit_code: 2,
            stdout: b"stdout".to_vec(),
            stderr: vec![b'e'; MAX_HOST_TOOL_OUTPUT_BYTES],
        };
        for request in [
            Request::ToolResult {
                id: "stderr-execute-request".into(),
                output: output.clone(),
            },
            job_reply(output.stdout, output.stderr),
        ] {
            assert!(semwright_protocol::encode(&request).is_err());
            let bounded = bounded_reply(request).unwrap();
            assert!(semwright_protocol::encode(&bounded).is_ok());
            let error = match bounded {
                Request::ToolFailure { error, .. }
                | Request::ToolJobState {
                    status: RuntimeToolJobStatus::Failed { error },
                    ..
                } => error,
                _ => panic!("oversized result must not remain a completed output"),
            };
            assert_eq!(error.code, ErrorCode::ResourceExhausted);
            assert!(!error.outcome_known);
            assert!(!error.message.contains("eeee"));
        }
    }

    #[test]
    fn exact_channel_bound_is_preserved_when_the_real_frame_fits() {
        let request = job_reply(b"stdout".to_vec(), vec![1; MAX_HOST_TOOL_OUTPUT_BYTES]);
        assert!(semwright_protocol::encode(&request).is_ok());
        match bounded_reply(request).unwrap() {
            Request::ToolJobState {
                status: RuntimeToolJobStatus::Succeeded { output },
                ..
            } => {
                assert_eq!(output.exit_code, 2);
                assert_eq!(output.stdout, b"stdout");
                assert_eq!(output.stderr, vec![1; MAX_HOST_TOOL_OUTPUT_BYTES]);
            }
            _ => panic!("complete output inside both independent bounds must be retained"),
        }
    }

    #[test]
    fn two_maximum_channels_still_obey_the_existing_frame_bound() {
        let request = job_reply(
            vec![1; MAX_HOST_TOOL_OUTPUT_BYTES],
            vec![1; MAX_HOST_TOOL_OUTPUT_BYTES],
        );
        assert!(semwright_protocol::encode(&request).is_err());
        match bounded_reply(request).unwrap() {
            Request::ToolJobState {
                status: RuntimeToolJobStatus::Failed { error },
                ..
            } => {
                assert_eq!(error.code, ErrorCode::ResourceExhausted);
                assert!(!error.outcome_known);
            }
            _ => panic!("per-channel limits do not override the wire-frame limit"),
        }
    }

    #[test]
    fn unencodable_failure_is_rejected_before_a_job_can_be_consumed() {
        let request = Request::ToolResult {
            id: "x".repeat(semwright_types::MAX_FRAME),
            output: semwright_driver_sdk::ToolExecutionOutput {
                exit_code: 2,
                stdout: Vec::new(),
                stderr: Vec::new(),
            },
        };
        assert!(bounded_reply(request).is_err());
    }

    #[test]
    fn terminal_failure_with_a_valid_incoming_frame_is_checked_before_consumption() {
        let job = semwright_driver_sdk::RuntimeToolJob {
            id: "stderr-original-job".into(),
        };
        let mut incoming = semwright_driver_sdk::Response::ToolJobStatus {
            id: String::new(),
            parent: "stderr-parent".into(),
            job: job.clone(),
        };
        let base_bytes = semwright_protocol::encode(&incoming).unwrap().len() - 4;
        let id = "x".repeat(semwright_types::MAX_FRAME - base_bytes);
        if let semwright_driver_sdk::Response::ToolJobStatus {
            id: incoming_id, ..
        } = &mut incoming
        {
            *incoming_id = id.clone();
        }
        assert_eq!(
            semwright_protocol::encode(&incoming).unwrap().len(),
            semwright_types::MAX_FRAME + 4
        );
        let reply = Request::ToolJobState {
            id,
            job,
            status: RuntimeToolJobStatus::Failed {
                error: Error::new(
                    ErrorCode::BackendFailed,
                    "Typed Host-mediated detached runtime-tool job failed",
                )
                .uncertain(),
            },
        };
        assert!(semwright_protocol::encode(&reply).is_err());
        assert!(bounded_reply(reply).is_err());
    }
}
