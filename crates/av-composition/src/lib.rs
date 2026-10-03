//! Fixed audiovisual coordination, not a new general workflow or authority engine.
mod coordinator;
mod executor;
mod model;
mod publication;
mod stage_adapter;
mod sync;
pub use coordinator::*;
pub use executor::*;
pub use model::*;
pub use publication::*;
pub use semwright_semantic_composition::{ContractError as Error, Result};
pub use stage_adapter::*;
pub use sync::*;
