//! Protocol-independent long-operation state. Transport adapters may map this to MCP Tasks.
use crate::Envelope;
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
    pub result: Option<Box<Envelope>>,
    pub result_omitted: bool,
}
