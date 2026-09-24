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
        let bytes = std::fs::read(path)?;
        let config: Self = serde_json::from_slice(&bytes)?;
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

pub fn is_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
