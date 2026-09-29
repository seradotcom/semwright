#![cfg(target_os = "linux")]
use semwright_effect_conformance::Predicate;
use semwright_godot_driver::authoring::native_observation::*;
use semwright_semantic_composition::strict_decode;
use std::{collections::BTreeSet, fs, path::Path};

fn load(root: &Path, name: &str) -> (NativeRequest, NativeObservation) {
    let request: NativeRequest =
        strict_decode(&fs::read(root.join(format!("request-{name}.json"))).unwrap()).unwrap();
    let actions: BTreeSet<String> = ["left", "right", "start", "restart"]
        .map(str::to_owned)
        .into();
    request.validate(&actions).unwrap();
    let observed = decode_observation(
        &fs::read(root.join(format!("observation-{name}.json"))).unwrap(),
        &request,
    )
    .unwrap();
    (request, observed)
}

#[test]
#[ignore = "requires product-authored project plus pinned Godot native observer receipts"]
fn product_authored_project_has_native_parse_reopen_animation_and_runtime_evidence() {
    let root =
        std::env::var_os("SEMWRIGHT_TEST_GODOT_NATIVE_EVIDENCE").expect("native evidence root");
    let root = Path::new(&root);
    let (inspect_request, inspect) = load(root, "inspect");
    assert!(inspect.dependency_complete);
    let native_root = inspect
        .authored
        .nodes
        .iter()
        .find(|node| node.path == ".")
        .unwrap();
    assert!(
        native_root
            .logical_id
            .as_deref()
            .is_some_and(|id| id.starts_with("asset_"))
    );

    let mut cursor = None;
    let mut observed_tracks = 0usize;
    let mut total = None;
    loop {
        let page = track_page(
            &inspect,
            &inspect_request.source_fingerprint,
            cursor.as_deref(),
            1,
        )
        .unwrap();
        total.get_or_insert(page.total as usize);
        observed_tracks += page.tracks.len();
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    assert!(total.unwrap_or_default() > 0);
    assert_eq!(observed_tracks, total.unwrap());

    let (_, saved) = load(root, "save");
    let (_, reopened) = load(root, "reopen");
    let persistence = persistence_value(&saved, &reopened).unwrap();
    assert_eq!(
        Predicate::Reopened.compare(&persistence).unwrap(),
        Some(true)
    );

    let (_, play) = load(root, "play");
    assert_eq!(play.inputs_delivered, 2);
    assert!(play.elapsed_physics_frames >= 10);
    assert_eq!(play.frames.len(), 3);
    let last = play.frames.last().unwrap();
    assert!(last.ticks.unwrap_or_default() >= 10);
    assert!(last.variables.contains_key("score"));
    assert!(play.live.is_some());
}
