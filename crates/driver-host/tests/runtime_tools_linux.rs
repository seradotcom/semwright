#![cfg(all(target_os = "linux", feature = "test-tools"))]

use semwright_backend_api::{Context, Provider};
use semwright_driver_host::DriverProvider;
use semwright_driver_sdk::{
    ApplicationMatch, DriverInterfaces, DriverMount, DriverResources, DriverToolMount, Manifest,
    SystemConfigMount, Transport,
};
use semwright_policy::FilesystemGrant;
use semwright_types::ErrorCode;
use sha2::{Digest, Sha256};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
use tokio_util::sync::CancellationToken;

fn digest(path: &Path) -> String {
    format!(
        "{:x}",
        Sha256::digest(std::fs::read(path).expect("read runtime-tool fixture"))
    )
}

fn harden(path: &Path) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o500))
        .expect("harden runtime-tool fixture");
}

fn sandbox_helper() -> PathBuf {
    PathBuf::from(
        std::env::var_os("SEMWRIGHT_TEST_SANDBOX_HELPER")
            .expect("SEMWRIGHT_TEST_SANDBOX_HELPER must point to semwright-sandbox"),
    )
}

fn manifest(executable: PathBuf, tool_digest: String, protocol: u32) -> Manifest {
    Manifest {
        manifest_version: 1,
        protocol,
        id: "fixture".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        publisher: "semwright-tests".into(),
        executable: executable.clone(),
        sha256: digest(&executable),
        application: ApplicationMatch {
            desktop_id: Some("org.semwright.DriverFixture".into()),
            process_names: vec![],
            supported_versions: vec![],
        },
        transport: Transport::StdioV1,
        mounts: vec![
            DriverMount {
                root: "tool-workspace".into(),
                read_only: false,
                execute: false,
            },
            DriverMount {
                root: "other-workspace".into(),
                read_only: false,
                execute: false,
            },
        ],
        system_config: vec![],
        secrets: vec![],
        tools: vec![DriverToolMount {
            root: "fixture-tool-root".into(),
            name: "probe".into(),
            sha256: tool_digest,
            mounts: vec!["tool-workspace".into()],
            system_config: vec![],
            dependencies: vec![],
        }],
        network: false,
        loopback_port: None,
        resources: DriverResources {
            open_files: 128,
            processes: 16,
            cpu_seconds: 20,
            operation_cpu_seconds: 0,
            address_space_bytes: 512 * 1024 * 1024,
            file_size_bytes: 16 * 1024 * 1024,
        },
        request_timeout_ms: 5_000,
        interfaces: DriverInterfaces {
            dynamic_capabilities: true,
            cooperative_cancellation: true,
            events: true,
            progress: true,
            artifacts: true,
            health: true,
            native_refs: false,
            host_tools: true,
        },
    }
}

fn manifest_v7(executable: PathBuf, probe_digest: String, helper_digest: String) -> Manifest {
    let mut candidate = manifest(executable, probe_digest, 7);
    candidate.tools[0].dependencies = vec!["helper".into()];
    candidate.tools.push(DriverToolMount {
        root: "helper-tool-root".into(),
        name: "helper".into(),
        sha256: helper_digest,
        mounts: vec![],
        system_config: vec![],
        dependencies: vec![],
    });
    candidate
}

fn manifest_v8(executable: PathBuf, probe_digest: String) -> Manifest {
    let mut candidate = manifest(executable, probe_digest, 8);
    candidate.system_config = vec![
        SystemConfigMount {
            root: "tool-config-root".into(),
            destination: "/etc/runtime-config".into(),
        },
        SystemConfigMount {
            root: "other-config-root".into(),
            destination: "/etc/other-config".into(),
        },
    ];
    candidate.tools[0].system_config = vec!["tool-config-root".into()];
    candidate
}

#[tokio::test]
#[ignore = "requires real Bubblewrap + Landlock support"]
async fn linux_v5_runtime_tool_is_host_mediated_and_mount_scoped() {
    assert!(Path::new("/usr/bin/bwrap").is_file());

    let driver_source = PathBuf::from(env!("CARGO_BIN_EXE_semwright-driver-fixture"));
    let tool_source = PathBuf::from(env!("CARGO_BIN_EXE_semwright-tool-fixture"));
    let binary_dir = tempfile::tempdir().expect("fixture directory");
    let driver = binary_dir.path().join("driver");
    let tool = binary_dir.path().join("tool");
    std::fs::copy(&driver_source, &driver).expect("copy driver fixture");
    std::fs::copy(&tool_source, &tool).expect("copy tool fixture");
    harden(&driver);
    harden(&tool);

    let allowed = tempfile::tempdir().expect("allowed tool workspace");
    let denied = tempfile::tempdir().expect("driver-only workspace");
    std::fs::write(allowed.path().join("allowed.txt"), b"allowed").unwrap();
    std::fs::write(denied.path().join("denied.txt"), b"denied").unwrap();

    let roots = vec![
        FilesystemGrant {
            name: "fixture-tool-root".into(),
            path: tool.canonicalize().expect("canonical tool"),
            read: true,
            write: false,
        },
        FilesystemGrant {
            name: "tool-workspace".into(),
            path: allowed
                .path()
                .canonicalize()
                .expect("canonical allowed workspace"),
            read: true,
            write: true,
        },
        FilesystemGrant {
            name: "other-workspace".into(),
            path: denied
                .path()
                .canonicalize()
                .expect("canonical denied workspace"),
            read: true,
            write: true,
        },
    ];

    let state = tempfile::tempdir().expect("driver state");
    std::fs::set_permissions(state.path(), std::fs::Permissions::from_mode(0o700))
        .expect("harden driver state");
    let provider = DriverProvider::connect(
        manifest(driver, digest(&tool), 5),
        state.path(),
        &sandbox_helper(),
        &roots,
        false,
    )
    .await
    .expect("Linux v5 runtime-tool Driver Host");

    let capabilities = Provider::capabilities(provider.as_ref())
        .await
        .expect("driver capabilities");
    let probe = capabilities
        .iter()
        .find(|capability| capability.descriptor.name == "driver.fixture.tool_probe")
        .expect("fixture tool capability")
        .descriptor
        .clone();

    let output = Provider::execute(
        provider.as_ref(),
        &Context {
            session: "linux-runtime-tool-cwd".into(),
            request_id: "linux-runtime-tool-cwd-allowed".into(),
            cancellation: CancellationToken::new(),
        },
        &probe,
        &serde_json::json!({"cwd_mount":"tool-workspace","cwd_relative":""}),
    )
    .await
    .expect("runtime tool should execute inside delegated Linux cwd");

    assert_eq!(output["exit_code"], 0);
    assert_eq!(
        output["read_ok"], false,
        "protocol-v5 Linux driver must not resolve a direct sealed-tool path: {output}"
    );
    assert_eq!(
        output["stdout"], "tool-ok|cwd=/workspace/tool-workspace",
        "Host-mediated Linux runtime tool must start at the logical delegated mount: {output}"
    );

    let error = Provider::execute(
        provider.as_ref(),
        &Context {
            session: "linux-runtime-tool-cwd".into(),
            request_id: "linux-runtime-tool-cwd-denied".into(),
            cancellation: CancellationToken::new(),
        },
        &probe,
        &serde_json::json!({"cwd_mount":"other-workspace","cwd_relative":""}),
    )
    .await
    .expect_err("Linux runtime tool must not inherit an undeclared workspace mount");
    assert_eq!(error.code, ErrorCode::PolicyDenied);

    Provider::shutdown(provider.as_ref())
        .await
        .expect("runtime-tool Driver Host shutdown");
}

#[tokio::test]
#[ignore = "requires real Bubblewrap + Landlock support"]
async fn linux_v7_runtime_tool_paths_are_mount_and_dependency_scoped() {
    assert!(Path::new("/usr/bin/bwrap").is_file());

    let driver_source = PathBuf::from(env!("CARGO_BIN_EXE_semwright-driver-fixture"));
    let tool_source = PathBuf::from(env!("CARGO_BIN_EXE_semwright-tool-fixture"));
    let binary_dir = tempfile::tempdir().expect("fixture directory");
    let driver = binary_dir.path().join("driver");
    let probe_tool = binary_dir.path().join("probe-tool");
    let helper_tool = binary_dir.path().join("helper-tool");
    std::fs::copy(&driver_source, &driver).expect("copy driver fixture");
    std::fs::copy(&tool_source, &probe_tool).expect("copy probe fixture");
    std::fs::copy(&tool_source, &helper_tool).expect("copy helper fixture");
    harden(&driver);
    harden(&probe_tool);
    harden(&helper_tool);

    let allowed = tempfile::tempdir().expect("typed tool workspace");
    let denied = tempfile::tempdir().expect("undelegated tool workspace");
    std::fs::write(allowed.path().join("allowed.txt"), b"allowed").unwrap();
    std::fs::write(denied.path().join("denied.txt"), b"denied").unwrap();

    let roots = vec![
        FilesystemGrant {
            name: "fixture-tool-root".into(),
            path: probe_tool.canonicalize().expect("canonical probe"),
            read: true,
            write: false,
        },
        FilesystemGrant {
            name: "helper-tool-root".into(),
            path: helper_tool.canonicalize().expect("canonical helper"),
            read: true,
            write: false,
        },
        FilesystemGrant {
            name: "tool-workspace".into(),
            path: allowed.path().canonicalize().expect("canonical workspace"),
            read: true,
            write: true,
        },
        FilesystemGrant {
            name: "other-workspace".into(),
            path: denied.path().canonicalize().expect("canonical denied"),
            read: true,
            write: true,
        },
    ];

    let state = tempfile::tempdir().expect("driver state");
    std::fs::set_permissions(state.path(), std::fs::Permissions::from_mode(0o700))
        .expect("harden driver state");
    let provider = DriverProvider::connect(
        manifest_v7(driver, digest(&probe_tool), digest(&helper_tool)),
        state.path(),
        &sandbox_helper(),
        &roots,
        false,
    )
    .await
    .expect("Linux v7 runtime-tool Driver Host");

    let capabilities = Provider::capabilities(provider.as_ref())
        .await
        .expect("driver capabilities");
    let probe = capabilities
        .iter()
        .find(|capability| capability.descriptor.name == "driver.fixture.tool_probe")
        .expect("fixture tool capability")
        .descriptor
        .clone();

    let call = |request_id: &str, args: serde_json::Value| {
        let provider = provider.clone();
        let probe = probe.clone();
        let context = Context {
            session: "linux-v7-runtime-tools".into(),
            request_id: request_id.into(),
            cancellation: CancellationToken::new(),
        };
        async move { Provider::execute(provider.as_ref(), &context, &probe, &args).await }
    };

    let output = call(
        "linux-v7-typed-success",
        serde_json::json!({
            "path_mount":"tool-workspace",
            "path_relative":"",
            "dependency":"helper"
        }),
    )
    .await
    .expect("typed Linux runtime-tool path/dependency should execute");
    assert_eq!(output["exit_code"], 0);
    assert_eq!(output["read_ok"], false);
    assert_eq!(output["stdout"], "tool-ok|path=allowed|dependency=tool-ok");

    let repeated = call(
        "linux-v7-typed-success-repeat",
        serde_json::json!({
            "path_mount":"tool-workspace",
            "path_relative":"",
            "dependency":"helper"
        }),
    )
    .await
    .expect("the same sealed dependency must be reusable with a fresh file offset");
    assert_eq!(repeated["exit_code"], 0);
    assert_eq!(
        repeated["stdout"],
        "tool-ok|path=allowed|dependency=tool-ok"
    );

    let mount_error = call(
        "linux-v7-typed-mount-denied",
        serde_json::json!({
            "path_mount":"other-workspace",
            "path_relative":""
        }),
    )
    .await
    .expect_err("typed path must not exceed the tool mount allowlist");
    assert_eq!(mount_error.code, ErrorCode::PolicyDenied);

    let dependency_error = call(
        "linux-v7-typed-dependency-denied",
        serde_json::json!({"dependency":"missing"}),
    )
    .await
    .expect_err("typed dependency must be explicitly declared for the primary tool");
    assert_eq!(dependency_error.code, ErrorCode::PolicyDenied);

    Provider::shutdown(provider.as_ref())
        .await
        .expect("v7 runtime-tool Driver Host shutdown");
}

#[tokio::test]
#[ignore = "requires real Bubblewrap + Landlock support"]
async fn linux_v8_runtime_tool_receives_only_declared_system_config() {
    assert!(Path::new("/usr/bin/bwrap").is_file());

    let driver_source = PathBuf::from(env!("CARGO_BIN_EXE_semwright-driver-fixture"));
    let tool_source = PathBuf::from(env!("CARGO_BIN_EXE_semwright-tool-fixture"));
    let binary_dir = tempfile::tempdir().expect("fixture directory");
    let driver = binary_dir.path().join("driver");
    let tool = binary_dir.path().join("tool");
    std::fs::copy(&driver_source, &driver).expect("copy driver fixture");
    std::fs::copy(&tool_source, &tool).expect("copy tool fixture");
    harden(&driver);
    harden(&tool);

    let workspace = tempfile::tempdir().expect("tool workspace");
    let other_workspace = tempfile::tempdir().expect("other workspace");
    let configs = tempfile::tempdir().expect("system config sources");
    let allowed_config = configs.path().join("runtime-config");
    let other_config = configs.path().join("other-config");
    std::fs::write(&allowed_config, b"delegated-config").unwrap();
    std::fs::write(&other_config, b"driver-only-config").unwrap();

    let roots = vec![
        FilesystemGrant {
            name: "fixture-tool-root".into(),
            path: tool.canonicalize().expect("canonical tool"),
            read: true,
            write: false,
        },
        FilesystemGrant {
            name: "tool-workspace".into(),
            path: workspace
                .path()
                .canonicalize()
                .expect("canonical workspace"),
            read: true,
            write: true,
        },
        FilesystemGrant {
            name: "other-workspace".into(),
            path: other_workspace
                .path()
                .canonicalize()
                .expect("canonical other workspace"),
            read: true,
            write: true,
        },
        FilesystemGrant {
            name: "tool-config-root".into(),
            path: allowed_config
                .canonicalize()
                .expect("canonical allowed config"),
            read: true,
            write: false,
        },
        FilesystemGrant {
            name: "other-config-root".into(),
            path: other_config.canonicalize().expect("canonical other config"),
            read: true,
            write: false,
        },
    ];

    let state = tempfile::tempdir().expect("driver state");
    std::fs::set_permissions(state.path(), std::fs::Permissions::from_mode(0o700))
        .expect("harden driver state");
    let provider = DriverProvider::connect(
        manifest_v8(driver, digest(&tool)),
        state.path(),
        &sandbox_helper(),
        &roots,
        false,
    )
    .await
    .expect("Linux v8 runtime-tool Driver Host");

    let capabilities = Provider::capabilities(provider.as_ref())
        .await
        .expect("driver capabilities");
    let probe = capabilities
        .iter()
        .find(|capability| capability.descriptor.name == "driver.fixture.tool_probe")
        .expect("fixture tool capability")
        .descriptor
        .clone();
    let call = |request_id: &str, mode: &str| {
        let provider = provider.clone();
        let probe = probe.clone();
        let context = Context {
            session: "linux-v8-system-config".into(),
            request_id: request_id.into(),
            cancellation: CancellationToken::new(),
        };
        let args = serde_json::json!({"system_config":mode});
        async move { Provider::execute(provider.as_ref(), &context, &probe, &args).await }
    };

    let allowed = call("linux-v8-config-allowed", "allowed")
        .await
        .expect("delegated system config must be readable");
    assert_eq!(allowed["exit_code"], 0);
    assert_eq!(allowed["stdout"], "tool-ok|system-config=delegated-config");

    let denied = call("linux-v8-config-not-delegated", "other")
        .await
        .expect("tool failure is returned as bounded execution output");
    assert_eq!(denied["exit_code"], 12);
    assert_eq!(
        denied["stdout"], "",
        "driver-only system config must not be inherited by the tool child"
    );

    Provider::shutdown(provider.as_ref())
        .await
        .expect("v8 runtime-tool Driver Host shutdown");
}

#[tokio::test]
#[ignore = "requires real Bubblewrap + Landlock support"]
async fn linux_v8_runtime_tool_sessions_are_provider_scoped_and_reaped() {
    assert!(Path::new("/usr/bin/bwrap").is_file());

    let driver_source = PathBuf::from(env!("CARGO_BIN_EXE_semwright-driver-fixture"));
    let tool_source = PathBuf::from(env!("CARGO_BIN_EXE_semwright-tool-fixture"));
    let binary_dir = tempfile::tempdir().expect("fixture directory");
    let driver = binary_dir.path().join("driver");
    let tool = binary_dir.path().join("tool");
    std::fs::copy(&driver_source, &driver).expect("copy driver fixture");
    std::fs::copy(&tool_source, &tool).expect("copy tool fixture");
    harden(&driver);
    harden(&tool);

    let workspace = tempfile::tempdir().expect("session workspace");
    let other_workspace = tempfile::tempdir().expect("unused workspace");
    let configs = tempfile::tempdir().expect("session system config sources");
    let allowed_config = configs.path().join("runtime-config");
    let other_config = configs.path().join("other-config");
    std::fs::write(&allowed_config, b"delegated-config").unwrap();
    std::fs::write(&other_config, b"driver-only-config").unwrap();
    let roots = vec![
        FilesystemGrant {
            name: "fixture-tool-root".into(),
            path: tool.canonicalize().expect("canonical tool"),
            read: true,
            write: false,
        },
        FilesystemGrant {
            name: "tool-workspace".into(),
            path: workspace
                .path()
                .canonicalize()
                .expect("canonical workspace"),
            read: true,
            write: true,
        },
        FilesystemGrant {
            name: "other-workspace".into(),
            path: other_workspace
                .path()
                .canonicalize()
                .expect("canonical unused workspace"),
            read: true,
            write: true,
        },
        FilesystemGrant {
            name: "tool-config-root".into(),
            path: allowed_config
                .canonicalize()
                .expect("canonical session config"),
            read: true,
            write: false,
        },
        FilesystemGrant {
            name: "other-config-root".into(),
            path: other_config
                .canonicalize()
                .expect("canonical other session config"),
            read: true,
            write: false,
        },
    ];

    let state = tempfile::tempdir().expect("driver state");
    std::fs::set_permissions(state.path(), std::fs::Permissions::from_mode(0o700))
        .expect("harden driver state");
    let provider = DriverProvider::connect(
        manifest_v8(driver, digest(&tool)),
        state.path(),
        &sandbox_helper(),
        &roots,
        false,
    )
    .await
    .expect("Linux v8 runtime-tool Driver Host");

    let capabilities = Provider::capabilities(provider.as_ref())
        .await
        .expect("driver capabilities");
    let session_capability = capabilities
        .iter()
        .find(|capability| capability.descriptor.name == "driver.fixture.tool_session")
        .expect("fixture tool-session capability")
        .descriptor
        .clone();

    let run = |session: &str, request_id: &str, args: serde_json::Value| {
        let provider = provider.clone();
        let capability = session_capability.clone();
        let context = Context {
            session: session.into(),
            request_id: request_id.into(),
            cancellation: CancellationToken::new(),
        };
        async move { Provider::execute(provider.as_ref(), &context, &capability, &args).await }
    };

    let started_marker = workspace.path().join("started.marker");
    let finished_marker = workspace.path().join("finished.marker");
    let started = run(
        "session-a",
        "linux-tool-session-start",
        serde_json::json!({
            "action":"start",
            "cwd_mount":"tool-workspace",
            "lifecycle_marker":true,
            "session_config":"read"
        }),
    )
    .await
    .expect("start provider-scoped runtime-tool session");
    let session_id = started["session"].as_str().expect("session id").to_owned();
    assert_eq!(started["state"], "open");
    for _ in 0..40 {
        if started_marker.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    assert!(started_marker.exists(), "persistent tool must start");

    let frame = run(
        "session-b",
        "linux-tool-session-frame",
        serde_json::json!({
            "action":"request",
            "session":session_id,
            "payload":"provider-scope"
        }),
    )
    .await
    .expect("another driver session may use provider-scoped runtime session");
    assert_eq!(frame["state"], "frame");
    assert_eq!(frame["payload"], "delegated-config|provider-scope");

    let forged = run(
        "session-b",
        "linux-tool-session-forged",
        serde_json::json!({
            "action":"request",
            "session":"tool-session-forged",
            "payload":"denied"
        }),
    )
    .await
    .expect_err("forged provider session handle must fail closed");
    assert_eq!(forged.code, ErrorCode::NotFound);

    let closed = run(
        "session-b",
        "linux-tool-session-close",
        serde_json::json!({"action":"close","session":session_id}),
    )
    .await
    .expect("another driver session may close provider-scoped runtime session");
    assert_eq!(closed["state"], "closed");
    assert!(!finished_marker.exists());

    let _ = std::fs::remove_file(&started_marker);
    let second = run(
        "session-c",
        "linux-tool-session-shutdown-start",
        serde_json::json!({
            "action":"start",
            "cwd_mount":"tool-workspace",
            "lifecycle_marker":true
        }),
    )
    .await
    .expect("start persistent tool for provider shutdown");
    assert_eq!(second["state"], "open");
    for _ in 0..40 {
        if started_marker.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    assert!(started_marker.exists(), "shutdown fixture must start");

    Provider::shutdown(provider.as_ref())
        .await
        .expect("v8 runtime-tool Driver Host shutdown");
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    assert!(
        !finished_marker.exists(),
        "provider shutdown must reap persistent runtime-tool session"
    );
}

#[tokio::test]
#[ignore = "requires real Bubblewrap + Landlock support"]
async fn linux_v6_runtime_tool_jobs_are_detached_session_bound_and_cancellable() {
    assert!(Path::new("/usr/bin/bwrap").is_file());

    let driver_source = PathBuf::from(env!("CARGO_BIN_EXE_semwright-driver-fixture"));
    let tool_source = PathBuf::from(env!("CARGO_BIN_EXE_semwright-tool-fixture"));
    let binary_dir = tempfile::tempdir().expect("fixture directory");
    let driver = binary_dir.path().join("driver");
    let tool = binary_dir.path().join("tool");
    std::fs::copy(&driver_source, &driver).expect("copy driver fixture");
    std::fs::copy(&tool_source, &tool).expect("copy tool fixture");
    harden(&driver);
    harden(&tool);

    let workspace = tempfile::tempdir().expect("tool workspace");
    let other_workspace = tempfile::tempdir().expect("unused workspace");
    let roots = vec![
        FilesystemGrant {
            name: "fixture-tool-root".into(),
            path: tool.canonicalize().expect("canonical tool"),
            read: true,
            write: false,
        },
        FilesystemGrant {
            name: "tool-workspace".into(),
            path: workspace
                .path()
                .canonicalize()
                .expect("canonical tool workspace"),
            read: true,
            write: true,
        },
        FilesystemGrant {
            name: "other-workspace".into(),
            path: other_workspace
                .path()
                .canonicalize()
                .expect("canonical unused workspace"),
            read: true,
            write: true,
        },
    ];

    let state = tempfile::tempdir().expect("driver state");
    std::fs::set_permissions(state.path(), std::fs::Permissions::from_mode(0o700))
        .expect("harden driver state");
    let provider = DriverProvider::connect(
        manifest(driver, digest(&tool), 6),
        state.path(),
        &sandbox_helper(),
        &roots,
        false,
    )
    .await
    .expect("Linux v6 runtime-tool Driver Host");

    let capabilities = Provider::capabilities(provider.as_ref())
        .await
        .expect("driver capabilities");
    let job_capability = capabilities
        .iter()
        .find(|capability| capability.descriptor.name == "driver.fixture.tool_job")
        .expect("fixture tool-job capability")
        .descriptor
        .clone();

    let run = |session: &str, request_id: &str, args: serde_json::Value| {
        let provider = provider.clone();
        let capability = job_capability.clone();
        let context = Context {
            session: session.into(),
            request_id: request_id.into(),
            cancellation: CancellationToken::new(),
        };
        async move { Provider::execute(provider.as_ref(), &context, &capability, &args).await }
    };

    let started = run(
        "session-a",
        "linux-tool-job-start",
        serde_json::json!({"action":"start","sleep_ms":250}),
    )
    .await
    .expect("start detached runtime-tool job");
    let job = started["job"].as_str().expect("job id").to_owned();
    assert_eq!(started["state"], "running");

    let cross_session = run(
        "session-b",
        "linux-tool-job-cross-session",
        serde_json::json!({"action":"status","job":job}),
    )
    .await
    .expect_err("another driver session must not observe the detached job");
    assert_eq!(cross_session.code, ErrorCode::PolicyDenied);

    let mut succeeded = None;
    for attempt in 0..40u32 {
        let status = run(
            "session-a",
            &format!("linux-tool-job-status-{attempt}"),
            serde_json::json!({"action":"status","job":job}),
        )
        .await
        .expect("poll detached runtime-tool job");
        match status["state"].as_str() {
            Some("succeeded") => {
                succeeded = Some(status);
                break;
            }
            Some("running" | "cancelling") => {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            other => panic!("unexpected detached job state: {other:?}"),
        }
    }
    let succeeded = succeeded.expect("detached job should reach success");
    assert_eq!(succeeded["exit_code"], 0);
    assert_eq!(succeeded["stdout"], "tool-ok");

    let collected = run(
        "session-a",
        "linux-tool-job-collected",
        serde_json::json!({"action":"status","job":job}),
    )
    .await
    .expect_err("terminal job result is collected exactly once");
    assert_eq!(collected.code, ErrorCode::NotFound);

    let started = run(
        "session-a",
        "linux-tool-job-cancel-start",
        serde_json::json!({"action":"start","sleep_ms":5000}),
    )
    .await
    .expect("start cancellable runtime-tool job");
    let cancel_job = started["job"].as_str().expect("cancel job id").to_owned();
    let cancelled = run(
        "session-a",
        "linux-tool-job-cancel",
        serde_json::json!({"action":"cancel","job":cancel_job}),
    )
    .await
    .expect("request detached job cancellation");
    assert!(matches!(
        cancelled["state"].as_str(),
        Some("cancelling" | "cancelled")
    ));

    let mut terminal_cancelled = false;
    for attempt in 0..40u32 {
        let status = run(
            "session-a",
            &format!("linux-tool-job-cancel-status-{attempt}"),
            serde_json::json!({"action":"status","job":cancel_job}),
        )
        .await
        .expect("poll cancelled detached runtime-tool job");
        match status["state"].as_str() {
            Some("cancelled") => {
                terminal_cancelled = true;
                break;
            }
            Some("running" | "cancelling") => {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            other => panic!("unexpected cancelled job state: {other:?}"),
        }
    }
    assert!(terminal_cancelled, "detached job should become cancelled");

    let started_marker = workspace.path().join("started.marker");
    let finished_marker = workspace.path().join("finished.marker");
    let _shutdown_job = run(
        "session-a",
        "linux-tool-job-shutdown-start",
        serde_json::json!({
            "action":"start",
            "sleep_ms":1000,
            "cwd_mount":"tool-workspace",
            "lifecycle_marker":true
        }),
    )
    .await
    .expect("start job that must be killed by provider shutdown");
    for _ in 0..40 {
        if started_marker.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    assert!(
        started_marker.exists(),
        "detached tool must start before provider shutdown"
    );

    Provider::shutdown(provider.as_ref())
        .await
        .expect("runtime-tool job Driver Host shutdown");
    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
    assert!(
        !finished_marker.exists(),
        "provider shutdown must reap a detached runtime-tool child"
    );
}
