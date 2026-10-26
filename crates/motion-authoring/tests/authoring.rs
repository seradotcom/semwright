use semwright_media_time::{Rate, Rational as Q, Round};
use semwright_motion_authoring::*;
use semwright_semantic_composition::{canonical_digest, strict_decode};
fn fixture() -> Film {
    strict_decode(include_bytes!(
        "../../../fixtures/composition/motion/technical.json"
    ))
    .unwrap()
}
fn q(n: i64, d: i64) -> Q {
    Q::new(n, d).unwrap()
}
#[test]
fn technical_fixture_realizes() {
    let f = fixture();
    let r = realize(&f).unwrap();
    assert_eq!(r.scenes.len(), 1);
    assert_eq!(r.schedule.frame_count(&f.output).unwrap(), 90);
    assert_eq!(r.instructions.len(), 1);
}
#[test]
fn every_grammar_primitive_realizes() {
    let primitives: Vec<Primitive> = strict_decode(include_bytes!(
        "../../../fixtures/composition/motion/grammar.json"
    ))
    .unwrap();
    let mut seen = std::collections::BTreeSet::new();
    assert_eq!(primitives.len(), 42);
    for primitive in primitives {
        assert!(seen.insert(primitive.name()));
        let mut f = fixture();
        f.sequences[0].beats[0].shots[0].motion[0].primitive = primitive;
        let r = realize(&f).unwrap();
        assert!(!r.instructions.is_empty());
    }
}
#[test]
fn all_archetypes_and_aspects_preserve_identity() {
    let f = fixture();
    for a in Archetype::ALL {
        for (aspect, w, h) in [
            (AspectFamily::Landscape, 1280, 720),
            (AspectFamily::Portrait, 720, 1280),
            (AspectFamily::Square, 720, 720),
        ] {
            let mut x = f.clone();
            x.output.aspect = aspect;
            x.output.width = w;
            x.output.height = h;
            x.sequences[0].beats[0].shots[0].archetype = a;
            let r = realize(&x).unwrap();
            assert_eq!(r.scenes[0].shots, ["shot"]);
            assert_eq!(x.shots().next().unwrap().subjects[3].id, "label");
        }
    }
}
#[test]
fn deterministic_realization() {
    let f = fixture();
    assert_eq!(
        canonical_digest(&realize(&f).unwrap()).unwrap(),
        canonical_digest(&realize(&f).unwrap()).unwrap()
    );
}
#[test]
fn injected_source_field_is_rejected() {
    let mut f = serde_json::to_value(fixture()).unwrap();
    f["execute"] = serde_json::json!("malicious");
    assert!(strict_decode::<Film>(&serde_json::to_vec(&f).unwrap()).is_err());
}
#[test]
fn duplicate_subject_rejected() {
    let mut f = fixture();
    let shot = &mut f.sequences[0].beats[0].shots[0];
    shot.subjects.push(shot.subjects[0].clone());
    assert!(realize(&f).is_err());
}
#[test]
fn self_parent_rejected() {
    let mut f = fixture();
    f.sequences[0].beats[0].shots[0].subjects[0].parent = Some("box_a".into());
    assert!(realize(&f).is_err());
}
#[test]
fn orphan_reference_rejected() {
    let mut f = fixture();
    f.sequences[0].beats[0].shots[0].motion[0].primitive = Primitive::MatchPosition {
        target: "box_a".into(),
        source: "missing".into(),
    };
    assert!(realize(&f).is_err());
}
#[test]
fn incompatible_morph_rejected() {
    let mut f = fixture();
    f.sequences[0].beats[0].shots[0].motion[0].primitive = Primitive::Morph {
        target: "path".into(),
        points: vec![Point { x: 0.0, y: 0.0 }; 3],
        closed: false,
    };
    assert!(realize(&f).is_err());
}
#[test]
fn nonfinite_values_rejected_before_canonicalization() {
    let mut f = fixture();
    f.editorial.stroke = f64::NAN;
    assert!(realize(&f).is_err());
}
#[test]
fn undeclared_assets_rejected() {
    let mut f = fixture();
    f.sequences[0].beats[0].shots[0].subjects[0].content = SubjectContent::Image {
        asset_id: "missing".into(),
        fit: MediaFit::Contain,
        ratio: 1.0,
    };
    assert!(realize(&f).is_err());
}
#[test]
fn overlapping_channel_mutations_rejected() {
    let mut f = fixture();
    let shot = &mut f.sequences[0].beats[0].shots[0];
    let mut duplicate = shot.motion[0].clone();
    duplicate.id = "other-motion".into();
    shot.motion.push(duplicate);
    assert!(realize(&f).is_err());
}
#[test]
fn contradiction_returns_constraint_witness() {
    let mut f = fixture();
    f.timing.constraints.push(TemporalConstraint::Duration {
        id: "impossible-duration".into(),
        span: "shot-span".into(),
        duration: q(2, 1),
    });
    let error = realize(&f).unwrap_err().to_string();
    assert!(error.contains("impossible-duration"), "{error}");
}
#[test]
fn preferred_duration_yields_to_hard_constraint() {
    let mut f = fixture();
    let s = &mut f.timing.spans[2];
    s.minimum = q(1, 2);
    s.maximum = q(2, 1);
    s.preferred = q(3, 2);
    f.timing.constraints.push(TemporalConstraint::Duration {
        id: "hard-duration".into(),
        span: "motion-span".into(),
        duration: q(1, 1),
    });
    let r = realize(&f).unwrap();
    assert_eq!(
        r.schedule
            .interval("motion-span")
            .unwrap()
            .duration()
            .unwrap(),
        q(1, 1)
    );
    assert_eq!(r.schedule.unmet_preferences.len(), 1);
}
#[test]
fn unknown_cue_never_becomes_time_zero() {
    let mut f = fixture();
    f.timing.spans[2].anchor = StartAnchor::Cue {
        cue: "unavailable-word".into(),
        offset: q(0, 1),
    };
    assert!(realize(&f).is_err());
}
#[test]
fn after_cycle_rejected() {
    let mut f = fixture();
    f.timing.spans[0].anchor = StartAnchor::After {
        span: "shot-span".into(),
        gap: q(0, 1),
    };
    f.timing.spans[1].anchor = StartAnchor::After {
        span: "sequence-span".into(),
        gap: q(0, 1),
    };
    assert!(realize(&f).is_err());
}
#[test]
fn fractions_remain_exact_until_render_boundary() {
    let mut f = fixture();
    f.output.frame_rate = Rate::new(30000, 1001).unwrap();
    let duration = f.output.frame_rate.at(90).unwrap();
    f.timing.duration = duration;
    for span in &mut f.timing.spans[..2] {
        span.minimum = duration;
        span.preferred = duration;
        span.maximum = duration;
    }
    let r = realize(&f).unwrap();
    assert_eq!(r.schedule.frame_count(&f.output).unwrap(), 90);
    assert_eq!(
        f.output
            .frame_rate
            .quantize(duration, Round::NearestAway)
            .unwrap()
            .error,
        Q::ZERO
    );
}
#[test]
fn delivery_duration_cannot_silently_round() {
    let mut f = fixture();
    f.output.frame_rate = Rate::new(30000, 1001).unwrap();
    let r = realize(&f).unwrap();
    assert!(r.schedule.frame_count(&f.output).is_err());
}
#[test]
fn unannounced_overlap_rejected() {
    let mut f = fixture();
    f.sequences[0].beats[0].shots[0].subjects[0].layout = SpatialIntent::Overlay {
        anchor: "box_b".into(),
        offset: Point { x: 0.0, y: 0.0 },
        intentional: false,
    };
    assert!(realize(&f).is_err());
}
#[test]
fn source_binding_detects_drift() {
    let f = fixture();
    let r = realize(&f).unwrap();
    let mut b = ManagedBinding {
        schema_version: 1,
        intent_digest: canonical_digest(&f).unwrap(),
        realization_digest: canonical_digest(&r).unwrap(),
        projection_digest: canonical_digest(&"projection").unwrap(),
        intent: f,
        realization: r,
    };
    b.validate().unwrap();
    b.intent.editorial.stroke = 3.0;
    assert!(b.validate().is_err());
}

fn geometry_film(primitive: Primitive) -> Film {
    let mut film = fixture();
    film.output.width = 1920;
    film.output.height = 1080;
    film.timing.duration = q(6, 1);
    for span in &mut film.timing.spans[..2] {
        span.minimum = q(6, 1);
        span.preferred = q(6, 1);
        span.maximum = q(6, 1);
    }
    film.timing.spans[2].anchor = StartAnchor::Absolute { time: q(3, 1) };
    let shot = &mut film.sequences[0].beats[0].shots[0];
    shot.constraints.clear();
    shot.motion[0].easing = MotionEasing::Linear;
    shot.motion[0].primitive = primitive;
    film
}
fn geometry_value(channel: Channel) -> Operand {
    match channel {
        Channel::Position => Operand::Vector(Point {
            x: -260.0,
            y: -340.0,
        }),
        Channel::Scale => Operand::Vector(Point { x: 1.05, y: 0.95 }),
        Channel::Rotation => Operand::Number(8.0),
        Channel::Width => Operand::Number(320.0),
        Channel::Height => Operand::Number(140.0),
        _ => Operand::Number(1.0),
    }
}
#[test]
fn geometry_set_keeps_its_start_and_has_zero_native_duration() {
    let film = geometry_film(Primitive::GeometrySet {
        target: "box_a".into(),
        channel: Channel::Width,
        value: Operand::Number(320.0),
    });
    let realized = realize(&film).unwrap();
    assert_eq!(realized.schedule.frame_count(&film.output).unwrap(), 180);
    assert_eq!(
        realized
            .schedule
            .interval("motion-span")
            .unwrap()
            .duration()
            .unwrap(),
        q(1, 1)
    );
    assert_eq!(realized.instructions.len(), 1);
    let instruction = &realized.instructions[0];
    assert_eq!(instruction.start, q(3, 1));
    assert_eq!(instruction.duration, Q::ZERO);
    assert_eq!(instruction.sequence, "sequence");
    assert_eq!(
        instruction.operation,
        NativeOp::Set {
            target: "box_a".into(),
            channel: Channel::Width,
            value: Operand::Number(320.0),
        }
    );
}
#[test]
fn geometry_tween_preserves_local_endpoints_and_the_exact_span() {
    let from = Operand::Vector(Point {
        x: -260.0,
        y: -340.0,
    });
    let to = Operand::Vector(Point {
        x: -260.0,
        y: -320.0,
    });
    let film = geometry_film(Primitive::GeometryTween {
        target: "box_a".into(),
        channel: Channel::Position,
        from: from.clone(),
        to: to.clone(),
    });
    let realized = realize(&film).unwrap();
    assert_eq!(realized.instructions.len(), 1);
    let instruction = &realized.instructions[0];
    assert_eq!(instruction.start, q(3, 1));
    assert_eq!(instruction.duration, q(1, 1));
    assert_eq!(instruction.easing, MotionEasing::Linear);
    assert_eq!(
        instruction.operation,
        NativeOp::Tween {
            target: "box_a".into(),
            channel: Channel::Position,
            from: Some(from),
            to,
        }
    );
}
#[test]
fn geometry_accepts_each_declared_channel_without_implicit_extra_writes() {
    for channel in [
        Channel::Position,
        Channel::Scale,
        Channel::Rotation,
        Channel::Width,
        Channel::Height,
    ] {
        let value = geometry_value(channel);
        for primitive in [
            Primitive::GeometrySet {
                target: "box_a".into(),
                channel,
                value: value.clone(),
            },
            Primitive::GeometryTween {
                target: "box_a".into(),
                channel,
                from: value.clone(),
                to: value.clone(),
            },
        ] {
            let realized = realize(&geometry_film(primitive)).unwrap();
            assert_eq!(realized.instructions.len(), 1);
            assert_eq!(realized.instructions[0].operation.writes().len(), 1);
        }
    }
}
#[test]
fn geometry_rejects_nonfinite_out_of_range_and_wrong_typed_literals() {
    let invalid = [
        (
            Channel::Position,
            Operand::Vector(Point {
                x: f64::NAN,
                y: 0.0,
            }),
        ),
        (
            Channel::Position,
            Operand::Vector(Point { x: 0.0, y: 32769.0 }),
        ),
        (Channel::Position, Operand::Number(1.0)),
        (Channel::Scale, Operand::Vector(Point { x: 0.0, y: 1.0 })),
        (Channel::Scale, Operand::Vector(Point { x: 1.0, y: -1.0 })),
        (Channel::Scale, Operand::Vector(Point { x: 10.01, y: 1.0 })),
        (
            Channel::Scale,
            Operand::Vector(Point {
                x: 1.0,
                y: f64::INFINITY,
            }),
        ),
        (Channel::Scale, Operand::Number(1.0)),
        (Channel::Rotation, Operand::Number(360.01)),
        (Channel::Rotation, Operand::Number(f64::NEG_INFINITY)),
        (Channel::Rotation, Operand::Vector(Point { x: 1.0, y: 1.0 })),
        (Channel::Width, Operand::Number(0.0)),
        (Channel::Width, Operand::Number(16384.01)),
        (Channel::Height, Operand::Number(-1.0)),
        (Channel::Height, Operand::Number(f64::NAN)),
    ];
    for (channel, bad) in invalid {
        let good = geometry_value(channel);
        for primitive in [
            Primitive::GeometrySet {
                target: "box_a".into(),
                channel,
                value: bad.clone(),
            },
            Primitive::GeometryTween {
                target: "box_a".into(),
                channel,
                from: bad.clone(),
                to: good.clone(),
            },
            Primitive::GeometryTween {
                target: "box_a".into(),
                channel,
                from: good,
                to: bad,
            },
        ] {
            assert!(
                realize(&geometry_film(primitive)).is_err(),
                "accepted invalid {channel:?}"
            );
        }
    }
}
#[test]
fn geometry_does_not_admit_other_channels_or_dynamic_operands() {
    for channel in [
        Channel::Y,
        Channel::WorldPosition,
        Channel::WorldScale,
        Channel::Opacity,
        Channel::LineStart,
        Channel::LineEnd,
        Channel::FontSize,
        Channel::Fill,
        Channel::LetterSpacing,
        Channel::CameraZoom,
        Channel::Code,
    ] {
        assert!(
            realize(&geometry_film(Primitive::GeometrySet {
                target: "box_a".into(),
                channel,
                value: Operand::Number(1.0),
            }))
            .is_err()
        );
    }
    for value in [
        Operand::Original,
        Operand::OriginalScale(1.0),
        Operand::OriginalOffset(Point { x: 0.0, y: 0.0 }),
        Operand::Peer {
            subject: "box_b".into(),
            channel: Channel::Position,
        },
        Operand::PeerOffset {
            subject: "box_b".into(),
            offset: Point { x: 0.0, y: 0.0 },
        },
        Operand::Exploded {
            origin: Point { x: 0.0, y: 0.0 },
            spread: 1.0,
        },
        Operand::Text("not geometry".into()),
        Operand::Color("#ffffff".into()),
    ] {
        for primitive in [
            Primitive::GeometrySet {
                target: "box_a".into(),
                channel: Channel::Position,
                value: value.clone(),
            },
            Primitive::GeometryTween {
                target: "box_a".into(),
                channel: Channel::Position,
                from: geometry_value(Channel::Position),
                to: value,
            },
        ] {
            assert!(realize(&geometry_film(primitive)).is_err());
        }
    }
}
#[test]
fn geometry_requires_a_local_target_and_layout_capable_dimensions() {
    for target in ["missing", "camera"] {
        for channel in [Channel::Width, Channel::Height] {
            let value = geometry_value(channel);
            for primitive in [
                Primitive::GeometrySet {
                    target: target.into(),
                    channel,
                    value: value.clone(),
                },
                Primitive::GeometryTween {
                    target: target.into(),
                    channel,
                    from: value.clone(),
                    to: value.clone(),
                },
            ] {
                assert!(realize(&geometry_film(primitive)).is_err());
            }
        }
    }
    assert!(
        realize(&geometry_film(Primitive::GeometrySet {
            target: "missing".into(),
            channel: Channel::Position,
            value: geometry_value(Channel::Position),
        }))
        .is_err()
    );
}
#[test]
fn geometry_keeps_positive_contained_spans_and_channel_conflict_guards() {
    let primitive = Primitive::GeometryTween {
        target: "box_a".into(),
        channel: Channel::Position,
        from: geometry_value(Channel::Position),
        to: geometry_value(Channel::Position),
    };
    let mut zero = geometry_film(primitive.clone());
    zero.timing.spans[2].minimum = Q::ZERO;
    zero.timing.spans[2].preferred = Q::ZERO;
    zero.timing.spans[2].maximum = Q::ZERO;
    assert!(realize(&zero).is_err());
    let mut outside = geometry_film(primitive.clone());
    outside.timing.spans[2].anchor = StartAnchor::Absolute { time: q(6, 1) };
    assert!(realize(&outside).is_err());
    let mut overlapping = geometry_film(primitive);
    let shot = &mut overlapping.sequences[0].beats[0].shots[0];
    let mut duplicate = shot.motion[0].clone();
    duplicate.id = "overlapping-geometry".into();
    shot.motion.push(duplicate);
    assert!(realize(&overlapping).is_err());
}
#[test]
fn geometry_wire_remains_closed_to_extra_native_operations() {
    let primitive = Primitive::GeometrySet {
        target: "box_a".into(),
        channel: Channel::Width,
        value: Operand::Number(320.0),
    };
    let mut value = serde_json::to_value(primitive).unwrap();
    value["operation"] = serde_json::json!({"operation":"set","channel":"opacity","value":1});
    assert!(strict_decode::<Primitive>(&serde_json::to_vec(&value).unwrap()).is_err());
}
