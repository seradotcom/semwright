use crate::{CANDIDATE_VERSION, Candidate, ParameterHint, WorkflowTrace, looks_like_ref};
use semwright_recipes::{Assertion, Input, Operator, Output, Recipe, Retry, Step, ValueType};
use semwright_types::{CommandDescriptor, Error, ErrorCode, Idempotency, Result, Risk};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{SystemTime, UNIX_EPOCH},
};

pub trait DescriptorLookup {
    fn describe(&self, command: &str) -> Result<CommandDescriptor>;
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 80
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}
fn canonical_slug(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 40
        && s.as_bytes()[0].is_ascii_lowercase()
        && s.as_bytes()[s.len() - 1].is_ascii_alphanumeric()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn value_type(value: &Value) -> Result<ValueType> {
    match value {
        Value::String(_) => Ok(ValueType::String),
        Value::Number(n) if n.is_i64() || n.is_u64() => Ok(ValueType::Integer),
        Value::Number(_) => Ok(ValueType::Number),
        Value::Bool(_) => Ok(ValueType::Boolean),
        Value::Object(_) => Ok(ValueType::Object),
        Value::Array(_) => Ok(ValueType::Array),
        Value::Null => Err(Error::invalid("Null cannot become a recipe input")),
    }
}

fn pointer_escape(s: &str) -> String {
    s.replace('~', "~0").replace('/', "~1")
}

fn scalar_paths(value: &Value, base: &str, out: &mut Vec<(String, Value)>, depth: usize) {
    if depth > 16 || out.len() > 4096 {
        return;
    }
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let pointer = format!("{base}/{}", pointer_escape(key));
                scalar_paths(child, &pointer, out, depth + 1);
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                let pointer = format!("{base}/{index}");
                scalar_paths(child, &pointer, out, depth + 1);
            }
        }
        Value::Null => {}
        _ => out.push((base.to_owned(), value.clone())),
    }
}

fn prior_binding(traces: &[WorkflowTrace], step: usize, values: &[&Value]) -> Option<Value> {
    if values.is_empty() {
        return None;
    }
    let mut selected: Option<(usize, String)> = None;
    for (trace_index, trace) in traces.iter().enumerate() {
        let wanted = values[trace_index];
        let mut found = vec![];
        for prior in 0..step {
            let result = trace.steps[prior].result.as_ref()?;
            let mut scalars = vec![];
            scalar_paths(result, "", &mut scalars, 0);
            for (pointer, value) in scalars {
                let structural = value.as_str().is_some_and(looks_like_ref)
                    || pointer.ends_with("/ref")
                    || pointer.ends_with("/id");
                if structural && &value == wanted {
                    found.push((prior, pointer));
                }
            }
        }
        if found.len() != 1 {
            return None;
        }
        if let Some(expected) = &selected {
            if expected != &found[0] {
                return None;
            }
        } else {
            selected = Some(found[0].clone());
        }
    }
    selected.map(|(prior, pointer)| {
        let suffix = pointer.trim_start_matches('/');
        let path = if suffix.is_empty() {
            format!("/steps/step-{}", prior + 1)
        } else {
            format!("/steps/step-{}/{suffix}", prior + 1)
        };
        json!({"$var":path})
    })
}

fn hint_for<'a>(
    hints: &'a [ParameterHint],
    step: usize,
    pointer: &str,
) -> Option<&'a ParameterHint> {
    hints
        .iter()
        .find(|hint| hint.step == step && hint.pointer == pointer)
}

fn make_input_name(
    hint: Option<&ParameterHint>,
    step: usize,
    pointer: &str,
    used: &mut BTreeSet<String>,
) -> Result<String> {
    let raw = if let Some(hint) = hint {
        hint.name.clone()
    } else {
        let leaf = pointer
            .rsplit('/')
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or("value");
        let leaf = leaf
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .collect::<String>();
        format!("step{}_{}", step + 1, leaf)
    };
    if !valid_name(&raw) {
        return Err(Error::invalid("Inferred workflow input name is invalid"));
    }
    let mut name = raw.clone();
    let mut suffix = 2;
    while !used.insert(name.clone()) {
        name = format!("{raw}_{suffix}");
        suffix += 1;
    }
    Ok(name)
}

fn compile_value(
    traces: &[WorkflowTrace],
    step: usize,
    pointer: &str,
    values: &[&Value],
    hints: &[ParameterHint],
    inputs: &mut BTreeMap<String, Input>,
    used: &mut BTreeSet<String>,
) -> Result<Value> {
    if let Some(binding) = prior_binding(traces, step, values) {
        return Ok(binding);
    }
    let first = values[0];
    let hint = hint_for(hints, step, pointer);
    if hint.is_some() {
        let kind = value_type(first)?;
        if values
            .iter()
            .any(|value| value_type(value).ok() != Some(kind))
        {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Traces disagree on the type of an explicitly parameterized workflow input",
            ));
        }
        let name = make_input_name(hint, step, pointer, used)?;
        inputs.insert(
            name.clone(),
            Input {
                kind,
                secret: hint.is_some_and(|value| value.secret),
                default: None,
            },
        );
        return Ok(json!({"$var":format!("/inputs/{name}")}));
    }

    if values.iter().all(|value| value.is_object()) {
        let mut keys = BTreeSet::new();
        for value in values {
            keys.extend(value.as_object().unwrap().keys().cloned());
        }
        let mut out = Map::new();
        for key in keys {
            let children = values
                .iter()
                .map(|value| value.get(&key).unwrap_or(&Value::Null))
                .collect::<Vec<_>>();
            let child_pointer = format!("{pointer}/{}", pointer_escape(&key));
            out.insert(
                key,
                compile_value(traces, step, &child_pointer, &children, hints, inputs, used)?,
            );
        }
        return Ok(Value::Object(out));
    }

    if let Some(first_array) = first.as_array()
        && values.iter().all(|value| {
            value
                .as_array()
                .is_some_and(|a| a.len() == first_array.len())
        })
    {
        let mut out = Vec::with_capacity(first_array.len());
        for index in 0..first_array.len() {
            let children = values
                .iter()
                .map(|value| &value.as_array().unwrap()[index])
                .collect::<Vec<_>>();
            out.push(compile_value(
                traces,
                step,
                &format!("{pointer}/{index}"),
                &children,
                hints,
                inputs,
                used,
            )?);
        }
        return Ok(Value::Array(out));
    }

    if values.iter().all(|value| *value == first) {
        if first.as_str().is_some_and(looks_like_ref) {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Workflow contains an unbound opaque reference; derive it from a prior step or parameterize it explicitly",
            ));
        }
        return Ok(first.clone());
    }

    let kind = value_type(first)?;
    if values
        .iter()
        .any(|value| value_type(value).ok() != Some(kind))
    {
        return Err(Error::new(
            ErrorCode::Conflict,
            "Traces disagree on the type of an inferred workflow input",
        ));
    }
    let name = make_input_name(None, step, pointer, used)?;
    inputs.insert(
        name.clone(),
        Input {
            kind,
            secret: false,
            default: None,
        },
    );
    Ok(json!({"$var":format!("/inputs/{name}")}))
}
fn inferred_assertions(step_id: &str, results: &[&Value]) -> Vec<Assertion> {
    let mut assertions = vec![];
    for key in [
        "ok",
        "completed",
        "valid",
        "invoked",
        "set",
        "selected",
        "toggled",
    ] {
        if results
            .iter()
            .all(|result| result.get(key).and_then(Value::as_bool) == Some(true))
        {
            assertions.push(Assertion {
                left: json!({"$var":format!("/steps/{step_id}/{key}")}),
                op: Operator::Equals,
                right: Value::Bool(true),
            });
        }
    }
    assertions.truncate(4);
    assertions
}

pub fn compile(
    traces: &[WorkflowTrace],
    name: &str,
    description: &str,
    hints: &[ParameterHint],
    lookup: &dyn DescriptorLookup,
) -> Result<Candidate> {
    if traces.is_empty() || traces.len() > 8 || !valid_name(name) {
        return Err(Error::invalid(
            "Compile requires 1..8 traces and a valid recipe name",
        ));
    }
    let unique_trace_ids = traces
        .iter()
        .map(|trace| trace.id.as_str())
        .collect::<BTreeSet<_>>();
    if unique_trace_ids.len() != traces.len() {
        return Err(Error::invalid("Compile requires distinct source trace IDs"));
    }
    if description.len() > 4096 || description.chars().any(char::is_control) {
        return Err(Error::invalid("Workflow description is invalid"));
    }
    let step_count = traces[0].steps.len();
    if step_count == 0 || step_count > 64 {
        return Err(Error::invalid("Workflow trace step count is invalid"));
    }
    for trace in traces {
        if trace.version != 1
            || !trace.successful
            || !trace.capture_values
            || trace.steps.len() != step_count
            || trace
                .steps
                .iter()
                .any(|step| !step.ok || !step.outcome_known || step.redacted)
        {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Only successful value-capturing traces with known, unredacted outcomes can compile",
            ));
        }
    }
    for index in 0..step_count {
        let command = &traces[0].steps[index].command;
        if traces
            .iter()
            .any(|trace| &trace.steps[index].command != command)
        {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Traces have different command sequences",
            ));
        }
    }
    let mut hinted_locations = BTreeSet::new();
    let mut hinted_names = BTreeSet::new();
    for hint in hints {
        if !valid_name(&hint.name)
            || hint.step >= step_count
            || (!hint.pointer.is_empty() && !hint.pointer.starts_with('/'))
            || !hinted_locations.insert((hint.step, hint.pointer.clone()))
            || !hinted_names.insert(hint.name.clone())
            || traces
                .iter()
                .any(|trace| trace.steps[hint.step].args.pointer(&hint.pointer).is_none())
        {
            return Err(Error::invalid(
                "Workflow parameter hints must be unique, valid, and present in every trace",
            ));
        }
    }
    let mut inputs = BTreeMap::new();
    let mut used_inputs = BTreeSet::new();
    let mut recipe_steps = vec![];
    let mut descriptor_digests = BTreeMap::new();

    for index in 0..step_count {
        let source = &traces[0].steps[index];
        let descriptor = lookup.describe(&source.command)?;
        if descriptor.risk == Risk::SecretAccess {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Secret-access operations cannot be compiled into learned recipes",
            ));
        }
        let current_digest = format!("{:x}", Sha256::digest(serde_json::to_vec(&descriptor)?));
        if source.descriptor_sha256.is_empty()
            || source.descriptor_sha256 != current_digest
            || source.capability_version != descriptor.version
            || source.risk != descriptor.risk
            || source.idempotency != descriptor.idempotency
            || traces.iter().any(|trace| {
                let step = &trace.steps[index];
                step.descriptor_sha256 != source.descriptor_sha256
                    || step.capability_version != source.capability_version
                    || step.risk != source.risk
                    || step.idempotency != source.idempotency
            })
        {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Source traces do not match the current capability descriptor",
            ));
        }
        descriptor_digests.insert(source.command.clone(), source.descriptor_sha256.clone());
        let args = traces
            .iter()
            .map(|trace| &trace.steps[index].args)
            .collect::<Vec<_>>();
        let compiled_args = compile_value(
            traces,
            index,
            "",
            &args,
            hints,
            &mut inputs,
            &mut used_inputs,
        )?;
        let results = traces
            .iter()
            .map(|trace| trace.steps[index].result.as_ref().unwrap())
            .collect::<Vec<_>>();
        let step_id = format!("step-{}", index + 1);
        recipe_steps.push(Step {
            id: step_id.clone(),
            command: source.command.clone(),
            args: compiled_args,
            timeout_ms: descriptor.timeout_ms,
            retry: Retry::default(),
            when: None,
            assertions: inferred_assertions(&step_id, &results),
        });
    }

    let final_results = traces
        .iter()
        .map(|trace| {
            trace
                .steps
                .last()
                .and_then(|step| step.result.as_ref())
                .unwrap()
        })
        .collect::<Vec<_>>();
    let mut outputs = BTreeMap::new();
    if let Ok(kind) = value_type(final_results[0])
        && final_results
            .iter()
            .all(|value| value_type(value).ok() == Some(kind))
    {
        outputs.insert(
            "result".into(),
            Output {
                kind,
                value: json!({"$var":format!("/steps/step-{step_count}")}),
                secret: false,
            },
        );
    }

    let recipe = Recipe {
        version: 1,
        name: name.into(),
        description: description.into(),
        inputs,
        steps: recipe_steps,
        outputs,
    };
    let source_trace_ids = traces
        .iter()
        .map(|trace| trace.id.clone())
        .collect::<Vec<_>>();
    let fingerprint = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&json!({
            "recipe":recipe,
            "traces":source_trace_ids,
            "digests":descriptor_digests
        }))?)
    );
    Ok(Candidate {
        version: CANDIDATE_VERSION,
        id: format!("candidate-{}", &fingerprint[..24]),
        recipe,
        source_trace_ids,
        source_descriptor_sha256: descriptor_digests,
        compiled_unix_ms: now_ms(),
        fingerprint,
        static_verified: false,
        successful_replays: 0,
    })
}

pub fn verify_drift(candidate: &Candidate, lookup: &dyn DescriptorLookup) -> Result<Value> {
    if candidate.version != CANDIDATE_VERSION {
        return Err(Error::new(
            ErrorCode::ProtocolMismatch,
            "Unsupported workflow candidate version",
        ));
    }
    let mut current = BTreeMap::new();
    for (command, expected) in &candidate.source_descriptor_sha256 {
        let descriptor = lookup.describe(command)?;
        let actual = format!("{:x}", Sha256::digest(serde_json::to_vec(&descriptor)?));
        if &actual != expected {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Capability descriptor drifted; re-record or explicitly recompile the workflow",
            ));
        }
        current.insert(command.clone(), actual);
    }
    Ok(json!({
        "valid":true,
        "candidate_id":candidate.id,
        "source_traces":candidate.source_trace_ids,
        "descriptor_sha256":current,
        "recipe_fingerprint":candidate.fingerprint
    }))
}

fn schema_for_type(kind: ValueType) -> Value {
    match kind {
        ValueType::String => json!({"type":"string","maxLength":8192}),
        ValueType::Number => json!({"type":"number"}),
        ValueType::Integer => json!({"type":"integer"}),
        ValueType::Boolean => json!({"type":"boolean"}),
        ValueType::Object => json!({"type":"object","maxProperties":256}),
        ValueType::Array => json!({"type":"array","maxItems":2048}),
    }
}

fn risk_rank(risk: Risk) -> u8 {
    match risk {
        Risk::ReadOnly => 0,
        Risk::MutatingReversible => 1,
        Risk::Mutating => 2,
        Risk::Destructive => 3,
        Risk::SecretAccess => 4,
        Risk::CodeExecution => 5,
        Risk::PrivilegeSensitive => 6,
    }
}

pub fn promoted_descriptor(
    slug: &str,
    candidate: &Candidate,
    lookup: &dyn DescriptorLookup,
) -> Result<CommandDescriptor> {
    if !canonical_slug(slug) {
        return Err(Error::invalid("Promotion slug is invalid"));
    }
    let mut requires = BTreeSet::new();
    let mut risk = Risk::ReadOnly;
    let mut idempotency = Idempotency::ReadOnly;
    let mut interactive_consent = false;
    let mut timeout_ms = 0u64;

    for step in &candidate.recipe.steps {
        let descriptor = lookup.describe(&step.command)?;
        requires.extend(descriptor.requires);
        if risk_rank(descriptor.risk) > risk_rank(risk) {
            risk = descriptor.risk;
        }
        idempotency = match (idempotency, descriptor.idempotency) {
            (Idempotency::Destructive, _) | (_, Idempotency::Destructive) => {
                Idempotency::Destructive
            }
            (Idempotency::NonIdempotent, _) | (_, Idempotency::NonIdempotent) => {
                Idempotency::NonIdempotent
            }
            (Idempotency::Idempotent, _) | (_, Idempotency::Idempotent) => Idempotency::Idempotent,
            _ => Idempotency::ReadOnly,
        };
        interactive_consent |= descriptor.interactive_consent;
        timeout_ms = timeout_ms.saturating_add(step.timeout_ms);
    }
    timeout_ms = timeout_ms.clamp(1, 300_000);

    let mut properties = Map::new();
    let mut required = vec![];
    for (name, input) in &candidate.recipe.inputs {
        let mut schema = schema_for_type(input.kind);
        if input.secret {
            schema["x-semwright-secret"] = Value::Bool(true);
        }
        if let Some(default) = &input.default {
            schema["default"] = default.clone();
        } else {
            required.push(Value::String(name.clone()));
        }
        properties.insert(name.clone(), schema);
    }
    let mut output_properties = Map::new();
    let mut output_required = vec![];
    for (name, output) in &candidate.recipe.outputs {
        output_properties.insert(
            name.clone(),
            json!({
                "anyOf":[
                    schema_for_type(output.kind),
                    {"type":"string","const":"[REDACTED]"}
                ]
            }),
        );
        output_required.push(Value::String(name.clone()));
    }

    Ok(CommandDescriptor {
        name: format!("recipe.{slug}.run"),
        version: format!("1+{}", &candidate.fingerprint[..12]),
        description: if candidate.recipe.description.is_empty() {
            format!(
                "Learned workflow promoted from {} verified trace(s).",
                candidate.source_trace_ids.len()
            )
        } else {
            candidate.recipe.description.clone()
        },
        input_schema: json!({
            "type":"object",
            "properties":properties,
            "required":required,
            "additionalProperties":false
        }),
        output_schema: json!({
            "oneOf":[
                {
                    "type":"object",
                    "properties":{
                        "dry_run":{"type":"boolean","const":true},
                        "plan":{"type":"object","maxProperties":128},
                        "side_effects":{"type":"boolean","const":false},
                        "bindings":{"type":"string","maxLength":4096}
                    },
                    "required":["dry_run","plan","side_effects","bindings"],
                    "additionalProperties":false
                },
                {
                    "type":"object",
                    "properties":{
                        "completed":{"type":"boolean","const":true},
                        "steps":{
                            "type":"array",
                            "items":{
                                "anyOf":[
                                    {
                                        "type":"object",
                                        "properties":{
                                            "step":{"type":"string","maxLength":80},
                                            "ok":{"type":"boolean","const":true},
                                            "attempts":{"type":"integer","minimum":1,"maximum":5}
                                        },
                                        "required":["step","ok","attempts"],
                                        "additionalProperties":false
                                    },
                                    {
                                        "type":"object",
                                        "properties":{
                                            "step":{"type":"string","maxLength":80},
                                            "skipped":{"type":"boolean","const":true}
                                        },
                                        "required":["step","skipped"],
                                        "additionalProperties":false
                                    }
                                ]
                            },
                            "maxItems":64
                        },
                        "outputs":{
                            "type":"object",
                            "properties":output_properties,
                            "required":output_required,
                            "additionalProperties":false
                        }
                    },
                    "required":["completed","steps","outputs"],
                    "additionalProperties":false
                }
            ]
        }),
        requires: requires.into_iter().collect(),
        risk,
        idempotency,
        timeout_ms,
        dry_run: true,
        interactive_consent,
        backends: vec!["core".into()],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Lookup(BTreeMap<String, CommandDescriptor>);

    impl DescriptorLookup for Lookup {
        fn describe(&self, command: &str) -> Result<CommandDescriptor> {
            self.0
                .get(command)
                .cloned()
                .ok_or_else(|| Error::new(ErrorCode::NotFound, "test capability missing"))
        }
    }

    fn descriptor(name: &str, risk: Risk) -> CommandDescriptor {
        CommandDescriptor {
            name: name.into(),
            version: "1".into(),
            description: name.into(),
            input_schema: json!({"type":"object"}),
            output_schema: json!({"type":"object"}),
            requires: vec!["desktop.observe".into()],
            risk,
            idempotency: if risk == Risk::ReadOnly {
                Idempotency::ReadOnly
            } else {
                Idempotency::NonIdempotent
            },
            timeout_ms: 1000,
            dry_run: true,
            interactive_consent: false,
            backends: vec!["fake".into()],
        }
    }

    fn digest(descriptor: &CommandDescriptor) -> String {
        format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(descriptor).unwrap())
        )
    }

    fn lookup() -> Lookup {
        Lookup(
            [
                ("ui.find".into(), descriptor("ui.find", Risk::ReadOnly)),
                ("ui.invoke".into(), descriptor("ui.invoke", Risk::Mutating)),
            ]
            .into_iter()
            .collect(),
        )
    }

    fn trace(id: &str, filename: &str, reference: &str, lookup: &Lookup) -> WorkflowTrace {
        let find = lookup.describe("ui.find").unwrap();
        let invoke = lookup.describe("ui.invoke").unwrap();
        WorkflowTrace {
            version: 1,
            id: id.into(),
            name: "export".into(),
            intent: "Export file".into(),
            started_unix_ms: 1,
            ended_unix_ms: 2,
            capture_values: true,
            successful: true,
            steps: vec![
                crate::TraceStep {
                    index: 0,
                    command: "ui.find".into(),
                    args: json!({
                        "selector":{"role":"button","name":"Export"},
                        "filename":filename
                    }),
                    result: Some(json!({
                        "nodes":[{"ref":reference,"name":"Export"}],
                        "ok":true
                    })),
                    ok: true,
                    error_code: None,
                    outcome_known: true,
                    risk: Risk::ReadOnly,
                    idempotency: Idempotency::ReadOnly,
                    capability_version: "1".into(),
                    descriptor_sha256: digest(&find),
                    backend: "fake".into(),
                    provider: None,
                    duration_ms: 1,
                    redacted: false,
                },
                crate::TraceStep {
                    index: 1,
                    command: "ui.invoke".into(),
                    args: json!({"ref":reference}),
                    result: Some(json!({"invoked":true})),
                    ok: true,
                    error_code: None,
                    outcome_known: true,
                    risk: Risk::Mutating,
                    idempotency: Idempotency::NonIdempotent,
                    capability_version: "1".into(),
                    descriptor_sha256: digest(&invoke),
                    backend: "fake".into(),
                    provider: None,
                    duration_ms: 1,
                    redacted: false,
                },
            ],
        }
    }

    #[test]
    fn multiple_traces_infer_input_and_ref_binding() {
        let lookup = lookup();
        let first = trace(
            "trace-a",
            "one.png",
            "ui:00000000000000000000000000000001",
            &lookup,
        );
        let second = trace(
            "trace-b",
            "two.png",
            "ui:00000000000000000000000000000002",
            &lookup,
        );
        let candidate = compile(&[first, second], "export", "Export a file", &[], &lookup).unwrap();
        assert_eq!(candidate.recipe.inputs.len(), 1);
        assert_eq!(
            candidate.recipe.steps[0].args["filename"],
            json!({"$var":"/inputs/step1_filename"})
        );
        assert_eq!(
            candidate.recipe.steps[1].args["ref"],
            json!({"$var":"/steps/step-1/nodes/0/ref"})
        );
        assert_eq!(candidate.recipe.steps[1].assertions.len(), 1);
    }

    #[test]
    fn single_trace_only_generalizes_explicit_hint() {
        let lookup = lookup();
        let trace = trace(
            "trace-a",
            "one.png",
            "ui:00000000000000000000000000000001",
            &lookup,
        );
        let candidate = compile(
            &[trace],
            "export",
            "",
            &[ParameterHint {
                name: "filename".into(),
                step: 0,
                pointer: "/filename".into(),
                secret: false,
            }],
            &lookup,
        )
        .unwrap();
        assert_eq!(
            candidate.recipe.steps[0].args["filename"],
            json!({"$var":"/inputs/filename"})
        );
        assert_eq!(candidate.recipe.steps[0].args["selector"]["name"], "Export");
    }

    #[test]
    fn descriptor_drift_is_rejected() {
        let lookup = lookup();
        let trace = trace(
            "trace-a",
            "one.png",
            "ui:00000000000000000000000000000001",
            &lookup,
        );
        let mut candidate = compile(&[trace], "export", "", &[], &lookup).unwrap();
        candidate
            .source_descriptor_sha256
            .insert("ui.find".into(), "bad".into());
        assert_eq!(
            verify_drift(&candidate, &lookup).unwrap_err().code,
            ErrorCode::Conflict
        );
    }

    #[test]
    fn promoted_descriptor_uses_union_authority_and_max_risk() {
        let lookup = lookup();
        let trace = trace(
            "trace-a",
            "one.png",
            "ui:00000000000000000000000000000001",
            &lookup,
        );
        let candidate = compile(&[trace], "export", "", &[], &lookup).unwrap();
        let descriptor = promoted_descriptor("export", &candidate, &lookup).unwrap();
        assert_eq!(descriptor.name, "recipe.export.run");
        assert_eq!(descriptor.risk, Risk::Mutating);
        assert_eq!(descriptor.requires, vec!["desktop.observe"]);
        assert_eq!(descriptor.idempotency, Idempotency::NonIdempotent);
    }

    #[test]
    fn secret_access_step_never_compiles() {
        let mut lookup = lookup();
        let secret = descriptor("ui.find", Risk::SecretAccess);
        lookup.0.insert("ui.find".into(), secret.clone());
        let mut trace = trace(
            "trace-a",
            "one.png",
            "ui:00000000000000000000000000000001",
            &lookup,
        );
        trace.steps[0].risk = Risk::SecretAccess;
        trace.steps[0].descriptor_sha256 = digest(&secret);
        assert_eq!(
            compile(&[trace], "export", "", &[], &lookup)
                .unwrap_err()
                .code,
            ErrorCode::PolicyDenied
        );
    }

    #[test]
    fn compile_rejects_descriptor_that_drifted_after_recording() {
        let original = lookup();
        let trace = trace(
            "trace-a",
            "one.png",
            "ui:00000000000000000000000000000001",
            &original,
        );
        let mut changed = lookup();
        changed.0.get_mut("ui.find").unwrap().description = "changed contract".into();
        assert_eq!(
            compile(&[trace], "export", "", &[], &changed)
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
    }

    #[test]
    fn compile_rejects_duplicate_trace_identity() {
        let lookup = lookup();
        let trace = trace(
            "trace-a",
            "one.png",
            "ui:00000000000000000000000000000001",
            &lookup,
        );
        assert_eq!(
            compile(&[trace.clone(), trace], "export", "", &[], &lookup)
                .unwrap_err()
                .code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn parameter_hints_must_be_unambiguous() {
        let lookup = lookup();
        let trace = trace(
            "trace-a",
            "one.png",
            "ui:00000000000000000000000000000001",
            &lookup,
        );
        let hints = vec![
            ParameterHint {
                name: "filename".into(),
                step: 0,
                pointer: "/filename".into(),
                secret: false,
            },
            ParameterHint {
                name: "other".into(),
                step: 0,
                pointer: "/filename".into(),
                secret: false,
            },
        ];
        assert_eq!(
            compile(&[trace], "export", "", &hints, &lookup)
                .unwrap_err()
                .code,
            ErrorCode::InvalidArgument
        );
    }

    fn direct_ref_trace(reference: &str, lookup: &Lookup) -> WorkflowTrace {
        let invoke = lookup.describe("ui.invoke").unwrap();
        WorkflowTrace {
            version: 1,
            id: "trace-direct-ref".into(),
            name: "direct-ref".into(),
            intent: "Invoke a caller-owned target".into(),
            started_unix_ms: 1,
            ended_unix_ms: 2,
            capture_values: true,
            successful: true,
            steps: vec![crate::TraceStep {
                index: 0,
                command: "ui.invoke".into(),
                args: json!({"ref":reference}),
                result: Some(json!({"invoked":true})),
                ok: true,
                error_code: None,
                outcome_known: true,
                risk: Risk::Mutating,
                idempotency: Idempotency::NonIdempotent,
                capability_version: invoke.version.clone(),
                descriptor_sha256: digest(&invoke),
                backend: "fake".into(),
                provider: None,
                duration_ms: 1,
                redacted: false,
            }],
        }
    }

    #[test]
    fn unbound_opaque_reference_is_never_baked_into_recipe() {
        let lookup = lookup();
        let trace = direct_ref_trace("video:00000000000000000000000000000001", &lookup);
        assert_eq!(
            compile(&[trace], "invoke-target", "", &[], &lookup)
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
    }

    #[test]
    fn explicit_opaque_reference_parameter_is_allowed() {
        let lookup = lookup();
        let trace = direct_ref_trace("obs-scene:00000000000000000000000000000001", &lookup);
        let candidate = compile(
            &[trace],
            "invoke-target",
            "",
            &[ParameterHint {
                name: "target".into(),
                step: 0,
                pointer: "/ref".into(),
                secret: false,
            }],
            &lookup,
        )
        .unwrap();
        assert_eq!(
            candidate.recipe.steps[0].args["ref"],
            json!({"$var":"/inputs/target"})
        );
        assert_eq!(candidate.recipe.inputs["target"].kind, ValueType::String);
    }

    #[test]
    fn explicit_object_parameter_is_not_ignored() {
        let lookup = lookup();
        let trace = trace(
            "trace-a",
            "one.png",
            "ui:00000000000000000000000000000001",
            &lookup,
        );
        let candidate = compile(
            &[trace],
            "find-target",
            "",
            &[ParameterHint {
                name: "selector".into(),
                step: 0,
                pointer: "/selector".into(),
                secret: false,
            }],
            &lookup,
        )
        .unwrap();
        assert_eq!(
            candidate.recipe.steps[0].args["selector"],
            json!({"$var":"/inputs/selector"})
        );
        assert_eq!(candidate.recipe.inputs["selector"].kind, ValueType::Object);
    }
}
