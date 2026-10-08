use semwright_media_time::{Rate, Rational as Q};
use semwright_motion_authoring::*;
use semwright_semantic_composition as c;

fn film() -> Film {
    let mut film: Film = c::strict_decode(include_bytes!(
        "../../../fixtures/composition/motion/technical.json"
    ))
    .unwrap();
    film.output.frame_rate = Rate::new(1, 1).unwrap();
    let shot = &mut film.sequences[0].beats[0].shots[0];
    shot.motion.clear();
    shot.constraints.clear();
    film
}

fn base() -> c::BaseStateSet {
    c::BaseStateSet(vec![c::BaseState {
        key: c::ResourceKey {
            provider: "driver:motion-canvas".into(),
            resource: "project".into(),
        },
        document_id: "document".into(),
        provider_session: "native-session".into(),
        generation: "1".into(),
        revision: c::Revision::Counter(1),
        concurrency: c::Concurrency::BestEffortRevalidate,
    }])
}

fn coverage() -> RangeCoverage {
    RangeCoverage {
        first_frame: 0,
        end_frame_exclusive: 3,
        observed_frames: 0,
        exhaustive: false,
        fps: Rate::new(1, 1).unwrap(),
        method: "native-probe".into(),
        observation_digest: c::Digest::of_bytes(b"observations"),
        render_input_digest: c::Digest::of_bytes(b"render-input"),
        font_resources_digest: Some(c::Digest::of_bytes(b"fonts")),
        artifact_digest: c::Digest::of_bytes(b"artifact"),
    }
}

fn unknown<T>(reason: &str) -> Known<T> {
    Known::Unknown {
        reason: reason.into(),
    }
}

fn subject(id: &str, bounds: Bounds) -> SubjectObservation {
    SubjectObservation {
        id: id.into(),
        local_size: Known::Known {
            value: Size {
                width: bounds.width,
                height: bounds.height,
            },
        },
        bounds: Known::Known { value: bounds },
        transform: Known::Known {
            value: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        },
        opacity: Known::Known { value: 1.0 },
        drawn: true,
        z_order: Known::Known { value: 0 },
        clip_active: Known::Known { value: false },
        pixel_visibility: Known::Known { value: true },
        text: None,
        asset_ready: Known::Known { value: true },
    }
}

fn frame(index: u64) -> FrameObservation {
    FrameObservation {
        frame: index,
        subjects: vec![],
        transitions: vec![],
        captions: vec![],
        unresolved_cues: vec![],
    }
}

fn measured(film: &Film, frames: Vec<FrameObservation>) -> MotionMeasurement {
    measure(
        film,
        c::Digest::of_bytes(b"plan"),
        base(),
        coverage(),
        frames.into_iter().map(Ok),
    )
    .unwrap()
}

#[test]
fn complete_native_frame_range_passes_and_missing_frame_is_unknown() {
    let film = film();
    let complete = measured(&film, vec![frame(0), frame(1), frame(2)]);
    assert!(complete.coverage.exhaustive);
    assert_eq!(complete.coverage.observed_frames, 3);
    assert_eq!(complete.validation.verdict().unwrap(), c::Verdict::Pass);
    assert!(
        complete
            .validation
            .checks
            .iter()
            .all(|check| check.verdict == c::Verdict::Pass)
    );

    let incomplete = measured(&film, vec![frame(0), frame(2)]);
    assert!(!incomplete.coverage.exhaustive);
    assert_eq!(incomplete.coverage.observed_frames, 2);
    assert_eq!(
        incomplete.validation.verdict().unwrap(),
        c::Verdict::Unknown
    );
    let coverage_check = incomplete
        .validation
        .checks
        .iter()
        .find(|check| check.rule == "native-frame-coverage")
        .unwrap();
    assert_eq!(coverage_check.verdict, c::Verdict::Unknown);
}

#[test]
fn transition_completion_requires_a_post_interval_frame_not_the_exact_boundary() {
    let mut film = film();
    film.sequences[0].beats[0].shots[0].motion = vec![Invocation {
        id: "motion".into(),
        span_id: "motion-span".into(),
        easing: MotionEasing::Linear,
        primitive: Primitive::FadeIn {
            target: "box_a".into(),
        },
    }];

    let boundary = TransitionObservation {
        invocation_id: "motion".into(),
        finished: Known::Known { value: false },
    };
    let completed = TransitionObservation {
        invocation_id: "motion".into(),
        finished: Known::Known { value: true },
    };
    let mut f0 = frame(0);
    f0.transitions = vec![boundary.clone()];
    let mut f1 = frame(1);
    // Frame 1 is exactly the end of motion-span [0, 1). Motion Canvas may
    // not have committed the generator completion receipt until evaluation
    // continues past this frame, so false here is not a failure.
    f1.transitions = vec![boundary];
    let mut f2 = frame(2);
    f2.transitions = vec![completed];

    let result = measured(&film, vec![f0, f1, f2]);
    let transition = result
        .validation
        .checks
        .iter()
        .find(|check| check.rule == "transition-completion")
        .unwrap();
    assert_eq!(transition.verdict, c::Verdict::Pass);
}

#[test]
fn transition_completion_without_post_interval_evidence_is_unknown() {
    let mut film = film();
    film.sequences[0].beats[0].shots[0].motion = vec![Invocation {
        id: "motion".into(),
        span_id: "shot-span".into(),
        easing: MotionEasing::Linear,
        primitive: Primitive::FadeIn {
            target: "box_a".into(),
        },
    }];
    let mut frames = vec![frame(0), frame(1), frame(2)];
    for value in &mut frames {
        value.transitions = vec![TransitionObservation {
            invocation_id: "motion".into(),
            finished: Known::Known { value: false },
        }];
    }
    let result = measured(&film, frames);
    let transition = result
        .validation
        .checks
        .iter()
        .find(|check| check.rule == "transition-completion")
        .unwrap();
    assert_eq!(transition.verdict, c::Verdict::Unknown);
}

#[test]
fn native_frame_order_range_and_identity_are_fail_closed() {
    let film = film();
    for frames in [
        vec![frame(0), frame(0)],
        vec![frame(1), frame(0)],
        vec![frame(0), frame(3)],
    ] {
        assert!(
            measure(
                &film,
                c::Digest::of_bytes(b"plan"),
                base(),
                coverage(),
                frames.into_iter().map(Ok),
            )
            .is_err()
        );
    }

    let mut foreign = frame(0);
    foreign.subjects.push(subject(
        "not-owned",
        Bounds {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        },
    ));
    assert!(
        measure(
            &film,
            c::Digest::of_bytes(b"plan"),
            base(),
            coverage(),
            vec![Ok(foreign)],
        )
        .is_err()
    );
}

#[test]
fn frame_observation_collection_limits_are_independent() {
    let base_subject = subject(
        "box_a",
        Bounds {
            x: 24.0,
            y: 24.0,
            width: 10.0,
            height: 10.0,
        },
    );
    let transition = TransitionObservation {
        invocation_id: "motion".into(),
        finished: Known::Known { value: true },
    };
    let caption = CaptionObservation {
        id: "caption".into(),
        active: Known::Known { value: true },
        cue_start: Known::Known { value: Q::ZERO },
        cue_end: Known::Known { value: Q::ONE },
    };

    let mut value = frame(0);
    value.subjects = (0..1025)
        .map(|index| {
            let mut item = base_subject.clone();
            item.id = format!("subject-{index}");
            item
        })
        .collect();
    assert!(value.validate().is_err());

    let mut value = frame(0);
    value.transitions = (0..1025)
        .map(|index| TransitionObservation {
            invocation_id: format!("motion-{index}"),
            ..transition.clone()
        })
        .collect();
    assert!(value.validate().is_err());

    let mut value = frame(0);
    value.captions = (0..513)
        .map(|index| CaptionObservation {
            id: format!("caption-{index}"),
            ..caption.clone()
        })
        .collect();
    assert!(value.validate().is_err());

    let mut value = frame(0);
    value.unresolved_cues = (0..513).map(|index| format!("cue-{index}")).collect();
    assert!(value.validate().is_err());
}

#[test]
fn frame_observation_rejects_duplicate_ids_and_invalid_native_numbers() {
    let observation = subject(
        "box_a",
        Bounds {
            x: 24.0,
            y: 24.0,
            width: 10.0,
            height: 10.0,
        },
    );
    let mut value = frame(0);
    value.subjects = vec![observation.clone(), observation.clone()];
    assert!(value.validate().is_err());

    let mut invalid = observation.clone();
    invalid.opacity = Known::Known { value: 1.1 };
    let mut value = frame(0);
    value.subjects = vec![invalid];
    assert!(value.validate().is_err());

    let mut invalid = observation.clone();
    invalid.bounds = Known::Known {
        value: Bounds {
            x: 0.0,
            y: 0.0,
            width: -1.0,
            height: 10.0,
        },
    };
    let mut value = frame(0);
    value.subjects = vec![invalid];
    assert!(value.validate().is_err());

    let mut invalid = observation;
    invalid.transform = Known::Known {
        value: [1.0, 0.0, f64::INFINITY, 1.0, 0.0, 0.0],
    };
    let mut value = frame(0);
    value.subjects = vec![invalid];
    assert!(value.validate().is_err());

    let duplicate_transition = TransitionObservation {
        invocation_id: "motion".into(),
        finished: Known::Known { value: true },
    };
    let mut value = frame(0);
    value.transitions = vec![duplicate_transition.clone(), duplicate_transition];
    assert!(value.validate().is_err());

    let duplicate_caption = CaptionObservation {
        id: "caption".into(),
        active: Known::Known { value: true },
        cue_start: unknown("not-observed"),
        cue_end: unknown("not-observed"),
    };
    let mut value = frame(0);
    value.captions = vec![duplicate_caption.clone(), duplicate_caption];
    assert!(value.validate().is_err());
}

#[test]
fn safe_area_uses_observed_bounds_not_desired_layout() {
    let mut film = film();
    film.sequences[0].beats[0].shots[0].constraints = vec![VisualConstraint::SafeArea {
        subject: "box_a".into(),
        tolerance: 0.0,
    }];
    let frames = (0..3)
        .map(|index| {
            let mut value = frame(index);
            value.subjects.push(subject(
                "box_a",
                Bounds {
                    x: 0.0,
                    y: 0.0,
                    width: 50.0,
                    height: 50.0,
                },
            ));
            value
        })
        .collect();
    let result = measured(&film, frames);
    assert_eq!(result.validation.verdict().unwrap(), c::Verdict::Fail);
    assert_eq!(result.findings.len(), 1);
    assert_eq!(result.findings[0].rule, "shot:constraint:0");
    assert_eq!(result.findings[0].subject, "box_a");
    assert_eq!(result.findings[0].first_frame, Some(0));
    assert_eq!(result.findings[0].affected_frames, 3);
}

#[test]
fn unknown_font_evidence_remains_unknown_and_false_font_is_fail() {
    let mut film = film();
    film.sequences[0].beats[0].shots[0].constraints = vec![VisualConstraint::FontLoaded {
        subject: "label".into(),
    }];

    let frames_with_font = |ready: Known<bool>| {
        (0..3)
            .map(|index| {
                let mut value = frame(index);
                let mut label = subject(
                    "label",
                    Bounds {
                        x: 24.0,
                        y: 24.0,
                        width: 200.0,
                        height: 40.0,
                    },
                );
                label.text = Some(TextObservation {
                    logical_text_digest: c::Digest::of_bytes(b"text"),
                    observed_text_digest: c::Digest::of_bytes(b"text"),
                    truncated: Known::Known { value: false },
                    lines: Known::Known { value: 1 },
                    font_family: Known::Known {
                        value: "Instrument Sans Variable".into(),
                    },
                    font_ready: ready.clone(),
                    fallback_used: Known::Known { value: false },
                    direction: Known::Known {
                        value: "ltr".into(),
                    },
                });
                value.subjects.push(label);
                value
            })
            .collect::<Vec<_>>()
    };

    let unknown_result = measured(&film, frames_with_font(unknown("font-unobservable")));
    assert_eq!(
        unknown_result.validation.verdict().unwrap(),
        c::Verdict::Unknown
    );

    let failed = measured(&film, frames_with_font(Known::Known { value: false }));
    assert_eq!(failed.validation.verdict().unwrap(), c::Verdict::Fail);

    let passed = measured(&film, frames_with_font(Known::Known { value: true }));
    assert_eq!(passed.validation.verdict().unwrap(), c::Verdict::Pass);
}

#[test]
fn measured_speed_uses_consecutive_native_centers_and_enforces_limit() {
    let mut film = film();
    film.sequences[0].beats[0].shots[0].constraints = vec![VisualConstraint::MaximumSpeed {
        subject: "box_a".into(),
        pixels_per_second: 15.0,
    }];
    let xs = [24.0, 34.0, 54.0];
    let frames = xs
        .into_iter()
        .enumerate()
        .map(|(index, x)| {
            let mut value = frame(index as u64);
            value.subjects.push(subject(
                "box_a",
                Bounds {
                    x,
                    y: 24.0,
                    width: 10.0,
                    height: 10.0,
                },
            ));
            value
        })
        .collect();
    let result = measured(&film, frames);
    assert_eq!(result.validation.verdict().unwrap(), c::Verdict::Fail);
    let metric = result
        .metrics
        .iter()
        .find(|metric| metric.subject == "box_a")
        .unwrap();
    assert_eq!(metric.observed_frames, 3);
    assert_eq!(metric.maximum_speed, Some(20.0));
    assert_eq!(metric.maximum_acceleration, Some(10.0));
    assert_eq!(metric.kinematics_unknown_frames, 0);
}

fn rule_verdict(result: &MotionMeasurement, rule: &str) -> c::Verdict {
    result
        .validation
        .checks
        .iter()
        .find(|check| check.rule == rule)
        .unwrap()
        .verdict
}

fn with_rule(rule: VisualConstraint) -> Film {
    let mut value = film();
    value.sequences[0].beats[0].shots[0].constraints = vec![rule];
    value
}

fn repeated(observations: Vec<SubjectObservation>) -> Vec<FrameObservation> {
    (0..3)
        .map(|index| {
            let mut value = frame(index);
            value.subjects = observations.clone();
            value
        })
        .collect()
}

fn bounds(x: f64, y: f64, width: f64, height: f64) -> Bounds {
    Bounds {
        x,
        y,
        width,
        height,
    }
}

#[test]
fn safe_area_checks_each_edge_and_includes_exact_tolerance() {
    let mut film = with_rule(VisualConstraint::SafeArea {
        subject: "box_a".into(),
        tolerance: 2.0,
    });
    film.output.safe_area = Insets {
        left: 23.0,
        top: 31.0,
        right: 47.0,
        bottom: 59.0,
    };
    for (native, expected) in [
        (bounds(21.0, 29.0, 40.0, 30.0), c::Verdict::Pass),
        (bounds(555.0, 393.0, 40.0, 30.0), c::Verdict::Pass),
        (bounds(20.5, 29.0, 40.0, 30.0), c::Verdict::Fail),
        (bounds(21.0, 28.5, 40.0, 30.0), c::Verdict::Fail),
        (bounds(555.5, 100.0, 40.0, 30.0), c::Verdict::Fail),
        (bounds(100.0, 393.5, 40.0, 30.0), c::Verdict::Fail),
    ] {
        let result = measured(&film, repeated(vec![subject("box_a", native)]));
        assert_eq!(
            rule_verdict(&result, "shot:constraint:0"),
            expected,
            "{native:?}"
        );
    }
}

#[test]
fn overlap_requires_intersection_on_both_axes_and_uses_native_edges() {
    let film = with_rule(VisualConstraint::NoOverlap {
        subject: "box_a".into(),
        other: "box_b".into(),
        tolerance: 1.0,
    });
    let a = subject("box_a", bounds(10.0, 20.0, 40.0, 60.0));
    for (native, expected) in [
        (bounds(30.0, 40.0, 17.0, 25.0), c::Verdict::Fail),
        (bounds(49.0, 40.0, 17.0, 25.0), c::Verdict::Pass),
        (bounds(48.5, 40.0, 17.0, 25.0), c::Verdict::Fail),
        (bounds(30.0, 79.0, 17.0, 25.0), c::Verdict::Pass),
        (bounds(30.0, 78.5, 17.0, 25.0), c::Verdict::Fail),
        (bounds(-6.0, 40.0, 17.0, 25.0), c::Verdict::Pass),
        (bounds(-5.5, 40.0, 17.0, 25.0), c::Verdict::Fail),
        (bounds(30.0, -4.0, 17.0, 25.0), c::Verdict::Pass),
        (bounds(30.0, -3.5, 17.0, 25.0), c::Verdict::Fail),
        (bounds(60.0, 40.0, 17.0, 25.0), c::Verdict::Pass),
        (bounds(30.0, 90.0, 17.0, 25.0), c::Verdict::Pass),
    ] {
        let result = measured(&film, repeated(vec![a.clone(), subject("box_b", native)]));
        assert_eq!(
            rule_verdict(&result, "shot:constraint:0"),
            expected,
            "{native:?}"
        );
    }
}

#[test]
fn overlap_proof_requires_both_unclipped_unrounded_axis_aligned_rectangles() {
    let original = with_rule(VisualConstraint::NoOverlap {
        subject: "box_a".into(),
        other: "box_b".into(),
        tolerance: 0.0,
    });
    let observations = vec![
        subject("box_a", bounds(10.0, 20.0, 40.0, 60.0)),
        subject("box_b", bounds(30.0, 40.0, 17.0, 25.0)),
    ];
    assert_eq!(
        rule_verdict(
            &measured(&original, repeated(observations.clone())),
            "shot:constraint:0"
        ),
        c::Verdict::Fail
    );
    for side in 0..2 {
        for change in 0..8 {
            let mut film = original.clone();
            let mut observed = observations.clone();
            match change {
                0 => {
                    if let SubjectContent::Rectangle { radius, .. } =
                        &mut film.sequences[0].beats[0].shots[0].subjects[side].content
                    {
                        *radius = 1.0;
                    }
                }
                1 => {
                    film.sequences[0].beats[0].shots[0].subjects[side].content =
                        SubjectContent::Circle {
                            fill: "#334455".into(),
                            stroke: None,
                        }
                }
                2 => observed[side].clip_active = Known::Known { value: true },
                3 => observed[side].clip_active = unknown("clip-unobservable"),
                4 => observed[side].transform = unknown("transform-unobservable"),
                5 => {
                    observed[side].transform = Known::Known {
                        value: [1.0, 1e-9, 0.0, 1.0, 0.0, 0.0],
                    }
                }
                6 => {
                    observed[side].transform = Known::Known {
                        value: [1.0, 0.0, -1e-9, 1.0, 0.0, 0.0],
                    }
                }
                _ => observed[side].bounds = unknown("bounds-unobservable"),
            }
            assert_eq!(
                rule_verdict(&measured(&film, repeated(observed)), "shot:constraint:0"),
                c::Verdict::Unknown,
                "side={side} change={change}"
            );
        }
        let mut observed = observations.clone();
        observed[side].transform = Known::Known {
            value: [1.0, -0.5e-9, 0.5e-9, 1.0, 0.0, 0.0],
        };
        assert_eq!(
            rule_verdict(
                &measured(&original, repeated(observed)),
                "shot:constraint:0"
            ),
            c::Verdict::Fail
        );
    }
    assert_eq!(
        rule_verdict(
            &measured(&original, repeated(vec![observations[0].clone()])),
            "shot:constraint:0"
        ),
        c::Verdict::Unknown
    );
}

#[test]
fn aspect_ratio_uses_local_size_with_closed_tolerance_and_zero_height_fails() {
    let film = with_rule(VisualConstraint::AspectRatio {
        subject: "box_a".into(),
        ratio: 2.0,
        tolerance: 0.125,
    });
    for (width, height, expected) in [
        (64.0, 32.0, c::Verdict::Pass),
        (68.0, 32.0, c::Verdict::Pass),
        (60.0, 32.0, c::Verdict::Pass),
        (69.0, 32.0, c::Verdict::Fail),
        (59.0, 32.0, c::Verdict::Fail),
        (0.0, 0.0, c::Verdict::Fail),
        (64.0, 0.0, c::Verdict::Fail),
    ] {
        let mut observation = subject("box_a", bounds(24.0, 24.0, 10.0, 10.0));
        observation.local_size = Known::Known {
            value: Size { width, height },
        };
        assert_eq!(
            rule_verdict(
                &measured(&film, repeated(vec![observation])),
                "shot:constraint:0"
            ),
            expected,
            "{width}/{height}"
        );
    }
}

#[test]
fn visibility_counts_pixel_evidence_instead_of_draw_calls() {
    let film = with_rule(VisualConstraint::MinimumVisible {
        subject: "box_a".into(),
        duration: Q::new(2, 1).unwrap(),
    });
    for (visibility, expected) in [
        ([Some(true), Some(false), Some(true)], c::Verdict::Pass),
        ([Some(true), Some(false), Some(false)], c::Verdict::Fail),
        ([Some(true), None, Some(false)], c::Verdict::Unknown),
    ] {
        let frames = visibility
            .into_iter()
            .enumerate()
            .map(|(index, pixel)| {
                let mut value = frame(index as u64);
                let mut o = subject("box_a", bounds(24.0, 24.0, 10.0, 10.0));
                o.drawn = index != 1;
                o.pixel_visibility = pixel.map_or_else(
                    || unknown("pixel-unobservable"),
                    |value| Known::Known { value },
                );
                value.subjects = vec![o];
                value
            })
            .collect();
        let result = measured(&film, frames);
        let metric = result
            .metrics
            .iter()
            .find(|m| m.subject == "box_a")
            .unwrap();
        assert_eq!(metric.observed_frames, 3);
        assert_eq!(metric.native_draw_frames, 2);
        assert_eq!(
            metric.pixel_visible_frames,
            visibility.iter().filter(|v| **v == Some(true)).count() as u64
        );
        assert_eq!(
            metric.pixel_visibility_unknown_frames,
            visibility.iter().filter(|v| v.is_none()).count() as u64
        );
        assert_eq!(rule_verdict(&result, "shot:constraint:0"), expected);
        if expected != c::Verdict::Pass {
            assert_eq!(result.findings[0].affected_frames, 1);
        }
    }
    let observations = repeated(vec![subject("box_a", bounds(24.0, 24.0, 10.0, 10.0))]);
    for indices in [vec![0, 2], vec![0, 1], vec![1, 2]] {
        let mut range = coverage();
        range.first_frame = *indices.first().unwrap();
        range.end_frame_exclusive = indices.last().unwrap() + 1;
        let result = measure(
            &film,
            c::Digest::of_bytes(b"plan"),
            base(),
            range,
            indices
                .into_iter()
                .map(|i| Ok(observations[i as usize].clone())),
        )
        .unwrap();
        assert_eq!(
            rule_verdict(&result, "shot:constraint:0"),
            c::Verdict::Unknown
        );
    }
}

#[test]
fn kinematic_metrics_use_changing_native_sizes_two_axes_and_frame_rate() {
    let mut film = film();
    film.output.frame_rate = Rate::new(2, 1).unwrap();
    let values = [
        bounds(7.0, 16.0, 6.0, 8.0),
        bounds(9.0, 19.0, 8.0, 10.0),
        bounds(14.0, 26.0, 10.0, 12.0),
        bounds(16.0, 29.0, 12.0, 14.0),
    ];
    for count in 2..=4 {
        let mut range = coverage();
        range.fps = film.output.frame_rate;
        range.end_frame_exclusive = count;
        let frames = values[..count as usize].iter().enumerate().map(|(i, b)| {
            let mut value = frame(i as u64);
            value.subjects = vec![subject("box_a", *b)];
            Ok(value)
        });
        let result = measure(&film, c::Digest::of_bytes(b"plan"), base(), range, frames).unwrap();
        let metric = result
            .metrics
            .iter()
            .find(|m| m.subject == "box_a")
            .unwrap();
        let expected_speed = if count == 2 { 10.0 } else { 20.0 };
        assert!((metric.maximum_speed.unwrap() - expected_speed).abs() < 1e-9);
        assert_eq!(
            metric.maximum_acceleration,
            if count == 2 { None } else { Some(20.0) }
        );
        assert_eq!(metric.kinematics_unknown_frames, 0);
    }
}

#[test]
fn complete_consecutive_speed_evidence_can_pass_and_incomplete_evidence_cannot() {
    let film = with_rule(VisualConstraint::MaximumSpeed {
        subject: "box_a".into(),
        pixels_per_second: 10.0,
    });
    let make = |index| {
        let mut value = frame(index);
        value.subjects = vec![subject(
            "box_a",
            bounds(24.0 + 10.0 * index as f64, 24.0, 10.0, 10.0),
        )];
        value
    };
    assert_eq!(
        rule_verdict(
            &measured(&film, (0..3).map(make).collect()),
            "shot:constraint:0"
        ),
        c::Verdict::Pass
    );
    let result = measured(&film, vec![make(0), make(2)]);
    let metric = result
        .metrics
        .iter()
        .find(|m| m.subject == "box_a")
        .unwrap();
    assert_eq!(metric.kinematics_unknown_frames, 1);
    assert_eq!(metric.maximum_speed, None);
    assert_eq!(
        rule_verdict(&result, "shot:constraint:0"),
        c::Verdict::Unknown
    );
    let mut missing = make(1);
    missing.subjects[0].bounds = unknown("bounds-unobservable");
    let result = measured(&film, vec![make(0), missing, make(2)]);
    assert_eq!(
        result
            .metrics
            .iter()
            .find(|m| m.subject == "box_a")
            .unwrap()
            .kinematics_unknown_frames,
        1
    );
    assert_eq!(
        rule_verdict(&result, "shot:constraint:0"),
        c::Verdict::Unknown
    );
    let mut range = coverage();
    range.end_frame_exclusive = 1;
    let result = measure(
        &film,
        c::Digest::of_bytes(b"plan"),
        base(),
        range,
        [Ok(make(0))],
    )
    .unwrap();
    assert_eq!(
        rule_verdict(&result, "shot:constraint:0"),
        c::Verdict::Unknown
    );
    let mut missing_last = make(2);
    missing_last.subjects[0].bounds = unknown("bounds-unobservable");
    let result = measured(&film, vec![make(0), make(1), missing_last]);
    let metric = result
        .metrics
        .iter()
        .find(|m| m.subject == "box_a")
        .unwrap();
    assert_eq!(metric.maximum_speed, Some(10.0));
    assert_eq!(metric.kinematics_unknown_frames, 1);
    assert_eq!(
        rule_verdict(&result, "shot:constraint:0"),
        c::Verdict::Unknown
    );
}

#[test]
fn a_gap_resets_acceleration_and_cannot_pass_a_speed_rule_with_prior_known_velocity() {
    let mut film = with_rule(VisualConstraint::MaximumSpeed {
        subject: "box_a".into(),
        pixels_per_second: 100.0,
    });
    film.output.frame_rate = Rate::new(2, 1).unwrap();
    let mut range = coverage();
    range.fps = film.output.frame_rate;
    range.end_frame_exclusive = 6;
    let frames = [(0, 20.0), (1, 25.0), (3, 30.0), (4, 50.0), (5, 55.0)]
        .into_iter()
        .map(|(i, x)| {
            let mut value = frame(i);
            value.subjects = vec![subject("box_a", bounds(x, 30.0, 10.0, 10.0))];
            Ok(value)
        });
    let result = measure(&film, c::Digest::of_bytes(b"plan"), base(), range, frames).unwrap();
    let metric = result
        .metrics
        .iter()
        .find(|m| m.subject == "box_a")
        .unwrap();
    assert_eq!(metric.maximum_speed, Some(40.0));
    assert_eq!(metric.maximum_acceleration, Some(60.0));
    assert_eq!(metric.kinematics_unknown_frames, 1);
    assert_eq!(
        rule_verdict(&result, "shot:constraint:0"),
        c::Verdict::Unknown
    );
}

#[test]
fn an_unknown_frame_does_not_replace_the_reason_for_a_proven_failure() {
    let film = with_rule(VisualConstraint::SafeArea {
        subject: "box_a".into(),
        tolerance: 0.0,
    });
    let mut failed = frame(0);
    failed.subjects = vec![subject("box_a", bounds(0.0, 0.0, 10.0, 10.0))];
    let mut passed = frame(2);
    passed.subjects = vec![subject("box_a", bounds(24.0, 24.0, 10.0, 10.0))];
    let result = measured(&film, vec![failed, frame(1), passed]);
    assert_eq!(rule_verdict(&result, "shot:constraint:0"), c::Verdict::Fail);
    assert_eq!(result.findings[0].first_frame, Some(0));
    assert_eq!(result.findings[0].affected_frames, 2);
    assert_eq!(
        result.findings[0].reason,
        "native transformed bounds exceed declared safe area"
    );
}

#[test]
fn measurement_ranges_reject_empty_mismatched_and_outside_native_frames() {
    let film = film();
    for (first, end, fps, frames) in [
        (0, 0, Rate::new(1, 1).unwrap(), vec![]),
        (0, 3, Rate::new(2, 1).unwrap(), vec![]),
        (0, 4, Rate::new(1, 1).unwrap(), vec![]),
        (1, 3, Rate::new(1, 1).unwrap(), vec![frame(0)]),
        (0, 3, Rate::new(1, 1).unwrap(), vec![frame(3)]),
        (0, 3, Rate::new(1, 1).unwrap(), vec![frame(1), frame(0)]),
        (0, 3, Rate::new(1, 1).unwrap(), vec![frame(1), frame(1)]),
    ] {
        let mut range = coverage();
        range.first_frame = first;
        range.end_frame_exclusive = end;
        range.fps = fps;
        assert!(
            measure(
                &film,
                c::Digest::of_bytes(b"plan"),
                base(),
                range,
                frames.into_iter().map(Ok)
            )
            .is_err()
        );
    }
    for indices in [vec![], vec![1, 2], vec![0, 1], vec![0, 2]] {
        let result = measured(&film, indices.into_iter().map(frame).collect());
        assert!(!result.coverage.exhaustive);
        assert_eq!(
            rule_verdict(&result, "native-frame-coverage"),
            c::Verdict::Unknown
        );
        assert!(
            result
                .validation
                .checks
                .iter()
                .all(|check| !check.evidence[0].exhaustive)
        );
    }
}

fn text_observation() -> TextObservation {
    TextObservation {
        logical_text_digest: c::Digest::of_bytes(b"text"),
        observed_text_digest: c::Digest::of_bytes(b"text"),
        truncated: Known::Known { value: false },
        lines: Known::Known { value: 1 },
        font_family: Known::Known {
            value: "Instrument Sans Variable".into(),
        },
        font_ready: Known::Known { value: true },
        fallback_used: Known::Known { value: false },
        direction: Known::Known {
            value: "ltr".into(),
        },
    }
}

#[test]
fn native_text_and_truncation_require_matching_observed_evidence() {
    for rule in [
        VisualConstraint::NativeText {
            subject: "label".into(),
        },
        VisualConstraint::NoTruncation {
            subject: "label".into(),
        },
    ] {
        let film = with_rule(rule.clone());
        let mut o = subject("label", bounds(24.0, 24.0, 100.0, 40.0));
        o.text = Some(text_observation());
        assert_eq!(
            rule_verdict(
                &measured(&film, repeated(vec![o.clone()])),
                "shot:constraint:0"
            ),
            c::Verdict::Pass
        );
        let text = o.text.as_mut().unwrap();
        text.observed_text_digest = c::Digest::of_bytes(b"other-text");
        text.truncated = Known::Known { value: true };
        assert_eq!(
            rule_verdict(
                &measured(&film, repeated(vec![o.clone()])),
                "shot:constraint:0"
            ),
            c::Verdict::Fail
        );
        o.text = None;
        let expected = if matches!(rule, VisualConstraint::NativeText { .. }) {
            c::Verdict::Fail
        } else {
            c::Verdict::Unknown
        };
        assert_eq!(
            rule_verdict(&measured(&film, repeated(vec![o])), "shot:constraint:0"),
            expected
        );
    }
}

#[test]
fn captions_obey_half_open_cues_and_any_wrong_frame_remains_a_failure() {
    use semwright_media_time::{Anchor, Cue};
    let mut film = film();
    film.cues.cues.push(Cue {
        id: "speech".into(),
        anchor: Anchor::Absolute {
            time: Q::new(1, 1).unwrap(),
        },
        duration: Q::new(1, 1).unwrap(),
        source: c::Digest::of_bytes(b"audio"),
        method: "alignment".into(),
        version: 1,
        confidence: None,
    });
    film.sequences[0].beats[0].shots[0].captions.push(Caption {
        id: "caption".into(),
        subject: "label".into(),
        cue_id: "speech".into(),
        language: "en".into(),
        text: "caption".into(),
    });
    let correct = (0..3)
        .map(|index| {
            let mut value = frame(index);
            value.captions.push(CaptionObservation {
                id: "caption".into(),
                active: Known::Known { value: index == 1 },
                cue_start: Known::Known {
                    value: Q::new(1, 1).unwrap(),
                },
                cue_end: Known::Known {
                    value: Q::new(2, 1).unwrap(),
                },
            });
            value
        })
        .collect::<Vec<_>>();
    assert_eq!(
        rule_verdict(&measured(&film, correct.clone()), "caption-timing"),
        c::Verdict::Pass
    );
    for index in 0..3 {
        for change in 0..4 {
            let mut frames = correct.clone();
            match change {
                0 => frames[index].captions[0].active = Known::Known { value: index != 1 },
                1 => frames[index].captions[0].active = unknown("caption-unobservable"),
                2 => frames[index].captions.clear(),
                _ => frames[index].captions[0].id = "other-caption".into(),
            }
            assert_eq!(
                rule_verdict(&measured(&film, frames), "caption-timing"),
                c::Verdict::Fail,
                "index={index} change={change}"
            );
        }
        let mut frames = correct.clone();
        frames[index].unresolved_cues.push("unresolved".into());
        assert_eq!(
            rule_verdict(&measured(&film, frames), "resolved-cues"),
            c::Verdict::Fail
        );
    }
    film.cues.cues[0].anchor = Anchor::Unknown {
        reason: "unavailable".into(),
    };
    assert_eq!(
        rule_verdict(&measured(&film, correct), "resolved-cues"),
        c::Verdict::Fail
    );
}

#[test]
fn observed_incomplete_transition_after_its_interval_is_a_failure() {
    let mut film = film();
    film.sequences[0].beats[0].shots[0].motion.push(Invocation {
        id: "motion".into(),
        span_id: "motion-span".into(),
        easing: MotionEasing::Linear,
        primitive: Primitive::FadeIn {
            target: "box_a".into(),
        },
    });
    let mut frames = vec![frame(0), frame(1), frame(2)];
    frames[2].transitions.push(TransitionObservation {
        invocation_id: "motion".into(),
        finished: Known::Known { value: false },
    });
    assert_eq!(
        rule_verdict(&measured(&film, frames), "transition-completion"),
        c::Verdict::Fail
    );
}
