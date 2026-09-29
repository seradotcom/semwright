//! CI-only adapter conformance: real native subprocesses feed the common evaluator.
//! This is NOT a Broker bypass or a public capability; product integration stays
//! owned by D/E. Fixed source scripts and disposable roots never accept user code.
use semwright_effect_conformance::composition::*;
use semwright_effect_conformance::*;
use serde::Deserialize;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::process::Command;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeMeasurement {
    schema_version: u32,
    backend: String,
    case: String,
    runtime: String,
    before_digest: Digest,
    after_digest: Digest,
    values: BTreeMap<String, ObservedValue>,
    writer_process: String,
    reader_process: String,
    isolation: String,
    limits: BTreeMap<String, u64>,
    route: String,
    crash_durability: String,
}
struct NativeAdapter {
    owner: Owner,
    session: String,
    measured: BTreeMap<String, ObservedValue>,
}
impl EvidenceAdapter for NativeAdapter {
    fn identity(&self, resource: &ResourceKey) -> Option<AdapterIdentity> {
        Some(AdapterIdentity { owner: self.owner.clone(), provider: resource.provider.clone(),
            provider_session: self.session.clone(), generation: "native-invocation-1".into() })
    }
    fn observe(&mut self, ctx: &EvaluationContext, rule: &EffectRule) -> Result<AdapterObservation> {
        let value = self.measured.get(&rule.id).cloned()
            .ok_or_else(|| ContractError::Unknown(format!("native observer did not produce {}", rule.id)))?;
        let enumeration = if let ObservedValue::Members { values } = &value {
            Some(vec![EnumerationPage { binding: ctx.enumeration_binding(rule)?, index: 0,
                cursor_in: None, cursor_out: None, items: values.iter().cloned().collect(),
                total: Some(values.len() as u32), final_page: true, truncated: false,
                consistency: EnumerationConsistency::Snapshot }])
        } else { None };
        Ok(AdapterObservation { binding: EvidenceBinding { owner: ctx.owner.clone(), request_id: ctx.request_id.clone(),
            operation_id: rule.operation_id.clone(), plan_digest: ctx.plan_digest.clone(), contract_digest: ctx.contract_digest.clone() },
            observation: ObservationRef { id: format!("{}:{}",ctx.request_id,rule.id), base: ctx.after.clone(), source: rule.method.source,
                method: rule.method.name.clone(), method_version: rule.method.version, scope: vec![rule.address.clone()], artifact: None,
                exhaustive: true }, readback: ReadbackState::Observed, value: Some(value),
            coverage: ObservationCoverage { consistent: true, missing: vec![], attribution: Attribution::Isolated, enumeration } })
    }
}
fn contract(backend: &str) -> EffectContract {
    let resource = ResourceKey { provider: format!("driver:{backend}"), resource: "isolated-root".into() };
    let mut specs = vec![
        ("position", Predicate::Within { expected: 1.0, tolerance: 0.000001, units: "metre".into() }, EvidenceSource::NativeApi),
        ("persistence", Predicate::Reopened, EvidenceSource::NativeApi),
        ("sentinel", Predicate::Preserved, EvidenceSource::FileRead),
        ("inventory", Predicate::Preserved, EvidenceSource::FileRead),
    ];
    if backend == "godot" {
        specs.push(("material",Predicate::Preserved,EvidenceSource::FileRead));
        specs.push(("animation",Predicate::Preserved,EvidenceSource::FileRead));
        specs.push(("relation",Predicate::Relation { target: "Anchor".into(), binding: "parent".into() },EvidenceSource::NativeApi));
    } else {
        specs.push(("membership",Predicate::Membership { expected: BTreeSet::from(["Hero".into()]) },EvidenceSource::DecodedMedia));
        specs.push(("artifact",Predicate::Preserved,EvidenceSource::FileRead));
    }
    EffectContract { version: 1, profile: format!("{backend}.native-adapter"), allowed: vec![], rules: specs.into_iter().map(|(id,predicate,source)| EffectRule {
        id: id.into(), version: 1, obligation: if matches!(id,"sentinel"|"inventory"|"material"|"animation") { Obligation::Forbidden } else { Obligation::Required },
        operation_id: "native-workflow".into(), address: Address { resource: resource.clone(), logical_id: id.into(), property: "projection".into() },
        universe: if id == "membership" { Some("exported-glb-nodes".into()) } else { None }, predicate,
        method: ObservationMethod { name: format!("{backend}.{id}.v1"), version: 1, source }, artifact: None, require_causal_attribution: false,
    }).collect() }
}
fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true") { return Err("native conformance is remote CI only".into()); }
    let backend = std::env::args().nth(1).ok_or("expected godot or blender")?;
    let cases: &[&str] = match backend.as_str() {
        "godot" => &["baseline","external-mutant","observation-mutant"],
        "blender" => &["baseline","external-mutant","membership-mutant"],
        _ => return Err("unknown native suite".into()),
    };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().and_then(|p|p.parent()).ok_or("repo root")?.to_owned();
    let output_dir = root.join("verification/effects"); std::fs::create_dir_all(&output_dir)?;
    let source_sha = std::env::var("EXPECTED_SHA")?;
    let mut receipts = Vec::new();
    for case in cases {
        // Pin the entire predicate/units/tolerance contract BEFORE native work.
        let rules = contract(&backend);
        let pinned = rules.digest()?;
        let mut command = Command::new("python3");
        command.arg(root.join("scripts/effects/native_probe.py")).arg(&backend).arg(case).env_clear();
        for name in ["PATH","GITHUB_ACTIONS","RUNNER_TEMP","GODOT_BIN","BLENDER_BIN"] {
            if let Ok(value) = std::env::var(name) { command.env(name,value); }
        }
        let outcome = command.output()?;
        if !outcome.status.success() { eprintln!("{}",String::from_utf8_lossy(&outcome.stderr)); return Err(format!("{backend}/{case} native process failed").into()); }
        if outcome.stdout.len() > MAX_PAYLOAD_BYTES { return Err("native measurement budget".into()); }
        let measured: NativeMeasurement = strict_decode(&outcome.stdout)?;
        ensure(measured.schema_version == 1 && measured.backend == backend && measured.case == *case, "native measurement identity")?;
        ensure(measured.writer_process != measured.reader_process, "fresh process not observed")?;
        ensure(if backend == "godot" { measured.runtime.starts_with("4.7.2") } else { measured.runtime.starts_with("4.5.14") }, "native runtime version differs from pinned workflow")?;
        let owner = Owner { session: format!("native-{backend}-{case}"), principal: PrincipalBinding::HostSession };
        let resource = rules.rules[0].address.resource.clone();
        let state = |revision| BaseStateSet(vec![BaseState { key: resource.clone(), document_id: "synthetic-fixture".into(),
            provider_session: measured.reader_process.clone(), generation: "native-invocation-1".into(),
            revision: Revision::Fingerprint(revision), concurrency: Concurrency::BestEffortRevalidate }]);
        let ctx = EvaluationContext { owner: owner.clone(), request_id: format!("{backend}-{case}"),
            plan_digest: canonical_digest(&(backend.as_str(),"native-workflow",&pinned))?, contract_digest: pinned,
            before: state(measured.before_digest.clone()), after: state(measured.after_digest.clone()),
            operations: BTreeSet::from(["native-workflow".into()]), observation_scope: rules.rules.iter().map(|r|r.address.clone()).collect(),
            execution_status: ExecutionStatus::Completed, support_level: SupportLevel::Native,
            budget: ConvergenceBudget { max_iterations: 1, max_operations: 32, max_findings: 32, max_observations: 32, max_elapsed_ms: 360000 } };
        let mut adapter = NativeAdapter { owner, session: measured.reader_process.clone(), measured: measured.values };
        let evidence = collect(&rules,&ctx,&mut adapter)?;
        let evaluation = evaluate(&rules,&ctx,&evidence)?;
        let expected = if *case == "baseline" { Verdict::Pass } else { Verdict::Fail };
        let verdict = evaluation.verdict()?;
        let expected_failure = match *case { "external-mutant" => Some("sentinel"), "observation-mutant" => Some("position"), "membership-mutant" => Some("membership"), _ => None };
        let negative_detected = expected_failure.is_none_or(|id| evaluation.report.validation.checks.iter().any(|r|r.rule==id && r.verdict==Verdict::Fail));
        receipts.push(json!({"case":case,"expected_verdict":expected,"actual_verdict":verdict,"negative_detected":negative_detected,
            "evaluation":evaluation,"runtime":measured.runtime,"isolation":measured.isolation,"limits":measured.limits,
            "route":measured.route,"crash_durability":measured.crash_durability,"source_sha":source_sha}));
        std::fs::write(output_dir.join(format!("{backend}-native.json")),serde_json::to_vec_pretty(&json!({"schema_version":1,"role":"F","source_sha":source_sha,"cases":receipts}))?)?;
        ensure(verdict == expected && negative_detected, "native evaluator verdict or targeted negative detection failed")?;
    }
    println!("executed {} native {backend} cases through effect evaluator",cases.len());
    Ok(())
}
