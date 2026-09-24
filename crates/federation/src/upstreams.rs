//! Owner-managed upstream definitions. Definitions never grant broker authority.
use crate::{StdioUpstreamConfig, executable_sha256};
#[cfg(unix)]
use semwright_protocol::current_uid;
use semwright_protocol::private_directory;
use semwright_types::{Error, ErrorCode, Result, unique_id};
use serde::{Deserialize, Serialize};
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::{
    collections::BTreeSet,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const MAX_REGISTRY: usize = 1_048_576;
const REGISTRY_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpstreamRegistry {
    #[serde(default = "version")]
    pub version: u32,
    #[serde(default)]
    pub upstreams: Vec<StdioUpstreamConfig>,
}
fn version() -> u32 {
    REGISTRY_VERSION
}
impl Default for UpstreamRegistry {
    fn default() -> Self {
        Self {
            version: REGISTRY_VERSION,
            upstreams: vec![],
        }
    }
}
impl UpstreamRegistry {
    pub fn validate_structure(&self) -> Result<()> {
        if self.version != REGISTRY_VERSION || self.upstreams.len() > 64 {
            return Err(Error::invalid(
                "Unsupported or oversized MCP upstream registry",
            ));
        }
        let mut slugs = BTreeSet::new();
        for upstream in &self.upstreams {
            upstream.validate_definition()?;
            if !slugs.insert(upstream.slug.clone()) {
                return Err(Error::new(
                    ErrorCode::Conflict,
                    "MCP upstream registry contains a duplicate slug",
                ));
            }
        }
        Ok(())
    }
    pub fn find(&self, slug: &str) -> Option<&StdioUpstreamConfig> {
        self.upstreams.iter().find(|entry| entry.slug == slug)
    }
    pub fn add(&mut self, upstream: StdioUpstreamConfig, replace: bool) -> Result<()> {
        upstream.validate()?;
        if let Some(existing) = self
            .upstreams
            .iter_mut()
            .find(|row| row.slug == upstream.slug)
        {
            if !replace {
                return Err(Error::new(
                    ErrorCode::Conflict,
                    "MCP upstream already exists; use --replace deliberately",
                ));
            }
            *existing = upstream;
        } else {
            if self.upstreams.len() >= 64 {
                return Err(Error::new(
                    ErrorCode::ResourceExhausted,
                    "MCP upstream registry limit reached",
                ));
            }
            self.upstreams.push(upstream);
        }
        self.upstreams.sort_by(|a, b| a.slug.cmp(&b.slug));
        self.validate_structure()
    }
    pub fn remove(&mut self, slug: &str) -> Result<StdioUpstreamConfig> {
        let index = self
            .upstreams
            .iter()
            .position(|row| row.slug == slug)
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "MCP upstream not found"))?;
        Ok(self.upstreams.remove(index))
    }
    pub fn set_enabled(&mut self, slug: &str, enabled: bool) -> Result<()> {
        let upstream = self
            .upstreams
            .iter_mut()
            .find(|row| row.slug == slug)
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "MCP upstream not found"))?;
        if enabled {
            upstream.validate()?;
        }
        upstream.enabled = enabled;
        Ok(())
    }
}

pub fn default_upstream_registry_path() -> Result<PathBuf> {
    Ok(semwright_platform_services::paths()?
        .config
        .join("mcp-upstreams.toml"))
}

#[cfg(unix)]
fn check_registry_file(path: &Path, metadata: &std::fs::Metadata) -> Result<()> {
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.uid() != current_uid()
        || metadata.permissions().mode() & 0o777 != 0o600
        || metadata.nlink() != 1
        || metadata.len() > MAX_REGISTRY as u64
    {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "MCP upstream registry must be a single-link owner file with mode 0600",
        ));
    }
    if path.parent().is_none() {
        return Err(Error::invalid(
            "MCP upstream registry has no parent directory",
        ));
    }
    Ok(())
}
#[cfg(target_os = "windows")]
fn check_registry_file(path: &Path, metadata: &std::fs::Metadata) -> Result<()> {
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_REGISTRY as u64
        || path.parent().is_none()
    {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Windows MCP upstream registry must be a bounded regular non-link file in the private config directory",
        ));
    }
    Ok(())
}

pub fn load_upstream_registry(path: &Path) -> Result<UpstreamRegistry> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(UpstreamRegistry::default());
        }
        Err(error) => return Err(error.into()),
    };
    let metadata = file.metadata()?;
    check_registry_file(path, &metadata)?;
    if let Some(parent) = path.parent() {
        private_directory(parent)?;
    }
    let mut text = String::new();
    file.take(MAX_REGISTRY as u64 + 1)
        .read_to_string(&mut text)?;
    if text.len() > MAX_REGISTRY {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "MCP upstream registry exceeds its size budget",
        ));
    }
    let registry: UpstreamRegistry =
        toml::from_str(&text).map_err(|_| Error::invalid("Invalid MCP upstream registry"))?;
    registry.validate_structure()?;
    Ok(registry)
}

pub fn save_upstream_registry(path: &Path, registry: &UpstreamRegistry) -> Result<()> {
    registry.validate_structure()?;
    let parent = path
        .parent()
        .ok_or_else(|| Error::invalid("MCP upstream registry has no parent directory"))?;
    if !parent.exists() {
        if let Some(base) = parent.parent() {
            std::fs::create_dir_all(base)?;
        }
        private_directory(parent)?;
    } else {
        private_directory(parent)?;
    }
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => check_registry_file(path, &metadata)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        Err(error) => return Err(error.into()),
    }
    let text = toml::to_string_pretty(registry)
        .map_err(|_| Error::invalid("Unable to serialize MCP upstream registry"))?;
    if text.len() > MAX_REGISTRY {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "MCP upstream registry serialization exceeds its size budget",
        ));
    }
    let temp = parent.join(format!(".mcp-upstreams-{}.tmp", unique_id()));
    let result = (|| -> Result<()> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options.open(&temp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&temp, path)?;
        std::fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

pub fn new_upstream(
    slug: String,
    program: PathBuf,
    sha256: Option<String>,
    args: Vec<String>,
    expected_name: Option<String>,
    expected_version: Option<String>,
    request_timeout_ms: u64,
    enabled: bool,
) -> Result<StdioUpstreamConfig> {
    let digest = match sha256 {
        Some(digest) => digest,
        None => executable_sha256(&program)?,
    };
    let upstream = StdioUpstreamConfig {
        slug,
        program,
        sha256: digest,
        args,
        expected_name,
        expected_version,
        request_timeout_ms,
        enabled,
    };
    upstream.validate()?;
    Ok(upstream)
}
