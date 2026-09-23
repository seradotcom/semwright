#![no_main]
use libfuzzer_sys::fuzz_target;
use semwright_obs_driver::{
    bounds,
    capability::{Catalog, map_arguments},
};
use std::sync::LazyLock;

static CATALOG: LazyLock<Catalog> =
    LazyLock::new(|| Catalog::load().expect("embedded catalog must be valid"));

fuzz_target!(|data: &[u8]| {
    if let Ok(value) = bounds::parse(data, 65_536) {
        if let Some(command) = value["command"].as_str() {
            if let Ok(entry) = CATALOG.get(command) {
                if entry.input(&value["args"]).is_ok() {
                    let mapped =
                        map_arguments(&entry.plan, &value["args"]).expect("validated fields");
                    bounds::check(&mapped, 65_536).expect("mapping remains bounded");
                }
            }
        }
    }
});
