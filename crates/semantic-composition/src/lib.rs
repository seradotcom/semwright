//! Backend-neutral contracts. No I/O, native APIs, media clocks or execution authority.
mod canonical;
mod controller;
mod model;
mod vault;
pub use canonical::*;
pub use controller::*;
pub use model::*;
pub use vault::*;

pub const CONTRACT_VERSION: u32 = 1;
pub const MAX_PAYLOAD_BYTES: usize = 524_288;
pub const MAX_ENTRIES: usize = 4096;
pub type Result<T> = std::result::Result<T, ContractError>;
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ContractError {
    #[error("invalid contract: {0}")]
    Invalid(String),
    #[error("stale state: {0}")]
    Stale(String),
    #[error("resource limit: {0}")]
    Limit(String),
    #[error("unknown outcome: {0}")]
    Unknown(String),
    #[error("denied: {0}")]
    Denied(String),
}
pub fn ensure(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(ContractError::Invalid(message.into()))
    }
}
pub fn bounded_id(value: &str) -> Result<()> {
    ensure(
        !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control),
        "identifier must be nonempty, bounded and control-free",
    )
}
