#![no_main]

use libfuzzer_sys::fuzz_target;
use semwright_godot_driver::catalog::Catalog;
use serde_json::Value;
use std::cell::RefCell;

thread_local! {
    static CATALOG: RefCell<Catalog> =
        RefCell::new(Catalog::load().expect("Godot catalog must load"));
}

fuzz_target!(|data: &[u8]| {
    let Ok(value) = serde_json::from_slice::<Value>(data) else {
        return;
    };
    let Some(name) = value.get("name").and_then(Value::as_str) else {
        return;
    };
    let args = value.get("args").unwrap_or(&Value::Null);
    CATALOG.with(|catalog| {
        if let Ok(entry) = catalog.borrow().get(name) {
            let _ = entry.validate_input(args);
        }
    });
});
