use semwright_faust_audio::{
    analysis_driver::capability_catalog as analysis_catalog,
    driver::capability_catalog as faust_catalog,
};
use serde_json::Value;
use std::collections::BTreeSet;

fn matrix() -> Value {
    serde_json::from_str(include_str!(
        "../../../docs/audio/AUDIO_CAPABILITY_MATRIX.json"
    ))
    .expect("checked-in audio capability matrix must parse")
}

#[test]
fn faust_and_analysis_catalogs_match_matrix_exactly() {
    let value = matrix();
    let documented: BTreeSet<_> = value["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| {
            matches!(
                row["provider"].as_str(),
                Some("driver:faust-audio" | "driver:audio-analysis")
            )
        })
        .map(|row| {
            assert_eq!(row["classified"], true, "{row}");
            for field in ["support", "fidelity", "evidence"] {
                assert_ne!(row[field], "unclassified", "{row}");
            }
            row["id"].as_str().unwrap().to_owned()
        })
        .collect();

    let actual: BTreeSet<_> = faust_catalog()
        .into_iter()
        .chain(analysis_catalog())
        .map(|capability| capability.descriptor.name)
        .collect();
    assert_eq!(documented, actual);
}

#[test]
fn upstream_surface_matrices_have_no_unclassified_entries() {
    for source in [
        include_str!("../../../docs/audio/FAUST_SURFACE_COVERAGE.json"),
        include_str!("../../../docs/audio/ARDOUR_SURFACE_COVERAGE.json"),
    ] {
        let value: Value = serde_json::from_str(source).unwrap();
        for row in value["surfaces"].as_array().unwrap() {
            let status = row["status"].as_str().unwrap_or("");
            assert!(!status.is_empty(), "{row}");
            assert_ne!(status, "unclassified", "{row}");
            assert!(row.get("evidence").is_some(), "{row}");
        }
    }
}
