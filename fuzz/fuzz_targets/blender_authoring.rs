#![no_main]

use libfuzzer_sys::fuzz_target;
use semwright_blender_driver::authoring::{AuthoringIntent, BlenderAuthoringSpec, NativeSnapshot};
use semwright_semantic_composition::{Owner, PrincipalBinding, canonical_bytes, strict_decode};

const MAX: usize = 196_608;

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX {
        return;
    }
    if let Ok(spec) = strict_decode::<BlenderAuthoringSpec>(data) {
        if spec.validate().is_ok() {
            if let Ok(encoded) = canonical_bytes(&spec) {
                let _ = strict_decode::<BlenderAuthoringSpec>(&encoded);
            }
        }
    }
    if let Ok(intent) = strict_decode::<AuthoringIntent>(data) {
        match intent {
            AuthoringIntent::Create { spec } => {
                let _ = spec.validate();
            }
            AuthoringIntent::Transform {
                island,
                entity,
                transform,
                meters_per_unit,
                ..
            } => {
                let _ = (island, entity, meters_per_unit);
                let _ = transform.validate();
            }
        }
    }
    if let Ok(snapshot) = strict_decode::<NativeSnapshot>(data) {
        let owner = Owner {
            session: "fuzz-host-session".into(),
            principal: PrincipalBinding::HostSession,
        };
        let _ = snapshot.base(&owner);
    }
});
