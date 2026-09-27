#![cfg(target_os = "windows")]

use semwright_platform_windows_sys::launch::verify_private_data_file;
use semwright_types::ErrorCode;
use std::path::Path;

fn harden_file(path: &Path) {
    let user = std::env::var("USERNAME").expect("Windows USERNAME");
    let principal = match std::env::var("USERDOMAIN") {
        Ok(domain) if !domain.is_empty() => format!(r"{domain}\{user}"),
        _ => user,
    };
    let status = std::process::Command::new("icacls")
        .arg(path)
        .arg("/inheritance:r")
        .arg("/grant:r")
        .arg(format!("{principal}:(F)"))
        .status()
        .expect("run icacls");
    assert!(status.success(), "private-file DACL hardening must succeed");
}

#[test]
fn private_data_accepts_small_owner_controlled_file() {
    let directory = tempfile::tempdir().expect("private-data directory");
    let secret = directory.path().join("secret.txt");
    std::fs::write(&secret, b"secret").expect("write private data");
    harden_file(&secret);
    verify_private_data_file(&secret, 4096).expect("verify private data");
}

#[test]
fn private_data_rejects_hard_linked_source() {
    let directory = tempfile::tempdir().expect("private-data directory");
    let secret = directory.path().join("secret.txt");
    let alias = directory.path().join("alias.txt");
    std::fs::write(&secret, b"secret").expect("write private data");
    harden_file(&secret);
    std::fs::hard_link(&secret, &alias).expect("create hard link");
    let error = verify_private_data_file(&secret, 4096).expect_err("hard-linked private data");
    assert_eq!(error.code, ErrorCode::PermissionDenied);
}

#[test]
fn private_data_rejects_empty_or_oversized_sources() {
    let directory = tempfile::tempdir().expect("private-data directory");

    let empty = directory.path().join("empty.txt");
    std::fs::write(&empty, b"").expect("write empty private data");
    harden_file(&empty);
    assert!(verify_private_data_file(&empty, 4096).is_err());

    let oversized = directory.path().join("oversized.txt");
    std::fs::write(&oversized, vec![b'x'; 4097]).expect("write oversized private data");
    harden_file(&oversized);
    assert!(verify_private_data_file(&oversized, 4096).is_err());
}
