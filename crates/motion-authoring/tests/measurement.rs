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
