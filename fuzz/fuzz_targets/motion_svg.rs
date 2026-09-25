#![no_main]
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| { if let Ok(s)=std::str::from_utf8(data) { let _=semwright_driver_motion_canvas::security::validate_svg(s); } });
