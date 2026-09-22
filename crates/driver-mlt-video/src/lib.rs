//! Offline semantic video editing. No network client, shell, Python runtime or native FFI to MLT.
//! Linux confinement is a separate module. All native application support is conservatively gated.
// The imported bounded parsers intentionally use sequential guards so every rejected condition
// keeps its own error classification. Collapsing them would make that review surface less clear.
#![allow(
    clippy::chunks_exact_to_as_chunks,
    clippy::collapsible_if,
    clippy::type_complexity
)]
pub mod adapters;
pub mod app;
pub mod catalog;
pub mod edit;
pub mod error;
pub mod fs;
pub mod hash;
pub mod jobs;
pub mod json;
pub mod model;
pub mod refs;
pub mod runtime;
pub mod time;
pub mod wire;
pub mod xml;
pub use error::{Error, Result};
