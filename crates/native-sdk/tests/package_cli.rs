#![cfg(unix)]

use semwright_driver_registry::{Index, IndexEntry};
use semwright_driver_sdk::{
    ApplicationMatch, DriverInterfaces, DriverResources, Manifest, Transport,
};
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn output(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_semwright-native-package"))
        .args(args)
        .output()
        .unwrap()
}

fn run(args: &[&str]) -> serde_json::Value {
    let output = output(args);
    assert!(
        output.status.success(),
        "package CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn run_fail(args: &[&str]) -> Output {
    let output = output(args);
    assert!(
        !output.status.success(),
        "package CLI unexpectedly succeeded: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    output
}

fn manifest(root: &Path, version: &str, payload: &[u8]) -> Manifest {
    let executable = root.join("native-clean-driver");
    fs::write(&executable, payload).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    Manifest {
        manifest_version: 1,
        protocol: 1,
        id: "native-clean-driver".into(),
        version: version.into(),
        publisher: "semwright-native-sdk-tests".into(),
        executable,
        sha256: hex::encode(Sha256::digest(payload)),
        application: ApplicationMatch {
            desktop_id: Some("org.semwright.NativeClean".into()),
            process_names: vec!["native-clean".into()],
            supported_versions: vec![],
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

fn pack(
    repo: &Path,
    source: &Path,
    version: &str,
    payload: &[u8],
    requirement: &str,
) -> (PathBuf, String) {
    fs::create_dir(source).unwrap();
    let manifest = manifest(source, version, payload);
    let manifest_path = source.join("manifest.json");
    fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let package = repo.join(format!("native-clean-driver-{version}.swdp"));
    let packed = run(&[
        "pack",
        manifest_path.to_str().unwrap(),
        requirement,
        package.to_str().unwrap(),
    ]);
    let digest = packed["package_sha256"].as_str().unwrap().to_owned();
    assert_eq!(digest.len(), 64);
    (package, digest)
}

fn entry(version: &str, package: &Path, digest: &str, requirement: &str) -> IndexEntry {
    IndexEntry {
        id: "native-clean-driver".into(),
        version: version.into(),
        publisher: "semwright-native-sdk-tests".into(),
        package: PathBuf::from(package.file_name().unwrap()),
        package_sha256: digest.into(),
        package_bytes: fs::metadata(package).unwrap().len(),
        semwright: requirement.into(),
        application_versions: vec![],
    }
}

#[test]
fn package_cli_roundtrip_installs_updates_and_revokes_without_execution_or_grants() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    let source_v1 = temp.path().join("source-v1");
    let source_v2 = temp.path().join("source-v2");
    let data = temp.path().join("data");
    let config = temp.path().join("config");
    fs::create_dir(&repo).unwrap();

    let requirement = format!("={}", env!("CARGO_PKG_VERSION"));
    let payload_v1 = b"\x7fELFsemwright-native-package-v1-not-runnable";
    let payload_v2 = b"\x7fELFsemwright-native-package-v2-not-runnable";
    let (package_v1, sha_v1) = pack(&repo, &source_v1, "1.2.3", payload_v1, &requirement);
    let (package_v2, sha_v2) = pack(&repo, &source_v2, "2.0.0", payload_v2, &requirement);

    let inspected = run(&["inspect", package_v1.to_str().unwrap()]);
    assert_eq!(inspected["package_sha256"], sha_v1);
    assert_eq!(
        inspected["metadata"]["manifest"]["id"],
        serde_json::Value::String("native-clean-driver".into())
    );
    assert_eq!(inspected["metadata"]["package_version"], 2);

    let index = Index {
        index_version: 1,
        drivers: vec![
            entry("1.2.3", &package_v1, &sha_v1, &requirement),
            entry("2.0.0", &package_v2, &sha_v2, &requirement),
        ],
    };
    index.validate().unwrap();
    let index_path = repo.join("index.json");
    fs::write(&index_path, serde_json::to_vec_pretty(&index).unwrap()).unwrap();

    let receipt_v1 = run(&[
        "install",
        index_path.to_str().unwrap(),
        "native-clean-driver",
        "1.2.3",
        data.to_str().unwrap(),
        config.to_str().unwrap(),
    ]);
    assert_eq!(receipt_v1["package_sha256"], sha_v1);
    let executable_v1 = data.join("native-clean-driver/1.2.3/driver");
    assert_eq!(fs::read(&executable_v1).unwrap(), payload_v1);
    assert_eq!(
        fs::metadata(&executable_v1).unwrap().permissions().mode() & 0o777,
        0o700
    );

    let receipt_v2 = run(&[
        "install",
        index_path.to_str().unwrap(),
        "native-clean-driver",
        "2.0.0",
        data.to_str().unwrap(),
        config.to_str().unwrap(),
    ]);
    assert_eq!(receipt_v2["package_sha256"], sha_v2);
    let executable_v2 = data.join("native-clean-driver/2.0.0/driver");
    assert_eq!(fs::read(&executable_v2).unwrap(), payload_v2);
    assert!(
        executable_v1.is_file(),
        "upgrade must not silently delete rollback bytes"
    );

    let installed_manifest_path = config.join("native-clean-driver.json");
    let installed: Manifest =
        serde_json::from_slice(&fs::read(&installed_manifest_path).unwrap()).unwrap();
    assert_eq!(installed.version, "2.0.0");
    assert_eq!(installed.executable, executable_v2);
    assert!(!installed.network);
    assert!(installed.mounts.is_empty());
    assert!(installed.secrets.is_empty());
    assert!(installed.tools.is_empty());

    let removed_old = run(&[
        "remove",
        "native-clean-driver",
        "1.2.3",
        data.to_str().unwrap(),
        config.to_str().unwrap(),
    ]);
    assert_eq!(removed_old["removed"], true);
    assert!(!executable_v1.exists());
    let still_active: Manifest =
        serde_json::from_slice(&fs::read(&installed_manifest_path).unwrap()).unwrap();
    assert_eq!(still_active.version, "2.0.0");

    let removed_new = run(&[
        "remove",
        "native-clean-driver",
        "2.0.0",
        data.to_str().unwrap(),
        config.to_str().unwrap(),
    ]);
    assert_eq!(removed_new["removed"], true);
    assert!(!executable_v2.exists());
    assert!(!installed_manifest_path.exists());
}

#[test]
fn altered_package_and_incompatible_index_are_rejected_before_installation() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    let source = temp.path().join("source");
    let data = temp.path().join("data");
    let config = temp.path().join("config");
    fs::create_dir(&repo).unwrap();

    let requirement = format!("={}", env!("CARGO_PKG_VERSION"));
    let (package, digest) = pack(
        &repo,
        &source,
        "3.0.0",
        b"\x7fELFsemwright-native-package-v3-not-runnable",
        &requirement,
    );

    let tampered = repo.join("tampered.swdp");
    fs::copy(&package, &tampered).unwrap();
    let mut bytes = fs::read(&tampered).unwrap();
    let last = bytes.last_mut().unwrap();
    *last ^= 0x01;
    fs::write(&tampered, bytes).unwrap();
    run_fail(&["inspect", tampered.to_str().unwrap()]);

    let mut incompatible = entry("3.0.0", &package, &digest, "=0.0.0");
    incompatible.package_bytes = fs::metadata(&package).unwrap().len();
    let index = Index {
        index_version: 1,
        drivers: vec![incompatible],
    };
    index.validate().unwrap();
    let index_path = repo.join("incompatible-index.json");
    fs::write(&index_path, serde_json::to_vec_pretty(&index).unwrap()).unwrap();

    run_fail(&[
        "install",
        index_path.to_str().unwrap(),
        "native-clean-driver",
        "3.0.0",
        data.to_str().unwrap(),
        config.to_str().unwrap(),
    ]);
    assert!(!data.exists());
    assert!(!config.exists());
}
