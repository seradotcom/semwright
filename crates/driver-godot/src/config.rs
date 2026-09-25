use semwright_types::{Error, ErrorCode, Result};
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
    #[serde(skip_serializing)]
    pub secret: String,
}

impl Drop for ProjectConfig {
    fn drop(&mut self) {
        self.secret.zeroize();
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerConfig {
    pub executable: PathBuf,
    pub sha256: String,
    pub output_root: PathBuf,
    #[serde(default)]
    pub display: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub port: u16,
    #[serde(default)]
    pub development_mode: bool,
    pub projects: Vec<ProjectConfig>,
    pub runner: Option<RunnerConfig>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredProjectConfig {
    project: String,
    root: PathBuf,
    #[serde(default)]
    secret: Option<String>,
    #[serde(default)]
    secret_file: Option<PathBuf>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredConfig {
    port: u16,
    #[serde(default)]
    development_mode: bool,
    projects: Vec<StoredProjectConfig>,
    runner: Option<RunnerConfig>,
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
            let secret = match (project.secret, project.secret_file) {
                (Some(secret), None) if stored.development_mode => secret,
                (Some(mut secret), None) => {
                    secret.zeroize();
                    return Err(Error::new(
                        ErrorCode::PermissionDenied,
                        "Inline Godot pairing secrets require development_mode",
                    ));
                }
                (None, Some(secret_file)) => load_secret_file(&secret_file)?,
                (Some(mut secret), Some(_)) => {
                    secret.zeroize();
                    return Err(Error::invalid(
                        "Godot project config requires exactly one secret source",
                    ));
                }
                (None, None) => {
                    return Err(Error::invalid(
                        "Godot project config requires exactly one secret source",
                    ));
                }
            };
            projects.push(ProjectConfig {
                project: project.project,
                root: project.root,
                secret,
            });
        }
        let config = Self {
            port: stored.port,
            development_mode: stored.development_mode,
            projects,
            runner: stored.runner,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        if self.port == 0 || self.projects.is_empty() || self.projects.len() > 8 {
            return Err(Error::invalid(
                "Godot config requires a port and 1..8 projects",
            ));
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
            if !project.root.is_absolute() || project.root.canonicalize()? != project.root {
                return Err(Error::new(
                    ErrorCode::PermissionDenied,
                    "Godot project root must be canonical",
                ));
            }
            if !project.root.join("project.godot").is_file() {
                return Err(Error::invalid(
                    "Godot project root is missing project.godot",
                ));
            }
        }
        if let Some(runner) = &self.runner
            && (!runner.executable.is_absolute()
                || runner.executable.canonicalize()? != runner.executable
                || !runner.executable.is_file()
                || !is_hex(&runner.sha256, 64)
                || !runner.output_root.is_absolute()
                || runner.output_root.canonicalize()? != runner.output_root
                || !runner.output_root.is_dir()
                || runner.display.as_ref().is_some_and(|display| {
                    display.len() > 32
                        || !display.starts_with(':')
                        || !display[1..]
                            .bytes()
                            .all(|b| b.is_ascii_digit() || b == b'.')
                }))
        {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "Godot runner paths or digest are invalid",
            ));
        }
        Ok(())
    }
}

fn load_secret_file(path: &Path) -> Result<String> {
    if !path.is_absolute()
        || path.parent() != Some(Path::new("/run/secrets"))
        || path.file_name().is_none()
    {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Godot pairing secret must come from /run/secrets",
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
        if metadata.uid() != unsafe { libc::getuid() }
            || metadata.mode() & 0o077 != 0
            || metadata.nlink() != 1
        {
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
    fn secret_file_must_live_under_run_secrets() {
        let error = load_secret_file(Path::new("/tmp/not-a-secret")).unwrap_err();
        assert_eq!(error.code, ErrorCode::PermissionDenied);
    }
}
