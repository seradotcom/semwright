#![no_main]
use libfuzzer_sys::fuzz_target;
use semwright_project_graph::*;
fuzz_target!(|data: &[u8]| {
    if data.len() > 65_536 {
        return;
    }
    if let Ok(receipt) = composition::strict_decode::<ExecutionReceipt>(data) {
        if receipt.validate().is_ok() {
            let bytes = composition::canonical_bytes(&receipt).unwrap();
            let decoded = composition::strict_decode::<ExecutionReceipt>(&bytes).unwrap();
            decoded.validate().unwrap();
            assert_eq!(
                composition::canonical_digest(&decoded).unwrap(),
                composition::canonical_digest(&receipt).unwrap()
            );
        }
    }
    // Parsing cannot produce AdmittedReceipt or acquire execution authority.
    let _ = composition::strict_decode::<Fingerprint>(data);
    let _ = composition::strict_decode::<DurableLocator>(data);
    let _ = composition::strict_decode::<RevisionRecord>(data);
    if let Ok(candidate) = composition::strict_decode::<RevisionCandidate>(data) {
        let _ = candidate.validate();
    }
    if let Ok(intent) = composition::strict_decode::<ExternalIntent>(data) {
        let _ = intent.validate();
    }
});
