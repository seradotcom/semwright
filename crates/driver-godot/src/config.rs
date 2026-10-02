use semwright_driver_sdk::{secret_mount, workspace_mount};
use semwright_types::{Error, ErrorCode, Result, provider::canonical_slug};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};
use zeroize::Zeroize;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    pub project: String,
    pub root: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mount: Option<String>,
    #[serde(skip_serializing)]
    pub secret: String,
}

impl Drop for ProjectConfig {
    fn drop(&mut self) {
        self.secret.zeroize();
    }
}

fn valid_logical_name(value: &str) -> bool {
    canonical_slug(value) && !value.starts_with("semwright-internal-")
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerConfig {
    #[serde(default)]
    pub executable: Option<PathBuf>,
    #[serde(default)]
    pub sha256: Option<String>,
    pub output_root: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_mount: Option<String>,
    #[serde(default)]
    pub display: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthoringConfig {
    pub output_root: PathBuf,
    pub state_root: PathBuf,
    pub input_root: Option<PathBuf>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub port: u16,
    #[serde(default)]
    pub development_mode: bool,
    pub projects: Vec<ProjectConfig>,
    pub runner: Option<RunnerConfig>,
    #[serde(default)]
    pub authoring: Option<AuthoringConfig>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredProjectConfig {
    project: String,
    #[serde(default)]
    root: Option<PathBuf>,
    #[serde(default)]
    mount: Option<String>,
    #[serde(default)]
    secret: Option<String>,
    #[serde(default)]
    secret_file: Option<PathBuf>,
    #[serde(default)]
    secret_name: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredRunnerConfig {
    #[serde(default)]
    executable: Option<PathBuf>,
    #[serde(default)]
    sha256: Option<String>,
    #[serde(default)]
    output_root: Option<PathBuf>,
    #[serde(default)]
    output_mount: Option<String>,
    #[serde(default)]
    display: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredConfig {
    port: u16,
    #[serde(default)]
    development_mode: bool,
    projects: Vec<StoredProjectConfig>,
    runner: Option<StoredRunnerConfig>,
    #[serde(default)]
    authoring: Option<AuthoringConfig>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::symlink_metadata(path)?;
        if !metadata.is_file() || metadata.len() > 32 * 1024 {
            return Err(Error::invalid(
                "Godot owner config must be a small regular file",
            ));
        }
        #[cfg(unix)]
        {
            // SAFETY: getuid takes no pointers and has no memory-safety preconditions.
            let uid = unsafe { libc::getuid() };
            if metadata.uid() != uid || metadata.mode() & 0o077 != 0 || metadata.nlink() != 1 {
                return Err(Error::new(
                    ErrorCode::PermissionDenied,
                    "Godot owner config must be private",
                ));
            }
        }
        let mut bytes = std::fs::read(path)?;
        let parsed = serde_json::from_slice(&bytes);
        bytes.zeroize();
        let stored: StoredConfig = parsed?;
        let mut projects = Vec::with_capacity(stored.projects.len());
        for project in stored.projects {
            let secret = match (project.secret, project.secret_file, project.secret_name) {
                (Some(secret), None, None) if stored.development_mode => secret,
                (Some(mut secret), None, None) => {
                    secret.zeroize();
                    return Err(Error::new(
                        ErrorCode::PermissionDenied,
                        "Inline Godot pairing secrets require development_mode",
                    ));
                }
                (None, Some(secret_file), None) => load_secret_file(&secret_file)?,
                (None, None, Some(secret_name)) => load_secret_mount(&secret_name)?,
                (Some(mut secret), _, _) => {
                    secret.zeroize();
                    return Err(Error::invalid(
                        "Godot project config requires exactly one secret source",
                    ));
                }
                _ => {
                    return Err(Error::invalid(
                        "Godot project config requires exactly one secret source",
                    ));
                }
            };
            let (root, mount) = match (project.root, project.mount) {
                (Some(root), None) if stored.development_mode => (root.canonicalize()?, None),
                (Some(_), None) => {
                    return Err(Error::new(
                        ErrorCode::PermissionDenied,
                        "Production Godot projects must use an owner-granted logical mount",
                    ));
                }
                (None, Some(mount)) if valid_logical_name(&mount) => {
                    (workspace_mount(&mount)?, Some(mount))
                }
                (None, Some(_)) => {
                    return Err(Error::invalid("Godot project mount name is not canonical"));
                }
                (Some(_), Some(_)) | (None, None) => {
                    return Err(Error::invalid(
                        "Godot project config requires exactly one root or mount",
                    ));
                }
            };
            projects.push(ProjectConfig {
                project: project.project,
                root,
                mount,
                secret,
            });
        }
        let runner = stored
            .runner
            .map(|runner| {
                let (output_root, output_mount) = match (runner.output_root, runner.output_mount) {
                    (Some(root), None) if stored.development_mode => (root.canonicalize()?, None),
                    (Some(_), None) => {
                        return Err(Error::new(
                            ErrorCode::PermissionDenied,
                            "Production Godot runner output must use a logical mount",
                        ));
                    }
                    (None, Some(mount)) if valid_logical_name(&mount) => {
                        (workspace_mount(&mount)?, Some(mount))
                    }
                    (None, Some(_)) => {
                        return Err(Error::invalid(
                            "Godot runner output mount name is not canonical",
                        ));
                    }
                    (Some(_), Some(_)) | (None, None) => {
                        return Err(Error::invalid(
                            "Godot runner requires exactly one output_root or output_mount",
                        ));
                    }
                };
                Ok(RunnerConfig {
                    executable: runner.executable,
                    sha256: runner.sha256,
                    output_root,
                    output_mount,
                    display: runner.display,
                })
            })
            .transpose()?;
        let config = Self {
            port: stored.port,
            development_mode: stored.development_mode,
            projects,
            runner,
            authoring: stored.authoring,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        if self.port == 0
            || (self.projects.is_empty() && self.authoring.is_none())
            || self.projects.len() > 8
        {
            return Err(Error::invalid(
                "Godot config requires a port and paired projects or an authoring grant",
            ));
        }
        if let Some(authoring) = &self.authoring {
            authoring.validate()?;
        }
        let mut ids = BTreeSet::new();
        for project in &self.projects {
            if !is_hex(&project.project, 64)
                || !is_hex(&project.secret, 64)
                || !ids.insert(project.project.clone())
            {
                return Err(Error::invalid(
                    "Godot project identity must be unique 256-bit hex",
                ));
            }
            if !project.root.is_absolute()
                || (project.mount.is_none() && project.root.canonicalize()? != project.root)
                || project
                    .mount
                    .as_ref()
                    .is_some_and(|mount| !valid_logical_name(mount))
            {
                return Err(Error::new(
                    ErrorCode::PermissionDenied,
                    "Godot project root/mount binding is invalid",
                ));
            }
            if !project.root.join("project.godot").is_file() {
                return Err(Error::invalid(
                    "Godot project root is missing project.godot",
                ));
            }
        }
        if let Some(runner) = &self.runner {
            match (&runner.executable, &runner.sha256) {
                (None, None) => {}
                (Some(executable), Some(sha256)) => {
                    if !self.development_mode
                        || !executable.is_absolute()
                        || executable.canonicalize()? != *executable
                        || !executable.is_file()
                        || !is_hex(sha256, 64)
                    {
                        return Err(Error::new(
                            ErrorCode::PermissionDenied,
                            "Direct Godot runner executable is development-only and must be canonical/digest-pinned",
                        ));
                    }
                }
                _ => {
                    return Err(Error::new(
                        ErrorCode::PermissionDenied,
                        "Godot runner executable and digest must be supplied together",
                    ));
                }
            }
            let host_managed = runner.executable.is_none();
            if host_managed
                && (runner.output_mount.is_none()
                    || self.projects.iter().any(|project| project.mount.is_none()))
            {
                return Err(Error::new(
                    ErrorCode::PermissionDenied,
                    "Host-managed Godot runners require logical project/output mounts",
                ));
            }
            if !runner.output_root.is_absolute()
                || (runner.output_mount.is_none()
                    && runner.output_root.canonicalize()? != runner.output_root)
                || !runner.output_root.is_dir()
                || runner
                    .output_mount
                    .as_ref()
                    .is_some_and(|mount| !valid_logical_name(mount))
                || runner.display.as_ref().is_some_and(|display| {
                    display.len() > 32
                        || !display.starts_with(':')
                        || !display[1..]
                            .bytes()
                            .all(|b| b.is_ascii_digit() || b == b'.')
                })
            {
                return Err(Error::new(
                    ErrorCode::PermissionDenied,
                    "Godot runner output/display configuration is invalid",
                ));
            }
        }
        Ok(())
    }
}

fn load_secret_mount(name: &str) -> Result<String> {
    if !valid_logical_name(name) {
        return Err(Error::invalid("Godot pairing secret name is not canonical"));
    }
    let path = secret_mount(name)?;
    load_secret_contents(&path)
}

fn load_secret_file(path: &Path) -> Result<String> {
    if !path.is_absolute()
        || path.parent() != Some(Path::new("/run/secrets"))
        || path.file_name().is_none()
    {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Legacy Godot pairing secret path must come from /run/secrets",
        ));
    }
    load_secret_contents(path)
}

fn load_secret_contents(path: &Path) -> Result<String> {
    if !path.is_absolute() || path.file_name().is_none() {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Godot pairing secret path must be absolute and file-backed",
        ));
    }
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > 128 {
        return Err(Error::invalid(
            "Godot pairing secret must be a small regular file",
        ));
    }

    #[cfg(unix)]
    let file = {
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
        // SAFETY: getuid takes no pointers and has no memory-safety preconditions.
        let uid = unsafe { libc::getuid() };
        if metadata.uid() != uid || metadata.mode() & 0o077 != 0 || metadata.nlink() != 1 {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "Godot pairing secret file must be owner-only and single-linked",
            ));
        }
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)?
    };

    #[cfg(not(unix))]
    let file = std::fs::OpenOptions::new().read(true).open(path)?;

    use std::io::Read;
    let mut text = String::new();
    let mut limited = file.take(129);
    limited.read_to_string(&mut text)?;
    let mut secret = text.trim().to_owned();
    text.zeroize();
    if !is_hex(&secret, 64) {
        secret.zeroize();
        return Err(Error::invalid(
            "Godot pairing secret must contain exactly 256-bit lowercase hex",
        ));
    }
    Ok(secret)
}

pub fn is_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn private_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        dir
    }

    #[test]
    fn production_config_rejects_inline_pairing_secret() {
        let config_dir = private_dir();
        let project = private_dir();
        std::fs::write(project.path().join("project.godot"), "config_version=5\n").unwrap();
        let config_path = config_dir.path().join("config.json");
        std::fs::write(
            &config_path,
            serde_json::to_vec(&serde_json::json!({
                "port": 9877,
                "development_mode": false,
                "projects": [{
                    "project": "a".repeat(64),
                    "root": project.path().canonicalize().unwrap(),
                    "secret": "b".repeat(64)
                }],
                "runner": null
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::set_permissions(&config_path, std::fs::Permissions::from_mode(0o600)).unwrap();

        let error = Config::load(&config_path)
            .err()
            .expect("production inline secret must be rejected");
        assert_eq!(error.code, ErrorCode::PermissionDenied);
    }

    #[test]
    fn development_config_still_accepts_inline_pairing_secret() {
        let config_dir = private_dir();
        let project = private_dir();
        std::fs::write(project.path().join("project.godot"), "config_version=5\n").unwrap();
        let config_path = config_dir.path().join("config.json");
        std::fs::write(
            &config_path,
            serde_json::to_vec(&serde_json::json!({
                "port": 9877,
                "development_mode": true,
                "projects": [{
                    "project": "a".repeat(64),
                    "root": project.path().canonicalize().unwrap(),
                    "secret": "b".repeat(64)
                }],
                "runner": null
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::set_permissions(&config_path, std::fs::Permissions::from_mode(0o600)).unwrap();

        let config = Config::load(&config_path).unwrap();
        assert_eq!(config.projects[0].secret, "b".repeat(64));
    }

    #[test]
    fn production_runner_uses_host_managed_tool_authority() {
        let project = private_dir();
        std::fs::write(project.path().join("project.godot"), "config_version=5\n").unwrap();
        let output = private_dir();
        let config = Config {
            port: 9877,
            development_mode: false,
            projects: vec![ProjectConfig {
                project: "a".repeat(64),
                root: project.path().canonicalize().unwrap(),
                mount: Some("godot-project".into()),
                secret: "b".repeat(64),
            }],
            runner: Some(RunnerConfig {
                executable: None,
                sha256: None,
                output_root: output.path().canonicalize().unwrap(),
                output_mount: Some("godot-output".into()),
                display: None,
            }),
        };
        config.validate().unwrap();
    }

    #[test]
    fn direct_runner_executable_is_development_only() {
        let project = private_dir();
        std::fs::write(project.path().join("project.godot"), "config_version=5\n").unwrap();
        let output = private_dir();
        let binary_dir = private_dir();
        let binary = binary_dir.path().join("godot");
        std::fs::write(&binary, b"fixture").unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o500)).unwrap();
        let binary = binary.canonicalize().unwrap();

        let make = |development_mode| Config {
            port: 9877,
            development_mode,
            projects: vec![ProjectConfig {
                project: "a".repeat(64),
                root: project.path().canonicalize().unwrap(),
                mount: None,
                secret: "b".repeat(64),
            }],
            runner: Some(RunnerConfig {
                executable: Some(binary.clone()),
                sha256: Some("c".repeat(64)),
                output_root: output.path().canonicalize().unwrap(),
                output_mount: None,
                display: None,
            }),
        };
        assert_eq!(
            make(false).validate().unwrap_err().code,
            ErrorCode::PermissionDenied
        );
        make(true).validate().unwrap();
    }

    #[test]
    fn logical_mount_and_secret_names_are_canonical() {
        for good in ["godot-project", "godot-output", "godot-pairing"] {
            assert!(valid_logical_name(good), "{good}");
        }
        for bad in [
            "",
            "../project",
            "Godot-Project",
            "godot/project",
            "semwright-internal-secret",
        ] {
            assert!(!valid_logical_name(bad), "{bad}");
        }
        let error = load_secret_mount("../pairing").unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }

    #[test]
    fn legacy_secret_file_must_live_under_run_secrets() {
        let error = load_secret_file(Path::new("/tmp/not-a-secret")).unwrap_err();
        assert_eq!(error.code, ErrorCode::PermissionDenied);
    }
}

impl AuthoringConfig {
    pub fn validate(&self) -> Result<()> {
        for root in [&self.output_root, &self.state_root]
            .into_iter()
            .chain(self.input_root.iter())
        {
            if !root.is_absolute() || root.canonicalize()? != *root || !root.is_dir() {
                return Err(Error::new(
                    ErrorCode::PermissionDenied,
                    "Authoring roots must be canonical directories",
                ));
            }
        }
        let overlaps = |a: &Path, b: &Path| a.starts_with(b) || b.starts_with(a);
        if overlaps(&self.state_root, &self.output_root)
            || self.input_root.as_ref().is_some_and(|input| {
                overlaps(input, &self.state_root) || overlaps(input, &self.output_root)
            })
        {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "Authoring input/output/private-state roots must not overlap",
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let state = std::fs::symlink_metadata(&self.state_root)?;
            // SAFETY: getuid takes no pointers and has no memory safety preconditions.
            let uid = unsafe { libc::getuid() };
            if state.uid() != uid || state.mode() & 0o077 != 0 {
                return Err(Error::new(
                    ErrorCode::PermissionDenied,
                    "Authoring derivation state must be owner-only",
                ));
            }
        }
        Ok(())
    }
}
