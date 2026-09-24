#![no_main]

use libfuzzer_sys::fuzz_target;
use semwright_video_domain::backend::{BackendContract, ProjectionReport};

fuzz_target!(|data: &[u8]| {
    if data.len() > 8_192 {
        return;
    }

    if let Ok(contract) = serde_json::from_slice::<BackendContract>(data) {
        let _ = contract.validate();
        let encoded = serde_json::to_vec(&contract)
            .expect("serializable backend contract must encode");
        let reparsed: BackendContract = serde_json::from_slice(&encoded)
            .expect("serialized backend contract must parse");
        assert_eq!(reparsed, contract);
    }

    if let Ok(report) = serde_json::from_slice::<ProjectionReport>(data) {
        let _ = report.validate();
        let encoded = serde_json::to_vec(&report)
            .expect("serializable projection report must encode");
        let reparsed: ProjectionReport = serde_json::from_slice(&encoded)
            .expect("serialized projection report must parse");
        assert_eq!(reparsed, report);
    }
});
