//! OBS Studio 5.x application integration.
//! Policy remains exclusively at the Semwright host boundary.
pub mod auth;
pub mod bounds;
pub mod capability;
pub mod client;
pub mod config;
pub mod driver;
pub mod error;
pub mod events;
pub mod graph;
pub mod lifecycle;
pub mod protocol;
pub mod refs;
pub mod state;

pub use driver::ObsDriver;
pub use error::{Fault, FaultKind, Result};
