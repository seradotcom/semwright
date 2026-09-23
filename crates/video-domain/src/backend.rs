//! Backend-neutral projection and capability contracts.
//!
//! Concrete editors own native state and expose a conservative semantic
//! projection plus explicit operation support. Support never grants authority;
//! policy, consent, persistence, rendering, sandboxing and revision checks stay
//! above or inside the concrete driver.

use crate::{
    Error, Result,
    model::{MODEL_VERSION, Project},
    support::{MutationSupport, VideoOperation},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const BACKEND_CONTRACT_VERSION: u32 = 1;
pub const MAX_PROJECTION_LOSSES: usize = 4096;
const MAX_ID_LEN: usize = 128;
const MAX_DETAIL_LEN: usize = 2048;
const MAX_PATH_LEN: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionFidelity {
    Exact,
    SemanticallyEquivalent,
    LossyReadOnly,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionLossKind {
    OpaqueNativeObject,
    UnsupportedSemantic,
    NativeMetadataOnly,
    UnknownNativeVersion,
    RoundTripRisk,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionLossImpact {
    Advisory,
    ReadOnly,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectionLoss {
    pub kind: ProjectionLossKind,
    pub impact: ProjectionLossImpact,
    pub code: String,
    pub semantic_path: Option<String>,
    pub detail: String,
}
impl ProjectionLoss {
    pub fn validate(&self) -> Result<()> {
        validate_token("projection loss code", &self.code, false)?;
        validate_text("projection loss detail", &self.detail, MAX_DETAIL_LEN)?;
        if let Some(path) = &self.semantic_path {
            validate_text("projection semantic path", path, MAX_PATH_LEN)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationCapability {
    pub operation: VideoOperation,
    pub support: MutationSupport,
    pub reason: Option<String>,
}
impl OperationCapability {
    pub fn validate(&self) -> Result<()> {
        if let Some(reason) = &self.reason {
            validate_text("operation capability reason", reason, MAX_DETAIL_LEN)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackendIdentity {
    pub backend_id: String,
    pub backend_version: Option<String>,
    pub adapter_id: String,
}

impl BackendIdentity {
    pub fn validate(&self) -> Result<()> {
        validate_token("backend ID", &self.backend_id, false)?;
        validate_token("adapter ID", &self.adapter_id, true)?;
        if let Some(version) = &self.backend_version {
            validate_text("backend version", version, MAX_ID_LEN)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackendContract {
    pub contract_version: u32,
    pub identity: BackendIdentity,
    pub semantic_model_version: u32,
    pub projection_fidelity: ProjectionFidelity,
    pub operations: Vec<OperationCapability>,
}

impl BackendContract {
    /// Build a complete project-scoped support matrix from one classification
    /// function. Every shared operation is inserted exactly once and the
    /// resulting contract is validated before publication.
    pub fn from_support(
        identity: BackendIdentity,
        projection_fidelity: ProjectionFidelity,
        mut classify: impl FnMut(VideoOperation) -> (MutationSupport, Option<String>),
    ) -> Result<Self> {
        let operations = VideoOperation::ALL
            .iter()
            .copied()
            .map(|operation| {
                let (support, reason) = classify(operation);
                OperationCapability {
                    operation,
                    support,
                    reason,
                }
            })
            .collect();

        let contract = Self {
            contract_version: BACKEND_CONTRACT_VERSION,
            identity,
            semantic_model_version: MODEL_VERSION,
            projection_fidelity,
            operations,
        };
        contract.validate()?;
        Ok(contract)
    }

    pub fn validate(&self) -> Result<()> {
        if self.contract_version != BACKEND_CONTRACT_VERSION {
            return Err(Error::unsupported(
                "Unsupported video backend contract version",
            ));
        }
        if self.semantic_model_version != MODEL_VERSION {
            return Err(Error::unsupported(
                "Unsupported semantic video model version",
            ));
        }
        self.identity.validate()?;
        if self.operations.len() != VideoOperation::ALL.len() {
            return Err(Error::invalid(
                "Backend contract must declare every semantic video operation exactly once",
            ));
        }

        let mut seen = BTreeSet::new();
        for capability in &self.operations {
            capability.validate()?;
            if !seen.insert(capability.operation) {
                return Err(Error::invalid(
                    "Backend contract contains duplicate semantic video operation",
                ));
            }
        }
        if VideoOperation::ALL
            .iter()
            .any(|operation| !seen.contains(operation))
        {
            return Err(Error::invalid(
                "Backend contract omits a semantic video operation",
            ));
        }
        Ok(())
    }

    pub fn support(&self, operation: VideoOperation) -> MutationSupport {
        self.operations
            .iter()
            .find(|capability| capability.operation == operation)
            .map_or(MutationSupport::Unsupported, |capability| {
                capability.support
            })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectionReport {
    pub project: Project,
    pub fidelity: ProjectionFidelity,
    #[serde(default)]
    pub losses: Vec<ProjectionLoss>,
}

impl ProjectionReport {
    pub fn validate(&self) -> Result<()> {
        self.project.validate()?;
        if self.losses.len() > MAX_PROJECTION_LOSSES {
            return Err(Error::limit("Projection loss budget exceeded"));
        }
        for loss in &self.losses {
            loss.validate()?;
        }
        if self.fidelity == ProjectionFidelity::Exact && !self.losses.is_empty() {
            return Err(Error::invalid(
                "Exact projection cannot report semantic projection losses",
            ));
        }
        if self
            .losses
            .iter()
            .any(|loss| loss.impact == ProjectionLossImpact::ReadOnly)
            && self.fidelity != ProjectionFidelity::LossyReadOnly
        {
            return Err(Error::invalid(
                "Read-only projection loss requires lossy_read_only fidelity",
            ));
        }
        Ok(())
    }
}

/// Pure semantic boundary implemented by each native video backend. Native
/// mutation and side effects intentionally remain outside this trait.
pub trait SemanticVideoProjection<Native> {
    fn contract(&self, native: &Native) -> Result<BackendContract>;
    fn project(&self, native: &Native) -> Result<ProjectionReport>;
}
fn validate_token(label: &str, value: &str, allow_slash: bool) -> Result<()> {
    let lexical_ok = !value.is_empty()
        && value.len() <= MAX_ID_LEN
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'-' | b'_' | b'.')
                || (allow_slash && byte == b'/')
        });

    let segments_ok = !allow_slash
        || (!value.starts_with('/')
            && !value.ends_with('/')
            && !value.contains("//")
            && value
                .split('/')
                .all(|segment| !matches!(segment, "" | "." | "..")));

    if !lexical_ok || !segments_ok {
        return Err(Error::invalid(format!("Invalid {label}")));
    }
    Ok(())
}

fn validate_text(label: &str, value: &str, max_len: usize) -> Result<()> {
    if value.is_empty() || value.len() > max_len || value.chars().any(char::is_control) {
        return Err(Error::invalid(format!("Invalid {label}")));
    }
    Ok(())
}
