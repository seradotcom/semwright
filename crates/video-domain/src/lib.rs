//! Backend-neutral semantic video primitives.
//!
//! This crate intentionally knows nothing about concrete media backends,
//! application processes, native project serialization, filesystem roots, or
//! render executables. Backends translate their native representation into
//! these types and keep round-trip metadata outside the semantic model.

pub mod conformance;
pub mod edit;
pub mod error;
pub mod hash;
pub mod model;
pub mod refs;
pub mod support;
pub mod time;

pub use error::{Error, Result};
