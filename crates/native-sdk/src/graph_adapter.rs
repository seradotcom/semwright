//! Candidate construction for the canonical Project Graph.
//!
//! This module deliberately cannot admit graph evidence. It only builds the
//! existing Project Graph's untrusted `RevisionCandidate`. A trusted Host must
//! bind a native resolver, authenticate the owner, construct the canonical
//! `RevisionAdapter`, and admit the candidate into the graph.
use crate::{Value, cooperation::ResourceVersion};
use graph::composition::{
    Address, BaseState, BaseStateSet, Concurrency, EvidenceSource, ObservationRef, ResourceKey,
    Revision, canonical_digest,
};
use semwright_project_graph as graph;

pub const GRAPH_OBSERVATION_METHOD: &str = "semwright-native-sdk-projection";
pub const GRAPH_OBSERVATION_METHOD_VERSION: u32 = 1;
pub const GRAPH_RESOLVER: &str = "semwright-native-sdk";
pub const GRAPH_RESOLVER_VERSION: u32 = 1;

/// A durable native locator is a re-resolution hint, never a session ref or grant.
pub fn native_locator(
    provider: &str,
    stable_id: &str,
    resource: &str,
) -> graph::Result<graph::DurableLocator> {
    let locator = graph::DurableLocator::Native {
        resource: ResourceKey {
            provider: provider.to_owned(),
            resource: resource.to_owned(),
        },
        stable_id: stable_id.to_owned(),
        resolver: GRAPH_RESOLVER.to_owned(),
        resolver_version: GRAPH_RESOLVER_VERSION,
    };
    locator.validate()?;
    Ok(locator)
}

/// Translate a fresh application observation into the Project Graph's existing
/// untrusted candidate type. Opaque application revisions stay opaque: Graph sees
/// a digest of the exact token tuple instead of parsing it as a number.
///
/// `provider_session` must come from the authenticated Driver Host channel. It
/// is carried in the candidate for later validation; this function does not
/// authenticate it and therefore confers no Graph authority.
#[allow(clippy::too_many_arguments)]
pub fn revision_candidate(
    project: &graph::ProjectId,
    asset: graph::LogicalAssetId,
    binding_generation: u64,
    provider: &str,
    provider_session: &str,
    version: &ResourceVersion,
    projection: &Value,
    observed_unix_ms: u64,
    coverage: graph::Coverage,
    exhaustive: bool,
) -> graph::Result<graph::RevisionCandidate> {
    version
        .validate()
        .map_err(|_| graph::GraphError::Invalid("native resource version"))?;
    if binding_generation == 0 || provider_session.is_empty() {
        return Err(graph::GraphError::Invalid(
            "native graph binding/session required",
        ));
    }

    let resource = ResourceKey {
        provider: provider.to_owned(),
        resource: version.resource.clone(),
    };
    let revision_digest = canonical_digest(&(
        "semwright-native-sdk-revision-v1",
        &version.resource,
        &version.generation,
        &version.revision,
    ))?;
    let projection_digest = canonical_digest(projection)?;
    let observation_digest = canonical_digest(&(
        "semwright-native-sdk-observation-v1",
        project.as_str(),
        asset.as_str(),
        binding_generation,
        provider,
        provider_session,
        version,
        &projection_digest,
    ))?;
    let base = BaseStateSet(vec![BaseState {
        key: resource.clone(),
        document_id: project.as_str().to_owned(),
        provider_session: provider_session.to_owned(),
        generation: version.generation.clone(),
        revision: Revision::Fingerprint(revision_digest),
        concurrency: Concurrency::CompareAndSwap,
    }]);
    let candidate = graph::RevisionCandidate {
        version: graph::SCHEMA_VERSION,
        asset,
        fingerprint: graph::Fingerprint {
            bytes: None,
            projection: Some(graph::ProjectionDigest {
                digest: projection_digest,
                method: GRAPH_OBSERVATION_METHOD.to_owned(),
                method_version: GRAPH_OBSERVATION_METHOD_VERSION,
            }),
        },
        equivalence: graph::Equivalence::Projection,
        observed_unix_ms,
        binding_generation,
        observation: ObservationRef {
            id: format!("native-observation-{}", observation_digest.as_str()),
            base,
            source: EvidenceSource::NativeApi,
            method: GRAPH_OBSERVATION_METHOD.to_owned(),
            method_version: GRAPH_OBSERVATION_METHOD_VERSION,
            scope: vec![Address {
                resource,
                logical_id: "projection".to_owned(),
                property: "projection".to_owned(),
            }],
            artifact: None,
            exhaustive,
        },
        coverage,
    };
    candidate.validate()?;
    Ok(candidate)
}
