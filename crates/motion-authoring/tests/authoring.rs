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
    assert_eq!(primitives.len(), 40);
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
