#![cfg(target_os = "linux")]
use semwright_backend_api::{Context, Provider};
use semwright_core::{Broker, NoApprover, audit::Audit};
use semwright_driver_host::DriverProvider;
use semwright_driver_sdk::{
    ApplicationMatch, DriverInterfaces, DriverMount, DriverResources, DriverToolMount, Manifest,
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
    hex::encode(Sha256::digest(std::fs::read(path).unwrap()))
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
fn manifest(executable: PathBuf, node_sha256: String) -> Manifest {
    Manifest {
        manifest_version: 1,
        protocol: 7,
        id: "motion-canvas".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        publisher: "semwright-tests".into(),
        executable: executable.clone(),
        sha256: digest(&executable),
        application: ApplicationMatch {
            desktop_id: None,
            process_names: vec!["node".into()],
            supported_versions: vec!["3.17.2".into(), "Node 22.22.0".into()],
        },
        transport: Transport::StdioV1,
        mounts: vec![
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
            DriverMount {
                root: "runtime".into(),
                read_only: true,
                execute: true,
            },
            DriverMount {
                root: "fontconfig".into(),
                read_only: true,
                execute: false,
            },
        ],
        system_config: vec![],
        secrets: vec![],
        tools: vec![DriverToolMount {
            root: "motion-node-tool".into(),
            name: "motion-node".into(),
            sha256: node_sha256,
            mounts: vec![
                "project".into(),
                "output".into(),
                "runtime".into(),
                "fontconfig".into(),
            ],
            system_config: vec![],
            dependencies: vec![],
            nvidia_gpu: false,
            resources: None,
        }],
        network: false,
        nvidia_gpu: false,
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
            host_tools: true,
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
    let node =
        PathBuf::from(std::env::var_os("SEMWRIGHT_TEST_MOTION_NODE").expect("Node tool env"));
    let grants = vec![
        grant("project", h.project.path(), true),
        grant("output", h.output.path(), true),
        grant("runtime", &runtime, false),
        grant("fontconfig", Path::new("/etc/fonts"), false),
        grant("motion-node-tool", &node, false),
    ];
    let provider = DriverProvider::connect(
        manifest(h.executable.clone(), digest(&node)),
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
        "protocol-v7 alpha render: {alpha_terminal:#}"
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
    let node =
        PathBuf::from(std::env::var_os("SEMWRIGHT_TEST_MOTION_NODE").expect("Node tool env"));
    let runtime =
        PathBuf::from(std::env::var_os("SEMWRIGHT_TEST_MOTION_RUNTIME").expect("runtime root env"));
    let grants = vec![
        grant("motion-node-tool", &node, false),
        grant("project", h.project.path(), true),
        grant("output", h.output.path(), true),
        grant("runtime", &runtime, false),
        grant("fontconfig", Path::new("/etc/fonts"), false),
    ];
    let provider = DriverProvider::connect(
        manifest(h.executable.clone(), digest(&node)),
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
    // Keep a geometry rule in the real native pipeline. The nested fixed group
    // mirrors production authoring where a semantic container owns text and both
    // must be observable on the first and final in-range frames.
    {
        let shot = &mut film["sequences"][0]["beats"][0]["shots"][0];
        let subjects = shot["subjects"].as_array_mut().expect("fixture subjects");
        let label = subjects
            .iter_mut()
            .find(|subject| subject["id"] == "label")
            .expect("fixture label");
        label["parent"] = json!("nested_group");
        label["layout"]["position"] = json!({"x":0.0,"y":0.0});
        label["layout"]["size"] = json!({"width":200.0,"height":80.0});
        subjects.insert(
            0,
            json!({
                "id":"nested_group",
                "role":"primary",
                "parent":null,
                "layer":"content",
                "content":{"kind":"group"},
                "layout":{"kind":"fixed","position":{"x":0.0,"y":0.0},"size":{"width":240.0,"height":180.0}},
                "initially_visible":true,
                "clip_intentional":false
            }),
        );
        shot["constraints"] = json!([
            {"rule":"safe_area","subject":"nested_group","tolerance":0.0},
            {"rule":"safe_area","subject":"label","tolerance":0.0}
        ]);
    }
    // Exercise the exact half-open terminal boundary used by Motionwright:
    // two seconds at 30 fps must keep authored subjects observable through frame 59.
    film["timing"]["duration"] = json!({"num":"2","den":"1"});
    for span in film["timing"]["spans"]
        .as_array_mut()
        .expect("fixture timing spans")
    {
        if span["id"] == "sequence-span" || span["id"] == "shot-span" {
            span["minimum"] = json!({"num":"2","den":"1"});
            span["preferred"] = json!({"num":"2","den":"1"});
            span["maximum"] = json!({"num":"2","den":"1"});
        }
    }
    film["output"]["width"] = json!(1920);
    film["output"]["height"] = json!(1080);
    film["output"]["aspect"] = json!("landscape");

    let planned = broker_call(
        &broker,
        "driver.motion-canvas.composition.plan",
        json!({
            "film": film.clone(),
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
                "end_frame_exclusive": 60,
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
    assert_eq!(terminal["artifact"]["frame_count"], 60);

    let rendered = broker_call(
        &broker,
        "driver.motion-canvas.render.result",
        json!({"job_ref": job_ref}),
    )
    .await;
    assert_eq!(rendered["state"], "succeeded");
    let first_manifest: Value = serde_json::from_slice(
        &std::fs::read(
            h.output
                .path()
                .join(rendered["artifact"]["directory"].as_str().unwrap())
                .join("artifact-manifest.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(first_manifest["plan"]["width"], 1920);
    assert_eq!(first_manifest["plan"]["height"], 1080);

    let verified = broker_call(
        &broker,
        "driver.motion-canvas.composition.verify",
        json!({"plan_ref": plan_ref, "job_ref": job_ref}),
    )
    .await;
    assert_eq!(verified["report"]["execution_status"], "completed");
    assert_eq!(verified["report"]["support_level"], "native");
    assert_eq!(
        verified["measurement"]["coverage"]["font_resources_digest"]
            .as_str()
            .expect("native font resource digest")
            .len(),
        64
    );
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

    // Exercise genuine pinned native caption timing using the same already
    // authenticated, disposable owner Broker and Driver Host. A failed
    // half-open cue must never pass as an observed native caption.
    assert_source_bound_native_caption_frames(&broker, film.clone(), h.output.path()).await;

    let expected_subject_ids = film["sequences"][0]["beats"][0]["shots"][0]["subjects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|subject| subject["id"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let mut reflow_evidence = vec![json!({
        "aspect":"landscape",
        "width":1920,
        "height":1080,
        "source_fingerprint":fingerprint.clone(),
        "render":rendered.clone(),
        "verification":verified.clone()
    })];

    for (aspect, width, height) in [("portrait", 360u64, 640u64), ("square", 480, 480)] {
        film["output"]["width"] = json!(width);
        film["output"]["height"] = json!(height);
        film["output"]["aspect"] = json!(aspect);
        let replan = broker_call(
            &broker,
            "driver.motion-canvas.composition.plan",
            json!({
                "film": film.clone(),
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
        let replan_ref = replan["plan_ref"].as_str().unwrap().to_owned();
        let reapplied = broker_call(
            &broker,
            "driver.motion-canvas.composition.apply",
            json!({"plan_ref":replan_ref,"dry_run":false}),
        )
        .await;
        assert_eq!(reapplied["execution_status"], "completed");
        let reflow_fingerprint = reapplied["fingerprint"].as_str().unwrap().to_owned();

        let reinspection = broker_call(
            &broker,
            "driver.motion-canvas.composition.inspect",
            json!({}),
        )
        .await;
        let observed_ids = reinspection["film"]["sequences"][0]["beats"][0]["shots"][0]["subjects"]
            .as_array()
            .unwrap()
            .iter()
            .map(|subject| subject["id"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            observed_ids, expected_subject_ids,
            "logical identity changed during reflow"
        );
        assert_eq!(reinspection["film"]["cues"], film["cues"]);

        let reflow_started = broker_call(
            &broker,
            "driver.motion-canvas.render.start",
            json!({
                "expected_fingerprint":reflow_fingerprint,
                "profile":{
                    "first_frame":0,
                    "end_frame_exclusive":60,
                    "scale":"full",
                    "transparent":false,
                    "timeout_ms":120000
                }
            }),
        )
        .await;
        let reflow_job = reflow_started["job_ref"].as_str().unwrap().to_owned();
        let reflow_terminal = loop {
            let status = broker_call(
                &broker,
                "driver.motion-canvas.render.status",
                json!({"job_ref":reflow_job}),
            )
            .await;
            match status["state"].as_str().unwrap() {
                "succeeded" | "failed" | "cancelled" => break status,
                _ => tokio::time::sleep(Duration::from_millis(100)).await,
            }
        };
        assert_eq!(
            reflow_terminal["state"], "succeeded",
            "reflow render: {reflow_terminal:#}"
        );
        let reflow_rendered = broker_call(
            &broker,
            "driver.motion-canvas.render.result",
            json!({"job_ref":reflow_job}),
        )
        .await;
        let manifest: Value = serde_json::from_slice(
            &std::fs::read(
                h.output
                    .path()
                    .join(reflow_rendered["artifact"]["directory"].as_str().unwrap())
                    .join("artifact-manifest.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(manifest["plan"]["width"], width);
        assert_eq!(manifest["plan"]["height"], height);
        let reflow_verified = broker_call(
            &broker,
            "driver.motion-canvas.composition.verify",
            json!({"plan_ref":replan_ref,"job_ref":reflow_job}),
        )
        .await;
        assert!(
            reflow_verified["measurement"]["validation"]["checks"]
                .as_array()
                .unwrap()
                .iter()
                .all(|check| check["verdict"].as_str() == Some("PASS")),
            "native reflow verification failed: {reflow_verified:#}"
        );
        reflow_evidence.push(json!({
            "aspect":aspect,
            "width":width,
            "height":height,
            "source_fingerprint":reflow_fingerprint,
            "render":reflow_rendered,
            "verification":reflow_verified
        }));
    }

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
                "reflows": reflow_evidence,
                "claim_scope": "Broker -> Driver Host -> Motion Canvas native render observations; native 16:9/9:16/1:1 reflow"
            }))
            .unwrap(),
        )
        .unwrap();
    }

    broker.shutdown().await;
}

/// A second explicitly authored native project revision in the existing
/// disposable Broker session, never a retry of the original baseline render.
/// These two synthetic text cues mirror Motionwright's exact 30 fps bounds,
/// without importing any user media or alternate renderer.
async fn assert_source_bound_native_caption_frames(
    broker: &Arc<Broker>,
    mut film: Value,
    private_owner_output: &Path,
) {
    let shot = &mut film["sequences"][0]["beats"][0]["shots"][0];
    shot["layers"].as_array_mut().unwrap().push(json!({
        "id":"caption-overlay","order":1,"intentional_overlay":true
    }));
    film["editorial"]["type_scale"]["mw-caption"] = json!(44.0);

    let source =
        semwright_semantic_composition::Digest::of_bytes(b"CI-only-synthetic-caption-source");
    for (id, start, duration, label) in [
        ("a", ("1", "2"), ("1", "2"), "REAL NATIVE CAPTION PROOF"),
        ("b", ("6", "5"), ("2", "5"), "SECOND SOURCE-BOUND CUE"),
    ] {
        let wrapper = format!("native-caption-wrapper-{id}");
        let text_id = format!("native-caption-text-{id}");
        let cue_id = format!("native-caption-cue-{id}");
        let shot = &mut film["sequences"][0]["beats"][0]["shots"][0];
        shot["subjects"].as_array_mut().unwrap().push(json!({
            "id":wrapper,"role":"caption-overlay-root","parent":null,
            "layer":"caption-overlay","content":{"kind":"group"},
            "layout":{"kind":"fixed","position":{"x":0.0,"y":350.0},
                      "size":{"width":1400.0,"height":160.0}},
            "initially_visible":true,"clip_intentional":false
        }));
        shot["subjects"].as_array_mut().unwrap().push(json!({
            "id":text_id,"role":"caption","parent":wrapper,
            "layer":"caption-overlay",
            "content":{"kind":"text","runs":[{"text":label,"weight":600,
                      "color":"#223344","emphasis":false}],
                       "style":"mw-caption","direction":"auto","language":"en",
                       "wrap":true,"truncate":false},
            "layout":{"kind":"fixed","position":{"x":0.0,"y":0.0},
                      "size":{"width":1400.0,"height":160.0}},
            "initially_visible":false,"clip_intentional":false
        }));
        shot["captions"].as_array_mut().unwrap().push(json!({
            "id":format!("native-caption-{id}"),"subject":text_id,
            "cue_id":cue_id,"language":"en","text":label
        }));
        film["cues"]["cues"].as_array_mut().unwrap().push(json!({
            "id":cue_id,
            "anchor":{"kind":"absolute","time":{"num":start.0,"den":start.1}},
            "duration":{"num":duration.0,"den":duration.1},
            "source":source,"method":"manual-timing",
            "version":1,"confidence":null
        }));
    }

    // Real authoritative Film parser checks native object grammar and exact
    // temporal cue boundaries before any Broker-backed mutating apply.
    let typed: semwright_motion_authoring::Film =
        semwright_semantic_composition::strict_decode(&serde_json::to_vec(&film).unwrap())
            .expect("native caption fixture must be canonical typed Film");
    typed.validate().expect("native source fixture validation");
    let resolved = semwright_motion_authoring::realize(&typed)
        .expect("native caption source schedule realization");
    assert_eq!(resolved.schedule.frame_count(&typed.output).unwrap(), 60);
    assert_eq!(typed.cues.cues.len(), 2);

    let planned = broker_call(
        broker,
        "driver.motion-canvas.composition.plan",
        json!({
            "film":film,
            "budget":{"max_iterations":4,"max_operations":128,
                      "max_findings":128,"max_observations":16,
                      "max_elapsed_ms":120000}
        }),
    )
    .await;
    let plan_ref = planned["plan_ref"]
        .as_str()
        .expect("host-bound caption plan")
        .to_owned();
    let applied = broker_call(
        broker,
        "driver.motion-canvas.composition.apply",
        json!({"plan_ref":plan_ref, "dry_run":false}),
    )
    .await;
    assert_eq!(applied["execution_status"], "completed");
    let fingerprint = applied["fingerprint"]
        .as_str()
        .expect("caption source fingerprint");

    let start = broker_call(
        broker,
        "driver.motion-canvas.render.start",
        json!({
            "expected_fingerprint":fingerprint,
            "profile":{"first_frame":0,"end_frame_exclusive":60,"scale":"full",
                       "transparent":false,"timeout_ms":120000}
        }),
    )
    .await;
    let job_ref = start["job_ref"]
        .as_str()
        .expect("unique caption render job")
        .to_owned();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
    let terminal = loop {
        assert!(
            std::time::Instant::now() < deadline,
            "native caption render did not reach terminal state"
        );
        let status = broker_call(
            broker,
            "driver.motion-canvas.render.status",
            json!({"job_ref":job_ref}),
        )
        .await;
        match status["state"].as_str() {
            Some("succeeded") | Some("failed") | Some("cancelled") => break status,
            _ => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    };
    assert_eq!(
        terminal["state"], "succeeded",
        "native caption render must finish"
    );
    let result = broker_call(
        broker,
        "driver.motion-canvas.render.result",
        json!({"job_ref":job_ref}),
    )
    .await;
    assert_eq!(result["state"], "succeeded");

    let verified = broker_call(
        broker,
        "driver.motion-canvas.composition.verify",
        json!({"plan_ref":plan_ref,"job_ref":job_ref}),
    )
    .await;
    assert_eq!(verified["report"]["support_level"], "native");
    let checks = verified["measurement"]["validation"]["checks"]
        .as_array()
        .expect("actual native rule set");
    let timing = checks
        .iter()
        .find(|row| row["rule"] == "caption-timing")
        .expect("caption timing is a mandatory native rule");
    let findings = verified["measurement"]["findings"].as_array().unwrap();
    if timing["verdict"] != "PASS" {
        // Examine the HOST-OWNED ORIGINAL native observation bytes on this
        // disposable test runner. Only integer frame indices are surfaced;
        // no captions, sessions, paths, fonts or source media are printed.
        let relative = result["artifact"]["directory"]
            .as_str()
            .expect("original render directory");
        assert!(!relative.starts_with('/') && !relative.contains(".."));
        let private = private_owner_output
            .join(relative)
            .join("native-observations.ndjson");
        let metadata = std::fs::symlink_metadata(&private).expect("private observation metadata");
        assert!(
            metadata.is_file()
                && !metadata.file_type().is_symlink()
                && metadata.len() <= 64 * 1024 * 1024
        );
        let original = std::fs::read(private).expect("genuine owner native observation file");
        let mut wrong_a = vec![];
        let mut wrong_b = vec![];
        let mut observed = 0_u64;
        for line in original
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
        {
            let item: Value =
                serde_json::from_slice(line).expect("typed observation from native renderer");
            let frame = item["frame"].as_u64().expect("native frame index");
            observed += 1;
            for (id, begin, end, failures) in [
                ("native-caption-a", 15_u64, 30_u64, &mut wrong_a),
                ("native-caption-b", 36_u64, 48_u64, &mut wrong_b),
            ] {
                let active = item["captions"]
                    .as_array()
                    .and_then(|captions| captions.iter().find(|caption| caption["id"] == id))
                    .and_then(|caption| caption["active"]["value"].as_bool());
                if active != Some((begin..end).contains(&frame)) {
                    failures.push(frame);
                }
            }
        }
        assert_eq!(observed, 60);
        eprintln!(
            "native source-free caption mismatched frame indices: A={wrong_a:?}; B={wrong_b:?}"
        );
        let finding = findings.iter().find(|row| row["rule"] == "caption-timing");
        let first = finding.and_then(|v| v["first_frame"].as_u64());
        let affected = finding.and_then(|v| v["affected_frames"].as_u64());
        // Numeric, source-free diagnostic. The test FAILS; no bypass or
        // implied native text/pixel acceptance is allowed.
        panic!(
            "native caption half-open timing FAILED, first_frame={first:?}, affected_frames={affected:?}"
        );
    }
    assert!(findings.iter().all(|row| row["rule"] != "caption-timing"));
    assert!(
        checks.iter().all(|row| row["verdict"] == "PASS"),
        "another original native verification rule failed"
    );
}

// Independent straight-alpha oracle for the authored sRGB colors. This does
// not call the driver's compositor or inspect its generated drawing commands.
fn subject_blend_expected(mode: &str, backdrop: [u8; 4], source: [u8; 4]) -> [u8; 4] {
    let ab = f64::from(backdrop[3]) / 255.0;
    let asrc = f64::from(source[3]) / 255.0;
    let alpha = if mode == "add" {
        (ab + asrc).min(1.0)
    } else {
        asrc + ab * (1.0 - asrc)
    };
    let quantize = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let mut result = [0; 4];
    if alpha > 0.0 {
        for i in 0..3 {
            let cb = f64::from(backdrop[i]) / 255.0;
            let cs = f64::from(source[i]) / 255.0;
            let premultiplied = match mode {
                "add" => (ab * cb + asrc * cs).min(1.0),
                _ => {
                    let blended = match mode {
                        "normal" => cs,
                        "multiply" => cb * cs,
                        "screen" => cb + cs - cb * cs,
                        _ => panic!("unsupported oracle mode"),
                    };
                    (1.0 - asrc) * ab * cb + (1.0 - ab) * asrc * cs + ab * asrc * blended
                }
            };
            result[i] = quantize(premultiplied / alpha);
        }
    }
    result[3] = quantize(alpha);
    result
}

#[tokio::test]
#[ignore = "requires pinned Motion Canvas runtime plus production Broker and Driver Host"]
async fn subject_blend_native_pixels() {
    assert!(
        std::env::var_os("SEMWRIGHT_TEST_MOTION_CANVAS_RENDER").is_some(),
        "native pixel oracle must not pass without renderer opt-in"
    );
    let h = Harness::new();
    std::fs::remove_file(h.project.path().join("semwright-motion.json")).unwrap();
    let node =
        PathBuf::from(std::env::var_os("SEMWRIGHT_TEST_MOTION_NODE").expect("Node tool env"));
    let runtime =
        PathBuf::from(std::env::var_os("SEMWRIGHT_TEST_MOTION_RUNTIME").expect("runtime root env"));
    let grants = vec![
        grant("motion-node-tool", &node, false),
        grant("project", h.project.path(), true),
        grant("output", h.output.path(), true),
        grant("runtime", &runtime, false),
        grant("fontconfig", Path::new("/etc/fonts"), false),
    ];
    let provider = DriverProvider::connect(
        manifest(h.executable.clone(), digest(&node)),
        h.state.path(),
        &h.helper,
        &grants,
        false,
    )
    .await
    .unwrap();
    let caps = Provider::capabilities(provider.as_ref()).await.unwrap();
    let plan_cap = find(&caps, "driver.motion-canvas.composition.plan");
    assert_eq!(plan_cap.descriptor.version, "1.1");
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

    let film: Value = serde_json::from_slice(include_bytes!(
        "../../../fixtures/composition/motion/subject-blend.json"
    ))
    .unwrap();
    let budget = json!({
        "max_iterations": 4, "max_operations": 64, "max_findings": 64,
        "max_observations": 8, "max_elapsed_ms": 120000
    });
    let mut unsupported = film.clone();
    unsupported["version"] = json!(1);
    let refusal = broker
        .clone()
        .execute(
            BROKER_SESSION.into(),
            semwright_types::unique_id(),
            semwright_types::ExecuteRequest {
                command: "driver.motion-canvas.composition.plan".into(),
                args: json!({"film": unsupported, "budget": budget.clone()}),
                dry_run: false,
                backend: None,
            },
            CancellationToken::new(),
        )
        .await;
    assert!(
        !refusal.ok,
        "v1 must not silently flatten non-normal blending"
    );
    assert_eq!(
        refusal.error.expect("unsupported profile error").code,
        semwright_types::ErrorCode::Unsupported
    );
    let empty = broker_call(
        &broker,
        "driver.motion-canvas.composition.inspect",
        json!({}),
    )
    .await;
    assert_eq!(empty["fingerprint"], Value::Null);
    assert_eq!(empty["film"], Value::Null);
    assert_eq!(empty["authoring_version"], 2);
    assert!(!h.project.path().join("semwright-motion.json").exists());

    let planned = broker_call(
        &broker,
        "driver.motion-canvas.composition.plan",
        json!({"film": film.clone(), "budget": budget}),
    )
    .await;
    let plan_ref = planned["plan_ref"].as_str().unwrap();
    let applied = broker_call(
        &broker,
        "driver.motion-canvas.composition.apply",
        json!({"plan_ref": plan_ref, "dry_run": false}),
    )
    .await;
    assert_eq!(applied["applied"], true);
    let fingerprint = applied["fingerprint"].as_str().unwrap();
    let inspected = broker_call(
        &broker,
        "driver.motion-canvas.composition.inspect",
        json!({}),
    )
    .await;
    assert_eq!(inspected["fingerprint"], fingerprint);
    assert_eq!(inspected["film"]["version"], 2);
    let subjects = inspected["film"]["sequences"][0]["beats"][0]["shots"][0]["subjects"]
        .as_array()
        .unwrap();
    for mode in ["normal", "multiply", "screen", "add"] {
        let id = format!("{mode}_source");
        let subject = subjects
            .iter()
            .find(|s| s["id"] == id)
            .expect("persisted subject");
        assert_eq!(subject["blend_mode"].as_str().unwrap_or("normal"), mode);
    }
    let started = broker_call(
        &broker,
        "driver.motion-canvas.render.start",
        json!({
            "expected_fingerprint": fingerprint,
            "profile": {"first_frame":0, "end_frame_exclusive":1, "scale":"full",
                "transparent":true, "timeout_ms":120000}
        }),
    )
    .await;
    let job_ref = started["job_ref"].as_str().unwrap();
    let terminal = tokio::time::timeout(Duration::from_secs(150), async {
        loop {
            let status = broker_call(
                &broker,
                "driver.motion-canvas.render.status",
                json!({"job_ref":job_ref}),
            )
            .await;
            match status["state"].as_str().unwrap() {
                "succeeded" | "failed" | "cancelled" => break status,
                _ => tokio::time::sleep(Duration::from_millis(100)).await,
            }
        }
    })
    .await
    .expect("bounded native render");
    assert_eq!(terminal["state"], "succeeded", "{terminal:#}");
    assert_eq!(terminal["artifact"]["frame_count"], 1);
    let rendered = broker_call(
        &broker,
        "driver.motion-canvas.render.result",
        json!({"job_ref":job_ref}),
    )
    .await;
    assert_eq!(rendered["state"], "succeeded");
    let file = h
        .output
        .path()
        .join(rendered["artifact"]["directory"].as_str().unwrap())
        .join("frames/000000.png");
    let bytes = std::fs::read(&file).expect("native PNG artifact");
    let mut decoder = png::Decoder::new(std::io::Cursor::new(&bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    decoder.set_limits(png::Limits { bytes: 1024 * 1024 });
    let mut reader = decoder.read_info().unwrap();
    let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut pixels).unwrap();
    assert_eq!((info.width, info.height), (192, 128));
    assert_eq!(info.color_type, png::ColorType::Rgba);
    assert_eq!(info.bit_depth, png::BitDepth::Eight);

    let backdrop = [64, 128, 192, 128];
    let source = [208, 96, 48, 128];
    let mut samples = vec![];
    let mut all_match = true;
    for (mode, cx, cy) in [
        ("normal", 48, 32),
        ("multiply", 144, 32),
        ("screen", 48, 96),
        ("add", 144, 96),
    ] {
        for (region, x, y, expected) in [
            (
                "overlap",
                cx + 4,
                cy,
                subject_blend_expected(mode, backdrop, source),
            ),
            ("inner_clip_left", cx - 24, cy, backdrop),
            ("inner_clip_bottom", cx, cy + 18, backdrop),
            ("outer_clip_right", cx + 36, cy, [0; 4]),
        ] {
            let i = (y * 192 + x) * 4;
            let actual: [u8; 4] = pixels[i..i + 4].try_into().unwrap();
            let passed = actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 3);
            all_match &= passed;
            samples.push(json!({"mode":mode,"region":region,"x":x,"y":y,
                "expected":expected,"actual":actual,"passed":passed}));
        }
    }
    let evidence = json!({"native_artifact_sha256":digest(&file),
        "fingerprint":fingerprint,"job_ref":job_ref,"width":info.width,"height":info.height,
        "tolerance_per_channel":3,"samples":samples,"passed":all_match});
    if let Some(dir) = std::env::var_os("SEMWRIGHT_TEST_MOTION_EVIDENCE").map(PathBuf::from) {
        let dir = dir.join("subject-blend");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("native.png"), &bytes).unwrap();
        std::fs::write(
            dir.join("pixel-oracle.json"),
            serde_json::to_vec_pretty(&evidence).unwrap(),
        )
        .unwrap();
    }
    assert!(
        all_match,
        "native blend pixel oracle mismatch: {evidence:#}"
    );
}

async fn source_call(
    broker: &Arc<Broker>,
    session: &str,
    command: &str,
    args: Value,
) -> semwright_types::Envelope {
    broker
        .clone()
        .execute(
            session.into(),
            semwright_types::unique_id(),
            semwright_types::ExecuteRequest {
                command: command.into(),
                args,
                dry_run: false,
                backend: None,
            },
            CancellationToken::new(),
        )
        .await
}

const SOURCE_READ: &str = "driver.motion-canvas.composition.source.read";

fn source_bound(binding: &str, args: Value) -> Value {
    json!({"binding_ref":binding,"request":{
        "command":SOURCE_READ,"args":args,"dry_run":false
    }})
}

fn source_denied(label: &str, reply: semwright_types::Envelope) -> Value {
    assert!(!reply.ok, "{label} unexpectedly disclosed source");
    assert!(
        reply.data.is_none(),
        "{label} returned data despite refusal"
    );
    let error = reply.error.expect("typed refusal");
    json!({"case":label,"code":error.code,"request_id":reply.request_id})
}

#[tokio::test]
#[ignore = "requires production Broker and Driver Host with owner-pinned runtime"]
async fn applied_source_readback_real_bound_host() {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    assert!(
        std::env::var_os("SEMWRIGHT_TEST_MOTION_SOURCE_READBACK").is_some(),
        "physical source test requires explicit opt-in"
    );
    let h = Harness::new();
    let source = h.project.path().join("semwright-motion.json");
    std::fs::remove_file(&source).unwrap();
    let node = PathBuf::from(std::env::var_os("SEMWRIGHT_TEST_MOTION_NODE").unwrap());
    let runtime = PathBuf::from(std::env::var_os("SEMWRIGHT_TEST_MOTION_RUNTIME").unwrap());
    let grants = vec![
        grant("motion-node-tool", &node, false),
        grant("project", h.project.path(), true),
        grant("output", h.output.path(), true),
        grant("runtime", &runtime, false),
        grant("fontconfig", Path::new("/etc/fonts"), false),
    ];
    let provider = DriverProvider::connect(
        manifest(h.executable.clone(), digest(&node)),
        h.state.path(),
        &h.helper,
        &grants,
        false,
    )
    .await
    .unwrap();
    let identity = provider.identity().id.clone();
    let policy = Policy::new(PolicyConfig {
        allow: [identity.clone(), "providers.execute_bound".into()].into(),
        filesystem: grants,
        ..Default::default()
    })
    .unwrap();
    let broker = Broker::new(
        policy,
        vec![],
        Audit::open(&h.state.path().join("audit"), 65_536, 2).unwrap(),
        Arc::new(NoApprover),
        None,
        json!({}),
        false,
    )
    .unwrap();
    // The concrete production DriverProvider supplies the genuine mount binding.
    let generation = broker
        .mount_driver_provider(provider.clone())
        .await
        .unwrap();
    let owner = BROKER_SESSION;
    let other = "motion-source-foreign-owner";
    let observe = |root: &str| json!({"provider":identity,"root":root});
    let mount = source_call(
        &broker,
        owner,
        "providers.mount.observe",
        observe("project"),
    )
    .await;
    assert!(mount.ok, "{mount:?}");
    let mount = mount.data.unwrap();
    assert_eq!(mount["provider_generation"], generation);
    assert_eq!(mount["mount"]["root"], "project");
    let binding = mount["binding_ref"].as_str().unwrap();
    let output_mount =
        source_call(&broker, owner, "providers.mount.observe", observe("output")).await;
    assert!(output_mount.ok, "{output_mount:?}");
    let output_mount = output_mount.data.unwrap();
    let output_binding = output_mount["binding_ref"].as_str().unwrap();
    let foreign = source_call(
        &broker,
        other,
        "providers.mount.observe",
        observe("project"),
    )
    .await;
    assert!(foreign.ok, "{foreign:?}");
    let foreign = foreign.data.unwrap();
    let foreign_binding = foreign["binding_ref"].as_str().unwrap();

    let film: Value = serde_json::from_slice(include_bytes!(
        "../../../fixtures/composition/motion/subject-blend.json"
    ))
    .unwrap();
    let budget = json!({"max_iterations":4,"max_operations":64,"max_findings":64,
                        "max_observations":8,"max_elapsed_ms":120000});
    let planned = source_call(
        &broker,
        owner,
        "driver.motion-canvas.composition.plan",
        json!({"film":film,"budget":budget}),
    )
    .await;
    assert!(planned.ok, "{planned:?}");
    let planned = planned.data.unwrap();
    let plan_ref = planned["plan_ref"].as_str().unwrap();
    let args = json!({"plan_ref":plan_ref,
        "expected_fingerprint":planned["resulting_fingerprint"],
        "expected_provider_generation":generation});
    let mut negatives = vec![];
    negatives.push(source_denied(
        "unapplied_plan",
        source_call(
            &broker,
            owner,
            "providers.execute_bound",
            source_bound(binding, args.clone()),
        )
        .await,
    ));
    assert!(!source.exists());
    let applied = source_call(
        &broker,
        owner,
        "driver.motion-canvas.composition.apply",
        json!({"plan_ref":plan_ref,"dry_run":false}),
    )
    .await;
    assert!(applied.ok, "{applied:?}");
    let applied = serde_json::to_value(&applied).unwrap();
    assert_eq!(applied["data"]["applied"], true);
    assert_eq!(applied["data"]["fingerprint"], args["expected_fingerprint"]);
    assert_eq!(
        applied["execution"]["provenance"]["provider_generation"],
        generation
    );

    // Read the authoritative physical file independently of driver serialization.
    let original = std::fs::read(&source).unwrap();
    let original_sha = digest(&source);
    assert_eq!(original_sha, args["expected_fingerprint"].as_str().unwrap());
    let readback = source_call(
        &broker,
        owner,
        "providers.execute_bound",
        source_bound(binding, args.clone()),
    )
    .await;
    assert!(readback.ok, "{readback:?}");
    let readback = serde_json::to_value(&readback).unwrap();
    let outer = &readback["data"];
    assert_eq!(outer["admission_current"], true);
    assert_eq!(outer["binding_ref"], binding);
    assert_eq!(
        outer["result"]["execution"]["provenance"]["provider_generation"],
        generation
    );
    let observed = &outer["result"]["data"];
    assert_eq!(observed["plan_ref"], plan_ref);
    assert_eq!(observed["fingerprint"], original_sha);
    assert_eq!(observed["resource"], "project");
    assert_eq!(observed["relative_source"], "semwright-motion.json");
    assert_eq!(observed["byte_count"], original.len());
    let decoded = STANDARD
        .decode(observed["source_base64"].as_str().unwrap())
        .unwrap();
    assert_eq!(decoded, original);
    let model: Value = serde_json::from_slice(&original).unwrap();
    assert_eq!(observed["project_id"], model["id"]);
    assert_eq!(observed["project_generation"], model["generation"]);
    assert_eq!(observed["revision"], model["revision"]);

    negatives.push(source_denied(
        "unbound",
        source_call(&broker, owner, SOURCE_READ, args.clone()).await,
    ));
    negatives.push(source_denied(
        "wrong_mount",
        source_call(
            &broker,
            owner,
            "providers.execute_bound",
            source_bound(output_binding, args.clone()),
        )
        .await,
    ));
    negatives.push(source_denied(
        "foreign_binding",
        source_call(
            &broker,
            other,
            "providers.execute_bound",
            source_bound(binding, args.clone()),
        )
        .await,
    ));
    negatives.push(source_denied(
        "foreign_plan",
        source_call(
            &broker,
            other,
            "providers.execute_bound",
            source_bound(foreign_binding, args.clone()),
        )
        .await,
    ));
    let mut wrong = args.clone();
    wrong["expected_provider_generation"] = json!(generation + 1);
    negatives.push(source_denied(
        "generation_mismatch",
        source_call(
            &broker,
            owner,
            "providers.execute_bound",
            source_bound(binding, wrong),
        )
        .await,
    ));
    let mut wrong = args.clone();
    wrong["expected_fingerprint"] = json!("f".repeat(64));
    negatives.push(source_denied(
        "fingerprint_mismatch",
        source_call(
            &broker,
            owner,
            "providers.execute_bound",
            source_bound(binding, wrong),
        )
        .await,
    ));
    let mut wrong = args.clone();
    wrong["path"] = json!("../outside");
    negatives.push(source_denied(
        "caller_path",
        source_call(
            &broker,
            owner,
            "providers.execute_bound",
            source_bound(binding, wrong),
        )
        .await,
    ));
    assert_eq!(std::fs::read(&source).unwrap(), original);
    assert_eq!(std::fs::read_dir(h.output.path()).unwrap().count(), 0);

    // A human edit can preserve semantic meaning while changing the actual bytes.
    let mut changed = original.clone();
    changed.push(b'\n');
    std::fs::write(&source, &changed).unwrap();
    negatives.push(source_denied(
        "concurrent_source_edit",
        source_call(
            &broker,
            owner,
            "providers.execute_bound",
            source_bound(binding, args.clone()),
        )
        .await,
    ));
    assert_eq!(std::fs::read(&source).unwrap(), changed);
    std::fs::write(&source, &original).unwrap();
    let refreshed = broker.refresh_provider(&identity).await.unwrap();
    assert_ne!(refreshed, generation);
    negatives.push(source_denied(
        "stale_mount_after_generation_change",
        source_call(
            &broker,
            owner,
            "providers.execute_bound",
            source_bound(binding, args.clone()),
        )
        .await,
    ));
    let current = source_call(
        &broker,
        owner,
        "providers.mount.observe",
        observe("project"),
    )
    .await;
    assert!(current.ok, "{current:?}");
    let current = current.data.unwrap();
    assert_eq!(current["provider_generation"], refreshed);
    let current_binding = current["binding_ref"].as_str().unwrap();
    negatives.push(source_denied(
        "old_generation_with_new_binding",
        source_call(
            &broker,
            owner,
            "providers.execute_bound",
            source_bound(current_binding, args.clone()),
        )
        .await,
    ));
    let mut fresh_args = args.clone();
    fresh_args["expected_provider_generation"] = json!(refreshed);
    negatives.push(source_denied(
        "relabel_old_apply_as_current_generation",
        source_call(
            &broker,
            owner,
            "providers.execute_bound",
            source_bound(current_binding, fresh_args.clone()),
        )
        .await,
    ));
    broker.revoke_session(owner);
    negatives.push(source_denied(
        "revoked_binding",
        source_call(
            &broker,
            owner,
            "providers.execute_bound",
            source_bound(current_binding, fresh_args),
        )
        .await,
    ));
    assert_eq!(std::fs::read(&source).unwrap(), original);
    assert_eq!(std::fs::read_dir(h.output.path()).unwrap().count(), 0);
    let evidence = PathBuf::from(
        std::env::var_os("SEMWRIGHT_TEST_MOTION_EVIDENCE").expect("explicit evidence root"),
    )
    .join("source-readback");
    std::fs::create_dir_all(&evidence).unwrap();
    std::fs::write(evidence.join("original-semantic.json"), &original).unwrap();
    let mut retained = readback.clone();
    retained["data"]["binding_ref"] = json!("[expired-test-reference-omitted]");
    std::fs::write(
        evidence.join("readback.json"),
        serde_json::to_vec_pretty(&retained).unwrap(),
    )
    .unwrap();
    // The evidence omits host binding bearer/reference and absolute mount paths.
    std::fs::write(
        evidence.join("verification.json"),
        serde_json::to_vec_pretty(&json!({
            "status":"PASS_BOUND_APPLIED_SOURCE_BYTES","source_sha256":original_sha,
            "original_bytes":original.len(),"provider_generation":generation,
            "refreshed_generation":refreshed,"physical_file_equal":true,
            "project_id":model["id"],"project_generation":model["generation"],
            "revision":model["revision"],"negative_cases":negatives,
            "native_render_executed":false,"output_files":0,
            "source_after_verification_sha256":digest(&source),
            "production_driver_sha256":digest(&h.executable),
            "sandbox_helper_sha256":digest(&h.helper)
        }))
        .unwrap(),
    )
    .unwrap();
}

/// Choose a valid Film near the actual persisted-byte ceiling. The pure
/// projection below is only fixture sizing, never the physical readback oracle.
fn boundary_source_film(base: &semwright_driver_motion_canvas::model::Project) -> Value {
    use semwright_driver_motion_canvas::{authoring, model::MAX_PROJECT_BYTES};
    const CLONES: usize = 32;
    const MAX_RUN: usize = 16384;
    let fixture: Value = serde_json::from_slice(include_bytes!(
        "../../../fixtures/composition/motion/technical.json"
    ))
    .unwrap();
    let mut template = fixture.clone();
    let subjects = template["sequences"][0]["beats"][0]["shots"][0]["subjects"]
        .as_array_mut()
        .unwrap();
    let label = subjects
        .iter()
        .find(|s| s["id"] == "label")
        .unwrap()
        .clone();
    for i in 0..CLONES {
        let mut extra = label.clone();
        extra["id"] = json!(format!("boundary-label-{i:02}"));
        subjects.push(extra);
    }
    let sized = |total: usize| -> Option<(Value, usize)> {
        let mut film = template.clone();
        let subjects = film["sequences"][0]["beats"][0]["shots"][0]["subjects"]
            .as_array_mut()
            .unwrap();
        let count = subjects.len();
        for (i, item) in subjects[count - CLONES..].iter_mut().enumerate() {
            let bytes = total / CLONES + usize::from(i < total % CLONES);
            assert!((1..=MAX_RUN).contains(&bytes));
            item["content"]["runs"][0]["text"] = json!("x".repeat(bytes));
        }
        let typed: semwright_motion_authoring::Film = serde_json::from_value(film.clone()).unwrap();
        let (project, _) = authoring::project(&typed, Some(base)).ok()?;
        let bytes = serde_json::to_vec_pretty(&project).unwrap().len();
        (bytes <= MAX_PROJECT_BYTES).then_some((film, bytes))
    };
    // The lower endpoint must be a real, valid projected Film. Search only
    // within the unchanged text, subject and project budgets.
    let mut best = sized(CLONES).expect("valid boundary fixture template");
    let mut low = CLONES;
    let mut high = CLONES * MAX_RUN;
    while low <= high {
        let mid = low + (high - low) / 2;
        if let Some(candidate) = sized(mid) {
            best = candidate;
            low = mid + 1;
        } else {
            high = mid - 1;
        }
    }
    assert!(
        (MAX_PROJECT_BYTES - 64..=MAX_PROJECT_BYTES).contains(&best.1),
        "valid applied fixture must reach the existing byte ceiling, got {}",
        best.1
    );
    best.0
}

#[tokio::test]
#[ignore = "requires production Broker and Driver Host with owner-pinned runtime"]
async fn applied_source_readback_managed_island_boundary() {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use semwright_driver_motion_canvas::model::{MAX_PROJECT_BYTES, Project};
    assert!(
        std::env::var_os("SEMWRIGHT_TEST_MOTION_SOURCE_READBACK").is_some(),
        "physical source test requires explicit opt-in"
    );
    let h = Harness::new();
    let project = h.project.path();
    let standalone = project.join("semwright-motion.json");
    std::fs::remove_file(&standalone).unwrap();
    std::fs::create_dir(project.join("src")).unwrap();
    let human_path = project.join("src/project.ts");
    let human = b"import {makeProject} from '@motion-canvas/core';\nexport default makeProject({scenes: []});\n";
    std::fs::write(&human_path, human).unwrap();
    let package_path = project.join("package.json");
    let package = br#"{"dependencies":{"@motion-canvas/core":"3.17.2"}}"#;
    std::fs::write(&package_path, package).unwrap();
    let node = PathBuf::from(std::env::var_os("SEMWRIGHT_TEST_MOTION_NODE").unwrap());
    let runtime = PathBuf::from(std::env::var_os("SEMWRIGHT_TEST_MOTION_RUNTIME").unwrap());
    let grants = vec![
        grant("motion-node-tool", &node, false),
        grant("project", project, true),
        grant("output", h.output.path(), true),
        grant("runtime", &runtime, false),
        grant("fontconfig", Path::new("/etc/fonts"), false),
    ];
    let provider = DriverProvider::connect(
        manifest(h.executable.clone(), digest(&node)),
        h.state.path(),
        &h.helper,
        &grants,
        false,
    )
    .await
    .unwrap();
    let identity = provider.identity().id.clone();
    let policy = Policy::new(PolicyConfig {
        allow: [identity.clone(), "providers.execute_bound".into()].into(),
        filesystem: grants,
        ..Default::default()
    })
    .unwrap();
    let broker = Broker::new(
        policy,
        vec![],
        Audit::open(&h.state.path().join("audit"), 65_536, 2).unwrap(),
        Arc::new(NoApprover),
        None,
        json!({}),
        false,
    )
    .unwrap();
    let generation = broker
        .mount_driver_provider(provider.clone())
        .await
        .unwrap();
    // The on-disk legacy fixture omits defaulted variables. The create
    // descriptor requires the complete typed project envelope, as callers do.
    let seed: Project = semwright_driver_motion_canvas::validate::parse(&fixture()).unwrap();
    let seed_args = json!({"project":seed,"dry_run":false});
    assert!(seed_args["project"]["variables"].is_object());
    let caps = Provider::capabilities(provider.as_ref()).await.unwrap();
    let cap = find(&caps, "driver.motion-canvas.project.island.create");
    let schema = jsonschema::validator_for(&cap.descriptor.input_schema).unwrap();
    assert!(
        schema.is_valid(&seed_args),
        "typed island seed must match the actual pinned descriptor"
    );
    let created = source_call(
        &broker,
        BROKER_SESSION,
        "driver.motion-canvas.project.island.create",
        seed_args,
    )
    .await;
    assert!(created.ok, "island creation failed: {:?}", created.error);
    let created = created.data.unwrap();
    assert_eq!(created["applied"], true);
    assert_eq!(created["managed_root"], ".semwright/motion");
    let source = project.join(".semwright/motion/semwright-motion.json");
    let bridge = project.join(".semwright/motion/managed-scenes.ts");
    assert!(source.is_file() && bridge.is_file() && !standalone.exists());
    let base: Project = serde_json::from_slice(&std::fs::read(&source).unwrap()).unwrap();
    let film = boundary_source_film(&base);
    let planned = source_call(
        &broker,
        BROKER_SESSION,
        "driver.motion-canvas.composition.plan",
        json!({"film":film,"budget":{"max_iterations":4,"max_operations":64,
            "max_findings":64,"max_observations":8,"max_elapsed_ms":120000}}),
    )
    .await;
    assert!(planned.ok, "boundary plan failed: {:?}", planned.error);
    let plan = planned.data.unwrap();
    let plan_ref = plan["plan_ref"].as_str().unwrap();
    let applied = source_call(
        &broker,
        BROKER_SESSION,
        "driver.motion-canvas.composition.apply",
        json!({"plan_ref":plan_ref,"dry_run":false}),
    )
    .await;
    assert!(applied.ok, "boundary apply failed: {:?}", applied.error);
    let applied = serde_json::to_value(&applied).unwrap();
    assert_eq!(applied["data"]["applied"], true);
    assert_eq!(applied["data"]["execution_status"], "completed");
    assert_eq!(
        applied["execution"]["provenance"]["provider_generation"],
        generation
    );
    // Capture the ORIGINAL file from the actual owner grant after apply.
    let original = std::fs::read(&source).unwrap();
    let source_sha = digest(&source);
    assert!(
        (MAX_PROJECT_BYTES - 64..=MAX_PROJECT_BYTES).contains(&original.len()),
        "persisted source is {} bytes",
        original.len()
    );
    assert_eq!(source_sha, applied["data"]["fingerprint"]);
    let bridge_sha = digest(&bridge);
    let mount = source_call(
        &broker,
        BROKER_SESSION,
        "providers.mount.observe",
        json!({"provider":identity,"root":"project"}),
    )
    .await;
    assert!(
        mount.ok,
        "owner mount observation failed: {:?}",
        mount.error
    );
    let mount = mount.data.unwrap();
    assert_eq!(mount["provider_generation"], generation);
    assert_eq!(mount["mount"]["root"], "project");
    let binding = mount["binding_ref"].as_str().unwrap();
    let args = json!({"plan_ref":plan_ref,"expected_fingerprint":source_sha,
        "expected_provider_generation":generation});
    let response = source_call(
        &broker,
        BROKER_SESSION,
        "providers.execute_bound",
        source_bound(binding, args.clone()),
    )
    .await;
    assert!(
        response.ok,
        "boundary readback failed: {:?}",
        response.error
    );
    let mut response = serde_json::to_value(&response).unwrap();
    assert_eq!(response["data"]["admission_current"], true);
    assert_eq!(response["data"]["binding_ref"], binding);
    let nested = &response["data"]["result"];
    assert_eq!(nested["ok"], true);
    assert_eq!(nested["execution"]["backend"], "driver:motion-canvas");
    assert_eq!(
        nested["execution"]["provenance"]["provider_generation"],
        generation
    );
    assert_eq!(nested["execution"]["fallbacks_attempted"], json!([]));
    let readback = &nested["data"];
    assert_eq!(readback["resource"], "project");
    assert_eq!(
        readback["relative_source"],
        ".semwright/motion/semwright-motion.json"
    );
    assert_eq!(readback["fingerprint"], source_sha);
    assert_eq!(readback["byte_count"], original.len());
    assert_eq!(
        STANDARD
            .decode(readback["source_base64"].as_str().unwrap())
            .unwrap(),
        original
    );
    let response_size = serde_json::to_vec(&response).unwrap().len();
    assert!(response_size < semwright_types::MAX_FRAME);
    assert_eq!(std::fs::read(&source).unwrap(), original);
    assert_eq!(digest(&bridge), bridge_sha);
    // The same admitted fingerprint must not disclose an over-budget leaf
    // even when an external editor merely appends insignificant whitespace.
    let mut oversize = original.clone();
    oversize.resize(MAX_PROJECT_BYTES + 1, b' ');
    std::fs::write(&source, &oversize).unwrap();
    let refused = source_denied(
        "oversized_physical_leaf",
        source_call(
            &broker,
            BROKER_SESSION,
            "providers.execute_bound",
            source_bound(binding, args),
        )
        .await,
    );
    assert!(matches!(
        refused["code"].as_str(),
        Some("PermissionDenied" | "ResourceExhausted")
    ));
    assert_eq!(std::fs::read(&source).unwrap(), oversize);
    std::fs::write(&source, &original).unwrap();
    assert_eq!(digest(&source), source_sha);
    assert_eq!(digest(&bridge), bridge_sha);
    assert_eq!(std::fs::read(&human_path).unwrap(), human);
    assert_eq!(std::fs::read(&package_path).unwrap(), package);
    assert!(!standalone.exists());
    assert_eq!(std::fs::read_dir(h.output.path()).unwrap().count(), 0);
    let evidence = PathBuf::from(
        std::env::var_os("SEMWRIGHT_TEST_MOTION_EVIDENCE").expect("explicit evidence root"),
    )
    .join("source-readback-island-boundary");
    std::fs::create_dir_all(&evidence).unwrap();
    std::fs::write(evidence.join("original-semantic.json"), &original).unwrap();
    response["data"]["binding_ref"] = json!("[expired-test-reference-omitted]");
    std::fs::write(
        evidence.join("readback.json"),
        serde_json::to_vec_pretty(&response).unwrap(),
    )
    .unwrap();
    std::fs::write(evidence.join("verification.json"), serde_json::to_vec_pretty(&json!({
        "status":"PASS_BOUND_MANAGED_ISLAND_SOURCE_BOUNDARY",
        "source_sha256":source_sha,"source_after_sha256":digest(&source),
        "original_bytes":original.len(),"max_source_bytes":MAX_PROJECT_BYTES,
        "serialized_response_bytes":response_size,"max_protocol_frame_bytes":semwright_types::MAX_FRAME,
        "provider_generation":generation,"physical_file_equal":true,
        "relative_source":".semwright/motion/semwright-motion.json",
        "oversized_leaf_bytes":oversize.len(),"oversized_leaf_refusal":refused,
        "human_project_ts_preserved":true,"package_json_preserved":true,"bridge_preserved":true,
        "standalone_file_created":false,"native_render_executed":false,"output_files":0,
        "production_driver_sha256":digest(&h.executable),"sandbox_helper_sha256":digest(&h.helper)
    })).unwrap()).unwrap();
}
