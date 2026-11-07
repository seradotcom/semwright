//! Closed wire-schema regressions. Synthetic inputs here are never Native evidence.
use semwright_motion_authoring::FrameObservation;
use serde_json::{Value, json};

fn unknown() -> Value {
    json!({"status":"unknown","reason":"not-observed"})
}
fn known(value: Value) -> Value {
    json!({"status":"known","value":value})
}
fn frame() -> Value {
    json!({"frame":0,"subjects":[{
        "id":"text","native_type":known(json!("Txt")),"layout_root":known(json!(true)),
        "screen_layout_box":known(json!({"x":10.0,"y":20.0,"width":40.0,"height":30.0})),
        "local_size":known(json!({"width":40.0,"height":30.0})),
        "bounds":known(json!({"x":10.0,"y":20.0,"width":40.0,"height":30.0})),
        "transform":known(json!([1.0,0.0,0.0,1.0,30.0,35.0])),
        "opacity":known(json!(1.0)),"drawn":true,"z_order":known(json!(0)),
        "clip_active":known(json!(false)),"pixel_visibility":unknown(),"text":null,
        "asset_ready":known(json!(true))}],"transitions":[],"captions":[],"unresolved_cues":[]})
}
fn node(key: &str, id: Option<&str>, kind: &str, parent: Option<&str>) -> Value {
    json!({"native_key":key,"subject_id":id,"native_type":kind,"parent_key":parent,
        "local_to_parent":[1.0,0.0,0.0,1.0,0.0,0.0],"opacity":1.0,
        "layout_enabled":if kind=="Node"||kind=="Camera" {unknown()} else {known(json!(false))},
        "clip":if kind=="Node"||kind=="Camera" {unknown()} else {known(json!(false))}})
}
fn geometry() -> Value {
    let mut value = frame();
    value["subjects"][0]["native_geometry"] = known(json!({"native_key":"text/key",
        "screen_layout_polygon":known(json!([[10.0,20.0],[50.0,20.0],[50.0,50.0],[10.0,50.0]])),
        "lineage":known(json!([node("text/key",Some("text"),"Txt",Some("wrapper/key")),
            node("wrapper/key",None,"Layout",Some("scene/key")),node("scene/key",None,"Node",None)])),
        "own_clip":unknown()}));
    value["subjects"][0]["camera_scene"] = unknown();
    value
}
fn clipped() -> Value {
    let mut value = geometry();
    let row = &mut value["subjects"][0];
    row["native_type"] = known(json!("Layout"));
    let geo = &mut row["native_geometry"]["value"];
    geo["lineage"]["value"][0]["native_type"] = json!("Layout");
    geo["lineage"]["value"][0]["clip"] = known(json!(true));
    geo["own_clip"] = known(json!({"kind":"rect","clip":{"native_key":"text/key",
        "local_box":{"x":-20.0,"y":-15.0,"width":40.0,"height":30.0},
        "matrix":[1.0,0.0,0.0,1.0,30.0,35.0],
        "screen_polygon":[[10.0,20.0],[50.0,20.0],[50.0,50.0],[10.0,50.0]]}}));
    value
}
fn camera() -> Value {
    let mut value = frame();
    let row = &mut value["subjects"][0];
    row["id"] = json!("camera");
    row["native_type"] = known(json!("Camera"));
    row["layout_root"] = unknown();
    row["screen_layout_box"] = unknown();
    row["native_geometry"] = known(
        json!({"native_key":"camera/key","screen_layout_polygon":unknown(),
        "lineage":known(json!([node("camera/key",Some("camera"),"Camera",None)])),"own_clip":unknown()}),
    );
    row["camera_scene"] = known(
        json!({"native_key":"camera/key","original_scene_key":"scene/key",
        "current_scene_key":"scene/key","current_scene_is_original":true,"draw_scene_key":"scene/key",
        "draw_uses_original_scene":true,"scene_transform":known(json!([1.0,0.0,0.0,1.0,960.0,540.0])),
        "position":[0.0,0.0],"rotation":0.0,"zoom":1.0,
        "cameras":known(json!([{"native_key":"camera/key","subject_id":"camera"}]))}),
    );
    value
}
fn accepted(value: &Value) -> FrameObservation {
    let frame: FrameObservation = serde_json::from_value(value.clone()).unwrap();
    frame.validate().unwrap();
    frame
}
fn rejected(value: Value) {
    if let Ok(frame) = serde_json::from_value::<FrameObservation>(value) {
        assert!(frame.validate().is_err());
    }
}

#[test]
fn geometry_absence_unknown_and_native_facts_roundtrip_without_acceptance() {
    for value in [frame(), geometry(), clipped(), camera()] {
        assert_eq!(serde_json::to_value(accepted(&value)).unwrap(), value);
    }
    let mut value = frame();
    value["subjects"][0]["native_geometry"] = unknown();
    value["subjects"][0]["camera_scene"] = unknown();
    let parsed = accepted(&value);
    assert!(
        parsed.subjects[0]
            .native_geometry
            .as_ref()
            .unwrap()
            .value()
            .is_none()
    );
    assert_eq!(serde_json::to_value(parsed).unwrap(), value);
    let mut value = clipped();
    value["subjects"][0]["native_geometry"]["value"]["lineage"] = unknown();
    accepted(&value); // unknown chain does not erase an independently observed clip
}

#[test]
fn geometry_closed_shapes_and_pair_presence_reject_ambiguous_wire_data() {
    for key in ["native_geometry", "camera_scene"] {
        let mut value = geometry();
        value["subjects"][0].as_object_mut().unwrap().remove(key);
        rejected(value);
    }
    let mut value = geometry();
    value["subjects"][0]["native_geometry"]["value"]["expected_as_observed"] = json!(true);
    rejected(value);
    for key in ["subject_id", "parent_key"] {
        let mut value = geometry();
        value["subjects"][0]["native_geometry"]["value"]["lineage"]["value"][0]
            .as_object_mut()
            .unwrap()
            .remove(key);
        rejected(value);
    }
    let mut value = clipped();
    value["subjects"][0]["native_geometry"]["value"]["own_clip"]["value"]["kind"] = json!("circle");
    rejected(value);
    let mut value = clipped();
    value["subjects"][0]["native_geometry"]["value"]["own_clip"] = json!({"status":"known"});
    rejected(value);
    let mut value = geometry();
    value["subjects"][0]["native_geometry"] = json!({"status":"unknown","reason":""});
    rejected(value);
}

#[test]
fn geometry_rejects_foreign_clip_owner_cycles_incomplete_edges_and_numbers() {
    let mut value = clipped();
    value["subjects"][0]["native_geometry"]["value"]["own_clip"]["value"]["clip"]["native_key"] =
        json!("child/key");
    rejected(value);
    let mut value = clipped();
    value["subjects"][0]["native_type"] = known(json!("Txt"));
    rejected(value);
    let mut value = geometry();
    value["subjects"][0]["native_geometry"]["value"]["lineage"]["value"][1]["native_key"] =
        json!("text/key");
    rejected(value);
    let mut value = geometry();
    value["subjects"][0]["native_geometry"]["value"]["lineage"]["value"][0]["parent_key"] =
        json!(null);
    rejected(value);
    let mut value = geometry();
    value["subjects"][0]["native_geometry"]["value"]["lineage"]["value"][0]["subject_id"] =
        json!("foreign");
    rejected(value);
    let mut value = geometry();
    value["subjects"][0]["native_geometry"]["value"]["screen_layout_polygon"]["value"][0][0] =
        json!(2_000_001.0);
    rejected(value);
    let mut value = geometry();
    value["subjects"][0]["native_geometry"]["value"]["lineage"]["value"][0]["local_to_parent"][0] =
        json!(1_000_001.0);
    rejected(value);
    let mut value = geometry();
    value["subjects"][0]["native_geometry"]["value"]["lineage"]["value"][0]["native_key"] =
        json!("x".repeat(513));
    rejected(value);
}

#[test]
fn polygon_requires_real_draw_mapping_and_the_same_separate_aabb() {
    for change in 0..4 {
        let mut value = geometry();
        let row = &mut value["subjects"][0];
        match change {
            0 => row["drawn"] = json!(false),
            1 => row["transform"] = unknown(),
            2 => {
                row["native_geometry"]["value"]["screen_layout_polygon"]["value"][0][0] = json!(9.0)
            }
            _ => row["screen_layout_box"] = unknown(),
        };
        rejected(value);
    }
    let mut value = geometry();
    value["subjects"][0]["screen_layout_box"] = unknown();
    value["subjects"][0]["native_geometry"]["value"]["screen_layout_polygon"] = unknown();
    accepted(&value); // a cache remains explicitly unmeasured
}

#[test]
fn camera_replacement_duplicate_inventory_and_unmapped_draw_remain_real_negatives() {
    let mut value = camera();
    let camera = &mut value["subjects"][0]["camera_scene"]["value"];
    camera["current_scene_key"] = json!("replacement/key");
    camera["draw_scene_key"] = json!("replacement/key");
    camera["current_scene_is_original"] = json!(false);
    camera["draw_uses_original_scene"] = json!(false);
    camera["cameras"]["value"]
        .as_array_mut()
        .unwrap()
        .push(json!({"native_key":"second-camera/key","subject_id":null}));
    camera["scene_transform"] = unknown();
    let parsed = accepted(&value);
    let actual = parsed.subjects[0]
        .camera_scene
        .as_ref()
        .unwrap()
        .value()
        .unwrap();
    assert!(!actual.current_scene_is_original && !actual.draw_uses_original_scene);
    assert_eq!(actual.cameras.value().unwrap().len(), 2);
    assert!(actual.scene_transform.value().is_none());
    assert_eq!(serde_json::to_value(parsed).unwrap(), value);
}

#[test]
fn camera_rejects_fabricated_identity_type_inventory_and_unbounded_signals() {
    for change in 0..7 {
        let mut value = camera();
        let row = &mut value["subjects"][0];
        match change {
            0 => row["native_type"] = known(json!("Node")),
            1 => row["camera_scene"]["value"]["native_key"] = json!("other-camera/key"),
            2 => row["camera_scene"]["value"]["current_scene_key"] = json!("replacement/key"),
            3 => row["camera_scene"]["value"]["draw_scene_key"] = json!("replacement/key"),
            4 => row["camera_scene"]["value"]["zoom"] = json!(0.0),
            5 => row["camera_scene"]["value"]["scene_transform"]["value"][0] = json!(1_000_001.0),
            _ => row["camera_scene"]["value"]["cameras"]["value"]
                .as_array_mut()
                .unwrap()
                .push(json!({"native_key":"camera/key","subject_id":null})),
        };
        rejected(value);
    }
    let mut value = camera();
    value["subjects"][0]["camera_scene"]["value"]["cameras"]["value"][0]
        .as_object_mut()
        .unwrap()
        .remove("subject_id");
    rejected(value);
}

#[test]
#[ignore = "requires explicit retained real DOM originals from the focal job"]
fn actual_dom_wire_originals_deserialize_and_validate_without_synthesizing_fields() {
    let root = std::path::PathBuf::from(
        std::env::var("SEMWRIGHT_NATIVE_GEOMETRY_DOM_ORIGINALS")
            .expect("explicit original directory"),
    );
    let cases =
        std::env::var("SEMWRIGHT_NATIVE_GEOMETRY_DOM_CASES").expect("explicit selected cases");
    let cases: Vec<&str> = cases.split(',').collect();
    assert!(!cases.is_empty() && cases.len() <= 16);
    for case in cases {
        assert!(case.bytes().all(|b| b.is_ascii_lowercase() || b == b'-'));
        let path = root.join(format!("{case}.captured.original.json"));
        let metadata = std::fs::symlink_metadata(&path).unwrap();
        assert!(
            metadata.is_file()
                && !metadata.file_type().is_symlink()
                && metadata.len() <= 2 * 1024 * 1024
        );
        let raw = std::fs::read(&path).unwrap();
        let value: Value = serde_json::from_slice(&raw).unwrap();
        let rows = value["original_native_results"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        for row in rows {
            let observation: FrameObservation =
                serde_json::from_value(row["probe"].clone()).unwrap();
            observation.validate().unwrap();
            assert!(!observation.subjects.is_empty());
            if row["mode"] == "observed" {
                assert!(
                    observation
                        .subjects
                        .iter()
                        .all(|s| s.native_geometry.is_some() && s.camera_scene.is_some())
                );
            }
        }
        assert_eq!(
            std::fs::read(&path).unwrap(),
            raw,
            "original changed during ABI readback"
        );
    }
}
