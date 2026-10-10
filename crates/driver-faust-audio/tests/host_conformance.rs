#![cfg(target_os = "linux")]
//! Actual Broker -> policy -> Driver Host -> sealed interpreter -> decoded PCM.
use semwright_audio_domain::{
    model::{
        AudioProfile, AudioProject, Envelope as AudioEnvelope, MidiEvent, MidiPhrase, Oscillator,
        Sample, SampleOrigin, SampleSource, Signal, SignalNodeKind, Synth, Waveform,
    },
    presets::{self, SfxPreset},
    time::{SampleFrame, SampleRate},
    units::{MilliDb, MilliHz, Permille},
};
use semwright_backend_api::{Context, Provider};
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
    hex::encode(Sha256::digest(fs::read(path).unwrap()))
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
        depth <= 32,
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
            assert!(
                relative.components().count() <= 33,
                "Faust .lib relative path exceeds runtime depth: {}",
                relative.display()
            );
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

async fn direct_host_call(
    provider: &DriverProvider,
    capabilities: &[semwright_backend_api::ProvidedCapability],
    command: &str,
    args: Value,
) -> semwright_types::Result<Value> {
    let name = format!("driver.faust-audio.{command}");
    let descriptor = capabilities
        .iter()
        .find(|capability| capability.descriptor.name == name)
        .unwrap_or_else(|| panic!("missing direct Host capability {name}"));
    Provider::execute(
        provider,
        &Context {
            session: "faust-host-diagnostic".into(),
            request_id: unique_id(),
            cancellation: CancellationToken::new(),
        },
        &descriptor.descriptor,
        &args,
    )
    .await
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
fn write_pcm16_mono_wav(path: &Path, sample_rate: u32, frames: u32) {
    let data_bytes = frames * 2;
    let mut bytes = Vec::with_capacity(44 + data_bytes as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_bytes.to_le_bytes());
    for frame in 0..frames {
        let phase = 2.0 * std::f64::consts::PI * 440.0 * frame as f64 / sample_rate as f64;
        let value = (phase.sin() * 0.5 * 32767.0).round() as i16;
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    fs::write(path, bytes).unwrap();
}

fn sample_synth(sample_id: &str, looped: bool) -> Synth {
    Synth {
        id: if looped {
            "sample-loop-synth".into()
        } else {
            "sample-once-synth".into()
        },
        name: "sample-backed gain proof".into(),
        polyphony: 1,
        signals: vec![
            Signal {
                id: "sample".into(),
                inputs: vec![],
                node: SignalNodeKind::SamplePlayer {
                    sample: sample_id.into(),
                    looped,
                },
            },
            Signal {
                id: "gain".into(),
                inputs: vec!["sample".into()],
                node: SignalNodeKind::Gain {
                    gain: MilliDb(-6_000),
                },
            },
        ],
        output: "gain".into(),
    }
}

fn instrument_fixture() -> (Synth, MidiPhrase) {
    let synth = Synth {
        id: "poly-instrument".into(),
        name: "polyphonic proof".into(),
        polyphony: 4,
        signals: vec![
            Signal {
                id: "osc".into(),
                inputs: vec![],
                node: SignalNodeKind::Oscillator {
                    oscillator: Oscillator {
                        waveform: Waveform::Sine,
                        frequency: MilliHz(440_000),
                        end_frequency: None,
                        amplitude: Permille(700),
                        phase_millidegrees: 0,
                        seed: None,
                    },
                },
            },
            Signal {
                id: "env".into(),
                inputs: vec!["osc".into()],
                node: SignalNodeKind::Envelope {
                    envelope: AudioEnvelope {
                        attack_frames: 48,
                        decay_frames: 96,
                        sustain: Permille(700),
                        release_frames: 2_400,
                    },
                },
            },
        ],
        output: "env".into(),
    };
    let phrase = MidiPhrase {
        id: "proof-phrase".into(),
        name: "proof phrase".into(),
        instrument_synth: Some(synth.id.clone()),
        events: vec![
            MidiEvent::Note {
                id: "n1".into(),
                start: SampleFrame(0),
                duration_frames: 2_400,
                channel: 0,
                note: 69,
                velocity: 96,
            },
            MidiEvent::Note {
                id: "n2".into(),
                start: SampleFrame(4_800),
                duration_frames: 2_400,
                channel: 0,
                note: 72,
                velocity: 100,
            },
        ],
    };
    (synth, phrase)
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
    for dir in ["binary", "state", "libraries", "assets", "output"] {
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
    let assets = root.path().join("assets").canonicalize().unwrap();
    let asset = assets.join("tone.wav");
    write_pcm16_mono_wav(&asset, 48_000, 4_800);
    fs::set_permissions(&asset, fs::Permissions::from_mode(0o400)).unwrap();
    let asset_sha256 = digest(&asset);
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
                root: "audio-assets".into(),
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
            sealed_executable_profile: semwright_driver_sdk::SealedExecutableProfile::Standard,
            root: "faust-tool".into(),
            name: "faust-interpreter".into(),
            sha256: digest(&tool),

            mounts: vec![],
            system_config: vec![],
            dependencies: vec![],
            nvidia_gpu: false,
            resources: None,
        }],
        network: false,
        nvidia_gpu: false,
        loopback_port: None,
        resources: DriverResources {
            open_files: 256,
            processes: 16,
            cpu_seconds: 120,
            operation_cpu_seconds: 120,
            address_space_bytes: 2_147_483_648,
            file_size_bytes: 536_870_912,
        },
        request_timeout_ms: 180_000,
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
            name: "audio-assets".into(),
            path: assets.clone(),
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
    let direct_capabilities = Provider::capabilities(provider.as_ref()).await.unwrap();
    let direct_probe = direct_host_call(
        provider.as_ref(),
        &direct_capabilities,
        "runtime.probe",
        json!({}),
    )
    .await;
    assert!(
        direct_probe.is_ok(),
        "raw Driver Host Faust runtime probe failed before Broker redaction: {direct_probe:?}"
    );
    let direct_probe = direct_probe.unwrap();
    assert_eq!(
        direct_probe["interpreter_compile"], true,
        "raw Driver Host Faust stdlib probe did not compile: {direct_probe}"
    );

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
    assert_eq!(
        runtime_probe.data.as_ref().unwrap()["library_mount"],
        true,
        "{runtime_probe:?}"
    );
    assert_eq!(
        runtime_probe.data.as_ref().unwrap()["stdlib_regular"],
        true,
        "{runtime_probe:?}"
    );
    assert_eq!(
        runtime_probe.data.as_ref().unwrap()["interpreter_compile"],
        true,
        "{runtime_probe:?}"
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
    let sample = Sample {
        id: "tone-sample".into(),
        name: "tone fixture".into(),
        channels: 1,
        sample_rate: SampleRate(48_000),
        frames: 4_800,
        source: SampleSource::RelativePath {
            path: "tone.wav".into(),
        },
        origin: SampleOrigin::Imported,
    };
    let once_synth = sample_synth(&sample.id, false);
    let sample_args = json!({
        "synth_json":serde_json::to_string(&once_synth).unwrap(),
        "sample_json":serde_json::to_string(&sample).unwrap(),
        "expected_sha256":asset_sha256,
        "sample_rate":48000,
        "duration_frames":9600,
        "channels":1,
        "format":"wav",
        "bit_depth":16,
        "output_file":"sample-once.wav"
    });
    let sample_result = call(&broker, "sample.render", sample_args).await;
    assert!(sample_result.ok, "{sample_result:?}");
    let sample_receipt = sample_result.data.unwrap();
    assert_eq!(sample_receipt["input"]["sha256"], digest(&asset));
    assert_eq!(sample_receipt["input"]["resampling"], "exact_only");
    assert_eq!(sample_receipt["input"]["channel_mapping"], "mono_average");
    let sample_pcm = decode_i16(&output.join("sample-once.wav"));
    assert_eq!(sample_pcm.len(), 9_600);
    assert!(
        sample_pcm[..4_800]
            .iter()
            .any(|value| value.unsigned_abs() > 100)
    );
    assert!(
        sample_pcm[4_800..]
            .iter()
            .all(|value| value.unsigned_abs() <= 1),
        "non-looping sample render must feed deterministic silence after EOF"
    );
    let sample_overwrite = call(
        &broker,
        "sample.render",
        json!({
            "synth_json":serde_json::to_string(&once_synth).unwrap(),
            "sample_json":serde_json::to_string(&sample).unwrap(),
            "expected_sha256":digest(&asset),
            "sample_rate":48000,
            "duration_frames":9600,
            "channels":1,
            "format":"wav",
            "bit_depth":16,
            "output_file":"sample-once.wav"
        }),
    )
    .await;
    assert!(!sample_overwrite.ok);
    assert_eq!(sample_overwrite.error.unwrap().code, ErrorCode::Conflict);

    let loop_synth = sample_synth(&sample.id, true);
    let loop_result = call(
        &broker,
        "sample.render",
        json!({
            "synth_json":serde_json::to_string(&loop_synth).unwrap(),
            "sample_json":serde_json::to_string(&sample).unwrap(),
            "expected_sha256":digest(&asset),
            "sample_rate":48000,
            "duration_frames":9600,
            "channels":1,
            "format":"wav",
            "bit_depth":16,
            "output_file":"sample-loop.wav"
        }),
    )
    .await;
    assert!(loop_result.ok, "{loop_result:?}");
    let loop_pcm = decode_i16(&output.join("sample-loop.wav"));
    assert!(
        loop_pcm[4_800..]
            .iter()
            .any(|value| value.unsigned_abs() > 100)
    );

    let bad_hash = call(
        &broker,
        "sample.render",
        json!({
            "synth_json":serde_json::to_string(&once_synth).unwrap(),
            "sample_json":serde_json::to_string(&sample).unwrap(),
            "expected_sha256":"0".repeat(64),
            "sample_rate":48000,
            "duration_frames":9600,
            "channels":1,
            "format":"wav",
            "bit_depth":16,
            "output_file":"sample-bad.wav"
        }),
    )
    .await;
    assert!(!bad_hash.ok);
    assert_eq!(bad_hash.error.unwrap().code, ErrorCode::Conflict);
    assert!(!output.join("sample-bad.wav").exists());

    let (instrument, phrase) = instrument_fixture();
    let instrument_result = call(
        &broker,
        "instrument.render",
        json!({
            "synth_json":serde_json::to_string(&instrument).unwrap(),
            "midi_phrase_json":serde_json::to_string(&phrase).unwrap(),
            "reference_midi_note":69,
            "sample_rate":48000,
            "duration_frames":12000,
            "channels":2,
            "format":"wav",
            "bit_depth":16,
            "output_file":"instrument.wav"
        }),
    )
    .await;
    assert!(instrument_result.ok, "{instrument_result:?}");
    let instrument_receipt = instrument_result.data.unwrap();
    assert_eq!(
        instrument_receipt["native_receipt"]["engine"],
        "faust-poly-interpreter"
    );
    assert_eq!(
        instrument_receipt["voice_policy"],
        "faust_first_free_then_oldest_release_then_oldest_playing"
    );
    let instrument_pcm = decode_i16(&output.join("instrument.wav"));
    assert_eq!(instrument_pcm.len(), 24_000);
    assert!(
        instrument_pcm
            .iter()
            .any(|value| value.unsigned_abs() > 100)
    );
    let tail_start = 7_200usize * 2;
    let tail_end = 9_600usize * 2;
    assert!(
        instrument_pcm[tail_start..tail_end]
            .iter()
            .any(|value| value.unsigned_abs() > 16),
        "semantic release tail must remain audible after final note-off"
    );

    let mut steal_synth = instrument.clone();
    steal_synth.id = "steal-instrument".into();
    steal_synth.polyphony = 2;
    let steal_phrase = MidiPhrase {
        id: "steal-phrase".into(),
        name: "voice stealing proof".into(),
        instrument_synth: Some(steal_synth.id.clone()),
        events: vec![
            MidiEvent::Note {
                id: "s1".into(),
                start: SampleFrame(0),
                duration_frames: 3_000,
                channel: 0,
                note: 60,
                velocity: 90,
            },
            MidiEvent::Note {
                id: "s2".into(),
                start: SampleFrame(0),
                duration_frames: 3_000,
                channel: 0,
                note: 64,
                velocity: 90,
            },
            MidiEvent::Note {
                id: "s3".into(),
                start: SampleFrame(500),
                duration_frames: 3_000,
                channel: 0,
                note: 67,
                velocity: 90,
            },
        ],
    };
    for file in ["steal-a.wav", "steal-b.wav"] {
        let rendered = call(
            &broker,
            "instrument.render",
            json!({
                "synth_json":serde_json::to_string(&steal_synth).unwrap(),
                "midi_phrase_json":serde_json::to_string(&steal_phrase).unwrap(),
                "reference_midi_note":69,
                "sample_rate":48000,
                "duration_frames":7000,
                "channels":2,
                "format":"wav",
                "bit_depth":16,
                "output_file":file
            }),
        )
        .await;
        assert!(rendered.ok, "{rendered:?}");
    }
    assert_eq!(
        digest(&output.join("steal-a.wav")),
        digest(&output.join("steal-b.wav")),
        "pinned Faust voice stealing must be deterministic for identical schedules"
    );

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
            .starts_with(".faust-")
    }));
    Provider::shutdown(provider.as_ref()).await.unwrap();
    let evidence = PathBuf::from("verification/audio/native-host.json");
    fs::create_dir_all(evidence.parent().unwrap()).unwrap();
    fs::write(evidence, serde_json::to_vec_pretty(&json!({"schema_version":1,"route":"broker-policy-driver-host-sealed-faust-interpreter","receipt":receipt,"frames":48000,"channels":2,"pcm_oracle":"independent-riff-i16-count-energy","policy_deny_verified":true,"no_overwrite_verified":true,"scratch_clean":true,"sample_playback_verified":true,"sample_hash_mismatch_denied":true,"sample_no_overwrite_verified":true,"sample_loop_verified":true,"polyphonic_midi_verified":true,"polyphonic_tail_verified":true,"voice_stealing_determinism_verified":true})).unwrap()).unwrap();
}
