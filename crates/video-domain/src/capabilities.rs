//! Complete backend capability snapshots for the semantic video domain.
//!
//! Concrete video drivers may expose richer native capabilities, but every
//! backend participating in the shared semantic edit contract must classify
//! every shared mutation explicitly. Missing entries are an error rather than
//! an implicit claim of support.

use crate::{
    Error, Result,
    model::MODEL_VERSION,
    support::{MutationSupport, VideoOperation},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Canonical shared operation set. VideoOperation is the single source of truth.
pub const SEMANTIC_OPERATIONS: &[VideoOperation] = VideoOperation::ALL;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendGuarantee {
    /// Mutations are checked against a backend-native revision or generation.
    OptimisticConcurrency,
    /// Native mutations are compared with the shared semantic edit engine.
    DifferentialSemanticConformance,
    /// Native state is serialized/reopened, or equivalently reloaded, before publication.
    NativeRoundtripValidation,
    /// New semantic identities are deterministic for a native revision and seed.
    DeterministicSemanticIdentity,
    /// Unknown native structures are preserved rather than flattened into editable semantics.
    UnknownNativePreservation,
    /// Publication is atomic at the concrete backend boundary.
    AtomicPublication,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackendCapabilities {
    /// Stable backend/adapter identity. This is descriptive, not an authority principal.
    pub backend: String,
    pub model_version: u32,
    /// Exhaustive support classification for every shared semantic mutation.
    pub operations: BTreeMap<VideoOperation, MutationSupport>,
    #[serde(default)]
    pub guarantees: BTreeSet<BackendGuarantee>,
}

impl BackendCapabilities {
    pub fn from_supports(
        backend: impl Into<String>,
        guarantees: impl IntoIterator<Item = BackendGuarantee>,
        mut support: impl FnMut(VideoOperation) -> MutationSupport,
    ) -> Result<Self> {
        let capabilities = Self {
            backend: backend.into(),
            model_version: MODEL_VERSION,
            operations: SEMANTIC_OPERATIONS
                .iter()
                .copied()
                .map(|operation| (operation, support(operation)))
                .collect(),
            guarantees: guarantees.into_iter().collect(),
        };
        capabilities.validate()?;
        Ok(capabilities)
    }

    pub fn unsupported(backend: impl Into<String>) -> Result<Self> {
        Self::from_supports(backend, [], |_| MutationSupport::Unsupported)
    }

    pub fn support(&self, operation: VideoOperation) -> Result<MutationSupport> {
        self.operations
            .get(&operation)
            .copied()
            .ok_or_else(|| Error::new("BackendFailed", "Backend capability snapshot is incomplete"))
    }

    pub fn validate(&self) -> Result<()> {
        if self.model_version != MODEL_VERSION {
            return Err(Error::unsupported(
                "Backend capability snapshot targets a different semantic video model version",
            ));
        }
        if self.backend.is_empty()
            || self.backend.len() > 128
            || !self.backend.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/' | b':')
            })
        {
            return Err(Error::invalid("Invalid backend capability identity"));
        }
        if self.operations.len() != SEMANTIC_OPERATIONS.len() {
            return Err(Error::new(
                "BackendFailed",
                "Backend capability snapshot must classify every shared operation exactly once",
            ));
        }
        for operation in SEMANTIC_OPERATIONS {
            if !self.operations.contains_key(operation) {
                return Err(Error::new(
                    "BackendFailed",
                    "Backend capability snapshot is missing a shared operation",
                ));
            }
        }
        Ok(())
    }
}
