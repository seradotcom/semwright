//! Ownership, digest, format and isolation are separate checks. No signature grants a sandbox.
use semwright_types::{Error, ErrorCode, Result};
use std::path::{Path, PathBuf};
use tokio::process::Command;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgramFormat {
    Elf,
    MachO,
    Pe,
}

pub trait ExecutableVerifier: Send + Sync {
    fn verify(&self, path: &Path, sha256: &str) -> Result<Vec<u8>>;
}

/// Logical mount identity. The platform host materializes its own filesystem namespace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MountClass {
    Workspace,
    SystemConfig,
}

#[derive(Clone, Debug)]
pub struct Mount {
    pub source: PathBuf,
    pub class: MountClass,
    pub logical_name: String,
    pub read_only: bool,
}
impl Mount {
    pub fn validate(&self) -> Result<()> {
        if !self.source.is_absolute() {
            return Err(Error::invalid("Mount source must be absolute"));
        }
        let p = Path::new(&self.logical_name);
        super::filesystem::validate_relative_path(p)?;
        if self.logical_name.len() > 255 {
            return Err(Error::invalid("Logical mount name exceeds budget"));
        }
        if self.class == MountClass::SystemConfig && !self.read_only {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "System-config mounts must be read-only",
            ));
        }
        Ok(())
    }
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
        for m in &self.mounts {
            m.validate()?;
            if !names.insert((m.class as u8, m.logical_name.clone())) {
                return Err(Error::invalid("Duplicate logical sandbox mount"));
            }
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
