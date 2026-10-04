use graph::composition::{Digest, EvidenceSource, Owner, PrincipalBinding, ResourceKey};
use semwright_native_sdk::{
    cooperation::{ResourceVersion, RevisionToken},
    graph_adapter::{
        GRAPH_OBSERVATION_METHOD, GRAPH_OBSERVATION_METHOD_VERSION, native_locator,
        revision_candidate,
    },
    json,
};
use semwright_project_graph as graph;

fn owner() -> Owner {
    Owner {
        session: "native-sdk-graph-test".into(),
        principal: PrincipalBinding::Named("native-sdk-owner".into()),
    }
}

fn admit(
    graph: &mut graph::ProjectGraph,
    access: &graph::ProjectAccess,
    asset: &graph::LogicalAssetId,
    provider: &str,
    version: &ResourceVersion,
    projection: serde_json::Value,
    tick: u64,
) -> graph::RevisionRecord {
    let binding = graph.inspect(access, asset).unwrap().binding_generation;
    let candidate = revision_candidate(
        graph.project_id(),
        asset.clone(),
        binding,
        provider,
        "authenticated-driver-session",
        version,
        &projection,
        tick,
        graph::Coverage::unknown(),
        false,
    )
    .unwrap();
    let adapter = graph::RevisionAdapter::registered(
        ResourceKey {
            provider: provider.into(),
            resource: version.resource.clone(),
        },
        EvidenceSource::NativeApi,
        GRAPH_OBSERVATION_METHOD.into(),
        GRAPH_OBSERVATION_METHOD_VERSION,
    )
    .unwrap();
    let admitted = adapter
        .admit(&owner(), graph.project_id(), asset, binding, candidate)
        .unwrap();
    let record = admitted.record().clone();
    graph.accept_revision(access, admitted).unwrap();
    record
}

#[test]
fn native_observation_enters_only_through_canonical_graph_admission() {
    let project = graph::ProjectId::new();
    let access = graph::ProjectAccess::authorized(
        owner(),
        project.clone(),
        None,
        true,
        Digest::of_bytes(b"native-sdk-test-grants"),
    )
    .unwrap();
    let mut graph = graph::ProjectGraph::new(project, owner().principal).unwrap();
    let asset = graph::LogicalAssetId::new();
    let provider = "driver:native-inventory";
    let version = ResourceVersion {
        resource: "inventory".into(),
        generation: "incarnation-a".into(),
        revision: RevisionToken::new("90071992547409930000001").unwrap(),
    };
    graph
        .register(
            &access,
            graph::Asset {
                id: asset.clone(),
                resource_type: "native-inventory".into(),
                label: "Inventory".into(),
                locator: Some(native_locator(provider, "inventory", "inventory").unwrap()),
            },
        )
        .unwrap();

    let first = admit(
        &mut graph,
        &access,
        &asset,
        provider,
        &version,
        json!({"alpha":{"available":120,"reserved":0}}),
        1,
    );
    assert_eq!(first.observation.source, EvidenceSource::NativeApi);
    assert!(!first.coverage.complete);
    assert!(first.pin.fingerprint.bytes.is_none());
    assert!(first.pin.fingerprint.projection.is_some());

    let mut changed = version.clone();
    changed.revision = RevisionToken::new("90071992547409930000002").unwrap();
    let second = admit(
        &mut graph,
        &access,
        &asset,
        provider,
        &changed,
        json!({"alpha":{"available":113,"reserved":7}}),
        2,
    );
    assert_ne!(first.pin.revision, second.pin.revision);
    assert_ne!(
        first.pin.fingerprint.projection.as_ref().unwrap().digest,
        second.pin.fingerprint.projection.as_ref().unwrap().digest,
    );
    assert_eq!(
        graph.inspect(&access, &asset).unwrap().latest_revision,
        Some(second.pin.revision),
    );
}

#[test]
fn opaque_revision_and_independent_assets_remain_independent() {
    let project = graph::ProjectId::new();
    let access = graph::ProjectAccess::authorized(
        owner(),
        project.clone(),
        None,
        true,
        Digest::of_bytes(b"native-sdk-independent-grants"),
    )
    .unwrap();
    let mut graph = graph::ProjectGraph::new(project, owner().principal).unwrap();
    let provider = "driver:native-inventory";
    let left = graph::LogicalAssetId::new();
    let right = graph::LogicalAssetId::new();
    for (asset, stable, resource) in [
        (&left, "inventory-left", "inventory-left"),
        (&right, "inventory-right", "inventory-right"),
    ] {
        graph
            .register(
                &access,
                graph::Asset {
                    id: asset.clone(),
                    resource_type: "native-inventory".into(),
                    label: stable.into(),
                    locator: Some(native_locator(provider, stable, resource).unwrap()),
                },
            )
            .unwrap();
    }
    let left_v1 = ResourceVersion {
        resource: "inventory-left".into(),
        generation: "generation-left".into(),
        revision: RevisionToken::new("opaque:000000000000000000000000000000000001").unwrap(),
    };
    let right_v1 = ResourceVersion {
        resource: "inventory-right".into(),
        generation: "generation-right".into(),
        revision: RevisionToken::new("rev-without-numeric-semantics").unwrap(),
    };
    let left_first = admit(
        &mut graph,
        &access,
        &left,
        provider,
        &left_v1,
        json!({"value":"left-1"}),
        10,
    );
    let right_first = admit(
        &mut graph,
        &access,
        &right,
        provider,
        &right_v1,
        json!({"value":"right-1"}),
        11,
    );

    let mut left_v2 = left_v1.clone();
    left_v2.revision = RevisionToken::new("opaque:000000000000000000000000000000000002").unwrap();
    let left_second = admit(
        &mut graph,
        &access,
        &left,
        provider,
        &left_v2,
        json!({"value":"left-2"}),
        12,
    );
    assert_ne!(left_first.pin.revision, left_second.pin.revision);
    assert_eq!(
        graph.inspect(&access, &right).unwrap().latest_revision,
        Some(right_first.pin.revision),
    );
}

#[test]
fn native_projection_change_is_stale_independent_projection_is_current_and_partial_is_unknown() {
    let project = graph::ProjectId::new();
    let source = graph::LogicalAssetId::new();
    let independent = graph::LogicalAssetId::new();
    let provider = "driver:native-inventory";
    let source_v1 = ResourceVersion {
        resource: "source".into(),
        generation: "generation-a".into(),
        revision: RevisionToken::new("opaque-source-1").unwrap(),
    };
    let mut source_v2 = source_v1.clone();
    source_v2.revision = RevisionToken::new("opaque-source-2").unwrap();
    let independent_v1 = ResourceVersion {
        resource: "independent".into(),
        generation: "generation-a".into(),
        revision: RevisionToken::new("opaque-independent-1").unwrap(),
    };

    let source_old = revision_candidate(
        &project,
        source.clone(),
        1,
        provider,
        "authenticated-session-a",
        &source_v1,
        &json!({"value":1}),
        1,
        graph::Coverage::complete(),
        true,
    )
    .unwrap();
    let source_new = revision_candidate(
        &project,
        source,
        1,
        provider,
        "authenticated-session-a",
        &source_v2,
        &json!({"value":2}),
        2,
        graph::Coverage::complete(),
        true,
    )
    .unwrap();
    assert_eq!(
        graph::dependency_freshness(
            [(
                &source_old.fingerprint,
                &source_new.fingerprint,
                graph::Equivalence::Projection,
            )],
            true,
        ),
        graph::Freshness::Stale
    );

    let independent_old = revision_candidate(
        &project,
        independent.clone(),
        1,
        provider,
        "authenticated-session-a",
        &independent_v1,
        &json!({"value":"unchanged"}),
        3,
        graph::Coverage::complete(),
        true,
    )
    .unwrap();
    let independent_now = revision_candidate(
        &project,
        independent,
        1,
        provider,
        "authenticated-session-b",
        &independent_v1,
        &json!({"value":"unchanged"}),
        4,
        graph::Coverage::complete(),
        true,
    )
    .unwrap();
    assert_eq!(
        graph::dependency_freshness(
            [(
                &independent_old.fingerprint,
                &independent_now.fingerprint,
                graph::Equivalence::Projection,
            )],
            true,
        ),
        graph::Freshness::Current
    );
    assert_eq!(
        graph::dependency_freshness(
            [(
                &independent_old.fingerprint,
                &independent_now.fingerprint,
                graph::Equivalence::Projection,
            )],
            false,
        ),
        graph::Freshness::Unknown
    );
}
