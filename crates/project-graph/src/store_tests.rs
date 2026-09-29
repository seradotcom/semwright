use super::*;
use composition::Owner;
fn fixture() -> (tempfile::TempDir, PathBuf, ProjectId, ProjectAccess) {
    let temp = tempfile::tempdir().unwrap();
    let path = std::fs::canonicalize(temp.path()).unwrap().join("graph");
    let project = ProjectId::new();
    let owner = Owner {
        session: "store-test".into(),
        principal: PrincipalBinding::Named("store-owner".into()),
    };
    let access = ProjectAccess::authorized(
        owner,
        project.clone(),
        None,
        true,
        Digest::of_bytes(b"grants"),
    )
    .unwrap();
    (temp, path, project, access)
}
fn item(label: &str) -> Asset {
    Asset {
        id: LogicalAssetId::new(),
        resource_type: "file".into(),
        label: label.into(),
        locator: Some(DurableLocator::ScopedFile {
            root: "workspace".into(),
            relative_path: "asset.bin".into(),
        }),
    }
}
fn open(path: &Path, project: &ProjectId, access: &ProjectAccess) -> GraphStore {
    GraphStore::open(path, project.clone(), access.owner.principal.clone(), false).unwrap()
}
#[test]
fn reopen_preserves_history_and_does_not_reuse_observation_epoch() {
    let (_tmp, path, project, access) = fixture();
    let mut store = open(&path, &project, &access);
    let a = item("source");
    store
        .transact(&access, |g| g.register(&access, a.clone()))
        .unwrap();
    let epoch = store.graph().unwrap().epoch.clone();
    drop(store);
    let reopened = open(&path, &project, &access);
    let view = reopened.graph().unwrap().inspect(&access, &a.id).unwrap();
    assert_eq!(view.asset.label, "source");
    assert_eq!(view.knowledge.label(), "UNKNOWN");
    assert_ne!(epoch, reopened.graph().unwrap().epoch);
    assert_eq!(reopened.recovery_report().journal_records, 1);
}
#[test]
fn every_transaction_fault_boundary_is_atomic_on_reopen() {
    for boundary in [
        Boundary::BeforeJournal,
        Boundary::AfterJournal,
        Boundary::AfterIndexes,
        Boundary::BeforeCommit,
        Boundary::AfterCommit,
    ] {
        let (_tmp, path, project, access) = fixture();
        let mut store = open(&path, &project, &access);
        let a = item("source");
        assert!(
            store
                .transact_inner(&access, |g| g.register(&access, a.clone()), Some(boundary))
                .is_err()
        );
        if boundary == Boundary::AfterCommit {
            assert!(store.graph().is_err());
        } else {
            assert_eq!(store.graph().unwrap().sequence, 0);
        }
        drop(store);
        let reopened = open(&path, &project, &access);
        assert_eq!(
            reopened.graph().unwrap().assets.len(),
            usize::from(boundary == Boundary::AfterCommit)
        );
    }
}
#[test]
fn second_writer_cannot_commit_against_a_stale_snapshot() {
    let (_tmp, path, project, access) = fixture();
    let mut first = open(&path, &project, &access);
    let mut stale = open(&path, &project, &access);
    first
        .transact(&access, |g| g.register(&access, item("first")))
        .unwrap();
    assert!(matches!(
        stale.transact(&access, |g| g.register(&access, item("stale"))),
        Err(GraphError::Conflict)
    ));
    assert_eq!(stale.graph().unwrap().sequence, 0);
}
#[test]
fn corrupt_indexes_require_explicit_rebuild_and_canonical_log_is_untouched() {
    let (_tmp, path, project, access) = fixture();
    let mut store = open(&path, &project, &access);
    let a = item("source");
    store
        .transact(&access, |g| g.register(&access, a.clone()))
        .unwrap();
    let journal: String = store
        .connection
        .query_row("SELECT digest FROM journal WHERE sequence=1", [], |r| {
            r.get(0)
        })
        .unwrap();
    store
        .connection
        .execute("UPDATE assets SET digest='corrupt'", [])
        .unwrap();
    drop(store);
    assert!(
        GraphStore::open(
            &path,
            project.clone(),
            access.owner.principal.clone(),
            false
        )
        .is_err()
    );
    let repaired = GraphStore::open(&path, project, access.owner.principal.clone(), true).unwrap();
    assert!(repaired.recovery_report().indexes_rebuilt);
    let retained: String = repaired
        .connection
        .query_row("SELECT digest FROM journal WHERE sequence=1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(journal, retained);
}
#[test]
fn corrupted_canonical_payload_is_preserved_and_not_repaired_as_an_index() {
    let (_tmp, path, project, access) = fixture();
    let mut store = open(&path, &project, &access);
    store
        .transact(&access, |g| g.register(&access, item("source")))
        .unwrap();
    store
        .connection
        .execute("UPDATE journal SET payload=x'7b7d'", [])
        .unwrap();
    drop(store);
    assert!(GraphStore::open(&path, project, access.owner.principal.clone(), true).is_err());
    let db = Connection::open(path.join("project.sqlite3")).unwrap();
    let payload: Vec<u8> = db
        .query_row("SELECT payload FROM journal", [], |r| r.get(0))
        .unwrap();
    assert_eq!(payload, b"{}");
}
#[test]
fn backup_verifies_hash_owner_version_and_preserves_live_history() {
    let (tmp, path, project, access) = fixture();
    let mut store = open(&path, &project, &access);
    let a = item("before");
    store
        .transact(&access, |g| g.register(&access, a.clone()))
        .unwrap();
    let backup = store.backup(&access).unwrap();
    store
        .transact(&access, |g| g.rename(&access, &a.id, "after".into()))
        .unwrap();
    let destination = std::fs::canonicalize(tmp.path()).unwrap().join("restore");
    let restored =
        GraphStore::restore(&path, &destination, access.owner.principal.clone(), &backup).unwrap();
    assert_eq!(
        restored
            .graph()
            .unwrap()
            .inspect(&access, &a.id)
            .unwrap()
            .asset
            .label,
        "before"
    );
    assert_eq!(
        store
            .graph()
            .unwrap()
            .inspect(&access, &a.id)
            .unwrap()
            .asset
            .label,
        "after"
    );
    assert!(
        GraphStore::restore(&path, &destination, access.owner.principal.clone(), &backup).is_err()
    );
    let mut bad = backup;
    bad.sha256 = Digest::of_bytes(b"bad");
    assert!(
        GraphStore::restore(
            &path,
            &tmp.path().join("bad"),
            access.owner.principal.clone(),
            &bad
        )
        .is_err()
    );
}
#[test]
fn other_owner_and_future_schema_are_not_silently_migrated() {
    let (_tmp, path, project, access) = fixture();
    let store = open(&path, &project, &access);
    assert!(
        GraphStore::open(
            &path,
            project.clone(),
            PrincipalBinding::Named("different".into()),
            false
        )
        .is_err()
    );
    store
        .connection
        .execute_batch("PRAGMA user_version=999;")
        .unwrap();
    drop(store);
    assert!(GraphStore::open(&path, project, access.owner.principal.clone(), true).is_err());
}

#[test]
fn partial_project_access_cannot_export_full_private_backup() {
    let (_tmp, path, project, access) = fixture();
    let mut store = open(&path, &project, &access);
    let a = item("visible");
    store
        .transact(&access, |g| g.register(&access, a.clone()))
        .unwrap();
    let subset = ProjectAccess::authorized(
        access.owner.clone(),
        project,
        Some([a.id].into()),
        true,
        access.grants.clone(),
    )
    .unwrap();
    assert!(matches!(store.backup(&subset), Err(GraphError::Denied)));
    assert!(!path.join("backup.sqlite3").exists());
}

#[test]
fn external_applying_intent_reopens_unknown_and_cannot_be_redispatched() {
    let (_tmp, path, project, access) = fixture();
    let mut store = open(&path, &project, &access);
    let output = item("external-output");
    store
        .transact(&access, |g| g.register(&access, output.clone()))
        .unwrap();
    let intent = ExternalIntent {
        version: SCHEMA_VERSION,
        id: ExternalIntentId::new(),
        project: project.clone(),
        owner: access.owner.clone(),
        request_id: "external-request".into(),
        operation: OperationIdentity {
            capability: "fixture.external".into(),
            descriptor: Digest::of_bytes(b"descriptor"),
            runtime: Digest::of_bytes(b"runtime"),
            plan: Digest::of_bytes(b"plan"),
            parameters: Digest::of_bytes(b"parameters"),
            recipe: None,
        },
        affected: vec![output.id.clone()],
        prepared_unix_ms: 1,
        observation_epoch: store.graph().unwrap().observation_epoch().into(),
        status: composition::ExecutionStatus::Prepared,
        receipt: None,
    };
    let intent_id = intent.id.clone();
    store
        .transact(&access, |g| g.prepare_external_intent(&access, intent))
        .unwrap();
    store
        .transact(&access, |g| {
            g.mark_external_intent_applying(&access, &intent_id)
        })
        .unwrap();
    drop(store);

    let mut reopened = open(&path, &project, &access);
    assert_eq!(
        reopened
            .graph()
            .unwrap()
            .external_intent(&access, &intent_id)
            .unwrap()
            .status,
        composition::ExecutionStatus::Unknown
    );
    assert!(
        reopened
            .transact(&access, |g| {
                g.mark_external_intent_applying(&access, &intent_id)
            })
            .is_err()
    );
    reopened
        .transact(&access, |g| {
            g.resolve_external_intent(
                &access,
                &intent_id,
                composition::ExecutionStatus::Unknown,
                None,
            )
        })
        .unwrap();
    drop(reopened);

    let restored = open(&path, &project, &access);
    assert_eq!(
        restored
            .graph()
            .unwrap()
            .external_intent(&access, &intent_id)
            .unwrap()
            .status,
        composition::ExecutionStatus::Unknown
    );
}
