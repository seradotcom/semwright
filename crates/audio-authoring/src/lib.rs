//! Audio-specific planning and measured constraints; common lifecycle remains in C0.
pub mod intent;
pub mod planner;
pub mod session;
pub mod validation;
pub use intent::*;
pub use planner::*;
use semwright_semantic_composition::{ContractError, Result};
pub use session::*;
pub use validation::*;
fn domain<T>(value: semwright_audio_domain::Result<T>) -> Result<T> {
    value.map_err(|error| ContractError::Invalid(format!("{}: {}", error.code, error.message)))
}
