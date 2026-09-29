use semwright_mlt_video::{
    app::App,
    catalog, hash,
    json::{self, Value, obj},
    wire,
};
use std::collections::{BTreeMap, BTreeSet};
fn hello() -> Value {
    obj([
        ("type", "hello".into()),
        ("protocol", 1u64.into()),
        (
            "provider",
            obj([
                ("id", catalog::PROVIDER.into()),
                ("kind", "driver".into()),
                ("version", "0.1.0".into()),
                ("namespace", catalog::PREFIX.into()),
                ("application", Value::Null),
                ("origin", "driver-manifest:fixture".into()),
            ]),
        ),
        ("executable_sha256", "a".repeat(64).into()),
    ])
}
#[test]
fn driver_frame_big_endian_roundtrip() {
    let value = obj([("type", "health".into()), ("id", "1".into())]);
    let mut bytes = vec![];
    wire::write_frame(&mut bytes, &value).unwrap();
    assert_eq!(
        u32::from_be_bytes(bytes[..4].try_into().unwrap()) as usize,
        bytes.len() - 4
    );
    assert_eq!(
        wire::read_frame(&mut bytes.as_slice()).unwrap(),
        Some(value)
    );
}
#[test]
fn driver_frame_eof() {
    assert_eq!(wire::read_frame(&mut b"".as_slice()).unwrap(), None);
}
#[test]
fn driver_frame_truncated_header() {
    assert!(wire::read_frame(&mut b"\0\0".as_slice()).is_err());
}
#[test]
fn driver_frame_zero_denied() {
    assert!(wire::read_frame(&mut [0u8; 4].as_slice()).is_err());
}
#[test]
fn driver_frame_oversize_denied() {
    assert!(wire::read_frame(&mut 1_048_577u32.to_be_bytes().as_slice()).is_err());
}
#[test]
fn hello_identity_and_executable_pin() {
    assert!(wire::validate_hello(&hello(), &"a".repeat(64)).is_ok());
    assert!(wire::validate_hello(&hello(), &"b".repeat(64)).is_err());
}
#[test]
fn hello_unknown_fields_denied() {
    let mut h = hello();
    if let Value::Object(m) = &mut h {
        m.insert("events".into(), true.into());
    }
    assert!(wire::validate_hello(&h, &"a".repeat(64)).is_err());
}
#[test]
fn capability_namespace_unique_and_bounded() {
    let caps = catalog::capabilities().unwrap();
    assert!(caps.len() >= 70);
    for required in [
        "driver.mlt-video.av.mux",
        "driver.mlt-video.frames.encode",
        "driver.mlt-video.sync.probe",
    ] {
        assert!(caps.iter().any(|capability| capability.name == required));
    }
    let mut seen = BTreeSet::new();
    for c in &caps {
        assert!(c.name.starts_with(catalog::PREFIX));
        assert!(seen.insert(&c.name));
        assert!(hash::valid_digest(&c.digest));
    }
    assert!(catalog::catalog_wire(&caps).len() < wire::MAX_FRAME);
}
#[test]
fn static_descriptor_golden_is_checked() {
    let caps = catalog::capabilities().unwrap();
    let expected = json::parse(include_bytes!("../fixtures/expected/digests.json")).unwrap();
    assert_eq!(
        catalog::digest(&caps),
        expected.str("catalog_sha256").unwrap()
    );
    for c in &caps {
        assert_eq!(
            expected
                .get("descriptor_sha256")
                .unwrap()
                .get(&c.name)
                .unwrap()
                .string()
                .unwrap(),
            c.digest
        );
    }
}
#[test]
fn schemas_reject_unknown_arguments() {
    let caps = catalog::capabilities().unwrap();
    let doctor = caps.iter().find(|c| c.name.ends_with(".doctor")).unwrap();
    assert!(
        doctor
            .input(&obj([("shell", "echo unsafe".into())]))
            .is_err()
    );
    assert!(doctor.input(&obj([])).is_ok());
}
#[test]
fn doctor_satisfies_actual_output_schema() {
    let mut app = App::new(BTreeMap::new(), None).unwrap();
    let c = app
        .capabilities
        .iter()
        .find(|c| c.name.ends_with(".doctor"))
        .unwrap()
        .clone();
    let v = app.execute(&c.name, &c.digest, obj([])).unwrap();
    assert!(!v.get("render_available").unwrap().boolean().unwrap());
    assert_eq!(v.uint("driver_protocol").unwrap(), 1);
}
#[test]
fn wrong_descriptor_digest_rejected_before_execution() {
    let mut app = App::new(BTreeMap::new(), None).unwrap();
    assert!(
        app.execute("driver.mlt-video.project.create", &"a".repeat(64), obj([]))
            .is_err()
    );
}
#[test]
fn lifecycle_hello_catalog_health_shutdown() {
    let mut input = vec![];
    for v in [
        hello(),
        obj([("type", "capabilities".into()), ("id", "c".into())]),
        obj([("type", "health".into()), ("id", "h".into())]),
        obj([("type", "shutdown".into()), ("id", "s".into())]),
    ] {
        wire::write_frame(&mut input, &v).unwrap();
    }
    let mut output = vec![];
    let mut app = App::new(BTreeMap::new(), None).unwrap();
    wire::serve(
        &mut app,
        &mut input.as_slice(),
        &mut output,
        &"a".repeat(64),
    )
    .unwrap();
    let mut cursor = output.as_slice();
    for kind in ["ready", "capabilities", "healthy", "shutdown"] {
        assert_eq!(
            wire::read_frame(&mut cursor)
                .unwrap()
                .unwrap()
                .str("type")
                .unwrap(),
            kind
        );
    }
    assert!(cursor.is_empty());
}
#[test]
fn mutation_catalog_does_not_claim_transport_dry_run() {
    for c in catalog::capabilities().unwrap() {
        if c.mutates() {
            assert!(!c.descriptor.get("dry_run").unwrap().boolean().unwrap());
        }
    }
}
#[test]
fn schema_pages_bounded() {
    let caps = catalog::capabilities().unwrap();
    let c = caps
        .iter()
        .find(|c| c.name.ends_with(".sequence.list"))
        .unwrap();
    let args = obj([
        ("project", "video:0123456789abcdef0123456789abcdef".into()),
        ("limit", 101u64.into()),
    ]);
    assert!(c.input(&args).is_err());
}

#[test]
fn av_mux_schema_is_closed_and_declares_only_certified_audio_profile() {
    let caps = catalog::capabilities().unwrap();
    let mux = caps
        .iter()
        .find(|capability| capability.name == "driver.mlt-video.av.mux")
        .unwrap();
    let valid = obj([
        ("video_root", "media".into()),
        ("video_path", "video.mkv".into()),
        ("video_sha256", "a".repeat(64).into()),
        ("audio_root", "media".into()),
        ("audio_path", "audio.wav".into()),
        ("audio_sha256", "b".repeat(64).into()),
        ("width", 1920u64.into()),
        ("height", 1080u64.into()),
        ("fps_num", 30000u64.into()),
        ("fps_den", 1001u64.into()),
        ("frame_count", 90u64.into()),
        ("sample_rate", 48000u64.into()),
        ("channels", 2u64.into()),
        ("profile", "h264-aac-mp4".into()),
        ("output_path", "master.mp4".into()),
        ("max_bytes", 64_000_000u64.into()),
    ]);
    mux.input(&valid).unwrap();

    let mut unknown = valid.clone();
    if let Value::Object(fields) = &mut unknown {
        fields.insert("shell".into(), "ffmpeg arbitrary".into());
    }
    assert!(mux.input(&unknown).is_err());

    let mut unsupported_rate = valid;
    if let Value::Object(fields) = &mut unsupported_rate {
        fields.insert("sample_rate".into(), 44100u64.into());
    }
    assert!(mux.input(&unsupported_rate).is_err());
}
