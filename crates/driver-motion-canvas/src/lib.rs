//! Managed Motion Canvas domain and deterministic compiler.
pub mod audio_codegen;
pub mod compiler;
pub mod diff;
pub mod driver;
pub mod edit;
pub mod model;
pub mod refs;
pub mod renderer;
pub mod security;
pub mod semantic;
pub mod store;
pub mod validate;
pub use semwright_types::{Error, ErrorCode, Result};
#[cfg(test)]
mod tests;
