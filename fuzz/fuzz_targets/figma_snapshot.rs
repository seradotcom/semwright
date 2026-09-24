#![no_main]
use libfuzzer_sys::fuzz_target;
use semwright_figma_driver::snapshot::{canonicalize, SnapshotMode};
use serde_json::Value;

fuzz_target!(|data: &[u8]| {
    if let Ok(value) = serde_json::from_slice::<Value>(data) {
        let _ = canonicalize(&value, SnapshotMode::Identity);
        let _ = canonicalize(&value, SnapshotMode::Portable);
    }
});
