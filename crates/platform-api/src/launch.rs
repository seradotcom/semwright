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
    Secret,
}

#[derive(Clone, Debug)]
pub struct Mount {
    pub source: PathBuf,
    pub class: MountClass,
    pub logical_name: String,
    pub read_only: bool,
    pub execute: bool,
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
        if matches!(self.class, MountClass::SystemConfig | MountClass::Secret)
            && (!self.read_only || self.execute)
        {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "System-config and secret mounts must be read-only and non-executable",
            ));
        }
        if self.execute && (self.class != MountClass::Workspace || !self.read_only) {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Executable mounts must be read-only workspace mounts",
            ));
        }
        if self.class == MountClass::Secret
            && (Path::new(&self.logical_name).components().count() != 1
                || self.logical_name.len() > 64)
        {
            return Err(Error::invalid(
                "Secret mount names must be one bounded path component",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct SealedToolMount {
    pub fd: i32,
    pub name: String,
}
impl SealedToolMount {
    pub fn validate(&self) -> Result<()> {
        if self.fd < 3
            || self.name.is_empty()
            || self.name.len() > 64
            || self.name.starts_with("semwright-internal-")
            || !self
                .name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
        {
            return Err(Error::invalid("Invalid sealed sandbox tool mount"));
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
    ExternalMcp,
}
#[derive(Clone, Debug)]
pub struct SandboxSpec {
    pub kind: SandboxKind,
    pub staged_executable: PathBuf,
    pub helper: PathBuf,
    pub mounts: Vec<Mount>,
    /// Arguments are passed directly to the staged executable; no shell is involved.
    pub args: Vec<String>,
    /// Host-controlled environment only. Manifests cannot populate this directly.
    pub environment: Vec<(String, String)>,
    /// Host-created immutable executable files mounted under `/plugin/tools/<name>`.
    pub sealed_tools: Vec<SealedToolMount>,
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
        if self.args.len() > 64
            || self
                .args
                .iter()
                .any(|arg| arg.len() > 4096 || arg.contains('\0'))
        {
            return Err(Error::invalid("Sandbox executable arguments exceed bounds"));
        }
        if self.environment.len() > 16 {
            return Err(Error::invalid("Sandbox environment exceeds bounds"));
        }
        if self.sealed_tools.len() > 8 {
            return Err(Error::invalid("Sandbox sealed tool count exceeds bounds"));
        }
        let mut tool_names = std::collections::BTreeSet::new();
        let mut tool_fds = std::collections::BTreeSet::new();
        for tool in &self.sealed_tools {
            tool.validate()?;
            if !tool_names.insert(&tool.name) || !tool_fds.insert(tool.fd) {
                return Err(Error::invalid("Duplicate sealed sandbox tool mount"));
            }
        }
        let mut environment_names = std::collections::BTreeSet::new();
        for (name, value) in &self.environment {
            if !name.starts_with("SEMWRIGHT_")
                || name.len() > 64
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
                || value.len() > 4096
                || value.contains('\0')
                || !environment_names.insert(name)
            {
                return Err(Error::invalid("Sandbox environment entry is invalid"));
            }
        }
        if matches!(self.kind, SandboxKind::Driver | SandboxKind::ExternalMcp)
            && self.limits.is_none()
        {
            return Err(Error::invalid(
                "Driver and external MCP resource limits are required",
            ));
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
