#![cfg(target_os = "linux")]
use semwright_audio_domain::{analysis::LoudnessAnalysis, wav::WaveReader};
use semwright_backend_api::Provider;
use semwright_core::{Broker, NoApprover, audit::Audit};
use semwright_driver_host::DriverProvider;
use semwright_driver_sdk::{
    ApplicationMatch, DriverInterfaces, DriverMount, DriverResources, DriverToolMount, Manifest,
    Transport,
};
use semwright_policy::{FilesystemGrant, Policy, PolicyConfig};
use semwright_types::{Envelope, ErrorCode, ExecuteRequest, unique_id};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
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
fn stereo_impulse_wav(path: &Path) {
    const SAMPLE_RATE: u32 = 48_000;
    const FRAMES: u32 = 96_000;
    const IMPULSE_FRAME: u32 = 48_000;
    let channels = 2u16;
    let bits = 16u16;
    let data_bytes = FRAMES * u32::from(channels) * 2;
    let mut out = Vec::with_capacity(44 + data_bytes as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&(SAMPLE_RATE * u32::from(channels) * 2).to_le_bytes());
    out.extend_from_slice(&(channels * 2).to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_bytes.to_le_bytes());
    for frame in 0..FRAMES {
        let sample = if frame == IMPULSE_FRAME { i16::MAX } else { 0 };
        out.extend_from_slice(&sample.to_le_bytes());
        out.extend_from_slice(&sample.to_le_bytes());
    }
    fs::write(path, out).unwrap();
}

fn wav(path: &Path, frames: u32, sample_rate: u32) {
    let channels = 1u16;
    let bits = 16u16;
    let data_bytes = frames * 2;
    let mut out = Vec::with_capacity(44 + data_bytes as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_bytes.to_le_bytes());
    for frame in 0..frames {
        let phase = 2.0 * std::f64::consts::PI * 997.0 * frame as f64 / sample_rate as f64;
        let sample = (phase.sin() * 0.25 * 32767.0).round() as i16;
        out.extend_from_slice(&sample.to_le_bytes());
    }
    fs::write(path, out).unwrap();
}
async fn call(broker: &Arc<Broker>, command: &str, args: Value) -> Envelope {
    broker
        .clone()
        .execute(
            "audio-analysis-host".into(),
            unique_id(),
            ExecuteRequest {
                command: format!("driver.audio-analysis.{command}"),
                args,
                dry_run: false,
                backend: None,
            },
            CancellationToken::new(),
        )
        .await
}

#[tokio::test]
#[ignore = "requires real libebur128 helper and Driver Host sandbox on a disposable runner"]
async fn broker_host_meter_measures_digest_bound_wav_and_rejects_substitution() {
    let sandbox = required_path("SEMWRIGHT_TEST_SANDBOX_HELPER");
    let meter_source = required_path("SEMWRIGHT_TEST_AUDIO_METER");
    let root = tempfile::tempdir().unwrap();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    for dir in ["binary", "state", "input"] {
        fs::create_dir(root.path().join(dir)).unwrap();
        fs::set_permissions(root.path().join(dir), fs::Permissions::from_mode(0o700)).unwrap();
    }
    let executable = root.path().join("binary/analysis-driver");
    let meter = root.path().join("binary/audio-meter");
    fs::copy(
        env!("CARGO_BIN_EXE_semwright-audio-analysis-driver"),
        &executable,
    )
    .unwrap();
    fs::copy(meter_source, &meter).unwrap();
    for file in [&executable, &meter] {
        fs::set_permissions(file, fs::Permissions::from_mode(0o500)).unwrap();
    }
    let input = root.path().join("input");
    let signal = input.join("tone.wav");
    wav(&signal, 192_000, 48_000);
    fs::set_permissions(&signal, fs::Permissions::from_mode(0o400)).unwrap();
    let expected = digest(&signal);

    let manifest = Manifest {
        manifest_version: 1,
        protocol: 4,
        id: "audio-analysis".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        publisher: "semwright-native-tests".into(),
        executable: executable.clone(),
        sha256: digest(&executable),
        application: ApplicationMatch {
            desktop_id: None,
            process_names: vec!["audio-meter".into()],
            supported_versions: vec!["libebur128-1.2.6".into()],
        },
        transport: Transport::StdioV1,
        mounts: vec![DriverMount {
            root: "analysis-input".into(),
            read_only: true,
            execute: false,
        }],
        system_config: vec![],
        secrets: vec![],
        tools: vec![DriverToolMount {
            root: "audio-meter-tool".into(),
            name: "audio-meter".into(),
            sha256: digest(&meter),
        }],
        network: false,
        loopback_port: None,
        resources: DriverResources {
            open_files: 128,
            processes: 8,
            cpu_seconds: 90,
            operation_cpu_seconds: 35,
            address_space_bytes: 1_073_741_824,
            file_size_bytes: 536_870_912,
        },
        request_timeout_ms: 40_000,
        interfaces: DriverInterfaces {
            cooperative_cancellation: true,
            health: true,
            ..Default::default()
        },
    };
    let grants = vec![
        FilesystemGrant {
            name: "analysis-input".into(),
            path: input.clone(),
            read: true,
            write: false,
        },
        FilesystemGrant {
            name: "audio-meter-tool".into(),
            path: meter,
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
            allow: ["driver:audio-analysis".into()].into(),
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

    let measured = call(
        &broker,
        "artifact.measure",
        json!({"file_name":"tone.wav","expected_sha256":expected,"layout":"mono"}),
    )
    .await;
    assert!(measured.ok, "{measured:?}");
    let value = measured.data.unwrap();
    assert_eq!(value["artifact"]["sha256"], digest(&signal));
    assert_eq!(value["loudness"]["method"], "libebur128");
    assert_eq!(value["loudness"]["version"], "1.2.6");
    assert_eq!(value["loudness"]["frames"], 192_000);
    assert_eq!(value["loudness"]["sample_rate"], 48_000);
    assert_eq!(value["loudness"]["channels"], 1);
    assert_eq!(value["pcm_statistics"]["frames"], 192_000);
    let sample_peak = value["pcm_statistics"]["peak_millidbfs"].as_i64().unwrap();
    assert!((-12_200..=-11_800).contains(&sample_peak), "{sample_peak}");
    let integrated = value["loudness"]["integrated_lufs_milli"].as_i64().unwrap();
    assert!((-16_500..=-14_500).contains(&integrated), "{integrated}");

    let substituted = call(
        &broker,
        "artifact.measure",
        json!({"file_name":"tone.wav","expected_sha256":"0".repeat(64),"layout":"mono"}),
    )
    .await;
    assert!(!substituted.ok);
    assert_eq!(substituted.error.unwrap().code, ErrorCode::Conflict);

    Provider::shutdown(provider.as_ref()).await.unwrap();
    let evidence = PathBuf::from("verification/audio/analysis-host.json");
    fs::create_dir_all(evidence.parent().unwrap()).unwrap();
    fs::write(
        evidence,
        serde_json::to_vec_pretty(&json!({
            "schema_version":1,
            "route":"broker-policy-driver-host-sealed-libebur128",
            "artifact_sha256":digest(&signal),
            "frames":192000,
            "sample_rate":48000,
            "digest_substitution_denied":true,
            "independent_pcm_decode":true
        }))
        .unwrap(),
    )
    .unwrap();
}

#[test]
#[ignore = "requires the pinned libebur128 helper on a disposable runner"]
fn direct_meter_and_wave_reader_accept_combined_av_impulse() {
    let meter = required_path("SEMWRIGHT_TEST_AUDIO_METER");
    let root = tempfile::tempdir().unwrap();
    let signal = root.path().join("combined-av-impulse.wav");
    stereo_impulse_wav(&signal);
    let expected = digest(&signal);
    assert_eq!(
        expected,
        "072d639c91011a0b5d183d8890eb92af1568136f241faae70a8956308a24133a"
    );

    let output = std::process::Command::new(&meter)
        .args(["analyze", signal.to_string_lossy().as_ref(), "stereo"])
        .output()
        .unwrap();
    eprintln!("meter status={}", output.status);
    eprintln!("meter stdout={}", String::from_utf8_lossy(&output.stdout));
    eprintln!("meter stderr={}", String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success());
    let loudness: LoudnessAnalysis = serde_json::from_slice(&output.stdout).unwrap();
    loudness.validate().unwrap();
    assert_eq!(loudness.frames, 96_000);
    assert_eq!(loudness.sample_rate, 48_000);
    assert_eq!(loudness.channels, 2);
    assert_eq!(loudness.layout, "stereo");

    let reader = WaveReader::open(fs::File::open(&signal).unwrap(), 512 * 1024 * 1024).unwrap();
    assert_eq!(reader.info().frames, 96_000);
    assert_eq!(reader.info().sample_rate.0, 48_000);
    assert_eq!(reader.info().channels, 2);
    let statistics = reader.analyze(-90_000, 480).unwrap();
    assert_eq!(statistics.frames, 96_000);
    assert_eq!(statistics.channels.len(), 2);
}
