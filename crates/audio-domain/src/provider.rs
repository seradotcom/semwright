//! Optional audio-asset provider contract.
//!
//! AI generation is intentionally an asset-source concern. Generated assets
//! enter the same AudioProject as recorded/imported/deterministic assets and do
//! not gain execution authority or unlock semantic capabilities.

use crate::{Error, Result, time::SampleRate};
use serde::{Deserialize, Serialize};

pub const ASSET_PROVIDER_CONTRACT_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetProviderKind {
    DeterministicLocal,
    ExternalAi,
    Custom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GenerationKind {
    SoundEffect,
    Music,
    Speech,
    Foley,
    Ambience,
    InstrumentSample,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetProviderDescriptor {
    pub contract_version: u32,
    pub id: String,
    pub kind: AssetProviderKind,
    pub generation_kinds: Vec<GenerationKind>,
    pub network_required: bool,
}
impl AssetProviderDescriptor {
    pub fn validate(&self) -> Result<()> {
        if self.contract_version != ASSET_PROVIDER_CONTRACT_VERSION {
            return Err(Error::unsupported(
                "Unsupported audio asset-provider contract",
            ));
        }
        validate_id(&self.id)?;
        if self.generation_kinds.is_empty() || self.generation_kinds.len() > 16 {
            return Err(Error::invalid(
                "Audio asset provider generation kinds are invalid",
            ));
        }
        if self.kind == AssetProviderKind::DeterministicLocal && self.network_required {
            return Err(Error::invalid(
                "Deterministic local provider cannot require network access",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetGenerationRequest {
    pub contract_version: u32,
    pub kind: GenerationKind,
    pub prompt: String,
    pub duration_frames: u64,
    pub sample_rate: SampleRate,
    pub channels: u16,
    pub seed: Option<u64>,
}
impl AssetGenerationRequest {
    pub fn validate(&self) -> Result<()> {
        if self.contract_version != ASSET_PROVIDER_CONTRACT_VERSION {
            return Err(Error::unsupported("Unsupported asset-generation request"));
        }
        if self.prompt.is_empty()
            || self.prompt.len() > 16_384
            || self.prompt.chars().any(char::is_control)
            || self.duration_frames == 0
            || self.duration_frames > 384_000 * 60 * 30
            || !(1..=16).contains(&self.channels)
        {
            return Err(Error::invalid(
                "Invalid bounded audio asset-generation request",
            ));
        }
        self.sample_rate.validate()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetGenerationReceipt {
    pub provider_id: String,
    pub provider_request_id: Option<String>,
    pub model_id: Option<String>,
    pub content_sha256: String,
    pub deterministic: bool,
}
impl AssetGenerationReceipt {
    pub fn validate(&self) -> Result<()> {
        validate_id(&self.provider_id)?;
        if self.content_sha256.len() != 64
            || !self.content_sha256.bytes().all(|b| b.is_ascii_hexdigit())
            || self
                .provider_request_id
                .iter()
                .chain(self.model_id.iter())
                .any(|v| v.is_empty() || v.len() > 256 || v.chars().any(char::is_control))
        {
            return Err(Error::invalid("Invalid audio asset-generation receipt"));
        }
        Ok(())
    }
}

fn validate_id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/'))
        || value.contains("..")
        || value.starts_with('/')
        || value.ends_with('/')
        || value.contains("//")
    {
        return Err(Error::invalid("Invalid audio asset-provider ID"));
    }
    Ok(())
}
