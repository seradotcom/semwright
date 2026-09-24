#![no_main]

use libfuzzer_sys::fuzz_target;
use semwright_godot_driver::Config;

fuzz_target!(|data: &[u8]| {
    let _ = serde_json::from_slice::<Config>(data);
});
