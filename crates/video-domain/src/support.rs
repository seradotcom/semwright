//! Backend-neutral mutation support classification.
//!
//! This is semantic capability information, not a permission decision. Policy,
//! consent and backend-specific preconditions remain outside this type.

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MutationSupport {
    /// The backend can preserve its native project representation safely.
    SafeRoundtrip,
    /// Semantics are supported but native application metadata may change.
    MetadataRisk,
    /// The representation can be rendered/observed but not safely mutated.
    RenderOnly,
    /// The backend cannot perform this semantic mutation.
    Unsupported,
}

impl MutationSupport {
    pub fn allows_mutation(self) -> bool {
        matches!(self, Self::SafeRoundtrip | Self::MetadataRisk)
    }

    pub fn requires_metadata_acknowledgement(self) -> bool {
        self == Self::MetadataRisk
    }
}
