//! Streaming native-frame verification. Desired geometry is never measurement.
use crate::*;
use c::{Digest, Result, Verdict, ensure};
use schemars::JsonSchema;
use semwright_media_time::{Rate, Rational as Q};
use semwright_semantic_composition as c;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum Known<T> {
    Known { value: T },
    Unknown { reason: String },
}
impl<T> Known<T> {
    pub fn value(&self) -> Option<&T> {
        match self {
            Self::Known { value } => Some(value),
            Self::Unknown { .. } => None,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
impl Bounds {
    fn validate(self) -> Result<()> {
        crate::validate::point(Point {
            x: self.x,
            y: self.y,
        })?;
        crate::validate::finite(self.width, 0.0, 1e6)?;
        crate::validate::finite(self.height, 0.0, 1e6)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextObservation {
    pub logical_text_digest: Digest,
    pub observed_text_digest: Digest,
    pub truncated: Known<bool>,
    pub lines: Known<u32>,
    pub font_family: Known<String>,
    pub font_ready: Known<bool>,
    pub fallback_used: Known<bool>,
    pub direction: Known<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SubjectObservation {
    pub id: String,
    pub local_size: Known<Size>,
    pub bounds: Known<Bounds>,
    pub transform: Known<[f64; 6]>,
    pub opacity: Known<f64>,
    pub drawn: bool,
    pub z_order: Known<u32>,
    pub clip_active: Known<bool>,
    pub pixel_visibility: Known<bool>,
    pub text: Option<TextObservation>,
    pub asset_ready: Known<bool>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransitionObservation {
    pub invocation_id: String,
    pub finished: Known<bool>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CaptionObservation {
    pub id: String,
    pub active: Known<bool>,
    pub cue_start: Known<Q>,
    pub cue_end: Known<Q>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrameObservation {
    pub frame: u64,
    pub subjects: Vec<SubjectObservation>,
    pub transitions: Vec<TransitionObservation>,
    pub captions: Vec<CaptionObservation>,
    pub unresolved_cues: Vec<String>,
}
impl FrameObservation {
    pub fn validate(&self) -> Result<()> {
        ensure(
            self.subjects.len() <= 1024
                && self.transitions.len() <= 1024
                && self.captions.len() <= 512
                && self.unresolved_cues.len() <= 512,
            "native observation collection budget",
        )?;
        let mut seen = BTreeSet::new();
        for subject in &self.subjects {
            crate::validate::id(&subject.id)?;
            ensure(seen.insert(&subject.id), "duplicate observed subject")?;
            if let Some(b) = subject.bounds.value() {
                b.validate()?;
            }
            if let Some(s) = subject.local_size.value() {
                crate::validate::finite(s.width, 0.0, 1e6)?;
                crate::validate::finite(s.height, 0.0, 1e6)?;
            }
            if let Some(t) = subject.transform.value() {
                for n in t {
                    crate::validate::finite(*n, -1e6, 1e6)?;
                }
            }
            if let Some(alpha) = subject.opacity.value() {
                crate::validate::finite(*alpha, 0.0, 1.0)?;
            }
        }
        let mut seen = BTreeSet::new();
        for t in &self.transitions {
            crate::validate::id(&t.invocation_id)?;
            ensure(
                seen.insert(&t.invocation_id),
                "duplicate observed transition",
            )?;
        }
        let mut seen = BTreeSet::new();
        for caption in &self.captions {
            crate::validate::id(&caption.id)?;
            ensure(seen.insert(&caption.id), "duplicate observed caption")?;
            for q in [caption.cue_start.value(), caption.cue_end.value()]
                .into_iter()
                .flatten()
            {
                q.validate()?;
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RangeCoverage {
    pub first_frame: u64,
    pub end_frame_exclusive: u64,
    pub observed_frames: u64,
    pub exhaustive: bool,
    pub fps: Rate,
    pub method: String,
    pub observation_digest: Digest,
    pub render_input_digest: Digest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_resources_digest: Option<Digest>,
    pub artifact_digest: Digest,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MetricSummary {
    pub subject: String,
    pub native_draw_frames: u64,
    pub pixel_visible_frames: u64,
    pub pixel_visibility_unknown_frames: u64,
    pub observed_frames: u64,
    pub maximum_speed: Option<f64>,
    pub maximum_acceleration: Option<f64>,
    pub kinematics_unknown_frames: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MotionFinding {
    pub id: String,
    pub rule: String,
    pub subject: String,
    pub verdict: Verdict,
    pub evidence_class: c::EvidenceClass,
    pub first_frame: Option<u64>,
    pub affected_frames: u64,
    pub reason: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MotionMeasurement {
    pub version: u32,
    pub intent_digest: Digest,
    pub coverage: RangeCoverage,
    pub metrics: Vec<MetricSummary>,
    pub findings: Vec<MotionFinding>,
    pub validation: c::ValidationReport,
}
struct Metric {
    summary: MetricSummary,
    previous: Option<(u64, Point, f64)>,
    known_velocity: bool,
}
struct RuleState {
    subject: String,
    failed: u64,
    unknown: u64,
    checked: u64,
    first: Option<u64>,
    reason: String,
}
fn combine(s: &mut RuleState, verdict: Verdict, frame: u64, reason: &str) {
    s.checked += 1;
    match verdict {
        Verdict::Pass => {}
        Verdict::Fail => {
            s.failed += 1;
            s.first.get_or_insert(frame);
            s.reason = reason.into();
        }
        Verdict::Unknown => {
            s.unknown += 1;
            if s.failed == 0 {
                s.reason = reason.into();
            }
        }
    }
}
fn rule_subject(rule: &VisualConstraint) -> &str {
    match rule {
        VisualConstraint::SafeArea { subject, .. }
        | VisualConstraint::NoOverlap { subject, .. }
        | VisualConstraint::MinimumVisible { subject, .. }
        | VisualConstraint::AspectRatio { subject, .. }
        | VisualConstraint::NativeText { subject }
        | VisualConstraint::FontLoaded { subject }
        | VisualConstraint::NoTruncation { subject }
        | VisualConstraint::MaximumSpeed { subject, .. } => subject,
    }
}
fn verdict(value: bool) -> Verdict {
    if value { Verdict::Pass } else { Verdict::Fail }
}
fn rectangle(subject: &Subject) -> bool {
    matches!(subject.content,SubjectContent::Rectangle{radius,..}if radius==0.0)
}
fn axis_aligned(o: &SubjectObservation) -> bool {
    o.transform
        .value()
        .is_some_and(|m| m[1].abs() < 1e-9 && m[2].abs() < 1e-9)
}
/// `coverage` and `base` must be built from a Host-owned completed render receipt.
/// This pure function does not authenticate user JSON; production adapters must
/// never accept a claimed native stream from capability arguments.
pub fn measure<I>(
    film: &Film,
    plan_digest: Digest,
    base: c::BaseStateSet,
    mut coverage: RangeCoverage,
    frames: I,
) -> Result<MotionMeasurement>
where
    I: IntoIterator<Item = Result<FrameObservation>>,
{
    let realized = realize(film)?;
    base.validate()?;
    coverage.fps.validate()?;
    ensure(
        coverage.fps == film.output.frame_rate
            && coverage.first_frame < coverage.end_frame_exclusive,
        "measurement range/rate mismatch",
    )?;
    ensure(
        coverage.end_frame_exclusive <= realized.schedule.frame_count(&film.output)?,
        "measurement range exceeds Film",
    )?;
    let mut states = BTreeMap::new();
    let mut rules = vec![];
    let mut subjects = BTreeMap::new();
    let mut metrics = BTreeMap::new();
    for shot in film.shots() {
        for subject in &shot.subjects {
            subjects.insert(subject.id.as_str(), subject);
            metrics.insert(
                subject.id.clone(),
                Metric {
                    summary: MetricSummary {
                        subject: subject.id.clone(),
                        native_draw_frames: 0,
                        pixel_visible_frames: 0,
                        pixel_visibility_unknown_frames: 0,
                        observed_frames: 0,
                        maximum_speed: None,
                        maximum_acceleration: None,
                        kinematics_unknown_frames: 0,
                    },
                    previous: None,
                    known_velocity: false,
                },
            );
        }
        for (i, rule) in shot.constraints.iter().enumerate() {
            let name = format!("{}:constraint:{i}", shot.id);
            states.insert(
                name.clone(),
                RuleState {
                    subject: rule_subject(rule).into(),
                    failed: 0,
                    unknown: 0,
                    checked: 0,
                    first: None,
                    reason: "required native observation absent".into(),
                },
            );
            rules.push((name, shot, rule));
        }
    }
    let mut previous = None;
    let mut observed = 0;
    let mut exhaustive = true;
    let mut cue_failure = false;
    let mut caption_failure = false;
    let transition_required = film
        .shots()
        .flat_map(|shot| shot.motion.iter().map(|invocation| invocation.id.clone()))
        .collect::<BTreeSet<_>>();
    let mut transition_observed = BTreeSet::new();
    let mut transition_failure = false;
    let mut transition_unknown = false;
    let cues = film.cues.resolve()?;
    for result in frames {
        let frame = result?;
        frame.validate()?;
        ensure(
            frame.frame >= coverage.first_frame && frame.frame < coverage.end_frame_exclusive,
            "frame outside declared measurement range",
        )?;
        if let Some(prev) = previous {
            ensure(frame.frame > prev, "duplicate/unordered native frame")?;
            exhaustive &= frame.frame == prev + 1;
        } else {
            exhaustive &= frame.frame == coverage.first_frame;
        }
        previous = Some(frame.frame);
        observed += 1;
        let now = coverage.fps.at(i64::try_from(frame.frame)
            .map_err(|_| c::ContractError::Limit("frame index".into()))?)?;
        let observations = frame
            .subjects
            .iter()
            .map(|s| (s.id.as_str(), s))
            .collect::<BTreeMap<_, _>>();
        ensure(
            observations.keys().all(|id| subjects.contains_key(id)),
            "renderer produced unowned subject identity",
        )?;
        for o in &frame.subjects {
            let m = metrics.get_mut(&o.id).expect("known subject");
            m.summary.observed_frames += 1;
            if o.drawn {
                m.summary.native_draw_frames += 1;
            }
            match o.pixel_visibility.value() {
                Some(true) => m.summary.pixel_visible_frames += 1,
                Some(false) => {}
                None => m.summary.pixel_visibility_unknown_frames += 1,
            }
            if let Some(b) = o.bounds.value() {
                let p = Point {
                    x: b.x + b.width / 2.0,
                    y: b.y + b.height / 2.0,
                };
                if let Some((last, old, speed)) = m.previous {
                    if frame.frame == last + 1 {
                        let dt = f64::from(coverage.fps.den) / f64::from(coverage.fps.num);
                        let v = ((p.x - old.x).powi(2) + (p.y - old.y).powi(2)).sqrt() / dt;
                        m.summary.maximum_speed =
                            Some(m.summary.maximum_speed.map_or(v, |n| n.max(v)));
                        if m.known_velocity {
                            let a = (v - speed).abs() / dt;
                            m.summary.maximum_acceleration =
                                Some(m.summary.maximum_acceleration.map_or(a, |n| n.max(a)));
                        }
                        m.previous = Some((frame.frame, p, v));
                        m.known_velocity = true;
                    } else {
                        m.summary.kinematics_unknown_frames += 1;
                        m.previous = Some((frame.frame, p, 0.0));
                        m.known_velocity = false;
                    }
                } else {
                    m.previous = Some((frame.frame, p, 0.0));
                }
            } else {
                m.summary.kinematics_unknown_frames += 1;
                m.previous = None;
                m.known_velocity = false;
            }
        }
        for (name, shot, rule) in &rules {
            if !realized.schedule.interval(&shot.span_id)?.contains(now) {
                continue;
            }
            let state = states.get_mut(name).expect("registered rule");
            let Some(o) = observations.get(rule_subject(rule)) else {
                combine(
                    state,
                    Verdict::Unknown,
                    frame.frame,
                    "subject was not observed in its active shot",
                );
                continue;
            };
            let (v, reason) = match rule {
                VisualConstraint::SafeArea { tolerance, .. } => {
                    if let Some(b) = o.bounds.value() {
                        let safe = film.output.safe_area;
                        (
                            verdict(
                                b.x >= safe.left - tolerance
                                    && b.y >= safe.top - tolerance
                                    && b.x + b.width
                                        <= f64::from(film.output.width) - safe.right + tolerance
                                    && b.y + b.height
                                        <= f64::from(film.output.height) - safe.bottom + tolerance,
                            ),
                            "native transformed bounds exceed declared safe area",
                        )
                    } else {
                        (Verdict::Unknown, "native output bounds unobservable")
                    }
                }
                VisualConstraint::NoOverlap {
                    other, tolerance, ..
                } => match (o.bounds.value(), observations.get(other.as_str())) {
                    (Some(a), Some(other_o)) => {
                        if let Some(b) = other_o.bounds.value() {
                            let w = (a.x + a.width).min(b.x + b.width) - a.x.max(b.x);
                            let h = (a.y + a.height).min(b.y + b.height) - a.y.max(b.y);
                            if w <= *tolerance || h <= *tolerance {
                                (Verdict::Pass, "disjoint observed bounds")
                            } else if rectangle(subjects[o.id.as_str()])
                                && rectangle(subjects[other.as_str()])
                                && axis_aligned(o)
                                && axis_aligned(other_o)
                                && o.clip_active.value() == Some(&false)
                                && other_o.clip_active.value() == Some(&false)
                            {
                                (
                                    Verdict::Fail,
                                    "explicit no-overlap rectangle constraint violated",
                                )
                            } else {
                                (
                                    Verdict::Unknown,
                                    "intersecting bounds do not prove overlap of clipped/transformed geometry",
                                )
                            }
                        } else {
                            (Verdict::Unknown, "other bounds unavailable")
                        }
                    }
                    _ => (Verdict::Unknown, "overlap measurements incomplete"),
                },
                VisualConstraint::MinimumVisible { .. } => (
                    Verdict::Pass,
                    "aggregated over declared frame range after stream",
                ),
                VisualConstraint::AspectRatio {
                    ratio, tolerance, ..
                } => {
                    if let Some(s) = o.local_size.value() {
                        (
                            verdict(
                                s.height > 0.0 && (s.width / s.height - ratio).abs() <= *tolerance,
                            ),
                            "observed aspect ratio differs from declared ratio",
                        )
                    } else {
                        (Verdict::Unknown, "native local size unavailable")
                    }
                }
                VisualConstraint::NativeText { .. } => {
                    if let Some(t) = &o.text {
                        (
                            verdict(t.logical_text_digest == t.observed_text_digest),
                            "native observed text differs from declared text",
                        )
                    } else {
                        (Verdict::Fail, "subject has no native text observation")
                    }
                }
                VisualConstraint::FontLoaded { .. } => {
                    match o.text.as_ref().and_then(|t| t.font_ready.value()) {
                        Some(v) => (verdict(*v), "required font face/glyph evidence missing"),
                        None => (Verdict::Unknown, "native font readiness unobservable"),
                    }
                }
                VisualConstraint::NoTruncation { .. } => {
                    match o.text.as_ref().and_then(|t| t.truncated.value()) {
                        Some(v) => (verdict(!v), "native text metrics report clipping"),
                        None => (Verdict::Unknown, "native text clipping unobservable"),
                    }
                }
                VisualConstraint::MaximumSpeed {
                    pixels_per_second, ..
                } => {
                    let m = &metrics[o.id.as_str()].summary;
                    if m.kinematics_unknown_frames > 0 {
                        (
                            Verdict::Unknown,
                            "trajectory has missing/nonconsecutive observations",
                        )
                    } else if let Some(v) = m.maximum_speed {
                        (
                            verdict(v <= *pixels_per_second),
                            "finite-difference speed exceeds configured profile bound",
                        )
                    } else {
                        (Verdict::Unknown, "at least two frames needed for speed")
                    }
                }
            };
            combine(state, v, frame.frame, reason);
        }
        cue_failure |= !frame.unresolved_cues.is_empty();
        for shot in film.shots() {
            if !realized.schedule.interval(&shot.span_id)?.contains(now) {
                continue;
            }
            for caption in &shot.captions {
                let Some(semwright_media_time::ResolvedCue::Resolved { start, end }) =
                    cues.get(&caption.cue_id)
                else {
                    cue_failure = true;
                    continue;
                };
                let expected = *start <= now && now < *end;
                caption_failure |= frame
                    .captions
                    .iter()
                    .find(|c| c.id == caption.id)
                    .and_then(|c| c.active.value())
                    .is_none_or(|active| *active != expected);
            }
            for invocation in &shot.motion {
                let end = realized.schedule.interval(&invocation.span_id)?.end;
                // Motion Canvas generators can finish while evaluating the frame
                // exactly at the half-open interval boundary. That frame proves
                // terminal visual state, but not yet that the generator's
                // continuation set the completion receipt. Require the first
                // observed frame strictly after the interval end.
                if now > end {
                    match frame
                        .transitions
                        .iter()
                        .find(|t| t.invocation_id == invocation.id)
                        .and_then(|t| t.finished.value())
                    {
                        Some(true) => {
                            transition_observed.insert(invocation.id.clone());
                        }
                        Some(false) => transition_failure = true,
                        None => transition_unknown = true,
                    }
                }
            }
        }
    }
    exhaustive &= observed == coverage.end_frame_exclusive - coverage.first_frame
        && previous == Some(coverage.end_frame_exclusive - 1);
    transition_unknown |= !transition_required.is_subset(&transition_observed);
    // Full-Film rules cannot PASS from a partial range even when that range is exhaustive.
    let full = coverage.first_frame == 0
        && coverage.end_frame_exclusive == realized.schedule.frame_count(&film.output)?;
    coverage.observed_frames = observed;
    coverage.exhaustive = exhaustive;
    for (name, _, rule) in &rules {
        if let VisualConstraint::MinimumVisible { subject, duration } = rule {
            let m = &metrics[subject].summary;
            let state = states.get_mut(name).expect("rule");
            let visible = coverage.fps.at(m.pixel_visible_frames as i64)?;
            if m.pixel_visibility_unknown_frames > 0 || !exhaustive || !full {
                state.unknown += 1;
                state.reason="drawn opacity is not proof of visible pixel contribution; incomplete pixel evidence".into();
            } else if visible < *duration {
                state.failed += 1;
                state.reason = "measured pixel-visible duration below declared minimum".into();
            }
        }
    }
    let resource = base.0.first().expect("validated base").key.clone();
    let observation = c::ObservationRef {
        id: format!("render-{}", &coverage.observation_digest.as_str()[..16]),
        base: base.clone(),
        source: c::EvidenceSource::RendererState,
        method: coverage.method.clone(),
        method_version: 1,
        scope: vec![c::Address {
            resource: resource.clone(),
            logical_id: film.id.clone(),
            property: "native-frame-state".into(),
        }],
        artifact: Some(coverage.artifact_digest.clone()),
        exhaustive: exhaustive && full,
    };
    let mut required = BTreeSet::new();
    let mut checks = vec![];
    let mut findings = vec![];
    for (name, state) in states {
        let verdict = if state.failed > 0 {
            Verdict::Fail
        } else if state.unknown > 0 || state.checked == 0 {
            Verdict::Unknown
        } else {
            Verdict::Pass
        };
        required.insert(name.clone());
        checks.push(c::RuleResult {
            rule: name.clone(),
            version: 1,
            verdict,
            evidence_class: c::EvidenceClass::Deterministic,
            evidence: vec![observation.clone()],
            reason: if verdict == Verdict::Pass {
                None
            } else {
                Some(state.reason.clone())
            },
        });
        if verdict != Verdict::Pass {
            findings.push(MotionFinding {
                id: name.clone(),
                rule: name,
                subject: state.subject,
                verdict,
                evidence_class: c::EvidenceClass::Deterministic,
                first_frame: state.first,
                affected_frames: state.failed + state.unknown,
                reason: state.reason,
            });
        }
    }
    for (name, passed, reason) in [
        (
            "native-frame-coverage",
            exhaustive && full,
            "full requested native frame coverage missing",
        ),
        ("resolved-cues", !cue_failure, "unresolved cue observed"),
        (
            "caption-timing",
            !caption_failure,
            "caption state differs from resolved cue",
        ),
    ] {
        required.insert(name.into());
        checks.push(c::RuleResult {
            rule: name.into(),
            version: 1,
            verdict: if passed {
                Verdict::Pass
            } else if name == "native-frame-coverage" {
                Verdict::Unknown
            } else {
                Verdict::Fail
            },
            evidence_class: c::EvidenceClass::Deterministic,
            evidence: vec![observation.clone()],
            reason: if passed { None } else { Some(reason.into()) },
        });
    }
    required.insert("transition-completion".into());
    checks.push(c::RuleResult {
        rule: "transition-completion".into(),
        version: 1,
        verdict: if transition_failure {
            Verdict::Fail
        } else if transition_unknown {
            Verdict::Unknown
        } else {
            Verdict::Pass
        },
        evidence_class: c::EvidenceClass::Deterministic,
        evidence: vec![observation.clone()],
        reason: if transition_failure {
            Some("transition observed incomplete after its half-open interval".into())
        } else if transition_unknown {
            Some("no post-interval native frame proved transition completion".into())
        } else {
            None
        },
    });
    let validation = c::ValidationReport {
        plan_digest,
        base,
        required_rules: required,
        checks,
    };
    validation.verdict()?;
    Ok(MotionMeasurement {
        version: 1,
        intent_digest: c::canonical_digest(film)?,
        coverage,
        metrics: metrics.into_values().map(|m| m.summary).collect(),
        findings,
        validation,
    })
}
