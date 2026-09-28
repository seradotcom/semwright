use semwright_registry::Metadata;
use semwright_types::{CommandDescriptor, SourceKind};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

pub const MAX_SKILL_MD_BYTES: u64 = 1024 * 1024;
pub const MAX_FRONTMATTER_BYTES: usize = 64 * 1024;
pub const MAX_RESOURCE_COUNT: usize = 500;
pub const MAX_PACKAGE_BYTES: u64 = 50 * 1024 * 1024;
pub const MAX_RESOURCE_BYTES: u64 = 25 * 1024 * 1024;
pub const MAX_DIRECTORY_DEPTH: usize = 16;
pub const MAX_NAME_CHARS: usize = 64;
pub const MAX_DESCRIPTION_CHARS: usize = 1024;
pub const MAX_REQUIREMENTS: usize = 128;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillManifest {
    pub name: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compatibility: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
    #[serde(
        default,
        rename = "allowed-tools",
        skip_serializing_if = "Option::is_none"
    )]
    pub allowed_tools: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    Reference,
    Script,
    Asset,
    SemwrightMetadata,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillResource {
    pub path: PathBuf,
    pub kind: ResourceKind,
    pub bytes: u64,
    pub executable: bool,
}

#[derive(Debug, Clone)]
pub struct SkillPackage {
    pub root: PathBuf,
    pub manifest: SkillManifest,
    pub body: String,
    pub resources: Vec<SkillResource>,
    pub total_bytes: u64,
    pub requirements: Option<RequirementsFile>,
    pub lock: Option<SkillLock>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RequirementsFile {
    pub version: u32,
    pub semwright: SemwrightRequirements,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct SemwrightRequirements {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum_version: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<CapabilityRequirement>,
    #[serde(default)]
    pub semantic_packages: Vec<SemanticPackageRequirement>,
}

fn required_default() -> bool {
    true
}
fn one_default() -> usize {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CapabilityRequirement {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<RequirementQuery>,
    #[serde(default = "one_default")]
    pub minimum_matches: usize,
    #[serde(default = "required_default")]
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct RequirementQuery {
    #[serde(default)]
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub application: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub object_types: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SemanticPackageRequirement {
    pub id: String,
    #[serde(default = "required_default")]
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillLock {
    pub version: u32,
    pub semwright_version: String,
    pub requirements_sha256: String,
    pub entries: Vec<SkillLockEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillLockEntry {
    pub requirement: String,
    pub capability_id: String,
    pub provider: String,
    pub provider_version: String,
    pub capability_version: String,
    pub descriptor_sha256: String,
    pub schema_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogCapability {
    pub descriptor: CommandDescriptor,
    pub provenance: Metadata,
    #[serde(default)]
    pub routes: Vec<CatalogRoute>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_preview: Option<PolicyPreview>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PolicyPreview {
    pub state: String,
    pub preview_only: bool,
    pub execution_rechecks: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CatalogRoute {
    pub provider: String,
    pub status: String,
    pub available: bool,
}

impl CatalogCapability {
    pub fn available(&self) -> bool {
        self.routes.iter().any(|route| route.available)
            || self
                .descriptor
                .backends
                .iter()
                .any(|backend| backend == "core")
    }
}
