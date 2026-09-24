#![no_main]
use libfuzzer_sys::fuzz_target;
use semwright_workflow::{sanitize, WorkflowTrace};

fuzz_target!(|data: &[u8]| {
    if data.len() > 256 * 1024 {
        return;
    }
    if let Ok(trace) = serde_json::from_slice::<WorkflowTrace>(data) {
        if let Ok(value) = serde_json::to_value(&trace) {
            let _ = sanitize(&value, false);
            let _ = sanitize(&value, true);
        }
        let _ = serde_json::to_vec(&trace);
    }
});
