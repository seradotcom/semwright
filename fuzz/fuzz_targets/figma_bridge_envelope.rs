#![no_main]
use libfuzzer_sys::fuzz_target;
use semwright_figma_driver::bridge::parse_message;

fuzz_target!(|data: &[u8]| {
    let _ = parse_message(data);
});
