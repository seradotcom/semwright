//! Versioned application-driver contract. Drivers are providers; this crate has no broker or MCP authority.
use async_trait::async_trait;
use semwright_protocol::{read_frame, write_frame};
use semwright_types::provider::canonical_slug;
use semwright_types::{
    CommandDescriptor, Error, ErrorCode, JobArtifact, JobProgress, NativeTarget, ProviderIdentity,
    Result, SourceKind,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path, PathBuf},
    sync::Arc,
};
use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;

pub const DRIVER_MANIFEST_VERSION: u32 = 1;
pub const DRIVER_PROTOCOL_MIN_VERSION: u32 = 1;
pub const DRIVER_PROTOCOL_VERSION: u32 = 3;

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

/// Owner-granted secret file exposed read-only under /run/secrets/<name>.
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
    #[serde(default = "default_cpu_seconds")]
    pub cpu_seconds: u64,
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
            address_space_bytes: default_address_space_bytes(),
            file_size_bytes: default_file_size_bytes(),
        }
    }
}
impl DriverResources {
    fn validate(&self) -> Result<()> {
        if !(32..=1024).contains(&self.open_files)
            || !(8..=256).contains(&self.processes)
            || !(5..=300).contains(&self.cpu_seconds)
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
        if self.publisher.is_empty()
            || self.publisher.len() > 128
            || self.publisher.chars().any(char::is_control)
            || !self.executable.is_absolute()
            || self.sha256.len() != 64
            || !self.sha256.bytes().all(|b| b.is_ascii_hexdigit())
            || self.mounts.len() > 16
            || self.system_config.len() > 8
            || self.secrets.len() > 8
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
            | Request::Validate { .. } => {
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
                tasks.spawn(async move {
                    let context = DriverExecutionContext {
                        request_id: id.clone(),
                        session: request_context.session,
                        native_target: request_context.native_target,
                        cancellation: token,
                        output: responses.clone(),
                        interfaces,
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
        Manifest {
            manifest_version: 1,
            protocol: 1,
            id: "fixture".into(),
            version: "1.0".into(),
            publisher: "semwright-tests".into(),
            executable: "/tmp/driver".into(),
            sha256: "a".repeat(64),
            application: ApplicationMatch {
                desktop_id: Some("org.example.Fixture".into()),
                ..Default::default()
            },
            transport: Transport::StdioV1,
            mounts: vec![],
            system_config: vec![],
            secrets: vec![],
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
        assert!(resources.validate().is_ok());

        let mut manifest = manifest();
        manifest.resources.address_space_bytes = 2_147_483_648;
        manifest.resources.cpu_seconds = 120;
        manifest.validate().unwrap();

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
