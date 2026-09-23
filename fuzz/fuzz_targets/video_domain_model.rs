#![no_main]

use libfuzzer_sys::fuzz_target;
use semwright_video_domain::model::Project;

fuzz_target!(|data: &[u8]| {
    if data.len() > 8_192 {
        return;
    }
    if let Ok(project) = serde_json::from_slice::<Project>(data) {
        let _ = project.validate();
        let _ = project.semantic_digest();

        if let Ok(encoded) = serde_json::to_vec(&project) {
            let reparsed: Project =
                serde_json::from_slice(&encoded).expect("serialized semantic project must parse");
            assert_eq!(reparsed, project);
        }
    }
});
