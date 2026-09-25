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
        let input = json!({
            "session": "a".repeat(32),
            "target": "Node",
            "properties": [{"name": "value", "value": value}],
            "expect": {"revision": 1, "fingerprint": "b".repeat(64)},
            "dry_run": false
        });
        let _ = patch.validate_input(&input);

        let resolve = catalog
            .get("driver.godot.ref.resolve")
            .expect("ref.resolve must exist");
        let ref_input = json!({
            "session": "a".repeat(32),
            "ref": value,
            "require_current": false
        });
        let _ = resolve.validate_input(&ref_input);
    });
});
