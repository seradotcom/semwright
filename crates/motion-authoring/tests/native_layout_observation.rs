//! Schema/validation regressions only. These inputs are not native evidence.
use semwright_motion_authoring::{FrameObservation, Known, NativeNodeType};
use serde_json::{Value, json};

fn legacy() -> Value {
    json!({
        "frame": 0,
        "subjects": [{
            "id": "label",
            "local_size": {"status": "known", "value": {"width": 777.0, "height": 333.0}},
            "bounds": {"status": "known", "value": {"x": 10.0, "y": 20.0, "width": 777.0, "height": 333.0}},
            "transform": {"status": "known", "value": [1.0, 0.0, 0.0, 1.0, 960.0, 540.0]},
            "opacity": {"status": "known", "value": 1.0},
            "drawn": true,
            "z_order": {"status": "known", "value": 0},
            "clip_active": {"status": "known", "value": false},
            "pixel_visibility": {"status": "unknown", "reason": "not-measured"},
            "text": null,
            "asset_ready": {"status": "known", "value": true}
        }],
        "transitions": [], "captions": [], "unresolved_cues": []
    })
}

fn observed() -> Value {
    let mut value = legacy();
    let subject = &mut value["subjects"][0];
    subject["native_type"] = json!({"status": "known", "value": "Txt"});
    subject["layout_root"] = json!({"status": "known", "value": false});
    subject["screen_layout_box"] = json!({"status": "known", "value": {
        "x": 860.0, "y": 520.0, "width": 200.0, "height": 40.0
    }});
    value
}

fn decode(value: Value) -> FrameObservation {
    serde_json::from_value(value).unwrap()
}

#[test]
fn native_layout_legacy_absence_stays_absent_without_inventing_evidence() {
    let original = legacy();
    let value = decode(original.clone());
    value.validate().unwrap();
    let subject = &value.subjects[0];
    assert!(subject.native_type.is_none());
    assert!(subject.layout_root.is_none());
    assert!(subject.screen_layout_box.is_none());
    assert_eq!(serde_json::to_value(value).unwrap(), original);
}

#[test]
fn native_layout_preserves_true_and_false_roots_and_separate_drawable_bounds() {
    for root in [false, true] {
        let mut input = observed();
        input["subjects"][0]["layout_root"]["value"] = json!(root);
        let value = decode(input.clone());
        value.validate().unwrap();
        let subject = &value.subjects[0];
        assert_eq!(
            subject.native_type.as_ref().unwrap().value(),
            Some(&NativeNodeType::Txt)
        );
        assert_eq!(subject.layout_root.as_ref().unwrap().value(), Some(&root));
        assert_eq!(subject.bounds.value().unwrap().width, 777.0);
        assert_eq!(
            subject
                .screen_layout_box
                .as_ref()
                .unwrap()
                .value()
                .unwrap()
                .width,
            200.0
        );
        assert_eq!(serde_json::to_value(value).unwrap(), input);
    }
}

#[test]
fn native_layout_unknown_values_remain_unknown() {
    let mut input = observed();
    for key in ["native_type", "layout_root", "screen_layout_box"] {
        input["subjects"][0][key] = json!({"status": "unknown", "reason": "not-observed"});
    }
    input["subjects"][0]["drawn"] = json!(false);
    let value = decode(input.clone());
    value.validate().unwrap();
    assert!(
        value.subjects[0]
            .screen_layout_box
            .as_ref()
            .unwrap()
            .value()
            .is_none()
    );
    assert_eq!(serde_json::to_value(value).unwrap(), input);
}

#[test]
fn native_layout_rejects_partial_extension_unknown_types_and_extra_fields() {
    for key in ["native_type", "layout_root", "screen_layout_box"] {
        let mut input = observed();
        input["subjects"][0].as_object_mut().unwrap().remove(key);
        assert!(decode(input).validate().is_err());
    }
    let mut input = observed();
    input["subjects"][0]["native_type"]["value"] = json!("AuthoredText");
    assert!(serde_json::from_value::<FrameObservation>(input).is_err());
    let mut input = observed();
    input["subjects"][0]["desired_layout_as_measured"] = json!(true);
    assert!(serde_json::from_value::<FrameObservation>(input).is_err());
    let mut input = observed();
    input["subjects"][0]["screen_layout_box"]["value"]["trusted"] = json!(true);
    assert!(serde_json::from_value::<FrameObservation>(input).is_err());
}

#[test]
fn native_layout_known_box_requires_actual_layout_draw_and_transform() {
    for change in 0..4 {
        let mut input = observed();
        let subject = &mut input["subjects"][0];
        match change {
            0 => subject["drawn"] = json!(false),
            1 => subject["native_type"]["value"] = json!("Camera"),
            2 => subject["transform"] = json!({"status": "unknown", "reason": "cache"}),
            _ => subject["layout_root"] = json!({"status": "unknown", "reason": "not-observed"}),
        }
        assert!(decode(input).validate().is_err(), "change={change}");
    }
}

#[test]
fn native_layout_observed_numbers_and_unknown_reasons_are_bounded() {
    for (field, number) in [("x", 1_000_001.0), ("width", -1.0), ("height", 1_000_001.0)] {
        let mut input = observed();
        input["subjects"][0]["screen_layout_box"]["value"][field] = json!(number);
        assert!(decode(input).validate().is_err());
    }
    let mut value = decode(observed());
    if let Some(Known::Known { value: bounds }) = &mut value.subjects[0].screen_layout_box {
        bounds.x = f64::INFINITY;
    }
    assert!(value.validate().is_err());
    for reason in [String::new(), "x".repeat(257)] {
        let mut input = observed();
        input["subjects"][0]["screen_layout_box"] = json!({"status": "unknown", "reason": reason});
        assert!(decode(input).validate().is_err());
    }
}
