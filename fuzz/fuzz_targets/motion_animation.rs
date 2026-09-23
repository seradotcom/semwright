#![no_main]
use libfuzzer_sys::fuzz_target;
use semwright_driver_motion_canvas::model::Animation;
fuzz_target!(|data: &[u8]| { if let Ok(value)=serde_json::from_slice::<Animation>(data) { let _=serde_json::to_vec(&value); } });
