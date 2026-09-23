#![no_main]
use libfuzzer_sys::fuzz_target;
use semwright_driver_motion_canvas::model::{Scene, Theme};

fuzz_target!(|data: &[u8]| {
    if let Ok((scene, theme)) = serde_json::from_slice::<(Scene, Theme)>(data) {
        let _ = semwright_driver_motion_canvas::validate::animations(&scene, &theme);
    }
});
