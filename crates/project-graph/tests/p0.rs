use composition::{Digest, strict_decode};
use proptest::prelude::*;
use semwright_project_graph::*;
fn fp(bytes: &[u8]) -> Fingerprint {
    Fingerprint {
        bytes: Some(Digest::of_bytes(bytes)),
        projection: None,
    }
}
#[test]
fn identity_is_neither_content_path_nor_other_namespace() {
    let a = LogicalAssetId::new();
    let b = LogicalAssetId::new();
    assert_ne!(a, b);
    assert!(LogicalAssetId::parse(ProjectId::new().as_str().to_owned()).is_err());
    assert!(LogicalAssetId::parse(Digest::of_bytes(b"same bytes").as_str().to_owned()).is_err());
    assert!(LogicalAssetId::parse("/projects/asset.glb".into()).is_err());
}
#[test]
fn strict_identity_wire_roundtrips_and_rejects_unknown_namespaces() {
    let p = ProjectId::new();
    let bytes = composition::canonical_bytes(&p).unwrap();
    assert_eq!(strict_decode::<ProjectId>(&bytes).unwrap(), p);
    assert!(strict_decode::<ProjectId>(b"\"prj_0000000000000000000000000000000A\"").is_err());
    assert!(strict_decode::<LogicalAssetId>(&bytes).is_err());
}
#[test]
fn fingerprint_rejects_duplicate_keys_and_unknown_fields() {
    assert!(
        strict_decode::<Fingerprint>(br#"{"bytes":null,"bytes":null,"projection":null}"#).is_err()
    );
    assert!(
        strict_decode::<Fingerprint>(br#"{"bytes":null,"projection":null,"verified":true}"#)
            .is_err()
    );
}
#[test]
fn denied_offline_and_ambiguous_are_not_missing() {
    for outcome in [
        ProbeOutcome::Denied,
        ProbeOutcome::Offline,
        ProbeOutcome::Ambiguous,
        ProbeOutcome::Failed,
    ] {
        assert_eq!(outcome.existence(), Existence::Unknown);
    }
    assert_eq!(
        ProbeOutcome::ConclusiveNotFound.existence(),
        Existence::Missing
    );
}
#[test]
fn stale_dependency_does_not_require_output_byte_change() {
    let expected = fp(b"source at export");
    let now = fp(b"source edited");
    let output = fp(b"unchanged derivative");
    assert_eq!(
        dependency_freshness([(&expected, &now, Equivalence::ExactBytes)], true),
        Freshness::Stale
    );
    assert_eq!(
        output.equivalent(&output, Equivalence::ExactBytes),
        Some(true)
    );
}
#[test]
fn exact_revert_can_be_reconsidered_but_mtime_is_not_evidence() {
    let expected = fp(b"source");
    assert_eq!(
        dependency_freshness([(&expected, &fp(b"source"), Equivalence::ExactBytes)], true),
        Freshness::Current
    );
    assert_eq!(
        dependency_freshness(
            [(&expected, &Fingerprint::default(), Equivalence::ExactBytes)],
            true
        ),
        Freshness::Unknown
    );
}
#[test]
fn semantic_method_version_is_part_of_equivalence() {
    let p = ProjectionDigest {
        digest: Digest::of_bytes(b"projection"),
        method: "mesh-topology".into(),
        method_version: 1,
    };
    let a = Fingerprint {
        bytes: Some(Digest::of_bytes(b"encoding 1")),
        projection: Some(p.clone()),
    };
    let mut b = Fingerprint {
        bytes: Some(Digest::of_bytes(b"encoding 2")),
        projection: Some(p),
    };
    assert_eq!(a.equivalent(&b, Equivalence::Projection), Some(true));
    assert_eq!(a.equivalent(&b, Equivalence::ExactBytes), Some(false));
    b.projection.as_mut().unwrap().method_version = 2;
    assert_eq!(a.equivalent(&b, Equivalence::Projection), None);
}
#[test]
fn partial_dependencies_prevent_cache_safe() {
    let mut k = Knowledge::unknown();
    k.existence = Existence::Present;
    k.freshness = Freshness::Current;
    k.divergence = Divergence::Clean;
    k.verification = composition::Verdict::Pass;
    k.requires_reconcile = false;
    assert!(!k.cache_safe());
    k.coverage = Coverage::complete();
    assert!(k.cache_safe());
    k.requires_reconcile = true;
    assert!(!k.cache_safe());
}
#[test]
fn required_unknown_and_known_stale_are_independent() {
    assert_eq!(
        dependency_freshness([(&fp(b"old"), &fp(b"new"), Equivalence::ExactBytes)], false),
        Freshness::Stale
    );
    assert_eq!(
        dependency_freshness(
            [(&fp(b"same"), &fp(b"same"), Equivalence::ExactBytes)],
            false
        ),
        Freshness::Unknown
    );
    let coverage = Coverage {
        complete: true,
        unknown_frontier: [DependencyClass::Font].into(),
    };
    assert!(coverage.validate().is_err());
}
#[test]
fn portable_locators_reject_traversal_devices_and_absolute_paths() {
    for path in [
        "/home/private",
        "../secret",
        "a/../secret",
        "a//b",
        "a/./b",
        "C:\\secret",
        "CON.txt",
        "a/x:stream",
        "file.",
        "a\\b",
        "NUL",
        "a/COM4.json",
    ] {
        assert!(
            DurableLocator::ScopedFile {
                root: "project".into(),
                relative_path: path.into()
            }
            .validate()
            .is_err(),
            "accepted {path}"
        );
    }
    assert!(
        DurableLocator::ScopedFile {
            root: "project".into(),
            relative_path: "assets/character.glb".into()
        }
        .validate()
        .is_ok()
    );
}
#[test]
fn replacing_locator_never_changes_or_discovers_logical_identity() {
    let a = Asset {
        id: LogicalAssetId::new(),
        resource_type: "model".into(),
        label: "Character".into(),
        locator: None,
    };
    let mut renamed = a.clone();
    renamed.label = "Renamed".into();
    assert_eq!(renamed.id, a.id);
    let replacement = Asset {
        id: LogicalAssetId::new(),
        ..a.clone()
    };
    assert_ne!(replacement.id, a.id);
}
proptest! {
    #[test]
    fn digest_reflexivity_does_not_imply_logical_identity(bytes in prop::collection::vec(any::<u8>(), 0..4096)) {
        let a = fp(&bytes); prop_assert_eq!(a.equivalent(&a, Equivalence::ExactBytes), Some(true)); prop_assert_ne!(LogicalAssetId::new(), LogicalAssetId::new());
    }
    #[test]
    fn opaque_ids_strictly_roundtrip(_seed in any::<u64>()) { let id = AssetRevision::new(); prop_assert_eq!(AssetRevision::parse(id.as_str().to_string()).unwrap(), id); }
}
