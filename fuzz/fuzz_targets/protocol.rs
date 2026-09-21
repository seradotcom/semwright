#![no_main]
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    if data.len()>semwright_types::MAX_FRAME+4 { return; }
    let _=semwright_protocol::decode::<semwright_protocol::ClientMessage>(data);
    let _=serde_json::from_slice::<semwright_protocol::ClientMessage>(data);
});
