//! Bounded independent file reader using canonical Effects evaluation.
//! This verifies selected immutable artifact properties, never native mutation,
//! noninterference, creative quality, execution authority or a producer's honesty.
use schemars::JsonSchema;
use semwright_effect_conformance::{composition::*, *};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Read,
    path::{Component, Path},
};

pub const SPEC_SCHEMA: &str = "semwright-native-effects-spec/1";
pub const RESULT_SCHEMA: &str = "semwright-native-effects-result/1";
pub const SCOPE: &str = "immutable_native_sdk_artifact_properties_only";
const READER: &str = "semwright.native-artifact-reader";
const METHOD: &str = "native-sdk.artifact-scalar-read";
const OPERATION: &str = "read-artifacts";
const MAX_FILE: u64 = 1_048_576;
const MAX_ELAPSED_MS: u64 = 10_000;
fn elapsed_budget(elapsed: std::time::Duration) -> Result<()> {
    if elapsed > std::time::Duration::from_millis(MAX_ELAPSED_MS) {
        Err(ContractError::Limit(
            "independent readback elapsed budget exceeded".into(),
        ))
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArtifactBinding {
    pub slot: String,
    pub path: String,
    pub sha256: Digest,
    pub bytes: u64,
    pub mime_type: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScalarKind {
    Bool,
    Text,
    Number { units: String },
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Selector {
    Json {
        pointer: String,
        scalar: ScalarKind,
    },
    Csv {
        row: u32,
        column: String,
        scalar: ScalarKind,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PropertyCheck {
    pub id: String,
    pub artifact_slot: String,
    pub selector: Selector,
    pub predicate: Predicate,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SpecificationInput {
    pub owner: Owner,
    pub request_id: String,
    pub source_digest: Digest,
    pub runtime_digest: Digest,
    /// An attribution supplied by the protected operator, not observed here.
    pub declared_producer_execution_status: ExecutionStatus,
    pub application_roots: Vec<String>,
    pub artifacts: Vec<ArtifactBinding>,
    pub checks: Vec<PropertyCheck>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProtectedSpec {
    pub schema_version: String,
    pub definition: SpecificationInput,
    pub profile: ProfileDescriptor,
    pub plan: PreparedPlan<Value, Value>,
    pub contract: EffectContract,
}

pub fn decoder_digest() -> Digest {
    Digest::of_bytes(b"native-artifact-reader/v1:json-strict-512k-depth32-csv1m-4096x64-scalar-binary64-abs-lt-2^53-independent-units")
}
fn token(value: &str) -> Result<()> {
    ensure(
        !value.is_empty()
            && value.len() <= 64
            && value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c)),
        "bounded token required",
    )
}
fn relative(path: &str) -> Result<()> {
    ensure(
        !path.is_empty() && path.len() <= 512 && !Path::new(path).is_absolute(),
        "relative artifact path required",
    )?;
    let parts = Path::new(path).components().collect::<Vec<_>>();
    ensure(
        !parts.is_empty()
            && parts.len() <= 8
            && parts.iter().all(|p| matches!(p, Component::Normal(_))),
        "artifact path components",
    )?;
    ensure(
        !path.contains('\\') && !path.chars().any(char::is_control),
        "artifact path characters",
    )
}
fn absolute(path: &Path) -> Result<()> {
    ensure(
        path.is_absolute()
            && path
                .components()
                .all(|p| matches!(p, Component::RootDir | Component::Normal(_))),
        "absolute normalized path required",
    )
}
fn session(d: &SpecificationInput) -> String {
    format!("readback:{}", d.request_id)
}
fn resource(slot: &str) -> ResourceKey {
    ResourceKey {
        provider: READER.into(),
        resource: slot.into(),
    }
}
fn address(c: &PropertyCheck) -> Address {
    Address {
        resource: resource(&c.artifact_slot),
        logical_id: c.id.clone(),
        property: "scalar".into(),
    }
}
fn base(d: &SpecificationInput, digests: &BTreeMap<String, Digest>) -> BaseStateSet {
    BaseStateSet(
        d.artifacts
            .iter()
            .filter_map(|a| {
                digests.get(&a.slot).map(|sha| BaseState {
                    key: resource(&a.slot),
                    document_id: format!("artifact:{}", a.slot),
                    provider_session: session(d),
                    generation: decoder_digest().as_str().into(),
                    revision: Revision::Fingerprint(sha.clone()),
                    concurrency: Concurrency::BestEffortRevalidate,
                })
            })
            .collect(),
    )
}
fn expected_base(d: &SpecificationInput) -> BaseStateSet {
    base(
        d,
        &d.artifacts
            .iter()
            .map(|a| (a.slot.clone(), a.sha256.clone()))
            .collect(),
    )
}
fn numeric(n: f64) -> Result<f64> {
    ensure(
        n.is_finite() && n.abs() < 9_007_199_254_740_992.0,
        "number outside exact integer/binary64 profile",
    )?;
    Ok(n)
}
fn validate_json(v: &Value, depth: usize) -> Result<()> {
    ensure(depth <= 32, "JSON depth exceeds32")?;
    match v {
        Value::Number(n) => {
            numeric(
                n.as_f64()
                    .ok_or_else(|| ContractError::Invalid("nonfinite number".into()))?,
            )?;
        }
        Value::Array(a) => {
            ensure(a.len() <= 4096, "JSON entry limit")?;
            for x in a {
                validate_json(x, depth + 1)?;
            }
        }
        Value::Object(o) => {
            ensure(o.len() <= 4096, "JSON entry limit")?;
            for x in o.values() {
                validate_json(x, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn validate_scalar_kind(k: &ScalarKind) -> Result<()> {
    if let ScalarKind::Number { units } = k {
        bounded_id(units)?;
        ensure(units.len() <= 32, "units limit")?;
    }
    Ok(())
}
fn validate_definition(d: &SpecificationInput) -> Result<()> {
    d.owner.validate()?;
    token(&d.request_id)?;
    ensure(
        (1..=16).contains(&d.application_roots.len()),
        "application root count",
    )?;
    for p in &d.application_roots {
        absolute(Path::new(p))?;
    }
    ensure(
        (1..=16).contains(&d.artifacts.len()) && (1..=64).contains(&d.checks.len()),
        "artifact/check count",
    )?;
    let mut artifacts = BTreeMap::new();
    let mut paths = BTreeSet::new();
    let mut total = 0u64;
    for a in &d.artifacts {
        token(&a.slot)?;
        relative(&a.path)?;
        ensure(
            artifacts.insert(a.slot.clone(), a).is_none() && paths.insert(&a.path),
            "duplicate artifact slot/path",
        )?;
        ensure(a.bytes > 0 && a.bytes <= MAX_FILE, "artifact size")?;
        total += a.bytes;
        ensure(
            matches!(a.mime_type.as_str(), "application/json" | "text/csv"),
            "unsupported artifact MIME",
        )?;
        if a.mime_type == "application/json" {
            ensure(a.bytes <= MAX_PAYLOAD_BYTES as u64, "JSON byte limit")?;
        }
    }
    ensure(total <= 16 * MAX_FILE, "total artifact size")?;
    let mut ids = BTreeSet::new();
    let mut used = BTreeSet::new();
    for c in &d.checks {
        token(&c.id)?;
        ensure(ids.insert(&c.id), "duplicate check")?;
        let a = artifacts
            .get(&c.artifact_slot)
            .ok_or_else(|| ContractError::Invalid("missing artifact slot".into()))?;
        used.insert(c.artifact_slot.clone());
        match &c.selector {
            Selector::Json { pointer, scalar } => {
                ensure(
                    a.mime_type == "application/json"
                        && pointer.starts_with('/')
                        && pointer.len() <= 256
                        && pointer.split('/').count() <= 9,
                    "JSON selector bounds/MIME",
                )?;
                for segment in pointer.split('/').skip(1) {
                    let mut chars = segment.chars();
                    while let Some(ch) = chars.next() {
                        if ch == '~' {
                            ensure(
                                matches!(chars.next(), Some('0' | '1')),
                                "invalid pointer escape",
                            )?;
                        }
                    }
                }
                validate_scalar_kind(scalar)?;
            }
            Selector::Csv {
                row,
                column,
                scalar,
            } => {
                ensure(
                    a.mime_type == "text/csv" && *row < 4096,
                    "CSV selector bounds/MIME",
                )?;
                bounded_id(column)?;
                ensure(column.len() <= 256, "column limit")?;
                validate_scalar_kind(scalar)?;
            }
        }
        c.predicate.validate()?;
        match &c.predicate {
            Predicate::Equals { expected } => match expected {
                ObservedValue::Bool { .. } | ObservedValue::Text { .. } => {}
                ObservedValue::Number { value, .. } => {
                    numeric(*value)?;
                }
                _ => return Err(ContractError::Invalid("non-scalar expected value".into())),
            },
            Predicate::Within {
                expected,
                tolerance,
                ..
            } => {
                numeric(*expected)?;
                numeric(*tolerance)?;
            }
            Predicate::Range { min, max, .. } => {
                numeric(*min)?;
                numeric(*max)?;
            }
            _ => {
                return Err(ContractError::Invalid(
                    "unsupported non-scalar predicate".into(),
                ));
            }
        }
    }
    ensure(used.len() == artifacts.len(), "unused artifact binding")
}
/// Pure canonical data preparation. It does not inspect or execute an application,
/// grant filesystem access, or authenticate the supplied context declarations.
pub fn prepare_spec(definition: SpecificationInput) -> Result<ProtectedSpec> {
    validate_definition(&definition)?;
    let contract = EffectContract {
        version: 1,
        profile: "native-artifact-properties".into(),
        allowed: vec![],
        rules: definition
            .checks
            .iter()
            .map(|c| EffectRule {
                id: c.id.clone(),
                version: 1,
                obligation: Obligation::Required,
                operation_id: OPERATION.into(),
                address: address(c),
                predicate: c.predicate.clone(),
                method: ObservationMethod {
                    name: METHOD.into(),
                    version: 1,
                    source: EvidenceSource::FileRead,
                },
                universe: None,
                artifact: Some(
                    definition
                        .artifacts
                        .iter()
                        .find(|a| a.slot == c.artifact_slot)
                        .unwrap()
                        .sha256
                        .clone(),
                ),
                require_causal_attribution: false,
            })
            .collect(),
    };
    let profile = ProfileDescriptor {
        identity: ProfileIdentity {
            id: contract.profile.clone(),
            version: 1,
            intent_schema: schema_digest::<SpecificationInput>()?,
            operation_schema: canonical_digest(&json!({"operation":"read-artifacts","version":1}))?,
        },
        capabilities: vec![CapabilityBinding {
            phase: Phase::Measure,
            command: METHOD.into(),
            descriptor: decoder_digest(),
            effects: BTreeSet::from([EffectClass::Inspect]),
        }],
        required_rules: contract.required_rules(),
        allowed_effects: BTreeSet::from([EffectClass::Inspect]),
    };
    let intent =
        serde_json::to_value(&definition).map_err(|e| ContractError::Invalid(e.to_string()))?;
    let scope = contract
        .rules
        .iter()
        .map(|r| r.address.clone())
        .collect::<Vec<_>>();
    let plan = PreparedPlan::prepare(
        PlanBody {
            contract_version: CONTRACT_VERSION,
            profile: profile.identity.clone(),
            owner: definition.owner.clone(),
            base: expected_base(&definition),
            intent_digest: canonical_digest(&intent)?,
            intent,
            dependencies: BTreeMap::from([
                ("effects.contract".into(), contract.digest()?),
                ("effects.source".into(), definition.source_digest.clone()),
                ("effects.runtime".into(), definition.runtime_digest.clone()),
                ("effects.decoder".into(), decoder_digest()),
            ]),
            changes: ChangeSet {
                atomicity: Atomicity::NonAtomicSequence,
                operations: vec![TypedOperation {
                    id: OPERATION.into(),
                    payload: Value::Null,
                    reads: scope.clone(),
                    writes: vec![],
                    effects: BTreeSet::from([EffectClass::Inspect]),
                    depends_on: vec![],
                    postconditions: contract.required_rules(),
                }],
            },
            required_rules: contract.required_rules(),
            observation_scope: scope,
            budget: ConvergenceBudget {
                max_iterations: 1,
                max_operations: 1,
                max_findings: 64,
                max_observations: 64,
                max_elapsed_ms: MAX_ELAPSED_MS,
            },
            require_compare_and_swap: false,
        },
        &profile,
    )?;
    Ok(ProtectedSpec {
        schema_version: SPEC_SCHEMA.into(),
        definition,
        profile,
        plan,
        contract,
    })
}
fn context(spec: &ProtectedSpec, after: BaseStateSet) -> EvaluationContext {
    EvaluationContext {
        owner: spec.plan.body.owner.clone(),
        request_id: spec.definition.request_id.clone(),
        plan_digest: spec.plan.digest.clone(),
        contract_digest: spec.contract.digest().expect("validated contract"),
        before: spec.plan.body.base.clone(),
        after,
        operations: BTreeSet::from([OPERATION.into()]),
        observation_scope: spec.plan.body.observation_scope.iter().cloned().collect(),
        execution_status: ExecutionStatus::Completed,
        support_level: SupportLevel::ReadOnly,
        budget: spec.plan.body.budget.clone(),
    }
}
impl ProtectedSpec {
    pub fn validate(&self) -> Result<()> {
        ensure(self.schema_version == SPEC_SCHEMA, "spec schema")?;
        let expected = prepare_spec(self.definition.clone())?;
        ensure(
            canonical_bytes(self)? == canonical_bytes(&expected)?,
            "spec/plan/profile/contract binding differs from compiled profile",
        )?;
        crate::composition_report::validate_plan(
            &self.plan,
            &self.profile,
            &self.contract,
            &context(self, expected_base(&self.definition)),
        )
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HelperState {
    Evaluated,
    Incomplete,
    Error,
}
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PrivateMeasurement {
    pub rule: String,
    pub rule_version: u32,
    pub address: Address,
    pub expected: Predicate,
    pub observed: Option<ObservedValue>,
    pub observation: Option<ObservationRef>,
    pub availability: String,
}
#[derive(Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VerificationResult {
    pub schema_version: String,
    pub spec_sha256: Digest,
    pub source_digest: Digest,
    pub runtime_digest: Digest,
    pub declared_producer_execution_status: ExecutionStatus,
    pub context_attestation: String,
    pub inspection_state: HelperState,
    pub scope: String,
    pub execution_authority: bool,
    pub evaluation: EffectEvaluation,
    pub verdict: Verdict,
    pub private_measurements: Vec<PrivateMeasurement>,
}
/// No Deserialize, public fields or constructor. It can only originate from the
/// fresh bounded reader below. An imported report cannot create this token.
#[derive(Debug)]
pub struct VerifiedRun {
    result: VerificationResult,
}
impl VerifiedRun {
    pub fn result(&self) -> &VerificationResult {
        &self.result
    }
}

struct Snapshot {
    digest: Option<Digest>,
    decoded: Option<Decoded>,
    issue: Option<(bool, String)>,
}
enum Decoded {
    Json(Value),
    Csv(Vec<Vec<String>>),
}
struct Reader<'a> {
    spec: &'a ProtectedSpec,
    snapshots: BTreeMap<String, Snapshot>,
    captured: Vec<(String, AdapterObservation)>,
}
fn scalar(v: &Value, k: &ScalarKind) -> Result<ObservedValue> {
    let result =
        match (k, v) {
            (ScalarKind::Bool, Value::Bool(value)) => ObservedValue::Bool { value: *value },
            (ScalarKind::Text, Value::String(value)) => ObservedValue::Text {
                value: value.clone(),
            },
            (ScalarKind::Number { units }, Value::Number(n)) => ObservedValue::Number {
                value: numeric(n.as_f64().ok_or_else(|| {
                    ContractError::Unknown("numeric conversion unavailable".into())
                })?)?,
                units: units.clone(),
            },
            _ => {
                return Err(ContractError::Unknown(
                    "selected scalar type is unavailable".into(),
                ));
            }
        };
    result.validate()?;
    Ok(result)
}
impl EvidenceAdapter for Reader<'_> {
    fn identity(&self, r: &ResourceKey) -> Option<AdapterIdentity> {
        self.snapshots
            .get(&r.resource)
            .filter(|s| s.digest.is_some())
            .map(|_| AdapterIdentity {
                owner: self.spec.definition.owner.clone(),
                provider: READER.into(),
                provider_session: session(&self.spec.definition),
                generation: decoder_digest().as_str().into(),
            })
    }
    fn observe(
        &mut self,
        ctx: &EvaluationContext,
        rule: &EffectRule,
    ) -> Result<AdapterObservation> {
        let c = self
            .spec
            .definition
            .checks
            .iter()
            .find(|c| c.id == rule.id)
            .ok_or_else(|| ContractError::Invalid("unbound check".into()))?;
        let snapshot = self
            .snapshots
            .get(&c.artifact_slot)
            .ok_or_else(|| ContractError::Unknown("artifact snapshot unavailable".into()))?;
        if let Some((_, reason)) = &snapshot.issue {
            return Err(ContractError::Unknown(reason.clone()));
        }
        let value = match (&c.selector, &snapshot.decoded) {
            (Selector::Json { pointer, scalar: k }, Some(Decoded::Json(v))) => scalar(
                v.pointer(pointer)
                    .ok_or_else(|| ContractError::Unknown("JSON property missing".into()))?,
                k,
            )?,
            (
                Selector::Csv {
                    row,
                    column,
                    scalar: k,
                },
                Some(Decoded::Csv(rows)),
            ) => {
                let col = rows[0]
                    .iter()
                    .position(|h| h == column)
                    .ok_or_else(|| ContractError::Unknown("CSV column missing".into()))?;
                let raw = rows
                    .get(*row as usize + 1)
                    .and_then(|r| r.get(col))
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| ContractError::Unknown("CSV value missing".into()))?;
                let v = match k {
                    ScalarKind::Text => Value::String(raw.clone()),
                    _ => strict_decode::<Value>(raw.as_bytes())?,
                };
                scalar(&v, k)?
            }
            _ => {
                return Err(ContractError::Unknown(
                    "decoded artifact unavailable".into(),
                ));
            }
        };
        let o = AdapterObservation {
            binding: EvidenceBinding {
                owner: ctx.owner.clone(),
                request_id: ctx.request_id.clone(),
                operation_id: rule.operation_id.clone(),
                plan_digest: ctx.plan_digest.clone(),
                contract_digest: ctx.contract_digest.clone(),
            },
            observation: ObservationRef {
                id: format!("file-readback:{}", rule.id),
                base: ctx.after.clone(),
                source: EvidenceSource::FileRead,
                method: METHOD.into(),
                method_version: 1,
                scope: vec![rule.address.clone()],
                artifact: rule.artifact.clone(),
                exhaustive: true,
            },
            readback: ReadbackState::Observed,
            value: Some(value),
            coverage: ObservationCoverage {
                consistent: true,
                missing: vec![],
                attribution: Attribution::Ordered,
                enumeration: None,
            },
        };
        self.captured.push((rule.id.clone(), o.clone()));
        Ok(o)
    }
}
fn measurements(
    spec: &ProtectedSpec,
    evaluation: &EffectEvaluation,
    captured: &[(String, AdapterObservation)],
) -> Result<Vec<PrivateMeasurement>> {
    spec.contract
        .rules
        .iter()
        .map(|rule| {
            let check = evaluation
                .report
                .validation
                .checks
                .iter()
                .find(|c| c.rule == rule.id && c.version == rule.version)
                .ok_or_else(|| ContractError::Invalid("canonical rule result missing".into()))?;
            let evidence = check
                .evidence
                .iter()
                .map(canonical_bytes)
                .collect::<Result<Vec<_>>>()?;
            let mut matches = Vec::new();
            for (id, o) in captured {
                if id == &rule.id
                    && o.observation.scope == vec![rule.address.clone()]
                    && evidence.contains(&canonical_bytes(&o.observation)?)
                {
                    matches.push(o);
                }
            }
            ensure(matches.len() <= 1, "ambiguous same-run readback")?;
            let accepted = if matches.len() == 1 && check.verdict != Verdict::Unknown {
                Some(matches[0])
            } else {
                None
            };
            Ok(PrivateMeasurement {
                rule: rule.id.clone(),
                rule_version: rule.version,
                address: rule.address.clone(),
                expected: rule.predicate.clone(),
                observed: accepted.and_then(|o| o.value.clone()),
                observation: accepted.map(|o| o.observation.clone()),
                availability: if accepted.is_some() {
                    "CANONICAL_SAME_RUN_READBACK"
                } else {
                    "UNKNOWN"
                }
                .into(),
            })
        })
        .collect()
}

fn csv(bytes: &[u8]) -> Result<Vec<Vec<String>>> {
    let text =
        std::str::from_utf8(bytes).map_err(|_| ContractError::Invalid("CSV UTF-8".into()))?;
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut closed = false;
    let mut at_start = true;
    let mut chars = text.chars().peekable();
    fn cell(row: &mut Vec<String>, field: &mut String) -> Result<()> {
        ensure(
            field.len() <= 4096 && row.len() < 64,
            "CSV field/column limit",
        )?;
        row.push(std::mem::take(field));
        Ok(())
    }
    while let Some(ch) = chars.next() {
        if quoted {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    quoted = false;
                    closed = true;
                }
            } else if ch == '\r' {
                ensure(chars.next() == Some('\n'), "bare CSV CR in quoted field")?;
                field.push('\r');
                field.push('\n');
            } else {
                field.push(ch);
            }
            ensure(field.len() <= 4096, "CSV field limit")?;
            continue;
        }
        if ch == '"' {
            ensure(at_start && !closed, "CSV quote placement")?;
            quoted = true;
            at_start = false;
            continue;
        }
        match ch {
            ',' => {
                cell(&mut row, &mut field)?;
                closed = false;
                at_start = true;
            }
            '\r' | '\n' => {
                if ch == '\r' {
                    ensure(chars.next() == Some('\n'), "bare CSV CR")?;
                }
                cell(&mut row, &mut field)?;
                ensure(rows.len() < 4097, "CSV row limit")?;
                rows.push(std::mem::take(&mut row));
                closed = false;
                at_start = true;
            }
            _ => {
                ensure(!closed, "data after CSV closing quote")?;
                field.push(ch);
                at_start = false;
                ensure(field.len() <= 4096, "CSV field limit")?;
            }
        }
    }
    ensure(!quoted, "unterminated CSV quote")?;
    if !row.is_empty() || !field.is_empty() || closed || !at_start {
        cell(&mut row, &mut field)?;
        ensure(rows.len() < 4097, "CSV row limit")?;
        rows.push(row);
    }
    ensure(
        !rows.is_empty() && !rows[0].is_empty(),
        "CSV header missing",
    )?;
    let header = &rows[0];
    let unique = header.iter().collect::<BTreeSet<_>>();
    ensure(
        unique.len() == header.len()
            && header
                .iter()
                .all(|s| !s.is_empty() && s.len() <= 256 && !s.chars().any(char::is_control)),
        "CSV header invalid/duplicate",
    )?;
    ensure(rows.iter().all(|r| r.len() == header.len()), "ragged CSV")?;
    Ok(rows)
}

#[cfg(target_os = "linux")]
mod files {
    use super::*;
    use rustix::fs::{Mode, OFlags, open, openat};
    use std::os::unix::fs::MetadataExt;
    pub struct Root {
        pub file: File,
        pub ancestry: Vec<(u64, u64)>,
    }
    fn err(_: impl std::fmt::Display) -> ContractError {
        ContractError::Unknown("bounded file open/read unavailable".into())
    }
    fn id(m: &std::fs::Metadata) -> (u64, u64) {
        (m.dev(), m.ino())
    }
    pub fn directory(path: &Path) -> Result<Root> {
        absolute(path)?;
        let mut file = File::from(
            open(
                "/",
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(err)?,
        );
        let mut ancestry = vec![id(&file.metadata().map_err(err)?)];
        for part in path.components() {
            if let Component::Normal(name) = part {
                file = File::from(
                    openat(
                        &file,
                        name,
                        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                        Mode::empty(),
                    )
                    .map_err(err)?,
                );
                ancestry.push(id(&file.metadata().map_err(err)?));
            }
        }
        Ok(Root { file, ancestry })
    }
    fn same(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
        id(a) == id(b)
            && a.len() == b.len()
            && a.mtime() == b.mtime()
            && a.mtime_nsec() == b.mtime_nsec()
            && a.ctime() == b.ctime()
            && a.ctime_nsec() == b.ctime_nsec()
    }
    pub fn read(root: &Root, path: &str, max: u64, protected: bool) -> Result<Vec<u8>> {
        relative(path)?;
        let parts = Path::new(path).components().collect::<Vec<_>>();
        let mut dir = root.file.try_clone().map_err(err)?;
        for part in &parts[..parts.len() - 1] {
            if let Component::Normal(name) = part {
                dir = File::from(
                    openat(
                        &dir,
                        *name,
                        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                        Mode::empty(),
                    )
                    .map_err(err)?,
                );
            }
        }
        let Component::Normal(name) = parts[parts.len() - 1] else {
            return Err(ContractError::Invalid("file name".into()));
        };
        let mut file = File::from(
            openat(
                &dir,
                name,
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(err)?,
        );
        let before = file.metadata().map_err(err)?;
        ensure(
            before.is_file() && before.len() <= max,
            "bounded regular file required",
        )?;
        if protected {
            ensure(
                before.mode() & 0o777 == 0o600
                    && before.uid() == rustix::process::geteuid().as_raw()
                    && before.nlink() == 1,
                "protected spec ownership/mode/hardlink",
            )?;
        }
        let mut bytes = Vec::new();
        (&mut file)
            .take(max + 1)
            .read_to_end(&mut bytes)
            .map_err(err)?;
        ensure(
            bytes.len() as u64 <= max
                && bytes.len() as u64 == before.len()
                && same(&before, &file.metadata().map_err(err)?),
            "file snapshot changed or exceeded bound",
        )?;
        Ok(bytes)
    }
    pub fn disjoint(a: &Root, b: &Root) -> Result<()> {
        ensure(
            !a.ancestry.contains(b.ancestry.last().unwrap())
                && !b.ancestry.contains(a.ancestry.last().unwrap()),
            "protected/admitted roots overlap application roots",
        )
    }
}

/// Read a protected exact spec and independently snapshot admitted bytes. Linux
/// FD-relative profile only. No subprocesses, sockets, application writes or grants.
#[cfg(target_os = "linux")]
pub fn verify(spec_path: &Path, spec_sha256: &Digest, artifact_root: &Path) -> Result<VerifiedRun> {
    let started = std::time::Instant::now();
    elapsed_budget(started.elapsed())?;
    absolute(spec_path)?;
    let spec_dir = files::directory(
        spec_path
            .parent()
            .ok_or_else(|| ContractError::Invalid("spec parent".into()))?,
    )?;
    let name = spec_path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| ContractError::Invalid("spec filename".into()))?;
    elapsed_budget(started.elapsed())?;
    let bytes = files::read(&spec_dir, name, MAX_PAYLOAD_BYTES as u64, true)?;
    elapsed_budget(started.elapsed())?;
    ensure(
        Digest::of_bytes(&bytes) == *spec_sha256,
        "protected spec digest differs",
    )?;
    let raw: Value = strict_decode(&bytes)?;
    validate_json(&raw, 0)?;
    let spec: ProtectedSpec =
        serde_json::from_value(raw).map_err(|e| ContractError::Invalid(e.to_string()))?;
    spec.validate()?;
    let root = files::directory(artifact_root)?;
    let application_roots = spec
        .definition
        .application_roots
        .iter()
        .map(|p| files::directory(Path::new(p)))
        .collect::<Result<Vec<_>>>()?;
    for app in &application_roots {
        files::disjoint(&spec_dir, app)?;
        files::disjoint(&root, app)?;
    }
    let mut snapshots = BTreeMap::new();
    let mut digests = BTreeMap::new();
    let mut error = false;
    for a in &spec.definition.artifacts {
        let mut snapshot = Snapshot {
            digest: None,
            decoded: None,
            issue: None,
        };
        elapsed_budget(started.elapsed())?;
        let read = files::read(&root, &a.path, MAX_FILE, false);
        elapsed_budget(started.elapsed())?;
        match read {
            Err(e) => {
                snapshot.issue = Some((
                    !matches!(e, ContractError::Unknown(_)),
                    "artifact read unavailable or unstable".into(),
                ));
            }
            Ok(bytes) => {
                let actual = Digest::of_bytes(&bytes);
                digests.insert(a.slot.clone(), actual.clone());
                snapshot.digest = Some(actual.clone());
                if actual != a.sha256 || bytes.len() as u64 != a.bytes {
                    snapshot.issue = Some((
                        true,
                        "artifact digest/size differs from admitted binding".into(),
                    ));
                } else {
                    let decoded = if a.mime_type == "application/json" {
                        strict_decode::<Value>(&bytes).and_then(|v| {
                            validate_json(&v, 0)?;
                            Ok(Decoded::Json(v))
                        })
                    } else {
                        csv(&bytes).map(Decoded::Csv)
                    };
                    match decoded {
                        Ok(v) => snapshot.decoded = Some(v),
                        Err(_) => {
                            snapshot.issue = Some((
                                true,
                                "artifact decode failed within supported profile".into(),
                            ))
                        }
                    }
                }
            }
        }
        error |= snapshot
            .issue
            .as_ref()
            .is_some_and(|(is_error, _)| *is_error);
        snapshots.insert(a.slot.clone(), snapshot);
    }
    ensure(
        !digests.is_empty(),
        "no artifact snapshot available; no canonical report can be established",
    )?;
    elapsed_budget(started.elapsed())?;
    let ctx = context(&spec, base(&spec.definition, &digests));
    ctx.validate_plan(&spec.plan, &spec.profile, &spec.contract)?;
    let mut reader = Reader {
        spec: &spec,
        snapshots,
        captured: vec![],
    };
    let batch = collect(&spec.contract, &ctx, &mut reader)?;
    let evaluation = evaluate(&spec.contract, &ctx, &batch)?;
    let private_measurements = measurements(&spec, &evaluation, &reader.captured)?;
    let verdict = evaluation.verdict()?;
    let inspection_state = if error {
        HelperState::Error
    } else if evaluation.coverage.iter().any(|c| !c.sufficient) {
        HelperState::Incomplete
    } else {
        HelperState::Evaluated
    };
    elapsed_budget(started.elapsed())?;
    Ok(VerifiedRun {
        result: VerificationResult {
            schema_version: RESULT_SCHEMA.into(),
            spec_sha256: spec_sha256.clone(),
            source_digest: spec.definition.source_digest.clone(),
            runtime_digest: spec.definition.runtime_digest.clone(),
            declared_producer_execution_status: spec.definition.declared_producer_execution_status,
            context_attestation: "DECLARED_CONTEXT_NOT_NATIVE_EXECUTION_ATTESTATION".into(),
            inspection_state,
            scope: SCOPE.into(),
            execution_authority: false,
            evaluation,
            verdict,
            private_measurements,
        },
    })
}
#[cfg(not(target_os = "linux"))]
pub fn verify(
    _spec_path: &Path,
    _spec_sha256: &Digest,
    _artifact_root: &Path,
) -> Result<VerifiedRun> {
    Err(ContractError::Denied(
        "Linux descriptor-relative read profile is required".into(),
    ))
}

pub fn validate_public_input(bytes: &[u8]) -> Result<()> {
    ensure(bytes.len() <= 64, "stdin byte limit")?;
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(());
    }
    let v: Value = strict_decode(bytes)?;
    ensure(
        v.as_object().is_some_and(|o| o.is_empty()),
        "public input must be empty; observations/context/authority are forbidden",
    )
}

#[cfg(test)]
mod budget_tests {
    use super::*;
    #[test]
    fn exact_elapsed_boundary_is_checked_without_sleep_or_clock_rounding() {
        let limit = std::time::Duration::from_millis(MAX_ELAPSED_MS);
        assert!(elapsed_budget(std::time::Duration::ZERO).is_ok());
        assert!(elapsed_budget(limit).is_ok());
        assert!(matches!(
            elapsed_budget(limit + std::time::Duration::from_nanos(1)),
            Err(ContractError::Limit(_))
        ));
    }
}
