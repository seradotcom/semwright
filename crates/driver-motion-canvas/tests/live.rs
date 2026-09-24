#![cfg(target_os = "linux")]
use semwright_backend_api::{Context, Provider};
use semwright_driver_host::DriverProvider;
use semwright_driver_sdk::{
    ApplicationMatch, DriverInterfaces, DriverMount, DriverResources, Manifest, Transport,
};
use semwright_policy::FilesystemGrant;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
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
            cancellation: CancellationToken::new(),
        },
        &find(caps, name).descriptor,
        &args,
    )
    .await
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
        },
        DriverMount {
            root: "output".into(),
            read_only: false,
        },
    ];
    if with_runtime {
        mounts.push(DriverMount {
            root: "runtime".into(),
            read_only: true,
        });
    }
    Manifest {
        manifest_version: 1,
        protocol: 1,
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
        system_config: vec![],
        network: false,
        resources: DriverResources {
            open_files: 512,
            processes: 128,
            cpu_seconds: 300,
            address_space_bytes: 4_294_967_296,
            file_size_bytes: 1_073_741_824,
        },
        request_timeout_ms: 300_000,
        interfaces: DriverInterfaces::default(),
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

    let alpha_started = call(provider.as_ref(), &caps, "driver.motion-canvas.render.start", json!({"expected_fingerprint":fingerprint,"profile":{"first_frame":0,"end_frame_exclusive":2,"scale":"full","transparent":true,"timeout_ms":120000}})).await.unwrap();
    let alpha_job = alpha_started["job_ref"].as_str().unwrap().to_owned();
    let alpha_terminal = loop {
        let status = call(
            provider.as_ref(),
            &caps,
            "driver.motion-canvas.render.status",
            json!({"job_ref":alpha_job}),
        )
        .await
        .unwrap();
        match status["state"].as_str().unwrap() {
            "succeeded" | "failed" | "cancelled" => break status,
            _ => tokio::time::sleep(Duration::from_millis(50)).await,
        }
    };
    assert_eq!(
        alpha_terminal["state"], "succeeded",
        "alpha: {alpha_terminal:#}"
    );
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
    Provider::shutdown(provider.as_ref()).await.unwrap();
}
