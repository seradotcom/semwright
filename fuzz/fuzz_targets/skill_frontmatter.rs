#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = semwright_skills::parse_skill_text("fuzz-skill", data);
});
