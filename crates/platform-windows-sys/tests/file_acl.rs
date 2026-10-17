#![cfg(target_os = "windows")]

use semwright_platform_windows_sys::launch::{
    verify_application_executable, verify_private_data_file,
};
use semwright_types::ErrorCode;
use sha2::{Digest, Sha256};
use std::{path::Path, process::Command};

fn icacls(path: &Path, arguments: &[String]) {
    let output = Command::new("icacls")
        .arg(path)
        .args(arguments)
        .output()
        .expect("run fixture icacls");
    assert!(
        output.status.success(),
        "fixture DACL update failed: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn fixture(
    inherited: bool,
    rights: &str,
    private: bool,
) -> (tempfile::TempDir, std::path::PathBuf, String) {
    let directory = tempfile::tempdir().expect("ACL fixture directory");
    let user = std::env::var("USERNAME").expect("Windows USERNAME");
    let principal = match std::env::var("USERDOMAIN") {
        Ok(domain) if !domain.is_empty() => format!(r"{domain}\{user}"),
        _ => user,
    };
    icacls(
        directory.path(),
        &[
            "/inheritance:r".into(),
            "/grant:r".into(),
            format!("{principal}:(OI)(CI)(F)"),
        ],
    );
    if inherited {
        icacls(
            directory.path(),
            &["/grant:r".into(), format!("*S-1-1-0:(OI)(CI)({rights})")],
        );
    }
    let path = directory.path().join("fixture.exe");
    let bytes = if private {
        b"private fixture".to_vec()
    } else {
        std::fs::read(std::env::current_exe().expect("current test executable"))
            .expect("read native PE fixture")
    };
    std::fs::write(&path, &bytes).expect("write ACL fixture");
    if !inherited {
        icacls(&path, &["/grant:r".into(), format!("*S-1-1-0:({rights})")]);
    }
    let digest = hex::encode(Sha256::digest(&bytes));
    (directory, path, digest)
}

fn check_executable_acl(inherited: bool, rights: &str, accepted: bool) {
    let (_directory, path, digest) = fixture(inherited, rights, false);
    let result = verify_application_executable(&path, &digest);
    if accepted {
        result.unwrap_or_else(|error| {
            panic!("executable rights {rights}, inherited={inherited}: {error:?}")
        });
    } else {
        let error = result.expect_err("untrusted executable mutation must be denied");
        assert_eq!(error.code, ErrorCode::PermissionDenied, "{rights}");
        assert!(error.message.contains("DACL"), "{rights}: {error:?}");
    }
}

fn check_private_acl(inherited: bool, rights: &str, accepted: bool) {
    let (_directory, path, _) = fixture(inherited, rights, true);
    let result = verify_private_data_file(&path, 4096);
    if accepted {
        result.unwrap_or_else(|error| {
            panic!("private-file rights {rights}, inherited={inherited}: {error:?}")
        });
    } else {
        let error = result.expect_err("untrusted private-file access must be denied");
        assert_eq!(error.code, ErrorCode::PermissionDenied, "{rights}");
        assert!(error.message.contains("DACL"), "{rights}: {error:?}");
    }
}

#[test]
fn executable_accepts_explicit_read_execute_and_metadata() {
    for rights in ["R", "RX", "RA", "REA", "RC", "S"] {
        check_executable_acl(false, rights, true);
    }
}

#[test]
fn executable_accepts_inherited_read_execute_and_metadata() {
    for rights in ["R", "RX", "RA", "REA", "RC", "S"] {
        check_executable_acl(true, rights, true);
    }
}

#[test]
fn executable_rejects_explicit_mutation() {
    for rights in ["WD", "AD", "WEA", "WA", "D", "WDAC", "WO", "W", "M", "F"] {
        check_executable_acl(false, rights, false);
    }
}

#[test]
fn executable_rejects_inherited_mutation() {
    for rights in ["WD", "AD", "WEA", "WA", "D", "WDAC", "WO", "W", "M", "F"] {
        check_executable_acl(true, rights, false);
    }
}

#[test]
fn private_data_accepts_explicit_metadata_only() {
    for rights in ["RC", "S"] {
        check_private_acl(false, rights, true);
    }
}

#[test]
fn private_data_accepts_inherited_metadata_only() {
    for rights in ["RC", "S"] {
        check_private_acl(true, rights, true);
    }
}

#[test]
fn private_data_rejects_explicit_read_and_write() {
    for rights in [
        "R", "RX", "RD", "RA", "REA", "WD", "AD", "WEA", "WA", "D", "WDAC", "WO", "W", "M", "F",
    ] {
        check_private_acl(false, rights, false);
    }
}

#[test]
fn private_data_rejects_inherited_read_and_write() {
    for rights in [
        "R", "RX", "RD", "RA", "REA", "WD", "AD", "WEA", "WA", "D", "WDAC", "WO", "W", "M", "F",
    ] {
        check_private_acl(true, rights, false);
    }
}
