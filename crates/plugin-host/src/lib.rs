//! Sandboxed, digest-pinned, out-of-process plugin host. No unsandboxed fallback.
#[cfg(feature = "test-tools")]
pub mod adversarial_fixture;
use semwright_plugin_sdk::Manifest;
#[cfg(unix)]
use semwright_plugin_sdk::{PLUGIN_PROTOCOL_VERSION, Request, Response, commands_digest};
use semwright_policy::FilesystemGrant;
use semwright_protocol::private_directory;
#[cfg(unix)]
use semwright_protocol::{read_frame, write_frame};
use semwright_types::*;
use serde_json::{Value, json};
#[cfg(all(test, unix))]
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::RwLock,
};
#[cfg(unix)]
use std::{
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
};
use tokio_util::sync::CancellationToken;

pub struct Host {
    manifests: RwLock<BTreeMap<String, Manifest>>,
    roots: Vec<FilesystemGrant>,
    #[cfg(unix)]
    state: PathBuf,
    helper: PathBuf,
    allow_network: bool,
}
impl Host {
    pub fn new(
        state: PathBuf,
        helper: PathBuf,
        roots: Vec<FilesystemGrant>,
        allow_network: bool,
    ) -> Result<Self> {
        private_directory(&state)?;
        Ok(Self {
            manifests: RwLock::new(BTreeMap::new()),
            roots,
            #[cfg(unix)]
            state,
            helper,
            allow_network,
        })
    }
    pub fn validate_install(&self, manifest: &Manifest) -> Result<()> {
        manifest.validate()?;
        if manifest.network && !self.allow_network {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Plugin requests network but owner configuration denies it",
            ));
        }
        for mount in &manifest.mounts {
            let grant = self
                .roots
                .iter()
                .find(|r| r.name == mount.root)
                .ok_or_else(|| {
                    Error::new(
                        ErrorCode::PolicyDenied,
                        "Plugin mount has no named filesystem grant",
                    )
                })?;
            if !grant.read || (!mount.read_only && !grant.write) {
                return Err(Error::new(
                    ErrorCode::PolicyDenied,
                    "Plugin mount exceeds root permissions",
                ));
            }
            if std::fs::canonicalize(&grant.path)? != grant.path {
                return Err(Error::new(
                    ErrorCode::PolicyDenied,
                    "Plugin roots must be canonical paths",
                ));
            }
        }
        verify_executable(&manifest.executable, &manifest.sha256).map(|_| ())
    }
    pub fn install(&self, manifest: Manifest) -> Result<()> {
        self.validate_install(&manifest)?;
        let mut manifests = self
            .manifests
            .write()
            .map_err(|_| Error::new(ErrorCode::Internal, "Plugin registry lock poisoned"))?;
        if manifests.contains_key(&manifest.name) {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Plugin is already installed; remove explicitly before replacing",
            ));
        }
        manifests.insert(manifest.name.clone(), manifest);
        Ok(())
    }
    pub fn remove(&self, name: &str) -> Result<Manifest> {
        self.manifests
            .write()
            .map_err(|_| Error::new(ErrorCode::Internal, "Plugin registry lock poisoned"))?
            .remove(name)
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "Plugin is not installed"))
    }
    pub fn describe(&self, name: &str) -> Result<Manifest> {
        self.manifests
            .read()
            .map_err(|_| Error::new(ErrorCode::Internal, "Plugin registry lock poisoned"))?
            .get(name)
            .cloned()
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "Plugin is not installed"))
    }
    pub fn list(&self) -> Result<Value> {
        let manifests = self
            .manifests
            .read()
            .map_err(|_| Error::new(ErrorCode::Internal, "Plugin registry lock poisoned"))?;
        let native = semwright_platform_services::sandbox_diagnostics(&self.helper);
        let rows:Vec<_>=manifests.values().map(|m|json!({"name":m.name,"version":m.version,"sha256":m.sha256,"commands":m.commands.iter().map(|c|&c.name).collect::<Vec<_>>(),"sandbox":semwright_platform_services::sandbox_mechanism(),"network":m.network})).collect();
        Ok(
            json!({"plugins":rows,"sandbox":native,"bubblewrap_present":native.get("bubblewrap_present").and_then(Value::as_bool).unwrap_or(false),"sandbox_helper_present":native["helper_present"],"lifecycle":"one sandboxed process per invocation","stderr":"discarded"}),
        )
    }
    pub async fn execute(
        &self,
        command: &str,
        args: Value,
        cancellation: CancellationToken,
    ) -> Result<Value> {
        let name = command
            .strip_prefix("plugin.")
            .and_then(|c| c.split('.').next())
            .ok_or_else(|| Error::invalid("Plugin command namespace required"))?;
        let manifest = self.describe(name)?;
        let descriptor = manifest
            .commands
            .iter()
            .find(|c| c.name == command)
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "Plugin command is not declared"))?;
        #[cfg(target_os = "windows")]
        {
            let _ = (&descriptor, &args, &cancellation);
            Err(Error::new(
                ErrorCode::SandboxDenied,
                "Windows arbitrary plugin execution is fail-closed until secure pre-exec containment is implemented",
            ))
        }
        #[cfg(unix)]
        {
            if !semwright_platform_services::sandbox_available(&self.helper) {
                return Err(Error::new(
                    ErrorCode::SandboxDenied,
                    "Bubblewrap and semwright-sandbox are required; refusing unsandboxed execution",
                ));
            }
            let bytes = verify_executable(&manifest.executable, &manifest.sha256)?;
            let staged = self.state.join(format!("binary-{}", unique_id()));
            let cleanup = StagedFile(staged.clone());
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&staged)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            file.set_permissions(std::fs::Permissions::from_mode(0o500))?;
            drop(file);

            use semwright_platform_api::launch::{Mount, SandboxKind, SandboxSpec};
            let mounts = manifest
                .mounts
                .iter()
                .map(|m| {
                    let grant = self
                        .roots
                        .iter()
                        .find(|g| g.name == m.root)
                        .ok_or_else(|| {
                            Error::new(ErrorCode::PolicyDenied, "Plugin grant disappeared")
                        })?;
                    Ok(Mount {
                        source: grant.path.clone(),
                        class: semwright_platform_api::launch::MountClass::Workspace,
                        logical_name: m.root.clone(),
                        read_only: m.read_only,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            let mut process = semwright_platform_services::sandbox_command(&SandboxSpec {
                kind: SandboxKind::Plugin,
                staged_executable: staged.clone(),
                helper: self.helper.clone(),
                mounts,
                args: vec![],
                network: manifest.network,
                limits: None,
            })?;
            if cancellation.is_cancelled() {
                return Err(Error::new(
                    ErrorCode::Cancelled,
                    "Plugin cancelled before spawn",
                ));
            }
            let mut child = process
                .spawn()
                .map_err(|_| Error::new(ErrorCode::SandboxDenied, "Sandbox launcher failed"))?;
            let mut input = child.stdin.take().ok_or_else(|| {
                Error::new(ErrorCode::PluginProtocolError, "Plugin stdin missing")
            })?;
            let mut output = child.stdout.take().ok_or_else(|| {
                Error::new(ErrorCode::PluginProtocolError, "Plugin stdout missing")
            })?;
            let id = unique_id();
            let expected_commands_sha256 = commands_digest(&manifest.commands)?;
            let conversation = async {
                write_frame(
                    &mut input,
                    &Request::Hello {
                        protocol: PLUGIN_PROTOCOL_VERSION,
                        name: name.into(),
                        version: manifest.version.clone(),
                        commands_sha256: expected_commands_sha256.clone(),
                    },
                )
                .await?;
                match read_frame::<_, Response>(&mut output).await? {
                    Response::Ready {
                        protocol: PLUGIN_PROTOCOL_VERSION,
                        name: reported,
                        version,
                        commands_sha256,
                    } if reported == name
                        && version == manifest.version
                        && commands_sha256 == expected_commands_sha256 => {}
                    _ => {
                        return Err(Error::new(
                            ErrorCode::PluginProtocolError,
                            "Plugin handshake mismatch",
                        ));
                    }
                }
                write_frame(
                    &mut input,
                    &Request::Execute {
                        id: id.clone(),
                        command: command.into(),
                        args,
                    },
                )
                .await?;
                let result = match read_frame::<_, Response>(&mut output).await? {
                    Response::Result {
                        id: reported,
                        value,
                    } if reported == id => Ok(value),
                    Response::Failure {
                        id: reported,
                        error,
                    } if reported == id => Err(Error::new(
                        error.code,
                        "Plugin reported an error; stderr and payload were redacted",
                    )),
                    _ => Err(Error::new(
                        ErrorCode::PluginProtocolError,
                        "Plugin replied with an unexpected id or message",
                    )),
                };
                let _ = write_frame(&mut input, &Request::Shutdown {}).await;
                result
            };
            let timeout = descriptor
                .timeout_ms
                .min(manifest.timeout_ms.unwrap_or(30000));
            let result = tokio::select! {
                _=cancellation.cancelled()=>Err(Error::new(ErrorCode::Cancelled,"Plugin cancelled").uncertain()),
                r=tokio::time::timeout(std::time::Duration::from_millis(timeout),conversation)=>r.unwrap_or_else(|_|Err(Error::new(ErrorCode::Timeout,"Plugin exceeded its timeout").uncertain())),
            };
            let _ = child.kill().await;
            let _ = child.wait().await;
            drop(cleanup);
            result.map_err(|e| {
                if e.code == ErrorCode::BackendFailed {
                    Error::new(
                        ErrorCode::SandboxDenied,
                        "Plugin could not establish its protocol; sandbox may be unsupported",
                    )
                    .uncertain()
                } else {
                    e
                }
            })
        }
    }
}
#[cfg(unix)]
struct StagedFile(PathBuf);
#[cfg(unix)]
impl Drop for StagedFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
fn verify_executable(path: &Path, digest: &str) -> Result<Vec<u8>> {
    semwright_platform_services::verify_executable(path, digest)
}
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn mismatched_digest_rejected() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("plugin");
        std::fs::write(&p, b"\x7fELFexample").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(verify_executable(&p, &"0".repeat(64)).is_err());
    }
    #[test]
    fn scripts_are_not_binary_plugins() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("plugin");
        let body = b"#!/bin/sh\necho bad";
        std::fs::write(&p, body).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(verify_executable(&p, &format!("{:x}", Sha256::digest(body))).is_err());
    }
    #[test]
    #[cfg(target_os = "linux")]
    fn exact_bytes_are_staged() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("plugin");
        let body = b"\x7fELFexample";
        std::fs::write(&p, body).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(
            verify_executable(&p, &format!("{:x}", Sha256::digest(body))).unwrap(),
            body
        );
    }
    #[test]
    fn writable_by_others_is_rejected_even_with_matching_digest() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("plugin");
        let body = b"\x7fELFexample";
        std::fs::write(&path, body).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o660)).unwrap();
        assert!(
            matches!(verify_executable(&path, &format!("{:x}", Sha256::digest(body))), Err(error) if error.code == ErrorCode::PermissionDenied)
        );
    }
}
