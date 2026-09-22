use crate::json::{Value, obj};
use std::fmt;
/// Error codes use the observed Semwright PascalCase wire vocabulary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub code: &'static str,
    pub message: String,
    pub outcome_known: bool,
}
pub type Result<T> = std::result::Result<T, Error>;
impl Error {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            outcome_known: true,
        }
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("InvalidArgument", message)
    }
    pub fn limit(message: impl Into<String>) -> Self {
        Self::new("ResourceExhausted", message)
    }
    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::new("Unsupported", message)
    }
    pub fn stale() -> Self {
        Self::new(
            "StaleReference",
            "Reference or expected revision is stale; observe again",
        )
    }
    pub fn json(&self) -> Value {
        obj([
            ("code", self.code.into()),
            ("message", self.message.clone().into()),
            ("outcome_known", self.outcome_known.into()),
        ])
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        let code = if value.kind() == std::io::ErrorKind::NotFound {
            "NotFound"
        } else {
            "BackendFailed"
        };
        Self::new(code, format!("I/O operation failed ({:?})", value.kind()))
    }
}
