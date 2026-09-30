use semwright_media_time::*;
use semwright_semantic_composition::{Digest, strict_decode};
fn q(n: i64, d: i64) -> Rational {
    Rational::new(n, d).unwrap()
}
#[test]
fn reduces_sign_and_zero() {
    assert_eq!(q(2, 4), q(1, 2));
    assert_eq!(q(1, -2), q(-1, 2));
    assert_eq!(q(0, 42), Rational::ZERO);
}
#[test]
fn zero_denominator_rejected() {
    assert!(Rational::new(1, 0).is_err());
}
#[test]
fn checked_overflow() {
    assert!(q(i64::MAX, 1).checked_add(q(1, 1)).is_err());
    assert!(q(i64::MIN, 1).checked_div(q(-1, 1)).is_err());
}
#[test]
fn all_rounding_policies_negative() {
    assert_eq!(q(-3, 2).round(Round::Floor).unwrap(), -2);
    assert_eq!(q(-3, 2).round(Round::Ceil).unwrap(), -1);
    assert_eq!(q(-3, 2).round(Round::TowardZero).unwrap(), -1);
    assert_eq!(q(-3, 2).round(Round::NearestAway).unwrap(), -2);
}
#[test]
fn noncanonical_wire_rejected() {
    for s in [
        r#"{"num":"2","den":"4"}"#,
        r#"{"num":"01","den":"1"}"#,
        r#"{"num":"-0","den":"1"}"#,
        r#"{"num":1,"den":2}"#,
        r#"{"num":"1","den":"0"}"#,
    ] {
        assert!(strict_decode::<Rational>(s.as_bytes()).is_err(), "{s}");
    }
}
#[test]
fn rational_wire_roundtrip() {
    for n in -99..=99 {
        for d in 1..=19 {
            let v = q(n, d);
            let bytes = serde_json::to_vec(&v).unwrap();
            assert_eq!(strict_decode::<Rational>(&bytes).unwrap(), v);
        }
    }
}
#[test]
fn frame_sample_matrix_no_accumulated_drift() {
    for (n, d) in [
        (24, 1),
        (25, 1),
        (30, 1),
        (60, 1),
        (24000, 1001),
        (30000, 1001),
        (60000, 1001),
    ] {
        let frames = Rate::new(n, d).unwrap();
        for hz in [44100, 48000, 96000] {
            let samples = Rate::new(hz, 1).unwrap();
            for i in [0, 1, 2, 999, 10000, 1000000] {
                let t = frames.at(i).unwrap();
                let s = samples.quantize(t, Round::NearestAway).unwrap();
                let err = s.error;
                if err.num != 0 {
                    assert!(q(err.num.abs(), err.den) <= q(1, 2 * i64::from(hz)));
                }
                let back = frames
                    .quantize(samples.at(s.index).unwrap(), Round::NearestAway)
                    .unwrap();
                assert_eq!(back.index, i);
            }
        }
    }
}
#[test]
fn addition_subtraction_property() {
    for n in -40..40 {
        for d in 1..30 {
            let a = q(n, d);
            let b = q(d, 31);
            assert_eq!(a.checked_add(b).unwrap().checked_sub(b).unwrap(), a);
        }
    }
}
#[test]
fn long_ntsc_boundary_is_exact() {
    let fps = Rate::new(30000, 1001).unwrap();
    let sr = Rate::new(48000, 1).unwrap();
    assert_eq!(
        sr.quantize(fps.at(30000).unwrap(), Round::NearestAway)
            .unwrap()
            .index,
        48048000
    );
}
#[test]
fn interval_half_open() {
    let i = Interval::new(q(0, 1), q(1, 1)).unwrap();
    assert!(i.contains(q(0, 1)));
    assert!(!i.contains(q(1, 1)));
    assert!(Interval::new(q(1, 1), q(1, 1)).is_err());
}
#[test]
fn preroll_is_explicit() {
    let i = Interval::new(q(-1, 1), q(1, 1)).unwrap();
    assert!(i.nonnegative().is_err());
    assert_eq!(i.duration().unwrap(), q(2, 1));
}
#[test]
fn uses_existing_video_frame_rate() {
    let native = semwright_video_domain::time::FrameRate::new(30000, 1001).unwrap();
    let r = Rate::try_from(native).unwrap();
    assert_eq!(
        semwright_video_domain::time::FrameRate::try_from(r).unwrap(),
        native
    );
}
fn cue(id: &str, anchor: Anchor) -> Cue {
    Cue {
        id: id.into(),
        anchor,
        duration: q(1, 1),
        source: Digest::of_bytes(b"source"),
        method: "fixture".into(),
        version: 1,
        confidence: Some(10000),
    }
}
#[test]
fn cue_dag_order_independent() {
    let g = CueGraph {
        version: 1,
        cues: vec![
            cue(
                "b",
                Anchor::After {
                    cue: "a".into(),
                    offset: q(1, 2),
                },
            ),
            cue("a", Anchor::Absolute { time: q(0, 1) }),
        ],
    };
    assert_eq!(
        g.resolve().unwrap()["b"],
        ResolvedCue::Resolved {
            start: q(3, 2),
            end: q(5, 2)
        }
    );
}
#[test]
fn cue_cycle_rejected() {
    let g = CueGraph {
        version: 1,
        cues: vec![
            cue(
                "a",
                Anchor::After {
                    cue: "b".into(),
                    offset: q(0, 1),
                },
            ),
            cue(
                "b",
                Anchor::After {
                    cue: "a".into(),
                    offset: q(0, 1),
                },
            ),
        ],
    };
    assert!(g.resolve().is_err());
}
#[test]
fn unknown_words_never_receive_timestamps() {
    let g = CueGraph {
        version: 1,
        cues: vec![
            cue(
                "a",
                Anchor::Unknown {
                    reason: "unaligned word".into(),
                },
            ),
            cue(
                "b",
                Anchor::After {
                    cue: "a".into(),
                    offset: q(0, 1),
                },
            ),
        ],
    };
    assert!(matches!(
        g.resolve().unwrap()["b"],
        ResolvedCue::Unknown { .. }
    ));
}
#[test]
fn time_map_does_not_extrapolate() {
    let m = TimeMap {
        version: 1,
        segments: vec![MapSegment {
            source: Interval::new(q(0, 1), q(2, 1)).unwrap(),
            target: Interval::new(q(3, 1), q(4, 1)).unwrap(),
        }],
    };
    assert_eq!(m.map(q(1, 1)).unwrap(), q(7, 2));
    assert!(m.map(q(2, 1)).is_err());
}

#[test]
fn rational_checked_div_uses_reciprocal_exactly() {
    assert_eq!(q(2, 3).checked_div(q(4, 5)).unwrap(), q(5, 6));
    assert_eq!(q(-7, 9).checked_div(q(14, 3)).unwrap(), q(-1, 6));
    assert!(q(1, 2).checked_div(Rational::ZERO).is_err());
}

#[test]
fn time_map_version_size_and_axis_overlap_are_independent_constraints() {
    let segment = || MapSegment {
        source: Interval::new(q(0, 1), q(1, 1)).unwrap(),
        target: Interval::new(q(10, 1), q(11, 1)).unwrap(),
    };

    let invalid_version = TimeMap {
        version: 2,
        segments: vec![segment()],
    };
    assert!(invalid_version.validate().is_err());

    let empty = TimeMap {
        version: 1,
        segments: vec![],
    };
    assert!(empty.validate().is_err());

    let too_many = TimeMap {
        version: 1,
        segments: (0..1025)
            .map(|index| MapSegment {
                source: Interval::new(q(index, 1), q(index + 1, 1)).unwrap(),
                target: Interval::new(q(index + 2000, 1), q(index + 2001, 1)).unwrap(),
            })
            .collect(),
    };
    assert!(too_many.validate().is_err());

    let source_overlap = TimeMap {
        version: 1,
        segments: vec![
            MapSegment {
                source: Interval::new(q(0, 1), q(2, 1)).unwrap(),
                target: Interval::new(q(0, 1), q(1, 1)).unwrap(),
            },
            MapSegment {
                source: Interval::new(q(1, 1), q(3, 1)).unwrap(),
                target: Interval::new(q(1, 1), q(2, 1)).unwrap(),
            },
        ],
    };
    assert!(source_overlap.validate().is_err());

    let target_overlap = TimeMap {
        version: 1,
        segments: vec![
            MapSegment {
                source: Interval::new(q(0, 1), q(1, 1)).unwrap(),
                target: Interval::new(q(0, 1), q(2, 1)).unwrap(),
            },
            MapSegment {
                source: Interval::new(q(1, 1), q(2, 1)).unwrap(),
                target: Interval::new(q(1, 1), q(3, 1)).unwrap(),
            },
        ],
    };
    assert!(target_overlap.validate().is_err());

    let adjacent = TimeMap {
        version: 1,
        segments: vec![
            MapSegment {
                source: Interval::new(q(0, 1), q(1, 1)).unwrap(),
                target: Interval::new(q(10, 1), q(11, 1)).unwrap(),
            },
            MapSegment {
                source: Interval::new(q(1, 1), q(2, 1)).unwrap(),
                target: Interval::new(q(11, 1), q(12, 1)).unwrap(),
            },
        ],
    };
    adjacent.validate().unwrap();
    assert_eq!(adjacent.map(q(1, 1)).unwrap(), q(11, 1));
}

#[test]
fn cue_graph_version_size_and_cue_metadata_are_bounded_independently() {
    let invalid_version = CueGraph {
        version: 2,
        cues: vec![cue("a", Anchor::Absolute { time: q(0, 1) })],
    };
    assert!(invalid_version.resolve().is_err());

    let too_many = CueGraph {
        version: 1,
        cues: (0..513)
            .map(|index| {
                cue(
                    &format!("cue-{index}"),
                    Anchor::Absolute { time: q(index, 1) },
                )
            })
            .collect(),
    };
    assert!(too_many.resolve().is_err());

    let mut version_zero = cue("a", Anchor::Absolute { time: q(0, 1) });
    version_zero.version = 0;
    assert!(
        CueGraph {
            version: 1,
            cues: vec![version_zero]
        }
        .resolve()
        .is_err()
    );

    let mut excessive_confidence = cue("a", Anchor::Absolute { time: q(0, 1) });
    excessive_confidence.confidence = Some(10001);
    assert!(
        CueGraph {
            version: 1,
            cues: vec![excessive_confidence]
        }
        .resolve()
        .is_err()
    );

    let duplicate = CueGraph {
        version: 1,
        cues: vec![
            cue("a", Anchor::Absolute { time: q(0, 1) }),
            cue("a", Anchor::Absolute { time: q(2, 1) }),
        ],
    };
    assert!(duplicate.resolve().is_err());
}

#[test]
fn cue_after_requires_existing_reference_and_propagates_unknown_without_timestamp() {
    let missing = CueGraph {
        version: 1,
        cues: vec![cue(
            "b",
            Anchor::After {
                cue: "missing".into(),
                offset: q(0, 1),
            },
        )],
    };
    assert!(missing.resolve().is_err());

    let graph = CueGraph {
        version: 1,
        cues: vec![
            cue(
                "a",
                Anchor::Unknown {
                    reason: "unaligned".into(),
                },
            ),
            cue(
                "b",
                Anchor::After {
                    cue: "a".into(),
                    offset: q(100, 1),
                },
            ),
        ],
    };
    let resolved = graph.resolve().unwrap();
    assert_eq!(
        resolved["a"],
        ResolvedCue::Unknown {
            reason: "unaligned".into()
        }
    );
    assert_eq!(
        resolved["b"],
        ResolvedCue::Unknown {
            reason: "unaligned".into()
        }
    );
}

#[test]
fn rational_validation_and_rounding_boundaries_are_exact() {
    assert!(Rational { num: 1, den: 0 }.validate().is_err());
    assert!(Rational { num: 2, den: 4 }.validate().is_err());
    assert!(Rational { num: 1, den: -2 }.validate().is_err());
    Rational { num: -1, den: 2 }.validate().unwrap();

    for integer in [-2, -1, 0, 1, 2] {
        let value = q(integer, 1);
        assert_eq!(value.round(Round::Floor).unwrap(), integer);
        assert_eq!(value.round(Round::Ceil).unwrap(), integer);
        assert_eq!(value.round(Round::TowardZero).unwrap(), integer);
        assert_eq!(value.round(Round::NearestAway).unwrap(), integer);
    }

    assert_eq!(q(3, 2).round(Round::Floor).unwrap(), 1);
    assert_eq!(q(3, 2).round(Round::Ceil).unwrap(), 2);
    assert_eq!(q(1, 2).round(Round::NearestAway).unwrap(), 1);
    assert_eq!(q(-1, 2).round(Round::NearestAway).unwrap(), -1);
    assert_eq!(q(49, 100).round(Round::NearestAway).unwrap(), 0);
}

#[test]
fn rate_validate_rejects_noncanonical_and_out_of_bounds_struct_values() {
    Rate { num: 30, den: 1 }.validate().unwrap();
    assert!(Rate { num: 2, den: 2 }.validate().is_err());
    assert!(Rate { num: 0, den: 1 }.validate().is_err());
    assert!(Rate { num: 1, den: 0 }.validate().is_err());
    assert!(
        Rate {
            num: 1_000_001,
            den: 1
        }
        .validate()
        .is_err()
    );
}

#[test]
fn rational_ordering_uses_cross_products_not_integer_division() {
    assert!(q(3, 2) > q(4, 3));
    assert!(q(-4, 3) < q(-5, 4));
    assert_eq!(q(6, 4).cmp(&q(3, 2)), std::cmp::Ordering::Equal);
}
