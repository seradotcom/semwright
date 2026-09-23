#![no_main]

use libfuzzer_sys::fuzz_target;
use semwright_video_domain::render::{RenderCapability, RenderIntent};

fuzz_target!(|data: &[u8]| {
    if data.len() > 8_192 {
        return;
    }

    if let Ok(intent) = serde_json::from_slice::<RenderIntent>(data) {
        let _ = intent.preset.validate();
        if let Ok(encoded) = serde_json::to_vec(&intent) {
            let reparsed: RenderIntent =
                serde_json::from_slice(&encoded).expect("serialized render intent must parse");
            assert_eq!(reparsed, intent);
        }
    }

    if let Ok(capability) = serde_json::from_slice::<RenderCapability>(data) {
        let _ = capability.validate();
    }
});
