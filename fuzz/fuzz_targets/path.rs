#![no_main]
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
if data.len()>4096 {return;}
if let Ok(text)=std::str::from_utf8(data){let _=semwright_policy::validate_relative_path(std::path::Path::new(text));}
});
