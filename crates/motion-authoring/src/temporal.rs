use schemars::JsonSchema;
use semwright_media_time::{CueGraph, Interval, Rational as Q, ResolvedCue, Round};
use semwright_semantic_composition::{ContractError as Error, Result, bounded_id, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum StartAnchor {
    Free,
    Absolute { time: Q },
    After { span: String, gap: Q },
    Cue { cue: String, offset: Q },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemporalSpan {
    pub id: String,
    pub minimum: Q,
    pub preferred: Q,
    pub maximum: Q,
    pub anchor: StartAnchor,
    #[serde(default)]
    pub preference_priority: u16,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TemporalConstraint {
    Contains {
        id: String,
        parent: String,
        child: String,
    },
    Precedes {
        id: String,
        before: String,
        after: String,
        minimum_gap: Q,
        maximum_gap: Option<Q>,
    },
    StartTogether {
        id: String,
        left: String,
        right: String,
    },
    EndTogether {
        id: String,
        left: String,
        right: String,
    },
    Duration {
        id: String,
        span: String,
        duration: Q,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemporalGraph {
    pub version: u32,
    pub duration: Q,
    pub spans: Vec<TemporalSpan>,
    #[serde(default)]
    pub constraints: Vec<TemporalConstraint>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UnmetPreference {
    pub span: String,
    pub requested: Q,
    pub actual: Q,
    pub reason: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Schedule {
    pub version: u32,
    pub duration: Q,
    pub spans: BTreeMap<String, Interval>,
    pub unmet_preferences: Vec<UnmetPreference>,
    pub solver: String,
    pub relaxations: u64,
}
impl Schedule {
    pub fn interval(&self, id: &str) -> Result<Interval> {
        self.spans
            .get(id)
            .copied()
            .ok_or_else(|| Error::Invalid(format!("unknown temporal span {id}")))
    }
    pub fn frame_count(&self, output: &crate::OutputProfile) -> Result<u64> {
        let count = output
            .frame_rate
            .quantize(self.duration, Round::NearestAway)?;
        ensure(
            count.error == Q::ZERO,
            "delivery duration does not end on an exact frame boundary",
        )?;
        u64::try_from(count.index).map_err(|_| Error::Invalid("negative frame count".into()))
    }
}
#[derive(Clone)]
struct Edge {
    from: usize,
    to: usize,
    bound: Q,
    label: String,
}
fn neg(q: Q) -> Result<Q> {
    Q::ZERO.checked_sub(q)
}
fn equal(edges: &mut Vec<Edge>, from: usize, to: usize, offset: Q, label: &str) -> Result<()> {
    edges.push(Edge {
        from,
        to,
        bound: offset,
        label: label.into(),
    });
    edges.push(Edge {
        from: to,
        to: from,
        bound: neg(offset)?,
        label: label.into(),
    });
    Ok(())
}
fn solve_edges(n: usize, edges: &[Edge], budget: &mut u64) -> Result<Vec<Q>> {
    let mut dist = vec![Q::ZERO; n];
    let mut pred: Vec<Option<(usize, String)>> = vec![None; n];
    for round in 0..n {
        let mut changed = None;
        for edge in edges {
            *budget = budget
                .checked_add(1)
                .ok_or_else(|| Error::Limit("solver budget overflow".into()))?;
            if *budget > 5_000_000 {
                return Err(Error::Limit(
                    "temporal solver relaxation budget exhausted before mutation".into(),
                ));
            }
            let candidate = dist[edge.from].checked_add(edge.bound)?;
            if candidate < dist[edge.to] {
                dist[edge.to] = candidate;
                pred[edge.to] = Some((edge.from, edge.label.clone()));
                changed = Some(edge.to);
            }
        }
        if changed.is_none() {
            let origin = dist[0];
            return dist.into_iter().map(|v| v.checked_sub(origin)).collect();
        }
        if round == n - 1 {
            let mut current = changed.expect("changed");
            for _ in 0..n {
                current = pred[current].as_ref().map(|p| p.0).unwrap_or(current);
            }
            let mut witness = BTreeSet::new();
            let start = current;
            for _ in 0..n {
                let Some((previous, label)) = &pred[current] else {
                    break;
                };
                witness.insert(label.clone());
                current = *previous;
                if current == start {
                    break;
                }
            }
            return Err(Error::Invalid(format!(
                "contradictory temporal constraints: {}",
                witness.into_iter().collect::<Vec<_>>().join(", ")
            )));
        }
    }
    Err(Error::Limit("solver loop bound".into()))
}
impl TemporalGraph {
    pub fn solve(&self, cues: &CueGraph) -> Result<Schedule> {
        ensure(
            self.version == 1
                && !self.spans.is_empty()
                && self.spans.len() <= 128
                && self.constraints.len() <= 512,
            "temporal graph size/version",
        )?;
        self.duration.validate()?;
        ensure(
            self.duration > Q::ZERO && self.duration <= Q::new(600, 1)?,
            "delivery duration bound",
        )?;
        let resolved = cues.resolve()?;
        let mut map = BTreeMap::new();
        for (i, s) in self.spans.iter().enumerate() {
            bounded_id(&s.id)?;
            ensure(
                map.insert(s.id.clone(), (i * 2 + 1, i * 2 + 2)).is_none(),
                "duplicate temporal span",
            )?;
            for q in [s.minimum, s.preferred, s.maximum] {
                q.validate()?;
            }
            ensure(
                s.minimum > Q::ZERO
                    && s.minimum <= s.preferred
                    && s.preferred <= s.maximum
                    && s.maximum <= self.duration,
                "invalid minimum/preferred/maximum duration",
            )?;
        }
        // Semantic after-anchors form a DAG; STN upper/lower-bound reverse edges
        // are mathematical constraints, not recursive execution dependencies.
        for s in &self.spans {
            let mut seen = BTreeSet::new();
            let mut next = Some(s.id.as_str());
            while let Some(id) = next {
                ensure(seen.insert(id), "after-anchor dependency cycle")?;
                let node = self
                    .spans
                    .iter()
                    .find(|s| s.id == id)
                    .ok_or_else(|| Error::Invalid("missing after-anchor span".into()))?;
                next = if let StartAnchor::After { span, .. } = &node.anchor {
                    Some(span.as_str())
                } else {
                    None
                };
            }
        }
        let get = |id: &str| {
            map.get(id)
                .copied()
                .ok_or_else(|| Error::Invalid(format!("unknown constraint span {id}")))
        };
        let mut edges = vec![];
        for s in &self.spans {
            let (start, end) = get(&s.id)?;
            edges.push(Edge {
                from: start,
                to: 0,
                bound: Q::ZERO,
                label: format!("{}:nonnegative", s.id),
            });
            edges.push(Edge {
                from: 0,
                to: end,
                bound: self.duration,
                label: format!("{}:horizon", s.id),
            });
            edges.push(Edge {
                from: end,
                to: start,
                bound: neg(s.minimum)?,
                label: format!("{}:minimum", s.id),
            });
            edges.push(Edge {
                from: start,
                to: end,
                bound: s.maximum,
                label: format!("{}:maximum", s.id),
            });
            match &s.anchor {
                StartAnchor::Free => {}
                StartAnchor::Absolute { time } => {
                    time.validate()?;
                    equal(&mut edges, 0, start, *time, &format!("{}:absolute", s.id))?;
                }
                StartAnchor::After { span, gap } => {
                    gap.validate()?;
                    let (_, previous) = get(span)?;
                    equal(
                        &mut edges,
                        previous,
                        start,
                        *gap,
                        &format!("{}:after:{span}", s.id),
                    )?;
                }
                StartAnchor::Cue { cue, offset } => {
                    offset.validate()?;
                    match resolved.get(cue) {
                        Some(ResolvedCue::Resolved { start: time, .. }) => equal(
                            &mut edges,
                            0,
                            start,
                            time.checked_add(*offset)?,
                            &format!("{}:cue:{cue}", s.id),
                        )?,
                        _ => return Err(Error::Unknown(format!("unresolved required cue {cue}"))),
                    }
                }
            }
        }
        let mut constraint_ids = BTreeSet::new();
        for c in &self.constraints {
            let id = match c {
                TemporalConstraint::Contains { id, .. }
                | TemporalConstraint::Precedes { id, .. }
                | TemporalConstraint::StartTogether { id, .. }
                | TemporalConstraint::EndTogether { id, .. }
                | TemporalConstraint::Duration { id, .. } => id,
            };
            bounded_id(id)?;
            ensure(constraint_ids.insert(id), "duplicate temporal constraint")?;
            match c {
                TemporalConstraint::Contains { parent, child, .. } => {
                    let (ps, pe) = get(parent)?;
                    let (cs, ce) = get(child)?;
                    edges.push(Edge {
                        from: cs,
                        to: ps,
                        bound: Q::ZERO,
                        label: id.clone(),
                    });
                    edges.push(Edge {
                        from: pe,
                        to: ce,
                        bound: Q::ZERO,
                        label: id.clone(),
                    });
                }
                TemporalConstraint::Precedes {
                    before,
                    after,
                    minimum_gap,
                    maximum_gap,
                    ..
                } => {
                    minimum_gap.validate()?;
                    let (_, end) = get(before)?;
                    let (start, _) = get(after)?;
                    edges.push(Edge {
                        from: start,
                        to: end,
                        bound: neg(*minimum_gap)?,
                        label: id.clone(),
                    });
                    if let Some(max) = maximum_gap {
                        max.validate()?;
                        ensure(max >= minimum_gap, "gap bounds inverted")?;
                        edges.push(Edge {
                            from: end,
                            to: start,
                            bound: *max,
                            label: id.clone(),
                        });
                    }
                }
                TemporalConstraint::StartTogether { left, right, .. } => {
                    equal(&mut edges, get(left)?.0, get(right)?.0, Q::ZERO, id)?
                }
                TemporalConstraint::EndTogether { left, right, .. } => {
                    equal(&mut edges, get(left)?.1, get(right)?.1, Q::ZERO, id)?
                }
                TemporalConstraint::Duration { span, duration, .. } => {
                    duration.validate()?;
                    let (s, e) = get(span)?;
                    equal(&mut edges, s, e, *duration, id)?;
                }
            }
        }
        let mut budget = 0;
        let mut solution = solve_edges(map.len() * 2 + 1, &edges, &mut budget)?;
        let mut preferences = self.spans.iter().collect::<Vec<_>>();
        preferences.sort_by(|a, b| {
            b.preference_priority
                .cmp(&a.preference_priority)
                .then(a.id.cmp(&b.id))
        });
        for s in preferences {
            let n = edges.len();
            let (start, end) = get(&s.id)?;
            equal(
                &mut edges,
                start,
                end,
                s.preferred,
                &format!("{}:preferred", s.id),
            )?;
            match solve_edges(map.len() * 2 + 1, &edges, &mut budget) {
                Ok(values) => solution = values,
                Err(Error::Invalid(_)) => edges.truncate(n),
                Err(e) => return Err(e),
            }
        }
        let mut spans = BTreeMap::new();
        let mut unmet = vec![];
        for s in &self.spans {
            let (start, end) = get(&s.id)?;
            let interval = Interval::new(solution[start], solution[end])?.nonnegative()?;
            let actual = interval.duration()?;
            if actual != s.preferred {
                unmet.push(UnmetPreference {
                    span: s.id.clone(),
                    requested: s.preferred,
                    actual,
                    reason: "hard constraints or higher-priority preferred duration".into(),
                });
            }
            spans.insert(s.id.clone(), interval);
        }
        Ok(Schedule {
            version: 1,
            duration: self.duration,
            spans,
            unmet_preferences: unmet,
            solver: "bounded-rational-stn-v1".into(),
            relaxations: budget,
        })
    }
}
