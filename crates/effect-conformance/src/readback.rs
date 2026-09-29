use crate::*;
use schemars::JsonSchema;
use semwright_semantic_composition::*;
use semwright_types::{CommandDescriptor, Risk};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReadbackRoute {
    NativeProperty,
    GraphProjection,
    GuaranteedEvent,
    FileReopen,
    ExportDecoder,
    Unavailable,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadbackMapping {
    pub rule: String,
    pub route: ReadbackRoute,
    pub observer_commands: BTreeMap<String, Digest>,
    pub isolation_required: bool,
    pub limitation: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadbackWorkflow {
    pub version: u32,
    pub workflow: String,
    pub mutation_command: String,
    pub mutation_descriptor: Digest,
    pub contract_digest: Digest,
    pub persistence_required: bool,
    pub mappings: Vec<ReadbackMapping>,
}
/// Static lint only. Descriptor registration and invocation remain Broker-owned.
/// A valid command name or readback mapping alone never creates native evidence.
pub fn lint_readback(
    descriptors: &[CommandDescriptor],
    contract: &EffectContract,
    workflow: &ReadbackWorkflow,
) -> Result<()> {
    contract.validate()?;
    ensure(
        workflow.version == 1 && workflow.mappings.len() <= 256 && descriptors.len() <= 4096,
        "readback schema/budget",
    )?;
    bounded_id(&workflow.workflow)?;
    bounded_id(&workflow.mutation_command)?;
    ensure(
        workflow.contract_digest == contract.digest()?,
        "readback effect-contract substitution",
    )?;
    let mut catalog = BTreeMap::new();
    for descriptor in descriptors {
        bounded_id(&descriptor.name)?;
        ensure(
            catalog
                .insert(descriptor.name.as_str(), descriptor)
                .is_none(),
            "duplicate descriptor identity",
        )?;
    }
    let mutation = catalog
        .get(workflow.mutation_command.as_str())
        .ok_or_else(|| {
            ContractError::Invalid("mutation command absent from actual descriptor catalog".into())
        })?;
    ensure(
        canonical_digest(mutation)? == workflow.mutation_descriptor,
        "mutation descriptor drift",
    )?;
    ensure(
        mutation.risk.mutates(),
        "mutation workflow cannot masquerade as read-only",
    )?;
    let mut mapped = BTreeSet::new();
    let mut persistence = false;
    for mapping in &workflow.mappings {
        let rule = contract
            .rules
            .iter()
            .find(|r| r.id == mapping.rule)
            .ok_or_else(|| ContractError::Invalid("mapping refers to absent effect rule".into()))?;
        ensure(mapped.insert(&mapping.rule), "duplicate readback mapping")?;
        ensure(
            mapping.observer_commands.len() <= 8,
            "observer command budget",
        )?;
        if mapping.route == ReadbackRoute::Unavailable {
            ensure(
                mapping.observer_commands.is_empty()
                    && mapping
                        .limitation
                        .as_ref()
                        .is_some_and(|s| !s.is_empty() && s.len() <= 1024),
                "unavailable readback needs an explicit limitation",
            )?;
            continue;
        }
        ensure(
            !mapping.observer_commands.is_empty(),
            "readback mapping has no observer operation",
        )?;
        let mut opens_project = false;
        for (name, digest) in &mapping.observer_commands {
            let observer = catalog.get(name.as_str()).ok_or_else(|| {
                ContractError::Invalid("observer command absent from catalog".into())
            })?;
            ensure(
                canonical_digest(observer)? == *digest,
                "observer descriptor drift",
            )?;
            ensure(
                observer.timeout_ms > 0 && observer.timeout_ms <= 3_600_000,
                "observer deadline missing or unbounded",
            )?;
            if !matches!(observer.risk, Risk::ReadOnly) {
                ensure(
                    mapping.isolation_required,
                    "effectful observer must explicitly require isolation and reauthorization",
                )?;
                opens_project = true;
            }
        }
        if mapping.route == ReadbackRoute::FileReopen {
            ensure(
                opens_project && mapping.isolation_required,
                "opening a native project is not a read-only operation",
            )?;
            persistence |= matches!(rule.predicate, Predicate::Reopened);
        }
    }
    ensure(
        contract
            .rules
            .iter()
            .filter(|r| r.obligation.required())
            .all(|r| mapped.contains(&r.id)),
        "required effect has no observation mapping",
    )?;
    ensure(
        !workflow.persistence_required || persistence,
        "persistent workflow lacks fresh-process reopen rule",
    )?;
    Ok(())
}
/// Executable conformance: actually invokes the adapter after static lint. A
/// metadata-only readback declaration cannot satisfy any rule. Existing Host
/// integration must authorize every observer and supply the invocation context.
pub fn run_readback_conformance<A: EvidenceAdapter>(
    descriptors: &[CommandDescriptor],
    workflow: &ReadbackWorkflow,
    contract: &EffectContract,
    context: &EvaluationContext,
    adapter: &mut A,
) -> Result<EffectEvaluation> {
    lint_readback(descriptors, contract, workflow)?;
    let mut scoped = context.clone();
    // Explicitly unavailable observations are not called and cannot get a PASS
    // merely because a broken adapter echoes the desired value anyway.
    for mapping in &workflow.mappings {
        if mapping.route == ReadbackRoute::Unavailable
            && let Some(rule) = contract.rules.iter().find(|r| r.id == mapping.rule)
        {
            scoped.observation_scope.remove(&rule.address);
        }
    }
    let collected = collect(contract, &scoped, adapter)?;
    evaluate(contract, &scoped, &collected)
}
