//! Static/local driver distribution.
//!
//! The package format intentionally has no arbitrary archive entries: one bounded metadata
//! document followed by one pinned ELF payload. Installation therefore never interprets paths
//! supplied by a package and never executes package content.
use semver::{Version, VersionReq};
use semwright_driver_sdk::Manifest;
#[cfg(unix)]
use semwright_protocol::current_uid;
#[cfg(unix)]
use semwright_protocol::private_directory;
#[cfg(unix)]
use semwright_types::unique_id;
use semwright_types::{Error, ErrorCode, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::{
    collections::BTreeSet,
    fs::OpenOptions,
    io::Read,
    path::{Component, Path, PathBuf},
};

pub const INDEX_VERSION: u32 = 1;
pub const PACKAGE_VERSION: u32 = 1;
const MAGIC: &[u8] = b"SEMWRIGHT-DRIVER-PACKAGE-V1\n";
const MAX_INDEX: usize = 2 * 1024 * 1024;
const MAX_METADATA: usize = 1024 * 1024;
const MAX_EXECUTABLE: usize = 64 * 1024 * 1024;
const MAX_PACKAGE: usize = MAX_METADATA + MAX_EXECUTABLE + 64;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageMetadata {
    pub package_version: u32,
    pub semwright: String,
    pub manifest: Manifest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Index {
    pub index_version: u32,
    pub drivers: Vec<IndexEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexEntry {
    pub id: String,
    pub version: String,
    pub publisher: String,
    pub package: PathBuf,
    pub package_sha256: String,
    pub package_bytes: u64,
    pub semwright: String,
    #[serde(default)]
    pub application_versions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub receipt_version: u32,
    pub id: String,
    pub version: String,
    pub publisher: String,
    pub package_sha256: String,
    pub executable_sha256: String,
    pub manifest_path: PathBuf,
    pub executable_path: PathBuf,
    pub source_index: PathBuf,
}

#[derive(Debug, Clone)]
pub struct InstallRoots {
    pub data: PathBuf,
    pub config: PathBuf,
}

impl InstallRoots {
    #[cfg(unix)]
    pub fn defaults() -> Result<Self> {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| Error::unavailable("HOME is required for driver installation"))?;
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"))
            .join("semwright/drivers");
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"))
            .join("semwright/drivers");
        if !data.is_absolute() || !config.is_absolute() {
            return Err(Error::invalid("Driver install roots must be absolute"));
        }
        Ok(Self { data, config })
    }

    #[cfg(target_os = "windows")]
    pub fn defaults() -> Result<Self> {
        let paths = semwright_platform_services::paths()?;
        Ok(Self {
            data: paths.state.join("drivers"),
            config: paths.config.join("drivers"),
        })
    }
}

fn hex_digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(unix)]
fn strict_file(path: &Path, max: usize) -> Result<Vec<u8>> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() > max as u64 {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Driver distribution file is not a bounded regular file",
        ));
    }
    let mut out = Vec::with_capacity(meta.len() as usize);
    Read::by_ref(&mut file)
        .take(max as u64 + 1)
        .read_to_end(&mut out)?;
    if out.len() > max {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Driver distribution file exceeded its size budget",
        ));
    }
    Ok(out)
}

#[cfg(target_os = "windows")]
fn strict_file(path: &Path, max: usize) -> Result<Vec<u8>> {
    let before = std::fs::symlink_metadata(path)?;
    if !before.is_file() || before.file_type().is_symlink() || before.len() > max as u64 {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Driver distribution file must be a bounded regular non-link file",
        ));
    }
    let mut file = OpenOptions::new().read(true).open(path)?;
    let after = file.metadata()?;
    if !after.is_file() || after.len() != before.len() || after.len() > max as u64 {
        return Err(Error::new(
            ErrorCode::Conflict,
            "Driver distribution file changed during open",
        ));
    }
    let mut out = Vec::with_capacity(after.len() as usize);
    Read::by_ref(&mut file)
        .take(max as u64 + 1)
        .read_to_end(&mut out)?;
    if out.len() > max {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Driver distribution file exceeded its size budget",
        ));
    }
    Ok(out)
}

fn valid_relative_package(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && !path.is_absolute()
        && path.as_os_str().len() <= 256
        && path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

fn validate_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn parse_semver(value: &str) -> Result<Version> {
    Version::parse(value).map_err(|_| Error::invalid("Driver version is not valid SemVer"))
}

fn parse_requirement(value: &str) -> Result<VersionReq> {
    VersionReq::parse(value)
        .map_err(|_| Error::invalid("Semwright compatibility requirement is invalid"))
}

impl IndexEntry {
    pub fn validate(&self) -> Result<()> {
        semwright_types::provider::ProviderIdentity::external(
            semwright_types::SourceKind::Driver,
            &self.id,
            &self.version,
        )?;
        parse_semver(&self.version)?;
        parse_requirement(&self.semwright)?;
        if self.publisher.is_empty()
            || self.publisher.len() > 128
            || self.publisher.chars().any(char::is_control)
            || !valid_relative_package(&self.package)
            || !validate_digest(&self.package_sha256)
            || self.package_bytes == 0
            || self.package_bytes > MAX_PACKAGE as u64
            || self.application_versions.len() > 32
            || self.application_versions.iter().any(|version| {
                version.is_empty() || version.len() > 128 || version.chars().any(char::is_control)
            })
        {
            return Err(Error::invalid("Driver index entry exceeds its bounds"));
        }
        Ok(())
    }

    pub fn compatible(
        &self,
        semwright_version: &str,
        application_version: Option<&str>,
    ) -> Result<bool> {
        let running = parse_semver(semwright_version)?;
        if !parse_requirement(&self.semwright)?.matches(&running) {
            return Ok(false);
        }
        if self.application_versions.is_empty() {
            return Ok(true);
        }
        Ok(application_version
            .is_some_and(|version| self.application_versions.iter().any(|v| v == version)))
    }
}

impl Index {
    pub fn load(path: &Path) -> Result<Self> {
        let index: Self = serde_json::from_slice(&strict_file(path, MAX_INDEX)?)?;
        index.validate()?;
        Ok(index)
    }

    pub fn validate(&self) -> Result<()> {
        if self.index_version != INDEX_VERSION || self.drivers.len() > 4096 {
            return Err(Error::invalid("Unsupported or oversized driver index"));
        }
        let mut keys = BTreeSet::new();
        for entry in &self.drivers {
            entry.validate()?;
            if !keys.insert((entry.id.clone(), entry.version.clone())) {
                return Err(Error::new(
                    ErrorCode::Conflict,
                    "Driver index contains a duplicate id/version",
                ));
            }
        }
        Ok(())
    }

    pub fn resolve(
        &self,
        id: &str,
        requested_version: Option<&str>,
        application_version: Option<&str>,
        semwright_version: &str,
    ) -> Result<&IndexEntry> {
        let mut matches = self
            .drivers
            .iter()
            .filter(|entry| entry.id == id)
            .filter(|entry| {
                requested_version
                    .map(|version| entry.version == version)
                    .unwrap_or(true)
            })
            .filter_map(
                |entry| match entry.compatible(semwright_version, application_version) {
                    Ok(true) => Some(Ok(entry)),
                    Ok(false) => None,
                    Err(error) => Some(Err(error)),
                },
            )
            .collect::<Result<Vec<_>>>()?;
        matches.sort_by(|a, b| {
            parse_semver(&b.version)
                .expect("validated")
                .cmp(&parse_semver(&a.version).expect("validated"))
        });
        matches.into_iter().next().ok_or_else(|| {
            Error::new(
                ErrorCode::NotFound,
                "No compatible driver version found in the selected index",
            )
        })
    }
}

pub fn package_digest(path: &Path) -> Result<String> {
    Ok(hex_digest(&strict_file(path, MAX_PACKAGE)?))
}

#[cfg(unix)]
fn read_executable(path: &Path) -> Result<Vec<u8>> {
    let bytes = strict_file(path, MAX_EXECUTABLE)?;
    if !bytes.starts_with(b"\x7fELF") {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "Driver package payload must be an ELF binary",
        ));
    }
    Ok(bytes)
}

#[cfg(unix)]
pub fn create_package(manifest: &Manifest, semwright: &str, output: &Path) -> Result<String> {
    manifest.validate()?;
    parse_requirement(semwright)?;
    let executable = read_executable(&manifest.executable)?;
    let executable_sha256 = hex_digest(&executable);
    if executable_sha256 != manifest.sha256.to_ascii_lowercase() {
        return Err(Error::new(
            ErrorCode::Conflict,
            "Manifest executable digest does not match the bytes being packaged",
        ));
    }
    let mut portable = manifest.clone();
    portable.executable = PathBuf::from("/package/driver");
    portable.sha256 = executable_sha256;
    let metadata = PackageMetadata {
        package_version: PACKAGE_VERSION,
        semwright: semwright.to_owned(),
        manifest: portable,
    };
    let metadata = serde_json::to_vec(&metadata)?;
    if metadata.len() > MAX_METADATA {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Driver package metadata exceeded its size budget",
        ));
    }
    let length = u32::try_from(metadata.len())
        .map_err(|_| Error::new(ErrorCode::ResourceExhausted, "Package metadata overflow"))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(output)?;
    file.write_all(MAGIC)?;
    file.write_all(&length.to_be_bytes())?;
    file.write_all(&metadata)?;
    file.write_all(&executable)?;
    file.sync_all()?;
    package_digest(output)
}

#[cfg(target_os = "windows")]
pub fn create_package(_manifest: &Manifest, _semwright: &str, _output: &Path) -> Result<String> {
    Err(Error::new(
        ErrorCode::Unsupported,
        "Driver Package v1 contains a pinned ELF payload; Windows packaging requires a future multi-platform package format",
    ))
}

pub fn inspect_package(path: &Path) -> Result<(PackageMetadata, Vec<u8>, String)> {
    let package = strict_file(path, MAX_PACKAGE)?;
    if package.len() < MAGIC.len() + 4 || &package[..MAGIC.len()] != MAGIC {
        return Err(Error::invalid("Not a Semwright driver package"));
    }
    let length_at = MAGIC.len();
    let metadata_len = u32::from_be_bytes(
        package[length_at..length_at + 4]
            .try_into()
            .map_err(|_| Error::invalid("Malformed driver package header"))?,
    ) as usize;
    if metadata_len > MAX_METADATA || package.len() < MAGIC.len() + 4 + metadata_len {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Driver package metadata length is invalid",
        ));
    }
    let payload_at = MAGIC.len() + 4 + metadata_len;
    let metadata: PackageMetadata = serde_json::from_slice(&package[MAGIC.len() + 4..payload_at])?;
    if metadata.package_version != PACKAGE_VERSION {
        return Err(Error::new(
            ErrorCode::ProtocolMismatch,
            "Unsupported driver package version",
        ));
    }
    parse_requirement(&metadata.semwright)?;
    metadata.manifest.validate()?;
    if metadata.manifest.executable != Path::new("/package/driver") {
        return Err(Error::invalid(
            "Packaged manifest executable location is not canonical",
        ));
    }
    let executable = package[payload_at..].to_vec();
    if executable.is_empty()
        || executable.len() > MAX_EXECUTABLE
        || !executable.starts_with(b"\x7fELF")
        || hex_digest(&executable) != metadata.manifest.sha256.to_ascii_lowercase()
    {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Driver package executable is invalid or its digest does not match",
        ));
    }
    Ok((metadata, executable, hex_digest(&package)))
}

#[cfg(unix)]
fn ensure_private(path: &Path) -> Result<()> {
    if !path.exists() {
        std::fs::create_dir_all(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    private_directory(path)?;
    let meta = std::fs::symlink_metadata(path)?;
    if !meta.is_dir() || meta.uid() != current_uid() || meta.permissions().mode() & 0o077 != 0 {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Driver store directory is not private to the current user",
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn write_new(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

#[cfg(unix)]
fn atomic_replace(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::invalid("Installed manifest has no parent directory"))?;
    ensure_private(parent)?;
    let temp = parent.join(format!(".manifest-{}.tmp", unique_id()));
    write_new(&temp, bytes, 0o600)?;
    std::fs::rename(&temp, path)?;
    Ok(())
}

#[cfg(unix)]
pub fn install_from_index(
    index_path: &Path,
    entry: &IndexEntry,
    application_version: Option<&str>,
    roots: &InstallRoots,
) -> Result<Receipt> {
    entry.validate()?;
    if !entry.compatible(env!("CARGO_PKG_VERSION"), application_version)? {
        return Err(Error::new(
            ErrorCode::Conflict,
            "Driver is incompatible with this Semwright/application version",
        ));
    }
    if !entry.application_versions.is_empty() && application_version.is_none() {
        return Err(Error::new(
            ErrorCode::Conflict,
            "Application version is required for this constrained driver",
        ));
    }
    let index_dir = index_path
        .parent()
        .ok_or_else(|| Error::invalid("Driver index has no containing directory"))?
        .canonicalize()?;
    let package_path = index_dir.join(&entry.package).canonicalize()?;
    if !package_path.starts_with(&index_dir) {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Driver package escaped the selected static index directory",
        ));
    }
    let (mut metadata, executable, digest) = inspect_package(&package_path)?;
    if digest != entry.package_sha256
        || std::fs::metadata(&package_path)?.len() != entry.package_bytes
        || metadata.manifest.id != entry.id
        || metadata.manifest.version != entry.version
        || metadata.manifest.publisher != entry.publisher
        || metadata.semwright != entry.semwright
        || metadata.manifest.application.supported_versions != entry.application_versions
    {
        return Err(Error::new(
            ErrorCode::Conflict,
            "Driver package does not match its index entry",
        ));
    }

    ensure_private(&roots.data)?;
    ensure_private(&roots.config)?;
    let id_dir = roots.data.join(&entry.id);
    ensure_private(&id_dir)?;
    let target = id_dir.join(&entry.version);
    if target.exists() {
        return Err(Error::new(
            ErrorCode::Conflict,
            "This driver version is already installed",
        ));
    }
    let staging = id_dir.join(format!(".install-{}", unique_id()));
    ensure_private(&staging)?;
    let executable_path = staging.join("driver");
    write_new(&executable_path, &executable, 0o700)?;
    metadata.manifest.executable = target.join("driver");
    let receipt_path = target.join("receipt.json");
    let manifest_bytes = serde_json::to_vec_pretty(&metadata.manifest)?;
    let receipt = Receipt {
        receipt_version: 1,
        id: entry.id.clone(),
        version: entry.version.clone(),
        publisher: entry.publisher.clone(),
        package_sha256: digest,
        executable_sha256: metadata.manifest.sha256.clone(),
        manifest_path: roots.config.join(format!("{}.json", entry.id)),
        executable_path: target.join("driver"),
        source_index: index_path.canonicalize()?,
    };
    write_new(&staging.join("manifest.json"), &manifest_bytes, 0o600)?;
    write_new(
        &staging.join("receipt.json"),
        &serde_json::to_vec_pretty(&receipt)?,
        0o600,
    )?;
    std::fs::rename(&staging, &target)?;
    atomic_replace(&receipt.manifest_path, &manifest_bytes)?;
    let _ = receipt_path;
    Ok(receipt)
}

#[cfg(target_os = "windows")]
pub fn install_from_index(
    _index_path: &Path,
    _entry: &IndexEntry,
    _application_version: Option<&str>,
    _roots: &InstallRoots,
) -> Result<Receipt> {
    Err(Error::new(
        ErrorCode::SandboxDenied,
        "Windows driver installation is fail-closed until package v2 and secure pre-exec Driver Host containment are implemented",
    ))
}

#[cfg(unix)]
pub fn remove_installed(id: &str, version: &str, roots: &InstallRoots) -> Result<()> {
    parse_semver(version)?;
    semwright_types::provider::ProviderIdentity::external(
        semwright_types::SourceKind::Driver,
        id,
        version,
    )?;
    let target = roots.data.join(id).join(version);
    let target_meta = std::fs::symlink_metadata(&target)?;
    if !target_meta.is_dir() || target_meta.uid() != current_uid() {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Installed driver directory ownership is invalid",
        ));
    }
    let receipt_bytes = strict_file(&target.join("receipt.json"), MAX_METADATA)?;
    let receipt: Receipt = serde_json::from_slice(&receipt_bytes)?;
    if receipt.receipt_version != 1
        || receipt.id != id
        || receipt.version != version
        || receipt.executable_path != target.join("driver")
    {
        return Err(Error::new(
            ErrorCode::Conflict,
            "Installed driver receipt does not match the removal target",
        ));
    }
    let active = roots.config.join(format!("{id}.json"));
    if active.is_file() {
        let active_manifest: Manifest =
            serde_json::from_slice(&strict_file(&active, MAX_METADATA)?)?;
        if active_manifest.executable == receipt.executable_path {
            std::fs::remove_file(active)?;
        }
    }
    std::fs::remove_dir_all(&target)?;
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn remove_installed(_id: &str, _version: &str, _roots: &InstallRoots) -> Result<()> {
    Err(Error::new(
        ErrorCode::SandboxDenied,
        "Windows driver removal is unavailable because Driver Package v1 is not installable on Windows",
    ))
}
