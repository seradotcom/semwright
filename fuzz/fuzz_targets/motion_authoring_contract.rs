#![no_main]
use libfuzzer_sys::fuzz_target;
use semwright_motion_authoring::{Film, TemporalGraph, realize};
use semwright_semantic_composition::{MAX_PAYLOAD_BYTES, canonical_bytes, strict_decode};

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_PAYLOAD_BYTES {
        return;
    }
    if let Ok(film) = strict_decode::<Film>(data) {
        let _ = film.validate();
        let _ = realize(&film);
        let _ = canonical_bytes(&film);
    }
    if let Ok(graph) = strict_decode::<TemporalGraph>(data) {
        let _ = canonical_bytes(&graph);
    }
});
