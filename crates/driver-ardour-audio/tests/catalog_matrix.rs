use semwright_audio_domain::support::AudioOperation;
use semwright_ardour_audio::driver::capability_catalog;
use serde_json::Value;
use std::collections::BTreeSet;

fn matrix() -> Value {
    serde_json::from_str(include_str!(
        "../../../docs/audio/AUDIO_CAPABILITY_MATRIX.json"
    ))
    .expect("checked-in audio capability matrix must parse")
}

#[test]
fn shared_operation_inventory_is_exact_and_classified() {
    let value = matrix();
    let documented: BTreeSet<_> = value["audio_operations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            assert_eq!(row["classified"], true, "{row}");
            row["id"].as_str().unwrap().to_owned()
        })
        .collect();
    let actual: BTreeSet<_> = AudioOperation::ALL
        .iter()
        .map(|operation| operation.as_str().to_owned())
        .collect();
    assert_eq!(documented, actual);
}

#[test]
fn ardour_catalog_matches_its_matrix_rows_exactly() {
    let value = matrix();
    let documented: BTreeSet<_> = value["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["provider"] == "driver:ardour-audio")
        .map(|row| {
            assert_eq!(row["classified"], true, "{row}");
            for field in ["support", "fidelity", "evidence"] {
                assert_ne!(row[field], "unclassified", "{row}");
            }
            row["id"].as_str().unwrap().to_owned()
        })
        .collect();
    let actual: BTreeSet<_> = capability_catalog()
        .into_iter()
        .map(|capability| capability.descriptor.name)
        .collect();
    assert_eq!(documented, actual);
}
