use super::*;

fn fixture_manifest_source() -> Vec<u8> {
    let executable_json = if cfg!(windows) {
        r#""C:\\semwright\\driver.exe""#
    } else {
        r#""/tmp/semwright-fixture-driver""#
    };
    format!(
        r#"
{{
    "manifest_version": 1,
    "protocol": 7,
    "id": "fixture",
    "version": "1.0",
    "publisher": "semwright-tests",
    "executable": {executable_json},
    "sha256": "{driver_sha256}",
    "application": {{ "desktop_id": "org.example.Fixture" }},
    "transport": "stdio_v1",
    "tools": [
        {{
            "root": "node",
            "name": "node",
            "sha256": "{tool_sha256}",
            "mounts": []
        }}
    ],
    "interfaces": {{ "host_tools": true }}
}}
"#,
        driver_sha256 = "a".repeat(64),
        tool_sha256 = "b".repeat(64),
    )
    .into_bytes()
}

fn fixture_manifest() -> Manifest {
    let manifest: Manifest = serde_json::from_slice(&fixture_manifest_source()).unwrap();
    manifest.validate().unwrap();
    manifest
}

fn request() -> Request {
    Request {
        directory: PathBuf::from("unused-private-directory"),
        provider: "driver:fixture".into(),
        tool: "node".into(),
    }
}

#[test]
fn default_off_and_partial_options_never_enable_custody() {
    assert!(Options::default().disabled());
    let absent = PathBuf::from("manifest-that-does-not-exist.json");
    assert!(
        Bootstrap::prepare(&Options::default(), true, &[absent])
            .unwrap()
            .is_none()
    );
    for mask in 1u8..7 {
        let options = Options {
            host_stdin_custody_dir: (mask & 1 != 0).then(|| PathBuf::from("unused")),
            host_stdin_custody_provider: (mask & 2 != 0).then(|| "driver:fixture".into()),
            host_stdin_custody_tool: (mask & 4 != 0).then(|| "node".into()),
        };
        assert!(options.request().is_err(), "partial mask {mask}");
    }
    let options = Options {
        host_stdin_custody_dir: Some(PathBuf::from("unused")),
        host_stdin_custody_provider: Some("driver:fixture".into()),
        host_stdin_custody_tool: Some("node".into()),
    };
    assert!(Bootstrap::prepare(&options, true, &[]).is_err());
}

#[test]
fn selection_requires_exact_existing_unique_provider_and_v7_tool() {
    let baseline = fixture_manifest();
    let path = PathBuf::from("fixture.json");
    let plan = Plan::select(vec![(path.clone(), baseline.clone())], &request()).unwrap();
    assert_eq!(plan.selected, baseline.identity().unwrap());
    assert_eq!(plan.driver_sha256, baseline.sha256);
    assert_eq!(plan.tool_sha256, "b".repeat(64));
    let mut wrong = request();
    wrong.provider = "driver:other".into();
    assert!(Plan::select(vec![(path.clone(), baseline.clone())], &wrong).is_err());
    wrong = request();
    wrong.tool = "unconfigured-tool".into();
    assert!(Plan::select(vec![(path.clone(), baseline.clone())], &wrong).is_err());
    assert!(
        Plan::select(
            vec![
                (path.clone(), baseline.clone()),
                (PathBuf::from("duplicate.json"), baseline.clone())
            ],
            &request()
        )
        .is_err()
    );
    let mut old = baseline.clone();
    old.protocol = 6;
    assert!(Plan::select(vec![(path.clone(), old)], &request()).is_err());
    let mut no_host = baseline;
    no_host.interfaces.host_tools = false;
    assert!(Plan::select(vec![(path, no_host)], &request()).is_err());
}

#[test]
fn quota_and_encoded_metadata_reject_before_reserving_output() {
    let mut budget = Budget::default();
    assert!(budget.reserve(MAX_INPUT + 1, 1).is_err());
    assert!(budget.reserve(1, MAX_METADATA + 1).is_err());
    assert_eq!((budget.records, budget.input_bytes), (0, 0));
    let metadata = json!({"private_field": "x".repeat(MAX_METADATA)});
    assert!(bounded_json(&metadata, MAX_METADATA).is_err());
    for expected in 1..=MAX_RECORDS {
        assert_eq!(budget.reserve(MAX_INPUT, MAX_METADATA).unwrap(), expected);
    }
    assert_eq!(budget.input_bytes, MAX_TOTAL_INPUT);
    assert!(budget.reserve(0, 0).is_err());
    assert_eq!(
        (budget.records, budget.input_bytes),
        (MAX_RECORDS, MAX_TOTAL_INPUT)
    );
}

#[test]
fn original_pipe_terminals_are_distinct_and_never_imply_consumption() {
    let cases = [
        (HostStdinWriteTerminal::NotObserved, "not_observed", true),
        (HostStdinWriteTerminal::NotAttempted, "not_attempted", false),
        (
            HostStdinWriteTerminal::EmptyInputNoWrite,
            "empty_input_no_write",
            false,
        ),
        (
            HostStdinWriteTerminal::WriteAllCompleted,
            "write_all_completed",
            false,
        ),
        (
            HostStdinWriteTerminal::WriteAllFailed {
                kind: std::io::ErrorKind::BrokenPipe,
                raw_os_error: Some(32),
            },
            "write_all_failed",
            true,
        ),
        (
            HostStdinWriteTerminal::WriteAllInterrupted,
            "write_all_interrupted",
            true,
        ),
    ];
    for (terminal, state, partial) in cases {
        let record = terminal_metadata(terminal);
        assert_eq!(record["write_state"], state);
        assert_eq!(record["partial_delivery_possible"], partial);
        assert_eq!(record["child_consumption"], "not_observed");
        assert_eq!(record["native_render_success"], "not_inferred");
        assert!(bounded_json(&record, MAX_TERMINAL).is_ok());
        if state == "write_all_failed" {
            assert_eq!(record["io_error_kind"], "BrokenPipe");
            assert_eq!(record["raw_os_error"], 32);
        }
    }
}

#[test]
fn setup_sanitization_preserves_known_and_unknown_without_private_details() {
    for outcome_known in [true, false] {
        let mut original = Error::new(ErrorCode::BackendFailed, "private path and payload");
        original.outcome_known = outcome_known;
        original.candidates.push("private correlation".into());
        let sanitized = setup_error(original);
        assert_eq!(sanitized.code, ErrorCode::BackendFailed);
        assert_eq!(sanitized.outcome_known, outcome_known);
        assert_eq!(sanitized.message, "Private Host stdin custody setup failed");
        assert!(sanitized.candidates.is_empty());
        assert!(sanitized.progress.is_none());
        assert!(sanitized.native_diagnostic.is_none());
    }
}

#[cfg(not(target_os = "linux"))]
#[test]
fn unsupported_writer_does_not_enable_a_false_platform_observation() {
    assert_eq!(
        PrivateDirectory::open(Path::new("unused"))
            .err()
            .unwrap()
            .code,
        ErrorCode::Unsupported
    );
    assert!(
        Bootstrap::prepare(&Options::default(), false, &[])
            .unwrap()
            .is_none()
    );
    assert_eq!(
        terminal_metadata(HostStdinWriteTerminal::NotObserved)["write_state"],
        "not_observed"
    );
}

#[cfg(target_os = "linux")]
fn private_fixture() -> (tempfile::TempDir, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let parent = tempfile::tempdir().unwrap();
    std::fs::set_permissions(parent.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let directory = parent.path().join("custody");
    std::fs::create_dir(&directory).unwrap();
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    (parent, directory)
}

#[cfg(target_os = "linux")]
#[test]
fn private_directory_refuses_reuse_links_and_nonprivate_parent() {
    use std::os::unix::fs::PermissionsExt;
    let (parent, directory) = private_fixture();
    let retained = PrivateDirectory::open(&directory).unwrap();
    retained.write_new("original.stdin", b"original").unwrap();
    assert!(
        retained
            .write_new("original.stdin", b"replacement")
            .is_err()
    );
    assert_eq!(
        std::fs::read(directory.join("original.stdin")).unwrap(),
        b"original"
    );
    assert!(PrivateDirectory::open(&directory).is_err());
    let alias = parent.path().join("alias");
    std::os::unix::fs::symlink(&directory, &alias).unwrap();
    assert!(PrivateDirectory::open(&alias).is_err());
    let empty = parent.path().join("empty");
    std::fs::create_dir(&empty).unwrap();
    std::fs::set_permissions(&empty, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::set_permissions(parent.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(PrivateDirectory::open(&empty).is_err());
    std::fs::set_permissions(parent.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn retained_directory_never_follows_a_replacement_path() {
    use std::os::unix::fs::PermissionsExt;
    let (parent, directory) = private_fixture();
    let retained = PrivateDirectory::open(&directory).unwrap();
    let moved = parent.path().join("retained-original");
    std::fs::rename(&directory, &moved).unwrap();
    std::fs::create_dir(&directory).unwrap();
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    retained
        .write_new("original.stdin", b"retained bytes")
        .unwrap();
    assert_eq!(
        std::fs::read(moved.join("original.stdin")).unwrap(),
        b"retained bytes"
    );
    assert!(!directory.join("original.stdin").exists());
}

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retained_manifest_and_drop_do_not_fabricate_native_terminals() {
    use std::os::unix::fs::PermissionsExt;
    let (parent, directory) = private_fixture();
    let manifest_path = parent.path().join("driver.json");
    let original_bytes = fixture_manifest_source();
    let original: Manifest = serde_json::from_slice(&original_bytes).unwrap();
    original.validate().unwrap();
    std::fs::write(&manifest_path, &original_bytes).unwrap();
    std::fs::set_permissions(&manifest_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let options = Options {
        host_stdin_custody_dir: Some(directory.clone()),
        host_stdin_custody_provider: Some("driver:fixture".into()),
        host_stdin_custody_tool: Some("node".into()),
    };
    let mut bootstrap = Bootstrap::prepare(&options, false, std::slice::from_ref(&manifest_path))
        .unwrap()
        .unwrap();
    let (_, cached_original) = bootstrap
        .retained_manifest(&manifest_path)
        .unwrap()
        .into_parts();
    assert_eq!(cached_original.as_ref(), original_bytes.as_slice());
    let changed_bytes = String::from_utf8(original_bytes.clone())
        .unwrap()
        .replacen("\"version\": \"1.0\"", "\"version\": \"2.0\"", 1)
        .replacen(&"a".repeat(64), &"c".repeat(64), 1)
        .into_bytes();
    let changed: Manifest = serde_json::from_slice(&changed_bytes).unwrap();
    changed.validate().unwrap();
    assert_eq!(changed.version, "2.0");
    assert_eq!(changed.sha256, "c".repeat(64));
    std::fs::write(&manifest_path, &changed_bytes).unwrap();
    let retained = bootstrap.retained_manifest(&manifest_path).unwrap();
    assert_eq!(retained.parsed().version, original.version);
    assert_eq!(retained.parsed().sha256, original.sha256);
    assert!(bootstrap.sink_for(retained.parsed()).is_some());
    assert!(bootstrap.sink_for(&changed).is_none());
    let retained_clone = retained.clone();
    let (_, retained_bytes) = retained.into_parts();
    let (_, cloned_bytes) = retained_clone.into_parts();
    assert_eq!(retained_bytes.as_ref(), original_bytes.as_slice());
    assert_ne!(retained_bytes.as_ref(), changed_bytes.as_slice());
    assert!(Arc::ptr_eq(&cached_original, &retained_bytes));
    assert!(Arc::ptr_eq(&retained_bytes, &cloned_bytes));
    assert!(
        bootstrap
            .retained_manifest(&parent.path().join("unselected.json"))
            .is_err()
    );
    let finished = bootstrap.worker.take().unwrap();
    drop(bootstrap);
    tokio::time::timeout(Duration::from_secs(2), finished)
        .await
        .unwrap()
        .unwrap();
    let closed: Value =
        serde_json::from_slice(&std::fs::read(directory.join("closed.json")).unwrap()).unwrap();
    assert_eq!(closed["capture_records_written"], 0);
    assert_eq!(closed["terminal_records_written"], 0);
    assert_eq!(closed["queue_disconnection_proven"], true);
    assert_eq!(closed["queued_captures_discarded"], 0);
    assert_eq!(closed["child_consumption"], "not_observed");
    assert_eq!(closed["native_render_success"], "not_inferred");
    assert!(!directory.join("capture-0001.stdin").exists());
    assert!(!directory.join("capture-0001.terminal.json").exists());
}

#[cfg(target_os = "linux")]
#[test]
fn custody_directory_is_denied_to_overlapping_filesystem_grants() {
    let (parent, directory) = private_fixture();
    for root in [parent.path().to_path_buf(), directory.clone()] {
        let mut config = semwright_daemon::config::Config::default();
        config
            .policy
            .filesystem
            .push(semwright_policy::FilesystemGrant {
                name: "fixture".into(),
                path: root,
                read: true,
                write: false,
            });
        let error =
            semwright_daemon::config::confine_grants(&config, std::slice::from_ref(&directory))
                .unwrap_err();
        assert_eq!(error.code, ErrorCode::PolicyDenied);
    }
    let separate = parent.path().join("separate");
    std::fs::create_dir(&separate).unwrap();
    let mut config = semwright_daemon::config::Config::default();
    config
        .policy
        .filesystem
        .push(semwright_policy::FilesystemGrant {
            name: "fixture".into(),
            path: separate,
            read: true,
            write: false,
        });
    semwright_daemon::config::confine_grants(&config, &[directory]).unwrap();
}
