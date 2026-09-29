use semwright_audio_domain::support::AudioOperation;
use semwright_ardour_audio::driver::capability_catalog as ardour_catalog;
use semwright_faust_audio::{
    analysis_driver::capability_catalog as analysis_catalog,
    driver::capability_catalog as faust_catalog,
};
use serde_json::Value;
use std::collections::BTreeSet;

fn object(path: &str) -> Value {
    serde_json::from_str(path).expect("checked-in audio coverage JSON must parse")
}

#[test]
fn capability_matrix_matches_real_rust_catalogs_and_has_no_unclassified_entries() {
    let matrix = object(include_str!("../../../docs/audio/AUDIO_CAPABILITY_MATRIX.json"));
    assert_eq!(matrix["schema_version"], 1);
    let documented_ops: BTreeSet<_> = matrix["audio_operations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            assert_eq!(row["classified"], true, "{row}");
            assert_ne!(row["classification"], "unclassified", "{row}");
            row["id"].as_str().unwrap().to_owned()
        })
        .collect();
    let actual_ops: BTreeSet<_> = AudioOperation::ALL
        .iter()
        .map(|operation| operation.as_str().to_owned())
        .collect();
    assert_eq!(documented_ops, actual_ops);

    let documented_caps: BTreeSet<_> = matrix["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            assert_eq!(row["classified"], true, "{row}");
            for field in ["provider", "family", "support", "fidelity", "evidence"] {
                let value = row[field].as_str().unwrap_or("");
                assert!(!value.is_empty() && value != "unclassified", "{row}");
            }
            row["id"].as_str().unwrap().to_owned()
        })
        .collect();

    let actual_caps: BTreeSet<_> = faust_catalog()
        .into_iter()
        .chain(analysis_catalog())
        .chain(ardour_catalog())
        .map(|capability| capability.descriptor.name)
        .collect();
    assert_eq!(documented_caps, actual_caps);
}

#[test]
fn upstream_surface_coverage_never_uses_unclassified_as_a_completion_escape() {
    for source in [
        include_str!("../../../docs/audio/FAUST_SURFACE_COVERAGE.json"),
        include_str!("../../../docs/audio/ARDOUR_SURFACE_COVERAGE.json"),
    ] {
        let value = object(source);
        let rows = value["surfaces"].as_array().unwrap();
        assert!(!rows.is_empty());
        for row in rows {
            let status = row["status"].as_str().unwrap_or("");
            assert!(!status.is_empty());
            assert_ne!(status, "unclassified", "{row}");
            assert!(row.get("evidence").is_some(), "{row}");
        }
    }
}
