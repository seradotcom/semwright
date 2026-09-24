#![no_main]
use libfuzzer_sys::fuzz_target;
use semwright_workflow::{
    Candidate, WorkflowTrace, pattern_fingerprint, sanitize, validate_candidate_integrity,
};

fuzz_target!(|data: &[u8]| {
    if data.len() > 256 * 1024 {
        return;
    }
    if let Ok(trace) = serde_json::from_slice::<WorkflowTrace>(data) {
        if let Ok(value) = serde_json::to_value(&trace) {
            let _ = sanitize(&value, false);
            let _ = sanitize(&value, true);
        }
        let _ = pattern_fingerprint(&trace);
        let _ = serde_json::to_vec(&trace);
    }
    if let Ok(candidate) = serde_json::from_slice::<Candidate>(data) {
        let _ = validate_candidate_integrity(&candidate);
        let _ = serde_json::to_vec(&candidate);
    }
});
