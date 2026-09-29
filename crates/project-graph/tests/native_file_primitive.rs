#![cfg(all(feature = "store", target_os = "linux"))]
//! Real disposable filesystem primitive tests. NOT Broker or Blender/Godot/AV E2E.
use semwright_platform_services::Root;
use semwright_project_graph::composition::Digest;
use std::path::Path;
#[test]
fn instance_identity_survives_in_place_edit_but_not_same_path_replacement() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("source.bin");
    std::fs::write(&path, b"first").unwrap();
    let root = Root::open(temp.path(), true, false).unwrap();
    let first = root.observe_file(Path::new("source.bin"), 1024).unwrap();
    std::fs::write(&path, b"edited in place").unwrap();
    let edited = root.observe_file(Path::new("source.bin"), 1024).unwrap();
    assert_eq!(first.instance_identity, edited.instance_identity);
    assert_ne!(
        Digest::of_bytes(&first.bytes),
        Digest::of_bytes(&edited.bytes)
    );
    std::fs::write(temp.path().join("replacement.bin"), &edited.bytes).unwrap();
    std::fs::rename(temp.path().join("replacement.bin"), &path).unwrap();
    let replaced = root.observe_file(Path::new("source.bin"), 1024).unwrap();
    assert_eq!(
        Digest::of_bytes(&edited.bytes),
        Digest::of_bytes(&replaced.bytes)
    );
    assert_ne!(edited.instance_identity, replaced.instance_identity);
    assert_eq!(replaced.method, "openat2-fd-btime-read-revalidate");
    assert_eq!(replaced.method_version, 1);
}
#[test]
fn native_missing_budget_and_read_denial_have_distinct_protocol_errors() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("source.bin"), b"12345").unwrap();
    let root = Root::open(temp.path(), true, false).unwrap();
    let missing = root
        .observe_file(Path::new("absent.bin"), 1024)
        .unwrap_err();
    let budget = root.observe_file(Path::new("source.bin"), 2).unwrap_err();
    let denied = Root::open(temp.path(), false, false)
        .unwrap()
        .observe_file(Path::new("source.bin"), 1024)
        .unwrap_err();
    assert_eq!(serde_json::to_value(missing.code).unwrap(), "NotFound");
    assert_eq!(
        serde_json::to_value(budget.code).unwrap(),
        "ResourceExhausted"
    );
    assert_eq!(serde_json::to_value(denied.code).unwrap(), "PolicyDenied");
}
