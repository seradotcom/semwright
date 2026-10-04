//! Bounded, untrusted diagnostic DATA; never authority or a known outcome.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
pub enum NativeFailurePhase {
    Startup,
    NativeExchange,
    HostRead,
    HostWrite,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
pub enum NativeFailureReason {
    ChildExit,
    UnexpectedEof,
    Timeout,
    InvalidFrame,
    IoFailure,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NativeDiagnostic {
    pub phase: NativeFailurePhase,
    pub reason: NativeFailureReason,
    #[schemars(range(min = 0, max = 255))]
    pub exit_code: Option<i32>,
    #[schemars(range(min = 1, max = 64))]
    pub signal: Option<i32>,
    #[schemars(length(min = 64, max = 64), regex(pattern = "^[a-f0-9]{64}$"))]
    pub stderr_tail_sha256: String,
    #[schemars(range(min = 0, max = 8192))]
    pub stderr_tail_bytes: u16,
    pub stderr_tail_truncated: bool,
}

impl NativeDiagnostic {
    pub fn is_valid(&self) -> bool {
        self.exit_code.is_none_or(|n| (0..=255).contains(&n))
            && self.signal.is_none_or(|n| (1..=64).contains(&n))
            && !(self.exit_code.is_some() && self.signal.is_some())
            && self.stderr_tail_bytes <= 8192
            && self.stderr_tail_sha256.len() == 64
            && self
                .stderr_tail_sha256
                .bytes()
                .all(|n| n.is_ascii_digit() || (b'a'..=b'f').contains(&n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Error, ErrorCode};

    fn diagnostic() -> NativeDiagnostic {
        NativeDiagnostic {
            phase: NativeFailurePhase::NativeExchange,
            reason: NativeFailureReason::UnexpectedEof,
            exit_code: None,
            signal: None,
            stderr_tail_sha256: "a".repeat(64),
            stderr_tail_bytes: 8192,
            stderr_tail_truncated: true,
        }
    }

    #[test]
    fn old_errors_round_trip_without_a_diagnostic_extension() {
        let value = serde_json::to_value(Error::new(ErrorCode::BackendFailed, "redacted")).unwrap();
        assert!(value.get("native_diagnostic").is_none());
        let restored: Error = serde_json::from_value(value).unwrap();
        assert!(restored.native_diagnostic.is_none());
    }

    #[test]
    fn invalid_untrusted_diagnostics_cannot_cross_the_host_filter() {
        for change in 0..7 {
            let mut candidate = diagnostic();
            match change {
                0 => candidate.exit_code = Some(256),
                1 => candidate.signal = Some(0),
                2 => candidate.signal = Some(65),
                3 => {
                    candidate.exit_code = Some(0);
                    candidate.signal = Some(9);
                }
                4 => candidate.stderr_tail_bytes = 8193,
                5 => candidate.stderr_tail_sha256 = "A".repeat(64),
                _ => candidate.stderr_tail_sha256.push('a'),
            }
            assert!(!candidate.is_valid());
        }
        assert!(diagnostic().is_valid());
    }

    #[test]
    fn log_text_paths_and_unknown_classifications_are_rejected() {
        let value = serde_json::to_value(diagnostic()).unwrap();
        for (key, extra) in [
            ("path", serde_json::json!("/private/log")),
            ("stderr", serde_json::json!("secret")),
            ("phase", serde_json::json!("untrusted")),
        ] {
            let mut candidate = value.clone();
            candidate[key] = extra;
            assert!(serde_json::from_value::<NativeDiagnostic>(candidate).is_err());
        }
    }
}
