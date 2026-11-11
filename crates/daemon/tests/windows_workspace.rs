#![cfg(target_os = "windows")]

use semwright_backend_api::{Backend, Context};
use semwright_daemon::config::{Config, confine_grants, directory_filesystem_grants};
use semwright_platform_common::filesystem::Filesystem;
use semwright_policy::FilesystemGrant;
use semwright_types::ErrorCode;
use serde_json::json;
use std::path::Path;
use tokio_util::sync::CancellationToken;

fn configuration(path: &Path) -> Config {
    let mut config = Config::default();
    config.policy.filesystem.push(FilesystemGrant {
        name: "workspace".into(),
        path: path.to_path_buf(),
        read: true,
        write: true,
    });
    config
}

fn junction(link: &Path, target: &Path) {
    let status = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .status()
        .unwrap();
    assert!(status.success(), "junction fixture creation must succeed");
}

#[tokio::test]
async fn ordinary_drive_grant_reaches_the_scoped_backend() {
    let parent = tempfile::tempdir().unwrap();
    let workspace = parent.path().join("workspace");
    let private = parent.path().join("private");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&private).unwrap();
    let config = configuration(&workspace);
    confine_grants(&config, &[private]).unwrap();

    let grants = directory_filesystem_grants(&config).unwrap();
    let backend = Filesystem::new(&grants).unwrap();
    let context = Context {
        session: "workspace-test".into(),
        request_id: "write".into(),
        cancellation: CancellationToken::new(),
    };
    for text in ["first", "second"] {
        let written = backend
            .execute(
                &context,
                "filesystem.write",
                &json!({"root":"workspace","path":"task.txt","text":text}),
            )
            .await
            .unwrap();
        assert_eq!(written["written"], true);
        let read = backend
            .execute(
                &context,
                "filesystem.read",
                &json!({"root":"workspace","path":"task.txt"}),
            )
            .await
            .unwrap();
        assert_eq!(read["text"], text);
        assert_eq!(
            std::fs::read(workspace.join("task.txt")).unwrap(),
            text.as_bytes()
        );
    }
    assert!(
        backend
            .execute(
                &context,
                "filesystem.read",
                &json!({"root":"workspace","path":"../private/secret.txt"}),
            )
            .await
            .is_err()
    );
}

#[test]
fn verbatim_owner_input_is_not_silently_rewritten() {
    let directory = tempfile::tempdir().unwrap();
    let canonical = std::fs::canonicalize(directory.path()).unwrap();
    let error = confine_grants(&configuration(&canonical), &[]).unwrap_err();
    assert_eq!(error.code, ErrorCode::PolicyDenied);
}

#[test]
fn protected_overlap_is_checked_in_both_directions() {
    let parent = tempfile::tempdir().unwrap();
    let private = parent.path().join("private");
    let nested = private.join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    for (grant, protected) in [
        (parent.path(), private.as_path()),
        (private.as_path(), private.as_path()),
        (nested.as_path(), private.as_path()),
    ] {
        let error = confine_grants(&configuration(grant), &[protected.into()]).unwrap_err();
        assert_eq!(error.code, ErrorCode::PolicyDenied);
        let canonical = std::fs::canonicalize(protected).unwrap();
        let error = confine_grants(&configuration(grant), &[canonical]).unwrap_err();
        assert_eq!(error.code, ErrorCode::PolicyDenied);
    }
}

#[test]
fn junction_at_root_or_in_an_ancestor_is_refused() {
    let parent = tempfile::tempdir().unwrap();
    let outside = parent.path().join("outside");
    let nested = outside.join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    let link = parent.path().join("link");
    junction(&link, &outside);
    for grant in [&link, &link.join("nested")] {
        let error = confine_grants(&configuration(grant), &[]).unwrap_err();
        assert_eq!(error.code, ErrorCode::PolicyDenied);
    }
    std::fs::remove_dir(&link).unwrap();
}

#[test]
fn case_alias_does_not_bypass_protected_overlap() {
    let parent = tempfile::tempdir().unwrap();
    let private = parent.path().join("PrivateState");
    std::fs::create_dir(&private).unwrap();
    let alias = private.to_string_lossy().to_ascii_lowercase();
    let error = confine_grants(&configuration(Path::new(&alias)), &[private]).unwrap_err();
    assert_eq!(error.code, ErrorCode::PolicyDenied);
}

#[test]
fn sibling_with_shared_name_prefix_is_not_an_overlap() {
    let parent = tempfile::tempdir().unwrap();
    let private = parent.path().join("private");
    let workspace = parent.path().join("private-project");
    std::fs::create_dir(&private).unwrap();
    std::fs::create_dir(&workspace).unwrap();
    confine_grants(&configuration(&workspace), &[private]).unwrap();
}
