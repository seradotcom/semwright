//! Concrete Agent-A stage realization through descriptor-pinned Broker commands.
//! Audio authoring/final-audio analysis remain delegated to Agent B's public provider.
use crate::{
    ArtifactHandoffHint, AvPlan, DeliveryCodec, DeliveryInput, Error, NativeResult, Result, Stage,
    StageCall, StageCommandRunner, StagePayload, TransferKind,
};
use semwright_media_time::{
    AudioMetadata, MediaArtifact, MediaMetadata, Rational, Retention, Round, VideoMetadata,
};
use semwright_recipes::Executor;
use semwright_semantic_composition::{
    self as c, Digest, EvidenceSource, VerificationReport, canonical_digest, ensure,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug)]
struct Locator {
    root: String,
    path: String,
    sha256: Digest,
}

#[derive(Clone, Debug)]
pub struct AgentAArtifactRoutes {
    /// Broker filesystem grant containing B's verified public audio output.
    pub audio_source_root: String,
    /// Broker filesystem grant that receives the byte-copy handoff.
    pub handoff_destination_root: String,
    /// MLT workspace root alias mapped by owner configuration to that destination.
    pub mlt_media_root: String,
}

impl AgentAArtifactRoutes {
    pub fn validate(&self) -> Result<()> {
        semwright_semantic_composition::bounded_id(&self.audio_source_root)?;
        semwright_semantic_composition::bounded_id(&self.handoff_destination_root)?;
        ensure(
            matches!(self.mlt_media_root.as_str(), "project" | "media" | "output"),
            "MLT media root alias is outside the curated driver roots",
        )
    }
}

pub fn agent_a_stage_commands(stage: Stage) -> Option<&'static [&'static str]> {
    Some(match stage {
        Stage::PlanDelivery => &["driver.mlt-video.render.profiles"],
        Stage::ApplyMotion => &["driver.motion-canvas.composition.apply"],
        Stage::RenderMotion => &[
            "driver.motion-canvas.render.plan",
            "driver.motion-canvas.render.execute",
        ],
        Stage::VerifyMotion => &["driver.motion-canvas.composition.verify"],
        Stage::TransferMotion => &["driver.mlt-video.frames.encode"],
        Stage::TransferAudio => &["artifact.handoff"],
        Stage::Mux => &["driver.mlt-video.av.mux"],
        Stage::VerifyFinalAudio => &["driver.audio-analysis.artifact.measure"],
        Stage::VerifySync => &["driver.mlt-video.sync.probe"],
        _ => return None,
    })
}

pub struct AgentAStageAdapter {
    plan: AvPlan,
    artifact_routes: Option<AgentAArtifactRoutes>,
    motion_fingerprint: Option<String>,
    motion_job_ref: Option<String>,
    source_locators: BTreeMap<String, Locator>,
    locators: BTreeMap<String, Locator>,
}

impl AgentAStageAdapter {
    pub fn new(plan: AvPlan) -> Result<Self> {
        Self::with_artifact_routes(plan, None)
    }

    pub fn with_artifact_routes(
        plan: AvPlan,
        artifact_routes: Option<AgentAArtifactRoutes>,
    ) -> Result<Self> {
        plan.validate()?;
        if let Some(routes) = &artifact_routes {
            routes.validate()?;
        }
        Ok(Self {
            plan,
            artifact_routes,
            motion_fingerprint: None,
            motion_job_ref: None,
            source_locators: BTreeMap::new(),
            locators: BTreeMap::new(),
        })
    }

    pub fn plan(&self) -> &AvPlan {
        &self.plan
    }

    fn commands(call: &StageCall, expected: &[&str]) -> Result<()> {
        let actual = call
            .proof
            .commands
            .get(&call.stage)
            .ok_or_else(|| Error::Invalid("AV stage command proof missing".into()))?
            .iter()
            .map(|binding| binding.command.as_str())
            .collect::<Vec<_>>();
        ensure(
            actual == expected,
            "AV stage command mapping differs from the reviewed Agent-A adapter",
        )
    }

    fn frame_count(&self) -> Result<u64> {
        let quantized = self
            .plan
            .body
            .spec
            .delivery
            .frame_rate
            .quantize(self.plan.body.spec.delivery.duration, Round::NearestAway)?;
        ensure(
            quantized.error == Rational::ZERO,
            "delivery duration is not an exact frame boundary",
        )?;
        u64::try_from(quantized.index)
            .map_err(|_| Error::Limit("negative/overflow AV frame count".into()))
    }

    fn render_profile(&self) -> Result<Value> {
        let frames = self.frame_count()?;
        Ok(json!({
            "first_frame": 0,
            "end_frame_exclusive": frames,
            "scale": "full",
            "transparent": false,
            "timeout_ms": 300000
        }))
    }

    fn token(kind: &str, digest: &Digest) -> String {
        format!("artifact:{kind}-{}", &digest.as_str()[..16])
    }

    fn digest_field(value: &Value, field: &str) -> Result<Digest> {
        Digest::parse(
            value
                .get(field)
                .and_then(Value::as_str)
                .ok_or_else(|| Error::Invalid(format!("provider omitted {field}")))?
                .to_owned(),
        )
        .map_err(|error| Error::Invalid(error.to_string()))
    }

    fn optional_u64_default(value: &Value, field: &str, default: u64) -> Result<u64> {
        match value.get(field) {
            None => Ok(default),
            Some(value) => value
                .as_u64()
                .ok_or_else(|| Error::Invalid(format!("provider returned invalid {field}"))),
        }
    }

    fn relative(path: &str) -> Result<()> {
        ensure(
            !path.is_empty()
                && path.len() <= 4096
                && !path.starts_with('/')
                && !path.contains('\\')
                && !path.contains(':')
                && !path.chars().any(char::is_control)
                && path
                    .split('/')
                    .all(|part| !part.is_empty() && part != "." && part != ".."),
            "provider locator is not a bounded grant-relative path",
        )
    }

    fn locator(&self, token: &str, digest: &Digest) -> Result<&Locator> {
        let locator = self
            .locators
            .get(token)
            .ok_or_else(|| Error::Unknown("private artifact locator is unavailable".into()))?;
        ensure(
            &locator.sha256 == digest,
            "artifact token/digest binding changed",
        )?;
        Ok(locator)
    }

    /// Bind the filesystem-grant locator reported by the public audio provider/integration.
    /// The public MediaArtifact token remains path-free. This mapping grants no authority:
    /// the later copy still traverses Broker policy and validates the expected digest.
    pub fn bind_audio_source_locator(
        &mut self,
        artifact: &MediaArtifact,
        path: &str,
    ) -> Result<()> {
        artifact.validate()?;
        ensure(
            artifact.owner == self.plan.body.spec.owner
                && artifact.source_plan == self.plan.body.audio.plan_digest
                && artifact.metadata.audio.is_some()
                && artifact.metadata.video.is_none(),
            "audio source locator does not describe the planned final audio artifact",
        )?;
        let routes = self.artifact_routes.as_ref().ok_or_else(|| {
            Error::Denied("audio locator binding requires owner-configured artifact routes".into())
        })?;
        routes.validate()?;
        Self::relative(path)?;
        if let Some(existing) = self.source_locators.get(&artifact.reference) {
            return ensure(
                existing.root == routes.audio_source_root
                    && existing.path == path
                    && existing.sha256 == artifact.sha256,
                "audio source locator cannot be rebound after it is pinned",
            );
        }
        self.source_locators.insert(
            artifact.reference.clone(),
            Locator {
                root: routes.audio_source_root.clone(),
                path: path.into(),
                sha256: artifact.sha256.clone(),
            },
        );
        Ok(())
    }

    /// Consume B's common public receipt without coupling AV to Ardour/Faust result shapes.
    /// The hint contains only a relative path + digest; the Broker root comes from host config.
    pub fn bind_audio_consumer_receipt(
        &mut self,
        receipt: &crate::AudioConsumerReceipt,
    ) -> Result<()> {
        receipt.validate()?;
        ensure(
            receipt.project.plan_digest == self.plan.body.audio.plan_digest
                && receipt.project.owner == self.plan.body.spec.owner,
            "audio consumer receipt belongs to another AV plan/owner",
        )?;
        let handoff = receipt.handoff.as_ref().ok_or_else(|| {
            Error::Unknown(
                "audio consumer receipt has no artifact handoff hint for AV delivery".into(),
            )
        })?;
        ensure(
            handoff.artifact_digest == receipt.master.sha256,
            "audio consumer handoff digest mismatch",
        )?;
        self.bind_audio_source_locator(&receipt.master, &handoff.relative_path)
    }

    /// Bind a locator returned by an already-authorized transfer provider.
    /// This remains useful to restore private adapter state after a persisted receipt.
    pub fn bind_transferred_locator(
        &mut self,
        input: &DeliveryInput,
        root: &str,
        path: &str,
    ) -> Result<()> {
        ensure(
            input.owner == self.plan.body.spec.owner,
            "foreign delivery input owner",
        )?;
        ensure(
            matches!(root, "project" | "media" | "output"),
            "unrecognized provider root",
        )?;
        Self::relative(path)?;
        self.locators.insert(
            input.token.clone(),
            Locator {
                root: root.into(),
                path: path.into(),
                sha256: input.artifact_digest.clone(),
            },
        );
        Ok(())
    }

    pub async fn execute(
        &mut self,
        call: &StageCall,
        executor: &dyn Executor,
        cancellation: CancellationToken,
    ) -> Result<NativeResult> {
        ensure(
            call.av_plan_digest == self.plan.digest,
            "Agent-A adapter received another AV plan",
        )?;
        ensure(
            call.owner == self.plan.body.spec.owner,
            "Agent-A adapter owner mismatch",
        )?;
        match call.stage {
            Stage::PlanDelivery => self.plan_delivery(call, executor, cancellation).await,
            Stage::ApplyMotion => self.apply_motion(call, executor, cancellation).await,
            Stage::RenderMotion => self.render_motion(call, executor, cancellation).await,
            Stage::VerifyMotion => self.verify_motion(call, executor, cancellation).await,
            Stage::TransferMotion => self.transfer_motion(call, executor, cancellation).await,
            Stage::TransferAudio => self.transfer_audio(call, executor, cancellation).await,
            Stage::Mux => self.mux(call, executor, cancellation).await,
            Stage::VerifyFinalAudio => self.verify_final_audio(call, executor, cancellation).await,
            Stage::VerifySync => self.verify_sync(call, executor, cancellation).await,
            Stage::ApplyAudio | Stage::RenderAudio | Stage::VerifyAudio => Err(Error::Unknown(
                "audio authoring/verification stage requires Agent B's verified public provider handoff".into(),
            )),
            Stage::PreparePublication | Stage::Publish => Err(Error::Invalid(
                "publication stages use BrokerPublisher with owner-configured roots".into(),
            )),
        }
    }

    async fn plan_delivery(
        &self,
        call: &StageCall,
        executor: &dyn Executor,
        cancellation: CancellationToken,
    ) -> Result<NativeResult> {
        Self::commands(call, &["driver.mlt-video.render.profiles"])?;
        let delivery = &self.plan.body.spec.delivery;
        ensure(
            delivery.codec == DeliveryCodec::Mp4H264Aac
                && delivery.sample_rate == 48_000
                && delivery.channels == 2,
            "current native MLT delivery adapter supports only MP4 H.264/AAC at 48 kHz stereo",
        )?;
        let mut runner = StageCommandRunner::new(executor, call)?;
        let value = runner.next(json!({}), cancellation).await?;
        runner.finish()?;
        let available = value
            .get("profiles")
            .and_then(Value::as_array)
            .is_some_and(|profiles| {
                profiles.iter().any(|profile| {
                    profile.get("id").and_then(Value::as_str) == Some("h264-1080p")
                        && profile.get("available").and_then(Value::as_bool) == Some(true)
                })
            });
        ensure(
            available,
            "pinned MLT runtime lacks H.264/AAC delivery services",
        )?;
        Ok(NativeResult::DeliveryPlanned {
            profile_digest: canonical_digest(delivery)?,
        })
    }

    async fn apply_motion(
        &mut self,
        call: &StageCall,
        executor: &dyn Executor,
        cancellation: CancellationToken,
    ) -> Result<NativeResult> {
        Self::commands(call, &["driver.motion-canvas.composition.apply"])?;
        let StagePayload::ApplyMotion { plan_ref } = &call.payload else {
            return Err(Error::Invalid("ApplyMotion payload mismatch".into()));
        };
        ensure(
            plan_ref == &self.plan.body.motion.plan_ref,
            "Motion plan ref substitution",
        )?;
        let mut runner = StageCommandRunner::new(executor, call)?;
        let value = runner
            .next(json!({"plan_ref":plan_ref,"dry_run":false}), cancellation)
            .await?;
        runner.finish()?;
        ensure(
            value.get("applied").and_then(Value::as_bool) == Some(true)
                && value.get("execution_status").and_then(Value::as_str) == Some("completed"),
            "Motion apply did not complete",
        )?;
        let fingerprint = value
            .get("fingerprint")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Invalid("Motion apply omitted source fingerprint".into()))?;
        ensure(
            fingerprint.len() == 64
                && fingerprint
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
            "Motion source fingerprint is malformed",
        )?;
        self.motion_fingerprint = Some(fingerprint.into());
        Ok(NativeResult::Applied)
    }

    async fn render_motion(
        &mut self,
        call: &StageCall,
        executor: &dyn Executor,
        cancellation: CancellationToken,
    ) -> Result<NativeResult> {
        Self::commands(
            call,
            &[
                "driver.motion-canvas.render.plan",
                "driver.motion-canvas.render.execute",
            ],
        )?;
        let StagePayload::RenderMotion { plan_ref } = &call.payload else {
            return Err(Error::Invalid("RenderMotion payload mismatch".into()));
        };
        ensure(
            plan_ref == &self.plan.body.motion.plan_ref,
            "Motion render plan ref substitution",
        )?;
        let fingerprint = self
            .motion_fingerprint
            .clone()
            .ok_or_else(|| Error::Unknown("Motion apply receipt is absent".into()))?;
        let profile = self.render_profile()?;
        let frames = self.frame_count()?;
        let delivery = &self.plan.body.spec.delivery;

        let mut runner = StageCommandRunner::new(executor, call)?;
        let planned = runner
            .next(json!({"profile":profile.clone()}), cancellation.clone())
            .await?;
        let planned_fps_denominator = Self::optional_u64_default(&planned, "fps_denominator", 1)?;
        ensure(
            planned.get("width").and_then(Value::as_u64) == Some(u64::from(delivery.width))
                && planned.get("height").and_then(Value::as_u64)
                    == Some(u64::from(delivery.height))
                && planned.get("fps").and_then(Value::as_u64)
                    == Some(u64::from(delivery.frame_rate.num))
                && planned_fps_denominator == u64::from(delivery.frame_rate.den)
                && planned.get("first_frame").and_then(Value::as_u64) == Some(0)
                && planned.get("end_frame_exclusive").and_then(Value::as_u64) == Some(frames),
            "Motion render plan differs from AV delivery profile",
        )?;

        let rendered = runner
            .next(
                json!({"expected_fingerprint":fingerprint,"profile":profile}),
                cancellation,
            )
            .await?;
        runner.finish()?;
        if rendered.get("state").and_then(Value::as_str) != Some("succeeded") {
            let failure_class = rendered
                .get("failure_class")
                .and_then(Value::as_str)
                .unwrap_or("unclassified");
            ensure(
                failure_class
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'_' || byte.is_ascii_digit())
                    && failure_class.len() <= 64,
                "Motion renderer returned an invalid failure classification",
            )?;
            return Err(Error::Unknown(format!(
                "Motion native render failed ({failure_class})"
            )));
        }
        let job_ref = rendered
            .get("job_ref")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Invalid("Motion render omitted job ref".into()))?
            .to_owned();
        let artifact = rendered
            .get("artifact")
            .and_then(Value::as_object)
            .ok_or_else(|| Error::Invalid("Motion render omitted artifact receipt".into()))?;
        let manifest_path = artifact
            .get("manifest")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Invalid("Motion artifact omitted manifest path".into()))?;
        Self::relative(manifest_path)?;
        let manifest_digest = Digest::parse(
            artifact
                .get("manifest_sha256")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::Invalid("Motion artifact omitted manifest digest".into()))?
                .to_owned(),
        )
        .map_err(|error| Error::Invalid(error.to_string()))?;
        let manifest_bytes = artifact
            .get("manifest_bytes")
            .and_then(Value::as_u64)
            .ok_or_else(|| Error::Invalid("Motion artifact omitted manifest byte count".into()))?;
        ensure(
            artifact.get("frame_count").and_then(Value::as_u64) == Some(frames)
                && manifest_bytes > 0
                && manifest_bytes <= 8 * 1024 * 1024,
            "Motion artifact receipt violates frame/manifest limits",
        )?;

        let reference = Self::token("motion-frames", &manifest_digest);
        self.locators.insert(
            reference.clone(),
            Locator {
                root: "output".into(),
                path: manifest_path.into(),
                sha256: manifest_digest.clone(),
            },
        );
        self.motion_job_ref = Some(job_ref);
        let mut dependencies = self.plan.body.motion.dependencies.clone();
        dependencies.insert(
            "motion-plan".into(),
            self.plan.body.motion.plan_digest.clone(),
        );
        Ok(NativeResult::Rendered {
            artifact: MediaArtifact {
                reference,
                owner: self.plan.body.spec.owner.clone(),
                sha256: manifest_digest,
                bytes: manifest_bytes,
                media_type: "application/vnd.semwright.motion-frame-sequence+json".into(),
                source_plan: self.plan.body.motion.plan_digest.clone(),
                source_state: self.plan.body.motion.base.clone(),
                metadata: MediaMetadata {
                    duration: delivery.duration,
                    encoded_duration: None,
                    video: Some(VideoMetadata {
                        width: delivery.width,
                        height: delivery.height,
                        frame_rate: delivery.frame_rate,
                        frames,
                        alpha: false,
                    }),
                    audio: None,
                },
                dependencies,
                provenance: Some("motion-canvas-native-frame-manifest-v1".into()),
                license: None,
                retention: Retention::PrivateCandidate,
            },
        })
    }

    async fn verify_motion(
        &self,
        call: &StageCall,
        executor: &dyn Executor,
        cancellation: CancellationToken,
    ) -> Result<NativeResult> {
        Self::commands(call, &["driver.motion-canvas.composition.verify"])?;
        let StagePayload::VerifyMotion { artifact, plan_ref } = &call.payload else {
            return Err(Error::Invalid("VerifyMotion payload mismatch".into()));
        };
        ensure(
            plan_ref == &self.plan.body.motion.plan_ref,
            "Motion verify plan ref substitution",
        )?;
        self.locator(&artifact.reference, &artifact.sha256)?;
        let job_ref = self
            .motion_job_ref
            .as_ref()
            .ok_or_else(|| Error::Unknown("Motion render job ref is absent".into()))?;
        let mut runner = StageCommandRunner::new(executor, call)?;
        let value = runner
            .next(json!({"plan_ref":plan_ref,"job_ref":job_ref}), cancellation)
            .await?;
        runner.finish()?;
        let report: VerificationReport = serde_json::from_value(
            value
                .get("report")
                .cloned()
                .ok_or_else(|| Error::Invalid("Motion verify omitted common report".into()))?,
        )
        .map_err(|error| Error::Invalid(format!("Motion verification report: {error}")))?;
        ensure(
            report.validation.plan_digest == self.plan.body.motion.plan_digest,
            "Motion verification belongs to another plan",
        )?;
        Ok(NativeResult::Verified {
            artifact_digest: artifact.sha256.clone(),
            report,
        })
    }

    async fn transfer_motion(
        &mut self,
        call: &StageCall,
        executor: &dyn Executor,
        cancellation: CancellationToken,
    ) -> Result<NativeResult> {
        Self::commands(call, &["driver.mlt-video.frames.encode"])?;
        let StagePayload::TransferMotion { artifact } = &call.payload else {
            return Err(Error::Invalid("TransferMotion payload mismatch".into()));
        };
        let locator = self.locator(&artifact.reference, &artifact.sha256)?.clone();
        let output_path = format!("av-motion-{}.mkv", &self.plan.digest.as_str()[..16]);
        let mut runner = StageCommandRunner::new(executor, call)?;
        let value = runner
            .next(
                json!({
                    "root":locator.root,
                    "manifest_path":locator.path,
                    "expected_manifest_sha256":artifact.sha256.as_str(),
                    "output_path":output_path,
                    "max_bytes":self.plan.body.spec.delivery.max_artifact_bytes
                }),
                cancellation,
            )
            .await?;
        runner.finish()?;
        let receipt = value
            .get("artifact")
            .and_then(Value::as_object)
            .ok_or_else(|| Error::Invalid("frames.encode omitted artifact".into()))?;
        let root = receipt
            .get("root")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Invalid("mezzanine root missing".into()))?;
        let path = receipt
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Invalid("mezzanine path missing".into()))?;
        Self::relative(path)?;
        let digest = Digest::parse(
            receipt
                .get("sha256")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::Invalid("mezzanine digest missing".into()))?
                .to_owned(),
        )
        .map_err(|error| Error::Invalid(error.to_string()))?;
        let bytes = receipt
            .get("bytes")
            .and_then(Value::as_u64)
            .ok_or_else(|| Error::Invalid("mezzanine byte count missing".into()))?;
        let video = artifact
            .metadata
            .video
            .as_ref()
            .ok_or_else(|| Error::Invalid("Motion source has no video metadata".into()))?;
        ensure(
            bytes > 0
                && bytes <= self.plan.body.spec.delivery.max_artifact_bytes
                && value.get("frame_count").and_then(Value::as_u64) == Some(video.frames)
                && value.get("width").and_then(Value::as_u64) == Some(u64::from(video.width))
                && value.get("height").and_then(Value::as_u64) == Some(u64::from(video.height))
                && value.get("fps_num").and_then(Value::as_u64)
                    == Some(u64::from(video.frame_rate.num))
                && value.get("fps_den").and_then(Value::as_u64)
                    == Some(u64::from(video.frame_rate.den)),
            "FFV1 mezzanine receipt differs from Motion frame/profile/byte budget",
        )?;
        let token = Self::token("motion-mezzanine", &digest);
        self.locators.insert(
            token.clone(),
            Locator {
                root: root.into(),
                path: path.into(),
                sha256: digest.clone(),
            },
        );
        Ok(NativeResult::Transferred {
            input: DeliveryInput {
                token,
                source_digest: artifact.sha256.clone(),
                artifact_digest: digest,
                owner: self.plan.body.spec.owner.clone(),
                metadata: artifact.metadata.clone(),
                operation: TransferKind::LosslessMezzanine,
                verification: None,
            },
        })
    }

    async fn transfer_audio(
        &mut self,
        call: &StageCall,
        executor: &dyn Executor,
        cancellation: CancellationToken,
    ) -> Result<NativeResult> {
        const MAX_HANDOFF_BYTES: u64 = 64 * 1024 * 1024;
        Self::commands(call, &["artifact.handoff"])?;
        let StagePayload::TransferAudio { artifact } = &call.payload else {
            return Err(Error::Invalid("TransferAudio payload mismatch".into()));
        };
        artifact.validate()?;
        ensure(
            artifact.owner == self.plan.body.spec.owner
                && artifact.source_plan == self.plan.body.audio.plan_digest
                && artifact.metadata.audio.is_some()
                && artifact.metadata.video.is_none()
                && matches!(artifact.media_type.as_str(), "audio/wav" | "audio/x-wav"),
            "audio transfer requires the verified planned WAV final mix",
        )?;
        ensure(
            artifact.bytes <= MAX_HANDOFF_BYTES,
            "audio artifact exceeds the existing bounded artifact.handoff limit",
        )?;
        let routes = self.artifact_routes.as_ref().ok_or_else(|| {
            Error::Denied(
                "audio transfer requires owner-configured artifact handoff/delivery roots".into(),
            )
        })?;
        routes.validate()?;
        let source = self
            .source_locators
            .get(&artifact.reference)
            .ok_or_else(|| {
                Error::Unknown(
                    "audio provider locator has not been bound from its public render receipt"
                        .into(),
                )
            })?
            .clone();
        ensure(
            source.sha256 == artifact.sha256,
            "audio source locator/digest binding changed",
        )?;
        let destination_path = format!("av-audio-{}.wav", &artifact.sha256.as_str()[..16]);
        let mut runner = StageCommandRunner::new(executor, call)?;
        let value = runner
            .next(
                json!({
                    "source_root":source.root.as_str(),
                    "source_path":source.path.as_str(),
                    "destination_root":routes.handoff_destination_root.as_str(),
                    "destination_path":destination_path.as_str(),
                    "max_bytes":artifact.bytes,
                    "expected_sha256":artifact.sha256.as_str(),
                    "semantic_type":"audio/final-mix",
                    "media_type":artifact.media_type.as_str()
                }),
                cancellation,
            )
            .await?;
        runner.finish()?;
        ensure(
            value.get("copied").and_then(Value::as_bool) == Some(true)
                && value.get("atomic").and_then(Value::as_bool) == Some(true)
                && value.get("bytes").and_then(Value::as_u64) == Some(artifact.bytes)
                && value.get("sha256").and_then(Value::as_str) == Some(artifact.sha256.as_str()),
            "artifact.handoff did not return the expected audio copy receipt",
        )?;
        let destination = value
            .get("destination")
            .and_then(Value::as_object)
            .ok_or_else(|| Error::Invalid("artifact.handoff destination receipt missing".into()))?;
        ensure(
            destination.get("root").and_then(Value::as_str)
                == Some(routes.handoff_destination_root.as_str())
                && destination.get("path").and_then(Value::as_str)
                    == Some(destination_path.as_str()),
            "artifact.handoff destination differs from owner-configured AV delivery root",
        )?;

        let token = Self::token("audio-delivery", &artifact.sha256);
        self.locators.insert(
            token.clone(),
            Locator {
                root: routes.mlt_media_root.clone(),
                path: destination_path,
                sha256: artifact.sha256.clone(),
            },
        );
        Ok(NativeResult::Transferred {
            input: DeliveryInput {
                token,
                source_digest: artifact.sha256.clone(),
                artifact_digest: artifact.sha256.clone(),
                owner: artifact.owner.clone(),
                metadata: artifact.metadata.clone(),
                operation: TransferKind::ByteCopy,
                verification: None,
            },
        })
    }

    async fn mux(
        &mut self,
        call: &StageCall,
        executor: &dyn Executor,
        cancellation: CancellationToken,
    ) -> Result<NativeResult> {
        Self::commands(call, &["driver.mlt-video.av.mux"])?;
        let StagePayload::Mux {
            motion,
            audio,
            profile,
        } = &call.payload
        else {
            return Err(Error::Invalid("Mux payload mismatch".into()));
        };
        ensure(
            canonical_digest(profile)? == canonical_digest(&self.plan.body.spec.delivery)?,
            "Mux delivery profile substitution",
        )?;
        let motion_locator = self
            .locator(&motion.token, &motion.artifact_digest)?
            .clone();
        let audio_locator = self.locator(&audio.token, &audio.artifact_digest)?.clone();
        let frames = self.frame_count()?;
        let output_path = format!("av-master-{}.mp4", &self.plan.digest.as_str()[..16]);
        let mut runner = StageCommandRunner::new(executor, call)?;
        let value = runner
            .next(
                json!({
                    "video_root":motion_locator.root,
                    "video_path":motion_locator.path,
                    "video_sha256":motion.artifact_digest.as_str(),
                    "audio_root":audio_locator.root,
                    "audio_path":audio_locator.path,
                    "audio_sha256":audio.artifact_digest.as_str(),
                    "width":profile.width,
                    "height":profile.height,
                    "fps_num":profile.frame_rate.num,
                    "fps_den":profile.frame_rate.den,
                    "frame_count":frames,
                    "sample_rate":profile.sample_rate,
                    "channels":profile.channels,
                    "profile":"h264-aac-mp4",
                    "output_path":output_path,
                    "max_bytes":profile.max_artifact_bytes
                }),
                cancellation,
            )
            .await?;
        runner.finish()?;
        let receipt = value
            .get("artifact")
            .and_then(Value::as_object)
            .ok_or_else(|| Error::Invalid("AV mux omitted artifact receipt".into()))?;
        let root = receipt
            .get("root")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Invalid("AV master root missing".into()))?;
        let path = receipt
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Invalid("AV master path missing".into()))?;
        Self::relative(path)?;
        let digest = Digest::parse(
            receipt
                .get("sha256")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::Invalid("AV master digest missing".into()))?
                .to_owned(),
        )
        .map_err(|error| Error::Invalid(error.to_string()))?;
        let bytes = receipt
            .get("bytes")
            .and_then(Value::as_u64)
            .ok_or_else(|| Error::Invalid("AV master byte count missing".into()))?;
        let sample_frames = value
            .get("audio_sample_frames")
            .and_then(Value::as_u64)
            .ok_or_else(|| Error::Invalid("AV mux omitted observed audio sample count".into()))?;
        let media = value
            .get("media")
            .and_then(Value::as_object)
            .ok_or_else(|| Error::Invalid("AV mux omitted decoded media metadata".into()))?;
        let duration_num = i64::try_from(
            media
                .get("duration_num")
                .and_then(Value::as_u64)
                .ok_or_else(|| Error::Invalid("AV mux duration numerator missing".into()))?,
        )
        .map_err(|_| Error::Limit("AV mux duration numerator overflow".into()))?;
        let duration_den = i64::try_from(
            media
                .get("duration_den")
                .and_then(Value::as_u64)
                .ok_or_else(|| Error::Invalid("AV mux duration denominator missing".into()))?,
        )
        .map_err(|_| Error::Limit("AV mux duration denominator overflow".into()))?;
        let encoded_duration = Rational::new(duration_num, duration_den)?;
        let metadata = MediaMetadata {
            duration: profile.duration,
            encoded_duration: Some(encoded_duration),
            video: Some(VideoMetadata {
                width: profile.width,
                height: profile.height,
                frame_rate: profile.frame_rate,
                frames,
                alpha: false,
            }),
            audio: Some(AudioMetadata {
                sample_rate: profile.sample_rate,
                channels: profile.channels,
                channel_layout: if profile.channels == 2 {
                    "stereo".into()
                } else {
                    format!("channels-{}", profile.channels)
                },
                sample_frames,
                priming_samples: None,
                padding_samples: None,
                latency_samples: None,
                tail_samples: None,
            }),
        };
        let reference = Self::token("av-master", &digest);
        self.locators.insert(
            reference.clone(),
            Locator {
                root: root.into(),
                path: path.into(),
                sha256: digest.clone(),
            },
        );
        let encoded_artifact = MediaArtifact {
            reference,
            owner: self.plan.body.spec.owner.clone(),
            sha256: digest.clone(),
            bytes,
            media_type: "video/mp4".into(),
            source_plan: self.plan.digest.clone(),
            source_state: call.expected_base.clone(),
            metadata,
            dependencies: BTreeMap::from([
                ("motion-artifact".into(), motion.artifact_digest.clone()),
                ("audio-artifact".into(), audio.artifact_digest.clone()),
            ]),
            provenance: Some("mlt-native-av-mux-v1".into()),
            license: None,
            retention: Retention::PrivateCandidate,
        };

        let decoded = value
            .get("decoded_audio")
            .and_then(Value::as_object)
            .ok_or_else(|| Error::Invalid("AV mux omitted decoded final audio artifact".into()))?;
        let decoded_root = decoded
            .get("root")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Invalid("decoded final audio root missing".into()))?;
        let decoded_path = decoded
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Invalid("decoded final audio path missing".into()))?;
        Self::relative(decoded_path)?;
        ensure(
            decoded_root == root,
            "decoded final audio was published outside the AV output root",
        )?;
        let decoded_digest = Digest::parse(
            decoded
                .get("sha256")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::Invalid("decoded final audio digest missing".into()))?
                .to_owned(),
        )
        .map_err(|error| Error::Invalid(error.to_string()))?;
        let decoded_bytes = decoded
            .get("bytes")
            .and_then(Value::as_u64)
            .ok_or_else(|| Error::Invalid("decoded final audio byte count missing".into()))?;
        ensure(
            decoded_bytes > 0 && decoded_bytes <= profile.max_artifact_bytes,
            "decoded final audio exceeds AV artifact budget",
        )?;
        let decoded_media = value
            .get("decoded_audio_media")
            .and_then(Value::as_object)
            .ok_or_else(|| Error::Invalid("decoded final audio media probe missing".into()))?;
        ensure(
            decoded_media.get("audio").and_then(Value::as_bool) == Some(true)
                && decoded_media.get("video").and_then(Value::as_bool) == Some(false),
            "decoded final audio probe is not audio-only",
        )?;
        let decoded_frames = value
            .get("decoded_audio_sample_frames")
            .and_then(Value::as_u64)
            .ok_or_else(|| Error::Invalid("decoded final audio sample count missing".into()))?;
        let decoded_duration_num = i64::try_from(
            decoded_media
                .get("duration_num")
                .and_then(Value::as_u64)
                .ok_or_else(|| {
                    Error::Invalid("decoded final audio duration numerator missing".into())
                })?,
        )
        .map_err(|_| Error::Limit("decoded final audio duration numerator overflow".into()))?;
        let decoded_duration_den = i64::try_from(
            decoded_media
                .get("duration_den")
                .and_then(Value::as_u64)
                .ok_or_else(|| {
                    Error::Invalid("decoded final audio duration denominator missing".into())
                })?,
        )
        .map_err(|_| Error::Limit("decoded final audio duration denominator overflow".into()))?;
        let decoded_duration = Rational::new(decoded_duration_num, decoded_duration_den)?;
        let decoded_reference = Self::token("decoded-final-audio", &decoded_digest);
        self.locators.insert(
            decoded_reference.clone(),
            Locator {
                root: decoded_root.into(),
                path: decoded_path.into(),
                sha256: decoded_digest.clone(),
            },
        );
        let decoded_audio = MediaArtifact {
            reference: decoded_reference,
            owner: self.plan.body.spec.owner.clone(),
            sha256: decoded_digest.clone(),
            bytes: decoded_bytes,
            media_type: "audio/wav".into(),
            source_plan: self.plan.digest.clone(),
            source_state: call.expected_base.clone(),
            metadata: MediaMetadata {
                duration: decoded_duration,
                encoded_duration: Some(decoded_duration),
                video: None,
                audio: Some(AudioMetadata {
                    sample_rate: profile.sample_rate,
                    channels: profile.channels,
                    channel_layout: "stereo".into(),
                    sample_frames: decoded_frames,
                    priming_samples: None,
                    padding_samples: None,
                    latency_samples: None,
                    tail_samples: None,
                }),
            },
            dependencies: BTreeMap::from([("encoded-master".into(), digest)]),
            provenance: Some("mlt-post-encode-audio-decode-v1".into()),
            license: None,
            retention: Retention::PrivateCandidate,
        };
        let decoded_audio_handoff = ArtifactHandoffHint {
            version: 1,
            artifact_digest: decoded_digest,
            relative_path: decoded_path.into(),
        };
        decoded_audio_handoff.validate()?;
        Ok(NativeResult::Encoded {
            artifact: encoded_artifact,
            decoded_audio,
            decoded_audio_handoff,
        })
    }

    async fn verify_final_audio(
        &self,
        call: &StageCall,
        executor: &dyn Executor,
        cancellation: CancellationToken,
    ) -> Result<NativeResult> {
        Self::commands(call, &["driver.audio-analysis.artifact.measure"])?;
        let StagePayload::VerifyFinalAudio {
            artifact,
            handoff,
            required_rules,
        } = &call.payload
        else {
            return Err(Error::Invalid("VerifyFinalAudio payload mismatch".into()));
        };
        artifact.validate()?;
        handoff.validate()?;
        ensure(
            handoff.artifact_digest == artifact.sha256,
            "final audio handoff digest mismatch",
        )?;
        let locator = self.locator(&artifact.reference, &artifact.sha256)?;
        ensure(
            locator.path == handoff.relative_path,
            "final audio handoff path differs from the pinned decoded artifact",
        )?;
        ensure(
            !handoff.relative_path.contains('/'),
            "audio-analysis consumes a single file in its fixed analysis-input root",
        )?;
        let audio =
            artifact.metadata.audio.as_ref().ok_or_else(|| {
                Error::Invalid("final decoded artifact has no audio metadata".into())
            })?;
        ensure(
            artifact.metadata.video.is_none()
                && audio.sample_rate == self.plan.body.spec.delivery.sample_rate
                && audio.channels == self.plan.body.spec.delivery.channels,
            "final decoded artifact stream profile differs from AV delivery",
        )?;

        let mut runner = StageCommandRunner::new(executor, call)?;
        let value = runner
            .next(
                json!({
                    "file_name": handoff.relative_path,
                    "expected_sha256": artifact.sha256.as_str(),
                    "layout": if audio.channels == 1 { "mono" } else { "stereo" }
                }),
                cancellation,
            )
            .await?;
        runner.finish()?;

        let measured_artifact = value
            .get("artifact")
            .and_then(Value::as_object)
            .ok_or_else(|| Error::Invalid("audio-analysis omitted artifact receipt".into()))?;
        ensure(
            measured_artifact.get("sha256").and_then(Value::as_str)
                == Some(artifact.sha256.as_str())
                && measured_artifact.get("bytes").and_then(Value::as_u64) == Some(artifact.bytes)
                && value.get("exhaustive").and_then(Value::as_bool) == Some(true),
            "audio-analysis measured another artifact or returned non-exhaustive evidence",
        )?;

        let pcm = value
            .get("pcm_statistics")
            .and_then(Value::as_object)
            .ok_or_else(|| Error::Unknown("final WAV PCM statistics are unavailable".into()))?;
        let actual_frames = pcm
            .get("frames")
            .and_then(Value::as_u64)
            .ok_or_else(|| Error::Invalid("final WAV PCM frame count missing".into()))?;
        let measured_rate = pcm
            .get("sample_rate")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| Error::Invalid("final WAV PCM sample rate missing".into()))?;
        let measured_channels = pcm
            .get("channels")
            .and_then(Value::as_array)
            .map(Vec::len)
            .and_then(|value| u16::try_from(value).ok())
            .ok_or_else(|| Error::Invalid("final WAV PCM channel statistics missing".into()))?;
        let nonfinite = pcm
            .get("nonfinite_samples")
            .and_then(Value::as_u64)
            .ok_or_else(|| Error::Invalid("final WAV nonfinite count missing".into()))?;
        let out_of_range = pcm
            .get("out_of_range_samples")
            .and_then(Value::as_u64)
            .ok_or_else(|| Error::Invalid("final WAV out-of-range count missing".into()))?;
        let entirely_silent = pcm
            .get("entirely_silent")
            .and_then(Value::as_bool)
            .ok_or_else(|| Error::Invalid("final WAV silence status missing".into()))?;
        let peak = pcm.get("peak_millidbfs").and_then(Value::as_i64);

        ensure(
            measured_rate == audio.sample_rate
                && measured_channels == audio.channels
                && actual_frames == audio.sample_frames,
            "audio-analysis disagrees with MLT decoded-WAV probe metadata",
        )?;
        if let Some(loudness) = value.get("loudness").and_then(Value::as_object) {
            ensure(
                loudness.get("method").and_then(Value::as_str) == Some("libebur128")
                    && loudness.get("version").and_then(Value::as_str) == Some("1.2.6")
                    && loudness.get("frames").and_then(Value::as_u64) == Some(actual_frames)
                    && loudness.get("sample_rate").and_then(Value::as_u64)
                        == Some(u64::from(measured_rate))
                    && loudness.get("channels").and_then(Value::as_u64)
                        == Some(u64::from(measured_channels)),
                "libebur128 receipt disagrees with the independent decoded PCM receipt",
            )?;
        }

        let expected = self
            .plan
            .body
            .spec
            .delivery
            .duration
            .mul_i64(i64::from(measured_rate))?;
        ensure(
            expected.den == 1 && expected.num > 0,
            "AV delivery duration is not an exact final-audio sample boundary",
        )?;
        let expected_frames = u64::try_from(expected.num)
            .map_err(|_| Error::Limit("expected final-audio sample count overflow".into()))?;
        // AAC-LC encoders operate in 1024-sample frames. Two frames bound encoder
        // delay/padding without observing this candidate first; this tolerance is
        // fixed by codec semantics, not tuned from the output.
        const AAC_PADDING_TOLERANCE_SAMPLES: u64 = 2_048;
        let duration_pass =
            actual_frames.abs_diff(expected_frames) <= AAC_PADDING_TOLERANCE_SAMPLES;
        let peak_pass = nonfinite == 0
            && out_of_range == 0
            && !entirely_silent
            && peak.is_some_and(|value| value <= 0);

        let observation = c::ObservationRef {
            id: format!("final-audio-{}", &artifact.sha256.as_str()[..16]),
            base: artifact.source_state.clone(),
            source: EvidenceSource::DecodedMedia,
            method: "libebur128+independent-wav-pcm".into(),
            method_version: 1,
            scope: vec![],
            artifact: Some(artifact.sha256.clone()),
            exhaustive: true,
        };
        let mut checks = Vec::with_capacity(required_rules.len());
        for rule in required_rules {
            let (verdict, reason) = match rule.as_str() {
                "decoded-audio-duration" => (
                    if duration_pass {
                        c::Verdict::Pass
                    } else {
                        c::Verdict::Fail
                    },
                    format!(
                        "decoded WAV frames={actual_frames}, planned={expected_frames}, fixed AAC padding tolerance={AAC_PADDING_TOLERANCE_SAMPLES}"
                    ),
                ),
                "decoded-audio-peak" => (
                    if peak_pass {
                        c::Verdict::Pass
                    } else {
                        c::Verdict::Fail
                    },
                    format!(
                        "decoded WAV peak={peak:?} millidBFS, nonfinite={nonfinite}, out_of_range={out_of_range}, silent={entirely_silent}"
                    ),
                ),
                _ => (
                    c::Verdict::Unknown,
                    "final-audio rule has no evidence-backed verifier in this adapter".into(),
                ),
            };
            checks.push(c::RuleResult {
                rule: rule.clone(),
                version: 1,
                verdict,
                evidence_class: c::EvidenceClass::Deterministic,
                evidence: vec![observation.clone()],
                reason: Some(reason),
            });
        }
        let report = VerificationReport {
            execution_status: c::ExecutionStatus::Completed,
            validation: c::ValidationReport {
                plan_digest: self.plan.digest.clone(),
                base: artifact.source_state.clone(),
                required_rules: required_rules.clone(),
                checks,
            },
            support_level: c::SupportLevel::Composed,
            effects_observed: vec![],
            effects_unobservable: vec![],
        };
        Ok(NativeResult::Verified {
            artifact_digest: artifact.sha256.clone(),
            report,
        })
    }

    async fn verify_sync(
        &self,
        call: &StageCall,
        executor: &dyn Executor,
        cancellation: CancellationToken,
    ) -> Result<NativeResult> {
        Self::commands(call, &["driver.mlt-video.sync.probe"])?;
        let StagePayload::VerifySync { artifact, spec } = &call.payload else {
            return Err(Error::Invalid("VerifySync payload mismatch".into()));
        };
        let locator = self.locator(&artifact.reference, &artifact.sha256)?.clone();
        ensure(
            spec.cues.len() <= 16,
            "native MLT sync probe supports at most 16 cues",
        )?;
        let micros = |time: Rational| -> Result<u64> {
            let value = time.mul_i64(1_000_000)?.round(Round::NearestAway)?;
            u64::try_from(value).map_err(|_| Error::Limit("sync microsecond conversion".into()))
        };
        let tolerance_us = [
            micros(spec.max_offset)?,
            micros(spec.max_drift)?,
            micros(spec.max_cue_error)?,
        ]
        .into_iter()
        .max()
        .unwrap_or(10_000);
        let window_us = tolerance_us.saturating_mul(4).clamp(10_000, 500_000);
        let cues = spec
            .cues
            .iter()
            .map(|cue| Ok(json!({"id":cue.id,"expected_us":micros(cue.expected_time)?})))
            .collect::<Result<Vec<_>>>()?;
        let mut runner = StageCommandRunner::new(executor, call)?;
        let value = runner
            .next(
                json!({
                    "root":locator.root,
                    "path":locator.path,
                    "expected_sha256":artifact.sha256.as_str(),
                    "window_us":window_us,
                    "full_scan":spec.require_full_scan,
                    "cues":cues
                }),
                cancellation,
            )
            .await?;
        runner.finish()?;
        ensure(
            Self::digest_field(&value, "artifact_sha256")? == artifact.sha256,
            "sync probe artifact digest mismatch",
        )?;
        let exhaustive_video = value
            .get("exhaustive_video")
            .and_then(Value::as_bool)
            .ok_or_else(|| Error::Invalid("sync exhaustive_video missing".into()))?;
        let exhaustive_audio = value
            .get("exhaustive_audio")
            .and_then(Value::as_bool)
            .ok_or_else(|| Error::Invalid("sync exhaustive_audio missing".into()))?;
        let expected_coverage = if exhaustive_video && exhaustive_audio {
            "full_scan"
        } else {
            "cue_windows"
        };
        ensure(
            value.get("coverage").and_then(Value::as_str) == Some(expected_coverage),
            "sync coverage label/exhaustiveness mismatch",
        )?;
        if spec.require_full_scan {
            ensure(
                exhaustive_video && exhaustive_audio,
                "sync specification requires a full decoded master scan",
            )?;
        }
        let decode = |rows: Option<&Vec<Value>>| -> Result<Vec<crate::Detection>> {
            rows.ok_or_else(|| Error::Invalid("sync probe detection list missing".into()))?
                .iter()
                .map(|row| {
                    let cue_id = row
                        .get("cue_id")
                        .and_then(Value::as_str)
                        .ok_or_else(|| Error::Invalid("sync cue id missing".into()))?
                        .to_owned();
                    let time = row
                        .get("presentation_time_us")
                        .and_then(Value::as_u64)
                        .ok_or_else(|| Error::Invalid("sync presentation time missing".into()))?;
                    let uncertainty = row
                        .get("uncertainty_us")
                        .and_then(Value::as_u64)
                        .ok_or_else(|| Error::Invalid("sync uncertainty missing".into()))?;
                    let confidence = u16::try_from(
                        row.get("confidence")
                            .and_then(Value::as_u64)
                            .ok_or_else(|| Error::Invalid("sync confidence missing".into()))?,
                    )
                    .map_err(|_| Error::Limit("sync confidence overflow".into()))?;
                    Ok(crate::Detection {
                        cue_id,
                        presentation_time: Rational::new(
                            i64::try_from(time)
                                .map_err(|_| Error::Limit("sync timestamp overflow".into()))?,
                            1_000_000,
                        )?,
                        uncertainty: Rational::new(
                            i64::try_from(uncertainty)
                                .map_err(|_| Error::Limit("sync uncertainty overflow".into()))?,
                            1_000_000,
                        )?,
                        confidence,
                    })
                })
                .collect()
        };
        let decoder_method = value
            .get("decoder")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Invalid("sync decoder method missing".into()))?
            .to_owned();
        let decoder_digest = Self::digest_field(&value, "decoder_sha256")?;
        Ok(NativeResult::SyncMeasured {
            probe: crate::DecodedSyncProbe {
                version: 1,
                artifact_digest: artifact.sha256.clone(),
                decoder_method,
                decoder_digest,
                source: EvidenceSource::DecodedMedia,
                exhaustive_video,
                exhaustive_audio,
                flashes: decode(value.get("flashes").and_then(Value::as_array))?,
                impulses: decode(value.get("impulses").and_then(Value::as_array))?,
            },
        })
    }
}

#[cfg(test)]
mod wire_default_tests {
    use super::*;

    #[test]
    fn omitted_motion_fps_denominator_uses_documented_wire_default_only() {
        let omitted = json!({"fps": 30});
        assert_eq!(
            AgentAStageAdapter::optional_u64_default(&omitted, "fps_denominator", 1).unwrap(),
            1
        );

        let explicit = json!({"fps_denominator": 1001});
        assert_eq!(
            AgentAStageAdapter::optional_u64_default(&explicit, "fps_denominator", 1).unwrap(),
            1001
        );

        for malformed in [
            json!({"fps_denominator": null}),
            json!({"fps_denominator": "1"}),
            json!({"fps_denominator": -1}),
            json!({"fps_denominator": 1.5}),
        ] {
            assert!(
                AgentAStageAdapter::optional_u64_default(&malformed, "fps_denominator", 1).is_err()
            );
        }
    }
}
