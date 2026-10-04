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
/// Internal host-only marker for a short-lived sealed tool child. Platform launchers may
/// consume this for compatibility policy, but must not forward it into the child environment.
pub const SANDBOX_HOST_TOOL_CHILD_ENV: &str = "SEMWRIGHT_HOST_TOOL_CHILD";
/// Internal host-only absolute working directory for a Host-mediated tool child.
/// It is derived from an already-authorized logical mount and never forwarded to the child.
pub const SANDBOX_HOST_TOOL_CWD_ENV: &str = "SEMWRIGHT_HOST_TOOL_CWD";
/// Internal marker enabling v7 typed argument resolution inside the platform launcher.
/// The marker and encoded placeholders are Host-generated and never forwarded to the child.
pub const SANDBOX_HOST_TOOL_TYPED_ARGS_ENV: &str = "SEMWRIGHT_HOST_TOOL_TYPED_ARGS";
pub const HOST_TOOL_ARG_PREFIX: &str = "__SEMWRIGHT_INTERNAL_TOOL_ARG_V1__";
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

/// Host-only v7 reference embedded temporarily in SandboxSpec.args.
/// Platform launchers resolve it after their mount/tool materialization is known.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HostToolArgRef {
    MountPath {
        mount: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        relative: String,
    },
    ToolPath {
        tool: String,
    },
}

impl HostToolArgRef {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::MountPath { mount, relative } => {
                super::filesystem::validate_relative_path(Path::new(mount))?;
                if mount.len() > 255 || !relative.is_empty() {
                    return Err(Error::new(
                        ErrorCode::Unsupported,
                        "Host runtime-tool mount paths are root-only until portable handle-relative resolution exists",
                    ));
                }
            }
            Self::ToolPath { tool } if valid_tool_name(tool) => {}
            Self::ToolPath { .. } => {
                return Err(Error::invalid(
                    "Invalid Host runtime-tool dependency argument",
                ));
            }
        }
        Ok(())
    }
}

pub fn encode_host_tool_arg_ref(value: &HostToolArgRef) -> Result<String> {
    value.validate()?;
    let encoded = serde_json::to_string(value)?;
    let token = format!("{HOST_TOOL_ARG_PREFIX}{encoded}");
    if token.len() > 4096 {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Host runtime-tool path argument exceeds sandbox argument budget",
        ));
    }
    Ok(token)
}

pub fn decode_host_tool_arg_ref(value: &str) -> Result<Option<HostToolArgRef>> {
    let Some(encoded) = value.strip_prefix(HOST_TOOL_ARG_PREFIX) else {
        return Ok(None);
    };
    let decoded: HostToolArgRef = serde_json::from_str(encoded)?;
    decoded.validate()?;
    Ok(Some(decoded))
}

/// Resolve Host-generated v7 path placeholders against paths already materialized for a child.
///
/// When typed mode is false, the reserved prefix is rejected so a legacy/raw caller can never
/// smuggle an internal placeholder into a platform launcher. When typed mode is true, every
/// referenced mount/tool must exist in the exact materialization selected for this child.
pub fn resolve_host_tool_args(
    args: &[String],
    mounts: &[MaterializedMount],
    tools: &[MaterializedTool],
    typed: bool,
) -> Result<Vec<String>> {
    for mount in mounts {
        mount.validate()?;
    }
    for tool in tools {
        tool.validate()?;
    }

    let mut resolved = Vec::with_capacity(args.len());
    for argument in args {
        let reference = decode_host_tool_arg_ref(argument)?;
        if !typed {
            if reference.is_some() || argument.starts_with(HOST_TOOL_ARG_PREFIX) {
                return Err(Error::new(
                    ErrorCode::PolicyDenied,
                    "Typed Host-tool argument requires the v7 internal marker",
                ));
            }
            resolved.push(argument.clone());
            continue;
        }

        match reference {
            None => resolved.push(argument.clone()),
            Some(HostToolArgRef::MountPath { mount, relative }) => {
                let materialized = mounts
                    .iter()
                    .find(|candidate| {
                        candidate.class == MountClass::Workspace && candidate.logical_name == mount
                    })
                    .ok_or_else(|| {
                        Error::new(
                            ErrorCode::PolicyDenied,
                            "Runtime-tool argument references an unmaterialized workspace mount",
                        )
                    })?;
                let mut path = PathBuf::from(&materialized.path);
                if !relative.is_empty() {
                    path.push(relative);
                }
                let value = path.to_str().ok_or_else(|| {
                    Error::invalid("Materialized runtime-tool mount path must be Unicode")
                })?;
                if value.len() > 4096 || value.contains('\0') {
                    return Err(Error::new(
                        ErrorCode::ResourceExhausted,
                        "Resolved runtime-tool mount path exceeds argument bounds",
                    ));
                }
                resolved.push(value.to_owned());
            }
            Some(HostToolArgRef::ToolPath { tool }) => {
                let materialized = tools
                    .iter()
                    .find(|candidate| candidate.name == tool)
                    .ok_or_else(|| {
                        Error::new(
                            ErrorCode::PolicyDenied,
                            "Runtime-tool argument references an unmaterialized tool dependency",
                        )
                    })?;
                resolved.push(materialized.path.clone());
            }
        }
    }
    Ok(resolved)
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
/// Fixed compute nodes only. The optional UVM tools node is never replaced by a broader /dev grant.
/// Fixed executable identity inside the Host-sealed NVIDIA Blender child.
pub const NVIDIA_BLENDER_EXECUTABLE: &str = "/plugin/tools/blender";
pub const NVIDIA_COMPUTE_DEVICE_PATHS: [&str; 4] = [
    "/dev/nvidia0",
    "/dev/nvidiactl",
    "/dev/nvidia-uvm",
    "/dev/nvidia-uvm-tools",
];
pub const NVIDIA_GPU_ENV: &str = "SEMWRIGHT_NVIDIA_GPU";
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
    /// Host-owned Linux NVIDIA compute grant. Application runner children only.
    pub nvidia_gpu: bool,
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
        let host_tool_child = self
            .environment
            .iter()
            .find(|(name, _)| name == SANDBOX_HOST_TOOL_CHILD_ENV)
            .map(|(_, value)| value.as_str());
        if self.nvidia_gpu {
            if !cfg!(target_os = "linux") {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "NVIDIA compute is supported only by the Linux sandbox",
                ));
            }
            if self.kind != SandboxKind::Driver || host_tool_child != Some("1") || self.network {
                return Err(Error::new(
                    ErrorCode::PolicyDenied,
                    "NVIDIA compute requires a network-isolated Host-tool child",
                ));
            }
        }
        if let Some(limits) = &self.limits {
            let maximum = if self.nvidia_gpu {
                8_589_934_592
            } else {
                4_294_967_296
            };
            if limits.address_space_bytes > maximum {
                return Err(Error::new(
                    ErrorCode::PolicyDenied,
                    "Address space above 4 GiB requires the bounded GPU Host-tool grant",
                ));
            }
        }
        if self
            .environment
            .iter()
            .any(|(name, _)| name == NVIDIA_GPU_ENV)
        {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "NVIDIA compute environment marker is platform-owned",
            ));
        }
        let has_host_tool_cwd = environment_names
            .iter()
            .any(|name| name.as_str() == SANDBOX_HOST_TOOL_CWD_ENV);
        let typed_args = self
            .environment
            .iter()
            .find(|(name, _)| name == SANDBOX_HOST_TOOL_TYPED_ARGS_ENV)
            .map(|(_, value)| value.as_str());
        if host_tool_child.is_some_and(|value| value != "1")
            || typed_args.is_some_and(|value| value != "1")
        {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Host-tool internal markers must use the canonical value",
            ));
        }
        if has_host_tool_cwd && host_tool_child != Some("1") {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Host-tool working directory marker requires a Host-tool child",
            ));
        }
        if typed_args.is_some() && host_tool_child != Some("1") {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Typed Host-tool arguments require a Host-tool child",
            ));
        }
        if typed_args.is_none()
            && self
                .args
                .iter()
                .any(|argument| argument.starts_with(HOST_TOOL_ARG_PREFIX))
        {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Reserved Host-tool argument prefix requires typed argument mode",
            ));
        }
        if typed_args.is_some() {
            for argument in &self.args {
                let _ = decode_host_tool_arg_ref(argument)?;
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
    // Dropped after native lifecycle control, retaining a platform launcher thread when needed.
    _launcher_guard: Option<Box<dyn Send>>,
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
            _launcher_guard: None,
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
            _launcher_guard: None,
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
            _launcher_guard: None,
        })
    }

    /// Retain a launcher lifetime resource until this exact sandbox process is dropped.
    /// This carries no policy authority and leaves native kill/reap operations intact.
    pub fn with_launcher_guard(mut self, guard: impl Send + 'static) -> Self {
        self._launcher_guard = Some(Box::new(guard));
        self
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
    fn typed_host_tool_args_resolve_only_against_selected_materialization() {
        #[cfg(unix)]
        let mount_path = "/workspace/project";
        #[cfg(windows)]
        let mount_path = r"C:\Sandbox\project";
        #[cfg(unix)]
        let tool_path = "/plugin/tools/helper";
        #[cfg(windows)]
        let tool_path = r"C:\Sandbox\tools\helper.exe";

        let mounts = vec![materialized("project", mount_path)];
        let tools = vec![MaterializedTool {
            name: "helper".into(),
            path: tool_path.into(),
        }];
        let args = vec![
            "--project".into(),
            encode_host_tool_arg_ref(&HostToolArgRef::MountPath {
                mount: "project".into(),
                relative: String::new(),
            })
            .unwrap(),
            encode_host_tool_arg_ref(&HostToolArgRef::ToolPath {
                tool: "helper".into(),
            })
            .unwrap(),
        ];

        assert!(resolve_host_tool_args(&args, &mounts, &tools, false).is_err());
        let resolved = resolve_host_tool_args(&args, &mounts, &tools, true).unwrap();
        assert_eq!(resolved[0], "--project");
        assert_eq!(PathBuf::from(&resolved[1]), PathBuf::from(mount_path));
        assert_eq!(resolved[2], tool_path);

        let missing_mount = vec![
            encode_host_tool_arg_ref(&HostToolArgRef::MountPath {
                mount: "missing".into(),
                relative: String::new(),
            })
            .unwrap(),
        ];
        assert!(resolve_host_tool_args(&missing_mount, &mounts, &tools, true).is_err());

        let missing_tool = vec![
            encode_host_tool_arg_ref(&HostToolArgRef::ToolPath {
                tool: "missing".into(),
            })
            .unwrap(),
        ];
        assert!(resolve_host_tool_args(&missing_tool, &mounts, &tools, true).is_err());
    }

    #[test]
    fn host_tool_cwd_marker_requires_host_tool_child_marker() {
        #[cfg(windows)]
        let executable = PathBuf::from(r"C:\Semwright\driver.exe");
        #[cfg(not(windows))]
        let executable = PathBuf::from("/tmp/driver");
        #[cfg(windows)]
        let helper = PathBuf::from(r"C:\Semwright\sandbox.exe");
        #[cfg(not(windows))]
        let helper = PathBuf::from("/tmp/sandbox");

        let base = SandboxSpec {
            kind: SandboxKind::Driver,
            staged_executable: executable,
            helper,
            mounts: vec![],
            args: vec![],
            environment: vec![(SANDBOX_HOST_TOOL_CWD_ENV.into(), "host-only".into())],
            sealed_tools: vec![],
            network: false,
            nvidia_gpu: false,
            limits: Some(ResourceLimits {
                open_files: 32,
                processes: 8,
                cpu_seconds: 5,
                address_space_bytes: 134_217_728,
                file_size_bytes: 1_048_576,
            }),
        };
        assert!(base.validate().is_err());

        let mut valid = base;
        valid
            .environment
            .push((SANDBOX_HOST_TOOL_CHILD_ENV.into(), "1".into()));
        assert!(valid.validate().is_ok());
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

    #[test]
    fn nvidia_compute_requires_an_isolated_host_application_child() {
        let mut spec = SandboxSpec {
            kind: SandboxKind::Driver,
            staged_executable: PathBuf::from(if cfg!(windows) {
                r"C:\Semwright\runner.exe"
            } else {
                "/tmp/runner"
            }),
            helper: PathBuf::from(if cfg!(windows) {
                r"C:\Semwright\helper.exe"
            } else {
                "/tmp/helper"
            }),
            mounts: vec![],
            args: vec![],
            environment: vec![],
            sealed_tools: vec![],
            network: false,
            nvidia_gpu: true,
            limits: Some(ResourceLimits {
                open_files: 128,
                processes: 32,
                cpu_seconds: 300,
                address_space_bytes: 4_294_967_296,
                file_size_bytes: 16_777_216,
            }),
        };
        assert!(spec.validate().is_err());
        spec.environment
            .push((SANDBOX_HOST_TOOL_CHILD_ENV.into(), "1".into()));
        #[cfg(target_os = "linux")]
        {
            assert!(spec.validate().is_ok());
            spec.limits.as_mut().unwrap().address_space_bytes = 8_589_934_592;
            assert!(spec.validate().is_ok());
            spec.nvidia_gpu = false;
            assert!(spec.validate().is_err());
            spec.nvidia_gpu = true;
            spec.limits.as_mut().unwrap().address_space_bytes += 1;
            assert!(spec.validate().is_err());
            spec.limits.as_mut().unwrap().address_space_bytes = 4_294_967_296;
        }
        #[cfg(not(target_os = "linux"))]
        assert_eq!(spec.validate().unwrap_err().code, ErrorCode::Unsupported);
        spec.network = true;
        assert!(spec.validate().is_err());
        spec.network = false;
        spec.kind = SandboxKind::Plugin;
        assert!(spec.validate().is_err());
        spec.nvidia_gpu = false;
        spec.kind = SandboxKind::Driver;
        spec.environment.push((NVIDIA_GPU_ENV.into(), "1".into()));
        assert_eq!(spec.validate().unwrap_err().code, ErrorCode::PolicyDenied);
    }
}
