#![no_main]
use libfuzzer_sys::fuzz_target;
use semwright_figma_driver::semantic_authoring::{
    FigmaChangeSetV1, FigmaCompositionSpecV1, FigmaPlanV1, MAX_COMPOSITION_BYTES,
};

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_COMPOSITION_BYTES {
        return;
    }
    if let Ok(spec) = serde_json::from_slice::<FigmaCompositionSpecV1>(data) {
        let _ = spec.validate();
    }
    if let Ok(changeset) = serde_json::from_slice::<FigmaChangeSetV1>(data) {
        let _ = changeset.validate();
    }
    if let Ok(plan) = serde_json::from_slice::<FigmaPlanV1>(data) {
        let _ = plan.verify();
    }
});
