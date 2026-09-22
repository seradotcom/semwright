//! Versioned application-driver contract. Drivers are providers; this crate has no broker or MCP authority.
use async_trait::async_trait;
use semwright_protocol::{read_frame, write_frame};
use semwright_types::provider::canonical_slug;
use semwright_types::{CommandDescriptor, Error, ErrorCode, ProviderIdentity, Result, SourceKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    path::{Component, Path, PathBuf},
};

pub const DRIVER_MANIFEST_VERSION: u32 = 1;
pub const DRIVER_PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    StdioV1,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DriverInterfaces {
    #[serde(default)]
    pub dynamic_capabilities: bool,
    #[serde(default)]
    pub cooperative_cancellation: bool,
    #[serde(default)]
    pub events: bool,
    #[serde(default = "default_health")]
    pub health: bool,
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
    pub network: bool,
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
            || self.protocol != DRIVER_PROTOCOL_VERSION
        {
            return Err(Error::new(
                ErrorCode::ProtocolMismatch,
                "Unsupported driver manifest or protocol version",
            ));
        }
        self.identity()?;
        self.application.validate()?;
        self.resources.validate()?;
        if self.publisher.is_empty()
            || self.publisher.len() > 128
            || self.publisher.chars().any(char::is_control)
            || !self.executable.is_absolute()
            || self.sha256.len() != 64
            || !self.sha256.bytes().all(|b| b.is_ascii_hexdigit())
            || self.mounts.len() > 16
            || self.system_config.len() > 8
            || self.request_timeout_ms == 0
            || self.request_timeout_ms > 300_000
        {
            return Err(Error::invalid("Driver manifest exceeds bounds"));
        }
        let mut roots = BTreeSet::new();
        for mount in &self.mounts {
            if !canonical_slug(&mount.root) || !roots.insert(&mount.root) {
                return Err(Error::invalid(
                    "Driver mount roots must be unique canonical policy-grant names",
                ));
            }
        }
        let mut destinations = BTreeSet::new();
        for mount in &self.system_config {
            mount.validate()?;
            if !roots.insert(&mount.root) || !destinations.insert(&mount.destination) {
                return Err(Error::invalid(
                    "Driver system config roots and destinations must be unique",
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
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Hello {
        protocol: u32,
        provider: ProviderIdentity,
        executable_sha256: String,
    },
    Capabilities {
        id: String,
    },
    Execute {
        id: String,
        command: String,
        descriptor_sha256: String,
        args: Value,
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
    Healthy {
        id: String,
        details: Value,
    },
    Shutdown {
        id: String,
    },
}

#[async_trait]
pub trait Driver: Send {
    fn id(&self) -> &str;
    fn version(&self) -> &str;
    async fn capabilities(&mut self) -> Result<Vec<Capability>>;
    async fn execute(
        &mut self,
        command: &str,
        descriptor_sha256: &str,
        args: Value,
    ) -> Result<Value>;
    async fn health(&mut self) -> Result<Value> {
        Ok(serde_json::json!({"healthy":true}))
    }
}

pub async fn serve<D: Driver>(mut driver: D) -> Result<()> {
    let mut input = tokio::io::stdin();
    let mut output = tokio::io::stdout();
    let owner_identity = match read_frame::<_, Request>(&mut input).await? {
        Request::Hello {
            protocol: DRIVER_PROTOCOL_VERSION,
            provider,
            executable_sha256,
        } if provider.kind == SourceKind::Driver
            && provider.id == format!("driver:{}", driver.id())
            && provider.version == driver.version()
            && executable_sha256.len() == 64 =>
        {
            provider
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
            protocol: DRIVER_PROTOCOL_VERSION,
            id: driver.id().into(),
            version: driver.version().into(),
        },
    )
    .await?;
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
            Request::Hello { .. } => {
                return Err(Error::new(
                    ErrorCode::ProtocolMismatch,
                    "Driver hello may only occur once",
                ));
            }
        }
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
            network: false,
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
        });
        duplicate_root.system_config.push(SystemConfigMount {
            root: "same".into(),
            destination: "/etc/example".into(),
        });
        assert!(duplicate_root.validate().is_err());
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
