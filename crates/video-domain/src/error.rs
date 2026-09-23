use std::fmt;

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
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for Error {}
