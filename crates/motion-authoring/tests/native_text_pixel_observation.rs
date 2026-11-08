//! Closed ABI regressions only. Synthetic records never constitute Native evidence.
use semwright_motion_authoring::FrameObservation;
use semwright_semantic_composition::Digest;
use serde_json::{Value, json};

fn known(value: Value) -> Value {
    json!({"status":"known","value":value})
}
fn unknown() -> Value {
    json!({"status":"unknown","reason":"not-observed"})
}
fn frame() -> Value {
    let text = Digest::of_bytes(b"AB");
    // Keep independent JSON fixture records below the macro expansion limit.
    let text_pixels = json!({"native_key":"text/key","frame":41,"width":1920,"height":1080,
            "playback_fps":30.0,"fps_num":30,"fps_den":1,"selected_frames":[0,41,179],
            "render_input_digest":"ab".repeat(32),"output_background":"#101020",
            "calls":[{"serial":0,"operation":"fill_text","matrix":[1.0,0.0,0.0,1.0,960.0,540.0],
                "text":"AB","text_digest":text,"font":"normal 400 36px Instrument Sans Variable",
                "color":[255,255,255],"x":10.0,"y":20.0,"max_width":null,"changed_pixels":2,
                "global_alpha":1.0,"composite":"source-over","filter":"none"}],
            // [y=20,x=10,len=2], little endian, exactly one canonical six-byte run.
            "mask":{"encoding":"row-runs-u16le-base64","data":"FAAKAAIA","runs":1,"changed_pixels":2}});
    json!({"frame":41,"subjects":[{
        "id":"text","native_type":known(json!("Txt")),"layout_root":unknown(),
        "screen_layout_box":unknown(),"native_geometry":known(json!({
            "native_key":"text/key","screen_layout_polygon":unknown(),"lineage":unknown(),"own_clip":unknown()})),
        "camera_scene":unknown(),"local_size":unknown(),"bounds":unknown(),"transform":unknown(),
        "opacity":known(json!(1.0)),"drawn":true,"z_order":known(json!(0)),"clip_active":unknown(),
        "pixel_visibility":unknown(),"asset_ready":known(json!(true)),
        "text":{"logical_text_digest":text,"observed_text_digest":text,"truncated":unknown(),
            "lines":known(json!(1)),"font_family":known(json!("Instrument Sans Variable")),
            "font_ready":known(json!(true)),"fallback_used":known(json!(false)),"direction":known(json!("ltr"))},
        "native_text_pixels":known(text_pixels)
        }],"transitions":[],"captions":[],"unresolved_cues":[]})
}
fn accepted(value: &Value) -> FrameObservation {
    let parsed: FrameObservation = serde_json::from_value(value.clone()).unwrap();
    parsed.validate().unwrap();
    parsed
}
fn rejected(value: Value) {
    if let Ok(parsed) = serde_json::from_value::<FrameObservation>(value) {
        assert!(parsed.validate().is_err());
    }
}
fn pixels(value: &mut Value) -> &mut Value {
    &mut value["subjects"][0]["native_text_pixels"]["value"]
}

#[test]
fn text_pixel_absence_unknown_and_known_are_distinct_and_roundtrip() {
    let mut legacy = frame();
    legacy["subjects"][0]
        .as_object_mut()
        .unwrap()
        .remove("native_text_pixels");
    assert_eq!(serde_json::to_value(accepted(&legacy)).unwrap(), legacy);
    let mut denied = frame();
    denied["subjects"][0]["native_text_pixels"] = unknown();
    let parsed = accepted(&denied);
    assert!(
        parsed.subjects[0]
            .native_text_pixels
            .as_ref()
            .unwrap()
            .value()
            .is_none()
    );
    assert_eq!(serde_json::to_value(parsed).unwrap(), denied);
    assert_eq!(serde_json::to_value(accepted(&frame())).unwrap(), frame());
    let mut inactive = frame();
    inactive["subjects"][0]["opacity"] = known(json!(0.0));
    inactive["subjects"][0]["drawn"] = json!(false);
    pixels(&mut inactive)["calls"] = json!([]);
    pixels(&mut inactive)["mask"] =
        json!({"encoding":"row-runs-u16le-base64","data":"","runs":0,"changed_pixels":0});
    accepted(&inactive);
    inactive["subjects"][0]["opacity"] = known(json!(1.0));
    rejected(inactive);
}

#[test]
fn text_pixel_rejects_foreign_identity_clock_digest_and_unsupported_context() {
    for (key, wrong) in [
        ("native_key", json!("other/key")),
        ("frame", json!(42)),
        ("width", json!(4097)),
        ("height", json!(1920)),
        ("playback_fps", json!(29.0)),
        ("fps_den", json!(0)),
        ("selected_frames", json!([0, 42, 179])),
        ("selected_frames", json!([41, 41])),
        ("selected_frames", json!([0, 1, 2, 3, 4, 5, 6, 41])),
        ("render_input_digest", json!("INVALID")),
    ] {
        let mut value = frame();
        pixels(&mut value)[key] = wrong;
        rejected(value);
    }
    for (key, wrong) in [
        ("serial", json!(64)),
        ("operation", json!("drawImage")),
        ("text", json!("different")),
        ("text_digest", json!("cd".repeat(32))),
        ("global_alpha", json!(0.5)),
        ("composite", json!("copy")),
        ("filter", json!("blur(2px)")),
        ("font", json!("")),
        ("max_width", json!(0.0)),
        ("x", json!(2_000_001.0)),
        ("changed_pixels", json!(0)),
    ] {
        let mut value = frame();
        pixels(&mut value)["calls"][0][key] = wrong;
        rejected(value);
    }
    let mut value = frame();
    pixels(&mut value)["output_background"] = json!("#ffffff");
    rejected(value);
    let mut value = frame();
    value["subjects"][0]["native_type"] = known(json!("Code"));
    rejected(value);
    let mut value = frame();
    value["subjects"][0]["text"] = Value::Null;
    rejected(value);
}

#[test]
fn text_pixel_rejects_noncanonical_masks_and_open_or_incomplete_records() {
    for (key, wrong) in [
        ("encoding", json!("png")),
        ("data", json!("FAAKAAI=")),
        ("data", json!("FAAKAAIA=")),
        ("data", json!("!AAKAAIA")),
        ("runs", json!(2)),
        ("changed_pixels", json!(3)),
        ("data", json!("FAAKAAAA")), // zero run length
        ("data", json!("//8KAAIA")), // y outside the canvas
        ("data", json!("FAAKAAIA".repeat(10923))),
    ] {
        let mut value = frame();
        pixels(&mut value)["mask"][key] = wrong;
        rejected(value);
    }
    for data in ["FAAKAAIAFAAKAAIA", "FAAKAAIAFAAMAAIA", "FAAKAAIAEwAKAAIA"] {
        let mut value = frame();
        pixels(&mut value)["mask"]["data"] = json!(data);
        pixels(&mut value)["mask"]["runs"] = json!(2);
        pixels(&mut value)["mask"]["changed_pixels"] = json!(4);
        pixels(&mut value)["calls"][0]["changed_pixels"] = json!(4);
        rejected(value); // overlap, adjacent runs, or decreasing row
    }
    for path in ["output_background", "max_width"] {
        let mut value = frame();
        if path == "output_background" {
            pixels(&mut value).as_object_mut().unwrap().remove(path);
        } else {
            pixels(&mut value)["calls"][0]
                .as_object_mut()
                .unwrap()
                .remove(path);
        }
        rejected(value);
    }
    for at in [0, 1, 2] {
        let mut value = frame();
        let item = match at {
            0 => pixels(&mut value),
            1 => &mut pixels(&mut value)["mask"],
            _ => &mut pixels(&mut value)["calls"][0],
        };
        item["expected_as_observed"] = json!(true);
        rejected(value);
    }
}

#[test]
fn text_pixel_per_frame_budget_and_render_binding_span_all_subjects() {
    let mut value = frame();
    let mut other = value["subjects"][0].clone();
    other["id"] = json!("other");
    other["native_geometry"]["value"]["native_key"] = json!("other/key");
    other["native_text_pixels"]["value"]["native_key"] = json!("other/key");
    other["native_text_pixels"]["value"]["calls"][0]["serial"] = json!(1);
    value["subjects"].as_array_mut().unwrap().push(other);
    accepted(&value);
    for (key, wrong) in [
        ("render_input_digest", json!("cd".repeat(32))),
        ("selected_frames", json!([41])),
        ("width", json!(1080)),
    ] {
        let mut bad = value.clone();
        bad["subjects"][1]["native_text_pixels"]["value"][key] = wrong;
        rejected(bad);
    }
    let mut bad = value.clone();
    bad["subjects"][1]["native_text_pixels"]["value"]["calls"][0]["serial"] = json!(0);
    rejected(bad);
    let mut third = value["subjects"][1].clone();
    third["id"] = json!("third");
    third["native_geometry"]["value"]["native_key"] = json!("third/key");
    third["native_text_pixels"]["value"]["native_key"] = json!("third/key");
    third["native_text_pixels"]["value"]["calls"][0]["serial"] = json!(2);
    value["subjects"].as_array_mut().unwrap().push(third);
    rejected(value);
}
