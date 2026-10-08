use semwright_media_time::{Anchor, Cue, CueGraph, Rational as Q};
use semwright_motion_authoring::*;
use semwright_semantic_composition::Digest;

fn q(n: i64, d: i64) -> Q {
    Q::new(n, d).unwrap()
}

fn graph() -> TemporalGraph {
    TemporalGraph {
        version: 1,
        duration: q(5, 1),
        spans: vec![TemporalSpan {
            id: "span".into(),
            minimum: q(1, 1),
            preferred: q(1, 1),
            maximum: q(1, 1),
            anchor: StartAnchor::Absolute { time: Q::ZERO },
            preference_priority: 0,
        }],
        constraints: vec![],
    }
}

fn cues() -> CueGraph {
    CueGraph {
        version: 1,
        cues: vec![],
    }
}

fn invalid(graph: &TemporalGraph, reason: &str) {
    let error = graph.solve(&cues()).unwrap_err().to_string();
    assert!(error.contains(reason), "{error}");
}

#[test]
fn graph_version_and_each_collection_limit_are_checked_before_solving() {
    let mut value = graph();
    value.version = 2;
    invalid(&value, "temporal graph size/version");
    let mut value = graph();
    value.spans.clear();
    invalid(&value, "temporal graph size/version");
    let mut value = graph();
    value.spans = (0..129)
        .map(|i| {
            let mut span = value.spans[0].clone();
            span.id = format!("span-{i}");
            span
        })
        .collect();
    invalid(&value, "temporal graph size/version");
    value.spans.pop();
    assert_eq!(value.solve(&cues()).unwrap().spans.len(), 128);
    let mut value = graph();
    value.constraints = (0..513)
        .map(|i| TemporalConstraint::Duration {
            id: format!("duration-{i}"),
            span: "span".into(),
            duration: q(1, 1),
        })
        .collect();
    invalid(&value, "temporal graph size/version");
    value.constraints.pop();
    assert_eq!(
        value
            .solve(&cues())
            .unwrap()
            .interval("span")
            .unwrap()
            .duration()
            .unwrap(),
        q(1, 1)
    );
}

#[test]
fn delivery_horizon_is_positive_and_has_a_closed_upper_bound() {
    for duration in [Q::ZERO, q(-1, 1), q(601, 1)] {
        let mut value = graph();
        value.duration = duration;
        invalid(&value, "delivery duration bound");
    }
    let mut value = graph();
    value.duration = q(600, 1);
    assert_eq!(value.solve(&cues()).unwrap().duration, q(600, 1));
}

#[test]
fn span_preferences_have_independent_ordered_positive_bounds() {
    for (minimum, preferred, maximum) in [
        (Q::ZERO, q(1, 1), q(1, 1)),
        (q(-1, 1), q(1, 1), q(1, 1)),
        (q(2, 1), q(1, 1), q(3, 1)),
        (q(1, 1), q(3, 1), q(2, 1)),
        (q(1, 1), q(2, 1), q(6, 1)),
    ] {
        let mut value = graph();
        value.spans[0].minimum = minimum;
        value.spans[0].preferred = preferred;
        value.spans[0].maximum = maximum;
        invalid(&value, "invalid minimum/preferred/maximum duration");
    }
    let mut value = graph();
    value.spans[0].minimum = q(5, 1);
    value.spans[0].preferred = q(5, 1);
    value.spans[0].maximum = q(5, 1);
    assert_eq!(
        value
            .solve(&cues())
            .unwrap()
            .interval("span")
            .unwrap()
            .duration()
            .unwrap(),
        q(5, 1)
    );
}

#[test]
fn after_anchors_resolve_the_named_predecessor_and_preserve_exact_gaps() {
    let mut value = graph();
    let mut child = value.spans[0].clone();
    child.id = "child".into();
    child.anchor = StartAnchor::After {
        span: "span".into(),
        gap: q(1, 2),
    };
    value.spans.push(child);
    let result = value.solve(&cues()).unwrap();
    let interval = result.interval("child").unwrap();
    assert_eq!(interval.start, q(3, 2));
    assert_eq!(interval.end, q(5, 2));
    value.spans[1].anchor = StartAnchor::After {
        span: "absent".into(),
        gap: Q::ZERO,
    };
    invalid(&value, "missing after-anchor span");
    value.spans[1].anchor = StartAnchor::After {
        span: "span".into(),
        gap: Q::ZERO,
    };
    value.spans[0].anchor = StartAnchor::After {
        span: "child".into(),
        gap: Q::ZERO,
    };
    invalid(&value, "after-anchor dependency cycle");
}

#[test]
fn resolved_cue_anchors_use_measured_time_and_offset() {
    let mut value = graph();
    value.spans[0].anchor = StartAnchor::Cue {
        cue: "speech".into(),
        offset: q(1, 2),
    };
    let cues = CueGraph {
        version: 1,
        cues: vec![Cue {
            id: "speech".into(),
            anchor: Anchor::Absolute { time: q(2, 1) },
            duration: q(1, 1),
            source: Digest::of_bytes(b"audio"),
            method: "alignment".into(),
            version: 1,
            confidence: None,
        }],
    };
    let result = value.solve(&cues).unwrap();
    assert_eq!(result.interval("span").unwrap().start, q(5, 2));
    assert_eq!(result.interval("span").unwrap().end, q(7, 2));
}

#[test]
fn precedence_accepts_equal_gap_bounds_and_rejects_inverted_bounds() {
    let mut value = graph();
    let mut next = value.spans[0].clone();
    next.id = "next".into();
    next.anchor = StartAnchor::Absolute { time: q(2, 1) };
    value.spans.push(next);
    value.constraints.push(TemporalConstraint::Precedes {
        id: "precedence".into(),
        before: "span".into(),
        after: "next".into(),
        minimum_gap: q(1, 1),
        maximum_gap: Some(q(1, 1)),
    });
    let result = value.solve(&cues()).unwrap();
    assert_eq!(
        result
            .interval("next")
            .unwrap()
            .start
            .checked_sub(result.interval("span").unwrap().end)
            .unwrap(),
        q(1, 1)
    );
    if let TemporalConstraint::Precedes { maximum_gap, .. } = &mut value.constraints[0] {
        *maximum_gap = Some(q(1, 2));
    }
    invalid(&value, "gap bounds inverted");
}
