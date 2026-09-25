#![no_main]

use libfuzzer_sys::fuzz_target;
use semwright_godot_driver::catalog::Catalog;
use serde_json::{Value, json};
use std::cell::RefCell;

thread_local! {
    static CATALOG: RefCell<Catalog> =
        RefCell::new(Catalog::load().expect("Godot catalog must load"));
}

fuzz_target!(|data: &[u8]| {
    let Ok(value) = serde_json::from_slice::<Value>(data) else {
        return;
    };
    CATALOG.with(|catalog| {
        let catalog = catalog.borrow();

        let patch = catalog
            .get("driver.godot.node.patch")
            .expect("node.patch must exist");
        let patch_input = json!({
            "session": "a".repeat(32),
            "target": "Node",
            "properties": [{"name": "value", "value": value.clone()}],
            "expect": {"revision": 1, "fingerprint": "b".repeat(64)},
            "dry_run": false
        });
        let _ = patch.validate_input(&patch_input);

        let describe = catalog
            .get("driver.godot.api.describe")
            .expect("api.describe must exist");
        let describe_input = json!({
            "session": "a".repeat(32),
            "class": value.as_str().unwrap_or("Node"),
            "include_inherited": true,
            "properties": true,
            "methods": true,
            "signals": true,
            "enums": true
        });
        let _ = describe.validate_input(&describe_input);

        let project_class = catalog
            .get("driver.godot.project.class.describe")
            .expect("project.class.describe must exist");
        let class_input = json!({
            "session": "a".repeat(32),
            "name": value.as_str().unwrap_or("SemanticClass")
        });
        let _ = project_class.validate_input(&class_input);

        let resolve = catalog
            .get("driver.godot.ref.resolve")
            .expect("ref.resolve must exist");
        let resolve_input = json!({
            "session": "a".repeat(32),
            "ref": value.clone(),
            "require_current": false
        });
        let _ = resolve.validate_input(&resolve_input);
    });
});
