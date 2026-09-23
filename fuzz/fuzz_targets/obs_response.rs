#![no_main]
use libfuzzer_sys::fuzz_target;
use semwright_obs_driver::{bounds, protocol};

fuzz_target!(|data: &[u8]| {
    if let Ok(value) = bounds::parse(data, bounds::MAX_FRAME) {
        let _ = protocol::status(&value);
    }
});
