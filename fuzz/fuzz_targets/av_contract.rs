#![no_main]
use libfuzzer_sys::fuzz_target;
use semwright_av_composition::{AvPlan, DecodedSyncProbe, SyncSpec, verify_sync};
use semwright_semantic_composition::{MAX_PAYLOAD_BYTES, canonical_bytes, strict_decode};

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_PAYLOAD_BYTES {
        return;
    }
    if let Ok(plan) = strict_decode::<AvPlan>(data) {
        let _ = plan.validate();
        let _ = canonical_bytes(&plan);
    }
    if let Ok(probe) = strict_decode::<DecodedSyncProbe>(data) {
        let _ = canonical_bytes(&probe);
    }
    if let Ok(spec) = strict_decode::<SyncSpec>(data) {
        let _ = spec.validate();
        let _ = canonical_bytes(&spec);
        if let Ok(probe) = strict_decode::<DecodedSyncProbe>(data) {
            let _ = verify_sync(&spec, &probe);
        }
    }
});
