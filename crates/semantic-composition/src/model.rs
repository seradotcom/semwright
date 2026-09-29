use crate::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalBinding {
    HostSession,
    Named(String),
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Owner {
    pub session: String,
    pub principal: PrincipalBinding,
}
impl Owner {
    pub fn validate(&self) -> Result<()> {
        bounded_id(&self.session)?;
        if let PrincipalBinding::Named(p) = &self.principal {
            bounded_id(p)?;
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResourceKey {
    pub provider: String,
    pub resource: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Revision {
    Counter(u64),
    Fingerprint(Digest),
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Concurrency {
    CompareAndSwap,
    BestEffortRevalidate,
    Unobservable,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BaseState {
    pub key: ResourceKey,
    pub document_id: String,
    pub provider_session: String,
    pub generation: String,
    pub revision: Revision,
    pub concurrency: Concurrency,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct BaseStateSet(pub Vec<BaseState>);
impl BaseStateSet {
    pub fn validate(&self) -> Result<()> {
        ensure(!self.0.is_empty() && self.0.len() <= 64, "base set size")?;
        let mut seen = BTreeSet::new();
        for s in &self.0 {
            for v in [
                &s.key.provider,
                &s.key.resource,
                &s.document_id,
                &s.provider_session,
                &s.generation,
            ] {
                bounded_id(v)?;
            }
            ensure(seen.insert(&s.key), "duplicate resource state")?;
            ensure(
                s.concurrency != Concurrency::CompareAndSwap
                    || !matches!(s.revision, Revision::Unknown),
                "CAS needs an observable revision",
            )?;
        }
        Ok(())
    }
    pub fn check_fresh(&self, observed: &Self, require_cas: bool) -> Result<()> {
        self.validate()?;
        observed.validate()?;
        if self.0.len() != observed.0.len() {
            return Err(ContractError::Stale("base-set membership".into()));
        }
        for expected in &self.0 {
            if expected.concurrency == Concurrency::Unobservable
                || matches!(expected.revision, Revision::Unknown)
            {
                return Err(ContractError::Unknown(
                    "resource freshness cannot be established".into(),
                ));
            }
            if require_cas && expected.concurrency != Concurrency::CompareAndSwap {
                return Err(ContractError::Denied("profile requires native CAS".into()));
            }
            if observed.0.iter().find(|s| s.key == expected.key) != Some(expected) {
                return Err(ContractError::Stale(expected.key.resource.clone()));
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Address {
    pub resource: ResourceKey,
    pub logical_id: String,
    pub property: String,
}
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum EffectClass {
    Inspect,
    CreateOwnedObject,
    UpdateOwnedObject,
    RenderPrivateArtifact,
    PublishArtifact,
}
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Inspect,
    Plan,
    Apply,
    Measure,
    Validate,
    RepairPlan,
    RepairApply,
    Verify,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProfileIdentity {
    pub id: String,
    pub version: u32,
    pub intent_schema: Digest,
    pub operation_schema: Digest,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CapabilityBinding {
    pub phase: Phase,
    pub command: String,
    pub descriptor: Digest,
    pub effects: BTreeSet<EffectClass>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProfileDescriptor {
    pub identity: ProfileIdentity,
    pub capabilities: Vec<CapabilityBinding>,
    pub required_rules: BTreeSet<String>,
    pub allowed_effects: BTreeSet<EffectClass>,
}
impl ProfileDescriptor {
    pub fn validate(&self) -> Result<()> {
        bounded_id(&self.identity.id)?;
        ensure(self.identity.version > 0, "profile version")?;
        ensure(
            !self.capabilities.is_empty() && self.capabilities.len() <= 8,
            "profile capabilities",
        )?;
        let mut phases = BTreeSet::new();
        let mut names = BTreeSet::new();
        for c in &self.capabilities {
            bounded_id(&c.command)?;
            ensure(
                phases.insert(c.phase) && names.insert(&c.command),
                "ambiguous capability binding",
            )?;
            ensure(
                c.effects.is_subset(&self.allowed_effects),
                "capability effects outside profile",
            )?;
        }
        for r in &self.required_rules {
            bounded_id(r)?;
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConvergenceBudget {
    pub max_iterations: u32,
    pub max_operations: u32,
    pub max_findings: u32,
    pub max_observations: u32,
    pub max_elapsed_ms: u64,
}
impl ConvergenceBudget {
    pub fn validate(&self) -> Result<()> {
        ensure(
            (1..=32).contains(&self.max_iterations)
                && (1..=4096).contains(&self.max_operations)
                && (1..=4096).contains(&self.max_findings)
                && (1..=1024).contains(&self.max_observations)
                && (1..=3_600_000).contains(&self.max_elapsed_ms),
            "budget outside hard limits",
        )
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Atomicity {
    InMemoryTransaction,
    AtomicFileReplacement,
    NativeUndoGroup,
    NonAtomicSequence,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TypedOperation<O> {
    pub id: String,
    pub payload: O,
    pub reads: Vec<Address>,
    pub writes: Vec<Address>,
    pub effects: BTreeSet<EffectClass>,
    pub depends_on: Vec<String>,
    pub postconditions: BTreeSet<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChangeSet<O> {
    pub operations: Vec<TypedOperation<O>>,
    pub atomicity: Atomicity,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlanBody<I, O> {
    pub contract_version: u32,
    pub profile: ProfileIdentity,
    pub owner: Owner,
    pub base: BaseStateSet,
    pub intent: I,
    pub intent_digest: Digest,
    pub dependencies: BTreeMap<String, Digest>,
    pub changes: ChangeSet<O>,
    pub required_rules: BTreeSet<String>,
    pub observation_scope: Vec<Address>,
    pub budget: ConvergenceBudget,
    pub require_compare_and_swap: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PreparedPlan<I, O> {
    pub body: PlanBody<I, O>,
    pub digest: Digest,
}
impl<I: Serialize, O: Serialize> PreparedPlan<I, O> {
    pub fn prepare(body: PlanBody<I, O>, profile: &ProfileDescriptor) -> Result<Self> {
        let digest = canonical_digest(&body)?;
        let p = Self { body, digest };
        p.verify(profile)?;
        Ok(p)
    }
    pub fn verify(&self, profile: &ProfileDescriptor) -> Result<()> {
        profile.validate()?;
        let b = &self.body;
        b.owner.validate()?;
        b.base.validate()?;
        b.budget.validate()?;
        ensure(
            b.contract_version == CONTRACT_VERSION && b.profile == profile.identity,
            "contract/profile version mismatch",
        )?;
        ensure(
            b.intent_digest == canonical_digest(&b.intent)? && self.digest == canonical_digest(b)?,
            "plan/intent digest mismatch",
        )?;
        ensure(
            b.required_rules == profile.required_rules,
            "required rules differ from trusted profile",
        )?;
        ensure(
            !b.changes.operations.is_empty()
                && b.changes.operations.len() <= b.budget.max_operations as usize,
            "operation count",
        )?;
        ensure(
            b.dependencies.len() <= 256 && b.observation_scope.len() <= 4096,
            "dependency/scope limit",
        )?;
        for k in b.dependencies.keys() {
            bounded_id(k)?;
        }
        let resources: BTreeSet<_> = b.base.0.iter().map(|s| &s.key).collect();
        let mut seen = BTreeSet::new();
        for op in &b.changes.operations {
            bounded_id(&op.id)?;
            ensure(
                op.depends_on.iter().all(|d| seen.contains(d)),
                "dependency cycle, missing dependency or non-topological order",
            )?;
            ensure(seen.insert(op.id.clone()), "duplicate operation")?;
            ensure(
                !op.effects.is_empty() && op.effects.is_subset(&profile.allowed_effects),
                "operation effect escalation",
            )?;
            ensure(
                op.reads.len() <= 4096 && op.writes.len() <= 4096,
                "read/write limit",
            )?;
            for a in op
                .reads
                .iter()
                .chain(&op.writes)
                .chain(&b.observation_scope)
            {
                bounded_id(&a.logical_id)?;
                bounded_id(&a.property)?;
                ensure(
                    resources.contains(&a.resource),
                    "address outside declared resource set",
                )?;
            }
        }
        if b.require_compare_and_swap {
            ensure(
                b.base
                    .0
                    .iter()
                    .all(|s| s.concurrency == Concurrency::CompareAndSwap),
                "backend lacks required CAS",
            )?;
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    Prepared,
    Applying,
    Completed,
    Partial,
    Denied,
    Cancelled,
    Failed,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EvidenceClass {
    Deterministic,
    Heuristic,
    AestheticAssist,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceSource {
    NativeApi,
    RendererState,
    DecodedMedia,
    FileRead,
    Simulation,
    Fixture,
    HumanReview,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SupportLevel {
    Native,
    Composed,
    ReadOnly,
    UpstreamRestricted,
    SecurityExcluded,
    Unsupported,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Verdict {
    Pass,
    Fail,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
    Info,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ObservationRef {
    pub id: String,
    pub base: BaseStateSet,
    pub source: EvidenceSource,
    pub method: String,
    pub method_version: u32,
    pub scope: Vec<Address>,
    pub artifact: Option<Digest>,
    pub exhaustive: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeasurementRef<T> {
    pub observation: ObservationRef,
    pub units: String,
    pub value: Option<T>,
    pub unknown_reason: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RepairCandidate<O> {
    pub id: String,
    pub operation: TypedOperation<O>,
    pub fresh_base: BaseStateSet,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Finding<T, O> {
    pub id: String,
    pub rule: String,
    pub rule_version: u32,
    pub subjects: Vec<Address>,
    pub expected: T,
    pub actual: Option<T>,
    pub severity: Severity,
    pub evidence_class: EvidenceClass,
    pub observation: ObservationRef,
    pub uncertainty: Option<String>,
    pub repairs: Vec<RepairCandidate<O>>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuleResult {
    pub rule: String,
    pub version: u32,
    pub verdict: Verdict,
    pub evidence_class: EvidenceClass,
    pub evidence: Vec<ObservationRef>,
    pub reason: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ValidationReport {
    pub plan_digest: Digest,
    pub base: BaseStateSet,
    pub required_rules: BTreeSet<String>,
    pub checks: Vec<RuleResult>,
}
impl ValidationReport {
    pub fn verdict(&self) -> Result<Verdict> {
        self.base.validate()?;
        ensure(
            !self.required_rules.is_empty()
                && self.required_rules.len() <= 256
                && self.checks.len() <= 256,
            "validation rule count",
        )?;
        let mut seen = BTreeSet::new();
        let mut unknown = false;
        let mut failed = false;
        for check in &self.checks {
            bounded_id(&check.rule)?;
            ensure(
                check.version > 0 && seen.insert(check.rule.clone()),
                "invalid or duplicate rule result",
            )?;
            if !self.required_rules.contains(&check.rule) {
                continue;
            }
            if check.verdict == Verdict::Fail {
                failed = true;
            }
            if check.verdict != Verdict::Pass
                || check.evidence.is_empty()
                || check.evidence_class != EvidenceClass::Deterministic
            {
                unknown = true;
            }
            for ev in &check.evidence {
                ev.base.validate()?;
                if ev.base != self.base
                    || !ev.exhaustive
                    || matches!(
                        ev.source,
                        EvidenceSource::Fixture | EvidenceSource::Simulation
                    )
                {
                    unknown = true;
                }
            }
        }
        if failed {
            Ok(Verdict::Fail)
        } else if unknown || !self.required_rules.is_subset(&seen) {
            Ok(Verdict::Unknown)
        } else {
            Ok(Verdict::Pass)
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VerificationReport {
    pub execution_status: ExecutionStatus,
    pub validation: ValidationReport,
    pub support_level: SupportLevel,
    pub effects_observed: Vec<Address>,
    pub effects_unobservable: Vec<Address>,
}
impl VerificationReport {
    pub fn verdict(&self) -> Result<Verdict> {
        let v = self.validation.verdict()?;
        Ok(
            if v == Verdict::Pass && self.execution_status != ExecutionStatus::Completed {
                Verdict::Unknown
            } else {
                v
            },
        )
    }
}
