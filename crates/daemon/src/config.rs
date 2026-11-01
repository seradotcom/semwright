use semwright_adapters::chromium::BrowserConfig;
use semwright_driver_sdk::Manifest as DriverManifest;
use semwright_federation::StdioUpstreamConfig;
use semwright_platform_common::Application;
use semwright_plugin_sdk::Manifest;
use semwright_policy::PolicyConfig;
#[cfg(unix)]
use semwright_protocol::current_uid;
use semwright_protocol::private_directory;
use semwright_types::*;
use serde::Deserialize;
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::{
    collections::BTreeMap,
    io::Read,
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
    pub drivers: Vec<PathBuf>,
    #[serde(default)]
    pub driver_network: bool,
    /// Owner opt-in: supervised recovery of already-authorized driver processes.
    /// Only the same pinned manifest and permissions may be reused; requests are never replayed.
    #[serde(default)]
    pub driver_recovery: bool,
    /// Owner opt-in; manifests cannot grant NVIDIA device authority on their own.
    #[serde(default)]
    pub driver_nvidia_gpu: bool,
    /// Explicit owner authority for the first-party Windows Blender 5.2.1 initial profile.
    /// Grants registryRead only to its one-process, network-disabled native session.
    #[serde(default)]
    pub driver_windows_blender52_initial: bool,
    #[serde(default)]
    pub plugin_network: bool,
    #[serde(default)]
    pub mcp_network: bool,
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
            drivers: vec![],
            driver_network: false,
            driver_recovery: false,
            driver_nvidia_gpu: false,
            driver_windows_blender52_initial: false,
            plugin_network: false,
            mcp_network: false,
            audit_max_bytes: audit_bytes(),
            audit_retention: retention(),
        }
    }
}
#[cfg(unix)]
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
#[cfg(target_os = "windows")]
pub fn owner_text(path: &Path, max: usize) -> Result<String> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::invalid("Owner configuration requires a parent directory"))?;
    private_directory(parent)?;
    let file = std::fs::OpenOptions::new().read(true).open(path)?;
    let meta = std::fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > max as u64 {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Windows owner configuration must be a bounded regular non-link file in a private directory",
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
pub fn driver_manifest(path: &Path) -> Result<DriverManifest> {
    let manifest: DriverManifest = serde_json::from_str(&owner_text(path, 1_048_576)?)?;
    manifest.validate()?;
    Ok(manifest)
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
    let directory = semwright_platform_services::paths()?.state;
    let base = directory
        .parent()
        .ok_or_else(|| Error::invalid("State directory requires a parent"))?;
    #[cfg(unix)]
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(base)?;
    #[cfg(target_os = "windows")]
    std::fs::create_dir_all(base)?;
    private_directory(&directory)?;
    Ok(directory)
}
/// Grants exposed to generic filesystem consumers must be directory-backed.
pub fn directory_filesystem_grants(
    config: &Config,
) -> Result<Vec<semwright_policy::FilesystemGrant>> {
    let mut grants = Vec::new();
    for grant in &config.policy.filesystem {
        if std::fs::metadata(&grant.path)?.is_dir() {
            grants.push(grant.clone());
        }
    }
    Ok(grants)
}

pub(crate) fn canonical_grant_root(path: &Path) -> Result<PathBuf> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use std::path::{Component, Prefix};
        let invalid = || {
            Error::invalid(
                "Filesystem grant must use a canonical absolute local drive path without aliases",
            )
        };
        let mut parts = path.components();
        if !matches!(parts.next(), Some(Component::Prefix(p)) if matches!(p.kind(), Prefix::Disk(_)))
            || !matches!(parts.next(), Some(Component::RootDir))
        {
            return Err(invalid());
        }
        let spelling = path.to_str().ok_or_else(invalid)?;
        if spelling.get(2..).is_none_or(|s| s.contains(':'))
            || spelling
                .split(['\\', '/'])
                .any(|p| p == "." || p == ".." || p.ends_with(' ') || p.ends_with('.'))
        {
            return Err(invalid());
        }
        for part in parts {
            if !matches!(part, Component::Normal(_)) {
                return Err(invalid());
            }
        }
        for ancestor in path.ancestors() {
            let metadata = std::fs::symlink_metadata(ancestor)?;
            if metadata.file_attributes() & 0x400 != 0 || (ancestor != path && !metadata.is_dir()) {
                return Err(Error::invalid(
                    "Filesystem grant path contains a reparse point or non-directory ancestor",
                ));
            }
        }
        let root = std::fs::canonicalize(path)?;
        let mut canonical = root.components();
        let drive = match canonical.next() {
            Some(Component::Prefix(p)) => match p.kind() {
                Prefix::VerbatimDisk(drive) => drive,
                _ => return Err(invalid()),
            },
            _ => return Err(invalid()),
        };
        if !matches!(canonical.next(), Some(Component::RootDir)) {
            return Err(invalid());
        }
        let mut local = PathBuf::from(format!("{}:\\", char::from(drive)));
        for part in canonical {
            match part {
                Component::Normal(name) => local.push(name),
                _ => return Err(invalid()),
            }
        }
        if local != path {
            return Err(invalid());
        }
        Ok(root)
    }
    #[cfg(not(windows))]
    {
        let root = std::fs::canonicalize(path)?;
        if root != path {
            return Err(Error::invalid(
                "Filesystem grant must be canonical; links in its path are not accepted",
            ));
        }
        Ok(root)
    }
}

pub fn confine_grants(config: &Config, protected: &[PathBuf]) -> Result<()> {
    for grant in &config.policy.filesystem {
        let root = canonical_grant_root(&grant.path)?;
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
    fn driver_configuration_is_explicit_and_strict() {
        let parsed: Config = toml::from_str(
            r#"
drivers = ["/tmp/fixture-driver.json"]
driver_network = false
"#,
        )
        .unwrap();
        assert_eq!(parsed.drivers.len(), 1);
        assert!(!parsed.driver_network);
        assert!(
            toml::from_str::<Config>(
                r#"
drivers = ["/tmp/fixture-driver.json"]
trust_driver_everything = true
"#
            )
            .is_err()
        );
    }
    #[test]
    fn driver_file_grants_are_not_exposed_as_directory_filesystem_roots() {
        let d = tempfile::tempdir().unwrap();
        let workspace = d.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let secret = d.path().join("pairing");
        let tool = d.path().join("godot");
        std::fs::write(&secret, b"secret").unwrap();
        std::fs::write(&tool, b"tool").unwrap();

        let mut c = Config::default();
        for (name, path, write) in [
            ("workspace", workspace, true),
            ("godot-pairing", secret, false),
            ("godot-runtime", tool, false),
        ] {
            c.policy.filesystem.push(semwright_policy::FilesystemGrant {
                name: name.into(),
                path: std::fs::canonicalize(path).unwrap(),
                read: true,
                write,
            });
        }

        let grants = directory_filesystem_grants(&c).unwrap();
        assert_eq!(grants.len(), 1);
        assert_eq!(grants[0].name, "workspace");
        assert!(grants[0].path.is_dir());
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

    #[test]
    fn nvidia_compute_owner_gate_defaults_closed_and_requires_explicit_opt_in() {
        assert!(!Config::default().driver_nvidia_gpu);
        let baseline: Config = toml::from_str("").unwrap();
        assert!(!baseline.driver_nvidia_gpu);
        let opted_in: Config = toml::from_str("driver_nvidia_gpu = true").unwrap();
        assert!(opted_in.driver_nvidia_gpu);
        assert!(!opted_in.driver_network);
        assert!(!opted_in.plugin_network);
        assert!(!opted_in.mcp_network);
    }
}

#[cfg(all(test, windows))]
mod windows_grant_tests {
    use super::*;
    #[test]
    fn windows_grant_accepts_canonical_local_directory_and_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("fixture.bin");
        std::fs::write(&file, b"fixture").unwrap();
        // GitHub Windows runners can hand us a temporary root through an 8.3
        // DOS alias. Production intentionally rejects such ambiguous paths.
        // Test positive grants with the actual resolved long-form drive path.
        let normalized = |path: &Path| {
            let canonical = std::fs::canonicalize(path).unwrap();
            let text = canonical.to_str().unwrap();
            PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(text))
        };
        let canonical_dir = normalized(dir.path());
        let canonical_file = normalized(&file);
        assert_eq!(
            canonical_grant_root(&canonical_dir).unwrap(),
            std::fs::canonicalize(&canonical_dir).unwrap()
        );
        assert_eq!(
            canonical_grant_root(&canonical_file).unwrap(),
            std::fs::canonicalize(&canonical_file).unwrap()
        );
        if canonical_dir != dir.path() {
            assert!(canonical_grant_root(dir.path()).is_err());
        }
    }
    #[test]
    fn windows_grant_rejects_namespace_and_lexical_aliases() {
        for path in [
            r"relative",
            r"C:relative",
            r"\\server\share",
            r"\\?\C:\fixture",
            r"\\.\C:\fixture",
            r"C:\fixture:stream",
            r"C:\fixture\..\elsewhere",
            r"C:\fixture\.\child",
            r"C:\fixture.\child",
            r"C:\fixture \child",
        ] {
            assert!(canonical_grant_root(Path::new(path)).is_err(), "{path}");
        }
    }
    #[test]
    fn windows_grant_rejects_real_junction_and_descendant() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("data.txt"), b"fixture").unwrap();
        let junction = dir.path().join("junction");
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&junction)
            .arg(&target)
            .status()
            .unwrap();
        assert!(
            status.success(),
            "test requires creating its private junction"
        );
        assert!(canonical_grant_root(&junction).is_err());
        assert!(canonical_grant_root(&junction.join("data.txt")).is_err());
    }
}

#[cfg(test)]
mod native_owner_config_tests {
    use super::*;
    #[test]
    fn windows_native_profile_owner_opt_in_is_independent_and_defaults_off() {
        assert!(!Config::default().driver_windows_blender52_initial);
        let old: Config = toml::from_str("driver_nvidia_gpu = true").unwrap();
        assert!(!old.driver_windows_blender52_initial);
        let explicit: Config = toml::from_str("driver_windows_blender52_initial = true").unwrap();
        assert!(explicit.driver_windows_blender52_initial);
        assert!(!explicit.driver_network);
        assert!(!explicit.driver_nvidia_gpu);
    }

    #[test]
    fn generic_driver_recovery_is_an_independent_owner_opt_in() {
        assert!(!Config::default().driver_recovery);
        let opted: Config = toml::from_str("driver_recovery = true").unwrap();
        assert!(opted.driver_recovery);
        assert!(!opted.driver_network);
        assert!(!opted.driver_nvidia_gpu);
        assert!(!opted.driver_windows_blender52_initial);
    }
}
