//! Repository and standalone integrity of compiled first-party resources.
#[allow(dead_code)]
#[path = "../build.rs"]
mod bundle;
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};
fn fake_tree(repository: bool) -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("crates/driver-host");
    fs::create_dir_all(root.join("src/windows_blender52_runtime")).unwrap();
    for (name, canonical, _) in bundle::SOURCES {
        fs::write(
            root.join("src/windows_blender52_runtime").join(name),
            b"bounded fixture",
        )
        .unwrap();
        if repository {
            let source = root.join(canonical);
            fs::create_dir_all(source.parent().unwrap()).unwrap();
            fs::write(source, b"bounded fixture").unwrap();
        }
    }
    (temp, root)
}
#[test]
fn windows_runtime_bundle_matches_and_detects_drift_and_missing_source() {
    let (_temp, root) = fake_tree(true);
    bundle::verify_bundle(&root).unwrap();
    let target = root
        .join("src/windows_blender52_runtime")
        .join(bundle::SOURCES[0].0);
    fs::write(&target, b"changed").unwrap();
    assert!(bundle::verify_bundle(&root).unwrap_err().contains("drift"));
    fs::write(target, b"bounded fixture").unwrap();
    fs::remove_file(root.join(bundle::SOURCES[0].1)).unwrap();
    assert!(
        bundle::verify_bundle(&root)
            .unwrap_err()
            .contains("Missing")
    );
}
#[test]
fn windows_runtime_bundle_standalone_requires_all_packaged_resources() {
    let (_temp, root) = fake_tree(false);
    bundle::verify_bundle(&root).unwrap();
    fs::remove_file(
        root.join("src/windows_blender52_runtime")
            .join(bundle::SOURCES[0].0),
    )
    .unwrap();
    assert!(bundle::verify_bundle(&root).is_err());
}
#[test]
fn windows_runtime_bundle_manifest_matches_physical_bytes_and_targets() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    bundle::verify_bundle(&root).unwrap();
    let directory = root.join("src/windows_blender52_runtime");
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(directory.join("manifest.json")).unwrap()).unwrap();
    let rows = manifest["files"].as_array().unwrap();
    assert_eq!(rows.len(), bundle::SOURCES.len());
    for ((name, canonical, target), row) in bundle::SOURCES.iter().zip(rows) {
        assert_eq!(row["file"], *name);
        assert_eq!(row["canonical"], *canonical);
        assert_eq!(row["private_target"], *target);
        let bytes = fs::read(directory.join(name)).unwrap();
        assert_eq!(row["bytes"], bytes.len());
        assert_eq!(row["sha256"], hex::encode(Sha256::digest(&bytes)));
    }
    let local = &manifest["host_local_bootstrap"];
    let bytes = fs::read(root.join(local["file"].as_str().unwrap())).unwrap();
    assert_eq!(local["sha256"], hex::encode(Sha256::digest(&bytes)));
    assert_eq!(
        manifest["empty_package"]["sha256"],
        hex::encode(Sha256::digest(b""))
    );
}
