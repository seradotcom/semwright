//! Project identity and evidence, not an execution authority or conversation memory.
//! Composition's owner, resources, base states, digests and verification are reused.
#[cfg(feature = "store")]
mod store;
#[cfg(feature = "store")]
pub use store::*;
mod watch;
pub use watch::*;
mod query;
pub use query::*;
mod manifest;
pub use manifest::*;
mod graph;
pub use graph::*;
mod identity;
mod knowledge;
mod model;
pub use identity::*;
pub use knowledge::*;
pub use model::*;
pub use semwright_semantic_composition as composition;
pub const SCHEMA_VERSION: u32 = 1;
pub const MAX_DEPENDENCIES: usize = 1024;
pub const MAX_RESOURCES: usize = 20_000;
pub const MAX_EDGES: usize = 100_000;
pub type Result<T> = std::result::Result<T, GraphError>;
#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    #[error("project database operation failed; original evidence preserved")]
    Storage,
    #[error("invalid project graph: {0}")]
    Invalid(&'static str),
    #[error("project access denied")]
    Denied,
    #[error("project state changed or conflicts with evidence")]
    Conflict,
    #[error("project graph budget exhausted: {0}")]
    Limit(&'static str),
    #[error("project evidence is corrupt; preserved for explicit recovery")]
    Corrupt,
    #[error("project operation cancelled")]
    Cancelled,
    #[error("composition contract rejected the record")]
    Contract(#[from] composition::ContractError),
    #[error("project storage I/O failed")]
    Io(#[from] std::io::Error),
}
pub(crate) fn ensure(condition: bool, why: &'static str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(GraphError::Invalid(why))
    }
}
pub(crate) fn name(value: &str) -> Result<()> {
    ensure(
        !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control),
        "bounded name",
    )
}
