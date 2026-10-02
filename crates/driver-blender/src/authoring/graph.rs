//! C P0 consumer mapping. This creates a candidate record from host-owned typed
//! identities; only C's ReceiptAdapter can promote it to AdmittedReceipt.
use super::{AuthoringIntent, NativeOperation};
use semwright_project_graph as graph;
use semwright_semantic_composition::{
    Digest, PreparedPlan, Result as CompositionResult, VerificationReport,
};
use std::collections::BTreeSet;

#[derive(Debug, Clone)]
pub struct GraphReceiptContext {
    pub id: graph::ReceiptId,
    pub derivation: graph::DerivationId,
    pub project: graph::ProjectId,
    pub inputs: Vec<graph::RevisionPin>,
    pub outputs: Vec<graph::RevisionPin>,
    pub additional_determinants: Vec<graph::Determinant>,
    /// Digest of the exact registered apply descriptor, supplied by trusted Host code.
    pub descriptor: Digest,
    /// Digest of the pinned Blender/runtime package, supplied by trusted Host code.
    pub runtime: Digest,
    pub completed_unix_ms: u64,
    pub coverage: graph::Coverage,
}

/// The driver cannot manufacture durable logical identities from Blender names/paths.
/// Call this only after C/the trusted host has resolved ProjectId/LogicalAssetId revisions.
pub fn graph_receipt_candidate(
    context: GraphReceiptContext,
    request_id: &str,
    plan: &PreparedPlan<AuthoringIntent, NativeOperation>,
    verification: VerificationReport,
) -> graph::Result<graph::ExecutionReceipt> {
    if request_id.is_empty() || context.completed_unix_ms == 0 {
        return Err(graph::GraphError::Invalid("receipt host binding"));
    }
    if verification.validation.plan_digest != plan.digest {
        return Err(graph::GraphError::Invalid("verification plan mismatch"));
    }
    let effect_digest = plan
        .body
        .dependencies
        .get("effects.contract")
        .cloned()
        .ok_or(graph::GraphError::Invalid("missing pinned effect contract"))?;
    let mut determinants = vec![
        graph::Determinant {
            class: graph::DependencyClass::Contract,
            key: "effects.contract".into(),
            digest: effect_digest,
        },
        graph::Determinant {
            class: graph::DependencyClass::Descriptor,
            key: "driver.blender.composition.apply".into(),
            digest: context.descriptor.clone(),
        },
        graph::Determinant {
            class: graph::DependencyClass::Runtime,
            key: "blender-runtime".into(),
            digest: context.runtime.clone(),
        },
        graph::Determinant {
            class: graph::DependencyClass::Parameters,
            key: "authoring-intent".into(),
            digest: plan.body.intent_digest.clone(),
        },
    ];
    determinants.extend(context.additional_determinants);
    let mut unique = BTreeSet::new();
    if !determinants
        .iter()
        .all(|item| unique.insert((item.class, item.key.clone())))
    {
        return Err(graph::GraphError::Invalid("duplicate receipt determinant"));
    }
    let receipt = graph::ExecutionReceipt {
        version: graph::SCHEMA_VERSION,
        id: context.id,
        derivation: context.derivation,
        project: context.project,
        owner: plan.body.owner.clone(),
        request_id: request_id.into(),
        operation: graph::OperationIdentity {
            capability: "driver.blender.composition.apply".into(),
            descriptor: context.descriptor,
            runtime: context.runtime,
            plan: plan.digest.clone(),
            parameters: plan.body.intent_digest.clone(),
            recipe: None,
        },
        source_base: plan.body.base.clone(),
        inputs: context.inputs,
        outputs: context.outputs,
        determinants,
        coverage: context.coverage,
        verification,
        completed_unix_ms: context.completed_unix_ms,
    };
    receipt.validate()?;
    Ok(receipt)
}

/// Explicitly show that C's errors are not Composition errors. Useful to callers
/// that want to keep plan validation and durable-graph admission as separate gates.
pub fn verify_plan_before_graph(
    plan: &PreparedPlan<AuthoringIntent, NativeOperation>,
    profile: &semwright_semantic_composition::ProfileDescriptor,
) -> CompositionResult<()> {
    plan.verify(profile)
}
