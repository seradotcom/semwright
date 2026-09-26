//! Static/local driver distribution.
//!
//! Driver packages are deterministic bounded containers, not general archives. Package v2 keeps
//! one pinned ELF driver payload plus an explicit list of companion files whose relative paths,
//! byte lengths and SHA-256 digests are declared in metadata. Installation never interprets
//! symlinks, post-install scripts or executable bits from package content.
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
pub const PACKAGE_VERSION: u32 = 2;
const MAGIC_V1: &[u8] = b"SEMWRIGHT-DRIVER-PACKAGE-V1\n";
const MAGIC_V2: &[u8] = b"SEMWRIGHT-DRIVER-PACKAGE-V2\n";
const MAX_INDEX: usize = 2 * 1024 * 1024;
const MAX_METADATA: usize = 1024 * 1024;
const MAX_EXECUTABLE: usize = 64 * 1024 * 1024;
const MAX_COMPANIONS: usize = 512;
const MAX_COMPANION_FILE: usize = 8 * 1024 * 1024;
const MAX_COMPANION_BYTES: usize = 32 * 1024 * 1024;
const MAX_PACKAGE: usize = MAX_METADATA + MAX_EXECUTABLE + MAX_COMPANION_BYTES + 64;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageMetadata {
    pub package_version: u32,
    pub semwright: String,
    pub manifest: Manifest,
    #[serde(default)]
    pub executable_bytes: u64,
    #[serde(default)]
    pub companions: Vec<CompanionMetadata>,
}

#[derive(Debug, Clone)]
pub struct CompanionInput {
    pub destination: PathBuf,
    pub source: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CompanionMetadata {
    pub path: PathBuf,
    pub sha256: String,
    pub bytes: u64,
}
impl CompanionMetadata {
    fn validate(&self) -> Result<()> {
        if !valid_relative_package(&self.path)
            || !validate_digest(&self.sha256)
            || self.bytes == 0
            || self.bytes > MAX_COMPANION_FILE as u64
        {
            return Err(Error::invalid("Driver companion metadata exceeds bounds"));
        }
        Ok(())
    }
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
    #[serde(default)]
    pub companion_files: Vec<CompanionMetadata>,
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
    create_package_with_companions(manifest, semwright, &[], output)
}

#[cfg(unix)]
pub fn create_package_with_companions(
    manifest: &Manifest,
    semwright: &str,
    companions: &[CompanionInput],
    output: &Path,
) -> Result<String> {
    manifest.validate()?;
    parse_requirement(semwright)?;
    if companions.len() > MAX_COMPANIONS {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Driver package has too many companion files",
        ));
    }
    let executable = read_executable(&manifest.executable)?;
    let executable_sha256 = hex_digest(&executable);
    if executable_sha256 != manifest.sha256.to_ascii_lowercase() {
        return Err(Error::new(
            ErrorCode::Conflict,
            "Manifest executable digest does not match the bytes being packaged",
        ));
    }

    let mut seen = BTreeSet::new();
    let mut descriptors = Vec::with_capacity(companions.len());
    let mut payloads = Vec::with_capacity(companions.len());
    let mut total = 0usize;
    for companion in companions {
        if !valid_relative_package(&companion.destination)
            || !seen.insert(companion.destination.clone())
        {
            return Err(Error::invalid(
                "Driver companion destinations must be unique bounded relative paths",
            ));
        }
        let bytes = strict_file(&companion.source, MAX_COMPANION_FILE)?;
        if bytes.is_empty() {
            return Err(Error::invalid("Driver companion files cannot be empty"));
        }
        total = total
            .checked_add(bytes.len())
            .ok_or_else(|| Error::new(ErrorCode::ResourceExhausted, "Companion size overflow"))?;
        if total > MAX_COMPANION_BYTES {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Driver companion payload exceeded its aggregate size budget",
            ));
        }
        descriptors.push(CompanionMetadata {
            path: companion.destination.clone(),
            sha256: hex_digest(&bytes),
            bytes: bytes.len() as u64,
        });
        payloads.push(bytes);
    }

    let mut portable = manifest.clone();
    portable.executable = PathBuf::from("/package/driver");
    portable.sha256 = executable_sha256;
    let metadata = PackageMetadata {
        package_version: PACKAGE_VERSION,
        semwright: semwright.to_owned(),
        manifest: portable,
        executable_bytes: executable.len() as u64,
        companions: descriptors,
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
    file.write_all(MAGIC_V2)?;
    file.write_all(&length.to_be_bytes())?;
    file.write_all(&metadata)?;
    file.write_all(&executable)?;
    for payload in payloads {
        file.write_all(&payload)?;
    }
    file.sync_all()?;
    package_digest(output)
}

#[cfg(target_os = "windows")]
pub fn create_package(_manifest: &Manifest, _semwright: &str, _output: &Path) -> Result<String> {
    Err(Error::new(
        ErrorCode::Unsupported,
        "Driver Package v2 still carries a platform-specific executable; Windows package creation remains fail-closed",
    ))
}

#[cfg(target_os = "windows")]
pub fn create_package_with_companions(
    _manifest: &Manifest,
    _semwright: &str,
    _companions: &[CompanionInput],
    _output: &Path,
) -> Result<String> {
    Err(Error::new(
        ErrorCode::Unsupported,
        "Driver Package v2 companion creation is unavailable until Windows driver execution is securely contained",
    ))
}

struct InspectedPackage {
    metadata: PackageMetadata,
    executable: Vec<u8>,
    companions: Vec<(CompanionMetadata, Vec<u8>)>,
    digest: String,
}

fn inspect_package_full(path: &Path) -> Result<InspectedPackage> {
    let package = strict_file(path, MAX_PACKAGE)?;
    let (magic, expected_version) = if package.starts_with(MAGIC_V2) {
        (MAGIC_V2, 2)
    } else if package.starts_with(MAGIC_V1) {
        (MAGIC_V1, 1)
    } else {
        return Err(Error::invalid("Not a Semwright driver package"));
    };
    if package.len() < magic.len() + 4 {
        return Err(Error::invalid("Malformed driver package header"));
    }
    let length_at = magic.len();
    let metadata_len = u32::from_be_bytes(
        package[length_at..length_at + 4]
            .try_into()
            .map_err(|_| Error::invalid("Malformed driver package header"))?,
    ) as usize;
    let payload_at = magic
        .len()
        .checked_add(4)
        .and_then(|offset| offset.checked_add(metadata_len))
        .ok_or_else(|| Error::new(ErrorCode::ResourceExhausted, "Metadata offset overflow"))?;
    if metadata_len > MAX_METADATA || package.len() < payload_at {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Driver package metadata length is invalid",
        ));
    }
    let metadata: PackageMetadata = serde_json::from_slice(&package[magic.len() + 4..payload_at])?;
    if metadata.package_version != expected_version {
        return Err(Error::new(
            ErrorCode::ProtocolMismatch,
            "Driver package magic/version mismatch",
        ));
    }
    parse_requirement(&metadata.semwright)?;
    metadata.manifest.validate()?;
    if metadata.manifest.executable != Path::new("/package/driver") {
        return Err(Error::invalid(
            "Packaged manifest executable location is not canonical",
        ));
    }

    let executable_len = if expected_version == 1 {
        if metadata.executable_bytes != 0 || !metadata.companions.is_empty() {
            return Err(Error::invalid(
                "Driver Package v1 cannot declare extra payloads",
            ));
        }
        package.len() - payload_at
    } else {
        usize::try_from(metadata.executable_bytes)
            .map_err(|_| Error::new(ErrorCode::ResourceExhausted, "Executable size overflow"))?
    };
    if executable_len == 0
        || executable_len > MAX_EXECUTABLE
        || payload_at + executable_len > package.len()
    {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Driver package executable length is invalid",
        ));
    }
    let executable = package[payload_at..payload_at + executable_len].to_vec();
    if !executable.starts_with(b"\x7fELF")
        || hex_digest(&executable) != metadata.manifest.sha256.to_ascii_lowercase()
    {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Driver package executable is invalid or its digest does not match",
        ));
    }

    let mut cursor = payload_at + executable_len;
    let mut seen = BTreeSet::new();
    let mut companion_total = 0usize;
    let mut companions = Vec::new();
    if metadata.companions.len() > MAX_COMPANIONS {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Driver package has too many companion files",
        ));
    }
    for descriptor in &metadata.companions {
        descriptor.validate()?;
        if !seen.insert(descriptor.path.clone()) {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Driver package repeats a companion destination",
            ));
        }
        let len = usize::try_from(descriptor.bytes)
            .map_err(|_| Error::new(ErrorCode::ResourceExhausted, "Companion size overflow"))?;
        companion_total = companion_total
            .checked_add(len)
            .ok_or_else(|| Error::new(ErrorCode::ResourceExhausted, "Companion size overflow"))?;
        let end = cursor
            .checked_add(len)
            .ok_or_else(|| Error::new(ErrorCode::ResourceExhausted, "Companion offset overflow"))?;
        if companion_total > MAX_COMPANION_BYTES || end > package.len() {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Driver companion payload length is invalid",
            ));
        }
        let bytes = package[cursor..end].to_vec();
        if hex_digest(&bytes) != descriptor.sha256 {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "Driver companion digest does not match package bytes",
            ));
        }
        cursor += len;
        companions.push((descriptor.clone(), bytes));
    }
    if cursor != package.len() {
        return Err(Error::invalid(
            "Driver package has unaccounted trailing bytes",
        ));
    }

    Ok(InspectedPackage {
        metadata,
        executable,
        companions,
        digest: hex_digest(&package),
    })
}

pub fn inspect_package(path: &Path) -> Result<(PackageMetadata, Vec<u8>, String)> {
    let inspected = inspect_package_full(path)?;
    if inspected.companions.len() != inspected.metadata.companions.len() {
        return Err(Error::new(
            ErrorCode::ProtocolMismatch,
            "Decoded companion payload count does not match package metadata",
        ));
    }
    Ok((inspected.metadata, inspected.executable, inspected.digest))
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
    let inspected = inspect_package_full(&package_path)?;
    let mut metadata = inspected.metadata;
    let executable = inspected.executable;
    let companions = inspected.companions;
    let digest = inspected.digest;
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
    if !companions.is_empty() {
        let companion_root = staging.join("companions");
        ensure_private(&companion_root)?;
        for (descriptor, bytes) in &companions {
            let mut parent = companion_root.clone();
            if let Some(relative_parent) = descriptor.path.parent() {
                for component in relative_parent.components() {
                    let Component::Normal(component) = component else {
                        return Err(Error::invalid("Invalid companion install path"));
                    };
                    parent.push(component);
                    ensure_private(&parent)?;
                }
            }
            write_new(&companion_root.join(&descriptor.path), bytes, 0o600)?;
        }
    }
    metadata.manifest.executable = target.join("driver");
    let receipt_path = target.join("receipt.json");
    let manifest_bytes = serde_json::to_vec_pretty(&metadata.manifest)?;
    let receipt = Receipt {
        receipt_version: 2,
        id: entry.id.clone(),
        version: entry.version.clone(),
        publisher: entry.publisher.clone(),
        package_sha256: digest,
        executable_sha256: metadata.manifest.sha256.clone(),
        manifest_path: roots.config.join(format!("{}.json", entry.id)),
        executable_path: target.join("driver"),
        companion_files: metadata.companions.clone(),
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
    if !matches!(receipt.receipt_version, 1 | 2)
        || receipt.id != id
        || receipt.version != version
        || receipt.executable_path != target.join("driver")
    {
        return Err(Error::new(
            ErrorCode::Conflict,
            "Installed driver receipt does not match the removal target",
        ));
    }
    let mut companion_paths = BTreeSet::new();
    for companion in &receipt.companion_files {
        companion.validate()?;
        if !companion_paths.insert(&companion.path) {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Installed driver receipt repeats a companion path",
            ));
        }
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
        "Windows driver removal is unavailable until secure Windows Driver Host installation is implemented",
    ))
}
