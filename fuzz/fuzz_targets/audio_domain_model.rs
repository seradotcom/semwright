#![no_main]

use libfuzzer_sys::fuzz_target;
use semwright_audio_domain::model::AudioProject;

fuzz_target!(|data: &[u8]| {
    if data.len() > 16_384 {
        return;
    }
    if let Ok(project) = serde_json::from_slice::<AudioProject>(data) {
        let _ = project.validate();
        let _ = project.semantic_digest();
        if let Ok(encoded) = serde_json::to_vec(&project) {
            let reparsed: AudioProject =
                serde_json::from_slice(&encoded).expect("serialized audio project must parse");
            assert_eq!(reparsed, project);
        }
    }
});
