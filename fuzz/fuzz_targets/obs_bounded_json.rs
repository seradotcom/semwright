#![no_main]
use libfuzzer_sys::fuzz_target;
use semwright_obs_driver::bounds;

fuzz_target!(|data: &[u8]| {
    if let Ok(value) = bounds::parse(data, bounds::MAX_FRAME) {
        let normalized = bounds::scrub(&value);
        let _ = bounds::check(&normalized, bounds::MAX_FRAME);
    }
});
