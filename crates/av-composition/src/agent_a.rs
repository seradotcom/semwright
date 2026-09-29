//! Concrete Agent-A stage realization through descriptor-pinned Broker commands.
//! Audio authoring/final-audio analysis remain delegated to Agent B's public provider.
use crate::{
    AvPlan, DeliveryCodec, DeliveryInput, Error, NativeResult, Result, Stage, StageCall,
    StageCommandRunner, StagePayload, TransferKind,
};
use semwright_media_time::{
    AudioMetadata, MediaArtifact, MediaMetadata, Rational, Retention, Round, VideoMetadata,
};
use semwright_recipes::Executor;
use semwright_semantic_composition::{
    Digest, EvidenceSource, VerificationReport, canonical_digest, ensure,
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
        Stage::Mux => &["driver.mlt-video.av.mux"],
        Stage::VerifySync => &["driver.mlt-video.sync.probe"],
        _ => return None,
    })
}

pub struct AgentAStageAdapter {
    plan: AvPlan,
    motion_fingerprint: Option<String>,
    motion_job_ref: Option<String>,
    locators: BTreeMap<String, Locator>,
}

impl AgentAStageAdapter {
    pub fn new(plan: AvPlan) -> Result<Self> {
        plan.validate()?;
        Ok(Self {
            plan,
            motion_fingerprint: None,
            motion_job_ref: None,
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

    /// Bind a locator returned by an already-authorized transfer provider.
    /// This does not grant access: subsequent use still goes through Broker policy
    /// and the destination driver's scoped-root + expected-SHA checks.
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
            Stage::Mux => self.mux(call, executor, cancellation).await,
            Stage::VerifySync => self.verify_sync(call, executor, cancellation).await,
            Stage::ApplyAudio
            | Stage::RenderAudio
            | Stage::VerifyAudio
            | Stage::TransferAudio
            | Stage::VerifyFinalAudio => Err(Error::Unknown(
                "audio stage requires Agent B's verified public provider handoff".into(),
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
        ensure(
            planned.get("width").and_then(Value::as_u64) == Some(u64::from(delivery.width))
                && planned.get("height").and_then(Value::as_u64)
                    == Some(u64::from(delivery.height))
                && planned.get("fps").and_then(Value::as_u64)
                    == Some(u64::from(delivery.frame_rate.num))
                && planned.get("fps_denominator").and_then(Value::as_u64)
                    == Some(u64::from(delivery.frame_rate.den))
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
        ensure(
            rendered.get("state").and_then(Value::as_str) == Some("succeeded"),
            "Motion native render did not succeed",
        )?;
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
            value.get("frame_count").and_then(Value::as_u64) == Some(video.frames)
                && value.get("width").and_then(Value::as_u64) == Some(u64::from(video.width))
                && value.get("height").and_then(Value::as_u64) == Some(u64::from(video.height))
                && value.get("fps_num").and_then(Value::as_u64)
                    == Some(u64::from(video.frame_rate.num))
                && value.get("fps_den").and_then(Value::as_u64)
                    == Some(u64::from(video.frame_rate.den)),
            "FFV1 mezzanine receipt differs from Motion frame profile",
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
        Ok(NativeResult::Encoded {
            artifact: MediaArtifact {
                reference,
                owner: self.plan.body.spec.owner.clone(),
                sha256: digest,
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
            },
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
        ensure(
            value.get("coverage").and_then(Value::as_str) == Some("cue_windows"),
            "unexpected sync probe coverage",
        )?;
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
        let missing_video = value
            .get("missing_video")
            .and_then(Value::as_array)
            .ok_or_else(|| Error::Invalid("sync missing_video absent".into()))?;
        let missing_audio = value
            .get("missing_audio")
            .and_then(Value::as_array)
            .ok_or_else(|| Error::Invalid("sync missing_audio absent".into()))?;
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
                // The provider exhaustively scans each originally declared cue
                // window; it does not claim full-frame inspection outside scope.
                exhaustive_video: missing_video.len() <= spec.cues.len(),
                exhaustive_audio: missing_audio.len() <= spec.cues.len(),
                flashes: decode(value.get("flashes").and_then(Value::as_array))?,
                impulses: decode(value.get("impulses").and_then(Value::as_array))?,
            },
        })
    }
}
