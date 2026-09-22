//! Sandboxed persistent application-driver host. Drivers are mounted into the broker as Providers.
use async_trait::async_trait;
use semwright_backend_api::{Context, ProvidedCapability, Provider, ProviderInterfaces};
use semwright_driver_sdk::{
    DRIVER_PROTOCOL_VERSION, Manifest, Request, Response, capabilities_digest, descriptor_digest,
};
use semwright_policy::FilesystemGrant;
use semwright_protocol::{read_frame, write_frame};
use semwright_types::{
    CapabilityStatus, CommandDescriptor, Error, ErrorCode, Feature, NativeTarget, ProviderIdentity,
    Result, unique_id,
};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};
use tokio::{
    process::{ChildStdin, ChildStdout, Command},
    sync::Mutex,
};
use tokio_util::sync::CancellationToken;

struct StagedFile(PathBuf);
impl Drop for StagedFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn verify_owned_elf(path: &Path, digest: &str) -> Result<Vec<u8>> {
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let meta = file.metadata()?;
    // SAFETY: getuid has no pointer arguments or preconditions.
    let uid = unsafe { libc::getuid() };
    if !meta.is_file()
        || meta.len() > 67_108_864
        || meta.mode() & 0o022 != 0
        || !(meta.uid() == uid || meta.uid() == 0)
    {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Driver executable must be an owned/root regular file, <=64 MiB and not writable by group/others",
        ));
    }
    let mut data = vec![];
    std::io::Read::by_ref(&mut file)
        .take(67_108_865)
        .read_to_end(&mut data)?;
    if data.len() > 67_108_864
        || format!("{:x}", Sha256::digest(&data)) != digest.to_ascii_lowercase()
    {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Driver executable SHA-256 does not match its manifest",
        ));
    }
    if !data.starts_with(b"ELF") {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "Driver host launches pinned ELF binaries only; interpreter selection must be explicit in a future transport",
        ));
    }
    Ok(data)
}

fn validate_owner_permissions(
    manifest: &Manifest,
    roots: &[FilesystemGrant],
    allow_network: bool,
) -> Result<()> {
    manifest.validate()?;
    if manifest.network && !allow_network {
        return Err(Error::new(
            ErrorCode::PolicyDenied,
            "Driver requests network but owner configuration denies it",
        ));
    }
    for mount in &manifest.mounts {
        let grant = roots
            .iter()
            .find(|grant| grant.name == mount.root)
            .ok_or_else(|| {
                Error::new(ErrorCode::PolicyDenied, "Driver mount has no owner grant")
            })?;
        if !grant.read || (!mount.read_only && !grant.write) {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Driver mount exceeds its filesystem grant",
            ));
        }
        if std::fs::canonicalize(&grant.path)? != grant.path {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Driver filesystem grants must be canonical paths",
            ));
        }
    }
    if manifest.interfaces.dynamic_capabilities
        || manifest.interfaces.cooperative_cancellation
        || manifest.interfaces.events
    {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "Driver protocol v1 host currently supports persistent capabilities and health; dynamic/events/cooperative cancellation require a later negotiated interface version",
        ));
    }
    Ok(())
}

struct Io {
    input: ChildStdin,
    output: ChildStdout,
}

async fn request<T: Serialize>(io: &mut Io, request: &T, timeout: Duration) -> Result<Response> {
    tokio::time::timeout(timeout, async {
        write_frame(&mut io.input, request).await?;
        read_frame::<_, Response>(&mut io.output).await
    })
    .await
    .map_err(|_| Error::new(ErrorCode::Timeout, "Driver protocol request timed out"))?
}

fn sandbox_command(
    manifest: &Manifest,
    staged: &Path,
    helper: &Path,
    roots: &[FilesystemGrant],
) -> Result<Command> {
    if !Path::new("/usr/bin/bwrap").is_file() || !helper.is_file() {
        return Err(Error::new(
            ErrorCode::SandboxDenied,
            "Bubblewrap and semwright-sandbox are required; refusing unsandboxed driver execution",
        ));
    }
    let mut process = Command::new("/usr/bin/bwrap");
    process.args([
        "--die-with-parent",
        "--new-session",
        "--unshare-all",
        "--clearenv",
        "--cap-drop",
        "ALL",
    ]);
    if manifest.network {
        process.arg("--share-net");
    }
    process.args([
        "--proc",
        "/proc",
        "--dev",
        "/dev",
        "--tmpfs",
        "/tmp",
        "--dir",
        "/home",
        "--dir",
        "/workspace",
        "--dir",
        "/plugin",
    ]);
    for runtime in ["/usr", "/lib", "/lib64"] {
        if Path::new(runtime).exists() {
            process.arg("--ro-bind").arg(runtime).arg(runtime);
        }
    }
    process.args(["--dir", "/etc"]);
    if Path::new("/etc/ld.so.cache").exists() {
        process.args(["--ro-bind", "/etc/ld.so.cache", "/etc/ld.so.cache"]);
    }
    process.arg("--ro-bind").arg(staged).arg("/plugin/bin");
    process.arg("--ro-bind").arg(helper).arg("/plugin/sandbox");
    let mut writable = vec![];
    for mount in &manifest.mounts {
        let grant = roots
            .iter()
            .find(|root| root.name == mount.root)
            .ok_or_else(|| Error::new(ErrorCode::PolicyDenied, "Driver root grant was removed"))?;
        let destination = format!("/workspace/{}", mount.root);
        process
            .arg(if mount.read_only {
                "--ro-bind"
            } else {
                "--bind"
            })
            .arg(&grant.path)
            .arg(&destination);
        if !mount.read_only {
            writable.push(destination);
        }
    }
    process.args([
        "--setenv",
        "HOME",
        "/home",
        "--setenv",
        "PATH",
        "/usr/bin:/bin",
        "--setenv",
        "LANG",
        "C.UTF-8",
        "--chdir",
        "/tmp",
        "--",
        "/plugin/sandbox",
    ]);
    for path in writable {
        process.arg("--write-root").arg(path);
    }
    process.arg("--").arg("/plugin/bin");
    process
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    Ok(process)
}

pub struct DriverProvider {
    identity: ProviderIdentity,
    manifest: Manifest,
    capabilities: Vec<ProvidedCapability>,
    descriptor_digests: BTreeMap<String, String>,
    io: Mutex<Io>,
    closed: CancellationToken,
    terminate: CancellationToken,
    _staged: Arc<StagedFile>,
}

impl DriverProvider {
    pub async fn connect(
        manifest: Manifest,
        state: &Path,
        helper: &Path,
        roots: &[FilesystemGrant],
        allow_network: bool,
    ) -> Result<Arc<Self>> {
        validate_owner_permissions(&manifest, roots, allow_network)?;
        semwright_protocol::private_directory(state)?;
        let identity = manifest.identity()?;
        let bytes = verify_owned_elf(&manifest.executable, &manifest.sha256)?;
        let staged_path = state.join(format!("driver-{}", unique_id()));
        let staged = Arc::new(StagedFile(staged_path.clone()));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&staged_path)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        file.set_permissions(std::fs::Permissions::from_mode(0o500))?;
        drop(file);

        let mut command = sandbox_command(&manifest, &staged_path, helper, roots)?;
        let mut child = command
            .spawn()
            .map_err(|_| Error::new(ErrorCode::SandboxDenied, "Driver sandbox failed to start"))?;
        let input = child
            .stdin
            .take()
            .ok_or_else(|| Error::new(ErrorCode::ProtocolMismatch, "Driver stdin missing"))?;
        let output = child
            .stdout
            .take()
            .ok_or_else(|| Error::new(ErrorCode::ProtocolMismatch, "Driver stdout missing"))?;
        let mut io = Io { input, output };
        let timeout = Duration::from_millis(manifest.request_timeout_ms.min(30_000));
        let hello = request(
            &mut io,
            &Request::Hello {
                protocol: DRIVER_PROTOCOL_VERSION,
                provider: identity.clone(),
                executable_sha256: manifest.sha256.to_ascii_lowercase(),
            },
            timeout,
        )
        .await?;
        match hello {
            Response::Ready {
                protocol: DRIVER_PROTOCOL_VERSION,
                id,
                version,
            } if id == manifest.id && version == manifest.version => {}
            _ => {
                let _ = child.kill().await;
                return Err(Error::new(
                    ErrorCode::ProtocolMismatch,
                    "Driver handshake did not attest the expected identity/version",
                ));
            }
        }

        let id = unique_id();
        let response = request(&mut io, &Request::Capabilities { id: id.clone() }, timeout).await?;
        let capabilities = match response {
            Response::Capabilities {
                id: response_id,
                capabilities,
                digest,
            } if response_id == id && digest == capabilities_digest(&capabilities)? => capabilities,
            _ => {
                let _ = child.kill().await;
                return Err(Error::new(
                    ErrorCode::ProtocolMismatch,
                    "Driver capability attestation mismatch",
                ));
            }
        };
        if capabilities.is_empty() || capabilities.len() > 2048 {
            let _ = child.kill().await;
            return Err(Error::invalid(
                "Driver returned an invalid capability count",
            ));
        }
        let mut provided = Vec::with_capacity(capabilities.len());
        let mut digests = BTreeMap::new();
        for capability in capabilities {
            capability.validate_for(&identity)?;
            let digest = descriptor_digest(&capability.descriptor)?;
            if digests
                .insert(capability.descriptor.name.clone(), digest)
                .is_some()
            {
                let _ = child.kill().await;
                return Err(Error::new(
                    ErrorCode::Conflict,
                    "Driver returned duplicate capability names",
                ));
            }
            provided.push(ProvidedCapability {
                descriptor: capability.descriptor,
                aliases: capability.aliases,
                tags: capability.tags,
                object_types: capability.object_types,
            });
        }

        let health_id = unique_id();
        match request(
            &mut io,
            &Request::Health {
                id: health_id.clone(),
            },
            timeout,
        )
        .await?
        {
            Response::Healthy { id, .. } if id == health_id => {}
            _ => {
                let _ = child.kill().await;
                return Err(Error::new(
                    ErrorCode::ProtocolMismatch,
                    "Driver health handshake failed",
                ));
            }
        }

        let closed = CancellationToken::new();
        let terminate = CancellationToken::new();
        let monitor_closed = closed.clone();
        let monitor_terminate = terminate.clone();
        let monitor_staged = staged.clone();
        tokio::spawn(async move {
            tokio::select! {
                _ = monitor_terminate.cancelled() => {
                    let _ = child.kill().await;
                }
                _ = child.wait() => {}
            }
            monitor_closed.cancel();
            drop(monitor_staged);
        });

        Ok(Arc::new(Self {
            identity,
            manifest,
            capabilities: provided,
            descriptor_digests: digests,
            io: Mutex::new(io),
            closed,
            terminate,
            _staged: staged,
        }))
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }
}

#[async_trait]
impl Provider for DriverProvider {
    fn identity(&self) -> &ProviderIdentity {
        &self.identity
    }
    fn supports(&self, command: &str) -> bool {
        self.descriptor_digests.contains_key(command)
    }
    async fn capabilities(&self) -> Result<Vec<ProvidedCapability>> {
        Ok(self.capabilities.clone())
    }
    async fn probe(&self) -> Vec<Feature> {
        self.capabilities
            .iter()
            .map(|capability| Feature {
                backend: self.identity.id.clone(),
                capability: capability.descriptor.name.clone(),
                status: if self.closed.is_cancelled() {
                    CapabilityStatus::Unavailable
                } else {
                    CapabilityStatus::Supported
                },
                reason: if self.closed.is_cancelled() {
                    "Driver process disconnected".into()
                } else {
                    "Pinned sandboxed driver is connected".into()
                },
                remediation: if self.closed.is_cancelled() {
                    "Restart the owner-configured driver".into()
                } else {
                    String::new()
                },
            })
            .collect()
    }
    fn operation_feature(&self, command: &str) -> Option<String> {
        self.supports(command).then(|| command.to_owned())
    }
    fn interfaces(&self) -> ProviderInterfaces {
        ProviderInterfaces {
            dynamic_capabilities: false,
            cooperative_cancellation: false,
            events: false,
            health: self.manifest.interfaces.health,
        }
    }
    fn closed(&self) -> Option<CancellationToken> {
        Some(self.closed.clone())
    }
    async fn execute(
        &self,
        _context: &Context,
        descriptor: &CommandDescriptor,
        args: &Value,
    ) -> Result<Value> {
        if self.closed.is_cancelled() {
            return Err(Error::unavailable("Driver process is disconnected"));
        }
        let expected = self
            .descriptor_digests
            .get(&descriptor.name)
            .ok_or_else(|| {
                Error::new(ErrorCode::NotFound, "Driver capability is not registered")
            })?;
        let actual = descriptor_digest(descriptor)?;
        if &actual != expected {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Pinned driver capability descriptor changed",
            ));
        }
        let id = unique_id();
        let mut io = self.io.lock().await;
        let response = request(
            &mut io,
            &Request::Execute {
                id: id.clone(),
                command: descriptor.name.clone(),
                descriptor_sha256: actual,
                args: args.clone(),
            },
            Duration::from_millis(descriptor.timeout_ms.min(self.manifest.request_timeout_ms)),
        )
        .await;
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                self.terminate.cancel();
                return Err(error.uncertain());
            }
        };
        match response {
            Response::Result {
                id: response_id,
                value,
            } if response_id == id => Ok(value),
            Response::Failure {
                id: response_id,
                error,
            } if response_id == id => Err(Error::new(
                error.code,
                "Driver reported an error; untrusted payload was redacted",
            )
            .uncertain()),
            _ => {
                self.terminate.cancel();
                Err(Error::new(
                    ErrorCode::ProtocolMismatch,
                    "Driver returned an unexpected response",
                )
                .uncertain())
            }
        }
    }
    async fn validate(&self, _target: &NativeTarget) -> Result<()> {
        Err(Error::new(
            ErrorCode::Unsupported,
            "Driver protocol v1 does not expose native reference validation",
        ))
    }
    async fn shutdown(&self) -> Result<()> {
        if self.closed.is_cancelled() {
            return Ok(());
        }
        let id = unique_id();
        let mut io = self.io.lock().await;
        let response = request(
            &mut io,
            &Request::Shutdown { id: id.clone() },
            Duration::from_secs(3),
        )
        .await?;
        if !matches!(response, Response::Shutdown { id: response_id } if response_id == id) {
            return Err(Error::new(
                ErrorCode::ProtocolMismatch,
                "Driver shutdown acknowledgement mismatch",
            ));
        }
        drop(io);
        if tokio::time::timeout(Duration::from_secs(3), self.closed.cancelled())
            .await
            .is_err()
        {
            self.terminate.cancel();
            tokio::time::timeout(Duration::from_secs(2), self.closed.cancelled())
                .await
                .map_err(|_| Error::new(ErrorCode::Timeout, "Driver could not be terminated"))?;
            return Err(Error::new(
                ErrorCode::Timeout,
                "Driver did not exit after shutdown acknowledgement",
            ));
        }
        Ok(())
    }
}
impl Drop for DriverProvider {
    fn drop(&mut self) {
        self.terminate.cancel();
    }
}

#[derive(Debug, Serialize)]
pub struct ConformanceReport {
    pub provider: String,
    pub namespace: String,
    pub version: String,
    pub capabilities: usize,
    pub sandboxed: bool,
    pub persistent_process: bool,
    pub health: bool,
    pub executed_read_only: bool,
    pub shutdown: bool,
}

pub async fn conformance(
    manifest: Manifest,
    state: &Path,
    helper: &Path,
    roots: &[FilesystemGrant],
    allow_network: bool,
) -> Result<ConformanceReport> {
    let provider = DriverProvider::connect(manifest, state, helper, roots, allow_network).await?;
    let capabilities = Provider::capabilities(provider.as_ref()).await?;
    let provider_id = provider.identity.id.clone();
    let namespace = provider.identity.namespace.clone();
    let version = provider.identity.version.clone();
    let safe = capabilities.iter().find(|capability| {
        capability.descriptor.risk == semwright_types::Risk::ReadOnly
            && capability.descriptor.dry_run
            && capability
                .descriptor
                .input_schema
                .get("type")
                .and_then(Value::as_str)
                == Some("object")
            && capability
                .descriptor
                .input_schema
                .get("required")
                .and_then(Value::as_array)
                .is_none_or(Vec::is_empty)
    });
    let executed_read_only = if let Some(capability) = safe {
        let context = Context {
            session: "driver-conformance".into(),
            cancellation: CancellationToken::new(),
        };
        Provider::execute(
            provider.as_ref(),
            &context,
            &capability.descriptor,
            &json!({}),
        )
        .await?;
        true
    } else {
        false
    };
    Provider::shutdown(provider.as_ref()).await?;
    Ok(ConformanceReport {
        provider: provider_id,
        namespace,
        version,
        capabilities: capabilities.len(),
        sandboxed: true,
        persistent_process: true,
        health: true,
        executed_read_only,
        shutdown: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn manifest() -> Manifest {
        Manifest {
            manifest_version: 1,
            protocol: 1,
            id: "fixture".into(),
            version: "1".into(),
            publisher: "tests".into(),
            executable: "/tmp/fixture".into(),
            sha256: "a".repeat(64),
            application: semwright_driver_sdk::ApplicationMatch {
                desktop_id: Some("org.example.Fixture".into()),
                ..Default::default()
            },
            transport: semwright_driver_sdk::Transport::StdioV1,
            mounts: vec![],
            network: false,
            request_timeout_ms: 1000,
            interfaces: semwright_driver_sdk::DriverInterfaces::default(),
        }
    }

    #[test]
    fn owner_network_consent_and_interface_negotiation_fail_closed() {
        let mut network = manifest();
        network.network = true;
        assert!(matches!(
            validate_owner_permissions(&network, &[], false),
            Err(error) if error.code == ErrorCode::PolicyDenied
        ));
        assert!(validate_owner_permissions(&network, &[], true).is_ok());

        let mut dynamic = manifest();
        dynamic.interfaces.dynamic_capabilities = true;
        assert!(matches!(
            validate_owner_permissions(&dynamic, &[], false),
            Err(error) if error.code == ErrorCode::Unsupported
        ));
    }

    #[test]
    fn writable_or_wrong_digest_executable_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("driver");
        let body = b"ELFfixture";
        std::fs::write(&path, body).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o660)).unwrap();
        assert!(verify_owned_elf(&path, &format!("{:x}", Sha256::digest(body))).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(verify_owned_elf(&path, &"0".repeat(64)).is_err());
    }
}
