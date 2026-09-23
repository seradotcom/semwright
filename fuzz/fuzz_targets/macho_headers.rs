#![no_main]
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data:&[u8]|{let _=semwright_platform_macos_sys::macho::architectures(data);});
