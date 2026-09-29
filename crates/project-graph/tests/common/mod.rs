#![allow(dead_code)]
use composition::*;
use semwright_project_graph::*;
pub fn digest(text: &str) -> Digest {
    Digest::of_bytes(text.as_bytes())
}
pub fn owner() -> Owner {
    Owner {
        session: "test-session".into(),
        principal: PrincipalBinding::Named("test-owner".into()),
    }
}
pub fn setup() -> (ProjectGraph, ProjectAccess) {
    let p = ProjectId::new();
    let a = ProjectAccess::authorized(owner(), p.clone(), None, true, digest("grants")).unwrap();
    (ProjectGraph::new(p, owner().principal).unwrap(), a)
}
pub fn asset(graph: &mut ProjectGraph, access: &ProjectAccess, label: &str) -> LogicalAssetId {
    let id = LogicalAssetId::new();
    graph
        .register(
            access,
            Asset {
                id: id.clone(),
                resource_type: "model".into(),
                label: label.into(),
                locator: None,
            },
        )
        .unwrap();
    id
}
pub fn observation(id: &LogicalAssetId, bytes: &str, tick: u64) -> RevisionRecord {
    let fp = digest(bytes);
    let resource = ResourceKey {
        provider: "synthetic-contract-adapter".into(),
        resource: id.as_str().into(),
    };
    let base = BaseStateSet(vec![BaseState {
        key: resource.clone(),
        document_id: "disposable-contract-fixture".into(),
        provider_session: "fixture-provider".into(),
        generation: "1".into(),
        revision: Revision::Fingerprint(fp.clone()),
        concurrency: Concurrency::BestEffortRevalidate,
    }]);
    RevisionRecord {
        pin: RevisionPin {
            asset: id.clone(),
            revision: AssetRevision::new(),
            fingerprint: Fingerprint {
                bytes: Some(fp.clone()),
                projection: None,
            },
            equivalence: Equivalence::ExactBytes,
        },
        observed_unix_ms: tick,
        binding_generation: 1,
        observation: ObservationRef {
            id: format!("observation-{tick}"),
            base,
            source: EvidenceSource::FileRead,
            method: "contract-fixture-not-native-acceptance".into(),
            method_version: 1,
            scope: vec![Address {
                resource,
                logical_id: id.as_str().into(),
                property: "bytes".into(),
            }],
            artifact: Some(fp),
            exhaustive: true,
        },
        coverage: Coverage::complete(),
    }
}
pub fn accept_observation(
    graph: &mut ProjectGraph,
    access: &ProjectAccess,
    mut record: RevisionRecord,
) -> semwright_project_graph::Result<RevisionRecord> {
    let resource = record
        .observation
        .scope
        .first()
        .ok_or(GraphError::Invalid("fixture observation scope"))?
        .resource
        .clone();
    for state in &mut record.observation.base.0 {
        if state.key == resource {
            state.document_id = graph.project_id().as_str().into();
        }
    }
    let current_generation = graph.inspect(access, &record.pin.asset)?.binding_generation;
    let adapter = RevisionAdapter::registered(
        resource,
        record.observation.source,
        record.observation.method.clone(),
        record.observation.method_version,
    )?;
    let candidate = RevisionCandidate {
        asset: record.pin.asset.clone(),
        fingerprint: record.pin.fingerprint.clone(),
        equivalence: record.pin.equivalence,
        observed_unix_ms: record.observed_unix_ms,
        binding_generation: record.binding_generation,
        observation: record.observation,
        coverage: record.coverage,
    };
    let admitted = adapter.admit(
        &owner(),
        graph.project_id(),
        &record.pin.asset,
        current_generation,
        candidate,
    )?;
    let accepted = admitted.record().clone();
    graph.accept_revision(access, admitted)?;
    Ok(accepted)
}
pub fn observe(
    graph: &mut ProjectGraph,
    access: &ProjectAccess,
    id: &LogicalAssetId,
    bytes: &str,
    tick: u64,
) -> RevisionRecord {
    accept_observation(graph, access, observation(id, bytes, tick)).unwrap()
}
pub fn receipt(
    project: ProjectId,
    inputs: &[RevisionRecord],
    output: &RevisionRecord,
    tick: u64,
) -> ExecutionReceipt {
    let plan = digest("plan");
    let report = VerificationReport {
        execution_status: ExecutionStatus::Completed,
        support_level: SupportLevel::Composed,
        effects_observed: output.observation.scope.clone(),
        effects_unobservable: vec![],
        validation: ValidationReport {
            plan_digest: plan.clone(),
            base: output.observation.base.clone(),
            required_rules: ["bytes".into()].into(),
            checks: vec![RuleResult {
                rule: "bytes".into(),
                version: 1,
                verdict: Verdict::Pass,
                evidence_class: EvidenceClass::Deterministic,
                evidence: vec![output.observation.clone()],
                reason: None,
            }],
        },
    };
    ExecutionReceipt {
        version: 1,
        id: ReceiptId::new(),
        derivation: DerivationId::new(),
        project,
        owner: owner(),
        request_id: format!("request-{tick}"),
        operation: OperationIdentity {
            capability: "fixture.export".into(),
            descriptor: digest("descriptor"),
            runtime: digest("runtime"),
            plan,
            parameters: digest("parameters"),
            recipe: None,
        },
        source_base: BaseStateSet(
            inputs
                .iter()
                .flat_map(|r| r.observation.base.0.clone())
                .collect(),
        ),
        inputs: inputs.iter().map(|r| r.pin.clone()).collect(),
        outputs: vec![output.pin.clone()],
        determinants: vec![],
        coverage: Coverage::complete(),
        verification: report,
        completed_unix_ms: tick,
    }
}
pub fn admit(r: ExecutionReceipt) -> AdmittedReceipt {
    let adapter = ReceiptAdapter::registered(
        r.operation.capability.clone(),
        r.operation.descriptor.clone(),
        r.operation.runtime.clone(),
    )
    .unwrap();
    adapter
        .admit(&r.owner.clone(), &r.request_id.clone(), r)
        .unwrap()
}
pub fn declared(a: &LogicalAssetId, b: &LogicalAssetId, relation: Relation) -> Edge {
    Edge {
        from: Vertex::Asset(a.clone()),
        to: Vertex::Asset(b.clone()),
        relation,
        evidence: EdgeEvidence::Declared {
            declaration: ReceiptId::new(),
        },
    }
}
pub fn external_intent(
    graph: &ProjectGraph,
    affected: Vec<LogicalAssetId>,
    tick: u64,
) -> ExternalIntent {
    ExternalIntent {
        version: SCHEMA_VERSION,
        id: ExternalIntentId::new(),
        project: graph.project_id().clone(),
        owner: owner(),
        request_id: format!("request-{tick}"),
        operation: OperationIdentity {
            capability: "fixture.export".into(),
            descriptor: digest("descriptor"),
            runtime: digest("runtime"),
            plan: digest("plan"),
            parameters: digest("parameters"),
            recipe: None,
        },
        affected,
        prepared_unix_ms: tick,
        observation_epoch: graph.observation_epoch().into(),
        status: ExecutionStatus::Prepared,
        receipt: None,
    }
}
pub fn budget() -> TraversalBudget {
    TraversalBudget {
        nodes: 20_000,
        edges: 100_000,
        depth: 256,
        results: 10_000,
    }
}
