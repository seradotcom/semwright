//! Managed Motion Canvas domain and deterministic compiler.
pub mod model;
pub mod security;
pub mod validate;
pub mod refs;
pub mod diff;
pub mod compiler;
pub use semwright_types::{Error, ErrorCode, Result};
#[cfg(test)]
mod tests;
