//! Independent G Project Graph adversarial contract/store probe.
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
fn observed(id: &LogicalAssetId, source: EvidenceSource, tag: &str) -> RevisionRecord {
    RevisionRecord {
        pin: RevisionPin {
            asset: id.clone(),
            revision: AssetRevision::new(),
            fingerprint: Fingerprint {
                bytes: Some(d(tag)),
                projection: None,
            },
            equivalence: Equivalence::ExactBytes,
        },
        observed_unix_ms: 1,
        binding_generation: 1,
        observation: ObservationRef {
            id: format!("g-observation-{tag}"),
            base: base(),
            source,
            method: "g-native-readback".into(),
            method_version: 1,
            scope: vec![address()],
            artifact: None,
            exhaustive: true,
        },
        coverage: Coverage::complete(),
    }
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
            graph.observe(&access, observed(&id, EvidenceSource::Fixture, "same"))?;
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
            graph.observe(&access, observed(&id, EvidenceSource::NativeApi, "v1"))?;
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
            graph.observe(&access, observed(&a, EvidenceSource::NativeApi, "v1"))?;
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
        _ => {
            eprintln!("unregistered graph selector");
            std::process::exit(2)
        }
    })
}
fn cases() -> Vec<String> {
    (1..=52).map(|i| format!("G-GRAPH-{i:03}")).collect()
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
