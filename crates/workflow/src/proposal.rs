use crate::{Candidate, DescriptorLookup, WorkflowPattern, WorkflowTrace, compile, verify_drift};
use semwright_recipes::ValueType;
use semwright_types::{Error, ErrorCode, Result, Risk};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const PROPOSAL_VERSION: u32 = 1;
pub const DEFAULT_MIN_PROPOSAL_TRACES: usize = 3;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceTier {
    Standard,
    Strong,
    VeryStrong,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProposalInput {
    pub name: String,
    pub kind: String,
    pub secret: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProposalEvidence {
    pub occurrences: usize,
    pub compile_ready_count: usize,
    pub source_trace_count: usize,
    pub parameter_count: usize,
    pub bound_value_count: usize,
    pub assertion_count: usize,
    pub tier: EvidenceTier,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkflowProposal {
    pub version: u32,
    pub id: String,
    pub pattern_id: String,
    pub suggestion_id: String,
    pub name: String,
    pub commands: Vec<String>,
    pub inputs: Vec<ProposalInput>,
    pub max_risk: Risk,
    pub requires: Vec<String>,
    pub static_verified: bool,
    pub evidence: ProposalEvidence,
    pub dismissed: bool,
    pub resurfaced: bool,
}

#[derive(Debug, Clone)]
pub struct ProposalBuild {
    pub proposal: WorkflowProposal,
    pub candidate: Candidate,
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

fn kind_name(kind: ValueType) -> &'static str {
    match kind {
        ValueType::String => "string",
        ValueType::Number => "number",
        ValueType::Integer => "integer",
        ValueType::Boolean => "boolean",
        ValueType::Object => "object",
        ValueType::Array => "array",
    }
}

fn proposal_description(pattern: &WorkflowPattern) -> String {
    let preview = pattern
        .commands
        .iter()
        .take(12)
        .cloned()
        .collect::<Vec<_>>()
        .join(" -> ");
    if pattern.commands.len() > 12 {
        format!(
            "Automatically proposed repeated workflow with {} steps: {preview} -> …",
            pattern.commands.len()
        )
    } else {
        format!("Automatically proposed repeated workflow: {preview}")
    }
}

fn count_step_bindings(value: &Value) -> usize {
    match value {
        Value::Object(map) if map.len() == 1 => {
            map.get("$var")
                .and_then(Value::as_str)
                .is_some_and(|path| path.starts_with("/steps/")) as usize
        }
        Value::Object(map) => map.values().map(count_step_bindings).sum(),
        Value::Array(values) => values.iter().map(count_step_bindings).sum(),
        _ => 0,
    }
}

fn evidence_tier(occurrences: usize, compile_ready: usize) -> EvidenceTier {
    if occurrences >= 8 && compile_ready >= 5 {
        EvidenceTier::VeryStrong
    } else if occurrences >= 5 && compile_ready >= 4 {
        EvidenceTier::Strong
    } else {
        EvidenceTier::Standard
    }
}

pub fn build_proposal(
    pattern: &WorkflowPattern,
    traces: &[WorkflowTrace],
    lookup: &dyn DescriptorLookup,
) -> Result<ProposalBuild> {
    if pattern.compile_ready_count < DEFAULT_MIN_PROPOSAL_TRACES
        || traces.len() < DEFAULT_MIN_PROPOSAL_TRACES
    {
        return Err(Error::new(
            ErrorCode::Conflict,
            "Automatic workflow proposals require at least three compile-ready observations",
        ));
    }
    if traces.len() > 8
        || traces
            .iter()
            .map(|trace| trace.id.as_str())
            .collect::<BTreeSet<_>>()
            .len()
            != traces.len()
        || traces
            .iter()
            .any(|trace| !pattern.compile_trace_ids.contains(&trace.id))
    {
        return Err(Error::new(
            ErrorCode::Conflict,
            "Workflow proposal source traces do not match the current suggestion evidence",
        ));
    }

    let candidate = compile(
        traces,
        &pattern.suggested_name,
        &proposal_description(pattern),
        &[],
        lookup,
    )?;
    verify_drift(&candidate, lookup)?;

    let mut requires = BTreeSet::new();
    let mut max_risk = Risk::ReadOnly;
    for step in &candidate.recipe.steps {
        let descriptor = lookup.describe(&step.command)?;
        requires.extend(descriptor.requires);
        if risk_rank(descriptor.risk) > risk_rank(max_risk) {
            max_risk = descriptor.risk;
        }
    }

    let inputs = candidate
        .recipe
        .inputs
        .iter()
        .map(|(name, input)| ProposalInput {
            name: name.clone(),
            kind: kind_name(input.kind).into(),
            secret: input.secret,
        })
        .collect::<Vec<_>>();
    let bound_value_count = candidate
        .recipe
        .steps
        .iter()
        .map(|step| count_step_bindings(&step.args))
        .sum();
    let assertion_count = candidate
        .recipe
        .steps
        .iter()
        .map(|step| step.assertions.len())
        .sum();
    // Proposal identity must not depend on captured workflow values. Candidate
    // fingerprints intentionally cover the full compiled recipe and therefore may
    // contain low-entropy constants. Hash only the structural pattern plus random
    // trace identities so new evidence invalidates stale proposal IDs without
    // creating a value-derived side channel.
    let mut source_trace_ids = traces
        .iter()
        .map(|trace| trace.id.clone())
        .collect::<Vec<_>>();
    source_trace_ids.sort();
    let proposal_fingerprint = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&json!({
            "version": PROPOSAL_VERSION,
            "pattern": pattern.fingerprint,
            "traces": source_trace_ids,
        }))?)
    );

    Ok(ProposalBuild {
        proposal: WorkflowProposal {
            version: PROPOSAL_VERSION,
            id: format!("proposal-{}", &proposal_fingerprint[..24]),
            pattern_id: pattern.id.clone(),
            suggestion_id: pattern.suggestion_id.clone(),
            name: candidate.recipe.name.clone(),
            commands: candidate
                .recipe
                .steps
                .iter()
                .map(|step| step.command.clone())
                .collect(),
            inputs,
            max_risk,
            requires: requires.into_iter().collect(),
            static_verified: false,
            evidence: ProposalEvidence {
                occurrences: pattern.occurrences,
                compile_ready_count: pattern.compile_ready_count,
                source_trace_count: traces.len(),
                parameter_count: candidate.recipe.inputs.len(),
                bound_value_count,
                assertion_count,
                tier: evidence_tier(pattern.occurrences, pattern.compile_ready_count),
            },
            dismissed: pattern.dismissed,
            resurfaced: pattern.resurfaced,
        },
        candidate,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_tiers_are_monotonic() {
        assert_eq!(evidence_tier(3, 3), EvidenceTier::Standard);
        assert_eq!(evidence_tier(5, 4), EvidenceTier::Strong);
        assert_eq!(evidence_tier(8, 5), EvidenceTier::VeryStrong);
    }

    #[test]
    fn step_binding_counter_does_not_count_input_bindings() {
        assert_eq!(
            count_step_bindings(&json!({
                "a":{"$var":"/steps/step-1/ref"},
                "b":{"$var":"/inputs/name"},
                "c":[{"$var":"/steps/step-2/id"}]
            })),
            2
        );
    }
}
