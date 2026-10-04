//! canonical plan binding and claimed-only report inspection.
//! Deserializing a report does not establish fresh evidence or native authority.
use schemars::JsonSchema;
use semwright_effect_conformance::{EffectContract, EvaluationContext, composition::*};
use serde::Serialize;

pub fn validate_plan<I: Serialize, O: Serialize>(
    plan: &PreparedPlan<I, O>,
    profile: &ProfileDescriptor,
    contract: &EffectContract,
    context: &EvaluationContext,
) -> Result<()> {
    context.validate_plan(plan, profile, contract)
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClaimedReport {
    pub report: VerificationReport,
    pub claimed_verdict: Verdict,
    pub execution_authority: bool,
    pub requires_independent_reverification: bool,
}
/// This projection preserves an imported claim. It is never an admission proof.
pub fn inspect_claim(bytes: &[u8]) -> Result<ClaimedReport> {
    let report: VerificationReport = strict_decode(bytes)?;
    let claimed_verdict = report.verdict()?;
    Ok(ClaimedReport {
        report,
        claimed_verdict,
        execution_authority: false,
        requires_independent_reverification: true,
    })
}
