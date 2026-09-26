#![no_main]
use libfuzzer_sys::fuzz_target;
use semwright_driver_motion_canvas::model::{NodeKind, SemanticValue, Theme};

fuzz_target!(|data: &[u8]| {
    if let Ok((kind, name, value, theme)) =
        serde_json::from_slice::<(NodeKind, String, SemanticValue, Theme)>(data)
    {
        let _ = semwright_driver_motion_canvas::semantic::property(kind, &name);
        let _ = semwright_driver_motion_canvas::semantic::validate_value(
            kind, &name, &value, &theme,
        );
        let _ = semwright_driver_motion_canvas::semantic::emitted_value(
            kind, &name, &value, &theme,
        );
    }
});
