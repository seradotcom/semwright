//! Ownership, digest, format and isolation are separate checks. No signature grants a sandbox.
use async_trait::async_trait;
use semwright_types::{Error, ErrorCode, Result};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    process::{Child, Command},
};

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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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

pub const SANDBOX_MOUNTS_ENV: &str = "SEMWRIGHT_SANDBOX_MOUNTS_V1";
const MAX_MATERIALIZED_MOUNTS: usize = 32;
const MAX_MOUNT_ENV_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterializedMount {
    pub class: MountClass,
    pub logical_name: String,
    /// Absolute path as seen by the sandboxed child, not the host-side grant source.
    pub path: String,
    pub read_only: bool,
}

impl MaterializedMount {
    pub fn validate(&self) -> Result<()> {
        super::filesystem::validate_relative_path(Path::new(&self.logical_name))?;
        if self.logical_name.len() > 255
            || self.path.len() > 4096
            || self.path.contains('\0')
            || !Path::new(&self.path).is_absolute()
        {
            return Err(Error::invalid("Invalid materialized sandbox mount"));
        }
        if matches!(self.class, MountClass::SystemConfig | MountClass::Secret) && !self.read_only {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Materialized system-config and secret mounts must be read-only",
            ));
        }
        if self.class == MountClass::Secret
            && (Path::new(&self.logical_name).components().count() != 1
                || self.logical_name.len() > 64)
        {
            return Err(Error::invalid(
                "Materialized secret names must be one bounded path component",
            ));
        }
        Ok(())
    }
}

pub fn encode_materialized_mounts(mounts: &[MaterializedMount]) -> Result<String> {
    if mounts.len() > MAX_MATERIALIZED_MOUNTS {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Materialized sandbox mount count exceeds budget",
        ));
    }
    let mut unique = std::collections::BTreeSet::new();
    for mount in mounts {
        mount.validate()?;
        if !unique.insert((mount.class as u8, mount.logical_name.clone())) {
            return Err(Error::invalid("Duplicate materialized sandbox mount"));
        }
    }
    let encoded = serde_json::to_string(mounts)?;
    if encoded.len() > MAX_MOUNT_ENV_BYTES {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Materialized sandbox mount table exceeds environment budget",
        ));
    }
    Ok(encoded)
}

pub fn decode_materialized_mounts(encoded: &str) -> Result<Vec<MaterializedMount>> {
    if encoded.len() > MAX_MOUNT_ENV_BYTES {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Materialized sandbox mount table exceeds environment budget",
        ));
    }
    let mounts: Vec<MaterializedMount> = serde_json::from_str(encoded)?;
    encode_materialized_mounts(&mounts)?;
    Ok(mounts)
}

fn valid_tool_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with("semwright-internal-")
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

#[derive(Clone, Debug)]
pub enum SealedToolSource {
    /// Linux/Bubblewrap immutable descriptor materialization.
    UnixFd(i32),
    /// Host-staged executable that the platform must re-verify before materializing.
    VerifiedFile { path: PathBuf, sha256: String },
}
impl SealedToolSource {
    fn validate(&self) -> Result<()> {
        match self {
            Self::UnixFd(fd) if *fd >= 3 => Ok(()),
            Self::VerifiedFile { path, sha256 }
                if path.is_absolute()
                    && sha256.len() == 64
                    && sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) =>
            {
                Ok(())
            }
            _ => Err(Error::invalid("Invalid sealed sandbox tool source")),
        }
    }
}

#[derive(Clone, Debug)]
pub struct SealedToolMount {
    pub source: SealedToolSource,
    pub name: String,
}
impl SealedToolMount {
    pub fn validate(&self) -> Result<()> {
        self.source.validate()?;
        if !valid_tool_name(&self.name) {
            return Err(Error::invalid("Invalid sealed sandbox tool mount"));
        }
        Ok(())
    }
}

pub const SANDBOX_TOOLS_ENV: &str = "SEMWRIGHT_SANDBOX_TOOLS_V1";
const MAX_TOOL_ENV_BYTES: usize = 8 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterializedTool {
    pub name: String,
    pub path: String,
}
impl MaterializedTool {
    pub fn validate(&self) -> Result<()> {
        if !valid_tool_name(&self.name)
            || self.path.is_empty()
            || self.path.len() > 4096
            || self.path.contains('\0')
            || !Path::new(&self.path).is_absolute()
        {
            return Err(Error::invalid("Invalid materialized sandbox tool"));
        }
        Ok(())
    }
}

pub fn encode_materialized_tools(tools: &[MaterializedTool]) -> Result<String> {
    if tools.len() > 8 {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Materialized sandbox tool count exceeds budget",
        ));
    }
    let mut names = std::collections::BTreeSet::new();
    for tool in tools {
        tool.validate()?;
        if !names.insert(&tool.name) {
            return Err(Error::invalid("Duplicate materialized sandbox tool"));
        }
    }
    let encoded = serde_json::to_string(tools)?;
    if encoded.len() > MAX_TOOL_ENV_BYTES {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Materialized sandbox tool table exceeds environment budget",
        ));
    }
    Ok(encoded)
}

pub fn decode_materialized_tools(encoded: &str) -> Result<Vec<MaterializedTool>> {
    if encoded.len() > MAX_TOOL_ENV_BYTES {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Materialized sandbox tool table exceeds environment budget",
        ));
    }
    let tools: Vec<MaterializedTool> = serde_json::from_str(encoded)?;
    encode_materialized_tools(&tools)?;
    Ok(tools)
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
        let mut tool_files = std::collections::BTreeSet::new();
        for tool in &self.sealed_tools {
            tool.validate()?;
            if !tool_names.insert(&tool.name) {
                return Err(Error::invalid("Duplicate sealed sandbox tool mount"));
            }
            let unique = match &tool.source {
                SealedToolSource::UnixFd(fd) => tool_fds.insert(*fd),
                SealedToolSource::VerifiedFile { path, .. } => tool_files.insert(path.clone()),
            };
            if !unique {
                return Err(Error::invalid("Duplicate sealed sandbox tool source"));
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

pub type SandboxStdin = Box<dyn AsyncWrite + Send + Unpin>;
pub type SandboxStdout = Box<dyn AsyncRead + Send + Unpin>;

/// Monotonic cumulative CPU accounting for every process inside one sandbox authority boundary.
///
/// Implementations must include CPU consumed by descendants that have already exited so a child
/// cannot evade an operation budget by rapidly spawning and reaping workers.
pub trait SandboxCpuAccounting: Send + Sync {
    fn total_cpu_time(&self) -> Result<Duration>;
}

#[async_trait]
pub trait SandboxChildControl: Send {
    fn id(&self) -> Option<u32>;
    async fn kill(&mut self) -> Result<()>;
    async fn wait(&mut self) -> Result<()>;
    fn exit_code(&self) -> Option<i32> {
        None
    }
}

struct TokioSandboxChild {
    child: Child,
    exit_code: Option<i32>,
}

#[async_trait]
impl SandboxChildControl for TokioSandboxChild {
    fn id(&self) -> Option<u32> {
        self.child.id()
    }

    async fn kill(&mut self) -> Result<()> {
        self.child.kill().await.map_err(Into::into)
    }

    async fn wait(&mut self) -> Result<()> {
        let status = self.child.wait().await?;
        self.exit_code = status.code();
        Ok(())
    }

    fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }
}

/// A child process whose isolation was established by the platform before this value is returned.
///
/// The broker-side hosts receive only bounded stdio plus lifecycle operations. Native process
/// handles, tokens, Job Objects and sandbox authorities remain owned by the platform backend.
pub struct SandboxProcess {
    stdin: Option<SandboxStdin>,
    stdout: Option<SandboxStdout>,
    control: Box<dyn SandboxChildControl>,
    cpu_accounting: Option<Arc<dyn SandboxCpuAccounting>>,
}

impl SandboxProcess {
    /// Construct a sandboxed process from platform-owned stdio and lifecycle control.
    ///
    /// Native process handles, tokens, jobs and sandbox authorities remain encapsulated by
    /// the platform implementation behind `SandboxChildControl`.
    pub fn from_parts(
        stdin: SandboxStdin,
        stdout: SandboxStdout,
        control: Box<dyn SandboxChildControl>,
    ) -> Self {
        Self {
            stdin: Some(stdin),
            stdout: Some(stdout),
            control,
            cpu_accounting: None,
        }
    }

    pub fn from_parts_with_cpu_accounting(
        stdin: SandboxStdin,
        stdout: SandboxStdout,
        control: Box<dyn SandboxChildControl>,
        cpu_accounting: Arc<dyn SandboxCpuAccounting>,
    ) -> Self {
        Self {
            stdin: Some(stdin),
            stdout: Some(stdout),
            control,
            cpu_accounting: Some(cpu_accounting),
        }
    }

    pub fn from_tokio_child(mut child: Child) -> Result<Self> {
        let stdin = child
            .stdin
            .take()
            .map(|value| Box::new(value) as SandboxStdin);
        let stdout = child
            .stdout
            .take()
            .map(|value| Box::new(value) as SandboxStdout);
        let (Some(stdin), Some(stdout)) = (stdin, stdout) else {
            let _ = child.start_kill();
            return Err(Error::new(
                ErrorCode::ProtocolMismatch,
                "Sandbox child must expose piped stdin and stdout",
            ));
        };
        Ok(Self {
            stdin: Some(stdin),
            stdout: Some(stdout),
            control: Box::new(TokioSandboxChild {
                child,
                exit_code: None,
            }),
            cpu_accounting: None,
        })
    }

    pub fn cpu_accounting(&self) -> Option<Arc<dyn SandboxCpuAccounting>> {
        self.cpu_accounting.clone()
    }

    pub fn take_stdin(&mut self) -> Result<SandboxStdin> {
        self.stdin.take().ok_or_else(|| {
            Error::new(
                ErrorCode::ProtocolMismatch,
                "Sandbox child stdin was already taken",
            )
        })
    }

    pub fn take_stdout(&mut self) -> Result<SandboxStdout> {
        self.stdout.take().ok_or_else(|| {
            Error::new(
                ErrorCode::ProtocolMismatch,
                "Sandbox child stdout was already taken",
            )
        })
    }

    pub fn id(&self) -> Option<u32> {
        self.control.id()
    }

    pub async fn kill(&mut self) -> Result<()> {
        self.control.kill().await
    }

    pub async fn wait(&mut self) -> Result<()> {
        self.control.wait().await
    }

    pub async fn wait_exit_code(&mut self) -> Result<Option<i32>> {
        self.control.wait().await?;
        Ok(self.control.exit_code())
    }
}

pub trait SandboxLauncher: Send + Sync {
    fn command(&self, spec: &SandboxSpec) -> Result<Command>;

    /// Spawn a process only after the platform-specific containment boundary is established.
    ///
    /// Linux keeps using the existing Bubblewrap + Landlock command path through this default.
    /// Platforms that require pre-first-instruction setup (for example Windows) override this
    /// method and must not fall back to an ordinary direct process spawn.
    fn spawn(&self, spec: &SandboxSpec) -> Result<SandboxProcess> {
        let mut command = self.command(spec)?;
        let child = command.spawn().map_err(|_| {
            Error::new(
                ErrorCode::SandboxDenied,
                "Platform sandbox process failed to start",
            )
        })?;
        SandboxProcess::from_tokio_child(child)
    }

    fn available(&self, helper: &Path) -> bool;
    fn mechanism(&self) -> &'static str;
    fn diagnostics(&self, helper: &Path) -> serde_json::Value {
        serde_json::json!({"available":self.available(helper),"helper_present":helper.is_file(),"mechanism":self.mechanism()})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn materialized(name: &str, path: &str) -> MaterializedMount {
        MaterializedMount {
            class: MountClass::Workspace,
            logical_name: name.into(),
            path: path.into(),
            read_only: true,
        }
    }

    #[test]
    fn materialized_mount_table_roundtrips() {
        #[cfg(unix)]
        let path = "/workspace/media";
        #[cfg(windows)]
        let path = r"C:\Users\owner\media";
        let mounts = vec![materialized("media", path)];
        let encoded = encode_materialized_mounts(&mounts).unwrap();
        assert_eq!(decode_materialized_mounts(&encoded).unwrap(), mounts);
    }

    #[test]
    fn materialized_mount_table_rejects_duplicates_and_relative_paths() {
        #[cfg(unix)]
        let absolute = "/workspace/media";
        #[cfg(windows)]
        let absolute = r"C:\Users\owner\media";
        let one = materialized("media", absolute);
        assert!(encode_materialized_mounts(&[one.clone(), one]).is_err());
        assert!(encode_materialized_mounts(&[materialized("media", "relative/path")]).is_err());
    }

    #[test]
    fn materialized_mount_table_is_bounded() {
        #[cfg(unix)]
        let path = "/workspace/media";
        #[cfg(windows)]
        let path = r"C:\Users\owner\media";
        let mounts = (0..=MAX_MATERIALIZED_MOUNTS)
            .map(|index| materialized(&format!("mount-{index}"), path))
            .collect::<Vec<_>>();
        assert!(encode_materialized_mounts(&mounts).is_err());
    }

    #[test]
    fn materialized_tool_table_roundtrips_and_rejects_duplicates() {
        #[cfg(unix)]
        let path = "/plugin/tools/probe";
        #[cfg(windows)]
        let path = r"C:\Users\owner\AppData\Local\Semwright\tools\probe.exe";
        let tool = MaterializedTool {
            name: "probe".into(),
            path: path.into(),
        };
        let encoded = encode_materialized_tools(std::slice::from_ref(&tool)).unwrap();
        assert_eq!(
            decode_materialized_tools(&encoded).unwrap(),
            vec![tool.clone()]
        );
        assert!(encode_materialized_tools(&[tool.clone(), tool]).is_err());
    }

    #[test]
    fn sealed_tool_sources_are_explicit_and_bounded() {
        assert!(
            SealedToolMount {
                source: SealedToolSource::UnixFd(3),
                name: "probe".into(),
            }
            .validate()
            .is_ok()
        );
        #[cfg(windows)]
        let path = PathBuf::from(r"C:\Semwright\probe.exe");
        #[cfg(not(windows))]
        let path = PathBuf::from("/tmp/probe");
        assert!(
            SealedToolMount {
                source: SealedToolSource::VerifiedFile {
                    path,
                    sha256: "a".repeat(64),
                },
                name: "probe".into(),
            }
            .validate()
            .is_ok()
        );
        assert!(
            SealedToolMount {
                source: SealedToolSource::UnixFd(2),
                name: "probe".into(),
            }
            .validate()
            .is_err()
        );
    }
}
