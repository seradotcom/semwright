//! Independent adversarial-lab Project Graph adversarial contract/store probe.
//! Synthetic IDs and files live only inside the disposable runner enclosure.
use rusqlite::Connection;
use semwright_project_graph::composition::*;
use semwright_project_graph::*;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

const SOURCE: &str = env!("G_LAB_COMPILED_SOURCE_SHA");
type ProbeResult<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn d(s: &str) -> Digest {
    Digest::of_bytes(s.as_bytes())
}
fn owner(session: &str) -> Owner {
    Owner {
        session: session.into(),
        principal: PrincipalBinding::Named("g-principal".into()),
    }
}
fn setup() -> ProbeResult<(ProjectGraph, ProjectAccess)> {
    let project = ProjectId::new();
    let authenticated = owner("g-session");
    let graph = ProjectGraph::new(project.clone(), authenticated.principal.clone())?;
    let access = ProjectAccess::authorized(authenticated, project, None, true, d("g-grants"))?;
    Ok((graph, access))
}
fn file(root: &str, path: &str) -> DurableLocator {
    DurableLocator::ScopedFile {
        root: root.into(),
        relative_path: path.into(),
    }
}
fn native(name: &str) -> DurableLocator {
    DurableLocator::Native {
        resource: ResourceKey {
            provider: "driver:g".into(),
            resource: name.into(),
        },
        stable_id: format!("stable-{name}"),
        resolver: "g-resolver".into(),
        resolver_version: 1,
    }
}
fn register_graph(
    graph: &mut ProjectGraph,
    access: &ProjectAccess,
    label: &str,
    locator: Option<DurableLocator>,
) -> semwright_project_graph::Result<LogicalAssetId> {
    let id = LogicalAssetId::new();
    graph.register(
        access,
        Asset {
            id: id.clone(),
            resource_type: "g-asset".into(),
            label: label.into(),
            locator,
        },
    )?;
    Ok(id)
}
fn register(
    graph: &mut ProjectGraph,
    access: &ProjectAccess,
    label: &str,
    locator: Option<DurableLocator>,
) -> ProbeResult<LogicalAssetId> {
    Ok(register_graph(graph, access, label, locator)?)
}
fn resource() -> ResourceKey {
    ResourceKey {
        provider: "driver:g-observer".into(),
        resource: "project".into(),
    }
}
fn address() -> Address {
    Address {
        resource: resource(),
        logical_id: "g-node".into(),
        property: "projection".into(),
    }
}
fn base() -> BaseStateSet {
    BaseStateSet(vec![BaseState {
        key: resource(),
        document_id: "g-doc".into(),
        provider_session: "g-provider-session".into(),
        generation: "g-generation".into(),
        revision: Revision::Counter(1),
        concurrency: Concurrency::BestEffortRevalidate,
    }])
}
fn revision_candidate(
    graph: &ProjectGraph,
    id: &LogicalAssetId,
    source: EvidenceSource,
    method: &str,
    tag: &str,
    coverage: Coverage,
) -> RevisionCandidate {
    let resource = resource();
    let fingerprint = d(tag);
    RevisionCandidate {
        version: SCHEMA_VERSION,
        asset: id.clone(),
        fingerprint: Fingerprint {
            bytes: Some(fingerprint.clone()),
            projection: None,
        },
        equivalence: Equivalence::ExactBytes,
        observed_unix_ms: 1,
        binding_generation: 1,
        observation: ObservationRef {
            id: format!("g-observation-{tag}"),
            base: BaseStateSet(vec![BaseState {
                key: resource.clone(),
                document_id: graph.project_id().as_str().into(),
                provider_session: "g-provider-session".into(),
                generation: "g-generation".into(),
                revision: Revision::Fingerprint(fingerprint.clone()),
                concurrency: Concurrency::BestEffortRevalidate,
            }]),
            source,
            method: method.into(),
            method_version: 1,
            scope: vec![Address {
                resource,
                logical_id: id.as_str().into(),
                property: "projection".into(),
            }],
            artifact: Some(fingerprint),
            exhaustive: true,
        },
        coverage,
    }
}
fn accept_revision_observation(
    graph: &mut ProjectGraph,
    access: &ProjectAccess,
    authenticated: &Owner,
    id: &LogicalAssetId,
    source: EvidenceSource,
    method: &str,
    tag: &str,
    coverage: Coverage,
) -> ProbeResult<RevisionRecord> {
    let candidate = revision_candidate(graph, id, source, method, tag, coverage);
    let generation = graph.inspect(access, id)?.binding_generation;
    let adapter = RevisionAdapter::registered(resource(), source, method.into(), 1)?;
    let admitted = adapter.admit(authenticated, graph.project_id(), id, generation, candidate)?;
    let record = admitted.record().clone();
    graph.accept_revision(access, admitted)?;
    Ok(record)
}
fn execution_receipt(
    project: ProjectId,
    authenticated: Owner,
    inputs: &[RevisionRecord],
    output: &RevisionRecord,
    tick: u64,
) -> ExecutionReceipt {
    let operation = OperationIdentity {
        capability: "g.fixture.export".into(),
        descriptor: d("g-receipt-descriptor"),
        runtime: d("g-receipt-runtime"),
        plan: d("g-receipt-plan"),
        parameters: d(&format!("g-receipt-parameters-{tick}")),
        recipe: None,
    };
    let report = VerificationReport {
        execution_status: ExecutionStatus::Completed,
        support_level: SupportLevel::Composed,
        effects_observed: output.observation.scope.clone(),
        effects_unobservable: vec![],
        validation: ValidationReport {
            plan_digest: operation.plan.clone(),
            base: output.observation.base.clone(),
            required_rules: ["g-output".into()].into(),
            checks: vec![RuleResult {
                rule: "g-output".into(),
                version: 1,
                verdict: Verdict::Pass,
                evidence_class: EvidenceClass::Deterministic,
                evidence: vec![output.observation.clone()],
                reason: None,
            }],
        },
    };
    ExecutionReceipt {
        version: SCHEMA_VERSION,
        id: ReceiptId::new(),
        derivation: DerivationId::new(),
        project,
        owner: authenticated,
        request_id: format!("g-receipt-request-{tick}"),
        operation,
        source_base: BaseStateSet(
            inputs
                .iter()
                .flat_map(|record| record.observation.base.0.clone())
                .collect(),
        ),
        inputs: inputs.iter().map(|record| record.pin.clone()).collect(),
        outputs: vec![output.pin.clone()],
        determinants: vec![],
        coverage: Coverage::complete(),
        verification: report,
        completed_unix_ms: tick,
    }
}
fn admit_receipt(receipt: ExecutionReceipt) -> ProbeResult<AdmittedReceipt> {
    let adapter = ReceiptAdapter::registered(
        receipt.operation.capability.clone(),
        receipt.operation.descriptor.clone(),
        receipt.operation.runtime.clone(),
    )?;
    Ok(adapter.admit(&receipt.owner.clone(), &receipt.request_id.clone(), receipt)?)
}
fn subset(
    graph: &ProjectGraph,
    session: &str,
    ids: BTreeSet<LogicalAssetId>,
    writable: bool,
) -> ProbeResult<ProjectAccess> {
    Ok(ProjectAccess::authorized(
        owner(session),
        graph.project_id().clone(),
        Some(ids),
        writable,
        d("g-grants"),
    )?)
}
fn manifest_one(relation: Option<Relation>) -> PortableManifest {
    let a = LogicalAssetId::new();
    let mut assets = vec![PortableAsset {
        source_id: a.clone(),
        label: "a".into(),
        resource_type: "g".into(),
    }];
    let mut declarations = vec![];
    if let Some(kind) = relation {
        let b = LogicalAssetId::new();
        assets.push(PortableAsset {
            source_id: b.clone(),
            label: "b".into(),
            resource_type: "g".into(),
        });
        declarations.push(PortableEdge {
            from: a,
            to: b,
            relation: kind,
        });
    }
    PortableManifest {
        version: SCHEMA_VERSION,
        source_project: ProjectId::new(),
        source_snapshot: 1,
        assets,
        declarations,
        coverage_complete: false,
    }
}
fn private_dir(tag: &str) -> ProbeResult<PathBuf> {
    let path = PathBuf::from(format!("/tmp/g-graph-{tag}-{}", std::process::id()));
    std::fs::create_dir(&path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(path)
}
fn store_access(project: &ProjectId, session: &str) -> ProbeResult<ProjectAccess> {
    Ok(ProjectAccess::authorized(
        owner(session),
        project.clone(),
        None,
        true,
        d("g-store-grants"),
    )?)
}
fn cleanup(path: &Path) {
    let _ = std::fs::remove_dir_all(path);
}
fn external_intent(
    graph: &ProjectGraph,
    historical_owner: Owner,
    affected: Vec<LogicalAssetId>,
    request_id: &str,
    id: ExternalIntentId,
) -> ExternalIntent {
    ExternalIntent {
        version: SCHEMA_VERSION,
        id,
        project: graph.project_id().clone(),
        owner: historical_owner,
        request_id: request_id.into(),
        operation: OperationIdentity {
            capability: "g-capability".into(),
            descriptor: d("g-descriptor"),
            runtime: d("g-runtime"),
            plan: d("g-plan"),
            parameters: d(request_id),
            recipe: None,
        },
        affected,
        prepared_unix_ms: 1,
        observation_epoch: graph.observation_epoch().into(),
        status: ExecutionStatus::Prepared,
        receipt: None,
    }
}

fn probe(case: &str) -> ProbeResult<Value> {
    Ok(match case {
        "G-GRAPH-001" => {
            let (mut graph, access) = setup()?;
            let id = register(&mut graph, &access, "new", Some(file("project", "a.txt")))?;
            let view = graph.inspect(&access, &id)?;
            json!({"label":view.knowledge.label(),"reconcile":view.knowledge.requires_reconcile,"latest":view.latest_revision.is_some()})
        }
        "G-GRAPH-002" => {
            let (mut graph, access) = setup()?;
            let id = register(
                &mut graph,
                &access,
                "fixture",
                Some(file("project", "a.txt")),
            )?;
            let _ = accept_revision_observation(
                &mut graph,
                &access,
                &owner("g-session"),
                &id,
                EvidenceSource::Fixture,
                "g-native-readback",
                "same",
                Coverage::complete(),
            )?;
            let k = graph.inspect(&access, &id)?.knowledge;
            json!({"label":k.label(),"cache_safe":k.cache_safe(),"reconcile":k.requires_reconcile})
        }
        "G-GRAPH-003" => {
            let (mut graph, access) = setup()?;
            let id = register(
                &mut graph,
                &access,
                "missing",
                Some(file("project", "a.txt")),
            )?;
            graph.record_probe(&access, &id, ProbeOutcome::ConclusiveNotFound)?;
            let k = graph.inspect(&access, &id)?.knowledge;
            json!({"label":k.label(),"reconcile":k.requires_reconcile})
        }
        "G-GRAPH-004" => {
            let (mut graph, access) = setup()?;
            let id = register(
                &mut graph,
                &access,
                "offline",
                Some(file("project", "a.txt")),
            )?;
            graph.record_probe(&access, &id, ProbeOutcome::Offline)?;
            let k = graph.inspect(&access, &id)?.knowledge;
            json!({"label":k.label(),"reconcile":k.requires_reconcile})
        }
        "G-GRAPH-005" => {
            let (mut graph, access) = setup()?;
            let id = register(
                &mut graph,
                &access,
                "rebind",
                Some(file("project", "a.txt")),
            )?;
            let _ = accept_revision_observation(
                &mut graph,
                &access,
                &owner("g-session"),
                &id,
                EvidenceSource::NativeApi,
                "g-native-readback",
                "v1",
                Coverage::complete(),
            )?;
            graph.rebind(
                &access,
                &id,
                1,
                file("project", "b.txt"),
                "synthetic-move".into(),
            )?;
            let view = graph.inspect(&access, &id)?;
            json!({"generation":view.binding_generation,"latest":view.latest_revision.is_some(),"label":view.knowledge.label()})
        }
        "G-GRAPH-006" => {
            let (mut graph, access) = setup()?;
            let id = register(
                &mut graph,
                &access,
                "instance",
                Some(file("project", "a.txt")),
            )?;
            graph.bind_instance(&access, &id, 1, d("instance-a"))?;
            let conflict = graph
                .bind_instance(&access, &id, 1, d("instance-b"))
                .is_err();
            json!({"replacement_rejected":conflict,"original_preserved":graph.bound_instance(&access,&id)?==Some(d("instance-a"))})
        }
        "G-GRAPH-007" => {
            let (mut graph, access) = setup()?;
            let id = register(
                &mut graph,
                &access,
                "generation",
                Some(file("project", "a.txt")),
            )?;
            json!({"wrong_generation_rejected":graph.rebind(&access,&id,2,file("project","b.txt"),"move".into()).is_err()})
        }
        "G-GRAPH-008" => {
            let (mut graph, access) = setup()?;
            let id = register(
                &mut graph,
                &access,
                "subset",
                Some(file("project", "a.txt")),
            )?;
            let limited = subset(&graph, "g-session", BTreeSet::from([id]), true)?;
            let values = vec![Determinant {
                class: DependencyClass::Runtime,
                key: "runtime".into(),
                digest: d("r"),
            }];
            json!({"subset_project_determinants_denied":graph.observe_determinants(&limited,values).is_err()})
        }
        "G-GRAPH-009" => {
            let (mut graph, access) = setup()?;
            let id = register(&mut graph, &access, "owner", Some(file("project", "a.txt")))?;
            let foreign_owner = Owner {
                session: "g-other".into(),
                principal: PrincipalBinding::Named("foreign-principal".into()),
            };
            let foreign = ProjectAccess::authorized(
                foreign_owner,
                graph.project_id().clone(),
                None,
                true,
                d("g-grants"),
            )?;
            json!({"cross_owner_denied":graph.inspect(&foreign,&id).is_err()})
        }
        "G-GRAPH-010" => {
            let (mut graph, access) = setup()?;
            register(&mut graph, &access, "alpha", Some(file("alpha", "a.txt")))?;
            register(&mut graph, &access, "beta", Some(file("beta", "b.txt")))?;
            register(&mut graph, &access, "native", Some(native("scene")))?;
            let scoped = graph.file_scope(&access, "alpha")?;
            let mut cursors = QueryCursors::default();
            let page = cursors.page(&graph, &scoped, &AssetQuery::default(), None, 256)?;
            json!({"visible":page.items.len(),"partial":page.scope_partial,"only_alpha":page.items.iter().all(|v|matches!(&v.asset.locator,Some(DurableLocator::ScopedFile{root,..}) if root=="alpha"))})
        }
        "G-GRAPH-011" => {
            let (mut graph, access) = setup()?;
            register(&mut graph, &access, "a", Some(file("project", "a.txt")))?;
            register(&mut graph, &access, "b", Some(file("project", "b.txt")))?;
            let query = AssetQuery::default();
            let mut cursors = QueryCursors::default();
            let first = cursors.page(&graph, &access, &query, None, 1)?;
            let token = first.next_cursor.as_deref().expect("continuation");
            let other = ProjectAccess::authorized(
                owner("other-session"),
                graph.project_id().clone(),
                None,
                true,
                d("g-grants"),
            )?;
            json!({"cross_session_cursor_denied":cursors.page(&graph,&other,&query,Some(token),1).is_err()})
        }
        "G-GRAPH-012" => {
            let (mut graph, access) = setup()?;
            register(&mut graph, &access, "a", Some(file("project", "a.txt")))?;
            register(&mut graph, &access, "b", Some(file("project", "b.txt")))?;
            let query = AssetQuery::default();
            let mut cursors = QueryCursors::default();
            let first = cursors.page(&graph, &access, &query, None, 1)?;
            let token = first.next_cursor.clone().expect("continuation");
            register(&mut graph, &access, "c", Some(file("project", "c.txt")))?;
            json!({"mutated_snapshot_cursor_rejected":cursors.page(&graph,&access,&query,Some(&token),1).is_err()})
        }
        "G-GRAPH-013" => {
            json!({"verified_by_manifest_rejected":manifest_one(Some(Relation::VerifiedBy)).validate().is_err()})
        }
        "G-GRAPH-014" => {
            let (mut graph, access) = setup()?;
            let manifest = manifest_one(None);
            let source = manifest.assets[0].source_id.clone();
            let remap = graph.import_manifest(&access, &manifest)?;
            let local = remap.get(&source).expect("mapping");
            let view = graph.inspect(&access, local)?;
            json!({"identity_remapped":local!=&source,"locator_not_imported":view.asset.locator.is_none(),"label":view.asset.label})
        }
        "G-GRAPH-015" => {
            let (mut graph, access) = setup()?;
            let a = register(&mut graph, &access, "a", Some(file("project", "a.txt")))?;
            let b = register(&mut graph, &access, "b", Some(file("project", "b.txt")))?;
            let edge = Edge {
                from: Vertex::Asset(a),
                to: Vertex::Asset(b),
                relation: Relation::VerifiedBy,
                evidence: EdgeEvidence::Declared {
                    declaration: ReceiptId::new(),
                },
            };
            json!({"declaration_cannot_certify":graph.declare(&access,edge).is_err()})
        }
        "G-GRAPH-016" => {
            let (mut graph, access) = setup()?;
            let a = register(&mut graph, &access, "a", Some(file("project", "a.txt")))?;
            let b = register(&mut graph, &access, "b", Some(file("project", "b.txt")))?;
            graph.declare(
                &access,
                Edge {
                    from: Vertex::Asset(a.clone()),
                    to: Vertex::Asset(b.clone()),
                    relation: Relation::Contains,
                    evidence: EdgeEvidence::Declared {
                        declaration: ReceiptId::new(),
                    },
                },
            )?;
            let reverse = Edge {
                from: Vertex::Asset(b),
                to: Vertex::Asset(a),
                relation: Relation::Contains,
                evidence: EdgeEvidence::Declared {
                    declaration: ReceiptId::new(),
                },
            };
            json!({"contains_cycle_rejected":graph.declare(&access,reverse).is_err()})
        }
        "G-GRAPH-017" => {
            let (mut graph, access) = setup()?;
            let a = register(&mut graph, &access, "a", Some(file("project", "a.txt")))?;
            let mut observer =
                ScopedObserver::register(&mut graph, &access, BTreeSet::from([a.clone()]))?;
            let hint = WatchHint::Changed(vec![a]);
            let first = observer.event(&mut graph, &access, 1, hint.clone())?;
            let duplicate = observer.event(&mut graph, &access, 1, hint)?;
            json!({"first_duplicate":first.duplicate,"second_duplicate":duplicate.duplicate,"requires_rescan":duplicate.requires_rescan})
        }
        "G-GRAPH-018" => {
            let (mut graph, access) = setup()?;
            let a = register(&mut graph, &access, "a", Some(file("project", "a.txt")))?;
            let b = register(&mut graph, &access, "b", Some(file("project", "b.txt")))?;
            let mut observer =
                ScopedObserver::register(&mut graph, &access, BTreeSet::from([a, b]))?;
            let update = observer.event(&mut graph, &access, 2, WatchHint::ProviderOffline)?;
            json!({"gap_invalidates_full_scope":update.scope.len()==2,"requires_rescan":update.requires_rescan})
        }
        "G-GRAPH-019" => {
            let a = Fingerprint {
                bytes: Some(d("a")),
                projection: None,
            };
            let b = Fingerprint {
                bytes: Some(d("b")),
                projection: None,
            };
            let c = Fingerprint::default();
            let e = Fingerprint::default();
            json!({"freshness":format!("{:?}",dependency_freshness([(&a,&b,Equivalence::ExactBytes),(&c,&e,Equivalence::ExactBytes)],true))})
        }
        "G-GRAPH-020" => {
            let a = Fingerprint {
                bytes: Some(d("same")),
                projection: None,
            };
            let b = a.clone();
            json!({"freshness":format!("{:?}",dependency_freshness([(&a,&b,Equivalence::ExactBytes)],false))})
        }
        "G-GRAPH-021" => {
            json!({"zero_traversal_budget_rejected":TraversalBudget{nodes:0,edges:1,depth:1,results:1}.validate().is_err()})
        }
        "G-GRAPH-022" => {
            let (mut graph, access) = setup()?;
            let a = register(&mut graph, &access, "a", Some(file("project", "a.txt")))?;
            let cancel = AtomicBool::new(true);
            let report = graph.impact(
                &access,
                &a,
                TraversalBudget {
                    nodes: 10,
                    edges: 10,
                    depth: 4,
                    results: 10,
                },
                &cancel,
            )?;
            json!({"cancelled":report.cancelled,"truncated":report.truncated,"known":report.known.len(),"possible":report.possible.len()})
        }
        "G-GRAPH-023" => {
            json!({"malformed_project_id_rejected":ProjectId::parse("prj_bad".into()).is_err()})
        }
        "G-GRAPH-024" => {
            json!({"path_traversal_rejected":file("project","../escape").validate().is_err()})
        }
        "G-GRAPH-025" => {
            json!({"portable_device_name_rejected":file("project","CON.txt").validate().is_err()})
        }
        "G-GRAPH-026" => {
            let manifest = manifest_one(None);
            let mut value = serde_json::to_value(&manifest)?;
            value
                .as_object_mut()
                .expect("object")
                .insert("authority".into(), json!("admin"));
            json!({"unknown_authority_field_rejected":PortableManifest::decode(&serde_json::to_vec(&value)?).is_err()})
        }
        "G-GRAPH-027" => {
            let (mut graph, access) = setup()?;
            let a = register(&mut graph, &access, "a", Some(file("project", "a.txt")))?;
            let limited = subset(&graph, "g-session", BTreeSet::from([a.clone()]), false)?;
            json!({"subset_revision_history_denied":graph.revisions(&limited,&a,None,10).is_err()})
        }
        "G-GRAPH-028" => {
            let (mut graph, access) = setup()?;
            let a = register(&mut graph, &access, "a", Some(file("project", "a.txt")))?;
            register(&mut graph, &access, "b", Some(file("project", "b.txt")))?;
            let limited = subset(&graph, "g-session", BTreeSet::from([a]), false)?;
            let mut cursors = QueryCursors::default();
            let page = cursors.page(&graph, &limited, &AssetQuery::default(), None, 256)?;
            json!({"items":page.items.len(),"scope_partial":page.scope_partial})
        }
        "G-GRAPH-029" => {
            let (mut graph, access) = setup()?;
            register(&mut graph, &access, "a", Some(file("project", "a.txt")))?;
            register(&mut graph, &access, "b", Some(file("project", "b.txt")))?;
            let query = AssetQuery::default();
            let mut cursors = QueryCursors::default();
            let token = cursors
                .page(&graph, &access, &query, None, 1)?
                .next_cursor
                .expect("continuation");
            graph.restart_observation_epoch();
            json!({"old_epoch_cursor_rejected":cursors.page(&graph,&access,&query,Some(&token),1).is_err()})
        }
        "G-GRAPH-030" => {
            let (mut graph, access) = setup()?;
            let a = register(
                &mut graph,
                &access,
                "same",
                Some(file("project", "same.bin")),
            )?;
            let b = register(
                &mut graph,
                &access,
                "same",
                Some(file("project", "same.bin")),
            )?;
            json!({"same_locator_not_identity":a!=b,"assets":2})
        }
        "G-GRAPH-031" => {
            let (mut graph, access) = setup()?;
            let a = register(&mut graph, &access, "a", Some(file("project", "a.txt")))?;
            let _ = accept_revision_observation(
                &mut graph,
                &access,
                &owner("g-session"),
                &a,
                EvidenceSource::NativeApi,
                "g-native-readback",
                "v1",
                Coverage::complete(),
            )?;
            let k = graph.inspect(&access, &a)?.knowledge;
            json!({"label":k.label(),"cache_safe":k.cache_safe(),"verification":format!("{:?}",k.verification),"divergence":format!("{:?}",k.divergence)})
        }
        "G-GRAPH-032" => {
            let (mut graph, access) = setup()?;
            let a = register(&mut graph, &access, "a", Some(file("project", "a.txt")))?;
            let exported = graph.export_manifest(&access, &[a])?;
            json!({"coverage_complete":exported.coverage_complete,"assets":exported.assets.len()})
        }
        "G-GRAPH-033" => {
            let project = ProjectId::new();
            let principal = owner("g-store").principal;
            let access = store_access(&project, "g-store")?;
            let dir = private_dir("reopen")?;
            {
                let mut store = GraphStore::open(&dir, project.clone(), principal.clone(), false)?;
                store.transact(&access, |g| {
                    let _ = register_graph(g, &access, "stored", Some(file("project", "a.txt")))?;
                    Ok(())
                })?;
            }
            let reopened = GraphStore::open(&dir, project, principal, false)?;
            let out = json!({"journal":reopened.recovery_report().journal_records,"assets":reopened.recovery_report().assets,"epoch_reset":reopened.recovery_report().observation_epoch_reset});
            cleanup(&dir);
            out
        }
        "G-GRAPH-034" => {
            let project = ProjectId::new();
            let principal = owner("g-store").principal;
            let dir = private_dir("owner")?;
            {
                let _ = GraphStore::open(&dir, project.clone(), principal.clone(), false)?;
            }
            let denied = GraphStore::open(
                &dir,
                project,
                PrincipalBinding::Named("foreign".into()),
                false,
            )
            .is_err();
            cleanup(&dir);
            json!({"wrong_principal_rejected":denied})
        }
        "G-GRAPH-035" => {
            let project = ProjectId::new();
            let principal = owner("g-store").principal;
            let dir = private_dir("symlink")?;
            let target = dir.join("target.sqlite3");
            std::fs::write(&target, b"synthetic")?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::{PermissionsExt, symlink};
                std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600))?;
                symlink(&target, dir.join("project.sqlite3"))?;
            }
            let denied = GraphStore::open(&dir, project, principal, false).is_err();
            cleanup(&dir);
            json!({"symlink_store_rejected":denied})
        }
        "G-GRAPH-036" => {
            let project = ProjectId::new();
            let principal = owner("g-store").principal;
            let dir = private_dir("corrupt")?;
            let db = dir.join("project.sqlite3");
            std::fs::write(&db, b"not sqlite")?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&db, std::fs::Permissions::from_mode(0o600))?;
            }
            let denied = GraphStore::open(&dir, project, principal, false).is_err();
            cleanup(&dir);
            json!({"corrupt_store_rejected":denied})
        }
        "G-GRAPH-037" => {
            let project = ProjectId::new();
            let principal = owner("g-store").principal;
            let access = store_access(&project, "g-store")?;
            let dir = private_dir("repair")?;
            {
                let mut store = GraphStore::open(&dir, project.clone(), principal.clone(), false)?;
                store.transact(&access, |g| {
                    let _ = register_graph(g, &access, "stored", Some(file("project", "a.txt")))?;
                    Ok(())
                })?;
            }
            let db = dir.join("project.sqlite3");
            {
                let connection = Connection::open(&db)?;
                connection.execute("DELETE FROM assets", [])?;
            }
            let strict = GraphStore::open(&dir, project.clone(), principal.clone(), false).is_err();
            let repaired = GraphStore::open(&dir, project, principal, true)?;
            let rebuilt = repaired.recovery_report().indexes_rebuilt;
            cleanup(&dir);
            json!({"strict_rejects_bad_index":strict,"explicit_repair_rebuilds_index":rebuilt})
        }
        "G-GRAPH-038" => {
            let project = ProjectId::new();
            let principal = owner("g-store").principal;
            let dir = private_dir("objects")?;
            {
                let _ = GraphStore::open(&dir, project.clone(), principal.clone(), false)?;
            }
            {
                let connection = Connection::open(dir.join("project.sqlite3"))?;
                connection.execute("CREATE TABLE g_unexpected(x INTEGER)", [])?;
            }
            let denied = GraphStore::open(&dir, project, principal, false).is_err();
            cleanup(&dir);
            json!({"unexpected_schema_object_rejected":denied})
        }
        "G-GRAPH-039" => {
            let project = ProjectId::new();
            let principal = owner("g-store").principal;
            let dir = private_dir("mode")?;
            {
                let _ = GraphStore::open(&dir, project.clone(), principal.clone(), false)?;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(
                    dir.join("project.sqlite3"),
                    std::fs::Permissions::from_mode(0o644),
                )?;
            }
            let denied = GraphStore::open(&dir, project, principal, false).is_err();
            cleanup(&dir);
            json!({"world_readable_store_rejected":denied})
        }

        "G-GRAPH-040" => {
            let (mut graph, access) = setup()?;
            let a = register(
                &mut graph,
                &access,
                "intent",
                Some(file("project", "intent.bin")),
            )?;
            let intent = external_intent(
                &graph,
                owner("g-session"),
                vec![a],
                "g-request-040",
                ExternalIntentId::new(),
            );
            let id = intent.id.clone();
            graph.prepare_external_intent(&access, intent)?;
            let view = graph.external_intent(&access, &id)?;
            json!({"status":format!("{:?}",view.status),"receipt":view.receipt.is_some(),"request":view.request_id})
        }
        "G-GRAPH-041" => {
            let (mut graph, access) = setup()?;
            let a = register(
                &mut graph,
                &access,
                "intent",
                Some(file("project", "intent.bin")),
            )?;
            let mut intent = external_intent(
                &graph,
                owner("g-session"),
                vec![a],
                "g-request-041",
                ExternalIntentId::new(),
            );
            intent.project = ProjectId::new();
            json!({"wrong_project_rejected":graph.prepare_external_intent(&access,intent).is_err()})
        }
        "G-GRAPH-042" => {
            let (mut graph, access) = setup()?;
            let a = register(
                &mut graph,
                &access,
                "intent",
                Some(file("project", "intent.bin")),
            )?;
            let intent = external_intent(
                &graph,
                owner("other-session"),
                vec![a],
                "g-request-042",
                ExternalIntentId::new(),
            );
            json!({"session_substitution_rejected":graph.prepare_external_intent(&access,intent).is_err()})
        }
        "G-GRAPH-043" => {
            let (mut graph, access) = setup()?;
            let a = register(
                &mut graph,
                &access,
                "intent",
                Some(file("project", "intent.bin")),
            )?;
            let intent = external_intent(
                &graph,
                owner("g-session"),
                vec![a.clone(), a],
                "g-request-043",
                ExternalIntentId::new(),
            );
            json!({"duplicate_affected_rejected":graph.prepare_external_intent(&access,intent).is_err()})
        }
        "G-GRAPH-044" => {
            let (mut graph, access) = setup()?;
            let a = register(
                &mut graph,
                &access,
                "intent",
                Some(file("project", "intent.bin")),
            )?;
            let first = external_intent(
                &graph,
                owner("g-session"),
                vec![a.clone()],
                "g-request-044",
                ExternalIntentId::new(),
            );
            graph.prepare_external_intent(&access, first)?;
            let second = external_intent(
                &graph,
                owner("g-session"),
                vec![a],
                "g-request-044",
                ExternalIntentId::new(),
            );
            json!({"request_identity_collision_rejected":graph.prepare_external_intent(&access,second).is_err()})
        }
        "G-GRAPH-045" => {
            let (mut graph, access) = setup()?;
            let a = register(
                &mut graph,
                &access,
                "intent",
                Some(file("project", "intent.bin")),
            )?;
            let intent = external_intent(
                &graph,
                owner("g-session"),
                vec![a],
                "g-request-045",
                ExternalIntentId::new(),
            );
            let id = intent.id.clone();
            graph.prepare_external_intent(&access, intent.clone())?;
            let idempotent = graph.prepare_external_intent(&access, intent).is_ok();
            let status = format!("{:?}", graph.external_intent(&access, &id)?.status);
            json!({"exact_duplicate_idempotent":idempotent,"status":status})
        }
        "G-GRAPH-046" => {
            let (mut graph, access) = setup()?;
            let a = register(
                &mut graph,
                &access,
                "intent",
                Some(file("project", "intent.bin")),
            )?;
            let intent = external_intent(
                &graph,
                owner("g-session"),
                vec![a],
                "g-request-046",
                ExternalIntentId::new(),
            );
            graph.prepare_external_intent(&access, intent.clone())?;
            let mut mutated = intent;
            mutated.prepared_unix_ms = 2;
            json!({"same_id_mutation_rejected":graph.prepare_external_intent(&access,mutated).is_err()})
        }
        "G-GRAPH-047" => {
            let (mut graph, access) = setup()?;
            let a = register(
                &mut graph,
                &access,
                "intent",
                Some(file("project", "intent.bin")),
            )?;
            let intent = external_intent(
                &graph,
                owner("g-session"),
                vec![a],
                "g-request-047",
                ExternalIntentId::new(),
            );
            let id = intent.id.clone();
            graph.prepare_external_intent(&access, intent)?;
            graph.mark_external_intent_applying(&access, &id)?;
            let second = graph.mark_external_intent_applying(&access, &id).is_err();
            let status = format!("{:?}", graph.external_intent(&access, &id)?.status);
            json!({"double_dispatch_rejected":second,"status":status})
        }
        "G-GRAPH-048" => {
            let (mut graph, access) = setup()?;
            let a = register(
                &mut graph,
                &access,
                "intent",
                Some(file("project", "intent.bin")),
            )?;
            let intent = external_intent(
                &graph,
                owner("g-session"),
                vec![a],
                "g-request-048",
                ExternalIntentId::new(),
            );
            let id = intent.id.clone();
            graph.prepare_external_intent(&access, intent)?;
            graph.mark_external_intent_applying(&access, &id)?;
            graph.restart_observation_epoch();
            let status = format!("{:?}", graph.external_intent(&access, &id)?.status);
            let redispatch = graph.mark_external_intent_applying(&access, &id).is_err();
            json!({"status_after_restart":status,"redispatch_rejected":redispatch})
        }
        "G-GRAPH-049" => {
            let (mut graph, access) = setup()?;
            let a = register(
                &mut graph,
                &access,
                "intent",
                Some(file("project", "intent.bin")),
            )?;
            let intent = external_intent(
                &graph,
                owner("g-session"),
                vec![a],
                "g-request-049",
                ExternalIntentId::new(),
            );
            let id = intent.id.clone();
            graph.prepare_external_intent(&access, intent)?;
            graph.mark_external_intent_applying(&access, &id)?;
            graph.restart_observation_epoch();
            let completed = graph
                .resolve_external_intent(&access, &id, ExecutionStatus::Completed, None)
                .is_err();
            let unknown = graph
                .resolve_external_intent(&access, &id, ExecutionStatus::Unknown, None)
                .is_ok();
            let status = format!("{:?}", graph.external_intent(&access, &id)?.status);
            json!({"completed_after_restart_rejected":completed,"unknown_resolution_allowed":unknown,"status":status})
        }
        "G-GRAPH-050" => {
            let (mut graph, access) = setup()?;
            let visible = register(
                &mut graph,
                &access,
                "visible",
                Some(file("project", "visible.bin")),
            )?;
            let hidden = register(
                &mut graph,
                &access,
                "hidden",
                Some(file("project", "hidden.bin")),
            )?;
            let intent = external_intent(
                &graph,
                owner("g-session"),
                vec![hidden],
                "g-request-050",
                ExternalIntentId::new(),
            );
            let id = intent.id.clone();
            graph.prepare_external_intent(&access, intent)?;
            let scoped = subset(&graph, "g-scoped", BTreeSet::from([visible]), false)?;
            json!({"hidden_intent_denied":graph.external_intent(&scoped,&id).is_err()})
        }
        "G-GRAPH-051" => {
            let project = ProjectId::new();
            let principal = owner("g-store").principal;
            let write_access = store_access(&project, "g-store")?;
            let dir = private_dir("intent-restart")?;
            let intent_id = ExternalIntentId::new();
            {
                let mut store = GraphStore::open(&dir, project.clone(), principal.clone(), false)?;
                store.transact(&write_access, |g| {
                    let a = register_graph(
                        g,
                        &write_access,
                        "durable-intent",
                        Some(file("project", "intent.bin")),
                    )?;
                    let intent = external_intent(
                        g,
                        owner("g-store"),
                        vec![a],
                        "g-request-051",
                        intent_id.clone(),
                    );
                    g.prepare_external_intent(&write_access, intent)?;
                    g.mark_external_intent_applying(&write_access, &intent_id)?;
                    Ok(())
                })?;
            }
            let mut reopened = GraphStore::open(&dir, project.clone(), principal, false)?;
            let restart_access = store_access(&project, "g-store-restart")?;
            let status = format!(
                "{:?}",
                reopened
                    .graph()?
                    .external_intent(&restart_access, &intent_id)?
                    .status
            );
            let redispatch = reopened.transact(&restart_access, |g| {
                Ok(g.mark_external_intent_applying(&restart_access, &intent_id)
                    .is_err())
            })?;
            let resolved = reopened.transact(&restart_access, |g| {
                Ok(g.resolve_external_intent(
                    &restart_access,
                    &intent_id,
                    ExecutionStatus::Unknown,
                    None,
                )
                .is_ok())
            })?;
            cleanup(&dir);
            json!({"status_after_reopen":status,"redispatch_rejected":redispatch,"unknown_resolution_allowed":resolved})
        }
        "G-GRAPH-052" => {
            let (mut graph, access) = setup()?;
            let a = register(
                &mut graph,
                &access,
                "intent",
                Some(file("project", "intent.bin")),
            )?;
            let intent = external_intent(
                &graph,
                owner("g-session"),
                vec![a],
                "g-request-052",
                ExternalIntentId::new(),
            );
            let id = intent.id.clone();
            graph.prepare_external_intent(&access, intent)?;
            let foreign = ProjectAccess::authorized(
                Owner {
                    session: "foreign-session".into(),
                    principal: PrincipalBinding::Named("foreign-principal".into()),
                },
                graph.project_id().clone(),
                None,
                false,
                d("foreign-grants"),
            )?;
            json!({"cross_principal_intent_denied":graph.external_intent(&foreign,&id).is_err()})
        }
        "G-GRAPH-053" => {
            let (mut graph, access) = setup()?;
            let asset = register(&mut graph, &access, "native-admitted", None)?;
            let candidate = revision_candidate(
                &graph,
                &asset,
                EvidenceSource::NativeApi,
                "g-native-final",
                "053",
                Coverage::unknown(),
            );
            let adapter = RevisionAdapter::registered(
                resource(),
                EvidenceSource::NativeApi,
                "g-native-final".into(),
                1,
            )?;
            let admitted = adapter.admit(
                &owner("g-session"),
                graph.project_id(),
                &asset,
                1,
                candidate,
            )?;
            graph.accept_revision(&access, admitted)?;
            let view = graph.inspect(&access, &asset)?;
            let provenance = graph.provenance(&access, &asset, 256)?;
            json!({"admitted":view.latest_revision.is_some(),"producer":provenance.producer.is_some()})
        }
        "G-GRAPH-054" => {
            let (mut graph, access) = setup()?;
            let asset = register(&mut graph, &access, "project-substitution", None)?;
            let mut candidate = revision_candidate(
                &graph,
                &asset,
                EvidenceSource::NativeApi,
                "g-native-final",
                "054",
                Coverage::unknown(),
            );
            candidate.observation.base.0[0].document_id = ProjectId::new().as_str().into();
            let adapter = RevisionAdapter::registered(
                resource(),
                EvidenceSource::NativeApi,
                "g-native-final".into(),
                1,
            )?;
            json!({"project_substitution_rejected":adapter.admit(&owner("g-session"),graph.project_id(),&asset,1,candidate).is_err()})
        }
        "G-GRAPH-055" => {
            let (mut graph, access) = setup()?;
            let asset = register(&mut graph, &access, "asset-substitution", None)?;
            let other = register(&mut graph, &access, "other-asset", None)?;
            let mut candidate = revision_candidate(
                &graph,
                &asset,
                EvidenceSource::NativeApi,
                "g-native-final",
                "055",
                Coverage::unknown(),
            );
            candidate.asset = other;
            let adapter = RevisionAdapter::registered(
                resource(),
                EvidenceSource::NativeApi,
                "g-native-final".into(),
                1,
            )?;
            json!({"asset_substitution_rejected":adapter.admit(&owner("g-session"),graph.project_id(),&asset,1,candidate).is_err()})
        }
        "G-GRAPH-056" => {
            let (mut graph, access) = setup()?;
            let asset = register(&mut graph, &access, "generation-substitution", None)?;
            let mut candidate = revision_candidate(
                &graph,
                &asset,
                EvidenceSource::NativeApi,
                "g-native-final",
                "056",
                Coverage::unknown(),
            );
            candidate.binding_generation = 2;
            let adapter = RevisionAdapter::registered(
                resource(),
                EvidenceSource::NativeApi,
                "g-native-final".into(),
                1,
            )?;
            json!({"generation_substitution_rejected":adapter.admit(&owner("g-session"),graph.project_id(),&asset,1,candidate).is_err()})
        }
        "G-GRAPH-057" => {
            let (mut graph, access) = setup()?;
            let asset = register(&mut graph, &access, "source-substitution", None)?;
            let candidate = revision_candidate(
                &graph,
                &asset,
                EvidenceSource::FileRead,
                "g-native-final",
                "057",
                Coverage::unknown(),
            );
            let adapter = RevisionAdapter::registered(
                resource(),
                EvidenceSource::NativeApi,
                "g-native-final".into(),
                1,
            )?;
            json!({"source_substitution_rejected":adapter.admit(&owner("g-session"),graph.project_id(),&asset,1,candidate).is_err()})
        }
        "G-GRAPH-058" => {
            let (mut graph, access) = setup()?;
            let asset = register(&mut graph, &access, "method-substitution", None)?;
            let candidate = revision_candidate(
                &graph,
                &asset,
                EvidenceSource::NativeApi,
                "g-native-final",
                "058",
                Coverage::unknown(),
            );
            let adapter = RevisionAdapter::registered(
                resource(),
                EvidenceSource::NativeApi,
                "g-native-final".into(),
                1,
            )?;
            let mut wrong_method = candidate.clone();
            wrong_method.observation.method = "client-method".into();
            let method_rejected = adapter
                .admit(
                    &owner("g-session"),
                    graph.project_id(),
                    &asset,
                    1,
                    wrong_method,
                )
                .is_err();
            let mut wrong_version = candidate;
            wrong_version.observation.method_version = 2;
            let version_rejected = adapter
                .admit(
                    &owner("g-session"),
                    graph.project_id(),
                    &asset,
                    1,
                    wrong_version,
                )
                .is_err();
            json!({"method_substitution_rejected":method_rejected,"version_substitution_rejected":version_rejected})
        }
        "G-GRAPH-059" => {
            let (mut graph, access) = setup()?;
            let asset = register(&mut graph, &access, "scope-substitution", None)?;
            let candidate = revision_candidate(
                &graph,
                &asset,
                EvidenceSource::NativeApi,
                "g-native-final",
                "059",
                Coverage::unknown(),
            );
            let mut empty = candidate.clone();
            empty.observation.scope.clear();
            let empty_rejected = empty.validate().is_err();
            let mut foreign = candidate;
            foreign.observation.scope = vec![Address {
                resource: ResourceKey {
                    provider: "foreign-provider".into(),
                    resource: "foreign-resource".into(),
                },
                logical_id: "foreign-node".into(),
                property: "projection".into(),
            }];
            let foreign_rejected = foreign.validate().is_err();
            json!({"empty_scope_rejected":empty_rejected,"foreign_scope_resource_rejected":foreign_rejected})
        }
        "G-GRAPH-060" => {
            let (mut graph, access) = setup()?;
            let asset = register(&mut graph, &access, "false-complete", None)?;
            let mut candidate = revision_candidate(
                &graph,
                &asset,
                EvidenceSource::NativeApi,
                "g-native-final",
                "060",
                Coverage::complete(),
            );
            candidate.observation.exhaustive = false;
            json!({"false_complete_rejected":candidate.validate().is_err()})
        }
        "G-GRAPH-061" => {
            let (mut graph, access) = setup()?;
            let asset = register(&mut graph, &access, "partial-admission", None)?;
            let coverage = Coverage {
                complete: false,
                unknown_frontier: BTreeSet::from([DependencyClass::ImportSettings]),
            };
            let _ = accept_revision_observation(
                &mut graph,
                &access,
                &owner("g-session"),
                &asset,
                EvidenceSource::NativeApi,
                "g-native-final",
                "061",
                coverage,
            )?;
            let knowledge = graph.inspect(&access, &asset)?.knowledge;
            json!({"verification":format!("{:?}",knowledge.verification),"coverage_cache_safe":knowledge.coverage.cache_safe(),"cache_safe":knowledge.cache_safe()})
        }
        "G-GRAPH-062" => {
            let (mut graph, access) = setup()?;
            let asset = register(&mut graph, &access, "owner-bound", None)?;
            let candidate = revision_candidate(
                &graph,
                &asset,
                EvidenceSource::NativeApi,
                "g-native-final",
                "062",
                Coverage::unknown(),
            );
            let adapter = RevisionAdapter::registered(
                resource(),
                EvidenceSource::NativeApi,
                "g-native-final".into(),
                1,
            )?;
            let admitted = adapter.admit(
                &owner("g-session"),
                graph.project_id(),
                &asset,
                1,
                candidate,
            )?;
            let other = ProjectAccess::authorized(
                owner("other-session"),
                graph.project_id().clone(),
                None,
                true,
                d("g-grants"),
            )?;
            json!({"owner_substitution_rejected":graph.accept_revision(&other,admitted).is_err()})
        }
        "G-GRAPH-063" => {
            let (mut graph, access) = setup()?;
            let source = register(&mut graph, &access, "provenance-limit", None)?;
            json!({"zero_limit_rejected":graph.provenance(&access,&source,0).is_err(),"oversize_limit_rejected":graph.provenance(&access,&source,257).is_err()})
        }
        "G-GRAPH-064" => {
            let (mut graph, access) = setup()?;
            let source = register(&mut graph, &access, "hidden-source", None)?;
            let end = register(&mut graph, &access, "visible-output", None)?;
            let source_record = accept_revision_observation(
                &mut graph,
                &access,
                &owner("g-session"),
                &source,
                EvidenceSource::NativeApi,
                "g-native-final",
                "064-source",
                Coverage::complete(),
            )?;
            let end_record = accept_revision_observation(
                &mut graph,
                &access,
                &owner("g-session"),
                &end,
                EvidenceSource::NativeApi,
                "g-native-final",
                "064-end",
                Coverage::complete(),
            )?;
            let receipt = execution_receipt(
                graph.project_id().clone(),
                owner("g-session"),
                std::slice::from_ref(&source_record),
                &end_record,
                64,
            );
            let receipt_id = receipt.id.clone();
            graph.accept_receipt(&access, admit_receipt(receipt)?)?;
            let scoped = subset(&graph, "g-session", BTreeSet::from([end.clone()]), false)?;
            let view = graph.provenance(&scoped, &end, 256)?;
            let wire = serde_json::to_string(&view)?;
            json!({"unknown_frontier":view.unknown_frontier,"hidden_id_absent":!wire.contains(source.as_str())&&!wire.contains(receipt_id.as_str())})
        }
        "G-GRAPH-065" => {
            let (mut graph, access) = setup()?;
            let source = register(&mut graph, &access, "declared-source", None)?;
            let derived = register(&mut graph, &access, "declared-derived", None)?;
            graph.declare(
                &access,
                Edge {
                    from: Vertex::Asset(derived),
                    to: Vertex::Asset(source.clone()),
                    relation: Relation::DerivedFrom,
                    evidence: EdgeEvidence::Declared {
                        declaration: ReceiptId::new(),
                    },
                },
            )?;
            let view = graph.provenance(&access, &source, 256)?;
            json!({"known":view.known_derivatives.len(),"possible":view.possible_derivatives.len()})
        }
        "G-GRAPH-066" => {
            let (mut graph, access) = setup()?;
            let source = register(&mut graph, &access, "bounded-source", None)?;
            for label in ["first", "second"] {
                let derived = register(&mut graph, &access, label, None)?;
                graph.declare(
                    &access,
                    Edge {
                        from: Vertex::Asset(derived),
                        to: Vertex::Asset(source.clone()),
                        relation: Relation::DerivedFrom,
                        evidence: EdgeEvidence::Declared {
                            declaration: ReceiptId::new(),
                        },
                    },
                )?;
            }
            let view = graph.provenance(&access, &source, 1)?;
            json!({"returned":view.known_derivatives.len()+view.possible_derivatives.len(),"truncated":view.truncated})
        }
        "G-GRAPH-067" => {
            let (mut graph, access) = setup()?;
            let asset = register(&mut graph, &access, "partial-gc", None)?;
            graph.tombstone(&access, &asset)?;
            let scoped = subset(&graph, "g-session", BTreeSet::from([asset]), true)?;
            json!({"partial_preview_rejected":graph.garbage_preview(&scoped,16).is_err()})
        }
        "G-GRAPH-068" => {
            let (graph, access) = setup()?;
            json!({"zero_limit_rejected":graph.garbage_preview(&access,0).is_err(),"oversize_limit_rejected":graph.garbage_preview(&access,257).is_err()})
        }
        "G-GRAPH-069" => {
            let (mut graph, access) = setup()?;
            let asset = register(&mut graph, &access, "duplicate-gc", None)?;
            graph.tombstone(&access, &asset)?;
            let empty = graph.collect_garbage(&access, vec![]).is_err();
            let duplicate = graph
                .collect_garbage(&access, vec![asset.clone(), asset])
                .is_err();
            json!({"empty_rejected":empty,"duplicate_rejected":duplicate})
        }
        "G-GRAPH-070" => {
            let dir = private_dir("gc-user-file")?;
            let native_path = dir.join("source.bin");
            std::fs::write(&native_path, b"keep-me")?;
            let (mut graph, access) = setup()?;
            let asset = register(
                &mut graph,
                &access,
                "history-free",
                Some(file("workspace", "source.bin")),
            )?;
            graph.tombstone(&access, &asset)?;
            let preview = graph.garbage_preview(&access, 16)?;
            let previewed = preview.candidates.contains(&asset);
            graph.collect_garbage(&access, vec![asset.clone()])?;
            let collected = graph.inspect(&access, &asset).is_err();
            let preserved = std::fs::read(&native_path)? == b"keep-me";
            cleanup(&dir);
            json!({"previewed":previewed,"user_files_deleted":preview.user_files_deleted||!preserved,"collected":collected})
        }
        "G-GRAPH-071" => {
            let (mut graph, access) = setup()?;
            let asset = register(&mut graph, &access, "history-block", None)?;
            let _ = accept_revision_observation(
                &mut graph,
                &access,
                &owner("g-session"),
                &asset,
                EvidenceSource::NativeApi,
                "g-native-final",
                "071",
                Coverage::complete(),
            )?;
            graph.tombstone(&access, &asset)?;
            let blocked = !graph
                .garbage_preview(&access, 16)?
                .candidates
                .contains(&asset)
                && graph.collect_garbage(&access, vec![asset]).is_err();
            json!({"revision_history_blocks_collection":blocked})
        }
        "G-GRAPH-072" => {
            let (mut graph, access) = setup()?;
            let asset = register(&mut graph, &access, "intent-block", None)?;
            graph.tombstone(&access, &asset)?;
            let intent = external_intent(
                &graph,
                owner("g-session"),
                vec![asset.clone()],
                "g-request-072",
                ExternalIntentId::new(),
            );
            graph.prepare_external_intent(&access, intent)?;
            let blocked = !graph
                .garbage_preview(&access, 16)?
                .candidates
                .contains(&asset)
                && graph.collect_garbage(&access, vec![asset]).is_err();
            json!({"intent_reference_blocks_collection":blocked})
        }
        "G-GRAPH-073" => {
            let (mut graph, access) = setup()?;
            let referenced = register(&mut graph, &access, "edge-block", None)?;
            let referrer = register(&mut graph, &access, "referrer", None)?;
            graph.declare(
                &access,
                Edge {
                    from: Vertex::Asset(referrer),
                    to: Vertex::Asset(referenced.clone()),
                    relation: Relation::References,
                    evidence: EdgeEvidence::Declared {
                        declaration: ReceiptId::new(),
                    },
                },
            )?;
            graph.tombstone(&access, &referenced)?;
            let blocked = !graph
                .garbage_preview(&access, 16)?
                .candidates
                .contains(&referenced)
                && graph.collect_garbage(&access, vec![referenced]).is_err();
            json!({"edge_reference_blocks_collection":blocked})
        }
        "G-GRAPH-074" => {
            let project = ProjectId::new();
            let principal = owner("g-store").principal;
            let access = store_access(&project, "g-store")?;
            let dir = private_dir("gc-reopen")?;
            let id;
            {
                let mut store = GraphStore::open(&dir, project.clone(), principal.clone(), false)?;
                id = store.transact(&access, |graph| {
                    register_graph(graph, &access, "durable-garbage", None)
                })?;
                store.transact(&access, |graph| graph.tombstone(&access, &id))?;
                store.transact(&access, |graph| {
                    graph.collect_garbage(&access, vec![id.clone()])
                })?;
            }
            let reopened = GraphStore::open(&dir, project, principal, false)?;
            let absent = reopened.graph()?.inspect(&access, &id).is_err();
            let journal_preserved = reopened.recovery_report().journal_records >= 3;
            drop(reopened);
            cleanup(&dir);
            json!({"collected_absent_after_reopen":absent,"journal_preserved":journal_preserved})
        }
        _ => {
            eprintln!("unregistered graph selector");
            std::process::exit(2)
        }
    })
}
fn cases() -> Vec<String> {
    (1..=74).map(|i| format!("G-GRAPH-{i:03}")).collect()
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 1 || std::env::var("G_LAB_TARGET_SHA").as_deref() != Ok(SOURCE) {
        std::process::exit(2)
    }
    if args[0] == "--list" {
        println!(
            "{}",
            json!({"schema_version":1,"source_sha":SOURCE,"cases":cases()})
        );
        return;
    }
    if !cases().contains(&args[0]) {
        std::process::exit(2)
    }
    match probe(&args[0]) {
        Ok(observed) => println!(
            "{}",
            json!({"schema_version":1,"source_sha":SOURCE,"case_id":args[0],"observed":observed})
        ),
        Err(error) => {
            eprintln!("graph probe contract/setup error: {error:?}");
            std::process::exit(1)
        }
    }
}
