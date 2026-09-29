//! Port of the isolated pass's fixed-stage coordinator to C0/C1 contracts.
//! Reservations are private, single use and checked before every Broker dispatch.
use crate::*;
use schemars::JsonSchema;
use semwright_media_time::{MediaArtifact, Retention};
use semwright_semantic_composition::{
    self as c, BaseStateSet, Digest, ExecutionStatus, Owner, Verdict, VerificationReport,
    bounded_id, canonical_digest, ensure,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Instant;
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum StagePayload {
    PlanDelivery {
        profile: DeliveryProfile,
    },
    ApplyMotion {
        plan_ref: String,
    },
    ApplyAudio {
        plan_ref: String,
    },
    RenderMotion {
        plan_ref: String,
    },
    RenderAudio {
        plan_ref: String,
    },
    VerifyMotion {
        artifact: MediaArtifact,
        plan_ref: String,
    },
    VerifyAudio {
        artifact: MediaArtifact,
        plan_ref: String,
    },
    TransferMotion {
        artifact: MediaArtifact,
    },
    TransferAudio {
        artifact: MediaArtifact,
    },
    Mux {
        motion: Box<DeliveryInput>,
        audio: Box<DeliveryInput>,
        profile: DeliveryProfile,
    },
    VerifyFinalAudio {
        artifact: MediaArtifact,
        required_rules: std::collections::BTreeSet<String>,
    },
    VerifySync {
        artifact: MediaArtifact,
        spec: SyncSpec,
    },
    PreparePublication {
        manifest: Box<PublicationManifest>,
    },
    Publish {
        receipt: PublicationCandidate,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StageCall {
    pub request_id: String,
    pub owner: Owner,
    pub av_plan_digest: Digest,
    pub stage: Stage,
    pub proof: ServiceProof,
    pub expected_base: BaseStateSet,
    pub payload: StagePayload,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeliveryInput {
    pub token: String,
    pub source_digest: Digest,
    pub artifact_digest: Digest,
    pub owner: Owner,
    pub metadata: semwright_media_time::MediaMetadata,
    pub operation: TransferKind,
    pub verification: Option<VerificationReport>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TransferKind {
    ByteCopy,
    LosslessMezzanine,
    VerifiedTranscode,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeResult {
    DeliveryPlanned {
        profile_digest: Digest,
    },
    Applied,
    Rendered {
        artifact: MediaArtifact,
    },
    Verified {
        artifact_digest: Digest,
        report: VerificationReport,
    },
    Transferred {
        input: DeliveryInput,
    },
    Encoded {
        artifact: MediaArtifact,
    },
    SyncMeasured {
        probe: DecodedSyncProbe,
    },
    PublicationPrepared {
        candidate: PublicationCandidate,
    },
    Published {
        manifest_digest: Digest,
        pointer: String,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NativeReceipt {
    pub request_id: String,
    pub av_plan_digest: Digest,
    pub owner: Owner,
    pub stage: Stage,
    pub proof: ServiceProof,
    pub observed_base: BaseStateSet,
    pub status: ExecutionStatus,
    pub result: Option<NativeResult>,
    pub effects: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LedgerEntry {
    pub request_id: String,
    pub stage: Stage,
    pub status: ExecutionStatus,
    pub effects: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AvState {
    Prepared,
    Running,
    AwaitingReceipt,
    Verified,
    PartiallyApplied,
    Denied,
    Cancelled,
    Conflicted,
    Exhausted,
    Unknown,
    Failed,
}
#[derive(Default)]
struct Outputs {
    motion: Option<MediaArtifact>,
    audio: Option<MediaArtifact>,
    motion_verification: Option<VerificationReport>,
    audio_verification: Option<VerificationReport>,
    motion_input: Option<DeliveryInput>,
    audio_input: Option<DeliveryInput>,
    encoded: Option<MediaArtifact>,
    final_audio: Option<VerificationReport>,
    sync: Option<SyncReport>,
    publication: Option<PublicationCandidate>,
}
pub struct AvCoordinator {
    plan: AvPlan,
    expected: BaseStateSet,
    started: Instant,
    state: AvState,
    cursor: usize,
    serial: u64,
    reservation: Option<StageCall>,
    dispatched: bool,
    ledger: Vec<LedgerEntry>,
    outputs: Outputs,
}
impl AvCoordinator {
    pub fn new(plan: AvPlan) -> Result<Self> {
        plan.validate()?;
        Ok(Self {
            expected: plan.body.base.clone(),
            plan,
            started: Instant::now(),
            state: AvState::Prepared,
            cursor: 0,
            serial: 0,
            reservation: None,
            dispatched: false,
            ledger: vec![],
            outputs: Outputs::default(),
        })
    }
    pub fn state(&self) -> AvState {
        self.state.clone()
    }
    pub fn ledger(&self) -> &[LedgerEntry] {
        &self.ledger
    }
    pub fn next_stage(&self) -> Option<Stage> {
        Stage::ALL.get(self.cursor).copied()
    }
    pub fn expected_base(&self) -> &BaseStateSet {
        &self.expected
    }
    pub fn plan(&self) -> &AvPlan {
        &self.plan
    }
    fn budget(&mut self) -> Result<()> {
        if self.started.elapsed().as_millis() > u128::from(self.plan.body.budget.max_elapsed_ms)
            || self.serial >= u64::from(self.plan.body.budget.max_operations)
        {
            self.state = AvState::Exhausted;
            return Err(Error::Limit(
                "AV cumulative invocation/deadline budget exhausted".into(),
            ));
        }
        Ok(())
    }
    fn proof(&self, stage: Stage) -> Result<&ServiceProof> {
        self.plan
            .body
            .services
            .iter()
            .find(|p| p.service == stage.service())
            .ok_or_else(|| Error::Invalid("native service binding missing".into()))
    }
    fn check_context(
        &mut self,
        owner: &Owner,
        proof: &ServiceProof,
        fresh: &BaseStateSet,
    ) -> Result<()> {
        if *owner != self.plan.body.spec.owner {
            self.state = AvState::Denied;
            return Err(Error::Denied("AV owner changed".into()));
        }
        let stage = self
            .next_stage()
            .ok_or_else(|| Error::Invalid("AV already terminal".into()))?;
        if proof != self.proof(stage)? || !proof.available {
            self.state = AvState::Conflicted;
            return Err(Error::Stale(
                "native service generation/catalog/runtime/availability changed".into(),
            ));
        }
        if let Err(e) = self.expected.check_fresh(fresh, false) {
            self.state = AvState::Conflicted;
            return Err(e);
        }
        Ok(())
    }
    fn payload(&self, stage: Stage) -> Result<StagePayload> {
        let missing = || Error::Unknown("required prior-stage receipt is absent".into());
        let o = &self.outputs;
        let b = &self.plan.body;
        Ok(match stage {
            Stage::PlanDelivery => StagePayload::PlanDelivery {
                profile: b.spec.delivery.clone(),
            },
            Stage::ApplyMotion => StagePayload::ApplyMotion {
                plan_ref: b.motion.plan_ref.clone(),
            },
            Stage::ApplyAudio => StagePayload::ApplyAudio {
                plan_ref: b.audio.plan_ref.clone(),
            },
            Stage::RenderMotion => StagePayload::RenderMotion {
                plan_ref: b.motion.plan_ref.clone(),
            },
            Stage::RenderAudio => StagePayload::RenderAudio {
                plan_ref: b.audio.plan_ref.clone(),
            },
            Stage::VerifyMotion => StagePayload::VerifyMotion {
                artifact: o.motion.clone().ok_or_else(missing)?,
                plan_ref: b.motion.plan_ref.clone(),
            },
            Stage::VerifyAudio => StagePayload::VerifyAudio {
                artifact: o.audio.clone().ok_or_else(missing)?,
                plan_ref: b.audio.plan_ref.clone(),
            },
            Stage::TransferMotion => StagePayload::TransferMotion {
                artifact: o.motion.clone().ok_or_else(missing)?,
            },
            Stage::TransferAudio => StagePayload::TransferAudio {
                artifact: o.audio.clone().ok_or_else(missing)?,
            },
            Stage::Mux => StagePayload::Mux {
                motion: Box::new(o.motion_input.clone().ok_or_else(missing)?),
                audio: Box::new(o.audio_input.clone().ok_or_else(missing)?),
                profile: b.spec.delivery.clone(),
            },
            Stage::VerifyFinalAudio => StagePayload::VerifyFinalAudio {
                artifact: o.encoded.clone().ok_or_else(missing)?,
                required_rules: b.spec.required_final_audio_rules.clone(),
            },
            Stage::VerifySync => StagePayload::VerifySync {
                artifact: o.encoded.clone().ok_or_else(missing)?,
                spec: b.spec.sync.clone(),
            },
            Stage::PreparePublication => StagePayload::PreparePublication {
                manifest: Box::new(self.manifest()?),
            },
            Stage::Publish => StagePayload::Publish {
                receipt: o.publication.clone().ok_or_else(missing)?,
            },
        })
    }
    pub fn reserve(
        &mut self,
        owner: &Owner,
        proof: &ServiceProof,
        fresh: &BaseStateSet,
    ) -> Result<StageCall> {
        ensure(
            matches!(self.state, AvState::Prepared | AvState::Running),
            "AV state does not allow another stage",
        )?;
        ensure(self.reservation.is_none(), "a stage is already reserved")?;
        self.budget()?;
        self.check_context(owner, proof, fresh)?;
        let stage = self
            .next_stage()
            .ok_or_else(|| Error::Invalid("AV terminal".into()))?;
        self.serial += 1;
        let call = StageCall {
            request_id: format!("av-{}-{}", &self.plan.digest.as_str()[..16], self.serial),
            owner: owner.clone(),
            av_plan_digest: self.plan.digest.clone(),
            stage,
            proof: proof.clone(),
            expected_base: self.expected.clone(),
            payload: self.payload(stage)?,
        };
        self.reservation = Some(call.clone());
        self.dispatched = false;
        Ok(call)
    }
    /// The executor must reapply Broker policy after this provenance check.
    pub fn before_dispatch(
        &mut self,
        call: &StageCall,
        owner: &Owner,
        proof: &ServiceProof,
        fresh: &BaseStateSet,
    ) -> Result<()> {
        self.check_context(owner, proof, fresh)?;
        let reserved = self
            .reservation
            .as_ref()
            .ok_or_else(|| Error::Denied("stage was not reserved by this coordinator".into()))?;
        ensure(
            !self.dispatched && canonical_digest(call)? == canonical_digest(reserved)?,
            "stage tampering or dispatch replay",
        )?;
        if self.started.elapsed().as_millis() > u128::from(self.plan.body.budget.max_elapsed_ms) {
            self.state = AvState::Exhausted;
            return Err(Error::Limit("AV deadline expired before dispatch".into()));
        }
        self.dispatched = true;
        self.state = AvState::AwaitingReceipt;
        self.ledger.push(LedgerEntry {
            request_id: call.request_id.clone(),
            stage: call.stage,
            status: ExecutionStatus::Applying,
            effects: vec![],
        });
        Ok(())
    }
    pub fn complete(&mut self, receipt: NativeReceipt) -> Result<()> {
        ensure(
            self.state == AvState::AwaitingReceipt && self.dispatched,
            "unsolicited or duplicate native receipt",
        )?;
        let call = self
            .reservation
            .as_ref()
            .ok_or_else(|| Error::Unknown("active reservation lost".into()))?;
        let binding = receipt.request_id == call.request_id
            && receipt.owner == call.owner
            && receipt.av_plan_digest == call.av_plan_digest
            && receipt.stage == call.stage
            && receipt.proof == call.proof;
        if !binding {
            self.state = AvState::Unknown;
            return Err(Error::Denied(
                "native receipt does not bind to the actual dispatched request".into(),
            ));
        }
        if receipt.effects.len() > 4096
            || receipt
                .effects
                .iter()
                .any(|s| s.len() > 512 || s.chars().any(char::is_control))
        {
            self.state = AvState::Unknown;
            return Err(Error::Limit("native effect receipt exceeds bounds".into()));
        }
        let entry = self.ledger.last_mut().expect("dispatched ledger");
        entry.status = receipt.status;
        entry.effects = receipt.effects;
        self.reservation = None;
        self.dispatched = false;
        if receipt.status != ExecutionStatus::Completed {
            self.state = match receipt.status {
                ExecutionStatus::Denied => AvState::Denied,
                ExecutionStatus::Cancelled => AvState::Cancelled,
                ExecutionStatus::Partial => AvState::PartiallyApplied,
                ExecutionStatus::Unknown => AvState::Unknown,
                _ => AvState::Failed,
            };
            return Err(Error::Unknown("native stage did not complete; prior effects retained and publication remains blocked".into()));
        }
        let result = receipt
            .result
            .ok_or_else(|| Error::Unknown("completed native stage omitted its result".into()));
        let validation =
            result.and_then(|r| self.accept_result(receipt.stage, r, &receipt.observed_base));
        if let Err(error) = validation {
            self.state = AvState::Unknown;
            return Err(error);
        }
        self.expected = receipt.observed_base;
        self.cursor += 1;
        self.state = if self.cursor == Stage::ALL.len() {
            AvState::Verified
        } else {
            AvState::Running
        };
        Ok(())
    }
    fn validate_state_delta(&self, stage: Stage, after: &BaseStateSet) -> Result<()> {
        after.validate()?;
        ensure(
            after.0.len() == self.expected.0.len(),
            "native receipt changed resource-set membership",
        )?;
        let provider = &self.proof(stage)?.provider;
        for previous in &self.expected.0 {
            let actual = after
                .0
                .iter()
                .find(|s| s.key == previous.key)
                .ok_or_else(|| Error::Stale("native receipt omitted resource".into()))?;
            if previous.key.provider != *provider || !stage.mutates() {
                ensure(
                    actual == previous,
                    "native receipt attributes a change to an unrelated/read-only resource",
                )?;
            }
        }
        Ok(())
    }
    fn accept_result(
        &mut self,
        stage: Stage,
        result: NativeResult,
        after: &BaseStateSet,
    ) -> Result<()> {
        self.validate_state_delta(stage, after)?;
        let b = &self.plan.body;
        let expected_owner = &b.spec.owner;
        match (stage, result) {
            (Stage::PlanDelivery, NativeResult::DeliveryPlanned { profile_digest }) => ensure(
                profile_digest == canonical_digest(&b.spec.delivery)?,
                "delivery plan profile drift",
            )?,
            (Stage::ApplyMotion | Stage::ApplyAudio, NativeResult::Applied) => {}
            (Stage::RenderMotion | Stage::RenderAudio, NativeResult::Rendered { artifact }) => {
                artifact.validate()?;
                let subplan = if stage == Stage::RenderMotion {
                    &b.motion
                } else {
                    &b.audio
                };
                ensure(
                    artifact.owner == *expected_owner
                        && artifact.source_plan == subplan.plan_digest
                        && artifact.metadata.duration == b.spec.delivery.duration,
                    "render artifact owner/plan/duration mismatch",
                )?;
                ensure(
                    artifact.retention == Retention::PrivateCandidate,
                    "renderer must not publish the AV candidate",
                )?;
                if stage == Stage::RenderMotion {
                    let video = artifact.metadata.video.as_ref().ok_or_else(|| {
                        Error::Invalid("motion output lacks observed video stream".into())
                    })?;
                    ensure(
                        video.frame_rate == b.spec.delivery.frame_rate
                            && video.width == b.spec.delivery.width
                            && video.height == b.spec.delivery.height,
                        "motion output profile mismatch",
                    )?;
                    self.outputs.motion = Some(artifact);
                } else {
                    let audio = artifact.metadata.audio.as_ref().ok_or_else(|| {
                        Error::Invalid("audio output lacks observed audio stream".into())
                    })?;
                    ensure(
                        audio.sample_rate == b.spec.delivery.sample_rate
                            && audio.channels == b.spec.delivery.channels,
                        "audio output profile mismatch",
                    )?;
                    self.outputs.audio = Some(artifact);
                }
            }
            (
                Stage::VerifyMotion | Stage::VerifyAudio | Stage::VerifyFinalAudio,
                NativeResult::Verified {
                    artifact_digest,
                    report,
                },
            ) => {
                let (expected, rules, plan_digest, sources) = match stage {
                    Stage::VerifyMotion => (
                        self.outputs.motion.as_ref(),
                        &b.motion.required_rules,
                        &b.motion.plan_digest,
                        vec![
                            c::EvidenceSource::RendererState,
                            c::EvidenceSource::DecodedMedia,
                        ],
                    ),
                    Stage::VerifyAudio => (
                        self.outputs.audio.as_ref(),
                        &b.audio.required_rules,
                        &b.audio.plan_digest,
                        vec![
                            c::EvidenceSource::NativeApi,
                            c::EvidenceSource::DecodedMedia,
                        ],
                    ),
                    _ => (
                        self.outputs.encoded.as_ref(),
                        &b.spec.required_final_audio_rules,
                        &self.plan.digest,
                        vec![c::EvidenceSource::DecodedMedia],
                    ),
                };
                let artifact = expected.ok_or_else(|| {
                    Error::Unknown("verification has no produced artifact".into())
                })?;
                ensure(
                    artifact.sha256 == artifact_digest
                        && report.validation.plan_digest == *plan_digest,
                    "verification artifact/plan substitution",
                )?;
                crate::model::require_verified(&report, rules, &sources)?;
                ensure(
                    report
                        .validation
                        .checks
                        .iter()
                        .flat_map(|r| &r.evidence)
                        .all(|e| e.artifact.as_ref() == Some(&artifact_digest)),
                    "verification observations are not bound to the produced artifact",
                )?;
                match stage {
                    Stage::VerifyMotion => self.outputs.motion_verification = Some(report),
                    Stage::VerifyAudio => self.outputs.audio_verification = Some(report),
                    _ => self.outputs.final_audio = Some(report),
                }
            }
            (Stage::TransferMotion | Stage::TransferAudio, NativeResult::Transferred { input }) => {
                bounded_id(&input.token)?;
                input.metadata.validate()?;
                let artifact = if stage == Stage::TransferMotion {
                    self.outputs.motion.as_ref()
                } else {
                    self.outputs.audio.as_ref()
                }
                .ok_or_else(|| Error::Unknown("transfer without produced artifact".into()))?;
                ensure(
                    input.owner == *expected_owner && input.source_digest == artifact.sha256,
                    "handoff owner/source mismatch",
                )?;
                match input.operation {
                    TransferKind::ByteCopy => ensure(
                        input.artifact_digest == artifact.sha256
                            && input.metadata == artifact.metadata,
                        "byte copy changed artifact content or metadata",
                    )?,
                    TransferKind::LosslessMezzanine => {
                        ensure(
                            stage == Stage::TransferMotion,
                            "lossless frame mezzanine applies only to Motion transfer",
                        )?;
                        ensure(
                            input.metadata.duration == artifact.metadata.duration
                                && input.metadata.video == artifact.metadata.video
                                && input.metadata.audio.is_none()
                                && input.artifact_digest != artifact.sha256,
                            "lossless Motion mezzanine changed timeline/profile or did not create a new artifact",
                        )?;
                    }
                    TransferKind::VerifiedTranscode => {
                        ensure(
                            stage == Stage::TransferMotion,
                            "audio cannot be remixed or normalized inside transfer",
                        )?;
                        let report = input.verification.as_ref().ok_or_else(|| {
                            Error::Unknown("transcode needs decoded equivalence evidence".into())
                        })?;
                        crate::model::require_verified(
                            report,
                            &std::collections::BTreeSet::from(["decoded-frame-equivalence".into()]),
                            &[c::EvidenceSource::DecodedMedia],
                        )?;
                        ensure(
                            input.metadata.duration == artifact.metadata.duration
                                && input.metadata.video == artifact.metadata.video,
                            "transcode changed exact timeline/profile",
                        )?;
                        ensure(
                            report
                                .validation
                                .checks
                                .iter()
                                .flat_map(|r| &r.evidence)
                                .all(|e| e.artifact.as_ref() == Some(&input.artifact_digest)),
                            "transcode verification artifact mismatch",
                        )?;
                    }
                }
                if stage == Stage::TransferMotion {
                    self.outputs.motion_input = Some(input)
                } else {
                    self.outputs.audio_input = Some(input)
                }
            }
            (Stage::Mux, NativeResult::Encoded { artifact }) => {
                artifact.validate()?;
                ensure(
                    artifact.owner == *expected_owner
                        && artifact.source_plan == self.plan.digest
                        && artifact.retention == Retention::PrivateCandidate
                        && artifact.bytes <= b.spec.delivery.max_artifact_bytes,
                    "mux produced an invalid, public or excessive candidate",
                )?;
                let video = artifact
                    .metadata
                    .video
                    .as_ref()
                    .ok_or_else(|| Error::Invalid("mux omitted video".into()))?;
                let audio = artifact
                    .metadata
                    .audio
                    .as_ref()
                    .ok_or_else(|| Error::Invalid("mux omitted audio".into()))?;
                let decoded_video_duration = video.frame_rate.at(i64::try_from(video.frames)
                    .map_err(|_| Error::Limit("decoded video frame count".into()))?)?;
                ensure(
                    artifact.metadata.duration == b.spec.delivery.duration
                        && decoded_video_duration == b.spec.delivery.duration
                        && video.width == b.spec.delivery.width
                        && video.height == b.spec.delivery.height
                        && video.frame_rate == b.spec.delivery.frame_rate
                        && audio.sample_rate == b.spec.delivery.sample_rate
                        && audio.channels == b.spec.delivery.channels,
                    "mux presentation duration or stream profile mismatch",
                )?;
                self.outputs.encoded = Some(artifact);
            }
            (Stage::VerifySync, NativeResult::SyncMeasured { probe }) => {
                let artifact = self
                    .outputs
                    .encoded
                    .as_ref()
                    .ok_or_else(|| Error::Unknown("sync has no encoded artifact".into()))?;
                ensure(
                    probe.artifact_digest == artifact.sha256,
                    "decoded sync probe is bound to a different encoded artifact",
                )?;
                let report = verify_sync(&b.spec.sync, &probe)?;
                ensure(
                    report.verdict == Verdict::Pass && report.missing.is_empty(),
                    "encoded sync is missing, failed or uncertain",
                )?;
                ensure(
                    report.observations.len() == b.spec.sync.cues.len()
                        && (!b.spec.sync.require_full_scan || report.exhaustive),
                    "sync coverage does not satisfy the original requirements",
                )?;
                self.outputs.sync = Some(report);
            }
            (Stage::PreparePublication, NativeResult::PublicationPrepared { candidate }) => {
                candidate.validate()?;
                ensure(
                    candidate.owner == *expected_owner
                        && candidate.manifest_digest == canonical_digest(&self.manifest()?)?,
                    "publication candidate content/owner mismatch",
                )?;
                self.outputs.publication = Some(candidate);
            }
            (
                Stage::Publish,
                NativeResult::Published {
                    manifest_digest,
                    pointer,
                },
            ) => {
                let candidate = self
                    .outputs
                    .publication
                    .as_ref()
                    .ok_or_else(|| Error::Unknown("publication candidate absent".into()))?;
                ensure(
                    candidate.manifest_digest == manifest_digest
                        && candidate.destination_path == pointer,
                    "published pointer differs from the prepared candidate",
                )?;
            }
            _ => {
                return Err(Error::Invalid(
                    "native result kind does not match the dispatched AV stage".into(),
                ));
            }
        }
        Ok(())
    }
    pub fn manifest(&self) -> Result<PublicationManifest> {
        let absent =
            || Error::Unknown("a required native verification or artifact is still absent".into());
        let o = &self.outputs;
        let manifest = PublicationManifest {
            version: 1,
            av_plan_digest: self.plan.digest.clone(),
            owner: self.plan.body.spec.owner.clone(),
            motion_plan_digest: self.plan.body.motion.plan_digest.clone(),
            audio_plan_digest: self.plan.body.audio.plan_digest.clone(),
            cue_digest: self.plan.body.spec.cues.digest()?,
            delivery: self.plan.body.spec.delivery.clone(),
            final_artifact: o.encoded.clone().ok_or_else(absent)?,
            motion_verification: o.motion_verification.clone().ok_or_else(absent)?,
            audio_verification: o.audio_verification.clone().ok_or_else(absent)?,
            final_audio_verification: o.final_audio.clone().ok_or_else(absent)?,
            sync: o.sync.clone().ok_or_else(absent)?,
            ready_for: vec!["verified-rendered-av-v1".into()],
            r16_closed: false,
            promotional_video: false,
        };
        manifest.validate()?;
        Ok(manifest)
    }
    pub fn ready(&self) -> bool {
        self.state == AvState::Verified && self.outputs.publication.is_some()
    }
    pub fn cancel_before_dispatch(&mut self) -> Result<()> {
        ensure(
            !self.dispatched,
            "an active native call requires cooperative cancellation and an observed terminal receipt",
        )?;
        self.state = AvState::Cancelled;
        self.reservation = None;
        Ok(())
    }
    pub fn lost_receipt(&mut self) -> Result<()> {
        ensure(
            self.dispatched && self.state == AvState::AwaitingReceipt,
            "no active request receipt is outstanding",
        )?;
        if let Some(entry) = self.ledger.last_mut() {
            entry.status = ExecutionStatus::Unknown;
        }
        self.state = AvState::Unknown;
        self.dispatched = false;
        self.reservation = None;
        // Never retry a non-idempotent native call after losing its result.
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "decision", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReuseDecision {
    Reuse { artifact_digest: Digest },
    Rebuild { invalidated: Vec<String> },
}
/// Reuse requires a complete versioned dependency set supplied by the planner.
/// Reuse of the video/audio intermediate never reuses final encode verification.
pub fn classify_reuse(
    artifact: &MediaArtifact,
    owner: &Owner,
    current: &BTreeMap<String, Digest>,
) -> Result<ReuseDecision> {
    artifact.validate()?;
    ensure(
        artifact.owner == *owner,
        "cannot reuse another owner's artifact",
    )?;
    ensure(
        !artifact.dependencies.is_empty(),
        "artifact without dependency provenance is not reusable",
    )?;
    let invalidated = artifact
        .dependencies
        .iter()
        .filter(|(key, digest)| current.get(*key) != Some(*digest))
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    Ok(if invalidated.is_empty() {
        ReuseDecision::Reuse {
            artifact_digest: artifact.sha256.clone(),
        }
    } else {
        ReuseDecision::Rebuild { invalidated }
    })
}
