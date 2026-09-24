#![no_main]
use libfuzzer_sys::fuzz_target;
use semwright_figma_driver::{model::MotionTimeline, motion};

fuzz_target!(|data: &[u8]| {
    if let Ok(timeline) = serde_json::from_slice::<MotionTimeline>(data) {
        let _ = motion::validate(&timeline);
    }
});
