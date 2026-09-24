#![no_main]

use libfuzzer_sys::fuzz_target;
use semwright_godot_driver::model::semantic_diff;
use serde_json::Value;

fuzz_target!(|data: &[u8]| {
    let Ok(value) = serde_json::from_slice::<Value>(data) else {
        return;
    };
    let before = value.get("before").unwrap_or(&value);
    let null = Value::Null;
    let after = value.get("after").unwrap_or(&null);
    let _ = semantic_diff(before, after);
});
