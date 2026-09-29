#![cfg(target_os = "linux")]
//! Actual Broker -> policy -> Driver Host -> sealed interpreter -> decoded PCM.
use semwright_audio_domain::{
    model::{AudioProfile, AudioProject},
    presets::{self, SfxPreset},
    time::SampleRate,
};
use semwright_backend_api::Provider;
use semwright_core::{Broker, NoApprover, audit::Audit};
use semwright_driver_host::DriverProvider;
use semwright_driver_sdk::{
    ApplicationMatch, DriverInterfaces, DriverMount, DriverResources, DriverToolMount, Manifest,
    Transport,
};
use semwright_faust_audio::faust::compile_project_synth;
use semwright_policy::{FilesystemGrant, Policy, PolicyConfig};
use semwright_types::{Envelope, ErrorCode, ExecuteRequest, unique_id};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
};
use tokio_util::sync::CancellationToken;

fn digest(path: &Path) -> String {
    format!("{:x}", Sha256::digest(fs::read(path).unwrap()))
}
fn required_path(name: &str) -> PathBuf {
    let path = PathBuf::from(
        std::env::var_os(name).unwrap_or_else(|| panic!("required native prerequisite: {name}")),
    );
    assert!(path.is_file(), "native prerequisite is not a file: {name}");
    path.canonicalize().unwrap()
}
fn materialize_libraries(
    source_root: &Path,
    destination_root: &Path,
    depth: usize,
    pins: &mut BTreeMap<String, String>,
) {
    assert!(
        depth <= 8,
        "Faust library source tree exceeded fixture depth"
    );
    for entry in fs::read_dir(source_root).unwrap() {
        let entry = entry.unwrap();
        let source = entry.path();
        let metadata = fs::symlink_metadata(&source).unwrap();
        let relative = source.strip_prefix("/usr/share/faust").unwrap();
        let destination = destination_root.join(relative);
        if metadata.is_dir() {
            fs::create_dir_all(&destination).unwrap();
            fs::set_permissions(&destination, fs::Permissions::from_mode(0o700)).unwrap();
            materialize_libraries(&source, destination_root, depth + 1, pins);
            continue;
        }
        // Trusted runner package inputs may use file symlinks. fs::copy follows
        // the source target and materializes ordinary private bytes in the fixture.
        if source.extension().is_some_and(|ext| ext == "lib") && source.is_file() {
            fs::create_dir_all(destination.parent().unwrap()).unwrap();
            fs::copy(&source, &destination).unwrap();
            fs::set_permissions(&destination, fs::Permissions::from_mode(0o400)).unwrap();
            let key = relative.to_string_lossy().replace('\\', "/");
            assert!(pins.insert(key, digest(&destination)).is_none());
        }
    }
}

fn assert_staged_faust_compiles(helper: &Path, libraries: &Path) {
    let mut project = AudioProject::new(AudioProfile::default()).unwrap();
    project.synths.insert(
        "host-validate".into(),
        presets::synth_for(
            SfxPreset::Notification,
            "host-validate",
            SampleRate(48_000),
            48_000,
            42,
        )
        .unwrap(),
    );
    project.validate().unwrap();
    let program = compile_project_synth(&project, "host-validate", 48_000, 2).unwrap();
    let mut child = Command::new(helper)
        .args(["validate", libraries.to_string_lossy().as_ref()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(program.source.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "staged Faust validation failed before Driver Host: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(receipt["valid"], true);
    assert_eq!(receipt["outputs"], 2);
}

async fn call(broker: &Arc<Broker>, command: &str, args: Value) -> Envelope {
    broker
        .clone()
        .execute(
            "audio-host-conformance".into(),
            unique_id(),
            ExecuteRequest {
                command: format!("driver.faust-audio.{command}"),
                args,
                dry_run: false,
                backend: None,
            },
            CancellationToken::new(),
        )
        .await
}
fn decode_i16(path: &Path) -> Vec<i16> {
    // Deliberately independent, small fixture-only WAV oracle. Production decoding
    // has its own bounded parser; this oracle never generates the tested PCM.
    let bytes = fs::read(path).unwrap();
    assert_eq!(&bytes[..4], b"RIFF");
    assert_eq!(&bytes[8..12], b"WAVE");
    let mut offset = 12;
    while offset + 8 <= bytes.len() {
        let size = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        assert!(offset + 8 + size <= bytes.len());
        if &bytes[offset..offset + 4] == b"data" {
            return bytes[offset + 8..offset + 8 + size]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|v| i16::from_le_bytes([v[0], v[1]]))
                .collect();
        }
        offset += 8 + size + (size % 2);
    }
    panic!("native output has no PCM data chunk");
}
#[tokio::test]
#[ignore = "requires actual Faust interpreter helper and Bubblewrap/Landlock on a disposable runner"]
async fn broker_sealed_faust_render_has_pcm_provenance_and_no_overwrite() {
    let sandbox = required_path("SEMWRIGHT_TEST_SANDBOX_HELPER");
    let helper_source = required_path("SEMWRIGHT_TEST_FAUST_HELPER");
    let root = tempfile::tempdir().unwrap();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    for dir in ["binary", "state", "libraries", "output"] {
        fs::create_dir(root.path().join(dir)).unwrap();
        fs::set_permissions(root.path().join(dir), fs::Permissions::from_mode(0o700)).unwrap();
    }
    let executable = root.path().join("binary/driver");
    let tool = root.path().join("binary/interpreter");
    fs::copy(
        env!("CARGO_BIN_EXE_semwright-faust-audio-driver"),
        &executable,
    )
    .unwrap();
    fs::copy(helper_source, &tool).unwrap();
    for file in [&executable, &tool] {
        fs::set_permissions(file, fs::Permissions::from_mode(0o500)).unwrap();
    }
    let libraries = root.path().join("libraries");
    let mut library_pins = BTreeMap::new();
    materialize_libraries(
        Path::new("/usr/share/faust"),
        &libraries,
        0,
        &mut library_pins,
    );
    assert!(library_pins.contains_key("stdfaust.lib"));
    assert!(library_pins.len() > 1);
    let config = libraries.join("semwright-runtime.json");
    fs::write(
        &config,
        serde_json::to_vec(
            &json!({"schema_version":1,"compiler_version":std::env::var("SEMWRIGHT_TEST_FAUST_VERSION").expect("explicit pinned runtime version"),"libraries":library_pins}),
        )
        .unwrap(),
    )
    .unwrap();
    fs::set_permissions(&config, fs::Permissions::from_mode(0o400)).unwrap();
    assert_staged_faust_compiles(&tool, &libraries);
    let output = root.path().join("output");
    let manifest = Manifest {
        manifest_version: 1,
        protocol: 4,
        id: "faust-audio".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        publisher: "semwright-native-tests".into(),
        executable: executable.clone(),
        sha256: digest(&executable),
        application: ApplicationMatch {
            desktop_id: None,
            process_names: vec!["faust".into()],
            supported_versions: vec![
                std::env::var("SEMWRIGHT_TEST_FAUST_VERSION")
                    .expect("explicit pinned runtime version"),
            ],
        },
        transport: Transport::StdioV1,
        mounts: vec![
            DriverMount {
                root: "faust-libraries".into(),
                read_only: true,
                execute: false,
            },
            DriverMount {
                root: "output".into(),
                read_only: false,
                execute: false,
            },
        ],
        system_config: vec![],
        secrets: vec![],
        tools: vec![DriverToolMount {
            root: "faust-tool".into(),
            name: "faust-interpreter".into(),
            sha256: digest(&tool),
        }],
        network: false,
        loopback_port: None,
        resources: DriverResources {
            open_files: 256,
            processes: 16,
            cpu_seconds: 120,
            operation_cpu_seconds: 25,
            address_space_bytes: 2_147_483_648,
            file_size_bytes: 536_870_912,
        },
        request_timeout_ms: 40_000,
        interfaces: DriverInterfaces {
            cooperative_cancellation: true,
            progress: true,
            artifacts: true,
            host_tools: false,
            health: true,
            ..Default::default()
        },
    };
    let grants = vec![
        FilesystemGrant {
            name: "faust-libraries".into(),
            path: libraries,
            read: true,
            write: false,
        },
        FilesystemGrant {
            name: "output".into(),
            path: output.clone(),
            read: true,
            write: true,
        },
        FilesystemGrant {
            name: "faust-tool".into(),
            path: tool,
            read: true,
            write: false,
        },
    ];
    let provider = DriverProvider::connect(
        manifest,
        &root.path().join("state"),
        &sandbox,
        &grants,
        false,
    )
    .await
    .unwrap();
    let audit = Audit::open(&root.path().join("audit"), 65536, 2).unwrap();
    let broker = Broker::new(
        Policy::new(PolicyConfig {
            allow: ["driver:faust-audio".into()].into(),
            ..Default::default()
        })
        .unwrap(),
        vec![],
        audit,
        Arc::new(NoApprover),
        None,
        json!({}),
        false,
    )
    .unwrap();
    broker.mount_provider(provider.clone()).await.unwrap();
    let health = call(&broker, "doctor", json!({})).await;
    assert!(health.ok, "{health:?}");
    let doctor = health.data.unwrap();
    assert_eq!(doctor["runtime_available"], true, "{doctor}");
    let runtime_probe = call(&broker, "runtime.probe", json!({})).await;
    assert!(runtime_probe.ok, "{runtime_probe:?}");
    assert_eq!(
        runtime_probe.data.as_ref().unwrap()["compiler_version"],
        std::env::var("SEMWRIGHT_TEST_FAUST_VERSION").unwrap()
    );
    assert_eq!(
        runtime_probe.data.as_ref().unwrap()["sealed_helper_executed"],
        true
    );
    let validation_synth = presets::synth_for(
        SfxPreset::Notification,
        "host-validate",
        SampleRate(48_000),
        48_000,
        42,
    )
    .unwrap();
    let validated = call(
        &broker,
        "synth.validate",
        json!({
            "synth_json": serde_json::to_string(&validation_synth).unwrap(),
            "sample_rate": 48_000,
            "duration_frames": 48_000,
            "channels": 2
        }),
    )
    .await;
    assert!(validated.ok, "{validated:?}");
    assert_eq!(validated.data.as_ref().unwrap()["valid"], true);
    let arguments = json!({"preset":"notification","seed":42,"sample_rate":48000,"duration_frames":48000,"channels":2,"format":"wav","bit_depth":16,"output_file":"proof.wav"});
    let result = call(&broker, "sfx.render", arguments.clone()).await;
    assert!(result.ok, "{result:?}");
    assert_eq!(result.execution.backend, "driver:faust-audio");
    assert!(result.execution.provenance.is_some());
    let receipt = result.data.unwrap();
    assert_eq!(receipt["native_receipt"]["engine"], "faust-interpreter");
    assert_eq!(
        receipt["artifact"]["sha256"],
        digest(&output.join("proof.wav"))
    );
    let pcm = decode_i16(&output.join("proof.wav"));
    assert_eq!(pcm.len(), 96_000);
    assert!(pcm.iter().any(|v| v.unsigned_abs() > 100));
    let sum_square = pcm
        .iter()
        .map(|v| (f64::from(*v) / 32768.0).powi(2))
        .sum::<f64>();
    assert!(sum_square > 0.001 && sum_square < 96_000.0);
    let second = call(&broker, "sfx.render", arguments).await;
    assert!(!second.ok, "existing artifact was overwritten");
    assert_eq!(second.error.unwrap().code, ErrorCode::Conflict);
    assert_eq!(
        receipt["artifact"]["sha256"],
        digest(&output.join("proof.wav"))
    );
    let denied_audit = Audit::open(&root.path().join("denied-audit"), 65536, 2).unwrap();
    let denied = Broker::new(
        Policy::new(PolicyConfig::default()).unwrap(),
        vec![],
        denied_audit,
        Arc::new(NoApprover),
        None,
        json!({}),
        false,
    )
    .unwrap();
    denied.mount_provider(provider.clone()).await.unwrap();
    let refusal = call(&denied, "sfx.render", json!({"preset":"click","seed":1,"sample_rate":48000,"duration_frames":4800,"channels":1,"format":"wav","bit_depth":16,"output_file":"denied.wav"})).await;
    assert!(!refusal.ok);
    assert_eq!(refusal.error.unwrap().code, ErrorCode::PolicyDenied);
    assert!(!output.join("denied.wav").exists());
    assert!(fs::read_dir(&output).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".faust-candidate-")
    }));
    Provider::shutdown(provider.as_ref()).await.unwrap();
    let evidence = PathBuf::from("verification/audio/native-host.json");
    fs::create_dir_all(evidence.parent().unwrap()).unwrap();
    fs::write(evidence, serde_json::to_vec_pretty(&json!({"schema_version":1,"route":"broker-policy-driver-host-sealed-faust-interpreter","receipt":receipt,"frames":48000,"channels":2,"pcm_oracle":"independent-riff-i16-count-energy","policy_deny_verified":true,"no_overwrite_verified":true,"scratch_clean":true})).unwrap()).unwrap();
}
