#![no_main]
use libfuzzer_sys::fuzz_target;
use semwright_project_graph::*;
fuzz_target!(|data: &[u8]| {
    if data.len() > 16 {
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let directory = std::fs::canonicalize(temp.path()).unwrap().join("graph");
    let project = ProjectId::new();
    let principal = composition::PrincipalBinding::Named("store-fuzz".into());
    let owner = composition::Owner {
        session: "fuzz".into(),
        principal: principal.clone(),
    };
    let access = ProjectAccess::authorized(
        owner,
        project.clone(),
        None,
        true,
        composition::Digest::of_bytes(b"grants"),
    )
    .unwrap();
    let mut store =
        GraphStore::open(&directory, project.clone(), principal.clone(), false).unwrap();
    let mut assets = Vec::new();
    for (index, byte) in data.iter().enumerate() {
        let asset = Asset {
            id: LogicalAssetId::new(),
            resource_type: "synthetic".into(),
            label: format!("asset-{index}-{byte}"),
            locator: None,
        };
        let sequence = store.graph().unwrap().snapshot_revision();
        let result = store.transact(&access, |g| {
            g.register(&access, asset.clone())?;
            if byte % 2 == 0 {
                g.register(&access, asset.clone())?;
            }
            Ok(())
        });
        if byte % 2 == 0 {
            assert!(result.is_err());
            assert_eq!(store.graph().unwrap().snapshot_revision(), sequence);
        } else {
            result.unwrap();
            assets.push(asset);
        }
    }
    let sequence = store.graph().unwrap().snapshot_revision();
    drop(store);
    let mut reopened = GraphStore::open(&directory, project, principal.clone(), false).unwrap();
    assert_eq!(reopened.graph().unwrap().snapshot_revision(), sequence);
    for a in &assets {
        let state = reopened.graph().unwrap().inspect(&access, &a.id).unwrap();
        assert_eq!(state.asset.label, a.label);
        assert_eq!(state.knowledge.label(), "UNKNOWN");
    }
    if data.first().is_some_and(|b| b % 4 == 1) {
        let backup = reopened.backup(&access).unwrap();
        let restored =
            GraphStore::restore(&directory, &temp.path().join("restore"), principal, &backup)
                .unwrap();
        assert_eq!(restored.graph().unwrap().snapshot_revision(), sequence);
    }
});
