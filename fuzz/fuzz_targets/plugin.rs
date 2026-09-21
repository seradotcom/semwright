#![no_main]
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
if data.len()>1_048_576 { return; }
if let Ok(manifest)=serde_json::from_slice::<semwright_plugin_sdk::Manifest>(data) { let _=manifest.validate(); }
let _=serde_json::from_slice::<semwright_plugin_sdk::Request>(data);
});
