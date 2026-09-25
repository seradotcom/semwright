#![no_main]
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| { let _ = semwright_driver_motion_canvas::validate::parse(data); });
