use crate::*;
use schemars::JsonSchema;
use semwright_semantic_composition::*;
use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct RuleCoverage {
    pub rule: String,
    pub required: bool,
    pub sufficient: bool,
    pub reasons: Vec<String>,
    pub attribution: Option<Attribution>,
}
/// Metadata plus A's unmodified VerificationReport, not a new report hierarchy.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct EffectEvaluation {
    pub report: VerificationReport,
    pub contract_digest: Digest,
    pub owner: Owner,
    pub request_id: String,
    pub coverage: Vec<RuleCoverage>,
    pub vacuous: bool,
}
impl EffectEvaluation {
    pub fn verdict(&self) -> Result<Verdict> {
        if self.vacuous {
            Ok(Verdict::Unknown)
        } else {
            self.report.verdict()
        }
    }
}
fn guard(
    rule: &EffectRule,
    context: &EvaluationContext,
    identity: Option<&AdapterIdentity>,
    o: &AdapterObservation,
) -> Result<Vec<String>> {
    let mut reasons = Vec::new();
    let binding = &o.binding;
    if binding.owner != context.owner
        || binding.request_id != context.request_id
        || binding.operation_id != rule.operation_id
        || binding.plan_digest != context.plan_digest
        || binding.contract_digest != context.contract_digest
    {
        reasons.push("receipt owner/request/operation/plan/contract mismatch".into());
    }
    let state = context
        .after
        .0
        .iter()
        .find(|s| s.key == rule.address.resource);
    match (identity, state) {
        (Some(producer), Some(base))
            if producer.owner == context.owner
                && producer.provider == base.key.provider
                && producer.provider_session == base.provider_session
                && producer.generation == base.generation => {}
        _ => reasons.push("untrusted or substituted observer channel".into()),
    }
    if !context.operations.contains(&rule.operation_id)
        || !context.observation_scope.contains(&rule.address)
    {
        reasons.push("rule outside trusted operation/scope".into());
    }
    if o.observation.base != context.after
        || context.after.check_fresh(&context.after, false).is_err()
    {
        reasons.push("receipt post-state/freshness mismatch or unknown revision".into());
    }
    if o.observation.method != rule.method.name
        || o.observation.method_version != rule.method.version
        || o.observation.source != rule.method.source
        || o.observation.artifact != rule.artifact
    {
        reasons.push("method/version/source/artifact mismatch".into());
    }
    if matches!(
        o.observation.source,
        EvidenceSource::Fixture | EvidenceSource::Simulation | EvidenceSource::HumanReview
    ) {
        reasons
            .push("non-native or non-deterministic evidence does not verify native effects".into());
    }
    if matches!(
        context.support_level,
        SupportLevel::Unsupported
            | SupportLevel::SecurityExcluded
            | SupportLevel::UpstreamRestricted
    ) {
        reasons.push("workflow support does not substantiate a verified outcome".into());
    }
    if bounded_id(&o.observation.id).is_err()
        || o.observation.scope.len() > 4096
        || o.coverage.missing.len() > 4096
        || !o.observation.scope.contains(&rule.address)
        || o.observation.scope.iter().collect::<BTreeSet<_>>().len() != o.observation.scope.len()
        || !o
            .observation
            .scope
            .iter()
            .all(|a| context.observation_scope.contains(a))
    {
        reasons.push("observation scope missing, duplicated, oversized or substituted".into());
    }
    if !o.observation.exhaustive
        || !o.coverage.consistent
        || o.coverage.missing.contains(&rule.address)
    {
        reasons.push("incomplete or inconsistent observation".into());
    }
    if o.readback != ReadbackState::Observed {
        reasons.push(format!("not independent readback: {:?}", o.readback));
    }
    if o.value.is_none() {
        reasons.push("missing observed value".into());
    }
    if rule.require_causal_attribution
        && matches!(
            o.coverage.attribution,
            Attribution::Concurrent | Attribution::Ambiguous
        )
    {
        reasons.push("causal attribution is concurrent or ambiguous".into());
    }
    if rule.predicate.needs_complete_universe() {
        match &o.coverage.enumeration {
            None => reasons.push("no enumeration transcript for global predicate".into()),
            Some(pages) => {
                let expected = context.enumeration_binding(rule)?;
                let audit = audit_enumeration(&expected, pages);
                if audit.verdict != Verdict::Pass {
                    reasons.extend(audit.reasons);
                }
                if !matches!(&o.value, Some(ObservedValue::Members { values }) if values == &audit.members)
                {
                    reasons.push("observed collection differs from enumerated members".into());
                }
            }
        }
    }
    Ok(reasons)
}
pub fn evaluate(
    contract: &EffectContract,
    context: &EvaluationContext,
    batch: &EvidenceBatch,
) -> Result<EffectEvaluation> {
    contract.validate()?;
    context.validate()?;
    ensure(
        batch.contract_digest == contract.digest()?
            && context.contract_digest == batch.contract_digest,
        "cannot remove/alter rules after evidence collection",
    )?;
    ensure(
        batch.context_digest == canonical_digest(context)?,
        "cannot replay evidence under another invocation",
    )?;
    ensure(
        batch.observations.len() == contract.rules.len(),
        "collected rule count mismatch",
    )?;
    let mut checks = Vec::new();
    let mut coverage = Vec::new();
    let mut observed = BTreeSet::new();
    let mut unobservable = BTreeSet::new();
    for (rule, collected) in contract.rules.iter().zip(&batch.observations) {
        ensure(rule.id == collected.rule_id, "collected rule substitution")?;
        let mut reasons = Vec::new();
        let mut evidence = Vec::new();
        let mut verdict = Verdict::Unknown;
        let mut attribution = None;
        match &collected.result {
            Err(error) => reasons.push(error.to_string()),
            Ok(o) => {
                reasons.extend(guard(rule, context, collected.identity.as_ref(), o)?);
                attribution = Some(o.coverage.attribution);
                if reasons.is_empty() {
                    match o.value.as_ref().map(|value| rule.predicate.compare(value)) {
                        Some(Ok(Some(true))) => verdict = Verdict::Pass,
                        Some(Ok(Some(false))) => {
                            verdict = Verdict::Fail;
                        }
                        Some(Ok(None)) => {
                            reasons.push("predicate/value type or units mismatch".into())
                        }
                        Some(Err(error)) => reasons.push(error.to_string()),
                        None => reasons.push("no observation".into()),
                    }
                    if verdict != Verdict::Unknown {
                        // Preserve the validated observation scope. Shrinking it to only the
                        // rule address would erase provenance about additional resources or
                        // properties that the trusted observer actually read. `guard` already
                        // proved this scope is unique, bounded, within the trusted invocation
                        // scope, exhaustive, and contains the rule address.
                        evidence.push(o.observation.clone());
                        observed.insert(rule.address.clone());
                    }
                }
            }
        }
        let sufficient = verdict != Verdict::Unknown;
        if !sufficient {
            unobservable.insert(rule.address.clone());
        }
        if verdict == Verdict::Fail {
            reasons.push("observed predicate is false".into());
        }
        checks.push(RuleResult {
            rule: rule.id.clone(),
            version: rule.version,
            verdict,
            evidence_class: EvidenceClass::Deterministic,
            evidence,
            reason: if reasons.is_empty() {
                None
            } else {
                Some(reasons.join("; "))
            },
        });
        coverage.push(RuleCoverage {
            rule: rule.id.clone(),
            required: rule.obligation.required(),
            sufficient,
            reasons,
            attribution,
        });
    }
    let required_rules = contract.required_rules();
    Ok(EffectEvaluation {
        report: VerificationReport {
            execution_status: context.execution_status,
            validation: ValidationReport {
                plan_digest: context.plan_digest.clone(),
                base: context.after.clone(),
                required_rules: required_rules.clone(),
                checks,
            },
            support_level: context.support_level,
            effects_observed: observed.into_iter().collect(),
            effects_unobservable: unobservable.into_iter().collect(),
        },
        contract_digest: context.contract_digest.clone(),
        owner: context.owner.clone(),
        request_id: context.request_id.clone(),
        coverage,
        vacuous: required_rules.is_empty(),
    })
}
