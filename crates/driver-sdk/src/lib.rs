//! Versioned application-driver contract. Drivers are providers; this crate has no broker or MCP authority.

pub mod continuity;
use async_trait::async_trait;
use semwright_platform_api::launch::{
    MountClass, SANDBOX_MOUNTS_ENV, SANDBOX_TOOLS_ENV, decode_materialized_mounts,
    decode_materialized_tools,
};
use semwright_protocol::{read_frame, write_frame};
use semwright_types::provider::canonical_slug;
use semwright_types::{
    CommandDescriptor, Error, ErrorCode, JobArtifact, JobProgress, NativeTarget, ProviderIdentity,
    Result, SourceKind, unique_id,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path, PathBuf},
    process::Stdio,
    sync::Arc,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::{Mutex, mpsc, oneshot},
};
use tokio_util::sync::CancellationToken;

pub const DRIVER_MANIFEST_VERSION: u32 = 1;
pub const DRIVER_PROTOCOL_MIN_VERSION: u32 = 1;
pub const DRIVER_PROTOCOL_VERSION: u32 = 5;

const MAX_TOOL_ARGS: usize = 32;
const MAX_TOOL_ARG_BYTES: usize = 4 * 1024;
const MAX_TOOL_STDIN_BYTES: usize = 64 * 1024;
const MAX_TOOL_OUTPUT_BYTES: usize = 256 * 1024;
const MAX_TOOL_TIMEOUT_MS: u64 = 30_000;

fn runtime_mount(class: MountClass, logical_name: &str) -> Result<PathBuf> {
    if logical_name.is_empty()
        || logical_name.len() > 255
        || logical_name.chars().any(char::is_control)
    {
        return Err(Error::invalid("Invalid sandbox mount name"));
    }

    match std::env::var(SANDBOX_MOUNTS_ENV) {
        Ok(encoded) => {
            let mounts = decode_materialized_mounts(&encoded)?;
            mounts
                .into_iter()
                .find(|mount| mount.class == class && mount.logical_name == logical_name)
                .map(|mount| PathBuf::from(mount.path))
                .ok_or_else(|| Error::unavailable("Requested sandbox mount was not materialized"))
        }
        Err(std::env::VarError::NotPresent) => {
            #[cfg(unix)]
            {
                let prefix = match class {
                    MountClass::Workspace => "/workspace",
                    MountClass::SystemConfig => "/etc",
                    MountClass::Secret => "/run/secrets",
                };
                Ok(Path::new(prefix).join(logical_name))
            }
            #[cfg(not(unix))]
            {
                let _ = class;
                Err(Error::unavailable(
                    "Sandbox mount table is required on this platform",
                ))
            }
        }
        Err(std::env::VarError::NotUnicode(_)) => Err(Error::invalid(
            "Sandbox mount table must be valid UTF-8 JSON",
        )),
    }
}

/// Resolve an owner-granted workspace root as materialized by the current platform sandbox.
pub fn workspace_mount(logical_name: &str) -> Result<PathBuf> {
    runtime_mount(MountClass::Workspace, logical_name)
}

/// Resolve a read-only system-configuration root as materialized by the current platform sandbox.
pub fn system_config_mount(logical_name: &str) -> Result<PathBuf> {
    runtime_mount(MountClass::SystemConfig, logical_name)
}

fn valid_tool_name(name: &str) -> bool {
    canonical_slug(name) && name.len() <= 64 && !name.starts_with("semwright-internal-")
}

/// Resolve one Host-verified executable tool as materialized by the current platform sandbox.
pub fn tool_path(name: &str) -> Result<PathBuf> {
    if !valid_tool_name(name) {
        return Err(Error::invalid("Invalid sandbox tool name"));
    }
    match std::env::var(SANDBOX_TOOLS_ENV) {
        Ok(encoded) => decode_materialized_tools(&encoded)?
            .into_iter()
            .find(|tool| tool.name == name)
            .map(|tool| PathBuf::from(tool.path))
            .ok_or_else(|| Error::unavailable("Requested sandbox tool was not materialized")),
        Err(std::env::VarError::NotPresent) => {
            #[cfg(unix)]
            {
                Ok(Path::new("/plugin/tools").join(name))
            }
            #[cfg(not(unix))]
            {
                Err(Error::unavailable(
                    "Sandbox tool table is required on this platform",
                ))
            }
        }
        Err(std::env::VarError::NotUnicode(_)) => Err(Error::invalid(
            "Sandbox tool table must be valid UTF-8 JSON",
        )),
    }
}

/// Platform-independent execution strategy for an owner-pinned secondary runtime tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeToolMode {
    /// The Host materialized the verified executable inside the driver sandbox.
    Materialized,
    /// The driver must ask Driver Host to execute the verified tool on its behalf.
    HostMediated,
}

async fn execute_materialized_tool(
    name: &str,
    args: Vec<String>,
    stdin: Vec<u8>,
    timeout: std::time::Duration,
    cwd: Option<&RuntimeToolCwd>,
    cancellation: CancellationToken,
) -> Result<ToolExecutionOutput> {
    let timeout_ms = u64::try_from(timeout.as_millis()).map_err(|_| {
        Error::new(
            ErrorCode::ResourceExhausted,
            "Runtime tool timeout exceeds protocol bounds",
        )
    })?;
    validate_tool_execute_request(name, &args, &stdin, timeout_ms)?;
    let path = tool_path(name)?;
    let working_directory = if let Some(cwd) = cwd {
        cwd.validate()?;
        let mut directory = workspace_mount(&cwd.mount)?;
        if !cwd.relative.is_empty() {
            directory.push(&cwd.relative);
        }
        let metadata = std::fs::metadata(&directory)
            .map_err(|_| Error::unavailable("Runtime tool working directory is unavailable"))?;
        if !metadata.is_dir() {
            return Err(Error::invalid(
                "Runtime tool working directory is not a directory",
            ));
        }
        Some(directory)
    } else {
        None
    };

    let mut command = tokio::process::Command::new(path);
    command
        .args(args)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(directory) = working_directory {
        command.current_dir(directory);
    }
    let mut child = command
        .spawn()
        .map_err(|_| Error::unavailable("Materialized runtime tool could not be launched"))?;
    let mut child_stdin = child
        .stdin
        .take()
        .ok_or_else(|| Error::unavailable("Runtime tool stdin is unavailable"))?;
    let child_stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::unavailable("Runtime tool stdout is unavailable"))?;
    let child_stderr = child
        .stderr
        .take()
        .ok_or_else(|| Error::unavailable("Runtime tool stderr is unavailable"))?;

    let execution = async move {
        if !stdin.is_empty() {
            child_stdin.write_all(&stdin).await?;
        }
        child_stdin.shutdown().await?;
        drop(child_stdin);

        let read_stdout = async move {
            let mut bytes = Vec::new();
            child_stdout
                .take((MAX_TOOL_OUTPUT_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
                .await?;
            Ok::<_, std::io::Error>(bytes)
        };
        let read_stderr = async move {
            let mut bytes = Vec::new();
            child_stderr
                .take((MAX_TOOL_OUTPUT_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
                .await?;
            Ok::<_, std::io::Error>(bytes)
        };
        let (stdout, stderr, status) = tokio::try_join!(read_stdout, read_stderr, child.wait())?;
        if stdout.len() > MAX_TOOL_OUTPUT_BYTES || stderr.len() > MAX_TOOL_OUTPUT_BYTES {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Materialized runtime tool output exceeds protocol bounds",
            ));
        }
        let output = ToolExecutionOutput {
            exit_code: status.code().unwrap_or(-1),
            stdout,
            stderr,
        };
        output.validate()?;
        Ok(output)
    };

    tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err(Error::new(
            ErrorCode::Cancelled,
            "Runtime tool execution cancelled",
        )),
        result = tokio::time::timeout(timeout, execution) => match result {
            Ok(result) => result,
            Err(_) => Err(Error::new(ErrorCode::Timeout, "Runtime tool execution timed out")),
        },
    }
}

/// Resolve an owner-granted secret file as materialized by the current platform sandbox.
pub fn secret_mount(logical_name: &str) -> Result<PathBuf> {
    runtime_mount(MountClass::Secret, logical_name)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    StdioV1,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriverInterfaces {
    #[serde(default)]
    pub dynamic_capabilities: bool,
    #[serde(default)]
    pub cooperative_cancellation: bool,
    #[serde(default)]
    pub events: bool,
    #[serde(default)]
    pub progress: bool,
    #[serde(default)]
    pub artifacts: bool,
    #[serde(default = "default_health")]
    pub health: bool,
    /// Protocol v3: driver can emit and validate provider-owned native references.
    #[serde(default)]
    pub native_refs: bool,
    /// Protocol v4: driver may request Host-mediated execution of owner-pinned sealed tools.
    #[serde(default)]
    pub host_tools: bool,
}
fn default_health() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationMatch {
    pub desktop_id: Option<String>,
    #[serde(default)]
    pub process_names: Vec<String>,
    #[serde(default)]
    pub supported_versions: Vec<String>,
}
impl ApplicationMatch {
    fn validate(&self) -> Result<()> {
        if self.desktop_id.is_none() && self.process_names.is_empty() {
            return Err(Error::invalid(
                "Driver must declare a desktop ID or process-name application match",
            ));
        }
        if self
            .desktop_id
            .as_ref()
            .is_some_and(|v| v.is_empty() || v.len() > 256 || v.chars().any(char::is_control))
            || self.process_names.len() > 16
            || self.supported_versions.len() > 32
            || self
                .process_names
                .iter()
                .chain(&self.supported_versions)
                .any(|v| v.is_empty() || v.len() > 128 || v.chars().any(char::is_control))
        {
            return Err(Error::invalid("Driver application match exceeds bounds"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriverMount {
    pub root: String,
    pub read_only: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub execute: bool,
}
fn is_false(value: &bool) -> bool {
    !*value
}

/// Owner-granted configuration exposed read-only at its canonical system location.
///
/// Protocol v1 deliberately supports only one normal child directly below /etc.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SystemConfigMount {
    pub root: String,
    pub destination: PathBuf,
}
impl SystemConfigMount {
    fn validate(&self) -> Result<()> {
        if !canonical_slug(&self.root)
            || self.destination.as_os_str().len() > 256
            || self.destination.parent() != Some(Path::new("/etc"))
            || self.destination.file_name().is_none()
            || self
                .destination
                .components()
                .any(|component| !matches!(component, Component::RootDir | Component::Normal(_)))
        {
            return Err(Error::invalid(
                "Driver system config mounts must map a canonical grant to one direct /etc child",
            ));
        }
        Ok(())
    }
}

/// Owner-granted executable verified and staged immutably by Driver Host.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriverToolMount {
    pub root: String,
    pub name: String,
    pub sha256: String,
    /// Workspace mounts this tool may receive when Host-mediated.
    /// Empty preserves the v4 zero-mount tool-child contract.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mounts: Vec<String>,
}
impl DriverToolMount {
    fn validate(&self) -> Result<()> {
        let unique_mounts = self.mounts.iter().collect::<BTreeSet<_>>();
        if !canonical_slug(&self.root)
            || self.root.starts_with("semwright-internal-")
            || !valid_tool_name(&self.name)
            || self.sha256.len() != 64
            || !self.sha256.bytes().all(|b| b.is_ascii_hexdigit())
            || self.mounts.len() > 8
            || unique_mounts.len() != self.mounts.len()
            || self
                .mounts
                .iter()
                .any(|mount| !canonical_slug(mount) || mount.starts_with("semwright-internal-"))
        {
            return Err(Error::invalid(
                "Driver tools require canonical grant/name/mounts and SHA-256 digest",
            ));
        }
        Ok(())
    }
}

/// Owner-granted secret file exposed read-only under `/run/secrets/<name>`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriverSecretMount {
    pub root: String,
    pub name: String,
}
impl DriverSecretMount {
    fn validate(&self) -> Result<()> {
        if !canonical_slug(&self.root)
            || self.root.starts_with("semwright-internal-")
            || !canonical_slug(&self.name)
            || self.name.len() > 64
            || self.name.starts_with("semwright-internal-")
        {
            return Err(Error::invalid(
                "Driver secret mounts require canonical owner grant and bounded secret name",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriverResources {
    #[serde(default = "default_open_files")]
    pub open_files: u64,
    #[serde(default = "default_processes")]
    pub processes: u64,
    /// Hard cumulative CPU lifetime cap enforced by the sandbox.
    #[serde(default = "default_cpu_seconds")]
    pub cpu_seconds: u64,
    /// Optional per-operation CPU budget on hosts with bounded process-tree accounting.
    /// Zero preserves the lifetime-only contract.
    #[serde(default)]
    pub operation_cpu_seconds: u64,
    #[serde(default = "default_address_space_bytes")]
    pub address_space_bytes: u64,
    #[serde(default = "default_file_size_bytes")]
    pub file_size_bytes: u64,
}
fn default_open_files() -> u64 {
    128
}
fn default_processes() -> u64 {
    32
}
fn default_cpu_seconds() -> u64 {
    20
}
fn default_address_space_bytes() -> u64 {
    536_870_912
}
fn default_file_size_bytes() -> u64 {
    16_777_216
}
impl Default for DriverResources {
    fn default() -> Self {
        Self {
            open_files: default_open_files(),
            processes: default_processes(),
            cpu_seconds: default_cpu_seconds(),
            operation_cpu_seconds: 0,
            address_space_bytes: default_address_space_bytes(),
            file_size_bytes: default_file_size_bytes(),
        }
    }
}
impl DriverResources {
    fn validate(&self) -> Result<()> {
        if !(32..=1024).contains(&self.open_files)
            || !(8..=256).contains(&self.processes)
            || !(5..=86_400).contains(&self.cpu_seconds)
            || (self.operation_cpu_seconds != 0
                && (!(1..=300).contains(&self.operation_cpu_seconds)
                    || self.operation_cpu_seconds > self.cpu_seconds))
            || (self.cpu_seconds > 300 && self.operation_cpu_seconds == 0)
            || !(134_217_728..=4_294_967_296).contains(&self.address_space_bytes)
            || !(1_048_576..=1_073_741_824).contains(&self.file_size_bytes)
        {
            return Err(Error::invalid(
                "Driver resource request exceeds sandbox bounds",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub manifest_version: u32,
    pub protocol: u32,
    pub id: String,
    pub version: String,
    pub publisher: String,
    pub executable: PathBuf,
    pub sha256: String,
    pub application: ApplicationMatch,
    pub transport: Transport,
    #[serde(default)]
    pub mounts: Vec<DriverMount>,
    #[serde(default)]
    pub system_config: Vec<SystemConfigMount>,
    #[serde(default)]
    pub secrets: Vec<DriverSecretMount>,
    #[serde(default)]
    pub tools: Vec<DriverToolMount>,
    #[serde(default)]
    pub network: bool,
    /// Optional owner-selected TCP port exposed through a Host-managed loopback proxy.
    /// This does not grant the driver a network namespace.
    #[serde(default)]
    pub loopback_port: Option<u16>,
    #[serde(default)]
    pub resources: DriverResources,
    #[serde(default = "default_timeout")]
    pub request_timeout_ms: u64,
    #[serde(default)]
    pub interfaces: DriverInterfaces,
}
fn default_timeout() -> u64 {
    30_000
}
impl Manifest {
    pub fn identity(&self) -> Result<ProviderIdentity> {
        let mut identity = ProviderIdentity::external(SourceKind::Driver, &self.id, &self.version)?;
        identity.application = self.application.desktop_id.clone();
        identity.origin = format!("driver-manifest:{}", self.publisher);
        identity.validate_external()?;
        Ok(identity)
    }
    pub fn validate(&self) -> Result<()> {
        if self.manifest_version != DRIVER_MANIFEST_VERSION
            || !(DRIVER_PROTOCOL_MIN_VERSION..=DRIVER_PROTOCOL_VERSION).contains(&self.protocol)
        {
            return Err(Error::new(
                ErrorCode::ProtocolMismatch,
                "Unsupported driver manifest or protocol version",
            ));
        }
        self.identity()?;
        self.application.validate()?;
        self.resources.validate()?;
        if self.network && self.loopback_port.is_some() {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Driver cannot request ambient network and loopback-only authority together",
            ));
        }
        if self
            .loopback_port
            .is_some_and(|port| !(1024..=u16::MAX).contains(&port))
        {
            return Err(Error::invalid(
                "Driver loopback port must be an unprivileged TCP port",
            ));
        }
        if self.protocol == 1
            && (self.interfaces.dynamic_capabilities
                || self.interfaces.cooperative_cancellation
                || self.interfaces.events
                || self.interfaces.progress
                || self.interfaces.artifacts)
        {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "Driver protocol v1 cannot negotiate dynamic/events/progress/artifacts/cancellation",
            ));
        }
        if self.protocol < 3 && self.interfaces.native_refs {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "Driver native-reference validation requires protocol v3",
            ));
        }
        if self.protocol < 4 && self.interfaces.host_tools {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "Host-mediated sealed tools require driver protocol v4",
            ));
        }
        if self.interfaces.host_tools && self.tools.is_empty() {
            return Err(Error::invalid(
                "Host-mediated sealed tools require at least one owner-pinned tool",
            ));
        }
        if self.publisher.is_empty()
            || self.publisher.len() > 128
            || self.publisher.chars().any(char::is_control)
            || !self.executable.is_absolute()
            || self.sha256.len() != 64
            || !self.sha256.bytes().all(|b| b.is_ascii_hexdigit())
            || self.mounts.len() > 16
            || self.system_config.len() > 8
            || self.secrets.len() > 8
            || self.tools.len() > 8
            || self.request_timeout_ms == 0
            || self.request_timeout_ms > 300_000
        {
            return Err(Error::invalid("Driver manifest exceeds bounds"));
        }
        let mut roots = BTreeSet::new();
        for mount in &self.mounts {
            if mount.root.starts_with("semwright-internal-")
                || !canonical_slug(&mount.root)
                || !roots.insert(&mount.root)
            {
                return Err(Error::invalid(
                    "Driver mount roots must be unique canonical policy-grant names",
                ));
            }
            if mount.execute && !mount.read_only {
                return Err(Error::invalid("Executable driver mounts must be read-only"));
            }
        }
        let mut destinations = BTreeSet::new();
        for mount in &self.system_config {
            mount.validate()?;
            if mount.root.starts_with("semwright-internal-")
                || !roots.insert(&mount.root)
                || !destinations.insert(&mount.destination)
            {
                return Err(Error::invalid(
                    "Driver system config roots and destinations must be unique",
                ));
            }
        }
        let mut secret_names = BTreeSet::new();
        for secret in &self.secrets {
            secret.validate()?;
            if !roots.insert(&secret.root) || !secret_names.insert(&secret.name) {
                return Err(Error::invalid(
                    "Driver secret roots and names must be unique and non-overlapping",
                ));
            }
        }
        let workspace_roots = self
            .mounts
            .iter()
            .map(|mount| mount.root.as_str())
            .collect::<BTreeSet<_>>();
        let mut tool_names = BTreeSet::new();
        for tool in &self.tools {
            tool.validate()?;
            if !roots.insert(&tool.root) || !tool_names.insert(&tool.name) {
                return Err(Error::invalid(
                    "Driver tool roots and names must be unique and non-overlapping",
                ));
            }
            if tool
                .mounts
                .iter()
                .any(|mount| !workspace_roots.contains(mount.as_str()))
            {
                return Err(Error::new(
                    ErrorCode::PolicyDenied,
                    "Driver tool may only receive declared workspace mounts",
                ));
            }
            if !tool.mounts.is_empty() && (self.protocol < 5 || !self.interfaces.host_tools) {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "Per-tool workspace mounts require Driver Protocol v5 Host mediation",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capability {
    pub descriptor: CommandDescriptor,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub object_types: Vec<String>,
}
fn valid_artifact_kind(kind: &str) -> bool {
    if kind.is_empty() || kind.len() > 96 {
        return false;
    }
    let mut parts = kind.split('/');
    let Some(category) = parts.next() else {
        return false;
    };
    let Some(subtype) = parts.next() else {
        return false;
    };
    if parts.next().is_some() {
        return false;
    }
    let valid_part = |part: &str| {
        !part.is_empty()
            && part.len() <= 48
            && part
                .bytes()
                .next()
                .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            && part.bytes().all(|b| {
                b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'+' | b'-')
            })
    };
    valid_part(category) && valid_part(subtype)
}

fn artifact_port_tag(prefix: &str, kind: &str) -> Result<String> {
    if !valid_artifact_kind(kind) {
        return Err(Error::invalid(
            "Artifact semantic type must be a bounded lowercase category/subtype",
        ));
    }
    Ok(format!("{prefix}{kind}"))
}

pub fn artifact_input_tag(kind: &str) -> Result<String> {
    artifact_port_tag("artifact-in:", kind)
}

pub fn artifact_output_tag(kind: &str) -> Result<String> {
    artifact_port_tag("artifact-out:", kind)
}

fn validate_artifact_port_tag(tag: &str) -> Result<()> {
    if let Some(kind) = tag
        .strip_prefix("artifact-in:")
        .or_else(|| tag.strip_prefix("artifact-out:"))
        && !valid_artifact_kind(kind)
    {
        return Err(Error::invalid("Invalid artifact capability tag"));
    }
    Ok(())
}

impl Capability {
    pub fn validate_for(&self, identity: &ProviderIdentity) -> Result<()> {
        let command = &self.descriptor;
        if !command.name.starts_with(&identity.namespace)
            || command.backends != [identity.id.clone()]
            || !command.requires.contains(&identity.id)
        {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Driver capability must remain inside its owner-assigned namespace and scope",
            ));
        }
        if [&self.aliases, &self.tags, &self.object_types]
            .iter()
            .any(|v| {
                v.len() > 32
                    || v.iter()
                        .any(|s| s.is_empty() || s.len() > 128 || s.chars().any(char::is_control))
            })
        {
            return Err(Error::invalid("Driver capability metadata exceeds bounds"));
        }
        for tag in &self.tags {
            validate_artifact_port_tag(tag)?;
        }
        Ok(())
    }
}

pub fn descriptor_digest(descriptor: &CommandDescriptor) -> Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(descriptor)?)
    ))
}

pub fn capabilities_digest(capabilities: &[Capability]) -> Result<String> {
    let bytes = serde_json::to_vec(capabilities)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriverRequestContext {
    pub session: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_target: Option<NativeTarget>,
}
impl DriverRequestContext {
    fn validate(&self) -> Result<()> {
        if self.session.is_empty()
            || self.session.len() > 256
            || self.session.chars().any(char::is_control)
        {
            return Err(Error::invalid("Driver request session exceeds bounds"));
        }
        if let Some(target) = &self.native_target {
            validate_native_target(target)?;
        }
        Ok(())
    }
}

fn validate_native_target(target: &NativeTarget) -> Result<()> {
    if target.kind != "native"
        || target.identity.is_empty()
        || target.identity.len() > 8192
        || target.fingerprint.len() > 256
        || target.app.len() > 256
        || target.identity.chars().any(char::is_control)
        || target.fingerprint.chars().any(char::is_control)
        || target.app.chars().any(char::is_control)
    {
        return Err(Error::invalid(
            "Driver native target exceeds bounded contract",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolExecutionOutput {
    pub exit_code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl ToolExecutionOutput {
    pub fn validate(&self) -> Result<()> {
        if self.stdout.len() > MAX_TOOL_OUTPUT_BYTES || self.stderr.len() > MAX_TOOL_OUTPUT_BYTES {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Host-mediated tool output exceeds protocol bounds",
            ));
        }
        Ok(())
    }
}

/// Working directory for a runtime-tool invocation, expressed only as the root
/// of an already-authorized workspace mount. Nested paths are intentionally deferred
/// until platform hosts can resolve them without reparse/symlink races.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeToolCwd {
    pub mount: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub relative: String,
}
impl RuntimeToolCwd {
    pub fn validate(&self) -> Result<()> {
        if !canonical_slug(&self.mount)
            || self.mount.starts_with("semwright-internal-")
            || !self.relative.is_empty()
        {
            return Err(Error::invalid(
                "Runtime tool working directory must be the root of a declared workspace mount",
            ));
        }
        Ok(())
    }
}

pub fn validate_tool_execute_request(
    name: &str,
    args: &[String],
    stdin: &[u8],
    timeout_ms: u64,
) -> Result<()> {
    if !valid_tool_name(name)
        || args.len() > MAX_TOOL_ARGS
        || args
            .iter()
            .any(|arg| arg.len() > MAX_TOOL_ARG_BYTES || arg.contains('\0'))
        || stdin.len() > MAX_TOOL_STDIN_BYTES
        || timeout_ms == 0
        || timeout_ms > MAX_TOOL_TIMEOUT_MS
    {
        return Err(Error::invalid(
            "Host-mediated tool request exceeds bounded contract",
        ));
    }
    Ok(())
}

pub fn validate_runtime_tool_execute_request(
    name: &str,
    args: &[String],
    stdin: &[u8],
    timeout_ms: u64,
    cwd: Option<&RuntimeToolCwd>,
) -> Result<()> {
    validate_tool_execute_request(name, args, stdin, timeout_ms)?;
    if let Some(cwd) = cwd {
        cwd.validate()?;
    }
    Ok(())
}

type ToolCallWaiters = Arc<Mutex<BTreeMap<String, oneshot::Sender<Result<ToolExecutionOutput>>>>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Hello {
        protocol: u32,
        provider: ProviderIdentity,
        executable_sha256: String,
    },
    Interfaces {
        id: String,
    },
    Capabilities {
        id: String,
    },
    Execute {
        id: String,
        command: String,
        descriptor_sha256: String,
        args: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        context: Option<DriverRequestContext>,
    },
    ToolResult {
        id: String,
        output: ToolExecutionOutput,
    },
    ToolFailure {
        id: String,
        error: Error,
    },
    Validate {
        id: String,
        target: NativeTarget,
    },
    Cancel {
        id: String,
        target: String,
    },
    Health {
        id: String,
    },
    Shutdown {
        id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Response {
    Ready {
        protocol: u32,
        id: String,
        version: String,
    },
    Interfaces {
        id: String,
        interfaces: DriverInterfaces,
    },
    Capabilities {
        id: String,
        capabilities: Vec<Capability>,
        digest: String,
    },
    Result {
        id: String,
        value: Value,
    },
    ToolExecute {
        id: String,
        parent: String,
        name: String,
        args: Vec<String>,
        stdin: Vec<u8>,
        timeout_ms: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cwd: Option<RuntimeToolCwd>,
    },
    Failure {
        id: String,
        error: Error,
    },
    Cancelled {
        id: String,
        target: String,
        accepted: bool,
    },
    Validated {
        id: String,
    },
    Healthy {
        id: String,
        details: Value,
    },
    Shutdown {
        id: String,
    },
    Event {
        kind: String,
        payload: Value,
    },
    CapabilitiesChanged,
    Progress {
        id: String,
        progress: JobProgress,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        artifacts: Vec<JobArtifact>,
    },
}

#[derive(Debug, Clone)]
pub enum DriverChildEvent {
    CapabilitiesChanged,
    Event { kind: String, payload: Value },
}

#[derive(Clone)]
pub struct DriverExecutionContext {
    request_id: String,
    session: String,
    native_target: Option<NativeTarget>,
    cancellation: CancellationToken,
    output: mpsc::UnboundedSender<Response>,
    interfaces: DriverInterfaces,
    protocol: u32,
    tool_calls: ToolCallWaiters,
}
impl DriverExecutionContext {
    pub fn request_id(&self) -> &str {
        &self.request_id
    }
    pub fn session(&self) -> &str {
        &self.session
    }
    pub fn native_target(&self) -> Option<&NativeTarget> {
        self.native_target.as_ref()
    }
    pub fn cancellation(&self) -> CancellationToken {
        self.cancellation.clone()
    }
    pub fn check_cancelled(&self) -> Result<()> {
        if self.cancellation.is_cancelled() {
            Err(Error::new(
                ErrorCode::Cancelled,
                "Driver execution cancelled",
            ))
        } else {
            Ok(())
        }
    }

    /// Select the platform-safe execution route for a declared secondary runtime tool.
    ///
    /// Callers do not branch on filesystem conventions. Linux drivers consume the
    /// Host-materialized sealed executable; Windows requires protocol-v4 Host mediation.
    /// macOS remains fail-closed at the platform sandbox boundary until arbitrary driver
    /// isolation is implemented.
    pub fn runtime_tool_mode(&self, name: &str) -> Result<RuntimeToolMode> {
        if !valid_tool_name(name) {
            return Err(Error::invalid("Invalid runtime tool name"));
        }
        #[cfg(target_os = "windows")]
        {
            if self.protocol >= 4 && self.interfaces.host_tools {
                Ok(RuntimeToolMode::HostMediated)
            } else {
                Err(Error::new(
                    ErrorCode::Unsupported,
                    "Windows runtime tools require Driver Protocol v4 Host mediation",
                ))
            }
        }
        #[cfg(target_os = "linux")]
        {
            tool_path(name)?;
            Ok(RuntimeToolMode::Materialized)
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            Err(Error::new(
                ErrorCode::Unsupported,
                "Secondary runtime tools are not implemented by this platform Host",
            ))
        }
    }

    /// Execute an owner-pinned secondary runtime tool without exposing platform-specific
    /// executable discovery to the driver.
    pub async fn execute_runtime_tool(
        &self,
        name: &str,
        args: Vec<String>,
        stdin: Vec<u8>,
        timeout: std::time::Duration,
    ) -> Result<ToolExecutionOutput> {
        match self.runtime_tool_mode(name)? {
            RuntimeToolMode::HostMediated => {
                self.execute_host_tool(name, args, stdin, timeout, None)
                    .await
            }
            RuntimeToolMode::Materialized => {
                execute_materialized_tool(
                    name,
                    args,
                    stdin,
                    timeout,
                    None,
                    self.cancellation.clone(),
                )
                .await
            }
        }
    }

    /// Execute a runtime tool from a logical workspace-relative working directory.
    /// Protocol v5 is required only when Host mediation is necessary.
    pub async fn execute_runtime_tool_with_cwd(
        &self,
        name: &str,
        args: Vec<String>,
        stdin: Vec<u8>,
        timeout: std::time::Duration,
        cwd: RuntimeToolCwd,
    ) -> Result<ToolExecutionOutput> {
        cwd.validate()?;
        match self.runtime_tool_mode(name)? {
            RuntimeToolMode::HostMediated => {
                self.execute_host_tool(name, args, stdin, timeout, Some(cwd))
                    .await
            }
            RuntimeToolMode::Materialized => {
                execute_materialized_tool(
                    name,
                    args,
                    stdin,
                    timeout,
                    Some(&cwd),
                    self.cancellation.clone(),
                )
                .await
            }
        }
    }

    pub async fn execute_tool(
        &self,
        name: &str,
        args: Vec<String>,
        stdin: Vec<u8>,
        timeout: std::time::Duration,
    ) -> Result<ToolExecutionOutput> {
        self.execute_host_tool(name, args, stdin, timeout, None)
            .await
    }

    async fn execute_host_tool(
        &self,
        name: &str,
        args: Vec<String>,
        stdin: Vec<u8>,
        timeout: std::time::Duration,
        cwd: Option<RuntimeToolCwd>,
    ) -> Result<ToolExecutionOutput> {
        let required_protocol = if cwd.is_some() { 5 } else { 4 };
        if self.protocol < required_protocol || !self.interfaces.host_tools {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "Driver did not negotiate Host-mediated sealed tools",
            ));
        }
        let timeout_ms = u64::try_from(timeout.as_millis()).map_err(|_| {
            Error::new(
                ErrorCode::ResourceExhausted,
                "Host-mediated tool timeout exceeds protocol bounds",
            )
        })?;
        validate_runtime_tool_execute_request(name, &args, &stdin, timeout_ms, cwd.as_ref())?;
        self.check_cancelled()?;

        let id = unique_id();
        let (sender, receiver) = oneshot::channel();
        {
            let mut pending = self.tool_calls.lock().await;
            if pending.len() >= 64 || pending.insert(id.clone(), sender).is_some() {
                return Err(Error::new(
                    ErrorCode::ResourceExhausted,
                    "Driver has too many pending Host-mediated tool calls",
                ));
            }
        }
        if self
            .output
            .send(Response::ToolExecute {
                id: id.clone(),
                parent: self.request_id.clone(),
                name: name.to_owned(),
                args,
                stdin,
                timeout_ms,
                cwd,
            })
            .is_err()
        {
            self.tool_calls.lock().await.remove(&id);
            return Err(Error::unavailable("Driver protocol writer is closed"));
        }

        tokio::select! {
            biased;
            _ = self.cancellation.cancelled() => {
                self.tool_calls.lock().await.remove(&id);
                Err(Error::new(
                    ErrorCode::Cancelled,
                    "Host-mediated tool execution cancelled",
                ))
            }
            result = tokio::time::timeout(timeout + std::time::Duration::from_secs(2), receiver) => {
                match result {
                    Ok(Ok(result)) => result,
                    Ok(Err(_)) => Err(Error::unavailable("Host-mediated tool response channel closed")),
                    Err(_) => {
                        self.tool_calls.lock().await.remove(&id);
                        Err(Error::new(
                            ErrorCode::Timeout,
                            "Host-mediated tool execution timed out",
                        ))
                    }
                }
            }
        }
    }

    pub fn report_progress(
        &self,
        progress: JobProgress,
        artifacts: Vec<JobArtifact>,
    ) -> Result<()> {
        if !self.interfaces.progress {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "Driver did not negotiate progress reporting",
            ));
        }
        if !artifacts.is_empty() && !self.interfaces.artifacts {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "Driver did not negotiate artifact reporting",
            ));
        }
        progress.validate()?;
        if artifacts.len() > 32 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Driver progress artifact count exceeds limit",
            ));
        }
        for artifact in &artifacts {
            artifact.validate()?;
        }
        self.output
            .send(Response::Progress {
                id: self.request_id.clone(),
                progress,
                artifacts,
            })
            .map_err(|_| Error::unavailable("Driver protocol writer is closed"))
    }
    pub fn capabilities_changed(&self) -> Result<()> {
        if !self.interfaces.dynamic_capabilities {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "Driver did not negotiate dynamic capabilities",
            ));
        }
        self.output
            .send(Response::CapabilitiesChanged)
            .map_err(|_| Error::unavailable("Driver protocol writer is closed"))
    }

    pub fn emit_event(&self, kind: impl Into<String>, payload: Value) -> Result<()> {
        if !self.interfaces.events {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "Driver did not negotiate child events",
            ));
        }
        let kind = kind.into();
        if kind.is_empty()
            || kind.len() > 128
            || kind.starts_with("provider.")
            || !kind.bytes().all(|b| {
                b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
            })
            || serde_json::to_vec(&payload)?.len() > 16_384
        {
            return Err(Error::invalid("Driver event exceeds its bounded contract"));
        }
        self.output
            .send(Response::Event { kind, payload })
            .map_err(|_| Error::unavailable("Driver protocol writer is closed"))
    }
}

#[async_trait]
pub trait Driver: Send + 'static {
    fn id(&self) -> &str;
    fn version(&self) -> &str;
    fn interfaces(&self) -> DriverInterfaces {
        DriverInterfaces {
            health: true,
            ..Default::default()
        }
    }
    fn take_events(&mut self) -> Option<mpsc::UnboundedReceiver<DriverChildEvent>> {
        None
    }
    async fn capabilities(&mut self) -> Result<Vec<Capability>>;
    async fn execute(
        &mut self,
        command: &str,
        descriptor_sha256: &str,
        args: Value,
    ) -> Result<Value>;
    async fn execute_with_context(
        &mut self,
        command: &str,
        descriptor_sha256: &str,
        args: Value,
        context: DriverExecutionContext,
    ) -> Result<Value> {
        context.check_cancelled()?;
        self.execute(command, descriptor_sha256, args).await
    }
    async fn validate_native_ref(&mut self, _target: &NativeTarget) -> Result<()> {
        Err(Error::new(
            ErrorCode::Unsupported,
            "Driver does not validate native references",
        ))
    }
    async fn health(&mut self) -> Result<Value> {
        Ok(serde_json::json!({"healthy":true}))
    }
}

fn valid_hello_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

async fn serve_v1<D: Driver>(
    mut driver: D,
    owner_identity: ProviderIdentity,
    mut input: tokio::io::Stdin,
    mut output: tokio::io::Stdout,
) -> Result<()> {
    loop {
        match read_frame::<_, Request>(&mut input).await? {
            Request::Capabilities { id } => {
                let capabilities = driver.capabilities().await?;
                if capabilities.len() > 2048 {
                    return Err(Error::new(
                        ErrorCode::ResourceExhausted,
                        "Driver capability catalog exceeds limit",
                    ));
                }
                for capability in &capabilities {
                    capability.validate_for(&owner_identity)?;
                }
                let digest = capabilities_digest(&capabilities)?;
                write_frame(
                    &mut output,
                    &Response::Capabilities {
                        id,
                        capabilities,
                        digest,
                    },
                )
                .await?;
            }
            Request::Execute {
                id,
                command,
                descriptor_sha256,
                args,
                context: _,
            } => {
                let response = match driver.execute(&command, &descriptor_sha256, args).await {
                    Ok(value) => Response::Result { id, value },
                    Err(error) => Response::Failure { id, error },
                };
                write_frame(&mut output, &response).await?;
            }
            Request::Health { id } => {
                let response = match driver.health().await {
                    Ok(details) => Response::Healthy { id, details },
                    Err(error) => Response::Failure { id, error },
                };
                write_frame(&mut output, &response).await?;
            }
            Request::Shutdown { id } => {
                write_frame(&mut output, &Response::Shutdown { id }).await?;
                return Ok(());
            }
            Request::Hello { .. }
            | Request::Interfaces { .. }
            | Request::Cancel { .. }
            | Request::Validate { .. }
            | Request::ToolResult { .. }
            | Request::ToolFailure { .. } => {
                return Err(Error::new(
                    ErrorCode::ProtocolMismatch,
                    "Driver protocol v1 received a v2-only or duplicate request",
                ));
            }
        }
    }
}

async fn serve_v2<D: Driver>(
    mut driver: D,
    owner_identity: ProviderIdentity,
    protocol: u32,
    mut input: tokio::io::Stdin,
    mut output: tokio::io::Stdout,
) -> Result<()> {
    let interfaces = driver.interfaces();
    let mut child_events = driver.take_events();
    let driver = Arc::new(Mutex::new(driver));
    let active = Arc::new(Mutex::new(BTreeMap::<String, CancellationToken>::new()));
    let tool_calls: ToolCallWaiters = Arc::new(Mutex::new(BTreeMap::new()));
    let (responses, mut response_rx) = mpsc::unbounded_channel::<Response>();

    let writer = tokio::spawn(async move {
        while let Some(response) = response_rx.recv().await {
            write_frame(&mut output, &response).await?;
        }
        Ok::<(), Error>(())
    });

    let event_task = child_events.take().map(|mut events| {
        let responses = responses.clone();
        tokio::spawn(async move {
            while let Some(event) = events.recv().await {
                let response = match event {
                    DriverChildEvent::CapabilitiesChanged => {
                        if !interfaces.dynamic_capabilities {
                            continue;
                        }
                        Response::CapabilitiesChanged
                    }
                    DriverChildEvent::Event { kind, payload } => {
                        if !interfaces.events
                            || kind.is_empty()
                            || kind.len() > 128
                            || kind.starts_with("provider.")
                            || !kind.bytes().all(|b| {
                                b.is_ascii_lowercase()
                                    || b.is_ascii_digit()
                                    || matches!(b, b'.' | b'_' | b'-')
                            })
                            || serde_json::to_vec(&payload).is_ok_and(|bytes| bytes.len() > 16_384)
                        {
                            continue;
                        }
                        Response::Event { kind, payload }
                    }
                };
                if responses.send(response).is_err() {
                    break;
                }
            }
        })
    });

    let mut tasks = tokio::task::JoinSet::new();
    loop {
        match read_frame::<_, Request>(&mut input).await? {
            Request::Interfaces { id } => {
                responses
                    .send(Response::Interfaces { id, interfaces })
                    .map_err(|_| Error::unavailable("Driver protocol writer is closed"))?;
            }
            Request::Capabilities { id } => {
                let driver = driver.clone();
                let responses = responses.clone();
                let owner_identity = owner_identity.clone();
                tasks.spawn(async move {
                    let response = {
                        let mut driver = driver.lock().await;
                        match driver.capabilities().await {
                            Ok(capabilities) if capabilities.len() <= 2048 => {
                                let valid = capabilities.iter().all(|capability| {
                                    capability.validate_for(&owner_identity).is_ok()
                                });
                                if !valid {
                                    Response::Failure {
                                        id,
                                        error: Error::new(
                                            ErrorCode::PolicyDenied,
                                            "Driver capability escaped its owner namespace",
                                        ),
                                    }
                                } else {
                                    match capabilities_digest(&capabilities) {
                                        Ok(digest) => Response::Capabilities {
                                            id,
                                            capabilities,
                                            digest,
                                        },
                                        Err(error) => Response::Failure { id, error },
                                    }
                                }
                            }
                            Ok(_) => Response::Failure {
                                id,
                                error: Error::new(
                                    ErrorCode::ResourceExhausted,
                                    "Driver capability catalog exceeds limit",
                                ),
                            },
                            Err(error) => Response::Failure { id, error },
                        }
                    };
                    let _ = responses.send(response);
                });
            }
            Request::Execute {
                id,
                command,
                descriptor_sha256,
                args,
                context,
            } => {
                let request_context = if protocol >= 3 {
                    let context = context.ok_or_else(|| {
                        Error::new(
                            ErrorCode::ProtocolMismatch,
                            "Driver protocol v3 execution requires request context",
                        )
                    })?;
                    context.validate()?;
                    context
                } else {
                    if context.is_some() {
                        return Err(Error::new(
                            ErrorCode::ProtocolMismatch,
                            "Driver protocol v2 cannot receive v3 request context",
                        ));
                    }
                    DriverRequestContext {
                        session: "driver-v2".into(),
                        native_target: None,
                    }
                };
                let token = CancellationToken::new();
                {
                    let mut active = active.lock().await;
                    if active.contains_key(&id) {
                        responses
                            .send(Response::Failure {
                                id,
                                error: Error::new(
                                    ErrorCode::Conflict,
                                    "Driver execution request ID is already active",
                                ),
                            })
                            .map_err(|_| Error::unavailable("Driver protocol writer is closed"))?;
                        continue;
                    }
                    if active.len() >= 64 {
                        responses
                            .send(Response::Failure {
                                id,
                                error: Error::new(
                                    ErrorCode::ResourceExhausted,
                                    "Driver has too many active executions",
                                ),
                            })
                            .map_err(|_| Error::unavailable("Driver protocol writer is closed"))?;
                        continue;
                    }
                    active.insert(id.clone(), token.clone());
                }
                let driver = driver.clone();
                let active = active.clone();
                let responses = responses.clone();
                let tool_calls = tool_calls.clone();
                tasks.spawn(async move {
                    let context = DriverExecutionContext {
                        request_id: id.clone(),
                        session: request_context.session,
                        native_target: request_context.native_target,
                        cancellation: token,
                        output: responses.clone(),
                        interfaces,
                        protocol,
                        tool_calls: tool_calls.clone(),
                    };
                    let result = {
                        let mut driver = driver.lock().await;
                        driver
                            .execute_with_context(&command, &descriptor_sha256, args, context)
                            .await
                    };
                    active.lock().await.remove(&id);
                    let response = match result {
                        Ok(value) => Response::Result { id, value },
                        Err(error) => Response::Failure { id, error },
                    };
                    let _ = responses.send(response);
                });
            }
            Request::ToolResult { id, output } => {
                if protocol < 4 || !interfaces.host_tools {
                    return Err(Error::new(
                        ErrorCode::ProtocolMismatch,
                        "Driver received an unnegotiated Host-mediated tool result",
                    ));
                }
                output.validate()?;
                if let Some(sender) = tool_calls.lock().await.remove(&id) {
                    let _ = sender.send(Ok(output));
                }
            }
            Request::ToolFailure { id, error } => {
                if protocol < 4 || !interfaces.host_tools {
                    return Err(Error::new(
                        ErrorCode::ProtocolMismatch,
                        "Driver received an unnegotiated Host-mediated tool failure",
                    ));
                }
                if let Some(sender) = tool_calls.lock().await.remove(&id) {
                    let _ = sender.send(Err(Error::new(
                        error.code,
                        "Host-mediated sealed tool failed",
                    )));
                }
            }
            Request::Validate { id, target } => {
                if protocol < 3 || !interfaces.native_refs {
                    responses
                        .send(Response::Failure {
                            id,
                            error: Error::new(
                                ErrorCode::Unsupported,
                                "Driver did not negotiate native-reference validation",
                            ),
                        })
                        .map_err(|_| Error::unavailable("Driver protocol writer is closed"))?;
                    continue;
                }
                let target_validation = validate_native_target(&target);
                let driver = driver.clone();
                let responses = responses.clone();
                tasks.spawn(async move {
                    let response = match target_validation {
                        Ok(()) => match driver.lock().await.validate_native_ref(&target).await {
                            Ok(()) => Response::Validated { id },
                            Err(error) => Response::Failure { id, error },
                        },
                        Err(error) => Response::Failure { id, error },
                    };
                    let _ = responses.send(response);
                });
            }
            Request::Cancel { id, target } => {
                let token = active.lock().await.get(&target).cloned();
                let accepted = token.is_some();
                if let Some(token) = token {
                    token.cancel();
                }
                responses
                    .send(Response::Cancelled {
                        id,
                        target,
                        accepted,
                    })
                    .map_err(|_| Error::unavailable("Driver protocol writer is closed"))?;
            }
            Request::Health { id } => {
                let driver = driver.clone();
                let responses = responses.clone();
                tasks.spawn(async move {
                    let response = match driver.lock().await.health().await {
                        Ok(details) => Response::Healthy { id, details },
                        Err(error) => Response::Failure { id, error },
                    };
                    let _ = responses.send(response);
                });
            }
            Request::Shutdown { id } => {
                for token in active.lock().await.values() {
                    token.cancel();
                }
                tasks.abort_all();
                while tasks.join_next().await.is_some() {}
                responses
                    .send(Response::Shutdown { id })
                    .map_err(|_| Error::unavailable("Driver protocol writer is closed"))?;
                break;
            }
            Request::Hello { .. } => {
                return Err(Error::new(
                    ErrorCode::ProtocolMismatch,
                    "Driver hello may only occur once",
                ));
            }
        }
    }
    if let Some(task) = event_task {
        task.abort();
    }
    drop(responses);
    writer
        .await
        .map_err(|_| Error::new(ErrorCode::Internal, "Driver protocol writer task failed"))??;
    Ok(())
}

pub async fn serve<D: Driver>(driver: D) -> Result<()> {
    let mut input = tokio::io::stdin();
    let mut output = tokio::io::stdout();
    let (protocol, owner_identity) = match read_frame::<_, Request>(&mut input).await? {
        Request::Hello {
            protocol,
            provider,
            executable_sha256,
        } if (DRIVER_PROTOCOL_MIN_VERSION..=DRIVER_PROTOCOL_VERSION).contains(&protocol)
            && provider.kind == SourceKind::Driver
            && provider.id == format!("driver:{}", driver.id())
            && provider.version == driver.version()
            && valid_hello_digest(&executable_sha256) =>
        {
            (protocol, provider)
        }
        _ => {
            return Err(Error::new(
                ErrorCode::ProtocolMismatch,
                "Driver hello does not match its pinned owner identity",
            ));
        }
    };
    owner_identity.validate_external()?;
    write_frame(
        &mut output,
        &Response::Ready {
            protocol,
            id: driver.id().into(),
            version: driver.version().into(),
        },
    )
    .await?;
    if protocol == 1 {
        serve_v1(driver, owner_identity, input, output).await
    } else {
        serve_v2(driver, owner_identity, protocol, input, output).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use semwright_types::{Idempotency, Risk};

    fn manifest() -> Manifest {
        #[cfg(windows)]
        let executable = PathBuf::from(r"C:\semwright\driver.exe");
        #[cfg(not(windows))]
        let executable = PathBuf::from("/tmp/driver");
        Manifest {
            manifest_version: 1,
            protocol: 1,
            id: "fixture".into(),
            version: "1.0".into(),
            publisher: "semwright-tests".into(),
            executable,
            sha256: "a".repeat(64),
            application: ApplicationMatch {
                desktop_id: Some("org.example.Fixture".into()),
                ..Default::default()
            },
            transport: Transport::StdioV1,
            mounts: vec![],
            system_config: vec![],
            secrets: vec![],
            tools: vec![],
            network: false,
            loopback_port: None,
            resources: DriverResources::default(),
            request_timeout_ms: 1000,
            interfaces: DriverInterfaces::default(),
        }
    }
    #[test]
    fn manifest_identity_is_owner_assigned_and_strict() {
        let manifest = manifest();
        manifest.validate().unwrap();
        let identity = manifest.identity().unwrap();
        assert_eq!(identity.id, "driver:fixture");
        assert_eq!(identity.namespace, "driver.fixture.");
        let mut value = serde_json::to_value(manifest).unwrap();
        value["allow_shell"] = serde_json::json!(true);
        assert!(serde_json::from_value::<Manifest>(value).is_err());
    }
    #[test]
    fn protocol_messages_reject_unknown_fields_and_wrong_manifest_version() {
        let mut manifest = manifest();
        manifest.manifest_version = 2;
        assert!(manifest.validate().is_err());
        assert!(
            serde_json::from_value::<Request>(serde_json::json!({
                "type":"health","id":"1","shell":"unexpected"
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<Response>(serde_json::json!({
                "type":"shutdown","id":"1","extra":true
            }))
            .is_err()
        );
    }
    #[test]
    fn resource_requests_are_bounded_and_default_to_existing_sandbox_limits() {
        let resources = DriverResources::default();
        assert_eq!(resources.address_space_bytes, 536_870_912);
        assert_eq!(resources.operation_cpu_seconds, 0);
        assert!(resources.validate().is_ok());

        let mut manifest = manifest();
        manifest.resources.address_space_bytes = 2_147_483_648;
        manifest.resources.cpu_seconds = 120;
        manifest.validate().unwrap();

        manifest.resources.cpu_seconds = 3_600;
        assert!(manifest.validate().is_err());
        manifest.resources.operation_cpu_seconds = 60;
        manifest.validate().unwrap();

        manifest.resources.operation_cpu_seconds = 301;
        assert!(manifest.validate().is_err());
        manifest.resources.operation_cpu_seconds = 60;
        manifest.resources.cpu_seconds = 30;
        assert!(manifest.validate().is_err());

        manifest.resources.cpu_seconds = 3_600;
        manifest.resources.operation_cpu_seconds = 60;
        manifest.resources.address_space_bytes = 4_294_967_297;
        assert!(manifest.validate().is_err());
        manifest.resources.address_space_bytes = 2_147_483_648;
        manifest.resources.processes = 257;
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn system_config_mounts_are_narrow_unique_and_owner_named() {
        let mut valid = manifest();
        valid.system_config = vec![SystemConfigMount {
            root: "libreoffice-config".into(),
            destination: "/etc/libreoffice".into(),
        }];
        valid.validate().unwrap();

        for bad in ["/etc", "/etc/libreoffice/share", "/home/user", "relative"] {
            let mut candidate = manifest();
            candidate.system_config = vec![SystemConfigMount {
                root: "config".into(),
                destination: bad.into(),
            }];
            assert!(candidate.validate().is_err(), "{bad}");
        }

        let mut duplicate_root = manifest();
        duplicate_root.mounts.push(DriverMount {
            root: "same".into(),
            read_only: true,
            execute: false,
        });
        duplicate_root.system_config.push(SystemConfigMount {
            root: "same".into(),
            destination: "/etc/example".into(),
        });
        assert!(duplicate_root.validate().is_err());
    }

    #[test]
    fn secret_mounts_are_unique_bounded_and_separate_from_other_grants() {
        let mut valid = manifest();
        valid.secrets = vec![DriverSecretMount {
            root: "pairing-secret".into(),
            name: "pairing".into(),
        }];
        valid.validate().unwrap();

        let mut duplicate_root = valid.clone();
        duplicate_root.mounts.push(DriverMount {
            root: "pairing-secret".into(),
            read_only: true,
            execute: false,
        });
        assert!(duplicate_root.validate().is_err());

        let mut duplicate_name = manifest();
        duplicate_name.secrets = vec![
            DriverSecretMount {
                root: "secret-a".into(),
                name: "pairing".into(),
            },
            DriverSecretMount {
                root: "secret-b".into(),
                name: "pairing".into(),
            },
        ];
        assert!(duplicate_name.validate().is_err());

        for bad in ["", "../pairing", "Pairing Secret", "semwright-internal-x"] {
            let mut candidate = manifest();
            candidate.secrets = vec![DriverSecretMount {
                root: "secret".into(),
                name: bad.into(),
            }];
            assert!(candidate.validate().is_err(), "{bad}");
        }
    }

    #[test]
    fn tool_mounts_are_digest_pinned_unique_and_separate_from_other_grants() {
        let mut valid = manifest();
        valid.tools = vec![DriverToolMount {
            root: "godot-runtime".into(),
            name: "godot".into(),
            sha256: "a".repeat(64),
            mounts: vec![],
        }];
        valid.validate().unwrap();

        let mut duplicate_root = valid.clone();
        duplicate_root.mounts.push(DriverMount {
            root: "godot-runtime".into(),
            read_only: true,
            execute: false,
        });
        assert!(duplicate_root.validate().is_err());

        let mut duplicate_name = manifest();
        duplicate_name.tools = vec![
            DriverToolMount {
                root: "tool-a".into(),
                name: "godot".into(),
                sha256: "a".repeat(64),
                mounts: vec![],
            },
            DriverToolMount {
                root: "tool-b".into(),
                name: "godot".into(),
                sha256: "b".repeat(64),
                mounts: vec![],
            },
        ];
        assert!(duplicate_name.validate().is_err());

        let mut bad_digest = manifest();
        bad_digest.tools = vec![DriverToolMount {
            root: "tool".into(),
            name: "godot".into(),
            sha256: "not-a-digest".into(),
            mounts: vec![],
        }];
        assert!(bad_digest.validate().is_err());
    }

    #[test]
    fn host_tool_protocol_requires_v4_tools_and_bounded_requests() {
        let mut candidate = manifest();
        candidate.protocol = 3;
        candidate.interfaces.host_tools = true;
        assert!(candidate.validate().is_err());

        candidate.protocol = 4;
        assert!(candidate.validate().is_err());

        candidate.tools = vec![DriverToolMount {
            root: "tool-root".into(),
            name: "probe".into(),
            sha256: "a".repeat(64),
            mounts: vec![],
        }];
        candidate.validate().unwrap();

        assert!(
            validate_tool_execute_request("probe", &[], &[], 1_000).is_ok(),
            "a bounded Host-tool request should validate"
        );
        assert!(validate_tool_execute_request("../probe", &[], &[], 1_000).is_err());
        assert!(
            validate_tool_execute_request(
                "probe",
                &vec!["x".into(); MAX_TOOL_ARGS + 1],
                &[],
                1_000
            )
            .is_err()
        );
        assert!(
            validate_tool_execute_request(
                "probe",
                &[],
                &vec![0u8; MAX_TOOL_STDIN_BYTES + 1],
                1_000
            )
            .is_err()
        );
        assert!(validate_tool_execute_request("probe", &[], &[], 0).is_err());
        assert!(validate_tool_execute_request("probe", &[], &[], MAX_TOOL_TIMEOUT_MS + 1).is_err());
    }

    #[test]
    fn v5_runtime_tool_mounts_and_cwd_are_bounded() {
        let mut candidate = manifest();
        candidate.protocol = 5;
        candidate.interfaces.host_tools = true;
        candidate.mounts.push(DriverMount {
            root: "project".into(),
            read_only: false,
            execute: false,
        });
        candidate.tools = vec![DriverToolMount {
            root: "godot-runtime".into(),
            name: "godot".into(),
            sha256: "a".repeat(64),
            mounts: vec!["project".into()],
        }];
        candidate.validate().unwrap();

        let mut v4 = candidate.clone();
        v4.protocol = 4;
        assert!(v4.validate().is_err());

        let mut undeclared = candidate.clone();
        undeclared.tools[0].mounts = vec!["other".into()];
        assert!(undeclared.validate().is_err());

        let cwd = RuntimeToolCwd {
            mount: "project".into(),
            relative: String::new(),
        };
        assert!(cwd.validate().is_ok());
        assert!(
            validate_runtime_tool_execute_request("godot", &[], &[], 1_000, Some(&cwd)).is_ok()
        );
        for relative in ["nested", "../escape", "./dot", r"windows\path"] {
            let invalid = RuntimeToolCwd {
                mount: "project".into(),
                relative: relative.into(),
            };
            assert!(invalid.validate().is_err(), "{relative}");
        }

        let v4_wire = serde_json::to_value(Response::ToolExecute {
            id: "tool-1".into(),
            parent: "request-1".into(),
            name: "godot".into(),
            args: vec![],
            stdin: vec![],
            timeout_ms: 1_000,
            cwd: None,
        })
        .unwrap();
        assert!(v4_wire.get("cwd").is_none());

        let v5_wire = serde_json::to_value(Response::ToolExecute {
            id: "tool-2".into(),
            parent: "request-1".into(),
            name: "godot".into(),
            args: vec![],
            stdin: vec![],
            timeout_ms: 1_000,
            cwd: Some(cwd),
        })
        .unwrap();
        assert_eq!(v5_wire["cwd"]["mount"], "project");
        assert!(v5_wire["cwd"].get("relative").is_none());
    }

    #[test]
    fn runtime_tool_mode_hides_platform_specific_execution() {
        let (output, _receiver) = mpsc::unbounded_channel();
        let context = DriverExecutionContext {
            request_id: "runtime-tool-test".into(),
            session: "owner-session".into(),
            native_target: None,
            cancellation: CancellationToken::new(),
            output,
            interfaces: DriverInterfaces {
                host_tools: true,
                ..Default::default()
            },
            protocol: 4,
            tool_calls: Arc::new(Mutex::new(BTreeMap::new())),
        };

        #[cfg(target_os = "windows")]
        assert_eq!(
            context.runtime_tool_mode("probe").unwrap(),
            RuntimeToolMode::HostMediated
        );
        #[cfg(target_os = "linux")]
        assert_eq!(
            context.runtime_tool_mode("probe").unwrap(),
            RuntimeToolMode::Materialized
        );
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        assert!(matches!(
            context.runtime_tool_mode("probe"),
            Err(error) if error.code == ErrorCode::Unsupported
        ));

        assert!(context.runtime_tool_mode("../probe").is_err());
    }

    #[test]
    fn executable_mounts_must_be_read_only_and_wire_compatible() {
        let mut candidate = manifest();
        candidate.mounts.push(DriverMount {
            root: "runtime".into(),
            read_only: true,
            execute: true,
        });
        candidate.validate().unwrap();
        let encoded = serde_json::to_value(&candidate).unwrap();
        assert_eq!(encoded["mounts"][0]["execute"], true);

        candidate.mounts[0].read_only = false;
        assert!(candidate.validate().is_err());

        let mut data_only = manifest();
        data_only.mounts.push(DriverMount {
            root: "media".into(),
            read_only: true,
            execute: false,
        });
        let encoded = serde_json::to_value(&data_only).unwrap();
        assert!(encoded["mounts"][0].get("execute").is_none());
    }

    #[test]
    fn artifact_port_tags_are_machine_readable_and_bounded() {
        assert_eq!(
            artifact_input_tag("model/3d").unwrap(),
            "artifact-in:model/3d"
        );
        assert_eq!(
            artifact_output_tag("video/clip").unwrap(),
            "artifact-out:video/clip"
        );
        for bad in ["Model/3d", "model", "model/", "model/3d/raw", "model/_3d"] {
            assert!(artifact_input_tag(bad).is_err(), "{bad}");
        }

        let identity = manifest().identity().unwrap();
        let mut descriptor = CommandDescriptor {
            name: "driver.fixture.export".into(),
            version: "1".into(),
            description: "Export fixture".into(),
            input_schema: serde_json::json!({"type":"object"}),
            output_schema: serde_json::json!({"type":"object"}),
            requires: vec![identity.id.clone()],
            risk: Risk::ReadOnly,
            idempotency: Idempotency::ReadOnly,
            timeout_ms: 1000,
            dry_run: true,
            interactive_consent: false,
            backends: vec![identity.id.clone()],
        };
        let good = Capability {
            descriptor: descriptor.clone(),
            aliases: vec![],
            tags: vec!["artifact-out:model/3d".into()],
            object_types: vec![],
        };
        good.validate_for(&identity).unwrap();
        descriptor.name = "driver.fixture.bad-export".into();
        let bad = Capability {
            descriptor,
            aliases: vec![],
            tags: vec!["artifact-out:Model/3d".into()],
            object_types: vec![],
        };
        assert!(bad.validate_for(&identity).is_err());
    }

    #[test]
    fn capability_cannot_escape_driver_namespace_or_scope() {
        let identity = manifest().identity().unwrap();
        let descriptor = CommandDescriptor {
            name: "driver.fixture.inspect".into(),
            version: "1".into(),
            description: "Inspect fixture".into(),
            input_schema: serde_json::json!({"type":"object","additionalProperties":false}),
            output_schema: serde_json::json!({"type":"object"}),
            requires: vec![identity.id.clone()],
            risk: Risk::ReadOnly,
            idempotency: Idempotency::ReadOnly,
            timeout_ms: 1000,
            dry_run: true,
            interactive_consent: false,
            backends: vec![identity.id.clone()],
        };
        Capability {
            descriptor: descriptor.clone(),
            aliases: vec![],
            tags: vec![],
            object_types: vec![],
        }
        .validate_for(&identity)
        .unwrap();
        let mut bad = descriptor;
        bad.name = "doctor".into();
        assert!(
            Capability {
                descriptor: bad,
                aliases: vec![],
                tags: vec![],
                object_types: vec![],
            }
            .validate_for(&identity)
            .is_err()
        );
    }
}
