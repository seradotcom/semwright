use serde::Serialize;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FaultKind {
    Configuration,
    Authentication,
    Protocol,
    Transport,
    Timeout,
    Cancelled,
    NotReady,
    Unsupported,
    StaleReference,
    Precondition,
    ResourceLimit,
    Application,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Fault {
    pub kind: FaultKind,
    pub outcome_known: bool,
}
pub type Result<T> = std::result::Result<T, Fault>;

impl Fault {
    pub fn new(kind: FaultKind) -> Self {
        Self {
            kind,
            outcome_known: true,
        }
    }
    pub fn uncertain(mut self) -> Self {
        self.outcome_known = false;
        self
    }
    pub fn retryable_connection(&self) -> bool {
        matches!(
            self.kind,
            FaultKind::Transport | FaultKind::Timeout | FaultKind::NotReady
        )
    }
    pub fn for_mutation(mut self, mutation: bool) -> Self {
        if mutation
            && matches!(
                self.kind,
                FaultKind::Transport
                    | FaultKind::Timeout
                    | FaultKind::Cancelled
                    | FaultKind::Closed
                    | FaultKind::Protocol
            )
        {
            self.outcome_known = false;
        }
        self
    }
    pub fn semwright(self) -> semwright_types::Error {
        use semwright_types::{Error, ErrorCode};
        let code = match self.kind {
            FaultKind::Configuration => ErrorCode::InvalidArgument,
            FaultKind::Authentication => ErrorCode::PermissionDenied,
            FaultKind::Protocol => ErrorCode::ProtocolMismatch,
            FaultKind::Transport | FaultKind::NotReady | FaultKind::Closed => {
                ErrorCode::Unavailable
            }
            FaultKind::Timeout => ErrorCode::Timeout,
            FaultKind::Cancelled => ErrorCode::Cancelled,
            FaultKind::Unsupported => ErrorCode::Unsupported,
            FaultKind::StaleReference => ErrorCode::StaleReference,
            FaultKind::Precondition => ErrorCode::Conflict,
            FaultKind::ResourceLimit => ErrorCode::ResourceExhausted,
            FaultKind::Application => ErrorCode::BackendFailed,
        };
        let mut error = Error::new(
            code,
            format!(
                "OBS driver: {:?}; external diagnostic text withheld",
                self.kind
            ),
        );
        error.outcome_known = self.outcome_known;
        error
    }
}
impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.kind)
    }
}
impl std::error::Error for Fault {}
impl From<serde_json::Error> for Fault {
    fn from(_: serde_json::Error) -> Self {
        Self::new(FaultKind::Protocol)
    }
}
impl From<std::io::Error> for Fault {
    fn from(_: std::io::Error) -> Self {
        Self::new(FaultKind::Transport)
    }
}
