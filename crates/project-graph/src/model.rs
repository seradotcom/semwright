use crate::composition::{
    BaseStateSet, Digest, EvidenceSource, ExecutionStatus, ObservationRef, Owner, ResourceKey,
    VerificationReport, canonical_digest,
};
use crate::{
    AssetRevision, DerivationId, ExternalIntentId, LogicalAssetId, MAX_DEPENDENCIES, ProjectId,
    ReceiptId, Result, SCHEMA_VERSION, ensure, name,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
/// A locator is a re-resolution hint, never a reusable session ref or a grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DurableLocator {
    ScopedFile {
        root: String,
        relative_path: String,
    },
    Native {
        resource: ResourceKey,
        stable_id: String,
        resolver: String,
        resolver_version: u32,
    },
}
impl DurableLocator {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::ScopedFile {
                root,
                relative_path,
            } => {
                name(root)?;
                ensure(
                    root.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')),
                    "root namespace",
                )?;
                ensure(
                    !relative_path.is_empty() && relative_path.len() <= 4096,
                    "locator length",
                )?;
                for part in relative_path.split('/') {
                    ensure(
                        !part.is_empty()
                            && part != "."
                            && part != ".."
                            && !part.ends_with(['.', ' ']),
                        "relative locator component",
                    )?;
                    ensure(
                        !part.chars().any(|c| {
                            c.is_control()
                                || matches!(c, '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
                        }),
                        "portable locator character",
                    )?;
                    let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
                    ensure(
                        !matches!(
                            stem.as_str(),
                            "CON"
                                | "PRN"
                                | "AUX"
                                | "NUL"
                                | "COM1"
                                | "COM2"
                                | "COM3"
                                | "COM4"
                                | "COM5"
                                | "COM6"
                                | "COM7"
                                | "COM8"
                                | "COM9"
                                | "LPT1"
                                | "LPT2"
                                | "LPT3"
                                | "LPT4"
                                | "LPT5"
                                | "LPT6"
                                | "LPT7"
                                | "LPT8"
                                | "LPT9"
                        ),
                        "device locator",
                    )?;
                }
                Ok(())
            }
            Self::Native {
                resource,
                stable_id,
                resolver,
                resolver_version,
            } => {
                for v in [&resource.provider, &resource.resource, stable_id, resolver] {
                    name(v)?;
                }
                ensure(*resolver_version > 0, "native resolver version")
            }
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectionDigest {
    pub digest: Digest,
    pub method: String,
    pub method_version: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Fingerprint {
    pub bytes: Option<Digest>,
    pub projection: Option<ProjectionDigest>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Equivalence {
    ExactBytes,
    Projection,
    BytesAndProjection,
}
impl Fingerprint {
    pub fn validate(&self) -> Result<()> {
        if let Some(p) = &self.projection {
            name(&p.method)?;
            ensure(p.method_version > 0, "projection method version")?;
        }
        Ok(())
    }
    /// None is unobserved or incomparable, not equality and not a proved change.
    pub fn equivalent(&self, other: &Self, policy: Equivalence) -> Option<bool> {
        let bytes = self
            .bytes
            .as_ref()
            .zip(other.bytes.as_ref())
            .map(|(a, b)| a == b);
        let projection = self
            .projection
            .as_ref()
            .zip(other.projection.as_ref())
            .and_then(|(a, b)| {
                (a.method == b.method && a.method_version == b.method_version)
                    .then_some(a.digest == b.digest)
            });
        match policy {
            Equivalence::ExactBytes => bytes,
            Equivalence::Projection => projection,
            Equivalence::BytesAndProjection => match (bytes, projection) {
                (Some(false), _) | (_, Some(false)) => Some(false),
                (Some(true), Some(true)) => Some(true),
                _ => None,
            },
        }
    }
}
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum DependencyClass {
    Bytes,
    Projection,
    Parameters,
    Runtime,
    Descriptor,
    Recipe,
    ImportSettings,
    Font,
    Texture,
    Plugin,
    Contract,
    External,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Determinant {
    pub class: DependencyClass,
    pub key: String,
    pub digest: Digest,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Coverage {
    pub complete: bool,
    pub unknown_frontier: BTreeSet<DependencyClass>,
}
impl Coverage {
    pub fn complete() -> Self {
        Self {
            complete: true,
            unknown_frontier: BTreeSet::new(),
        }
    }
    pub fn unknown() -> Self {
        Self {
            complete: false,
            unknown_frontier: [DependencyClass::External].into(),
        }
    }
    pub fn validate(&self) -> Result<()> {
        ensure(
            !self.complete || self.unknown_frontier.is_empty(),
            "complete coverage cannot have unknown dependencies",
        )
    }
    pub fn cache_safe(&self) -> bool {
        self.complete && self.unknown_frontier.is_empty()
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    pub id: LogicalAssetId,
    pub resource_type: String,
    pub label: String,
    pub locator: Option<DurableLocator>,
}
impl Asset {
    pub fn validate(&self) -> Result<()> {
        name(&self.resource_type)?;
        name(&self.label)?;
        if let Some(locator) = &self.locator {
            locator.validate()?;
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RevisionPin {
    pub asset: LogicalAssetId,
    pub revision: AssetRevision,
    pub fingerprint: Fingerprint,
    pub equivalence: Equivalence,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RevisionRecord {
    pub pin: RevisionPin,
    pub observed_unix_ms: u64,
    pub binding_generation: u64,
    pub observation: ObservationRef,
    pub coverage: Coverage,
}
impl RevisionRecord {
    pub fn validate(&self) -> Result<()> {
        self.pin.fingerprint.validate()?;
        self.coverage.validate()?;
        self.observation.base.validate()?;
        name(&self.observation.id)?;
        name(&self.observation.method)?;
        ensure(
            self.observed_unix_ms > 0
                && self.binding_generation > 0
                && self.observation.method_version > 0,
            "revision observation version/time",
        )
    }
}

/// Untrusted native/readback observation candidate. It contains no durable
/// revision identity and certifies no production activity.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RevisionCandidate {
    pub asset: LogicalAssetId,
    pub fingerprint: Fingerprint,
    pub equivalence: Equivalence,
    pub observed_unix_ms: u64,
    pub binding_generation: u64,
    pub observation: ObservationRef,
    pub coverage: Coverage,
}
impl RevisionCandidate {
    pub fn validate(&self) -> Result<()> {
        self.fingerprint.validate()?;
        self.coverage.validate()?;
        self.observation.base.validate()?;
        name(&self.observation.id)?;
        name(&self.observation.method)?;
        ensure(
            self.observed_unix_ms > 0
                && self.binding_generation > 0
                && self.observation.method_version > 0
                && !self.observation.scope.is_empty()
                && self.observation.scope.len() <= 4096,
            "revision candidate observation bounds",
        )?;
        ensure(
            self.observation.exhaustive || !self.coverage.complete,
            "non-exhaustive observation cannot claim complete coverage",
        )?;
        for address in &self.observation.scope {
            name(&address.logical_id)?;
            name(&address.property)?;
            name(&address.resource.provider)?;
            name(&address.resource.resource)?;
            ensure(
                self.observation
                    .base
                    .0
                    .iter()
                    .any(|state| state.key == address.resource),
                "revision scope resource absent from observation base",
            )?;
        }
        canonical_digest(self)?;
        Ok(())
    }
}

/// Trusted-host promotion of a native/readback observation. No Deserialize
/// implementation exists, so caller JSON cannot acquire this authority.
#[derive(Debug, Clone)]
pub struct AdmittedRevision {
    pub(crate) project: ProjectId,
    pub(crate) owner: Owner,
    pub(crate) record: RevisionRecord,
}
impl AdmittedRevision {
    pub fn record(&self) -> &RevisionRecord {
        &self.record
    }
}

/// Registered observation origin/method binding. This adapter admits evidence;
/// it does not verify effects, create activities or authorize native execution.
pub struct RevisionAdapter {
    resource: ResourceKey,
    source: EvidenceSource,
    method: String,
    method_version: u32,
}
impl RevisionAdapter {
    pub fn registered(
        resource: ResourceKey,
        source: EvidenceSource,
        method: String,
        method_version: u32,
    ) -> Result<Self> {
        name(&resource.provider)?;
        name(&resource.resource)?;
        name(&method)?;
        ensure(method_version > 0, "revision adapter method version")?;
        Ok(Self {
            resource,
            source,
            method,
            method_version,
        })
    }

    pub fn admit(
        &self,
        authenticated: &Owner,
        project: &ProjectId,
        expected_asset: &LogicalAssetId,
        expected_generation: u64,
        candidate: RevisionCandidate,
    ) -> Result<AdmittedRevision> {
        candidate.validate()?;
        if &candidate.asset != expected_asset
            || candidate.binding_generation != expected_generation
            || candidate.observation.source != self.source
            || candidate.observation.method != self.method
            || candidate.observation.method_version != self.method_version
        {
            return Err(crate::GraphError::Denied);
        }
        let base = candidate
            .observation
            .base
            .0
            .iter()
            .find(|state| state.key == self.resource)
            .ok_or(crate::GraphError::Denied)?;
        if base.document_id != project.as_str()
            || !candidate
                .observation
                .scope
                .iter()
                .any(|address| address.resource == self.resource)
        {
            return Err(crate::GraphError::Denied);
        }
        Ok(AdmittedRevision {
            project: project.clone(),
            owner: authenticated.clone(),
            record: RevisionRecord {
                pin: RevisionPin {
                    asset: candidate.asset,
                    revision: AssetRevision::new(),
                    fingerprint: candidate.fingerprint,
                    equivalence: candidate.equivalence,
                },
                observed_unix_ms: candidate.observed_unix_ms,
                binding_generation: candidate.binding_generation,
                observation: candidate.observation,
                coverage: candidate.coverage,
            },
        })
    }
}
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Relation {
    Contains,
    References,
    DerivedFrom,
    ProducedBy,
    ConsumedBy,
    Realizes,
    PublishedAs,
    VerifiedBy,
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "id",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Vertex {
    Asset(LogicalAssetId),
    Revision(AssetRevision),
    Activity(DerivationId),
    Receipt(ReceiptId),
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EdgeEvidence {
    Declared { declaration: ReceiptId },
    Observed { observation: ObservationRef },
    Executed { receipt: ReceiptId },
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Edge {
    pub from: Vertex,
    pub to: Vertex,
    pub relation: Relation,
    pub evidence: EdgeEvidence,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OperationIdentity {
    pub capability: String,
    pub descriptor: Digest,
    pub runtime: Digest,
    pub plan: Digest,
    pub parameters: Digest,
    pub recipe: Option<Digest>,
}

/// Durable record of an external operation boundary. It is evidence/recovery
/// state only: storing one never authorizes or schedules the operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExternalIntent {
    pub version: u32,
    pub id: ExternalIntentId,
    pub project: ProjectId,
    /// Historical authenticated owner. The session component is evidence, not
    /// a restart credential.
    pub owner: Owner,
    pub request_id: String,
    pub operation: OperationIdentity,
    /// Resources that may be affected by the external operation. This scope is
    /// used for visibility/reconciliation, not as an authorization grant.
    pub affected: Vec<LogicalAssetId>,
    pub prepared_unix_ms: u64,
    /// The live observation epoch in which the operation was prepared.
    pub observation_epoch: String,
    pub status: ExecutionStatus,
    pub receipt: Option<ReceiptId>,
}
impl ExternalIntent {
    pub fn validate(&self) -> Result<()> {
        ensure(self.version == SCHEMA_VERSION, "external intent version")?;
        self.owner.validate()?;
        name(&self.request_id)?;
        name(&self.operation.capability)?;
        name(&self.observation_epoch)?;
        ensure(self.prepared_unix_ms > 0, "external intent time")?;
        ensure(
            !self.affected.is_empty() && self.affected.len() <= MAX_DEPENDENCIES,
            "external intent affected scope",
        )?;
        let mut seen = BTreeSet::new();
        ensure(
            self.affected.iter().all(|id| seen.insert(id)),
            "duplicate external intent asset",
        )?;
        ensure(
            !matches!(
                self.status,
                ExecutionStatus::Prepared | ExecutionStatus::Applying
            ) || self.receipt.is_none(),
            "in-flight external intent cannot already carry a receipt",
        )?;
        ensure(
            self.status != ExecutionStatus::Completed || self.receipt.is_some(),
            "completed external intent requires a persisted receipt",
        )?;
        canonical_digest(self)?;
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExecutionReceipt {
    pub version: u32,
    pub id: ReceiptId,
    pub derivation: DerivationId,
    pub project: ProjectId,
    /// Historical authenticated execution owner, NOT a restart credential.
    pub owner: Owner,
    pub request_id: String,
    pub operation: OperationIdentity,
    pub source_base: BaseStateSet,
    pub inputs: Vec<RevisionPin>,
    pub outputs: Vec<RevisionPin>,
    pub determinants: Vec<Determinant>,
    pub coverage: Coverage,
    pub verification: VerificationReport,
    pub completed_unix_ms: u64,
}
impl ExecutionReceipt {
    pub fn validate(&self) -> Result<()> {
        ensure(self.version == SCHEMA_VERSION, "receipt version")?;
        self.owner.validate()?;
        self.source_base.validate()?;
        self.coverage.validate()?;
        name(&self.request_id)?;
        name(&self.operation.capability)?;
        ensure(self.completed_unix_ms > 0, "receipt observation time")?;
        ensure(
            self.inputs.len() <= MAX_DEPENDENCIES
                && !self.outputs.is_empty()
                && self.outputs.len() <= MAX_DEPENDENCIES
                && self.determinants.len() <= MAX_DEPENDENCIES,
            "receipt dependency count",
        )?;
        for pins in [&self.inputs, &self.outputs] {
            let mut seen = BTreeSet::new();
            for pin in pins {
                pin.fingerprint.validate()?;
                ensure(seen.insert(&pin.asset), "duplicate receipt asset")?;
            }
        }
        let mut seen = BTreeSet::new();
        for d in &self.determinants {
            name(&d.key)?;
            ensure(seen.insert((d.class, &d.key)), "duplicate determinant")?;
        }
        ensure(
            self.verification.validation.plan_digest == self.operation.plan,
            "receipt/report plan substitution",
        )?;
        self.verification.verdict()?;
        canonical_digest(self)?;
        Ok(())
    }
}
/// Public wire data never carries acquired authority. Only trusted host code may
/// instantiate an adapter and call admit after a Broker/Driver Host result.
/// No public graph route accepts an ExecutionReceipt for promotion to this type.
#[derive(Debug, Clone)]
pub struct AdmittedReceipt(pub(crate) ExecutionReceipt);
impl AdmittedReceipt {
    pub fn record(&self) -> &ExecutionReceipt {
        &self.0
    }
}
pub struct ReceiptAdapter {
    capability: String,
    descriptor: Digest,
    runtime: Digest,
}
impl ReceiptAdapter {
    pub fn registered(capability: String, descriptor: Digest, runtime: Digest) -> Result<Self> {
        name(&capability)?;
        Ok(Self {
            capability,
            descriptor,
            runtime,
        })
    }
    pub fn admit(
        &self,
        authenticated: &Owner,
        request_id: &str,
        receipt: ExecutionReceipt,
    ) -> Result<AdmittedReceipt> {
        receipt.validate()?;
        if &receipt.owner != authenticated
            || receipt.request_id != request_id
            || receipt.operation.capability != self.capability
            || receipt.operation.descriptor != self.descriptor
            || receipt.operation.runtime != self.runtime
        {
            return Err(crate::GraphError::Denied);
        }
        Ok(AdmittedReceipt(receipt))
    }
}
impl ExecutionReceipt {
    /// Current runtime/descriptor/parameters must be observed for this activity;
    /// copying these pins from history is not a reconcile operation.
    pub fn required_determinants(&self) -> Vec<Determinant> {
        let key = self.derivation.as_str().to_owned();
        let mut values = vec![
            Determinant {
                class: DependencyClass::Runtime,
                key: key.clone(),
                digest: self.operation.runtime.clone(),
            },
            Determinant {
                class: DependencyClass::Descriptor,
                key: key.clone(),
                digest: self.operation.descriptor.clone(),
            },
            Determinant {
                class: DependencyClass::Parameters,
                key: key.clone(),
                digest: self.operation.parameters.clone(),
            },
        ];
        if let Some(recipe) = &self.operation.recipe {
            values.push(Determinant {
                class: DependencyClass::Recipe,
                key,
                digest: recipe.clone(),
            });
        }
        values.extend(self.determinants.iter().cloned());
        values
    }
}
