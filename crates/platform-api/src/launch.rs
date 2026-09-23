//! Ownership, digest, format and isolation are separate checks. No signature grants a sandbox.
use semwright_types::{Error, ErrorCode, Result};
use std::path::{Path, PathBuf};
use tokio::process::Command;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgramFormat {
    Elf,
    MachO,
}
pub trait ExecutableVerifier: Send + Sync {
    fn verify(&self, path: &Path, sha256: &str) -> Result<Vec<u8>>;
}
#[derive(Clone, Debug)]
pub struct Mount {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub read_only: bool,
}
#[derive(Clone, Debug)]
pub struct ResourceLimits {
    pub open_files: u64,
    pub processes: u64,
    pub cpu_seconds: u64,
    pub address_space_bytes: u64,
    pub file_size_bytes: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SandboxKind {
    Driver,
    Plugin,
}
#[derive(Clone, Debug)]
pub struct SandboxSpec {
    pub kind: SandboxKind,
    pub staged_executable: PathBuf,
    pub helper: PathBuf,
    pub mounts: Vec<Mount>,
    pub system_config: Vec<Mount>,
    pub network: bool,
    pub limits: Option<ResourceLimits>,
}
impl SandboxSpec {
    pub fn validate(&self) -> Result<()> {
        if !self.staged_executable.is_absolute() || !self.helper.is_absolute() {
            return Err(Error::invalid(
                "Pinned absolute executable and helper required",
            ));
        }
        let mut names = std::collections::BTreeSet::new();
        for m in self.mounts.iter().chain(&self.system_config) {
            if !m.source.is_absolute() || !names.insert(m.destination.clone()) {
                return Err(Error::invalid(
                    "Absolute source and unique destination required",
                ));
            }
            let system = self
                .system_config
                .iter()
                .any(|s| s.destination == m.destination);
            let root = if system {
                Path::new("/etc")
            } else {
                Path::new("/workspace")
            };
            if !m.destination.starts_with(root) || m.destination == root || (system && !m.read_only)
            {
                return Err(Error::new(
                    ErrorCode::PolicyDenied,
                    "Invalid sandbox mount class",
                ));
            }
            super::filesystem::validate_relative_path(
                m.destination
                    .strip_prefix("/")
                    .map_err(|_| Error::invalid("Mount is not absolute"))?,
            )?;
        }
        if self.kind == SandboxKind::Driver && self.limits.is_none() {
            return Err(Error::invalid("Driver resource limits required"));
        }
        Ok(())
    }
}
pub trait SandboxLauncher: Send + Sync {
    fn command(&self, spec: &SandboxSpec) -> Result<Command>;
    fn available(&self, helper: &Path) -> bool;
    fn mechanism(&self) -> &'static str;
    fn diagnostics(&self, helper: &Path) -> serde_json::Value {
        serde_json::json!({"available":self.available(helper),"helper_present":helper.is_file(),"mechanism":self.mechanism()})
    }
}
