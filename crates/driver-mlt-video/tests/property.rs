mod common;
use semwright_mlt_video::{
    adapters,
    edit::{self, Edit},
    hash,
    json::{self, Value},
    model,
    time::{FrameRange, FrameRate},
};
// Deterministic generated properties: no external proptest dependency and no claim of random coverage.
fn rng(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}
#[test]
fn property_ranges_partition_10000() {
    let mut seed = 0x1234_5678;
    for _ in 0..10_000 {
        let start = rng(&mut seed) % 10_000;
        let len = 2 + rng(&mut seed) % 1000;
        let at = start + 1 + rng(&mut seed) % (len - 1);
        let r = FrameRange::new(start, start + len).unwrap();
        let (a, b) = r.split(at).unwrap();
        assert_eq!(a.start, r.start);
        assert_eq!(b.end, r.end);
        assert_eq!(a.end, b.start);
        assert_eq!(a.duration() + b.duration(), len);
    }
}
#[test]
fn property_fractional_rates_roundtrip_milliseconds() {
    for (n, d) in [
        (24000, 1001),
        (24, 1),
        (25, 1),
        (30000, 1001),
        (30, 1),
        (50, 1),
        (60000, 1001),
        (60, 1),
    ] {
        let rate = FrameRate::new(n, d).unwrap();
        for f in (0..100_000).step_by(37) {
            assert_eq!(
                rate.from_milliseconds(rate.milliseconds(f).unwrap())
                    .unwrap(),
                f
            );
        }
    }
}
#[test]
fn property_drop_frame_roundtrip_one_day() {
    for (n, daily) in [(30000, 2_589_408), (60000, 5_178_816)] {
        let rate = FrameRate::new(n, 1001).unwrap();
        for f in (0..daily).step_by(997) {
            assert_eq!(
                rate.parse_timecode(&rate.timecode(f, true).unwrap())
                    .unwrap(),
                f
            );
        }
    }
}
#[test]
fn property_fixed_point_serialization() {
    for value in (-60000..24000).step_by(37) {
        assert_eq!(model::parse_decimal(&model::decimal(value)).unwrap(), value);
    }
}
#[test]
fn property_json_unicode_roundtrip() {
    let chars = ['a', 'ñ', '\n', '\t', '\u{1b}', '😀', '中', '"', '\\'];
    let mut seed = 15;
    for _ in 0..1024 {
        let s: String = (0..32)
            .map(|_| chars[(rng(&mut seed) % chars.len() as u64) as usize])
            .collect();
        let v = Value::from(s);
        assert_eq!(json::parse(v.encode().as_bytes()).unwrap(), v);
    }
}
#[test]
fn property_sha_chunking() {
    let mut seed = 17;
    for length in 0..512 {
        let bytes: Vec<u8> = (0..length).map(|_| rng(&mut seed) as u8).collect();
        let mut h = hash::Sha256::new();
        for part in bytes.chunks(17) {
            h.update(part);
        }
        assert_eq!(h.finish(), hash::sha256(&bytes));
    }
}
#[test]
fn property_split_source_and_timeline() {
    let p = common::small();
    for at in 1..100 {
        let plan = edit::plan(
            &p,
            &edit::revision(&p).unwrap(),
            Edit::Split {
                sequence: "sequence0".into(),
                clip: "c".into(),
                at,
            },
            &at.to_string(),
            false,
        )
        .unwrap();
        let clips = &plan.result.sequences[0].tracks[0].lanes[0].clips;
        assert_eq!(clips.len(), 2);
        assert_eq!(clips[0].source.end, clips[1].source.start);
        assert_eq!(clips[0].end().unwrap(), clips[1].start);
        assert_eq!(plan.after_frames, 100);
        assert_eq!(p.sequences[0].tracks[0].lanes[0].clips.len(), 1);
    }
}
#[test]
fn property_normal_form_roundtrip() {
    for count in 1..100 {
        let mut p = common::small();
        let lane = &mut p.sequences[0].tracks[0].lanes[0];
        lane.clips = (0..count)
            .map(|i| common::clip(&format!("c{i}"), "a", i * 7, 7))
            .collect();
        let xml = adapters::save(&p).unwrap();
        let back = adapters::load(xml.as_bytes()).unwrap();
        assert!(back.generated);
        assert_eq!(p.semantic_json(), back.semantic_json());
        assert_eq!(adapters::save(&back).unwrap(), xml);
    }
}
