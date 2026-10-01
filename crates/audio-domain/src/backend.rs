//! Versioned backend projection and capability contract for semantic audio.

use crate::{
    Error, Result,
    model::{AudioProject, MODEL_VERSION},
    support::{AudioOperation, OperationSupport},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const BACKEND_CONTRACT_VERSION: u32 = 1;
pub const MAX_PROJECTION_LOSSES: usize = 4096;
const MAX_ID_LEN: usize = 128;
const MAX_TEXT_LEN: usize = 2048;
const MAX_PATH_LEN: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionFidelity {
    Exact,
    SemanticallyEquivalent,
    LossyReadOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionLossKind {
    OpaqueNativeObject,
    UnsupportedSemantic,
    NativeMetadataOnly,
    UnknownNativeVersion,
    RoundTripRisk,
    MissingMedia,
    UnsupportedPlugin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionLossImpact {
    Advisory,
    ReadOnly,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
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
        validate_text("projection loss detail", &self.detail, MAX_TEXT_LEN)?;
        if let Some(path) = &self.semantic_path {
            validate_text("projection semantic path", path, MAX_PATH_LEN)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OperationCapability {
    pub operation: AudioOperation,
    pub support: OperationSupport,
    pub reason: Option<String>,
}
impl OperationCapability {
    pub fn validate(&self) -> Result<()> {
        if let Some(reason) = &self.reason {
            validate_text("operation support reason", reason, MAX_TEXT_LEN)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BackendContract {
    pub contract_version: u32,
    pub identity: BackendIdentity,
    pub semantic_model_version: u32,
    pub projection_fidelity: ProjectionFidelity,
    pub operations: Vec<OperationCapability>,
}
impl BackendContract {
    pub fn from_support(
        identity: BackendIdentity,
        projection_fidelity: ProjectionFidelity,
        mut classify: impl FnMut(AudioOperation) -> (OperationSupport, Option<String>),
    ) -> Result<Self> {
        let operations = AudioOperation::ALL
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
        let value = Self {
            contract_version: BACKEND_CONTRACT_VERSION,
            identity,
            semantic_model_version: MODEL_VERSION,
            projection_fidelity,
            operations,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<()> {
        if self.contract_version != BACKEND_CONTRACT_VERSION {
            return Err(Error::unsupported(
                "Unsupported audio backend contract version",
            ));
        }
        if self.semantic_model_version != MODEL_VERSION {
            return Err(Error::unsupported(
                "Unsupported semantic audio model version",
            ));
        }
        self.identity.validate()?;
        if self.operations.len() != AudioOperation::ALL.len() {
            return Err(Error::invalid(
                "Audio backend contract must declare every operation exactly once",
            ));
        }
        let mut seen = BTreeSet::new();
        for capability in &self.operations {
            capability.validate()?;
            if !seen.insert(capability.operation) {
                return Err(Error::invalid(
                    "Duplicate audio operation in backend contract",
                ));
            }
        }
        if AudioOperation::ALL.iter().any(|op| !seen.contains(op)) {
            return Err(Error::invalid("Audio backend contract omits an operation"));
        }
        Ok(())
    }

    pub fn support(&self, operation: AudioOperation) -> OperationSupport {
        self.operations
            .iter()
            .find(|value| value.operation == operation)
            .map_or(OperationSupport::Unsupported, |value| value.support)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectionReport {
    pub project: AudioProject,
    pub fidelity: ProjectionFidelity,
    #[serde(default)]
    pub losses: Vec<ProjectionLoss>,
}
impl ProjectionReport {
    pub fn validate(&self) -> Result<()> {
        self.project.validate()?;
        if self.losses.len() > MAX_PROJECTION_LOSSES {
            return Err(Error::limit("Audio projection loss budget exceeded"));
        }
        for loss in &self.losses {
            loss.validate()?;
        }
        if self.fidelity == ProjectionFidelity::Exact && !self.losses.is_empty() {
            return Err(Error::invalid(
                "Exact audio projection cannot report losses",
            ));
        }
        if self
            .losses
            .iter()
            .any(|loss| loss.impact == ProjectionLossImpact::ReadOnly)
            && self.fidelity != ProjectionFidelity::LossyReadOnly
        {
            return Err(Error::invalid(
                "Read-only audio projection loss requires lossy_read_only fidelity",
            ));
        }
        Ok(())
    }
}

pub trait SemanticAudioProjection<Native> {
    fn contract(&self, native: &Native) -> Result<BackendContract>;
    fn project(&self, native: &Native) -> Result<ProjectionReport>;
}

fn validate_token(label: &str, value: &str, allow_slash: bool) -> Result<()> {
    let lexical = !value.is_empty()
        && value.len() <= MAX_ID_LEN
        && value.bytes().all(|b| {
            b.is_ascii_alphanumeric()
                || matches!(b, b'-' | b'_' | b'.')
                || (allow_slash && b == b'/')
        });
    let segments = !allow_slash
        || (!value.starts_with('/')
            && !value.ends_with('/')
            && !value.contains("//")
            && value.split('/').all(|s| !matches!(s, "" | "." | "..")));
    if !lexical || !segments {
        return Err(Error::invalid(format!("Invalid {label}")));
    }
    Ok(())
}

fn validate_text(label: &str, value: &str, max: usize) -> Result<()> {
    if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(Error::invalid(format!("Invalid {label}")));
    }
    Ok(())
}
