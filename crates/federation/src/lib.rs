//! MCP federation as a Semwright Provider. Upstream metadata is untrusted data.
pub mod upstreams;
use async_trait::async_trait;
use rmcp::{
    ClientHandler, RoleClient, ServiceExt,
    model::{CallToolRequest, CallToolRequestParams, ClientRequest, ServerResult},
    service::{NotificationContext, Peer, PeerRequestOptions, RunningServiceCancellationToken},
    transport::TokioChildProcess,
};
use semwright_backend_api::{
    Context, ProvidedCapability, Provider, ProviderInterfaces, ProviderSignal,
};
use semwright_registry::catalog::descriptor_digest;
use semwright_types::{
    CapabilityStatus, CommandDescriptor, Error, ErrorCode, Feature, Idempotency, ProviderIdentity,
    Result, Risk, SourceKind,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::io::Read;
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    process::Command,
    sync::{RwLock, broadcast},
};
use tokio_util::sync::CancellationToken;
pub use upstreams::{
    UpstreamRegistry, default_upstream_registry_path, load_upstream_registry, new_upstream,
    save_upstream_registry,
};

fn default_timeout() -> u64 {
    30_000
}
fn default_enabled() -> bool {
    true
}

/// Owner-controlled configuration for a trusted local stdio MCP executable.
///
/// This type deliberately does not inherit the daemon environment. Wiring arbitrary user-space
/// executables into the long-lived daemon is a separate sandboxing decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StdioUpstreamConfig {
    pub slug: String,
    pub program: PathBuf,
    /// Lowercase SHA-256 of the executable selected by the owner.
    pub sha256: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub expected_name: Option<String>,
    pub expected_version: Option<String>,
    #[serde(default = "default_timeout")]
    pub request_timeout_ms: u64,
    /// Definition state only. Enabling never grants the corresponding policy scope.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

impl StdioUpstreamConfig {
    pub fn validate_definition(&self) -> Result<()> {
        ProviderIdentity::external(SourceKind::ExternalMcp, &self.slug, "validation")?;
        if !self.program.is_absolute()
            || self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || self.args.len() > 64
            || self
                .args
                .iter()
                .any(|arg| arg.len() > 4096 || arg.contains('\0'))
            || self
                .expected_name
                .as_ref()
                .is_some_and(|v| v.is_empty() || v.len() > 128 || v.chars().any(char::is_control))
            || self
                .expected_version
                .as_ref()
                .is_some_and(|v| v.is_empty() || v.len() > 80 || v.chars().any(char::is_control))
            || !(100..=120_000).contains(&self.request_timeout_ms)
        {
            return Err(Error::invalid(
                "Federated stdio configuration exceeds its bounds",
            ));
        }
        Ok(())
    }
    pub fn validate(&self) -> Result<()> {
        self.validate_definition()?;
        if std::fs::canonicalize(&self.program).ok().as_ref() != Some(&self.program) {
            return Err(Error::invalid(
                "Federated stdio executable must be an absolute canonical path",
            ));
        }
        let digest = executable_sha256(&self.program)?;
        if digest != self.sha256 {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "Federated executable digest does not match owner configuration",
            ));
        }
        Ok(())
    }
}

#[cfg(unix)]
pub fn executable_sha256(program: &std::path::Path) -> Result<String> {
    if !program.is_absolute() || std::fs::canonicalize(program).ok().as_deref() != Some(program) {
        return Err(Error::invalid(
            "Federated stdio executable must be an absolute canonical path",
        ));
    }
    let before = std::fs::symlink_metadata(program)?;
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(program)?;
    let meta = file.metadata()?;
    let uid = meta.uid();
    if !meta.is_file()
        || meta.permissions().mode() & 0o022 != 0
        || meta.len() > 536_870_912
        || (uid != 0 && uid != semwright_protocol::current_uid())
        || meta.dev() != before.dev()
        || meta.ino() != before.ino()
    {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Federated stdio executable must be stable, root/owner-owned, bounded and not writable by group or others",
        ));
    }
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}
#[cfg(target_os = "windows")]
pub fn executable_sha256(_program: &std::path::Path) -> Result<String> {
    Err(Error::new(
        ErrorCode::SandboxDenied,
        "Windows external MCP executable trust is fail-closed until owner/DACL and signing policy is implemented",
    ))
}

#[derive(Clone)]
struct UpstreamClient {
    signals: broadcast::Sender<ProviderSignal>,
}

impl ClientHandler for UpstreamClient {
    async fn on_tool_list_changed(&self, _context: NotificationContext<RoleClient>) {
        let _ = self.signals.send(ProviderSignal::CapabilitiesChanged);
    }
}

#[derive(Clone)]
struct ImportedTool {
    upstream_name: String,
    descriptor_sha256: String,
    structured_output: bool,
}

pub struct ExternalMcpProvider {
    identity: ProviderIdentity,
    peer: Peer<RoleClient>,
    imported: RwLock<BTreeMap<String, ImportedTool>>,
    signals: broadcast::Sender<ProviderSignal>,
    closed: CancellationToken,
    service_cancel: Mutex<Option<RunningServiceCancellationToken>>,
    request_timeout: Duration,
}

impl ExternalMcpProvider {
    /// Connect a trusted, owner-selected executable.
    pub async fn connect_trusted_stdio(config: StdioUpstreamConfig) -> Result<Arc<Self>> {
        config.validate()?;
        let (signals, _) = broadcast::channel(32);
        let handler = UpstreamClient {
            signals: signals.clone(),
        };
        let mut command = Command::new(&config.program);
        command.args(&config.args);
        command.env_clear();
        command.current_dir("/");
        let (transport, _) = TokioChildProcess::builder(command)
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| {
                Error::new(
                    ErrorCode::BackendFailed,
                    "Failed to launch owner-configured MCP executable",
                )
            })?;
        let service = tokio::time::timeout(Duration::from_secs(10), handler.serve(transport))
            .await
            .map_err(|_| Error::new(ErrorCode::Timeout, "MCP initialization timed out"))?
            .map_err(|_| {
                Error::new(
                    ErrorCode::BackendFailed,
                    "MCP initialization or protocol negotiation failed",
                )
            })?;
        let peer = service.peer().clone();
        let info = peer
            .peer_info()
            .ok_or_else(|| Error::new(ErrorCode::ProtocolMismatch, "MCP server info missing"))?;
        let server_info = info.server_info.as_ref().ok_or_else(|| {
            Error::new(
                ErrorCode::ProtocolMismatch,
                "MCP implementation identity missing",
            )
        })?;
        if config
            .expected_name
            .as_ref()
            .is_some_and(|name| name != &server_info.name)
            || config
                .expected_version
                .as_ref()
                .is_some_and(|version| version != &server_info.version)
        {
            return Err(Error::new(
                ErrorCode::ProtocolMismatch,
                "MCP server identity does not match owner configuration",
            ));
        }
        let mut identity = ProviderIdentity::external(
            SourceKind::ExternalMcp,
            &config.slug,
            &server_info.version,
        )?;
        identity.origin = format!("trusted-stdio-sha256:{}", config.sha256);
        identity.validate_external()?;
        let closed = CancellationToken::new();
        let cancel = service.cancellation_token();
        let provider = Arc::new(Self {
            identity,
            peer,
            imported: RwLock::new(BTreeMap::new()),
            signals: signals.clone(),
            closed: closed.clone(),
            service_cancel: Mutex::new(Some(cancel)),
            request_timeout: Duration::from_millis(config.request_timeout_ms),
        });
        tokio::spawn(async move {
            let _ = service.waiting().await;
            closed.cancel();
            let _ = signals.send(ProviderSignal::Disconnected);
        });
        if let Err(error) = provider.refresh_tools().await {
            let _ = provider.shutdown().await;
            return Err(error);
        }
        Ok(provider)
    }

    fn safe_description(remote: Option<&str>, upstream: &str) -> String {
        let text: String = remote
            .unwrap_or("No upstream description supplied")
            .chars()
            .map(|c| if c.is_control() { '�' } else { c })
            .take(8_000)
            .collect();
        format!("Imported MCP tool {upstream:?}. Remote description (untrusted data): {text}")
    }

    fn capability_name(&self, upstream: &str) -> Result<String> {
        if upstream.is_empty() || upstream.len() > 128 || upstream.chars().any(char::is_control) {
            return Err(Error::invalid("MCP tool name exceeds its bounds"));
        }
        let digest = format!("{:x}", Sha256::digest(upstream.as_bytes()));
        let mut normalized = String::new();
        for ch in upstream.chars() {
            let ch = ch.to_ascii_lowercase();
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-') {
                normalized.push(ch);
            } else {
                normalized.push('-');
            }
        }
        while normalized.contains("--") {
            normalized = normalized.replace("--", "-");
        }
        let normalized = normalized.trim_matches('-');
        let normalized = if normalized.is_empty() {
            "tool"
        } else {
            normalized
        };
        let suffix_budget = 128usize
            .saturating_sub(self.identity.namespace.len())
            .saturating_sub(13);
        if suffix_budget < 1 {
            return Err(Error::invalid("MCP provider namespace is too long"));
        }
        let prefix: String = normalized.chars().take(suffix_budget).collect();
        Ok(format!(
            "{}{}-{}",
            self.identity.namespace,
            prefix,
            &digest[..12]
        ))
    }

    async fn refresh_tools(&self) -> Result<Vec<ProvidedCapability>> {
        let tools = tokio::time::timeout(Duration::from_secs(10), self.peer.list_all_tools())
            .await
            .map_err(|_| Error::new(ErrorCode::Timeout, "MCP tools/list timed out"))?
            .map_err(|_| Error::new(ErrorCode::BackendFailed, "MCP tools/list failed"))?;
        if tools.len() > 512 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "MCP upstream exposes too many tools",
            ));
        }
        let mut names = BTreeSet::new();
        let mut capabilities = Vec::with_capacity(tools.len());
        let mut imported = BTreeMap::new();
        for tool in tools {
            let upstream = tool.name.to_string();
            if !names.insert(upstream.clone()) {
                return Err(Error::new(
                    ErrorCode::Conflict,
                    "MCP upstream returned duplicate tool names",
                ));
            }
            let name = self.capability_name(&upstream)?;
            let input_schema = Value::Object((*tool.input_schema).clone());
            let output_schema = tool
                .output_schema
                .as_ref()
                .map(|schema| Value::Object((**schema).clone()))
                .unwrap_or_else(|| json!({}));
            let descriptor = CommandDescriptor {
                name: name.clone(),
                version: self.identity.version.clone(),
                description: Self::safe_description(tool.description.as_deref(), &upstream),
                input_schema,
                output_schema,
                requires: vec![self.identity.id.clone()],
                risk: Risk::PrivilegeSensitive,
                idempotency: Idempotency::NonIdempotent,
                timeout_ms: self.request_timeout.as_millis() as u64,
                dry_run: false,
                interactive_consent: true,
                backends: vec![self.identity.id.clone()],
            };
            let digest = descriptor_digest(&descriptor)?;
            imported.insert(
                name,
                ImportedTool {
                    upstream_name: upstream.clone(),
                    descriptor_sha256: digest,
                    structured_output: tool.output_schema.is_some(),
                },
            );
            capabilities.push(ProvidedCapability {
                descriptor,
                aliases: vec![upstream],
                tags: vec!["mcp".into(), "external".into()],
                object_types: vec![],
            });
        }
        *self.imported.write().await = imported;
        Ok(capabilities)
    }

    fn transport_error(message: &'static str) -> Error {
        Error::new(ErrorCode::BackendFailed, message).uncertain()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct UpstreamDoctor {
    pub provider: String,
    pub namespace: String,
    pub server_version: String,
    pub capabilities: usize,
}

pub async fn doctor_stdio(config: StdioUpstreamConfig) -> Result<UpstreamDoctor> {
    let provider = ExternalMcpProvider::connect_trusted_stdio(config).await?;
    let result = async {
        let capabilities = Provider::capabilities(provider.as_ref()).await?;
        Ok(UpstreamDoctor {
            provider: provider.identity.id.clone(),
            namespace: provider.identity.namespace.clone(),
            server_version: provider.identity.version.clone(),
            capabilities: capabilities.len(),
        })
    }
    .await;
    let shutdown = Provider::shutdown(provider.as_ref()).await;
    match (result, shutdown) {
        (Ok(result), Ok(())) => Ok(result),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
    }
}

#[async_trait]
impl Provider for ExternalMcpProvider {
    fn identity(&self) -> &ProviderIdentity {
        &self.identity
    }

    fn supports(&self, command: &str) -> bool {
        if self.closed.is_cancelled() || self.peer.is_transport_closed() {
            return false;
        }
        self.imported
            .try_read()
            .is_ok_and(|tools| tools.contains_key(command))
    }

    async fn capabilities(&self) -> Result<Vec<ProvidedCapability>> {
        self.refresh_tools().await
    }

    async fn probe(&self) -> Vec<Feature> {
        let usable = !self.closed.is_cancelled() && !self.peer.is_transport_closed();
        self.imported
            .read()
            .await
            .keys()
            .take(512)
            .map(|name| Feature {
                backend: self.identity.id.clone(),
                capability: name.clone(),
                status: if usable {
                    CapabilityStatus::Supported
                } else {
                    CapabilityStatus::Unavailable
                },
                reason: "Owner-configured external MCP provider operation".into(),
                remediation:
                    "Inspect the MCP provider connection; availability is not authorization".into(),
            })
            .collect()
    }

    fn interfaces(&self) -> ProviderInterfaces {
        ProviderInterfaces {
            dynamic_capabilities: true,
            cooperative_cancellation: true,
            events: true,
            progress: false,
            artifacts: false,
            health: true,
        }
    }

    fn events(&self) -> Option<broadcast::Receiver<ProviderSignal>> {
        Some(self.signals.subscribe())
    }

    fn closed(&self) -> Option<CancellationToken> {
        Some(self.closed.clone())
    }

    async fn execute(
        &self,
        context: &Context,
        descriptor: &CommandDescriptor,
        args: &Value,
    ) -> Result<Value> {
        let imported = self
            .imported
            .read()
            .await
            .get(&descriptor.name)
            .cloned()
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::Conflict,
                    "Imported MCP tool is no longer current",
                )
            })?;
        if descriptor_digest(descriptor)? != imported.descriptor_sha256 {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Imported MCP tool descriptor changed; rediscover before executing",
            ));
        }
        let arguments = args
            .as_object()
            .cloned()
            .ok_or_else(|| Error::invalid("MCP tool arguments must be a JSON object"))?;
        let params = CallToolRequestParams::new(imported.upstream_name).with_arguments(arguments);
        let request = ClientRequest::CallToolRequest(CallToolRequest::new(params));
        let mut handle = self
            .peer
            .send_cancellable_request(request, PeerRequestOptions::no_options())
            .await
            .map_err(|_| Self::transport_error("MCP tools/call could not be dispatched"))?;
        let response = tokio::select! {
            response = &mut handle.rx => {
                response
                    .map_err(|_| Self::transport_error("MCP tools/call response channel closed"))?
                    .map_err(|_| Self::transport_error("MCP tools/call failed"))?
            }
            _ = context.cancellation.cancelled() => {
                let _ = handle.cancel(Some("Semwright command cancelled".into())).await;
                return Err(Error::new(
                    ErrorCode::Cancelled,
                    "MCP tool call cancelled; inspect upstream state before retrying",
                ).uncertain());
            }
        };
        let result = match response {
            ServerResult::CallToolResult(result) => result,
            ServerResult::InputRequiredResult(_) => {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "Federated MCP input-required rounds are not enabled",
                ));
            }
            ServerResult::CreateTaskResult(_) => {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "Federated MCP task results are not enabled yet",
                ));
            }
            _ => {
                return Err(Self::transport_error(
                    "MCP tools/call returned an unexpected response",
                ));
            }
        };
        if result.is_error == Some(true) {
            return Err(Error::new(
                ErrorCode::BackendFailed,
                "Upstream MCP tool reported an error",
            )
            .uncertain());
        }
        if imported.structured_output {
            return result.structured_content.ok_or_else(|| {
                Error::new(
                    ErrorCode::BackendFailed,
                    "Upstream MCP tool omitted required structuredContent",
                )
                .uncertain()
            });
        }
        Ok(result
            .structured_content
            .unwrap_or_else(|| serde_json::to_value(result.content).unwrap_or_else(|_| json!([]))))
    }

    async fn shutdown(&self) -> Result<()> {
        let cancel = self
            .service_cancel
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "MCP shutdown lock poisoned"))?
            .take();
        if let Some(cancel) = cancel {
            cancel.cancel();
        }
        if !self.closed.is_cancelled()
            && tokio::time::timeout(Duration::from_secs(4), self.closed.cancelled())
                .await
                .is_err()
        {
            return Err(Error::new(
                ErrorCode::Timeout,
                "MCP child transport did not close within the shutdown budget",
            ));
        }
        Ok(())
    }
}
