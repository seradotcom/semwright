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
    process::Command,
    time::Duration,
};
use tokio_util::sync::CancellationToken;

fn digest(path: &Path) -> String {
    format!("{:x}", Sha256::digest(std::fs::read(path).unwrap()))
}

fn configured_tool(variable: &str) -> PathBuf {
    std::fs::canonicalize(
        std::env::var_os(variable)
            .unwrap_or_else(|| panic!("{variable} must point to the real executable")),
    )
    .unwrap()
}

fn find<'a>(
    capabilities: &'a [semwright_backend_api::ProvidedCapability],
    name: &str,
) -> &'a semwright_backend_api::ProvidedCapability {
    capabilities
        .iter()
        .find(|capability| capability.descriptor.name == name)
        .unwrap_or_else(|| panic!("missing capability {name}"))
}

async fn call(
    provider: &DriverProvider,
    capabilities: &[semwright_backend_api::ProvidedCapability],
    name: &str,
    args: Value,
) -> semwright_types::Result<Value> {
    Provider::execute(
        provider,
        &Context {
            session: "mlt-video-live".into(),
            request_id: semwright_types::unique_id(),
            cancellation: CancellationToken::new(),
        },
        &find(capabilities, name).descriptor,
        &args,
    )
    .await
}

fn created(value: &Value, kind: &str) -> (String, String) {
    let row = value["created_refs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["kind"].as_str() == Some(kind))
        .unwrap_or_else(|| panic!("missing created {kind} ref"));
    (
        row["id"].as_str().unwrap().to_owned(),
        row["reference"].as_str().unwrap().to_owned(),
    )
}

fn page_ref(value: &Value, name: &str) -> String {
    value["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"].as_str() == Some(name))
        .and_then(|row| row["reference"].as_str())
        .unwrap_or_else(|| panic!("missing live ref for {name}"))
        .to_owned()
}

async fn sequence_ref(
    provider: &DriverProvider,
    capabilities: &[semwright_backend_api::ProvidedCapability],
    project: &str,
) -> String {
    let sequences = call(
        provider,
        capabilities,
        "driver.mlt-video.sequence.list",
        json!({"project":project,"limit":100}),
    )
    .await
    .unwrap();
    page_ref(&sequences, "Main")
}

#[tokio::test]
#[ignore = "requires bubblewrap, real melt and ffprobe on a Linux host"]
async fn real_mlt_video_driver_runs_inside_sandbox() {
    if std::env::var_os("SEMWRIGHT_TEST_MLT_VIDEO").is_none() {
        return;
    }

    let cargo_executable = PathBuf::from(env!("CARGO_BIN_EXE_semwright-mlt-video-driver"));
    let binary_dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(binary_dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let executable = binary_dir.path().join("semwright-mlt-video-driver");
    std::fs::copy(&cargo_executable, &executable).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();

    let helper = PathBuf::from(
        std::env::var_os("SEMWRIGHT_TEST_SANDBOX_HELPER")
            .expect("SEMWRIGHT_TEST_SANDBOX_HELPER must point to semwright-sandbox"),
    );
    let melt = configured_tool("SEMWRIGHT_TEST_MELT");
    let ffprobe = configured_tool("SEMWRIGHT_TEST_FFPROBE");
    let ffmpeg = configured_tool("SEMWRIGHT_TEST_FFMPEG");
    let bwrap = configured_tool("SEMWRIGHT_TEST_BWRAP");

    let project = tempfile::tempdir().unwrap();
    let media = tempfile::tempdir().unwrap();
    let output = tempfile::tempdir().unwrap();
    let runtime = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    for directory in [
        project.path(),
        media.path(),
        output.path(),
        runtime.path(),
        state.path(),
    ] {
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    }

    // /usr is mounted read-only by DriverProvider. Pin the canonical host tools directly:
    // copying them into a user-owned bind mount can change ownership presentation across
    // Bubblewrap user namespaces even though the bytes and mode are unchanged.
    let runtime_json = json!({
        "schema": 1,
        "melt": {
            "path": melt,
            "sha256": digest(&melt)
        },
        "ffprobe": {
            "path": ffprobe,
            "sha256": digest(&ffprobe)
        },
        "bubblewrap": {
            "path": bwrap.to_string_lossy(),
            "sha256": digest(&bwrap)
        },
        "timeout_seconds": 120
    });
    let runtime_file = runtime.path().join("runtime.json");
    std::fs::write(
        &runtime_file,
        serde_json::to_vec_pretty(&runtime_json).unwrap(),
    )
    .unwrap();
    std::fs::set_permissions(&runtime_file, std::fs::Permissions::from_mode(0o600)).unwrap();

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/media");
    std::fs::copy(fixtures.join("red.mkv"), media.path().join("red.mkv")).unwrap();
    std::fs::copy(fixtures.join("sine.wav"), media.path().join("sine.wav")).unwrap();
    let h264_source = media.path().join("h264-source.mp4");
    let status = Command::new(&ffmpeg)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "color=c=red:s=1920x1080:r=30:d=2",
            "-frames:v",
            // Match the 50-frame semantic clip exactly. This mirrors the launch
            // mezzanine's full-file consumption and avoids making partial-GOP
            // trimming part of the H.264 render conformance contract.
            "50",
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-crf",
            "12",
            "-pix_fmt",
            "yuv420p",
            "-an",
        ])
        .arg(&h264_source)
        .status()
        .unwrap();
    assert!(
        status.success(),
        "failed to create 1080p H.264 input fixture"
    );

    let manifest = Manifest {
        manifest_version: 1,
        protocol: 1,
        id: "mlt-video".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        publisher: "semwright-tests".into(),
        executable: executable.clone(),
        sha256: digest(&executable),
        application: ApplicationMatch {
            desktop_id: Some("org.mltframework.melt".into()),
            process_names: vec!["melt".into()],
            supported_versions: vec![],
        },
        transport: Transport::StdioV1,
        mounts: vec![
            DriverMount {
                root: "project".into(),
                read_only: true,
                execute: false,
            },
            DriverMount {
                root: "media".into(),
                read_only: true,
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
                execute: false,
            },
        ],
        system_config: vec![],
        network: false,
        resources: DriverResources {
            address_space_bytes: 2_147_483_648,
            cpu_seconds: 120,
            file_size_bytes: 1_073_741_824,
            processes: 64,
            open_files: 256,
        },
        request_timeout_ms: 30_000,
        interfaces: DriverInterfaces::default(),
    };
    let grants = vec![
        FilesystemGrant {
            name: "project".into(),
            path: project.path().canonicalize().unwrap(),
            read: true,
            write: false,
        },
        FilesystemGrant {
            name: "media".into(),
            path: media.path().canonicalize().unwrap(),
            read: true,
            write: false,
        },
        FilesystemGrant {
            name: "output".into(),
            path: output.path().canonicalize().unwrap(),
            read: true,
            write: true,
        },
        FilesystemGrant {
            name: "runtime".into(),
            path: runtime.path().canonicalize().unwrap(),
            read: true,
            write: false,
        },
    ];

    let provider = DriverProvider::connect(manifest, state.path(), &helper, &grants, false)
        .await
        .unwrap();
    let capabilities = Provider::capabilities(provider.as_ref()).await.unwrap();
    assert_eq!(capabilities.len(), 68);
    assert!(
        capabilities
            .iter()
            .all(|capability| capability.descriptor.name.starts_with("driver.mlt-video."))
    );

    let doctor = call(
        provider.as_ref(),
        &capabilities,
        "driver.mlt-video.doctor",
        json!({}),
    )
    .await
    .unwrap();
    assert_eq!(doctor["capabilities"], 68);
    assert_eq!(doctor["network"], false);
    assert_eq!(doctor["render_available"], true, "MLT doctor: {doctor}");
    assert!(
        doctor["mlt_version"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );

    let created_project = call(
        provider.as_ref(),
        &capabilities,
        "driver.mlt-video.project.create",
        json!({
            "profile": {
                "width": 160,
                "height": 90,
                "fps_num": 25,
                "fps_den": 1,
                "progressive": true,
                "sample_aspect_num": 1,
                "sample_aspect_den": 1,
                "display_aspect_num": 16,
                "display_aspect_den": 9,
                "colorspace": 709,
                "audio_channels": 2
            }
        }),
    )
    .await
    .unwrap();
    let mut project_ref = created_project["project"].as_str().unwrap().to_owned();
    let mut revision = created_project["revision"].as_str().unwrap().to_owned();

    let sequence = call(
        provider.as_ref(),
        &capabilities,
        "driver.mlt-video.sequence.create",
        json!({"project":project_ref,"expected_revision":revision,"name":"Main"}),
    )
    .await
    .unwrap();
    let (_, initial_sequence_ref) = created(&sequence, "sequence");
    assert!(initial_sequence_ref.starts_with("video:"));
    project_ref = sequence["project"].as_str().unwrap().to_owned();
    revision = sequence["resulting_revision"].as_str().unwrap().to_owned();

    for (name, path) in [("Video", "red.mkv"), ("Audio", "sine.wav")] {
        let value = call(
            provider.as_ref(),
            &capabilities,
            "driver.mlt-video.asset.import",
            json!({
                "project":project_ref,
                "expected_revision":revision,
                "kind":"file",
                "name":name,
                "root":"media",
                "path":path
            }),
        )
        .await
        .unwrap();
        project_ref = value["project"].as_str().unwrap().to_owned();
        revision = value["resulting_revision"].as_str().unwrap().to_owned();
    }

    for (name, kind) in [("Video Track", "video"), ("Audio Track", "audio")] {
        let current_sequence = sequence_ref(provider.as_ref(), &capabilities, &project_ref).await;
        let value = call(
            provider.as_ref(),
            &capabilities,
            "driver.mlt-video.track.create",
            json!({
                "project":project_ref,
                "expected_revision":revision,
                "sequence":current_sequence,
                "name":name,
                "kind":kind
            }),
        )
        .await
        .unwrap();
        project_ref = value["project"].as_str().unwrap().to_owned();
        revision = value["resulting_revision"].as_str().unwrap().to_owned();
    }

    let assets = call(
        provider.as_ref(),
        &capabilities,
        "driver.mlt-video.asset.list",
        json!({"project":project_ref,"limit":100}),
    )
    .await
    .unwrap();
    let current_sequence = sequence_ref(provider.as_ref(), &capabilities, &project_ref).await;
    let tracks = call(
        provider.as_ref(),
        &capabilities,
        "driver.mlt-video.track.list",
        json!({"project":project_ref,"sequence":current_sequence,"limit":100}),
    )
    .await
    .unwrap();
    let video_asset = page_ref(&assets, "Video");
    let video_track = page_ref(&tracks, "Video Track");

    let video_clip = call(
        provider.as_ref(),
        &capabilities,
        "driver.mlt-video.clip.insert",
        json!({
            "project":project_ref,
            "expected_revision":revision,
            "sequence":current_sequence,
            "track":video_track,
            "asset":video_asset,
            "start":0,
            "source_in":0,
            "source_out":50,
            "name":"Video Clip"
        }),
    )
    .await
    .unwrap();
    project_ref = video_clip["project"].as_str().unwrap().to_owned();
    revision = video_clip["resulting_revision"]
        .as_str()
        .unwrap()
        .to_owned();

    let assets = call(
        provider.as_ref(),
        &capabilities,
        "driver.mlt-video.asset.list",
        json!({"project":project_ref,"limit":100}),
    )
    .await
    .unwrap();
    let current_sequence = sequence_ref(provider.as_ref(), &capabilities, &project_ref).await;
    let tracks = call(
        provider.as_ref(),
        &capabilities,
        "driver.mlt-video.track.list",
        json!({"project":project_ref,"sequence":current_sequence,"limit":100}),
    )
    .await
    .unwrap();
    let audio_asset = page_ref(&assets, "Audio");
    let audio_track = page_ref(&tracks, "Audio Track");

    let audio_clip = call(
        provider.as_ref(),
        &capabilities,
        "driver.mlt-video.clip.insert",
        json!({
            "project":project_ref,
            "expected_revision":revision,
            "sequence":current_sequence,
            "track":audio_track,
            "asset":audio_asset,
            "start":0,
            "source_in":0,
            "source_out":50,
            "name":"Audio Clip"
        }),
    )
    .await
    .unwrap();
    project_ref = audio_clip["project"].as_str().unwrap().to_owned();
    revision = audio_clip["resulting_revision"]
        .as_str()
        .unwrap()
        .to_owned();

    let profiles = call(
        provider.as_ref(),
        &capabilities,
        "driver.mlt-video.render.profiles",
        json!({}),
    )
    .await
    .unwrap();
    for id in ["lossless", "h264-1080p"] {
        assert!(
            profiles["profiles"]
                .as_array()
                .unwrap()
                .iter()
                .any(|profile| { profile["id"] == id && profile["available"] == true }),
            "missing live render profile {id}"
        );
    }

    let current_sequence = sequence_ref(provider.as_ref(), &capabilities, &project_ref).await;
    let plan = call(
        provider.as_ref(),
        &capabilities,
        "driver.mlt-video.render.plan",
        json!({
            "project":project_ref,
            "sequence":current_sequence,
            "profile":"lossless",
            "output":"real-runtime.mkv"
        }),
    )
    .await
    .unwrap();
    assert_eq!(plan["runnable"], true);
    assert_eq!(plan["frames"], 50);

    let started = call(
        provider.as_ref(),
        &capabilities,
        "driver.mlt-video.render.start",
        json!({
            "project":project_ref,
            "expected_revision":revision,
            "sequence":current_sequence,
            "profile":"lossless",
            "output":"real-runtime.mkv"
        }),
    )
    .await
    .unwrap();
    let job = started["job"].as_str().unwrap().to_owned();

    let terminal = loop {
        let status = call(
            provider.as_ref(),
            &capabilities,
            "driver.mlt-video.render.status",
            json!({"job":job}),
        )
        .await
        .unwrap();
        match status["state"].as_str().unwrap() {
            "succeeded" | "failed" | "cancelled" | "unknown" => break status,
            _ => tokio::time::sleep(Duration::from_millis(50)).await,
        }
    };
    assert_eq!(terminal["state"], "succeeded", "{terminal:#}");

    let result = call(
        provider.as_ref(),
        &capabilities,
        "driver.mlt-video.render.result",
        json!({"job":job}),
    )
    .await
    .unwrap();
    assert_eq!(result["state"], "succeeded");
    assert_eq!(result["artifact"]["root"], "output");
    assert_eq!(result["artifact"]["path"], "real-runtime.mkv");
    assert!(result["artifact"]["bytes"].as_u64().unwrap() > 100);
    assert_eq!(result["media"]["video"], true);
    assert_eq!(result["media"]["audio"], true);
    assert_eq!(result["media"]["width"], 160);
    assert_eq!(result["media"]["height"], 90);

    let artifact = output.path().join("real-runtime.mkv");
    assert!(artifact.is_file());
    assert!(std::fs::metadata(&artifact).unwrap().len() > 100);

    // Exercise the same curated H.264 consumer used by the launch film on a
    // bounded 50-frame timeline. Relink the video to a synthetic 1080p/30 input
    // first so this tests the film's no-scale geometry instead of MLT 7.22's
    // unrelated 160x90 -> 1080p scaling path.
    let assets = call(
        provider.as_ref(),
        &capabilities,
        "driver.mlt-video.asset.list",
        json!({"project":project_ref,"limit":100}),
    )
    .await
    .unwrap();
    let video_row = assets["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "Video")
        .unwrap();
    let video_ref = video_row["reference"].as_str().unwrap().to_owned();
    let video_resource = video_row["resource"].as_str().unwrap().to_owned();
    let relinked = call(
        provider.as_ref(),
        &capabilities,
        "driver.mlt-video.asset.relink",
        json!({
            "project":project_ref,
            "expected_revision":revision,
            "asset":video_ref,
            "expected_resource":video_resource,
            "root":"media",
            "path":"h264-source.mp4"
        }),
    )
    .await
    .unwrap();
    project_ref = relinked["project"].as_str().unwrap().to_owned();
    revision = relinked["resulting_revision"].as_str().unwrap().to_owned();

    let profiled = call(
        provider.as_ref(),
        &capabilities,
        "driver.mlt-video.project.profile.set",
        json!({
            "project":project_ref,
            "expected_revision":revision,
            "profile": {
                "width":1920,
                "height":1080,
                "fps_num":30,
                "fps_den":1,
                "progressive":true,
                "sample_aspect_num":1,
                "sample_aspect_den":1,
                "display_aspect_num":16,
                "display_aspect_den":9,
                "colorspace":709,
                "audio_channels":2
            }
        }),
    )
    .await
    .unwrap();
    project_ref = profiled["project"].as_str().unwrap().to_owned();
    revision = profiled["resulting_revision"].as_str().unwrap().to_owned();

    let current_sequence = sequence_ref(provider.as_ref(), &capabilities, &project_ref).await;
    let h264_plan = call(
        provider.as_ref(),
        &capabilities,
        "driver.mlt-video.render.plan",
        json!({
            "project":project_ref,
            "sequence":current_sequence,
            "profile":"h264-1080p",
            "output":"real-runtime-h264.mp4"
        }),
    )
    .await
    .unwrap();
    assert_eq!(h264_plan["runnable"], true);
    assert_eq!(h264_plan["frames"], 50);

    let h264_started = call(
        provider.as_ref(),
        &capabilities,
        "driver.mlt-video.render.start",
        json!({
            "project":project_ref,
            "expected_revision":revision,
            "sequence":current_sequence,
            "profile":"h264-1080p",
            "output":"real-runtime-h264.mp4"
        }),
    )
    .await
    .unwrap();
    let h264_job = h264_started["job"].as_str().unwrap().to_owned();
    let h264_terminal = loop {
        let status = call(
            provider.as_ref(),
            &capabilities,
            "driver.mlt-video.render.status",
            json!({"job":h264_job}),
        )
        .await
        .unwrap();
        match status["state"].as_str().unwrap() {
            "succeeded" | "failed" | "cancelled" | "unknown" => break status,
            _ => tokio::time::sleep(Duration::from_millis(50)).await,
        }
    };
    assert_eq!(
        h264_terminal["state"], "succeeded",
        "H.264 live render: {h264_terminal:#}"
    );
    let h264_result = call(
        provider.as_ref(),
        &capabilities,
        "driver.mlt-video.render.result",
        json!({"job":h264_job}),
    )
    .await
    .unwrap();
    assert_eq!(h264_result["state"], "succeeded");
    assert_eq!(h264_result["media"]["video"], true);
    assert_eq!(h264_result["media"]["audio"], true);
    assert_eq!(h264_result["media"]["width"], 1920);
    assert_eq!(h264_result["media"]["height"], 1080);
    assert_eq!(h264_result["media"]["frames"], 50);
    let h264_artifact = output.path().join("real-runtime-h264.mp4");
    assert!(h264_artifact.is_file());
    assert!(std::fs::metadata(&h264_artifact).unwrap().len() > 100);

    Provider::shutdown(provider.as_ref()).await.unwrap();
}
