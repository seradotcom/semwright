#![no_main]

use libfuzzer_sys::fuzz_target;
use semwright_audio_domain::{edit::{self, Edit}, model::AudioProject};
use serde_json::Value;

fuzz_target!(|data: &[u8]| {
    if data.len() > 32_768 {
        return;
    }
    let Ok(value) = serde_json::from_slice::<Value>(data) else {
        return;
    };
    let Some(object) = value.as_object() else {
        return;
    };
    let (Some(project), Some(edit_value)) = (object.get("project"), object.get("edit")) else {
        return;
    };
    let Ok(project) = serde_json::from_value::<AudioProject>(project.clone()) else {
        return;
    };
    let Ok(edit) = serde_json::from_value::<Edit>(edit_value.clone()) else {
        return;
    };
    if project.validate().is_err() {
        return;
    }
    let Ok(revision) = project.semantic_digest() else {
        return;
    };
    if let Ok(outcome) = edit::apply(&project, &revision, edit, "fuzz-edit") {
        outcome.result.validate().expect("successful audio edit preserves model invariants");
        outcome.result.semantic_digest().expect("successful audio edit remains digestible");
        project.validate().expect("source audio project stays valid");
        assert_eq!(project.semantic_digest().unwrap(), revision);
    }
});
