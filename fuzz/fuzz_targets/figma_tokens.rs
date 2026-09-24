#![no_main]
use libfuzzer_sys::fuzz_target;
use semwright_figma_driver::design_system::parse_css_variables;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = parse_css_variables(text);
    }
});
