//! exact comparison requires a private result of this process's reader.
//! A caller-constructible VerificationReport is not a freshness token.
use crate::effects_readback::{PrivateMeasurement, VerifiedRun};
use semwright_effect_conformance::composition::*;
use serde_json::Value;

pub fn require_exact_report(claim: &[u8], fresh: &VerifiedRun) -> Result<()> {
    let incoming: Value = strict_decode(claim)?;
    ensure(
        canonical_bytes(&incoming)? == canonical_bytes(&fresh.result().evaluation.report)?,
        "report differs from the independently evaluated snapshot",
    )
}
/// Includes protected predicates. Only the trusted specification owner should
/// receive this envelope. Public products must use an explicit separate view.
pub fn private_measurements(fresh: &VerifiedRun) -> &[PrivateMeasurement] {
    &fresh.result().private_measurements
}
