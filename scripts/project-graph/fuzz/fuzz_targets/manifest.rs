#![no_main]
use libfuzzer_sys::fuzz_target;
use semwright_project_graph::*;
fuzz_target!(|data: &[u8]| {
    if data.len() > 65_536 {
        return;
    }
    if let Ok(manifest) = PortableManifest::decode(data) {
        let canonical = composition::canonical_bytes(&manifest).unwrap();
        let roundtrip = PortableManifest::decode(&canonical).unwrap();
        assert_eq!(
            composition::canonical_digest(&manifest).unwrap(),
            composition::canonical_digest(&roundtrip).unwrap()
        );
        let project = ProjectId::new();
        let principal = composition::PrincipalBinding::Named("manifest-fuzz-owner".into());
        let owner = composition::Owner {
            session: "manifest-fuzz-session".into(),
            principal: principal.clone(),
        };
        let access = ProjectAccess::authorized(
            owner,
            project.clone(),
            None,
            true,
            composition::Digest::of_bytes(b"scoped-fuzz-grants"),
        )
        .unwrap();
        let mut graph = ProjectGraph::new(project, principal).unwrap();
        if let Ok(mapping) = graph.import_manifest(&access, &manifest) {
            for id in mapping.values() {
                let view = graph.inspect(&access, id).unwrap();
                assert!(view.asset.locator.is_none());
                assert!(!view.knowledge.cache_safe());
                assert_eq!(view.knowledge.label(), "UNKNOWN");
            }
        } else {
            assert_eq!(graph.snapshot_revision(), 0);
        }
    }
});
