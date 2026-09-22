use semwright_adapters::chromium::BrowserConfig;
use semwright_backends::system::Application;
use semwright_federation::StdioUpstreamConfig;
use semwright_plugin_sdk::Manifest;
use semwright_policy::PolicyConfig;
use semwright_protocol::{current_uid, private_directory};
use semwright_types::*;
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    io::Read,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub policy: PolicyConfig,
    #[serde(default)]
    pub applications: BTreeMap<String, Application>,
    #[serde(default)]
    pub browser: BrowserConfig,
    pub blender_socket: Option<PathBuf>,
    /// Trusted executables explicitly selected by the owner. Tool calls remain policy-gated.
    #[serde(default)]
    pub trusted_mcp_stdio_upstreams: Vec<StdioUpstreamConfig>,
    #[serde(default)]
    pub plugins: Vec<PathBuf>,
    #[serde(default)]
    pub plugin_network: bool,
    #[serde(default = "audit_bytes")]
    pub audit_max_bytes: u64,
    #[serde(default = "retention")]
    pub audit_retention: usize,
}
fn audit_bytes() -> u64 {
    8 * 1024 * 1024
}
fn retention() -> usize {
    4
}
impl Default for Config {
    fn default() -> Self {
        Self {
            policy: PolicyConfig::default(),
            applications: BTreeMap::new(),
            browser: BrowserConfig::default(),
            blender_socket: None,
            trusted_mcp_stdio_upstreams: vec![],
            plugins: vec![],
            plugin_network: false,
            audit_max_bytes: audit_bytes(),
            audit_retention: retention(),
        }
    }
}
pub fn owner_text(path: &Path, max: usize) -> Result<String> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file()
        || meta.uid() != current_uid()
        || meta.mode() & 0o777 != 0o600
        || meta.nlink() != 1
        || meta.len() > max as u64
    {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Owner configuration must be a single-link regular file, owned by this user, mode 0600, within the size limit",
        ));
    }
    let mut text = String::new();
    file.take(max as u64 + 1).read_to_string(&mut text)?;
    if text.len() > max {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Configuration grew beyond its budget",
        ));
    }
    Ok(text)
}
pub fn load(path: Option<&Path>) -> Result<Config> {
    match path {
        Some(path) => toml::from_str(&owner_text(path, 1_048_576)?)
            .map_err(|_| Error::invalid("Invalid owner configuration; unknown keys are rejected")),
        None => Ok(Config::default()),
    }
}
pub fn manifest(path: &Path) -> Result<Manifest> {
    let manifest: Manifest = serde_json::from_str(&owner_text(path, 1_048_576)?)?;
    manifest.validate()?;
    Ok(manifest)
}
pub fn state_directory(fake: bool, runtime: &Path) -> Result<PathBuf> {
    if fake {
        let path = runtime.join("fake-state");
        private_directory(&path)?;
        return Ok(path);
    }
    let base = match std::env::var_os("XDG_STATE_HOME") {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(
            std::env::var_os("HOME")
                .ok_or_else(|| Error::unavailable("HOME or XDG_STATE_HOME required"))?,
        )
        .join(".local/state"),
    };
    if !base.is_absolute() {
        return Err(Error::invalid("XDG_STATE_HOME must be absolute"));
    }
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&base)?;
    let directory = base.join("semwright");
    private_directory(&directory)?;
    Ok(directory)
}
pub fn confine_grants(config: &Config, protected: &[PathBuf]) -> Result<()> {
    for grant in &config.policy.filesystem {
        let root = std::fs::canonicalize(&grant.path)?;
        if root != grant.path {
            return Err(Error::invalid(
                "Filesystem grant must be canonical; links in its path are not accepted",
            ));
        }
        for private in protected {
            let private = std::fs::canonicalize(private)?;
            if root.starts_with(&private) || private.starts_with(&root) {
                return Err(Error::new(
                    ErrorCode::PolicyDenied,
                    "Filesystem grant overlaps broker configuration, runtime or state; choose a separate project directory",
                ));
            }
        }
        for blocked in ["/proc", "/sys", "/dev", "/run"] {
            if root.starts_with(blocked) {
                return Err(Error::new(
                    ErrorCode::PolicyDenied,
                    "Runtime/device/proc trees cannot be filesystem grant roots",
                ));
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_config_key_rejected() {
        assert!(toml::from_str::<Config>("enable_everything = true").is_err());
    }
    #[test]
    fn trusted_mcp_upstream_config_is_explicit_and_strict() {
        let parsed: Config = toml::from_str(
            r#"
[policy]
profile = "observe"
allow = ["external-mcp:fixture"]

[[trusted_mcp_stdio_upstreams]]
slug = "fixture"
program = "/usr/bin/true"
sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
args = ["--fixture"]
expected_name = "fixture-server"
expected_version = "1.0.0"
request_timeout_ms = 2500
"#,
        )
        .unwrap();
        assert_eq!(parsed.trusted_mcp_stdio_upstreams.len(), 1);
        assert_eq!(parsed.trusted_mcp_stdio_upstreams[0].slug, "fixture");
        assert!(parsed.policy.allow.contains("external-mcp:fixture"));
        assert!(
            toml::from_str::<Config>(
                r#"
[[trusted_mcp_stdio_upstreams]]
slug = "fixture"
program = "/usr/bin/true"
sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
trust_everything = true
"#
            )
            .is_err()
        );
    }

    #[test]
    fn root_scope_must_not_contain_broker_state() {
        let d = tempfile::tempdir().unwrap();
        let private = d.path().join("private");
        std::fs::create_dir(&private).unwrap();
        let mut c = Config::default();
        c.policy.filesystem.push(semwright_policy::FilesystemGrant {
            name: "bad".into(),
            path: d.path().into(),
            read: true,
            write: true,
        });
        assert!(confine_grants(&c, &[private]).is_err());
    }
}
