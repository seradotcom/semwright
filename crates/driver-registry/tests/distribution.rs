#![cfg(unix)]

use semwright_driver_registry::{
    CompanionInput, Index, IndexEntry, InstallRoots, PackageMetadata, create_package,
    create_package_with_companions, inspect_package, install_from_index, package_digest,
    remove_installed,
};
use semwright_driver_sdk::{
    ApplicationMatch, DriverInterfaces, DriverResources, Manifest, Transport,
};
use semwright_types::{ErrorCode, unique_id};
use sha2::{Digest, Sha256};
use std::{
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
};

fn fake_manifest(root: &Path, version: &str, supported: Vec<String>) -> Manifest {
    let executable = root.join("driver");
    let bytes = b"ELFnot-runnable-by-design";
    std::fs::write(&executable, bytes).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    Manifest {
        manifest_version: 1,
        protocol: 1,
        id: "fixture".into(),
        version: version.into(),
        publisher: "semwright-tests".into(),
        executable,
        sha256: format!("{:x}", Sha256::digest(bytes)),
        application: ApplicationMatch {
            desktop_id: Some("org.example.Fixture".into()),
            process_names: vec!["fixture".into()],
            supported_versions: supported,
        },
        transport: Transport::StdioV1,
        mounts: vec![],
        system_config: vec![],
        secrets: vec![],
        tools: vec![],
        network: false,
        loopback_port: None,
        resources: DriverResources::default(),
        request_timeout_ms: 5_000,
        interfaces: DriverInterfaces::default(),
    }
}

fn make_index(dir: &Path, versions: &[(&str, &str, Vec<String>)]) -> (PathBuf, Vec<IndexEntry>) {
    let mut entries = vec![];
    for (version, requirement, application_versions) in versions {
        let source = tempfile::tempdir_in(dir).unwrap();
        let manifest = fake_manifest(source.path(), version, application_versions.clone());
        let package_name = format!("fixture-{version}.swdp");
        let package = dir.join(&package_name);
        create_package(&manifest, requirement, &package).unwrap();
        entries.push(IndexEntry {
            id: "fixture".into(),
            version: (*version).into(),
            publisher: "semwright-tests".into(),
            package: PathBuf::from(package_name),
            package_sha256: package_digest(&package).unwrap(),
            package_bytes: std::fs::metadata(package).unwrap().len(),
            semwright: (*requirement).into(),
            application_versions: application_versions.clone(),
        });
    }
    let index = Index {
        index_version: 1,
        drivers: entries.clone(),
    };
    index.validate().unwrap();
    let path = dir.join("index.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&index).unwrap()).unwrap();
    (path, entries)
}

#[test]
fn package_roundtrip_and_install_do_not_execute_payload() {
    let d = tempfile::tempdir().unwrap();
    let repo = d.path().join("repo");
    let data = d.path().join("data");
    let config = d.path().join("config");
    std::fs::create_dir(&repo).unwrap();
    let requirement = format!("={}", env!("CARGO_PKG_VERSION"));
    let (index_path, entries) = make_index(&repo, &[("1.2.3", &requirement, vec![])]);

    let package = repo.join(&entries[0].package);
    let (metadata, executable, digest) = inspect_package(&package).unwrap();
    assert_eq!(metadata.manifest.id, "fixture");
    assert_eq!(metadata.package_version, 2);
    assert!(metadata.companions.is_empty());
    assert_eq!(digest, entries[0].package_sha256);
    assert_eq!(executable, b"ELFnot-runnable-by-design");

    let roots = InstallRoots { data, config };
    let receipt = install_from_index(&index_path, &entries[0], None, &roots).unwrap();
    assert_eq!(
        std::fs::read(&receipt.executable_path).unwrap(),
        b"ELFnot-runnable-by-design"
    );
    assert!(receipt.manifest_path.is_file());
    assert_eq!(
        std::fs::metadata(&receipt.executable_path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    remove_installed("fixture", "1.2.3", &roots).unwrap();
    assert!(!receipt.executable_path.exists());
    assert!(!receipt.manifest_path.exists());
}

#[test]
fn index_rejects_traversal_duplicate_and_noncanonical_digest() {
    let base = IndexEntry {
        id: "fixture".into(),
        version: "1.0.0".into(),
        publisher: "publisher".into(),
        package: "fixture.swdp".into(),
        package_sha256: "a".repeat(64),
        package_bytes: 100,
        semwright: format!("={}", env!("CARGO_PKG_VERSION")),
        application_versions: vec![],
    };
    let mut traversal = base.clone();
    traversal.package = "../escape.swdp".into();
    assert!(traversal.validate().is_err());
    let mut uppercase = base.clone();
    uppercase.package_sha256 = "A".repeat(64);
    assert!(uppercase.validate().is_err());
    assert!(
        Index {
            index_version: 1,
            drivers: vec![base.clone(), base],
        }
        .validate()
        .is_err()
    );
}

#[test]
fn resolver_prefers_highest_compatible_version_and_checks_application_version() {
    let d = tempfile::tempdir().unwrap();
    let requirement = format!("={}", env!("CARGO_PKG_VERSION"));
    let (_path, entries) = make_index(
        d.path(),
        &[
            ("1.0.0", &requirement, vec!["7.6".into()]),
            ("2.0.0", &requirement, vec!["24.2".into()]),
        ],
    );
    let index = Index {
        index_version: 1,
        drivers: entries,
    };
    assert_eq!(
        index
            .resolve("fixture", None, Some("24.2"), env!("CARGO_PKG_VERSION"))
            .unwrap()
            .version,
        "2.0.0"
    );
    assert_eq!(
        index
            .resolve("fixture", None, Some("7.6"), env!("CARGO_PKG_VERSION"))
            .unwrap()
            .version,
        "1.0.0"
    );
    assert!(
        index
            .resolve(
                "fixture",
                None,
                Some("unsupported"),
                env!("CARGO_PKG_VERSION")
            )
            .is_err()
    );
}

#[test]
fn install_rejects_tampering_index_escape_and_missing_application_version() {
    let d = tempfile::tempdir().unwrap();
    let repo = d.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let requirement = format!("={}", env!("CARGO_PKG_VERSION"));
    let (index_path, mut entries) =
        make_index(&repo, &[("1.0.0", &requirement, vec!["24.2".into()])]);
    let roots = InstallRoots {
        data: d.path().join("data"),
        config: d.path().join("config"),
    };

    assert_eq!(
        install_from_index(&index_path, &entries[0], None, &roots)
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );

    entries[0].package_sha256 = "b".repeat(64);
    assert_eq!(
        install_from_index(&index_path, &entries[0], Some("24.2"), &roots)
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );

    let outside = d.path().join(format!("outside-{}.swdp", unique_id()));
    let real = repo.join("fixture-1.0.0.swdp");
    std::fs::rename(&real, &outside).unwrap();
    symlink(&outside, &real).unwrap();
    let mut entry = entries[0].clone();
    entry.package_sha256 = package_digest(&outside).unwrap();
    assert!(install_from_index(&index_path, &entry, Some("24.2"), &roots).is_err());
}

#[test]
fn malformed_package_lengths_and_metadata_mismatch_fail_closed() {
    let d = tempfile::tempdir().unwrap();
    let bad = d.path().join("bad.swdp");
    let mut malformed = b"SEMWRIGHT-DRIVER-PACKAGE-V1\n".to_vec();
    malformed.extend_from_slice(&u32::MAX.to_be_bytes());
    malformed.extend_from_slice(b"{}");
    std::fs::write(&bad, malformed).unwrap();
    assert!(inspect_package(&bad).is_err());

    let repo = d.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let requirement = format!("={}", env!("CARGO_PKG_VERSION"));
    let (index_path, mut entries) = make_index(&repo, &[("1.0.0", &requirement, vec![])]);
    entries[0].publisher = "different".into();
    let roots = InstallRoots {
        data: d.path().join("data"),
        config: d.path().join("config"),
    };
    assert_eq!(
        install_from_index(&index_path, &entries[0], None, &roots)
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
}

#[test]
fn remove_rejects_version_path_escape_before_touching_store() {
    let d = tempfile::tempdir().unwrap();
    let roots = InstallRoots {
        data: d.path().join("data"),
        config: d.path().join("config"),
    };
    let error = remove_installed("fixture", "../../escape", &roots).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert!(!d.path().join("escape").exists());
}

#[test]
fn installing_newer_version_switches_stable_manifest_without_deleting_old_version() {
    let d = tempfile::tempdir().unwrap();
    let repo = d.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let requirement = format!("={}", env!("CARGO_PKG_VERSION"));
    let (index_path, entries) = make_index(
        &repo,
        &[
            ("1.0.0", &requirement, vec![]),
            ("2.0.0", &requirement, vec![]),
        ],
    );
    let roots = InstallRoots {
        data: d.path().join("data"),
        config: d.path().join("config"),
    };
    let first = entries.iter().find(|e| e.version == "1.0.0").unwrap();
    let second = entries.iter().find(|e| e.version == "2.0.0").unwrap();
    install_from_index(&index_path, first, None, &roots).unwrap();
    install_from_index(&index_path, second, None, &roots).unwrap();

    assert!(roots.data.join("fixture/1.0.0/driver").is_file());
    assert!(roots.data.join("fixture/2.0.0/driver").is_file());
    let active: semwright_driver_sdk::Manifest =
        serde_json::from_slice(&std::fs::read(roots.config.join("fixture.json")).unwrap()).unwrap();
    assert_eq!(active.version, "2.0.0");
    assert_eq!(active.executable, roots.data.join("fixture/2.0.0/driver"));
}

#[test]
fn package_v2_companions_install_privately_without_activation() {
    let d = tempfile::tempdir().unwrap();
    let repo = d.path().join("repo");
    let source = d.path().join("source");
    std::fs::create_dir(&repo).unwrap();
    std::fs::create_dir(&source).unwrap();

    let manifest = fake_manifest(&source, "3.0.0", vec![]);
    let plugin_cfg = source.join("plugin.cfg");
    let plugin_gd = source.join("plugin.gd");
    std::fs::write(&plugin_cfg, b"[plugin]\nname=Semwright\n").unwrap();
    std::fs::write(&plugin_gd, b"@tool\nextends EditorPlugin\n").unwrap();

    let package = repo.join("fixture-3.0.0.swdp");
    let requirement = format!("={}", env!("CARGO_PKG_VERSION"));
    create_package_with_companions(
        &manifest,
        &requirement,
        &[
            CompanionInput {
                destination: "addons/semwright/plugin.cfg".into(),
                source: plugin_cfg.clone(),
            },
            CompanionInput {
                destination: "addons/semwright/plugin.gd".into(),
                source: plugin_gd.clone(),
            },
        ],
        &package,
    )
    .unwrap();

    let (metadata, _, digest) = inspect_package(&package).unwrap();
    assert_eq!(metadata.package_version, 2);
    assert_eq!(metadata.companions.len(), 2);
    assert_eq!(
        metadata.companions[0].path,
        PathBuf::from("addons/semwright/plugin.cfg")
    );

    let entry = IndexEntry {
        id: "fixture".into(),
        version: "3.0.0".into(),
        publisher: "semwright-tests".into(),
        package: "fixture-3.0.0.swdp".into(),
        package_sha256: digest,
        package_bytes: std::fs::metadata(&package).unwrap().len(),
        semwright: requirement,
        application_versions: vec![],
    };
    let index = Index {
        index_version: 1,
        drivers: vec![entry.clone()],
    };
    let index_path = repo.join("index.json");
    std::fs::write(&index_path, serde_json::to_vec_pretty(&index).unwrap()).unwrap();

    let roots = InstallRoots {
        data: d.path().join("data"),
        config: d.path().join("config"),
    };
    let receipt = install_from_index(&index_path, &entry, None, &roots).unwrap();
    assert_eq!(receipt.receipt_version, 2);
    assert_eq!(receipt.companion_files.len(), 2);

    let companion_root = roots.data.join("fixture/3.0.0/companions");
    let installed_cfg = companion_root.join("addons/semwright/plugin.cfg");
    let installed_gd = companion_root.join("addons/semwright/plugin.gd");
    assert_eq!(
        std::fs::read(&installed_cfg).unwrap(),
        std::fs::read(plugin_cfg).unwrap()
    );
    assert_eq!(
        std::fs::read(&installed_gd).unwrap(),
        std::fs::read(plugin_gd).unwrap()
    );
    assert_eq!(
        std::fs::metadata(&installed_cfg)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert!(!roots.data.join("fixture/3.0.0/addons").exists());

    remove_installed("fixture", "3.0.0", &roots).unwrap();
    assert!(!companion_root.exists());
}

#[test]
fn package_v2_rejects_companion_traversal_duplicates_and_tampering() {
    let d = tempfile::tempdir().unwrap();
    let manifest = fake_manifest(d.path(), "4.0.0", vec![]);
    let source = d.path().join("plugin.gd");
    std::fs::write(&source, b"extends EditorPlugin\n").unwrap();
    let requirement = format!("={}", env!("CARGO_PKG_VERSION"));

    let traversal = d.path().join("traversal.swdp");
    assert!(
        create_package_with_companions(
            &manifest,
            &requirement,
            &[CompanionInput {
                destination: "../escape.gd".into(),
                source: source.clone(),
            }],
            &traversal,
        )
        .is_err()
    );

    let duplicate = d.path().join("duplicate.swdp");
    assert!(
        create_package_with_companions(
            &manifest,
            &requirement,
            &[
                CompanionInput {
                    destination: "addons/semwright/plugin.gd".into(),
                    source: source.clone(),
                },
                CompanionInput {
                    destination: "addons/semwright/plugin.gd".into(),
                    source: source.clone(),
                },
            ],
            &duplicate,
        )
        .is_err()
    );

    let valid = d.path().join("valid.swdp");
    create_package_with_companions(
        &manifest,
        &requirement,
        &[CompanionInput {
            destination: "addons/semwright/plugin.gd".into(),
            source,
        }],
        &valid,
    )
    .unwrap();
    let mut bytes = std::fs::read(&valid).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0x01;
    let tampered = d.path().join("tampered.swdp");
    std::fs::write(&tampered, bytes).unwrap();
    assert_eq!(
        inspect_package(&tampered).unwrap_err().code,
        ErrorCode::PermissionDenied
    );
}

#[test]
fn package_v1_remains_readable_after_v2_upgrade() {
    let d = tempfile::tempdir().unwrap();
    let manifest = fake_manifest(d.path(), "5.0.0", vec![]);
    let executable = std::fs::read(&manifest.executable).unwrap();
    let mut portable = manifest.clone();
    portable.executable = "/package/driver".into();

    let metadata = PackageMetadata {
        package_version: 1,
        semwright: format!("={}", env!("CARGO_PKG_VERSION")),
        manifest: portable,
        executable_bytes: 0,
        companions: vec![],
    };
    let mut metadata = serde_json::to_value(&metadata).unwrap();
    metadata.as_object_mut().unwrap().remove("executable_bytes");
    metadata.as_object_mut().unwrap().remove("companions");
    let metadata = serde_json::to_vec(&metadata).unwrap();
    let mut package = b"SEMWRIGHT-DRIVER-PACKAGE-V1\n".to_vec();
    package.extend_from_slice(&(metadata.len() as u32).to_be_bytes());
    package.extend_from_slice(&metadata);
    package.extend_from_slice(&executable);
    let path = d.path().join("legacy.swdp");
    std::fs::write(&path, package).unwrap();

    let (metadata, recovered, _) = inspect_package(&path).unwrap();
    assert_eq!(metadata.package_version, 1);
    assert!(metadata.companions.is_empty());
    assert_eq!(recovered, executable);
}
