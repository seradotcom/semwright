//! Shared fixture, configuration and scoped-filesystem Backend; no desktop framework.
pub mod artifact;
pub mod fake;
pub mod filesystem;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Owner-selected fixed argv. Not an agent-supplied executable command channel.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Application {
    pub executable: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
}
