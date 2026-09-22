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
    assert_eq!(caps.len(), 68);
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
