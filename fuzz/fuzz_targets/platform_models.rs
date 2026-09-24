#![no_main]
use libfuzzer_sys::fuzz_target;
use semwright_platform_api::{filesystem::validate_relative_path,model::Rect};
fuzz_target!(|data:&[u8]|{
    if let Ok(s)=std::str::from_utf8(data){let _=validate_relative_path(std::path::Path::new(s));}
    if data.len()>=40{
        let get=|i:usize|f64::from_le_bytes(data[i..i+8].try_into().unwrap());
        let rect=Rect{x:get(0),y:get(8),width:get(16),height:get(24)};
        let _=rect.pixels(Rect{x:-1920.,y:-1080.,width:5760.,height:2160.},get(32));
    }
});
