//! Explicit workflow recording and deterministic trace-to-recipe compilation.
//!
//! This crate never grants authority. Recorded values are opt-in, candidates
//! are checked against descriptor digests, and promoted recipes still execute
//! each step through the Semwright broker/policy boundary.
mod compiler;
mod miner;
mod model;
mod proposal;
mod sanitize;
mod store;

pub use compiler::{
    DescriptorLookup, compile, promoted_descriptor, validate_candidate_integrity, verify_drift,
};
pub use miner::{
    default_suggestions, mine_patterns, mine_suggestions, pattern_fingerprint, trace_compile_ready,
    validate_min_occurrences,
};
pub use model::*;
pub use proposal::{
    DEFAULT_MIN_PROPOSAL_TRACES, EvidenceTier, PROPOSAL_VERSION, ProposalBuild, ProposalEvidence,
    ProposalInput, WorkflowProposal, build_proposal,
};
pub use sanitize::{looks_like_ref, sanitize};
pub use store::WorkflowManager;
