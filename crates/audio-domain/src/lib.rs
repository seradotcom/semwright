//! Backend-neutral semantic audio domain.
//!
//! This crate models audio intent. It deliberately does not know about Ardour,
//! Faust, FFmpeg, a model vendor, a shell, a filesystem layout or an agent.
//! Concrete drivers project native state into these types and separately
//! declare fidelity and operation support.

pub mod analysis;
pub mod backend;
pub mod conformance;
pub mod edit;
pub mod error;
pub mod hash;
pub mod model;
pub mod presets;
pub mod provider;
pub mod refs;
pub mod render;
pub mod support;
pub mod time;
pub mod units;

pub use error::{Error, Result};
