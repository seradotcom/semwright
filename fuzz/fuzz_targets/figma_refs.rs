#![no_main]
use libfuzzer_sys::fuzz_target;
use semwright_figma_driver::model::NodeRef;

fuzz_target!(|data: &[u8]| {
    let _ = serde_json::from_slice::<NodeRef>(data);
});
