#![no_main]
use libfuzzer_sys::fuzz_target;
use semwright_media_time::{CueGraph, Rate, Rational, TimeMap};
use semwright_semantic_composition::{MAX_PAYLOAD_BYTES, canonical_bytes, strict_decode};

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_PAYLOAD_BYTES {
        return;
    }
    if let Ok(value) = strict_decode::<Rational>(data) {
        let _ = value.validate();
        let _ = canonical_bytes(&value);
    }
    if let Ok(value) = strict_decode::<Rate>(data) {
        let _ = value.validate();
        let _ = canonical_bytes(&value);
    }
    if let Ok(value) = strict_decode::<CueGraph>(data) {
        let _ = value.resolve();
        let _ = value.digest();
        let _ = canonical_bytes(&value);
    }
    if let Ok(value) = strict_decode::<TimeMap>(data) {
        let _ = value.validate();
        let _ = canonical_bytes(&value);
    }
});
