use crate::Result;
use schemars::JsonSchema;
use semwright_media_time::{CueGraph, MediaArtifact, Rate, Rational};
use semwright_semantic_composition::{
    self as c, BaseStateSet, ConvergenceBudget, Digest, Owner, VerificationReport, bounded_id,
    canonical_digest, ensure,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Service {
    Motion,
    Audio,
    Delivery,
    Artifacts,
    Decode,
}
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    PlanDelivery,
    ApplyMotion,
    ApplyAudio,
    RenderMotion,
    RenderAudio,
    VerifyMotion,
    VerifyAudio,
    TransferMotion,
    TransferAudio,
    Mux,
    VerifyFinalAudio,
    VerifySync,
    PreparePublication,
    Publish,
}
impl Stage {
    pub const ALL: [Self; 14] = [
        Self::PlanDelivery,
        Self::ApplyMotion,
        Self::ApplyAudio,
        Self::RenderMotion,
        Self::RenderAudio,
        Self::VerifyMotion,
        Self::VerifyAudio,
        Self::TransferMotion,
        Self::TransferAudio,
        Self::Mux,
        Self::VerifyFinalAudio,
        Self::VerifySync,
        Self::PreparePublication,
        Self::Publish,
    ];
    pub fn service(self) -> Service {
        match self {
            Self::ApplyMotion | Self::RenderMotion | Self::VerifyMotion => Service::Motion,
            Self::ApplyAudio | Self::RenderAudio | Self::VerifyAudio | Self::VerifyFinalAudio => {
                Service::Audio
            }
            Self::PlanDelivery | Self::TransferMotion | Self::Mux => Service::Delivery,
            Self::TransferAudio | Self::PreparePublication | Self::Publish => Service::Artifacts,
            Self::VerifySync => Service::Decode,
        }
    }
    pub fn mutates(self) -> bool {
        !matches!(
            self,
            Self::PlanDelivery
                | Self::VerifyMotion
                | Self::VerifyAudio
                | Self::VerifyFinalAudio
                | Self::VerifySync
        )
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CommandProof {
    pub command: String,
    pub descriptor: Digest,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ServiceProof {
    pub service: Service,
    pub provider: String,
    pub generation: u64,
    pub catalog_digest: Digest,
    pub runtime_digest: Digest,
    pub commands: BTreeMap<Stage, Vec<CommandProof>>,
    pub available: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Subplan {
    pub version: u32,
    pub service: Service,
    pub owner: Owner,
    pub plan_ref: String,
    pub plan_digest: Digest,
    pub base: BaseStateSet,
    pub cue_digest: Digest,
    pub duration: Rational,
    pub dependencies: BTreeMap<String, Digest>,
    pub required_rules: BTreeSet<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryCodec {
    Mp4H264Aac,
    WebmVp9Opus,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeliveryProfile {
    pub codec: DeliveryCodec,
    pub frame_rate: Rate,
    pub width: u32,
    pub height: u32,
    pub duration: Rational,
    pub sample_rate: u32,
    pub channels: u16,
    pub audio_is_final_mix: bool,
    pub max_artifact_bytes: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AvSpec {
    pub version: u32,
    pub id: String,
    pub owner: Owner,
    pub cues: CueGraph,
    pub delivery: DeliveryProfile,
    pub sync: crate::SyncSpec,
    pub required_final_audio_rules: BTreeSet<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AvPlanBody {
    pub version: u32,
    pub spec: AvSpec,
    pub motion: Subplan,
    pub audio: Subplan,
    pub base: BaseStateSet,
    pub services: Vec<ServiceProof>,
    pub budget: ConvergenceBudget,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AvPlan {
    pub body: AvPlanBody,
    pub digest: Digest,
}
impl AvPlan {
    pub fn prepare(body: AvPlanBody) -> Result<Self> {
        let digest = canonical_digest(&body)?;
        let value = Self { body, digest };
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<()> {
        let b = &self.body;
        b.spec.owner.validate()?;
        b.base.validate()?;
        b.budget.validate()?;
        ensure(b.version == 1 && b.spec.version == 1, "AV contract version")?;
        bounded_id(&b.spec.id)?;
        ensure(
            self.digest == canonical_digest(b)?,
            "AV plan digest mismatch",
        )?;
        b.spec.delivery.validate()?;
        b.spec.sync.validate()?;
        let cues = b.spec.cues.digest()?;
        for (plan, service) in [(&b.motion, Service::Motion), (&b.audio, Service::Audio)] {
            plan.base.validate()?;
            plan.duration.validate()?;
            bounded_id(&plan.plan_ref)?;
            ensure(
                plan.version == 1 && plan.service == service && plan.owner == b.spec.owner,
                "subplan owner/service mismatch",
            )?;
            ensure(
                plan.cue_digest == cues && plan.duration == b.spec.delivery.duration,
                "subplan cue/duration mismatch; no implicit narration stretch",
            )?;
            ensure(
                !plan.required_rules.is_empty()
                    && plan.required_rules.len() <= 256
                    && plan.dependencies.len() <= 256,
                "subplan rule/dependency budget",
            )?;
            for key in plan.required_rules.iter().chain(plan.dependencies.keys()) {
                bounded_id(key)?;
            }
            for state in &plan.base.0 {
                ensure(
                    b.base.0.iter().any(|s| s == state),
                    "subplan base not in AV resource set",
                )?;
            }
        }
        ensure(
            !b.spec.required_final_audio_rules.is_empty()
                && b.spec.required_final_audio_rules.len() <= 256,
            "final audio analysis rules required",
        )?;
        ensure(
            b.services.len() == 5,
            "AV requires five explicit service bindings",
        )?;
        let mut services = BTreeSet::new();
        for proof in &b.services {
            bounded_id(&proof.provider)?;
            ensure(
                services.insert(proof.service) && proof.available,
                "duplicate or unavailable native service; no fallback",
            )?;
            for stage in Stage::ALL
                .into_iter()
                .filter(|s| s.service() == proof.service)
            {
                let commands = proof.commands.get(&stage).ok_or_else(|| {
                    c::ContractError::Invalid(
                        "native service lacks a required stage binding".into(),
                    )
                })?;
                ensure(
                    !commands.is_empty() && commands.len() <= 16,
                    "native stage command binding budget",
                )?;
                let mut names = BTreeSet::new();
                for command in commands {
                    bounded_id(&command.command)?;
                    ensure(
                        names.insert(command.command.as_str()),
                        "duplicate native command in one AV stage",
                    )?;
                }
            }
            ensure(
                proof
                    .commands
                    .keys()
                    .all(|stage| stage.service() == proof.service),
                "native service proof contains a foreign stage",
            )?;
        }
        ensure(
            b.budget.max_operations >= 14,
            "AV budget cannot cover the fixed stage graph",
        )?;
        Ok(())
    }
}
impl DeliveryProfile {
    pub fn validate(&self) -> Result<()> {
        self.frame_rate.validate()?;
        self.duration.validate()?;
        ensure(
            self.duration > Rational::ZERO && self.duration <= Rational::new(600, 1)?,
            "delivery duration limits",
        )?;
        ensure(
            self.frame_rate
                .quantize(self.duration, semwright_media_time::Round::NearestAway)?
                .error
                == Rational::ZERO,
            "delivery must end at an exact frame boundary",
        )?;
        ensure(
            (16..=4096).contains(&self.width)
                && (16..=4096).contains(&self.height)
                && [44100, 48000, 96000].contains(&self.sample_rate)
                && (1..=8).contains(&self.channels),
            "delivery stream profile bounds",
        )?;
        ensure(
            self.audio_is_final_mix,
            "MLT must not silently apply a second mix, ducking or normalization",
        )?;
        ensure(
            self.max_artifact_bytes > 0 && self.max_artifact_bytes <= 1_073_741_824,
            "delivery artifact byte limit",
        )?;
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AudioConsumerReceipt {
    pub version: u32,
    pub project: Subplan,
    pub master: MediaArtifact,
    pub stems: Vec<MediaArtifact>,
    pub verification: VerificationReport,
    pub cue_digest: Digest,
}
impl AudioConsumerReceipt {
    pub fn validate(&self) -> Result<()> {
        ensure(
            self.version == 1 && self.project.service == Service::Audio && self.stems.len() <= 64,
            "audio consumer version/service/stems",
        )?;
        self.master.validate()?;
        ensure(
            self.master.owner == self.project.owner
                && self.master.source_plan == self.project.plan_digest
                && self.master.metadata.audio.is_some(),
            "audio master owner/source/stream mismatch",
        )?;
        ensure(
            self.cue_digest == self.project.cue_digest
                && self.verification.validation.plan_digest == self.project.plan_digest,
            "audio verification/timing binding mismatch",
        )?;
        require_verified(
            &self.verification,
            &self.project.required_rules,
            &[
                c::EvidenceSource::NativeApi,
                c::EvidenceSource::DecodedMedia,
            ],
        )?;
        for stem in &self.stems {
            stem.validate()?;
            ensure(
                stem.owner == self.project.owner && stem.source_plan == self.project.plan_digest,
                "stem provenance mismatch",
            )?;
        }
        Ok(())
    }
}
pub(crate) fn require_verified(
    report: &VerificationReport,
    rules: &BTreeSet<String>,
    sources: &[c::EvidenceSource],
) -> Result<()> {
    ensure(
        report.validation.required_rules == *rules,
        "verification requirement substitution",
    )?;
    ensure(
        report.verdict()? == c::Verdict::Pass,
        "required native verification is not PASS",
    )?;
    ensure(
        report.support_level == c::SupportLevel::Native
            || report.support_level == c::SupportLevel::Composed,
        "unsupported service cannot certify AV",
    )?;
    for check in &report.validation.checks {
        if rules.contains(&check.rule) {
            ensure(
                check.evidence.iter().all(|e| sources.contains(&e.source)),
                "fixture/simulation or wrong-source evidence cannot certify native output",
            )?;
        }
    }
    Ok(())
}
