#![no_main]
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
if data.len()>65_536 { return; }
if let Ok(selector)=serde_json::from_slice::<semwright_types::Selector>(data) {let _=selector.select(&[]);}
});
