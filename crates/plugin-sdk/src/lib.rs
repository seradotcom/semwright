//! Semwright plugin protocol v2. Stdio is reserved for bounded framed JSON.
use semwright_protocol::{read_frame, write_frame};
use semwright_types::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, path::PathBuf};

pub const PLUGIN_PROTOCOL_VERSION: u32 = 2;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub protocol: u32,
    pub name: String,
    pub version: String,
    pub executable: PathBuf,
    pub sha256: String,
    pub commands: Vec<CommandDescriptor>,
    #[serde(default)]
    pub mounts: Vec<Mount>,
    #[serde(default)]
    pub network: bool,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mount {
    pub root: String,
    pub read_only: bool,
}
impl Manifest {
    pub fn validate(&self) -> Result<()> {
        if self.protocol != PLUGIN_PROTOCOL_VERSION {
            return Err(Error::new(
                ErrorCode::ProtocolMismatch,
                "Plugin protocol must match the current attested protocol",
            ));
        }
        if self.name.is_empty()
            || self.name.len() > 40
            || !self
                .name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(Error::invalid("Plugin name must be a lowercase identifier"));
        }
        if !self.executable.is_absolute()
            || self.sha256.len() != 64
            || !self.sha256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(Error::invalid(
                "Plugins require an absolute executable and pinned SHA-256 digest",
            ));
        }
        if self.commands.is_empty()
            || self.commands.len() > 64
            || self.mounts.len() > 16
            || self.timeout_ms.unwrap_or(30000) > 300000
        {
            return Err(Error::invalid("Plugin manifest exceeds bounds"));
        }
        let mut names = BTreeSet::new();
        let mut roots = BTreeSet::new();
        for mount in &self.mounts {
            if mount.root.is_empty()
                || mount.root.len() > 64
                || !mount
                    .root
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
                || !roots.insert(&mount.root)
            {
                return Err(Error::invalid(
                    "Plugin mount roots must be unique named policy grants",
                ));
            }
        }
        for command in &self.commands {
            let prefix = format!("plugin.{}.", self.name);
            if !command.name.starts_with(&prefix) || !names.insert(&command.name) {
                return Err(Error::invalid(
                    "Plugin commands must use their own namespace",
                ));
            }
            let required = format!("plugin:{}", self.name);
            if !command.requires.contains(&required) || command.backends != ["plugin"] {
                return Err(Error::invalid(
                    "Plugin commands must require the plugin-specific capability and use the plugin backend",
                ));
            }
            if self.mounts.iter().any(|m| !m.read_only) && !command.risk.mutates() {
                return Err(Error::invalid(
                    "A plugin with writable mounts cannot claim read-only commands",
                ));
            }
        }
        Ok(())
    }
}
pub fn commands_digest(commands: &[CommandDescriptor]) -> Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(commands)?)
    ))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Hello {
        protocol: u32,
        name: String,
        version: String,
        commands_sha256: String,
    },
    Execute {
        id: String,
        command: String,
        args: Value,
    },
    Health {},
    Shutdown {},
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Response {
    Ready {
        protocol: u32,
        name: String,
        version: String,
        commands_sha256: String,
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
        protocol: u32,
    },
}
/// SDK runner. Plugin authors supply a typed dispatcher; arbitrary code evaluation
/// is not part of the SDK contract. Panics terminate only the sandboxed child.
pub async fn serve<F>(
    name: &str,
    version: &str,
    commands: &[CommandDescriptor],
    mut dispatch: F,
) -> Result<()>
where
    F: FnMut(&str, Value) -> Result<Value>,
{
    let mut input = tokio::io::stdin();
    let mut output = tokio::io::stdout();
    let local_digest = commands_digest(commands)?;
    match read_frame::<_, Request>(&mut input).await? {
        Request::Hello {
            protocol: PLUGIN_PROTOCOL_VERSION,
            name: expected,
            version: expected_version,
            commands_sha256,
        } if expected == name && expected_version == version && commands_sha256 == local_digest => {
        }
        _ => {
            return Err(Error::new(
                ErrorCode::PluginProtocolError,
                "Plugin hello mismatch",
            ));
        }
    }
    write_frame(
        &mut output,
        &Response::Ready {
            protocol: PLUGIN_PROTOCOL_VERSION,
            name: name.into(),
            version: version.into(),
            commands_sha256: local_digest,
        },
    )
    .await?;
    loop {
        match read_frame::<_, Request>(&mut input).await? {
            Request::Execute { id, command, args } => {
                let response = match dispatch(&command, args) {
                    Ok(value) => Response::Result { id, value },
                    Err(error) => Response::Failure { id, error },
                };
                write_frame(&mut output, &response).await?;
            }
            Request::Health {} => {
                write_frame(
                    &mut output,
                    &Response::Healthy {
                        protocol: PLUGIN_PROTOCOL_VERSION,
                    },
                )
                .await?
            }
            Request::Shutdown {} => return Ok(()),
            _ => {
                return Err(Error::new(
                    ErrorCode::PluginProtocolError,
                    "Unexpected plugin handshake",
                ));
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manifest_requires_hash() {
        let raw = serde_json::json!({"protocol":PLUGIN_PROTOCOL_VERSION,"name":"stats","version":"1","executable":"/tmp/plugin","sha256":"","commands":[]});
        let m: Manifest = serde_json::from_value(raw).unwrap();
        assert!(m.validate().is_err());
    }
    #[test]
    fn reject_extra_wire_keys() {
        assert!(
            serde_json::from_value::<Request>(serde_json::json!({"type":"health","shell":"x"}))
                .is_err()
        );
    }
    #[test]
    fn empty_control_messages_are_strict_and_wire_compatible() {
        for message in [Request::Health {}, Request::Shutdown {}] {
            let encoded = serde_json::to_value(&message).unwrap();
            assert_eq!(encoded.as_object().unwrap().len(), 1);
            assert!(serde_json::from_value::<Request>(encoded.clone()).is_ok());
            let mut invalid = encoded;
            invalid["unexpected"] = serde_json::json!(true);
            assert!(serde_json::from_value::<Request>(invalid).is_err());
        }
    }
}
