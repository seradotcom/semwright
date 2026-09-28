#![cfg(target_os = "linux")]
use semwright_backend_api::{Context, Provider};
use semwright_core::{Broker, NoApprover, audit::Audit};
use semwright_driver_host::DriverProvider;
use semwright_driver_sdk::{
    ApplicationMatch, DriverInterfaces, DriverMount, DriverResources, Manifest, SystemConfigMount,
    Transport,
};
use semwright_policy::{FilesystemGrant, Policy, PolicyConfig};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio_util::sync::CancellationToken;

fn digest(path: &Path) -> String {
    format!("{:x}", Sha256::digest(std::fs::read(path).unwrap()))
}
fn find<'a>(
    caps: &'a [semwright_backend_api::ProvidedCapability],
    name: &str,
) -> &'a semwright_backend_api::ProvidedCapability {
    caps.iter()
        .find(|cap| cap.descriptor.name == name)
        .unwrap_or_else(|| panic!("missing capability {name}"))
}
async fn call(
    provider: &DriverProvider,
    caps: &[semwright_backend_api::ProvidedCapability],
    name: &str,
    args: Value,
) -> semwright_types::Result<Value> {
    Provider::execute(
        provider,
        &Context {
            session: "motion-canvas-live".into(),
            request_id: semwright_types::unique_id(),
            cancellation: CancellationToken::new(),
        },
        &find(caps, name).descriptor,
        &args,
    )
    .await
}

const BROKER_SESSION: &str = "motion-composition-native";
async fn broker_call(broker: &Arc<Broker>, name: &str, args: Value) -> Value {
    let envelope = broker
        .clone()
        .execute(
            BROKER_SESSION.into(),
            semwright_types::unique_id(),
            semwright_types::ExecuteRequest {
                command: name.into(),
                args,
                dry_run: false,
                backend: None,
            },
            CancellationToken::new(),
        )
        .await;
    assert!(envelope.ok, "Broker call {name} failed: {envelope:#?}");
    envelope.data.expect("successful Broker envelope has data")
}
fn fixture() -> Vec<u8> {
    include_bytes!("../../../fixtures/motion-canvas/hello-text/semwright-motion.json").to_vec()
}
fn grant(name: &str, path: &Path, write: bool) -> FilesystemGrant {
    FilesystemGrant {
        name: name.into(),
        path: path.canonicalize().unwrap(),
        read: true,
        write,
    }
}
fn manifest(executable: PathBuf, with_runtime: bool) -> Manifest {
    let mut mounts = vec![
        DriverMount {
            root: "project".into(),
            read_only: false,
            execute: false,
        },
        DriverMount {
            root: "output".into(),
            read_only: false,
            execute: false,
        },
    ];
    if with_runtime {
        mounts.push(DriverMount {
            root: "runtime".into(),
            read_only: true,
            execute: true,
        });
    }
    Manifest {
        manifest_version: 1,
        protocol: 3,
        id: "motion-canvas".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        publisher: "semwright-tests".into(),
        executable: executable.clone(),
        sha256: digest(&executable),
        application: ApplicationMatch {
            desktop_id: None,
            process_names: vec!["node".into()],
            supported_versions: vec!["3.17.2".into()],
        },
        transport: Transport::StdioV1,
        mounts,
        system_config: if with_runtime {
            vec![SystemConfigMount {
                root: "fontconfig".into(),
                destination: "/etc/fonts".into(),
            }]
        } else {
            vec![]
        },
        secrets: vec![],
        tools: vec![],
        network: false,
        loopback_port: None,
        resources: DriverResources {
            open_files: 512,
            processes: 256,
            cpu_seconds: 300,
            operation_cpu_seconds: 0,
            address_space_bytes: 4_294_967_296,
            file_size_bytes: 1_073_741_824,
        },
        request_timeout_ms: 300_000,
        interfaces: DriverInterfaces {
            cooperative_cancellation: true,
            progress: true,
            artifacts: true,
            health: true,
            ..DriverInterfaces::default()
        },
    }
}
struct Harness {
    _binary: tempfile::TempDir,
    project: tempfile::TempDir,
    output: tempfile::TempDir,
    state: tempfile::TempDir,
    executable: PathBuf,
    helper: PathBuf,
}
impl Harness {
    fn new() -> Self {
        let binary = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        for dir in [binary.path(), project.path(), output.path(), state.path()] {
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let mut semantic: Value = serde_json::from_slice(&fixture()).unwrap();
        semantic["scenes"][0]["duration_ms"] = json!(10_000);
        std::fs::write(
            project.path().join("semwright-motion.json"),
            serde_json::to_vec_pretty(&semantic).unwrap(),
        )
        .unwrap();
        let executable = binary.path().join("semwright-motion-canvas-driver");
        std::fs::copy(
            PathBuf::from(env!("CARGO_BIN_EXE_semwright-motion-canvas-driver")),
            &executable,
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let helper = PathBuf::from(
            std::env::var_os("SEMWRIGHT_TEST_SANDBOX_HELPER").expect("sandbox helper env"),
        );
        Self {
            _binary: binary,
            project,
            output,
            state,
            executable,
            helper,
        }
    }
}

#[tokio::test]
#[ignore = "requires pinned Node/Firefox Motion Canvas runtime plus production Driver Host"]
async fn real_motion_canvas_render_runs_inside_sandbox() {
    if std::env::var_os("SEMWRIGHT_TEST_MOTION_CANVAS_RENDER").is_none() {
        return;
    }
    let h = Harness::new();
    let runtime =
        PathBuf::from(std::env::var_os("SEMWRIGHT_TEST_MOTION_RUNTIME").expect("runtime root env"));
    let grants = vec![
        grant("project", h.project.path(), true),
        grant("output", h.output.path(), true),
        grant("runtime", &runtime, false),
        grant("fontconfig", Path::new("/etc/fonts"), false),
    ];
    let provider = DriverProvider::connect(
        manifest(h.executable.clone(), true),
        h.state.path(),
        &h.helper,
        &grants,
        false,
    )
    .await
    .unwrap();
    let caps = Provider::capabilities(provider.as_ref()).await.unwrap();
    let doctor = call(
        provider.as_ref(),
        &caps,
        "driver.motion-canvas.doctor",
        json!({}),
    )
    .await
    .unwrap();
    assert_eq!(doctor["render_available"], true, "doctor: {doctor:#}");
    let inspected = call(
        provider.as_ref(),
        &caps,
        "driver.motion-canvas.project.inspect",
        json!({}),
    )
    .await
    .unwrap();
    let fingerprint = inspected["fingerprint"].as_str().unwrap().to_owned();
    let started = call(provider.as_ref(), &caps, "driver.motion-canvas.render.start", json!({"expected_fingerprint":fingerprint,"profile":{"first_frame":0,"end_frame_exclusive":30,"scale":"full","transparent":false,"timeout_ms":120000}})).await.unwrap();
    let job = started["job_ref"].as_str().unwrap().to_owned();
    let terminal = loop {
        let status = call(
            provider.as_ref(),
            &caps,
            "driver.motion-canvas.render.status",
            json!({"job_ref":job}),
        )
        .await
        .unwrap();
        match status["state"].as_str().unwrap() {
            "succeeded" | "failed" | "cancelled" => break status,
            _ => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    };
    assert_eq!(terminal["state"], "succeeded", "terminal: {terminal:#}");
    assert_eq!(terminal["artifact"]["frame_count"], 30);
    let result = call(
        provider.as_ref(),
        &caps,
        "driver.motion-canvas.render.result",
        json!({"job_ref":job}),
    )
    .await
    .unwrap();
    assert_eq!(result["state"], "succeeded");
    let artifact_dir = h
        .output
        .path()
        .join(result["artifact"]["directory"].as_str().unwrap());
    assert!(artifact_dir.join("artifact-manifest.json").is_file());
    let first = artifact_dir.join("frames/000000.png");
    let evidence = std::env::var_os("SEMWRIGHT_TEST_MOTION_EVIDENCE").map(PathBuf::from);
    if let Some(evidence) = &evidence {
        std::fs::create_dir_all(evidence).unwrap();
        std::fs::copy(
            artifact_dir.join("artifact-manifest.json"),
            evidence.join("opaque-manifest.json"),
        )
        .unwrap();
        std::fs::copy(&first, evidence.join("opaque-first.png")).unwrap();
    }

    let alpha_terminal = call(
        provider.as_ref(),
        &caps,
        "driver.motion-canvas.render.execute",
        json!({"expected_fingerprint":fingerprint,"profile":{"first_frame":0,"end_frame_exclusive":2,"scale":"full","transparent":true,"timeout_ms":120000}}),
    )
    .await
    .unwrap();
    assert_eq!(
        alpha_terminal["state"], "succeeded",
        "protocol-v3 alpha render: {alpha_terminal:#}"
    );
    assert_eq!(alpha_terminal["artifact"]["frame_count"], 2);
    let alpha_dir = h
        .output
        .path()
        .join(alpha_terminal["artifact"]["directory"].as_str().unwrap());
    let alpha_first = alpha_dir.join("frames/000000.png");
    let png = semwright_driver_motion_canvas::security::inspect_png(
        &std::fs::read(&alpha_first).unwrap(),
    )
    .unwrap();
    assert!(
        png.min_alpha < 255,
        "transparent render had no alpha: {png:?}"
    );
    if let Some(evidence) = &evidence {
        std::fs::copy(
            alpha_dir.join("artifact-manifest.json"),
            evidence.join("alpha-manifest.json"),
        )
        .unwrap();
        std::fs::copy(&alpha_first, evidence.join("alpha-first.png")).unwrap();
    }

    let started = call(provider.as_ref(), &caps, "driver.motion-canvas.render.start", json!({"expected_fingerprint":fingerprint,"profile":{"first_frame":0,"end_frame_exclusive":300,"scale":"full","transparent":false,"timeout_ms":120000}})).await.unwrap();
    let cancel_job = started["job_ref"].as_str().unwrap().to_owned();
    call(
        provider.as_ref(),
        &caps,
        "driver.motion-canvas.render.cancel",
        json!({"job_ref":cancel_job}),
    )
    .await
    .unwrap();
    let cancelled = loop {
        let status = call(
            provider.as_ref(),
            &caps,
            "driver.motion-canvas.render.status",
            json!({"job_ref":cancel_job}),
        )
        .await
        .unwrap();
        match status["state"].as_str().unwrap() {
            "succeeded" | "failed" | "cancelled" => break status,
            _ => tokio::time::sleep(Duration::from_millis(50)).await,
        }
    };
    assert_eq!(cancelled["state"], "cancelled", "cancelled: {cancelled:#}");

    let request_cancel = CancellationToken::new();
    let request_context = Context {
        session: "motion-canvas-live-v3-cancel".into(),
        request_id: semwright_types::unique_id(),
        cancellation: request_cancel.clone(),
    };
    let execute_descriptor = find(&caps, "driver.motion-canvas.render.execute")
        .descriptor
        .clone();
    let execute_args = json!({"expected_fingerprint":fingerprint,"profile":{"first_frame":0,"end_frame_exclusive":300,"scale":"full","transparent":false,"timeout_ms":120000}});
    let provider_for_cancel = provider.clone();
    let execute_task = tokio::spawn(async move {
        Provider::execute(
            provider_for_cancel.as_ref(),
            &request_context,
            &execute_descriptor,
            &execute_args,
        )
        .await
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    request_cancel.cancel();
    let error = execute_task.await.unwrap().unwrap_err();
    assert_eq!(error.code, semwright_types::ErrorCode::Cancelled);
    let still_healthy = call(
        provider.as_ref(),
        &caps,
        "driver.motion-canvas.doctor",
        json!({}),
    )
    .await
    .unwrap();
    assert_eq!(still_healthy["render_available"], true);
    assert_eq!(still_healthy["network"], false);
    assert_eq!(still_healthy["capability_count"], caps.len());

    Provider::shutdown(provider.as_ref()).await.unwrap();
}

#[tokio::test]
#[ignore = "requires pinned Motion Canvas runtime plus production Broker and Driver Host"]
async fn composition_authoring_runs_through_broker_driver_host_and_native_renderer() {
    if std::env::var_os("SEMWRIGHT_TEST_MOTION_CANVAS_RENDER").is_none() {
        return;
    }
    let h = Harness::new();
    std::fs::remove_file(h.project.path().join("semwright-motion.json")).unwrap();
    let runtime =
        PathBuf::from(std::env::var_os("SEMWRIGHT_TEST_MOTION_RUNTIME").expect("runtime root env"));
    let grants = vec![
        grant("project", h.project.path(), true),
        grant("output", h.output.path(), true),
        grant("runtime", &runtime, false),
        grant("fontconfig", Path::new("/etc/fonts"), false),
    ];
    let provider = DriverProvider::connect(
        manifest(h.executable.clone(), true),
        h.state.path(),
        &h.helper,
        &grants,
        false,
    )
    .await
    .unwrap();

    let audit = Audit::open(&h.state.path().join("broker-audit"), 65_536, 2).unwrap();
    let policy = Policy::new(PolicyConfig {
        allow: [provider.identity().id.clone()].into(),
        ..Default::default()
    })
    .unwrap();
    let broker = Broker::new(
        policy,
        vec![],
        audit,
        Arc::new(NoApprover),
        None,
        json!({}),
        false,
    )
    .unwrap();
    broker.mount_provider(provider.clone()).await.unwrap();

    let empty = broker_call(
        &broker,
        "driver.motion-canvas.composition.inspect",
        json!({}),
    )
    .await;
    assert_eq!(empty["fingerprint"], Value::Null);
    assert_eq!(empty["film"], Value::Null);
    assert_eq!(empty["low_level_project"], false);

    let mut film: Value = serde_json::from_slice(include_bytes!(
        "../../../fixtures/composition/motion/technical.json"
    ))
    .unwrap();
    // Native pipeline proof is intentionally taste-neutral: keep lifecycle/cue/
    // transition checks and omit the fixture's optional geometry rule.
    film["sequences"][0]["beats"][0]["shots"][0]["constraints"] = json!([]);

    let planned = broker_call(
        &broker,
        "driver.motion-canvas.composition.plan",
        json!({
            "film": film,
            "budget": {
                "max_iterations": 4,
                "max_operations": 64,
                "max_findings": 64,
                "max_observations": 8,
                "max_elapsed_ms": 120000
            }
        }),
    )
    .await;
    let plan_ref = planned["plan_ref"]
        .as_str()
        .expect("server-issued plan ref")
        .to_owned();
    assert_eq!(planned["repair"], false);

    let applied = broker_call(
        &broker,
        "driver.motion-canvas.composition.apply",
        json!({"plan_ref": plan_ref, "dry_run": false}),
    )
    .await;
    assert_eq!(applied["applied"], true);
    assert_eq!(applied["execution_status"], "completed");
    let fingerprint = applied["fingerprint"]
        .as_str()
        .expect("applied source fingerprint")
        .to_owned();

    let inspected = broker_call(
        &broker,
        "driver.motion-canvas.composition.inspect",
        json!({}),
    )
    .await;
    assert_eq!(inspected["fingerprint"], fingerprint);
    assert_eq!(inspected["low_level_project"], false);
    assert_eq!(inspected["film"]["id"], "technical-motion");

    let started = broker_call(
        &broker,
        "driver.motion-canvas.render.start",
        json!({
            "expected_fingerprint": fingerprint,
            "profile": {
                "first_frame": 0,
                "end_frame_exclusive": 90,
                "scale": "full",
                "transparent": false,
                "timeout_ms": 120000
            }
        }),
    )
    .await;
    let job_ref = started["job_ref"]
        .as_str()
        .expect("render job ref")
        .to_owned();

    let terminal = loop {
        let status = broker_call(
            &broker,
            "driver.motion-canvas.render.status",
            json!({"job_ref": job_ref}),
        )
        .await;
        match status["state"].as_str().expect("render state") {
            "succeeded" | "failed" | "cancelled" => break status,
            _ => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    };
    assert_eq!(terminal["state"], "succeeded", "terminal: {terminal:#}");
    assert_eq!(terminal["artifact"]["frame_count"], 90);

    let rendered = broker_call(
        &broker,
        "driver.motion-canvas.render.result",
        json!({"job_ref": job_ref}),
    )
    .await;
    assert_eq!(rendered["state"], "succeeded");

    let verified = broker_call(
        &broker,
        "driver.motion-canvas.composition.verify",
        json!({"plan_ref": plan_ref, "job_ref": job_ref}),
    )
    .await;
    assert_eq!(verified["report"]["execution_status"], "completed");
    assert_eq!(verified["report"]["support_level"], "native");
    assert!(
        verified["measurement"]["findings"]
            .as_array()
            .expect("findings")
            .is_empty(),
        "native authoring findings: {verified:#}"
    );
    let checks = verified["measurement"]["validation"]["checks"]
        .as_array()
        .expect("verification checks");
    assert!(!checks.is_empty());
    assert!(
        checks
            .iter()
            .all(|check| check["verdict"].as_str() == Some("PASS")),
        "native verification did not fully pass: {verified:#}"
    );

    if let Some(evidence) = std::env::var_os("SEMWRIGHT_TEST_MOTION_EVIDENCE").map(PathBuf::from) {
        std::fs::create_dir_all(&evidence).unwrap();
        std::fs::write(
            evidence.join("composition-broker-native.json"),
            serde_json::to_vec_pretty(&json!({
                "schema_version": 1,
                "source_sha": option_env!("GITHUB_SHA"),
                "broker_session": BROKER_SESSION,
                "plan_ref": plan_ref,
                "source_fingerprint": fingerprint,
                "render": rendered,
                "verification": verified,
                "claim_scope": "Broker -> Driver Host -> Motion Canvas native render observations"
            }))
            .unwrap(),
        )
        .unwrap();
    }

    broker.shutdown().await;
}
