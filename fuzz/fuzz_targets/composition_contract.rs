#![no_main]
use libfuzzer_sys::fuzz_target;
use semwright_semantic_composition::{
    BaseStateSet, ConvergenceBudget, MAX_PAYLOAD_BYTES, ProfileDescriptor, ValidationReport,
    VerificationReport, canonical_bytes, strict_decode,
};
use serde_json::Value;

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_PAYLOAD_BYTES {
        return;
    }
    if let Ok(value) = strict_decode::<Value>(data) {
        let _ = canonical_bytes(&value);
    }
    if let Ok(base) = strict_decode::<BaseStateSet>(data) {
        let _ = base.validate();
        let _ = canonical_bytes(&base);
    }
    if let Ok(profile) = strict_decode::<ProfileDescriptor>(data) {
        let _ = profile.validate();
        let _ = canonical_bytes(&profile);
    }
    if let Ok(budget) = strict_decode::<ConvergenceBudget>(data) {
        let _ = budget.validate();
        let _ = canonical_bytes(&budget);
    }
    if let Ok(report) = strict_decode::<ValidationReport>(data) {
        let _ = report.verdict();
        let _ = canonical_bytes(&report);
    }
    if let Ok(report) = strict_decode::<VerificationReport>(data) {
        let _ = report.verdict();
        let _ = canonical_bytes(&report);
    }
});
