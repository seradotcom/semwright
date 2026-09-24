#![no_main]

use libfuzzer_sys::fuzz_target;
use semwright_godot_driver::bridge::{proof, transcript};

fuzz_target!(|data: &[u8]| {
    let split = data.len().min(64);
    let (secret, payload) = data.split_at(split);
    let text = String::from_utf8_lossy(payload);
    let transcript = transcript("client", &text, "00", "11", "22", "33");
    let _ = proof(secret, &transcript);
});
