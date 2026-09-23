//! Protocol-independent long-operation state. Transport adapters may map this to MCP Tasks.
use crate::{Envelope, Error, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}
impl JobState {
    pub fn terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JobProgress {
    pub completed: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}
impl JobProgress {
    pub fn validate(&self) -> Result<()> {
        if self.total == Some(0)
            || self.total.is_some_and(|total| self.completed > total)
            || self.message.as_ref().is_some_and(|message| {
                message.is_empty() || message.len() > 256 || message.chars().any(char::is_control)
            })
        {
            return Err(Error::invalid("Job progress exceeds its bounded contract"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JobArtifact {
    pub name: String,
    pub reference: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
}
impl JobArtifact {
    pub fn validate(&self) -> Result<()> {
        let bounded = |value: &str, max: usize| {
            !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
        };
        if !bounded(&self.name, 128)
            || !bounded(&self.reference, 512)
            || self
                .media_type
                .as_ref()
                .is_some_and(|value| !bounded(value, 128))
            || self.sha256.as_ref().is_some_and(|digest| {
                digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
        {
            return Err(Error::invalid(
                "Job artifact metadata exceeds its bounded contract",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JobSnapshot {
    pub id: String,
    pub command: String,
    pub state: JobState,
    pub created_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at_ms: Option<u64>,
    pub cancellation_requested: bool,
    pub cancellable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<JobProgress>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<JobArtifact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Box<Envelope>>,
    pub result_omitted: bool,
}
