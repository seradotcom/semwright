use crate::{
    DEFAULT_MIN_OCCURRENCES, MAX_MIN_OCCURRENCES, PatternDismissal, PatternVariation,
    WorkflowPattern, WorkflowTrace, looks_like_ref,
};
use semwright_types::{Error, Result};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const MAX_PATTERN_TRACES: usize = 256;
const MAX_COMPILE_TRACES: usize = 8;
const MAX_VARIATIONS: usize = 64;
const MAX_SHAPE_DEPTH: usize = 24;

fn pointer_escape(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

fn leaf(pointer: &str) -> &str {
    pointer.rsplit('/').next().unwrap_or("")
}

fn semantic_scalar_class(pointer: &str, value: &Value) -> &'static str {
    if value.as_str().is_some_and(looks_like_ref) {
        return "reference";
    }
    let name = leaf(pointer);
    if matches!(name, "ref" | "reference") {
        "reference"
    } else if matches!(
        name,
        "path" | "source_path" | "destination_path" | "resource"
    ) {
        "path"
    } else if matches!(name, "sha256" | "expected_sha256" | "digest" | "checksum") {
        "digest"
    } else if matches!(
        name,
        "revision" | "expected_revision" | "resulting_revision"
    ) {
        "revision"
    } else if matches!(name, "root" | "source_root" | "destination_root") {
        "root"
    } else if name == "id" || name == "job_id" || name == "artifact_id" || name.ends_with("_id") {
        "identity"
    } else {
        "scalar"
    }
}

fn scalar_kind(pointer: &str, value: &Value) -> String {
    let base = match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(number) if number.is_i64() || number.is_u64() => "integer",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    };
    let class = semantic_scalar_class(pointer, value);
    if class == "scalar" {
        base.into()
    } else {
        format!("{base}:{class}")
    }
}

fn value_shape(value: &Value, pointer: &str, depth: usize) -> Value {
    if depth > MAX_SHAPE_DEPTH {
        return json!({"kind":"truncated"});
    }
    match value {
        Value::Object(map) => {
            let mut ordered = BTreeMap::new();
            for (key, child) in map {
                let child_pointer = format!("{pointer}/{}", pointer_escape(key));
                ordered.insert(key.clone(), value_shape(child, &child_pointer, depth + 1));
            }
            json!({"kind":"object","fields":ordered})
        }
        Value::Array(values) => {
            let mut unique = BTreeMap::<String, Value>::new();
            for child in values.iter().take(64) {
                let shape = value_shape(child, &format!("{pointer}/*"), depth + 1);
                unique.entry(shape.to_string()).or_insert(shape);
            }
            json!({
                "kind":"array",
                "items":unique.into_values().collect::<Vec<_>>(),
                "truncated":values.len() > 64
            })
        }
        _ => json!({"kind":scalar_kind(pointer, value)}),
    }
}

pub fn trace_compile_ready(trace: &WorkflowTrace) -> bool {
    trace.successful
        && trace.capture_values
        && !trace.steps.is_empty()
        && trace.steps.iter().all(|step| {
            step.ok
                && step.outcome_known
                && !step.redacted
                && step.result.is_some()
                && step.descriptor_sha256.len() == 64
        })
}

fn signature(trace: &WorkflowTrace) -> Value {
    let steps = trace
        .steps
        .iter()
        .map(|step| {
            json!({
                "command":step.command,
                "capability_version":step.capability_version,
                "descriptor_sha256":step.descriptor_sha256,
                "risk":step.risk,
                "idempotency":step.idempotency,
                "args":value_shape(&step.args, "", 0),
                "result":step.result.as_ref()
                    .map(|value| value_shape(value, "", 0))
                    .unwrap_or_else(|| json!({"kind":"missing"})),
            })
        })
        .collect::<Vec<_>>();
    json!({"version":1,"steps":steps})
}

pub fn pattern_fingerprint(trace: &WorkflowTrace) -> Result<String> {
    let bytes = serde_json::to_vec(&signature(trace))?;
    let digest = Sha256::digest(bytes);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn valid_recipe_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn suggested_name(traces: &[&WorkflowTrace]) -> String {
    if let Some(first) = traces.first()
        && valid_recipe_name(&first.name)
        && traces.iter().all(|trace| trace.name == first.name)
    {
        return first.name.clone();
    }
    let mut pieces = Vec::new();
    if let Some(trace) = traces.first() {
        for step in trace.steps.iter().take(4) {
            let leaf = step.command.rsplit('.').next().unwrap_or("step");
            let piece = leaf
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() {
                        c.to_ascii_lowercase()
                    } else {
                        '-'
                    }
                })
                .collect::<String>()
                .trim_matches('-')
                .to_owned();
            if !piece.is_empty() && pieces.last() != Some(&piece) {
                pieces.push(piece);
            }
        }
    }
    let body = if pieces.is_empty() {
        "workflow".into()
    } else {
        pieces.join("-")
    };
    let mut name = format!("learned-{body}");
    name.truncate(80);
    while name.ends_with('-') {
        name.pop();
    }
    if valid_recipe_name(&name) {
        name
    } else {
        "learned-workflow".into()
    }
}

fn collect_scalars(value: &Value, pointer: &str, out: &mut BTreeMap<String, Value>, depth: usize) {
    if depth > MAX_SHAPE_DEPTH || out.len() >= 4096 {
        return;
    }
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                collect_scalars(
                    child,
                    &format!("{pointer}/{}", pointer_escape(key)),
                    out,
                    depth + 1,
                );
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate().take(256) {
                collect_scalars(child, &format!("{pointer}/{index}"), out, depth + 1);
            }
        }
        Value::Null => {}
        _ => {
            out.insert(pointer.to_owned(), value.clone());
        }
    }
}

fn varying_arguments(traces: &[&WorkflowTrace]) -> Vec<PatternVariation> {
    if traces.len() < 2 {
        return vec![];
    }
    let step_count = traces[0].steps.len();
    let mut variations = Vec::new();
    for step in 0..step_count {
        let mut scalar_maps = Vec::with_capacity(traces.len());
        for trace in traces {
            let mut values = BTreeMap::new();
            collect_scalars(&trace.steps[step].args, "", &mut values, 0);
            scalar_maps.push(values);
        }
        let mut pointers = BTreeSet::new();
        for values in &scalar_maps {
            pointers.extend(values.keys().cloned());
        }
        for pointer in pointers {
            if variations.len() >= MAX_VARIATIONS {
                return variations;
            }
            let Some(first) = scalar_maps[0].get(&pointer) else {
                continue;
            };
            if scalar_maps
                .iter()
                .any(|values| values.get(&pointer).is_none())
            {
                continue;
            }
            if semantic_scalar_class(&pointer, first) == "reference" {
                continue;
            }
            let kind = scalar_kind(&pointer, first);
            if scalar_maps.iter().any(|values| {
                values
                    .get(&pointer)
                    .is_some_and(|value| scalar_kind(&pointer, value) != kind)
            }) {
                continue;
            }
            let distinct = scalar_maps
                .iter()
                .filter_map(|values| values.get(&pointer))
                .map(Value::to_string)
                .collect::<BTreeSet<_>>();
            if distinct.len() > 1 {
                variations.push(PatternVariation {
                    step,
                    pointer,
                    kind,
                    observations: distinct.len(),
                });
            }
        }
    }
    variations
}

fn pattern_id(fingerprint: &str) -> String {
    format!("pattern-{}", &fingerprint[..24])
}

fn suggestion_id(fingerprint: &str) -> String {
    format!("suggestion-{}", &fingerprint[..24])
}

pub fn validate_min_occurrences(min_occurrences: usize) -> Result<()> {
    if !(2..=MAX_MIN_OCCURRENCES).contains(&min_occurrences) {
        return Err(Error::invalid(
            "Workflow pattern threshold must be between 2 and 32 occurrences",
        ));
    }
    Ok(())
}

pub fn mine_patterns(
    traces: &[WorkflowTrace],
    min_occurrences: usize,
    dismissals: &BTreeMap<String, PatternDismissal>,
) -> Result<Vec<WorkflowPattern>> {
    validate_min_occurrences(min_occurrences)?;
    let mut groups = BTreeMap::<String, Vec<&WorkflowTrace>>::new();
    for trace in traces
        .iter()
        .filter(|trace| trace.successful && !trace.steps.is_empty())
    {
        let fingerprint = pattern_fingerprint(trace)?;
        groups.entry(fingerprint).or_default().push(trace);
    }

    let mut patterns = Vec::new();
    for (fingerprint, mut group) in groups {
        if group.len() < min_occurrences {
            continue;
        }
        group.sort_by_key(|trace| (trace.ended_unix_ms, trace.id.as_str()));
        if group.len() > MAX_PATTERN_TRACES {
            group = group.split_off(group.len() - MAX_PATTERN_TRACES);
        }
        let id = pattern_id(&fingerprint);
        let suggestion = suggestion_id(&fingerprint);
        let compile_ready = group
            .iter()
            .copied()
            .filter(|trace| trace_compile_ready(trace))
            .collect::<Vec<_>>();
        let compile_trace_ids = compile_ready
            .iter()
            .rev()
            .take(MAX_COMPILE_TRACES)
            .rev()
            .map(|trace| trace.id.clone())
            .collect::<Vec<_>>();
        let occurrence_count = group.len();
        let dismissal = dismissals.get(&id);
        let dismissed = dismissal.is_some_and(|dismissal| {
            dismissal.permanent || occurrence_count <= dismissal.dismissed_through_occurrences
        });
        let resurfaced = dismissal.is_some() && !dismissed;
        patterns.push(WorkflowPattern {
            version: crate::PATTERN_VERSION,
            id,
            suggestion_id: suggestion,
            fingerprint,
            commands: group[0]
                .steps
                .iter()
                .map(|step| step.command.clone())
                .collect(),
            occurrences: occurrence_count,
            compile_ready_count: compile_ready.len(),
            trace_ids: group.iter().map(|trace| trace.id.clone()).collect(),
            compile_trace_ids,
            first_seen_unix_ms: group
                .first()
                .map(|trace| trace.ended_unix_ms)
                .unwrap_or_default(),
            last_seen_unix_ms: group
                .last()
                .map(|trace| trace.ended_unix_ms)
                .unwrap_or_default(),
            suggested_name: suggested_name(&group),
            varying_arguments: varying_arguments(&compile_ready),
            dismissed,
            resurfaced,
        });
    }
    patterns.sort_by(|left, right| {
        right
            .occurrences
            .cmp(&left.occurrences)
            .then_with(|| right.last_seen_unix_ms.cmp(&left.last_seen_unix_ms))
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(patterns)
}

pub fn mine_suggestions(
    traces: &[WorkflowTrace],
    min_occurrences: usize,
    dismissals: &BTreeMap<String, PatternDismissal>,
    include_dismissed: bool,
) -> Result<Vec<WorkflowPattern>> {
    Ok(mine_patterns(traces, min_occurrences, dismissals)?
        .into_iter()
        .filter(|pattern| include_dismissed || !pattern.dismissed)
        .collect())
}

pub fn default_suggestions(
    traces: &[WorkflowTrace],
    dismissals: &BTreeMap<String, PatternDismissal>,
) -> Result<Vec<WorkflowPattern>> {
    mine_suggestions(traces, DEFAULT_MIN_OCCURRENCES, dismissals, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use semwright_types::{Idempotency, Risk};
    use serde_json::json;

    fn trace(id: &str, name: &str, path: &str, captured: bool) -> WorkflowTrace {
        WorkflowTrace {
            version: 1,
            id: id.into(),
            name: name.into(),
            intent: "export an artifact".into(),
            started_unix_ms: 1,
            ended_unix_ms: id.bytes().map(u64::from).sum(),
            capture_values: captured,
            successful: true,
            steps: vec![
                crate::TraceStep {
                    index: 0,
                    command: "ui.find".into(),
                    args: json!({"selector":{"role":"button","name":"Export"}}),
                    result: Some(json!({"ref":"ui:abc","count":1})),
                    ok: true,
                    error_code: None,
                    outcome_known: true,
                    risk: Risk::ReadOnly,
                    idempotency: Idempotency::ReadOnly,
                    capability_version: "1".into(),
                    descriptor_sha256: "a".repeat(64),
                    backend: "atspi".into(),
                    provider: None,
                    duration_ms: 1,
                    redacted: false,
                },
                crate::TraceStep {
                    index: 1,
                    command: "filesystem.write".into(),
                    args: json!({"path":path,"ref":"ui:abc"}),
                    result: Some(json!({"ok":true,"path":path})),
                    ok: true,
                    error_code: None,
                    outcome_known: true,
                    risk: Risk::MutatingReversible,
                    idempotency: Idempotency::Idempotent,
                    capability_version: "1".into(),
                    descriptor_sha256: "b".repeat(64),
                    backend: "filesystem".into(),
                    provider: None,
                    duration_ms: 1,
                    redacted: false,
                },
            ],
        }
    }

    #[test]
    fn values_do_not_change_pattern_identity() {
        let a = trace("trace-a", "export", "/tmp/a.png", true);
        let b = trace("trace-b", "export", "/tmp/b.png", true);
        assert_eq!(
            pattern_fingerprint(&a).unwrap(),
            pattern_fingerprint(&b).unwrap()
        );
    }

    #[test]
    fn descriptor_changes_split_patterns() {
        let a = trace("trace-a", "export", "/tmp/a.png", true);
        let mut b = trace("trace-b", "export", "/tmp/b.png", true);
        b.steps[0].descriptor_sha256 = "c".repeat(64);
        assert_ne!(
            pattern_fingerprint(&a).unwrap(),
            pattern_fingerprint(&b).unwrap()
        );
    }

    #[test]
    fn repeated_pattern_surfaces_variation_without_values() {
        let traces = vec![
            trace("trace-a", "export", "/tmp/a.png", true),
            trace("trace-b", "export", "/tmp/b.png", true),
            trace("trace-c", "export", "/tmp/c.png", true),
        ];
        let patterns = mine_patterns(&traces, 3, &BTreeMap::new()).unwrap();
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].occurrences, 3);
        assert_eq!(patterns[0].compile_ready_count, 3);
        assert!(
            patterns[0]
                .varying_arguments
                .iter()
                .any(|value| { value.pointer == "/path" && value.kind == "string:path" })
        );
        let encoded = serde_json::to_string(&patterns).unwrap();
        assert!(!encoded.contains("/tmp/a.png"));
        assert!(!encoded.contains("/tmp/b.png"));
    }

    #[test]
    fn metadata_only_evidence_is_detected_but_not_compile_ready() {
        let traces = vec![
            trace("trace-a", "export", "/tmp/a.png", false),
            trace("trace-b", "export", "/tmp/b.png", false),
            trace("trace-c", "export", "/tmp/c.png", false),
        ];
        let patterns = mine_patterns(&traces, 3, &BTreeMap::new()).unwrap();
        assert_eq!(patterns[0].compile_ready_count, 0);
        assert!(patterns[0].compile_trace_ids.is_empty());
    }

    #[test]
    fn dismissal_resurfaces_when_new_evidence_arrives() {
        let mut traces = vec![
            trace("trace-a", "export", "/tmp/a.png", true),
            trace("trace-b", "export", "/tmp/b.png", true),
            trace("trace-c", "export", "/tmp/c.png", true),
        ];
        let initial = mine_patterns(&traces, 3, &BTreeMap::new()).unwrap();
        let pattern = &initial[0];
        let dismissal = PatternDismissal {
            version: crate::DISMISSAL_VERSION,
            pattern_id: pattern.id.clone(),
            fingerprint: pattern.fingerprint.clone(),
            dismissed_unix_ms: 1,
            dismissed_through_occurrences: 3,
            permanent: false,
        };
        let dismissals = [(pattern.id.clone(), dismissal)].into_iter().collect();
        assert!(
            mine_suggestions(&traces, 3, &dismissals, false)
                .unwrap()
                .is_empty()
        );

        traces.push(trace("trace-d", "export", "/tmp/d.png", true));
        let resurfaced = mine_suggestions(&traces, 3, &dismissals, false).unwrap();
        assert_eq!(resurfaced.len(), 1);
        assert!(resurfaced[0].resurfaced);
    }

    #[test]
    fn permanent_dismissal_stays_hidden() {
        let traces = vec![
            trace("trace-a", "export", "/tmp/a.png", true),
            trace("trace-b", "export", "/tmp/b.png", true),
            trace("trace-c", "export", "/tmp/c.png", true),
        ];
        let initial = mine_patterns(&traces, 3, &BTreeMap::new()).unwrap();
        let pattern = &initial[0];
        let dismissal = PatternDismissal {
            version: crate::DISMISSAL_VERSION,
            pattern_id: pattern.id.clone(),
            fingerprint: pattern.fingerprint.clone(),
            dismissed_unix_ms: 1,
            dismissed_through_occurrences: 3,
            permanent: true,
        };
        let dismissals = [(pattern.id.clone(), dismissal)].into_iter().collect();
        assert!(
            mine_suggestions(&traces, 3, &dismissals, false)
                .unwrap()
                .is_empty()
        );
    }
}
