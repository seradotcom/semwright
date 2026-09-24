//! Explicit workflow recording and deterministic trace-to-recipe compilation.
//!
//! This crate never grants authority. Recorded values are opt-in, candidates
//! are checked against descriptor digests, and promoted recipes still execute
//! each step through the Semwright broker/policy boundary.
mod compiler;
mod model;
mod sanitize;
mod store;

pub use compiler::{
    DescriptorLookup, compile, promoted_descriptor, validate_candidate_integrity, verify_drift,
};
pub use model::*;
pub use sanitize::{looks_like_ref, sanitize};
pub use store::WorkflowManager;
