//! Final manifests are generated from verified receipts, then written via Broker.
//! Atomic pointer publication does not make prior native mutations atomic.
use crate::*;
use schemars::JsonSchema;
use semwright_media_time::MediaArtifact;
use semwright_recipes::Executor;
use semwright_semantic_composition::{
    Digest, Owner, Verdict, VerificationReport, bounded_id, canonical_bytes, canonical_digest,
    ensure,
};
use semwright_types::{ErrorCode, ExecuteRequest};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PublicationManifest {
    pub version: u32,
    pub av_plan_digest: Digest,
    pub owner: Owner,
    pub motion_plan_digest: Digest,
    pub audio_plan_digest: Digest,
    pub cue_digest: Digest,
    pub delivery: DeliveryProfile,
    pub final_artifact: MediaArtifact,
    pub motion_verification: VerificationReport,
    pub audio_verification: VerificationReport,
    pub final_audio_verification: VerificationReport,
    pub sync: SyncReport,
    pub ready_for: Vec<String>,
    pub r16_closed: bool,
    pub promotional_video: bool,
}
impl PublicationManifest {
    pub fn validate(&self) -> Result<()> {
        ensure(
            self.version == 1 && !self.r16_closed && !self.promotional_video,
            "publication contract/release boundary",
        )?;
        self.owner.validate()?;
        self.delivery.validate()?;
        self.final_artifact.validate()?;
        ensure(
            self.final_artifact.owner == self.owner
                && self.final_artifact.source_plan == self.av_plan_digest,
            "final artifact publication provenance mismatch",
        )?;
        ensure(
            self.ready_for == ["verified-rendered-av-v1"],
            "publication readiness scope is fixed",
        )?;
        for report in [
            &self.motion_verification,
            &self.audio_verification,
            &self.final_audio_verification,
        ] {
            ensure(
                report.verdict()? == Verdict::Pass,
                "publication cannot hide incomplete verification",
            )?;
        }
        ensure(
            self.motion_verification.validation.plan_digest == self.motion_plan_digest
                && self.audio_verification.validation.plan_digest == self.audio_plan_digest
                && self.final_audio_verification.validation.plan_digest == self.av_plan_digest,
            "publication references mixed plan revisions",
        )?;
        ensure(
            self.sync.verdict == Verdict::Pass
                && self.sync.artifact_digest == self.final_artifact.sha256,
            "publication sync does not describe the final encoded artifact",
        )?;
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PublicationCandidate {
    pub owner: Owner,
    pub manifest_digest: Digest,
    pub source_root: String,
    pub source_path: String,
    pub destination_root: String,
    pub destination_path: String,
    pub bytes: u64,
}
fn relative(path: &str) -> Result<()> {
    ensure(
        !path.is_empty()
            && path.len() <= 1024
            && !path.contains('\\')
            && !path.contains(':')
            && !path.starts_with('/')
            && !path.chars().any(char::is_control)
            && path
                .split('/')
                .all(|p| !p.is_empty() && p != "." && p != ".."),
        "publication path must be grant-relative",
    )
}
impl PublicationCandidate {
    pub fn validate(&self) -> Result<()> {
        self.owner.validate()?;
        bounded_id(&self.source_root)?;
        bounded_id(&self.destination_root)?;
        relative(&self.source_path)?;
        relative(&self.destination_path)?;
        ensure(
            (self.source_root != self.destination_root
                || self.source_path != self.destination_path)
                && self.bytes > 0
                && self.bytes <= 524288,
            "publication candidate bounds or self-overwrite",
        )
    }
}
#[derive(Debug, Clone)]
pub struct PublicationTargets {
    pub candidate_root: String,
    pub output_root: String,
    pub pointer_path: String,
    pub allow_pointer_replacement: bool,
}
/// Construct this adapter only with the actual session-bound Broker executor.
/// Plans/Skills/results never supply this executor or expand its grants.
pub struct BrokerPublisher<'a> {
    executor: &'a dyn Executor,
    owner: Owner,
    targets: PublicationTargets,
    descriptors: std::collections::BTreeMap<String, Digest>,
}
impl<'a> BrokerPublisher<'a> {
    pub fn new(
        executor: &'a dyn Executor,
        owner: Owner,
        targets: PublicationTargets,
    ) -> Result<Self> {
        owner.validate()?;
        bounded_id(&targets.candidate_root)?;
        bounded_id(&targets.output_root)?;
        relative(&targets.pointer_path)?;
        let mut descriptors = std::collections::BTreeMap::new();
        for name in ["filesystem.read", "filesystem.write", "artifact.handoff"] {
            let descriptor = executor
                .describe(name)
                .map_err(|e| Error::Denied(e.to_string()))?;
            let digest = semwright_driver_sdk::descriptor_digest(&descriptor)
                .map_err(|e| Error::Invalid(e.to_string()))?;
            descriptors.insert(name.into(), Digest::parse(digest)?);
        }
        Ok(Self {
            executor,
            owner,
            targets,
            descriptors,
        })
    }
    async fn call(
        &self,
        command: &str,
        args: Value,
        cancel: CancellationToken,
    ) -> semwright_types::Result<Value> {
        let descriptor = self.executor.describe(command)?;
        let actual = semwright_driver_sdk::descriptor_digest(&descriptor)?;
        if self.descriptors.get(command).map(Digest::as_str) != Some(actual.as_str()) {
            return Err(semwright_types::Error::new(
                ErrorCode::StaleReference,
                "publication capability descriptor changed",
            ));
        }
        self.executor
            .execute(
                ExecuteRequest {
                    command: command.into(),
                    args,
                    dry_run: false,
                    backend: None,
                },
                cancel,
            )
            .await
    }
    async fn read(
        &self,
        root: &str,
        path: &str,
        cancel: CancellationToken,
    ) -> semwright_types::Result<String> {
        let value = self
            .call(
                "filesystem.read",
                json!({"root":root,"path":path,"max_bytes":524288}),
                cancel,
            )
            .await?;
        value
            .get("text")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| {
                semwright_types::Error::new(
                    ErrorCode::ProtocolMismatch,
                    "filesystem read did not return text content",
                )
            })
    }
    pub async fn prepare(
        &self,
        coordinator: &AvCoordinator,
        cancel: CancellationToken,
    ) -> Result<PublicationCandidate> {
        ensure(
            coordinator.next_stage() == Some(Stage::PreparePublication),
            "publication preparation is out of sequence",
        )?;
        let manifest = coordinator.manifest()?;
        ensure(
            manifest.owner == self.owner,
            "publisher is bound to another owner",
        )?;
        let bytes = canonical_bytes(&manifest)?;
        let digest = canonical_digest(&manifest)?;
        let path = format!("av-candidate-{}.json", digest.as_str());
        let content = String::from_utf8(bytes.clone())
            .map_err(|_| Error::Invalid("manifest UTF-8".into()))?;
        match self
            .read(&self.targets.candidate_root, &path, cancel.clone())
            .await
        {
            Ok(existing) => ensure(
                Digest::of_bytes(existing.as_bytes()) == digest,
                "existing candidate has different content",
            )?,
            Err(error) if error.code == ErrorCode::NotFound => {
                self.call(
                    "filesystem.write",
                    json!({"root":self.targets.candidate_root,"path":path,"text":content}),
                    cancel.clone(),
                )
                .await
                .map_err(|e| Error::Denied(e.to_string()))?;
            }
            Err(error) => return Err(Error::Denied(error.to_string())),
        }
        let observed = self
            .read(&self.targets.candidate_root, &path, cancel)
            .await
            .map_err(|e| Error::Unknown(e.to_string()))?;
        ensure(
            Digest::of_bytes(observed.as_bytes()) == digest,
            "candidate readback digest mismatch",
        )?;
        let candidate = PublicationCandidate {
            owner: self.owner.clone(),
            manifest_digest: digest,
            source_root: self.targets.candidate_root.clone(),
            source_path: path,
            destination_root: self.targets.output_root.clone(),
            destination_path: self.targets.pointer_path.clone(),
            bytes: bytes.len() as u64,
        };
        candidate.validate()?;
        Ok(candidate)
    }
    pub async fn publish(
        &self,
        coordinator: &AvCoordinator,
        candidate: &PublicationCandidate,
        cancel: CancellationToken,
    ) -> Result<NativeResult> {
        ensure(
            coordinator.next_stage() == Some(Stage::Publish),
            "publication is out of sequence",
        )?;
        candidate.validate()?;
        ensure(
            candidate.owner == self.owner
                && candidate.source_root == self.targets.candidate_root
                && candidate.destination_root == self.targets.output_root
                && candidate.destination_path == self.targets.pointer_path,
            "publication target/owner substitution",
        )?;
        ensure(
            candidate.manifest_digest == canonical_digest(&coordinator.manifest()?)?,
            "publication candidate no longer matches the verified coordinator",
        )?;
        let current = self
            .read(
                &candidate.source_root,
                &candidate.source_path,
                cancel.clone(),
            )
            .await
            .map_err(|e| Error::Unknown(e.to_string()))?;
        ensure(
            Digest::of_bytes(current.as_bytes()) == candidate.manifest_digest,
            "private candidate changed before publication",
        )?;
        if !self.targets.allow_pointer_replacement {
            match self
                .read(
                    &candidate.destination_root,
                    &candidate.destination_path,
                    cancel.clone(),
                )
                .await
            {
                Ok(_) => {
                    return Err(Error::Denied(
                        "replacing an existing AV pointer requires explicit owner configuration"
                            .into(),
                    ));
                }
                Err(error) if error.code == ErrorCode::NotFound => {}
                Err(error) => return Err(Error::Denied(error.to_string())),
            }
        }
        // Scoped-root atomic copy is the existing authority path. The read-before-
        // copy check is best effort, not a new filesystem compare-and-swap claim.
        let result=self.call("artifact.handoff",json!({"source_root":candidate.source_root,"source_path":candidate.source_path,"destination_root":candidate.destination_root,"destination_path":candidate.destination_path,"max_bytes":candidate.bytes,"expected_sha256":candidate.manifest_digest.as_str(),"semantic_type":"application/vnd.semwright.av-publication-manifest+json","media_type":"application/json"}),cancel.clone()).await.map_err(|e|Error::Unknown(e.to_string()))?;
        ensure(
            result.get("copied") == Some(&json!(true))
                && result.get("atomic") == Some(&json!(true))
                && result.get("sha256").and_then(Value::as_str)
                    == Some(candidate.manifest_digest.as_str())
                && result.get("bytes").and_then(Value::as_u64) == Some(candidate.bytes),
            "publication did not return the expected native copy receipt",
        )?;
        let observed = self
            .read(
                &candidate.destination_root,
                &candidate.destination_path,
                cancel,
            )
            .await
            .map_err(|e| Error::Unknown(e.to_string()))?;
        ensure(
            Digest::of_bytes(observed.as_bytes()) == candidate.manifest_digest,
            "published manifest readback mismatch",
        )?;
        Ok(NativeResult::Published {
            manifest_digest: candidate.manifest_digest.clone(),
            pointer: candidate.destination_path.clone(),
        })
    }
}
