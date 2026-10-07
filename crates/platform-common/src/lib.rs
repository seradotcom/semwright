//! Shared fixture, configuration and scoped-filesystem Backend; no desktop framework.
pub mod artifact;
pub mod fake;
pub mod filesystem;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Owner-selected digest-pinned executable plus fixed argv. Not an agent-supplied command channel.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Application {
    pub executable: PathBuf,
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
}

#[cfg(test)]
fn fixture_root(path: &std::path::Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        // Windows confinement accepts ordinary absolute drive paths; canonicalize
        // returns device syntax that the production boundary deliberately refuses.
        assert!(path.is_absolute());
        path.to_path_buf()
    }
    #[cfg(not(target_os = "windows"))]
    {
        path.canonicalize().unwrap()
    }
}
