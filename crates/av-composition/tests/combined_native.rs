#![cfg(target_os = "linux")]

use semwright_audio_authoring::{
    AudioIntent, AudioSession, BusRole, ClipIntent, DeliveryProfile as AudioDeliveryProfile,
    LoudnessMeasurement, Material, MeasuredAudio, Placement, TrackIntent, base_for, profile,
};
use semwright_audio_domain::{
    analysis::LoudnessAnalysis,
    model::{
        AudioProfile, AudioProject, Sample, SampleOrigin, SampleSource, Signal, SignalNodeKind,
        Synth,
    },
    signal_analysis::SignalStatistics,
    time::SampleRate,
    units::MilliDb,
};
use semwright_av_composition::*;
use semwright_backend_api::{Backend, Context, Provider};
use semwright_core::{Broker, NoApprover, audit::Audit};
use semwright_driver_host::DriverProvider;
use semwright_driver_sdk::{
    ApplicationMatch, DriverInterfaces, DriverMount, DriverResources, DriverToolMount, Manifest,
    Transport, descriptor_digest,
};
use semwright_media_time::{MediaArtifact, MediaMetadata, Rate, Rational, Retention};
use semwright_motion_authoring::Film;
use semwright_platform_common::{artifact::ArtifactHandoff, filesystem::Filesystem};
use semwright_policy::{FilesystemGrant, Policy, PolicyConfig};
use semwright_project_graph as pg;
use semwright_recipes::Executor;
use semwright_registry::{Metadata, Registry};
use semwright_semantic_composition::{
    Address, BaseState, BaseStateSet, CapabilityBinding, Concurrency, ConvergenceBudget, Digest,
    EffectClass, EvidenceClass, EvidenceSource, ExecutionStatus, ObservationRef, Owner, Phase,
    PrincipalBinding, ResourceKey, Revision, RuleResult, SupportLevel, ValidationReport, Verdict,
    VerificationReport, canonical_digest,
};
use semwright_types::ExecuteRequest;
use serde_json::{Value, json};
use sha2::{Digest as ShaDigest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::Arc,
};
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

const SESSION: &str = "combined-av-native-e2e";
const B_AUDIO_SHA: &str = "df2654bed6d2ac57d547846b69d16ea48b4a9ee3";

fn file_sha(path: &Path) -> String {
    format!("{:x}", Sha256::digest(fs::read(path).unwrap()))
}

fn required_file(name: &str) -> PathBuf {
    let path = PathBuf::from(
        std::env::var_os(name).unwrap_or_else(|| panic!("missing native prerequisite {name}")),
    );
    assert!(path.is_file(), "{name} is not a file: {}", path.display());
    path.canonicalize().unwrap()
}

fn required_dir(name: &str) -> PathBuf {
    let path = PathBuf::from(
        std::env::var_os(name).unwrap_or_else(|| panic!("missing native prerequisite {name}")),
    );
    assert!(
        path.is_dir(),
        "{name} is not a directory: {}",
        path.display()
    );
    path.canonicalize().unwrap()
}

fn make_dir(root: &Path, name: &str) -> PathBuf {
    let path = root.join(name);
    fs::create_dir_all(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path
}

fn copy_exec(source: &Path, destination: &Path) -> PathBuf {
    copy_exec_bounded(source, destination, 64 * 1024 * 1024)
}

fn copy_tool(source: &Path, destination: &Path) -> PathBuf {
    copy_exec_bounded(source, destination, 256 * 1024 * 1024)
}

fn copy_exec_bounded(source: &Path, destination: &Path, limit: u64) -> PathBuf {
    fs::copy(source, destination).unwrap();
    fs::set_permissions(destination, fs::Permissions::from_mode(0o500)).unwrap();
    let metadata = fs::metadata(destination).unwrap();
    assert!(metadata.is_file(), "provider executable is not regular");
    assert!(
        metadata.len() <= limit,
        "executable {} exceeds its Driver Host {} byte budget: {} bytes",
        source.display(),
        limit,
        metadata.len()
    );
    assert_eq!(
        metadata.permissions().mode() & 0o022,
        0,
        "provider executable is writable by group/other"
    );
    destination.to_path_buf()
}

fn grant(name: &str, path: &Path, read: bool, write: bool) -> FilesystemGrant {
    FilesystemGrant {
        name: name.into(),
        path: path.canonicalize().unwrap(),
        read,
        write,
    }
}

fn binary_resources(cpu_seconds: u64, processes: u64) -> DriverResources {
    DriverResources {
        open_files: 512,
        processes,
        cpu_seconds,
        operation_cpu_seconds: 0,
        address_space_bytes: 4_294_967_296,
        file_size_bytes: 1_073_741_824,
    }
}

fn motion_manifest(executable: &Path, node: &Path) -> Manifest {
    Manifest {
        manifest_version: 1,
        protocol: 7,
        id: "motion-canvas".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        publisher: "semwright-combined-native".into(),
        executable: executable.into(),
        sha256: file_sha(executable),
        application: ApplicationMatch {
            desktop_id: None,
            process_names: vec!["node".into()],
            supported_versions: vec!["3.17.2".into()],
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
            sha256: file_sha(node),
            mounts: vec![
                "project".into(),
                "output".into(),
                "runtime".into(),
                "fontconfig".into(),
            ],
            system_config: vec![],
            dependencies: vec![],
        }],
        network: false,
        loopback_port: None,
        resources: binary_resources(300, 256),
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

fn faust_manifest(executable: &Path, helper: &Path, version: &str) -> Manifest {
    Manifest {
        manifest_version: 1,
        protocol: 4,
        id: "faust-audio".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        publisher: "semwright-combined-native".into(),
        executable: executable.into(),
        sha256: file_sha(executable),
        application: ApplicationMatch {
            desktop_id: None,
            process_names: vec!["faust".into()],
            supported_versions: vec![version.into()],
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
            root: "faust-tool".into(),
            name: "faust-interpreter".into(),
            sha256: file_sha(helper),

            mounts: vec![],
            system_config: vec![],
            dependencies: vec![],
        }],
        network: false,
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
            health: true,
            ..DriverInterfaces::default()
        },
    }
}

fn analysis_manifest(executable: &Path, meter: &Path) -> Manifest {
    Manifest {
        manifest_version: 1,
        protocol: 4,
        id: "audio-analysis".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        publisher: "semwright-combined-native".into(),
        executable: executable.into(),
        sha256: file_sha(executable),
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
            sha256: file_sha(meter),

            mounts: vec![],
            system_config: vec![],
            dependencies: vec![],
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
            ..DriverInterfaces::default()
        },
    }
}

fn mlt_manifest(h: &Harness) -> Manifest {
    Manifest {
        manifest_version: 1,
        protocol: 7,
        id: "mlt-video".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        publisher: "semwright-combined-native".into(),
        executable: h.mlt_exe.clone(),
        sha256: file_sha(&h.mlt_exe),
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
                root: "mlt-runtime".into(),
                read_only: true,
                execute: true,
            },
        ],
        system_config: vec![],
        secrets: vec![],
        tools: vec![
            DriverToolMount {
                root: "mlt-runner-tool".into(),
                name: "mlt-runner".into(),
                sha256: file_sha(&h.mlt_runner),
                mounts: vec![
                    "mlt-runtime".into(),
                    "project".into(),
                    "media".into(),
                    "output".into(),
                ],
                dependencies: vec!["melt".into(), "ffprobe".into(), "ffmpeg".into()],
                system_config: vec![],
            },
            DriverToolMount {
                root: "melt-tool".into(),
                name: "melt".into(),
                sha256: file_sha(&h.melt),
                mounts: vec![],
                dependencies: vec![],
                system_config: vec![],
            },
            DriverToolMount {
                root: "ffprobe-tool".into(),
                name: "ffprobe".into(),
                sha256: file_sha(&h.ffprobe),
                mounts: vec![],
                dependencies: vec![],
                system_config: vec![],
            },
            DriverToolMount {
                root: "ffmpeg-tool".into(),
                name: "ffmpeg".into(),
                sha256: file_sha(&h.ffmpeg),
                mounts: vec![],
                dependencies: vec![],
                system_config: vec![],
            },
        ],
        network: false,
        loopback_port: None,
        resources: binary_resources(300, 256),
        request_timeout_ms: 300_000,
        interfaces: DriverInterfaces {
            host_tools: true,
            cooperative_cancellation: true,
            ..Default::default()
        },
    }
}

struct Harness {
    _root: TempDir,
    project: PathBuf,
    output: PathBuf,
    media: PathBuf,
    mlt_runtime_root: PathBuf,
    mlt_runner: PathBuf,
    node: PathBuf,
    melt: PathBuf,
    ffprobe: PathBuf,
    ffmpeg: PathBuf,
    state_motion: PathBuf,
    state_faust: PathBuf,
    state_analysis: PathBuf,
    state_mlt: PathBuf,
    motion_exe: PathBuf,
    faust_exe: PathBuf,
    analysis_exe: PathBuf,
    mlt_exe: PathBuf,
    sandbox: PathBuf,
    faust_helper: PathBuf,
    meter: PathBuf,
    faust_libraries: PathBuf,
    motion_runtime: PathBuf,
    faust_version: String,
}

impl Harness {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let bin = make_dir(root.path(), "bin");
        let project = make_dir(root.path(), "motion-project");
        let output = make_dir(root.path(), "shared-output");
        let media = make_dir(root.path(), "av-delivery");
        let runtime = make_dir(root.path(), "mlt-runtime");
        let state_motion = make_dir(root.path(), "state-motion");
        let state_faust = make_dir(root.path(), "state-faust");
        let state_analysis = make_dir(root.path(), "state-analysis");
        let state_mlt = make_dir(root.path(), "state-mlt");
        let assets = make_dir(root.path(), "audio-assets");
        write_sync_impulse_wav(&assets.join("sync-impulse.wav"));

        let motion_exe = copy_exec(
            &required_file("SEMWRIGHT_TEST_COMBINED_MOTION_DRIVER"),
            &bin.join("semwright-motion-canvas-driver"),
        );
        let faust_exe = copy_exec(
            &required_file("SEMWRIGHT_TEST_COMBINED_FAUST_DRIVER"),
            &bin.join("semwright-faust-audio-driver"),
        );
        let analysis_exe = copy_exec(
            &required_file("SEMWRIGHT_TEST_COMBINED_ANALYSIS_DRIVER"),
            &bin.join("semwright-audio-analysis-driver"),
        );
        let mlt_exe = copy_exec(
            &required_file("SEMWRIGHT_TEST_COMBINED_MLT_DRIVER"),
            &bin.join("semwright-mlt-video-driver"),
        );
        let mlt_runner = copy_tool(
            &required_file("SEMWRIGHT_TEST_COMBINED_MLT_RUNNER"),
            &bin.join("semwright-mlt-runtime-runner"),
        );
        let node = copy_tool(
            &required_file("SEMWRIGHT_TEST_MOTION_NODE"),
            &bin.join("motion-node"),
        );
        let sandbox = required_file("SEMWRIGHT_TEST_SANDBOX_HELPER");
        let faust_helper = copy_tool(
            &required_file("SEMWRIGHT_TEST_FAUST_HELPER"),
            &bin.join("faust-interpreter"),
        );
        let meter = copy_tool(
            &required_file("SEMWRIGHT_TEST_AUDIO_METER"),
            &bin.join("audio-meter"),
        );
        let faust_libraries = required_dir("SEMWRIGHT_TEST_FAUST_LIBRARIES");
        let motion_runtime = required_dir("SEMWRIGHT_TEST_MOTION_RUNTIME");
        let faust_version =
            std::env::var("SEMWRIGHT_TEST_FAUST_VERSION").expect("pinned Faust version");

        let melt = required_file("SEMWRIGHT_TEST_MELT");
        let ffprobe = required_file("SEMWRIGHT_TEST_FFPROBE");
        let ffmpeg = required_file("SEMWRIGHT_TEST_FFMPEG");
        let mlt_runtime_root = melt
            .parent()
            .and_then(Path::parent)
            .unwrap()
            .canonicalize()
            .unwrap();
        let bwrap = required_file("SEMWRIGHT_TEST_BWRAP");
        let runtime_json = json!({
            "schema": 1,
            "melt": {"path": melt, "sha256": file_sha(&melt)},
            "ffprobe": {"path": ffprobe, "sha256": file_sha(&ffprobe)},
            "ffmpeg": {"path": ffmpeg, "sha256": file_sha(&ffmpeg)},
            "bubblewrap": {"path": bwrap, "sha256": file_sha(&bwrap)},
            "timeout_seconds": 120
        });
        let runtime_file = runtime.join("runtime.json");
        fs::write(
            &runtime_file,
            serde_json::to_vec_pretty(&runtime_json).unwrap(),
        )
        .unwrap();
        fs::set_permissions(&runtime_file, fs::Permissions::from_mode(0o600)).unwrap();

        Self {
            _root: root,
            project,
            output,
            media,
            mlt_runtime_root,
            mlt_runner,
            node,
            melt,
            ffprobe,
            ffmpeg,
            state_motion,
            state_faust,
            state_analysis,
            state_mlt,
            motion_exe,
            faust_exe,
            analysis_exe,
            mlt_exe,
            sandbox,
            faust_helper,
            meter,
            faust_libraries,
            motion_runtime,
            faust_version,
        }
    }

    fn audio_assets(&self) -> PathBuf {
        self._root.path().join("audio-assets")
    }

    async fn providers(&self) -> Vec<Arc<DriverProvider>> {
        let motion = DriverProvider::connect(
            motion_manifest(&self.motion_exe, &self.node),
            &self.state_motion,
            &self.sandbox,
            &[
                grant("project", &self.project, true, true),
                grant("output", &self.output, true, true),
                grant("runtime", &self.motion_runtime, true, false),
                grant("fontconfig", Path::new("/etc/fonts"), true, false),
                grant("motion-node-tool", &self.node, true, false),
            ],
            false,
        )
        .await
        .unwrap();

        let faust = DriverProvider::connect(
            faust_manifest(&self.faust_exe, &self.faust_helper, &self.faust_version),
            &self.state_faust,
            &self.sandbox,
            &[
                grant("faust-libraries", &self.faust_libraries, true, false),
                grant("audio-assets", &self.audio_assets(), true, false),
                grant("output", &self.output, true, true),
                grant("faust-tool", &self.faust_helper, true, false),
            ],
            false,
        )
        .await
        .unwrap();

        let analysis = DriverProvider::connect(
            analysis_manifest(&self.analysis_exe, &self.meter),
            &self.state_analysis,
            &self.sandbox,
            &[
                grant("analysis-input", &self.output, true, false),
                grant("audio-meter-tool", &self.meter, true, false),
            ],
            false,
        )
        .await
        .unwrap();

        let mlt = DriverProvider::connect(
            mlt_manifest(self),
            &self.state_mlt,
            &self.sandbox,
            &[
                grant("project", &self.project, true, false),
                grant("media", &self.media, true, false),
                grant("output", &self.output, true, true),
                grant("mlt-runtime", &self.mlt_runtime_root, true, false),
                grant("mlt-runner-tool", &self.mlt_runner, true, false),
                grant("melt-tool", &self.melt, true, false),
                grant("ffprobe-tool", &self.ffprobe, true, false),
                grant("ffmpeg-tool", &self.ffmpeg, true, false),
            ],
            false,
        )
        .await
        .unwrap();

        vec![motion, faust, analysis, mlt]
    }

    async fn direct_faust_sample_render(&self, mut args: Value) -> semwright_types::Result<Value> {
        let state = make_dir(self._root.path(), "state-faust-direct-diagnostic");
        let provider = DriverProvider::connect(
            faust_manifest(&self.faust_exe, &self.faust_helper, &self.faust_version),
            &state,
            &self.sandbox,
            &[
                grant("faust-libraries", &self.faust_libraries, true, false),
                grant("audio-assets", &self.audio_assets(), true, false),
                grant("output", &self.output, true, true),
                grant("faust-tool", &self.faust_helper, true, false),
            ],
            false,
        )
        .await?;
        let capabilities = Provider::capabilities(provider.as_ref()).await?;
        let descriptor = capabilities
            .iter()
            .find(|capability| capability.descriptor.name == "driver.faust-audio.sample.render")
            .expect("direct Faust sample.render descriptor");
        args["output_file"] = Value::String("sync-direct-diagnostic.wav".into());
        Provider::execute(
            provider.as_ref(),
            &Context {
                session: "combined-av-direct-faust-diagnostic".into(),
                request_id: "combined-av-direct-faust-sample-render".into(),
                cancellation: CancellationToken::new(),
            },
            &descriptor.descriptor,
            &args,
        )
        .await
    }

    async fn broker(&self) -> Arc<Broker> {
        let providers = self.providers().await;
        let global_grants = vec![
            grant("audio-output", &self.output, true, false),
            grant("av-delivery", &self.media, true, true),
            grant("candidate", &self.output, true, true),
            grant("published", &self.media, true, true),
        ];
        let allow = providers
            .iter()
            .map(|provider| provider.identity().id.clone())
            .collect::<BTreeSet<_>>();
        let policy = Policy::new(PolicyConfig {
            allow,
            filesystem: global_grants.clone(),
            ..Default::default()
        })
        .unwrap();
        let backends: Vec<Arc<dyn Backend>> = vec![
            Arc::new(Filesystem::new(&global_grants).unwrap()),
            Arc::new(ArtifactHandoff::new(&global_grants).unwrap()),
        ];
        let broker = Broker::new(
            policy,
            backends,
            Audit::open(&self._root.path().join("audit"), 262_144, 4).unwrap(),
            Arc::new(NoApprover),
            None,
            json!({}),
            false,
        )
        .unwrap();
        for provider in providers {
            let identity = provider.identity().clone();
            let provider_id = identity.id.clone();
            let capabilities = provider
                .capabilities()
                .await
                .unwrap_or_else(|error| panic!("enumerate {provider_id}: {error:?}"));
            let capability_count = capabilities.len();
            eprintln!("combined-e2e mount provider={provider_id} capabilities={capability_count}");
            for capability in &capabilities {
                let mut registry = Registry::empty();
                let mut metadata = Metadata::for_provider(&identity);
                metadata.aliases = capability.aliases.clone();
                metadata.tags = capability.tags.clone();
                metadata.object_types = capability.object_types.clone();
                registry
                    .register_with_metadata(capability.descriptor.clone(), metadata)
                    .unwrap_or_else(|error| {
                        panic!(
                            "registry preflight provider={provider_id} command={}: {error:?}",
                            capability.descriptor.name
                        )
                    });
            }
            broker
                .mount_provider(provider)
                .await
                .unwrap_or_else(|error| {
                    panic!(
                        "mount provider={provider_id} capabilities={capability_count}: {error:?}"
                    )
                });
        }
        broker
    }
}

async fn call(executor: &dyn Executor, command: &str, args: Value) -> Value {
    executor
        .execute(
            ExecuteRequest {
                command: command.into(),
                args,
                dry_run: false,
                backend: None,
            },
            CancellationToken::new(),
        )
        .await
        .unwrap_or_else(|error| panic!("{command} failed: {error:?}"))
}

fn command_proof(executor: &dyn Executor, command: &str) -> CommandProof {
    let descriptor = executor.describe(command).unwrap();
    assert_eq!(descriptor.name, command);
    CommandProof {
        command: command.into(),
        descriptor: Digest::parse(descriptor_digest(&descriptor).unwrap()).unwrap(),
    }
}

fn service_proof(
    executor: &dyn Executor,
    service: Service,
    provider: String,
    runtime_digest: Digest,
    bindings: &[(Stage, &[&str])],
) -> ServiceProof {
    let commands = bindings
        .iter()
        .map(|(stage, names)| {
            (
                *stage,
                names
                    .iter()
                    .map(|name| command_proof(executor, name))
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    ServiceProof {
        service,
        provider,
        generation: 1,
        catalog_digest: canonical_digest(&commands).unwrap(),
        runtime_digest,
        commands,
        available: true,
    }
}

fn write_sync_impulse_wav(path: &Path) {
    const SAMPLE_RATE: u32 = 48_000;
    const FRAMES: u32 = 96_000;
    const IMPULSE_FRAME: u32 = 48_000;
    let data_bytes = FRAMES * 2;
    let mut bytes = Vec::with_capacity(44 + data_bytes as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    bytes.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_bytes.to_le_bytes());
    for frame in 0..FRAMES {
        let sample = if frame == IMPULSE_FRAME { i16::MAX } else { 0 };
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o400)).unwrap();
}

fn sync_sample_synth(sample_id: &str) -> Synth {
    Synth {
        id: "sync-synth".into(),
        name: "Technical sync impulse".into(),
        polyphony: 1,
        signals: vec![Signal {
            id: "sync-sample".into(),
            inputs: vec![],
            node: SignalNodeKind::SamplePlayer {
                sample: sample_id.into(),
                looped: false,
            },
        }],
        output: "sync-sample".into(),
    }
}

async fn audio_consumer_receipt(
    executor: &dyn Executor,
    owner: &Owner,
    cues: &semwright_media_time::CueGraph,
    harness: &Harness,
) -> AudioConsumerReceipt {
    let sample = Sample {
        id: "sync-impulse".into(),
        name: "Technical one-sample sync impulse".into(),
        channels: 1,
        sample_rate: SampleRate(48_000),
        frames: 96_000,
        source: SampleSource::RelativePath {
            path: "sync-impulse.wav".into(),
        },
        origin: SampleOrigin::Deterministic {
            generator: "combined-av-sync-impulse-v1".into(),
            seed: 0,
        },
    };
    let sample_sha = file_sha(&harness.audio_assets().join("sync-impulse.wav"));
    let sample_digest = Digest::parse(sample_sha.clone()).unwrap();
    let synth = sync_sample_synth(&sample.id);
    let mut project = AudioProject::new(AudioProfile {
        sample_rate: SampleRate(48_000),
        channels: 2,
        tempo_milli_bpm: 120_000,
        time_signature_numerator: 4,
        time_signature_denominator: 4,
    })
    .unwrap();
    project.id = "combined-audio".into();
    project.samples.insert(sample.id.clone(), sample.clone());
    project.synths.insert(synth.id.clone(), synth.clone());
    project.validate().unwrap();

    let base = base_for(
        &project,
        "driver:faust-audio",
        owner,
        "1",
        Concurrency::BestEffortRevalidate,
    )
    .unwrap();
    let render_descriptor = executor
        .describe("driver.faust-audio.sample.render")
        .unwrap();
    let trusted_profile = profile(vec![CapabilityBinding {
        phase: Phase::Apply,
        command: render_descriptor.name.clone(),
        descriptor: Digest::parse(descriptor_digest(&render_descriptor).unwrap()).unwrap(),
        effects: BTreeSet::from([EffectClass::RenderPrivateArtifact]),
    }])
    .unwrap();
    let dependency = cues.cues.first().expect("technical cue").source.clone();
    let intent = AudioIntent {
        version: 1,
        id: "combined-audio-intent".into(),
        tracks: vec![TrackIntent {
            id: "sync-track".into(),
            name: "Technical sync".into(),
            role: BusRole::SoundEffect,
            existing_stem: None,
            output_bus: project.master_bus.clone(),
            channels: 2,
            gain: MilliDb(0),
            pan_milli: 0,
            sends: vec![],
            effects: semwright_audio_domain::model::EffectChain::default(),
            clips: vec![ClipIntent {
                id: "sync-clip".into(),
                material: Material::Sample {
                    sample: sample.id.clone(),
                    sha256: sample_digest.clone(),
                },
                placement: Placement::Absolute {
                    start: Rational::ZERO,
                    duration: Rational::new(2, 1).unwrap(),
                },
                source_offset_frames: 0,
                gain: MilliDb(0),
                fade_in_frames: 0,
                fade_out_frames: 0,
            }],
        }],
        ducking: vec![],
        cues: cues.clone(),
        dependencies: BTreeMap::from([
            ("sync-cue-source".into(), dependency),
            (sample.id.clone(), sample_digest.clone()),
        ]),
        delivery: AudioDeliveryProfile {
            id: "combined-av".into(),
            peak_ceiling_millidbfs: 0,
            integrated_lufs_milli: None,
            loudness_tolerance_milli: 0,
            true_peak_ceiling_millidbtp: None,
            allow_silence: false,
            minimum_master_gain: MilliDb(-12_000),
            maximum_master_gain: MilliDb(12_000),
        },
        budget: ConvergenceBudget {
            max_iterations: 4,
            max_operations: 64,
            max_findings: 64,
            max_observations: 8,
            max_elapsed_ms: 120_000,
        },
    };

    assert_eq!(
        intent.dependencies.get(&sample.id),
        Some(&sample_digest),
        "technical sample digest must be bound before audio planning"
    );
    let clip_digest = match &intent.tracks[0].clips[0].material {
        Material::Sample { sample: id, sha256 } if id == &sample.id => sha256,
        other => panic!("unexpected technical sync material: {other:?}"),
    };
    assert_eq!(
        clip_digest, &sample_digest,
        "technical clip and dependency must share the exact digest"
    );

    let mut session = AudioSession::new(trusted_profile).unwrap();
    let planned = session
        .prepare(owner.clone(), base.clone(), &project, intent)
        .unwrap();
    let plan_id = planned.plan.digest.as_str().to_owned();
    let candidate = session
        .begin_apply(owner, &planned, &project, &base, "combined-audio-apply")
        .unwrap();
    let applied = candidate.model;
    let applied_model = Digest::parse(applied.semantic_digest().unwrap()).unwrap();
    assert_eq!(applied_model, planned.resulting_model_digest);
    let mut applied_base = base.clone();
    applied_base.0[0].revision = Revision::Fingerprint(applied_model.clone());

    let render_args = json!({
        "synth_json": serde_json::to_string(applied.synths.get("sync-synth").unwrap()).unwrap(),
        "sample_json": serde_json::to_string(applied.samples.get("sync-impulse").unwrap()).unwrap(),
        "expected_sha256": sample_sha,
        "sample_rate": 48_000,
        "duration_frames": 96_000,
        "channels": 2,
        "format": "wav",
        "bit_depth": 16,
        "output_file": "sync-final.wav"
    });
    let direct = harness
        .direct_faust_sample_render(render_args.clone())
        .await;
    eprintln!("combined-e2e direct Faust sample.render diagnostic={direct:?}");
    direct.expect("raw Driver Host Faust sample.render must succeed before Broker dispatch");
    let render = call(executor, "driver.faust-audio.sample.render", render_args).await;
    session
        .finish_apply(candidate.permit, ExecutionStatus::Completed)
        .unwrap();

    let artifact = render
        .get("artifact")
        .and_then(Value::as_object)
        .expect("Faust render artifact");
    let artifact_digest = Digest::parse(
        artifact
            .get("sha256")
            .and_then(Value::as_str)
            .unwrap()
            .to_owned(),
    )
    .unwrap();
    let artifact_bytes = artifact.get("bytes").and_then(Value::as_u64).unwrap();
    assert_eq!(
        artifact.get("file").and_then(Value::as_str),
        Some("sync-final.wav")
    );
    assert_eq!(render["native_receipt"]["frames"].as_u64(), Some(96_000));
    assert_eq!(
        render["native_receipt"]["sample_rate"].as_u64(),
        Some(48_000)
    );
    assert_eq!(render["native_receipt"]["channels"].as_u64(), Some(2));

    let measured = call(
        executor,
        "driver.audio-analysis.artifact.measure",
        json!({
            "file_name": "sync-final.wav",
            "expected_sha256": artifact_digest.as_str(),
            "layout": "stereo"
        }),
    )
    .await;
    let statistics: SignalStatistics =
        serde_json::from_value(measured["pcm_statistics"].clone()).unwrap();
    let loudness: LoudnessAnalysis = serde_json::from_value(measured["loudness"].clone()).unwrap();
    assert_eq!(statistics.frames, 96_000);
    assert_eq!(statistics.sample_rate, SampleRate(48_000));
    assert_eq!(statistics.channels.len(), 2);
    assert_eq!(loudness.frames, 96_000);
    assert_eq!(loudness.sample_rate, 48_000);
    assert_eq!(loudness.channels, 2);
    assert_eq!(loudness.layout, "stereo");
    assert_eq!(loudness.momentary_lufs_milli, None);
    assert_eq!(
        loudness.unknown_reason.as_deref(),
        Some("one_or_more_windows_have_insufficient_frames")
    );

    let measurement = MeasuredAudio {
        artifact: artifact_digest.clone(),
        source_model: applied_model.clone(),
        base: applied_base.clone(),
        statistics,
        loudness: Some(LoudnessMeasurement {
            method: loudness.method,
            version: loudness.version,
            integrated_lufs_milli: loudness.integrated_lufs_milli,
            momentary_lufs_milli: loudness.momentary_lufs_milli,
            short_term_lufs_milli: loudness.short_term_lufs_milli,
            loudness_range_milli: loudness.loudness_range_milli,
            true_peak_millidbtp: loudness.true_peak_millidbtp,
            unknown_reason: loudness.unknown_reason,
        }),
        decoder: "libebur128+independent-wav-pcm".into(),
        decoder_version: 1,
        exhaustive: true,
    };
    let measurement_id = session
        .record_measurement(owner, &plan_id, measurement)
        .unwrap();
    let verification = session
        .verify(owner, &plan_id, &measurement_id, &applied)
        .unwrap();
    assert_eq!(verification.verdict().unwrap(), Verdict::Pass);
    assert_eq!(verification.execution_status, ExecutionStatus::Completed);
    assert_eq!(verification.support_level, SupportLevel::Composed);

    let project = Subplan {
        version: 1,
        service: Service::Audio,
        owner: owner.clone(),
        plan_ref: plan_id,
        plan_digest: planned.plan.digest.clone(),
        base: planned.plan.body.base.clone(),
        cue_digest: cues.digest().unwrap(),
        duration: Rational::new(2, 1).unwrap(),
        dependencies: planned.plan.body.dependencies.clone(),
        required_rules: planned.plan.body.required_rules.clone(),
    };
    let master = MediaArtifact {
        reference: format!("artifact:sha256:{}", artifact_digest.as_str()),
        owner: owner.clone(),
        sha256: artifact_digest.clone(),
        bytes: artifact_bytes,
        media_type: "audio/wav".into(),
        source_plan: planned.plan.digest.clone(),
        source_state: applied_base,
        metadata: MediaMetadata {
            duration: Rational::new(2, 1).unwrap(),
            encoded_duration: Some(Rational::new(2, 1).unwrap()),
            video: None,
            audio: Some(semwright_media_time::AudioMetadata {
                sample_rate: 48_000,
                channels: 2,
                channel_layout: "stereo".into(),
                sample_frames: 96_000,
                priming_samples: None,
                padding_samples: None,
                latency_samples: None,
                tail_samples: None,
            }),
        },
        dependencies: BTreeMap::from([
            ("semantic-model".into(), applied_model),
            (
                "audio-implementation".into(),
                Digest::of_bytes(B_AUDIO_SHA.as_bytes()),
            ),
        ]),
        provenance: Some(format!(
            "role-b-certified-sha={B_AUDIO_SHA}; faust-native+libebur128"
        )),
        license: None,
        retention: Retention::PrivateCandidate,
    };
    let receipt = AudioConsumerReceipt {
        version: 1,
        project,
        master,
        stems: vec![],
        verification,
        cue_digest: cues.digest().unwrap(),
        handoff: Some(ArtifactHandoffHint {
            version: 1,
            artifact_digest,
            relative_path: "sync-final.wav".into(),
        }),
    };
    receipt.validate().unwrap();
    receipt
}

async fn motion_subplan(executor: &dyn Executor, film: &Film) -> (Owner, Subplan) {
    let planned = call(
        executor,
        "driver.motion-canvas.composition.plan",
        json!({
            "film": film,
            "budget": {
                "max_iterations": 4,
                "max_operations": 64,
                "max_findings": 64,
                "max_observations": 8,
                "max_elapsed_ms": 120_000
            }
        }),
    )
    .await;
    assert_eq!(planned["repair"], false);
    let body = &planned["plan"]["body"];
    let owner: Owner = serde_json::from_value(body["owner"].clone()).unwrap();
    assert_eq!(owner.session, SESSION);
    let base: BaseStateSet = serde_json::from_value(body["base"].clone()).unwrap();
    let dependencies: BTreeMap<String, Digest> =
        serde_json::from_value(body["dependencies"].clone()).unwrap();
    let required_rules: BTreeSet<String> =
        serde_json::from_value(body["required_rules"].clone()).unwrap();
    let plan_digest = Digest::parse(
        planned["plan"]["digest"]
            .as_str()
            .expect("Motion plan digest")
            .to_owned(),
    )
    .unwrap();
    let plan_ref = planned["plan_ref"]
        .as_str()
        .expect("Motion plan ref")
        .to_owned();
    assert_eq!(plan_ref, plan_digest.as_str());

    (
        owner.clone(),
        Subplan {
            version: 1,
            service: Service::Motion,
            owner,
            plan_ref,
            plan_digest,
            base,
            cue_digest: film.cues.digest().unwrap(),
            duration: Rational::new(2, 1).unwrap(),
            dependencies,
            required_rules,
        },
    )
}

fn runtime_digest(path: &Path) -> Digest {
    Digest::parse(file_sha(path)).unwrap()
}

fn host_state(provider: &str, resource: &str, owner: &Owner, revision: Digest) -> BaseState {
    BaseState {
        key: ResourceKey {
            provider: provider.into(),
            resource: resource.into(),
        },
        document_id: format!("combined-{resource}"),
        provider_session: owner.session.clone(),
        generation: "1".into(),
        revision: Revision::Fingerprint(revision),
        concurrency: Concurrency::BestEffortRevalidate,
    }
}

fn combined_av_plan(
    executor: &dyn Executor,
    harness: &Harness,
    film: &Film,
    motion: Subplan,
    audio: &AudioConsumerReceipt,
) -> AvPlan {
    let owner = motion.owner.clone();
    assert_eq!(audio.project.owner, owner);
    assert_eq!(motion.cue_digest, audio.cue_digest);

    let mlt_runtime = runtime_digest(&harness.mlt_exe);
    let artifact_runtime = Digest::of_bytes(b"semwright-artifact-handoff-filesystem-v1");
    let audio_runtime = canonical_digest(&(
        file_sha(&harness.faust_exe),
        file_sha(&harness.analysis_exe),
        B_AUDIO_SHA.to_owned(),
    ))
    .unwrap();

    let motion_provider = motion.base.0[0].key.provider.clone();
    let audio_provider = audio.project.base.0[0].key.provider.clone();
    let delivery_provider = "driver:mlt-video".to_owned();
    let artifacts_provider = "artifacts".to_owned();
    let decode_provider = "driver:mlt-video".to_owned();

    let services = vec![
        service_proof(
            executor,
            Service::Motion,
            motion_provider,
            runtime_digest(&harness.motion_exe),
            &[
                (
                    Stage::ApplyMotion,
                    &["driver.motion-canvas.composition.apply"],
                ),
                (
                    Stage::RenderMotion,
                    &[
                        "driver.motion-canvas.render.plan",
                        "driver.motion-canvas.render.execute",
                    ],
                ),
                (
                    Stage::VerifyMotion,
                    &["driver.motion-canvas.composition.verify"],
                ),
            ],
        ),
        service_proof(
            executor,
            Service::Audio,
            audio_provider,
            audio_runtime,
            &[
                (Stage::ApplyAudio, &["driver.faust-audio.sample.render"]),
                (Stage::RenderAudio, &["driver.faust-audio.sample.render"]),
                (
                    Stage::VerifyAudio,
                    &["driver.audio-analysis.artifact.measure"],
                ),
                (
                    Stage::VerifyFinalAudio,
                    &["driver.audio-analysis.artifact.measure"],
                ),
            ],
        ),
        service_proof(
            executor,
            Service::Delivery,
            delivery_provider.clone(),
            mlt_runtime.clone(),
            &[
                (Stage::PlanDelivery, &["driver.mlt-video.render.profiles"]),
                (Stage::TransferMotion, &["driver.mlt-video.frames.encode"]),
                (Stage::Mux, &["driver.mlt-video.av.mux"]),
            ],
        ),
        service_proof(
            executor,
            Service::Artifacts,
            artifacts_provider.clone(),
            artifact_runtime.clone(),
            &[
                (Stage::TransferAudio, &["artifact.handoff"]),
                (Stage::PreparePublication, &["filesystem.write"]),
                (Stage::Publish, &["artifact.handoff"]),
            ],
        ),
        service_proof(
            executor,
            Service::Decode,
            decode_provider.clone(),
            mlt_runtime.clone(),
            &[(Stage::VerifySync, &["driver.mlt-video.sync.probe"])],
        ),
    ];

    let mut base = motion.base.0.clone();
    base.extend(audio.project.base.0.clone());
    base.push(host_state(
        &delivery_provider,
        "delivery",
        &owner,
        mlt_runtime.clone(),
    ));
    base.push(host_state(
        &artifacts_provider,
        "artifacts",
        &owner,
        artifact_runtime,
    ));
    base.push(host_state(&decode_provider, "decode", &owner, mlt_runtime));

    AvPlan::prepare(AvPlanBody {
        version: 1,
        spec: AvSpec {
            version: 1,
            id: "combined-native-av".into(),
            owner,
            cues: film.cues.clone(),
            delivery: DeliveryProfile {
                codec: DeliveryCodec::Mp4H264Aac,
                frame_rate: Rate::new(30, 1).unwrap(),
                width: 320,
                height: 180,
                duration: Rational::new(2, 1).unwrap(),
                sample_rate: 48_000,
                channels: 2,
                audio_is_final_mix: true,
                max_artifact_bytes: 64 * 1024 * 1024,
            },
            sync: SyncSpec {
                cues: vec![SyncCue {
                    id: "sync-pulse".into(),
                    expected_time: Rational::ONE,
                }],
                max_offset: Rational::new(1, 20).unwrap(),
                max_drift: Rational::new(1, 30).unwrap(),
                max_cue_error: Rational::new(1, 20).unwrap(),
                confidence_floor: 8_000,
                require_full_scan: true,
            },
            required_final_audio_rules: BTreeSet::from([
                "decoded-audio-duration".into(),
                "decoded-audio-peak".into(),
            ]),
        },
        motion,
        audio: audio.project.clone(),
        base: BaseStateSet(base),
        services,
        budget: ConvergenceBudget {
            max_iterations: 4,
            max_operations: 32,
            max_findings: 64,
            max_observations: 32,
            max_elapsed_ms: 300_000,
        },
    })
    .unwrap()
}

fn proof_for(plan: &AvPlan, stage: Stage) -> ServiceProof {
    plan.body
        .services
        .iter()
        .find(|proof| proof.service == stage.service())
        .unwrap()
        .clone()
}

async fn execute_native_stage(
    coordinator: &mut AvCoordinator,
    adapter: &mut AgentAStageAdapter,
    executor: &dyn Executor,
) {
    let stage = coordinator.next_stage().expect("next AV stage");
    let proof = proof_for(coordinator.plan(), stage);
    let owner = coordinator.plan().body.spec.owner.clone();
    let fresh = coordinator.expected_base().clone();
    let call = coordinator.reserve(&owner, &proof, &fresh).unwrap();
    coordinator
        .before_dispatch(&call, &owner, &proof, &fresh)
        .unwrap();
    let result = adapter
        .execute(&call, executor, CancellationToken::new())
        .await
        .unwrap_or_else(|error| panic!("{stage:?} failed: {error:?}"));
    coordinator
        .complete(NativeReceipt {
            request_id: call.request_id,
            av_plan_digest: call.av_plan_digest,
            owner: call.owner,
            stage,
            proof,
            observed_base: fresh,
            status: ExecutionStatus::Completed,
            result: Some(result),
            effects: vec![],
        })
        .unwrap();
}

async fn execute_publication_stages(
    coordinator: &mut AvCoordinator,
    executor: &dyn Executor,
    owner: &Owner,
) -> PublicationCandidate {
    let publisher = BrokerPublisher::new(
        executor,
        owner.clone(),
        PublicationTargets {
            candidate_root: "candidate".into(),
            output_root: "published".into(),
            pointer_path: "verified-av.json".into(),
            allow_pointer_replacement: false,
        },
    )
    .unwrap();

    let proof = proof_for(coordinator.plan(), Stage::PreparePublication);
    let fresh = coordinator.expected_base().clone();
    let call = coordinator.reserve(owner, &proof, &fresh).unwrap();
    coordinator
        .before_dispatch(&call, owner, &proof, &fresh)
        .unwrap();
    let candidate = publisher
        .prepare(coordinator, CancellationToken::new())
        .await
        .unwrap();
    coordinator
        .complete(NativeReceipt {
            request_id: call.request_id,
            av_plan_digest: call.av_plan_digest,
            owner: call.owner,
            stage: Stage::PreparePublication,
            proof,
            observed_base: fresh,
            status: ExecutionStatus::Completed,
            result: Some(NativeResult::PublicationPrepared {
                candidate: candidate.clone(),
            }),
            effects: vec![],
        })
        .unwrap();

    assert_eq!(coordinator.next_stage(), Some(Stage::Publish));
    let proof = proof_for(coordinator.plan(), Stage::Publish);
    let fresh = coordinator.expected_base().clone();
    let call = coordinator.reserve(owner, &proof, &fresh).unwrap();
    coordinator
        .before_dispatch(&call, owner, &proof, &fresh)
        .unwrap();
    let result = publisher
        .publish(coordinator, &candidate, CancellationToken::new())
        .await
        .unwrap();
    coordinator
        .complete(NativeReceipt {
            request_id: call.request_id,
            av_plan_digest: call.av_plan_digest,
            owner: call.owner,
            stage: Stage::Publish,
            proof,
            observed_base: fresh,
            status: ExecutionStatus::Completed,
            result: Some(result),
            effects: vec![],
        })
        .unwrap();
    assert!(coordinator.ready());
    candidate
}

fn c14_asset(
    graph: &mut pg::ProjectGraph,
    access: &pg::ProjectAccess,
    resource_type: &str,
    label: &str,
) -> pg::LogicalAssetId {
    let id = pg::LogicalAssetId::new();
    graph
        .register(
            access,
            pg::Asset {
                id: id.clone(),
                resource_type: resource_type.into(),
                label: label.into(),
                locator: None,
            },
        )
        .unwrap();
    id
}

fn c14_observe(
    graph: &mut pg::ProjectGraph,
    access: &pg::ProjectAccess,
    owner: &Owner,
    asset: &pg::LogicalAssetId,
    digest: Digest,
    source: EvidenceSource,
    method: &str,
    tick: u64,
) -> pg::RevisionRecord {
    let resource = ResourceKey {
        provider: "composition-av-c14".into(),
        resource: asset.as_str().into(),
    };
    let base = BaseStateSet(vec![BaseState {
        key: resource.clone(),
        document_id: graph.project_id().as_str().into(),
        provider_session: owner.session.clone(),
        generation: "1".into(),
        revision: Revision::Fingerprint(digest.clone()),
        concurrency: Concurrency::BestEffortRevalidate,
    }]);
    let observation = ObservationRef {
        id: format!("c14-observation-{tick}"),
        base,
        source,
        method: method.into(),
        method_version: 1,
        scope: vec![Address {
            resource: resource.clone(),
            logical_id: asset.as_str().into(),
            property: "bytes".into(),
        }],
        artifact: Some(digest.clone()),
        exhaustive: true,
    };
    let generation = graph.inspect(access, asset).unwrap().binding_generation;
    let adapter = pg::RevisionAdapter::registered(resource, source, method.into(), 1).unwrap();
    let admitted = adapter
        .admit(
            owner,
            graph.project_id(),
            asset,
            generation,
            pg::RevisionCandidate {
                version: pg::SCHEMA_VERSION,
                asset: asset.clone(),
                fingerprint: pg::Fingerprint {
                    bytes: Some(digest),
                    projection: None,
                },
                equivalence: pg::Equivalence::ExactBytes,
                observed_unix_ms: tick,
                binding_generation: generation,
                observation,
                coverage: pg::Coverage::complete(),
            },
        )
        .unwrap();
    let record = admitted.record().clone();
    graph.accept_revision(access, admitted).unwrap();
    record
}

fn c14_byte_verification(
    plan: Digest,
    revision: &pg::RevisionRecord,
    rule: &str,
) -> VerificationReport {
    VerificationReport {
        execution_status: ExecutionStatus::Completed,
        support_level: SupportLevel::Composed,
        effects_observed: revision.observation.scope.clone(),
        effects_unobservable: vec![],
        validation: ValidationReport {
            plan_digest: plan,
            base: revision.observation.base.clone(),
            required_rules: BTreeSet::from([rule.into()]),
            checks: vec![RuleResult {
                rule: rule.into(),
                version: 1,
                verdict: Verdict::Pass,
                evidence_class: EvidenceClass::Deterministic,
                evidence: vec![revision.observation.clone()],
                reason: None,
            }],
        },
    }
}

#[tokio::test]
#[ignore = "requires pinned Motion Canvas, Faust, libebur128, MLT/FFmpeg and production Driver Host"]
async fn combined_a_b_native_av_candidate_uses_post_encode_audio_and_full_scan_sync() {
    if std::env::var_os("SEMWRIGHT_TEST_COMBINED_AV").is_none() {
        return;
    }

    let harness = Harness::new();
    let broker = harness.broker().await;
    let executor = broker.session_executor(SESSION);
    let film: Film = serde_json::from_slice(include_bytes!(
        "../../../fixtures/composition/av/technical-film.json"
    ))
    .unwrap();
    film.validate().unwrap();
    assert_eq!(film.timing.duration, Rational::new(2, 1).unwrap());
    assert_eq!(film.cues.cues.len(), 1);
    assert_eq!(film.cues.cues[0].id, "sync-pulse");

    let (owner, motion) = motion_subplan(&executor, &film).await;
    let audio = audio_consumer_receipt(&executor, &owner, &film.cues, &harness).await;
    assert_eq!(audio.verification.verdict().unwrap(), Verdict::Pass);
    let pre_encode_audio_digest = audio.master.sha256.clone();

    let plan = combined_av_plan(&executor, &harness, &film, motion, &audio);
    let plan_digest = plan.digest.clone();
    let mut coordinator = AvCoordinator::new(plan.clone()).unwrap();
    coordinator.import_audio_receipt(audio.clone()).unwrap();

    let routes = AgentAArtifactRoutes {
        audio_source_root: "audio-output".into(),
        handoff_destination_root: "av-delivery".into(),
        mlt_media_root: "media".into(),
    };
    let mut adapter = AgentAStageAdapter::with_artifact_routes(plan, Some(routes)).unwrap();
    adapter.bind_audio_consumer_receipt(&audio).unwrap();

    assert_eq!(coordinator.next_stage(), Some(Stage::PlanDelivery));
    execute_native_stage(&mut coordinator, &mut adapter, &executor).await;
    assert_eq!(coordinator.next_stage(), Some(Stage::ApplyMotion));
    execute_native_stage(&mut coordinator, &mut adapter, &executor).await;

    assert_eq!(coordinator.next_stage(), Some(Stage::ApplyAudio));
    coordinator.complete_imported_audio_stage().unwrap();
    assert_eq!(coordinator.next_stage(), Some(Stage::RenderMotion));
    execute_native_stage(&mut coordinator, &mut adapter, &executor).await;

    assert_eq!(coordinator.next_stage(), Some(Stage::RenderAudio));
    coordinator.complete_imported_audio_stage().unwrap();
    assert_eq!(coordinator.next_stage(), Some(Stage::VerifyMotion));
    execute_native_stage(&mut coordinator, &mut adapter, &executor).await;

    assert_eq!(coordinator.next_stage(), Some(Stage::VerifyAudio));
    coordinator.complete_imported_audio_stage().unwrap();
    for stage in [
        Stage::TransferMotion,
        Stage::TransferAudio,
        Stage::Mux,
        Stage::VerifyFinalAudio,
        Stage::VerifySync,
    ] {
        assert_eq!(coordinator.next_stage(), Some(stage));
        execute_native_stage(&mut coordinator, &mut adapter, &executor).await;
    }

    assert_eq!(coordinator.next_stage(), Some(Stage::PreparePublication));
    assert!(!coordinator.ready());
    let manifest = coordinator.manifest().unwrap();
    manifest.validate().unwrap();
    assert_eq!(manifest.av_plan_digest, plan_digest);
    assert_eq!(manifest.audio_plan_digest, audio.project.plan_digest);
    assert_eq!(
        manifest.audio_verification.verdict().unwrap(),
        Verdict::Pass
    );
    assert_eq!(
        manifest.final_audio_verification.verdict().unwrap(),
        Verdict::Pass
    );
    assert_eq!(manifest.sync.verdict, Verdict::Pass);
    assert!(manifest.sync.exhaustive);
    assert_eq!(
        manifest.sync.artifact_digest,
        manifest.final_artifact.sha256
    );
    assert_eq!(manifest.final_artifact.media_type, "video/mp4");
    assert_eq!(
        manifest.final_artifact.dependencies.get("audio-artifact"),
        Some(&pre_encode_audio_digest)
    );
    assert!(!manifest.r16_closed);
    assert!(!manifest.promotional_video);

    let final_audio_evidence = manifest
        .final_audio_verification
        .validation
        .checks
        .iter()
        .flat_map(|check| &check.evidence)
        .filter_map(|evidence| evidence.artifact.as_ref())
        .cloned()
        .collect::<BTreeSet<_>>();
    assert_eq!(final_audio_evidence.len(), 1);
    let post_encode_audio_digest = final_audio_evidence.iter().next().unwrap().clone();
    assert_ne!(
        post_encode_audio_digest, pre_encode_audio_digest,
        "pre-encode PASS must never certify encoded audio"
    );
    assert_ne!(
        post_encode_audio_digest, manifest.final_artifact.sha256,
        "final-audio evidence must bind the decoded WAV, not the MP4 container digest"
    );

    let imported = coordinator
        .ledger()
        .iter()
        .filter(|entry| entry.origin == CompletionOrigin::ImportedReceipt)
        .map(|entry| entry.stage)
        .collect::<Vec<_>>();
    assert_eq!(
        imported,
        [Stage::ApplyAudio, Stage::RenderAudio, Stage::VerifyAudio]
    );
    assert_eq!(
        coordinator
            .ledger()
            .iter()
            .filter(|entry| entry.origin == CompletionOrigin::NativeDispatch)
            .count(),
        9
    );
    assert!(
        coordinator
            .ledger()
            .iter()
            .all(|entry| entry.status == ExecutionStatus::Completed)
    );

    let mp4 = fs::read_dir(&harness.output)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .find(|path| path.extension().and_then(|value| value.to_str()) == Some("mp4"))
        .expect("combined AV MP4");
    let decoded_wav = fs::read_dir(&harness.output)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.ends_with(".decoded.wav"))
        })
        .expect("post-encode decoded WAV");
    assert_eq!(
        Digest::parse(file_sha(&mp4)).unwrap(),
        manifest.final_artifact.sha256
    );
    assert_eq!(
        Digest::parse(file_sha(&decoded_wav)).unwrap(),
        post_encode_audio_digest
    );

    let publication = execute_publication_stages(&mut coordinator, &executor, &owner).await;
    let manifest_digest = canonical_digest(&manifest).unwrap();
    assert_eq!(publication.manifest_digest, manifest_digest);
    let candidate_manifest = harness.output.join(&publication.source_path);
    let published_manifest = harness.media.join(&publication.destination_path);
    let candidate_bytes = fs::read(&candidate_manifest).unwrap();
    let published_bytes = fs::read(&published_manifest).unwrap();
    assert_eq!(Digest::of_bytes(&candidate_bytes), manifest_digest);
    assert_eq!(Digest::of_bytes(&published_bytes), manifest_digest);
    assert_eq!(candidate_bytes, published_bytes);

    // A/B plans intentionally use the request-scoped HostSession owner. Project Graph
    // persistence is a separate trusted-host boundary and requires a durable principal.
    // Preserve the authenticated session, but bind C evidence to the host principal instead
    // of laundering the ephemeral Composition owner into durable graph authority.
    assert!(matches!(owner.principal, PrincipalBinding::HostSession));
    let graph_owner = Owner {
        session: owner.session.clone(),
        principal: PrincipalBinding::Named("os-user-v1:ci:composition-av".into()),
    };
    let graph_project = pg::ProjectId::new();
    let graph_access = pg::ProjectAccess::authorized(
        graph_owner.clone(),
        graph_project.clone(),
        None,
        true,
        Digest::of_bytes(b"combined-av-c14-grants"),
    )
    .unwrap();
    let mut graph =
        pg::ProjectGraph::new(graph_project.clone(), graph_owner.principal.clone()).unwrap();
    let audio_asset = c14_asset(&mut graph, &graph_access, "audio", "audio-master");
    let final_master_asset =
        c14_asset(&mut graph, &graph_access, "av-master", "verified-av-master");
    let candidate_manifest_asset =
        c14_asset(&mut graph, &graph_access, "manifest", "private-av-manifest");
    let published_manifest_asset = c14_asset(
        &mut graph,
        &graph_access,
        "manifest",
        "published-av-manifest",
    );

    let audio_revision = c14_observe(
        &mut graph,
        &graph_access,
        &graph_owner,
        &audio_asset,
        audio.master.sha256.clone(),
        EvidenceSource::DecodedMedia,
        "combined_av_audio_master",
        1,
    );
    let master_revision = c14_observe(
        &mut graph,
        &graph_access,
        &graph_owner,
        &final_master_asset,
        manifest.final_artifact.sha256.clone(),
        EvidenceSource::DecodedMedia,
        "combined_av_final_master",
        2,
    );
    let candidate_revision = c14_observe(
        &mut graph,
        &graph_access,
        &graph_owner,
        &candidate_manifest_asset,
        manifest_digest.clone(),
        EvidenceSource::FileRead,
        "combined_av_private_manifest",
        3,
    );
    let published_revision = c14_observe(
        &mut graph,
        &graph_access,
        &graph_owner,
        &published_manifest_asset,
        manifest_digest.clone(),
        EvidenceSource::FileRead,
        "combined_av_published_manifest",
        4,
    );

    let mux_descriptor = Digest::parse(
        descriptor_digest(&executor.describe("driver.mlt-video.av.mux").unwrap()).unwrap(),
    )
    .unwrap();
    let mux_runtime = runtime_digest(&harness.mlt_exe);
    let mux_receipt = pg::ExecutionReceipt {
        version: pg::SCHEMA_VERSION,
        id: pg::ReceiptId::new(),
        derivation: pg::DerivationId::new(),
        project: graph_project.clone(),
        owner: graph_owner.clone(),
        request_id: "c14-mux".into(),
        operation: pg::OperationIdentity {
            capability: "driver.mlt-video.av.mux".into(),
            descriptor: mux_descriptor.clone(),
            runtime: mux_runtime.clone(),
            plan: manifest.av_plan_digest.clone(),
            parameters: canonical_digest(&manifest.delivery).unwrap(),
            recipe: None,
        },
        source_base: audio_revision.observation.base.clone(),
        inputs: vec![audio_revision.pin.clone()],
        outputs: vec![master_revision.pin.clone()],
        determinants: vec![
            pg::Determinant {
                class: pg::DependencyClass::Contract,
                key: "c14-mux-motion-verification".into(),
                digest: canonical_digest(&manifest.motion_verification).unwrap(),
            },
            pg::Determinant {
                class: pg::DependencyClass::Contract,
                key: "c14-mux-audio-verification".into(),
                digest: canonical_digest(&manifest.audio_verification).unwrap(),
            },
            pg::Determinant {
                class: pg::DependencyClass::Contract,
                key: "c14-mux-final-audio-verification".into(),
                digest: canonical_digest(&manifest.final_audio_verification).unwrap(),
            },
            pg::Determinant {
                class: pg::DependencyClass::Contract,
                key: "c14-mux-sync-report".into(),
                digest: canonical_digest(&manifest.sync).unwrap(),
            },
            pg::Determinant {
                class: pg::DependencyClass::External,
                key: "c14-mux-agent-b-source".into(),
                digest: Digest::of_bytes(B_AUDIO_SHA.as_bytes()),
            },
        ],
        coverage: pg::Coverage::unknown(),
        verification: c14_byte_verification(
            manifest.av_plan_digest.clone(),
            &master_revision,
            "c14-final-master-byte-readback",
        ),
        completed_unix_ms: 5,
    };
    let mux_adapter = pg::ReceiptAdapter::registered(
        mux_receipt.operation.capability.clone(),
        mux_descriptor,
        mux_runtime,
    )
    .unwrap();
    graph
        .accept_receipt(
            &graph_access,
            mux_adapter
                .admit(
                    &graph_owner,
                    &mux_receipt.request_id.clone(),
                    mux_receipt.clone(),
                )
                .unwrap(),
        )
        .unwrap();

    let handoff_descriptor =
        Digest::parse(descriptor_digest(&executor.describe("artifact.handoff").unwrap()).unwrap())
            .unwrap();
    let handoff_runtime = Digest::of_bytes(b"semwright-artifact-handoff-filesystem-v1");
    let mut publication_source = candidate_revision.observation.base.0.clone();
    publication_source.extend(master_revision.observation.base.0.clone());
    let publication_receipt = pg::ExecutionReceipt {
        version: pg::SCHEMA_VERSION,
        id: pg::ReceiptId::new(),
        derivation: pg::DerivationId::new(),
        project: graph_project.clone(),
        owner: graph_owner.clone(),
        request_id: "c14-publication".into(),
        operation: pg::OperationIdentity {
            capability: "artifact.handoff".into(),
            descriptor: handoff_descriptor.clone(),
            runtime: handoff_runtime.clone(),
            plan: manifest.av_plan_digest.clone(),
            parameters: canonical_digest(&publication).unwrap(),
            recipe: None,
        },
        source_base: BaseStateSet(publication_source),
        inputs: vec![candidate_revision.pin.clone(), master_revision.pin.clone()],
        outputs: vec![published_revision.pin.clone()],
        determinants: vec![
            pg::Determinant {
                class: pg::DependencyClass::Contract,
                key: "c14-publish-manifest".into(),
                digest: manifest_digest.clone(),
            },
            pg::Determinant {
                class: pg::DependencyClass::Bytes,
                key: "c14-publish-final-master".into(),
                digest: manifest.final_artifact.sha256.clone(),
            },
            pg::Determinant {
                class: pg::DependencyClass::Contract,
                key: "c14-publish-cue".into(),
                digest: manifest.cue_digest.clone(),
            },
        ],
        coverage: pg::Coverage::unknown(),
        verification: c14_byte_verification(
            manifest.av_plan_digest.clone(),
            &published_revision,
            "c14-published-manifest-byte-readback",
        ),
        completed_unix_ms: 6,
    };
    let publication_adapter = pg::ReceiptAdapter::registered(
        publication_receipt.operation.capability.clone(),
        handoff_descriptor,
        handoff_runtime,
    )
    .unwrap();
    graph
        .accept_receipt(
            &graph_access,
            publication_adapter
                .admit(
                    &graph_owner,
                    &publication_receipt.request_id.clone(),
                    publication_receipt.clone(),
                )
                .unwrap(),
        )
        .unwrap();

    let mut determinants = mux_receipt.required_determinants();
    determinants.extend(publication_receipt.required_determinants());
    graph
        .observe_determinants(&graph_access, determinants)
        .unwrap();
    assert_eq!(
        graph
            .inspect(&graph_access, &published_manifest_asset)
            .unwrap()
            .knowledge
            .divergence,
        pg::Divergence::Clean
    );

    // Deliberate harness fault: edit A's published pointer after successful publication.
    // C observes/reconciles it; C never owns or rewrites the publication protocol.
    fs::write(&published_manifest, br#"{"external_edit":true}"#).unwrap();
    let external_bytes = fs::read(&published_manifest).unwrap();
    c14_observe(
        &mut graph,
        &graph_access,
        &graph_owner,
        &published_manifest_asset,
        Digest::of_bytes(&external_bytes),
        EvidenceSource::FileRead,
        "combined_av_published_manifest",
        7,
    );
    assert_eq!(
        graph
            .inspect(&graph_access, &published_manifest_asset)
            .unwrap()
            .knowledge
            .divergence,
        pg::Divergence::Diverged
    );
    fs::write(&published_manifest, &published_bytes).unwrap();
    c14_observe(
        &mut graph,
        &graph_access,
        &graph_owner,
        &published_manifest_asset,
        manifest_digest.clone(),
        EvidenceSource::FileRead,
        "combined_av_published_manifest",
        8,
    );
    assert_eq!(
        graph
            .inspect(&graph_access, &published_manifest_asset)
            .unwrap()
            .knowledge
            .divergence,
        pg::Divergence::Clean
    );

    let evidence_root = std::env::var_os("GITHUB_WORKSPACE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("verification/composition-av");
    let c14_evidence = evidence_root.join("c14-project-graph.json");
    fs::create_dir_all(&evidence_root).unwrap();
    fs::write(
        &c14_evidence,
        serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "classification": "C14_PROJECT_GRAPH_AV_AUDIO_INTEGRATION",
            "source_sha": std::env::var("GITHUB_SHA").ok(),
            "project_graph_source_sha": "77b34d8abad50f242c4c8494e280fe82d5cbcf55",
            "audio_source_sha": B_AUDIO_SHA,
            "project": graph_project.as_str(),
            "mux_receipt": mux_receipt.id.as_str(),
            "publication_receipt": publication_receipt.id.as_str(),
            "audio_asset": audio_asset.as_str(),
            "final_master_asset": final_master_asset.as_str(),
            "published_manifest_asset": published_manifest_asset.as_str(),
            "pre_encode_audio_sha256": pre_encode_audio_digest.as_str(),
            "post_encode_audio_sha256": post_encode_audio_digest.as_str(),
            "master_mp4_sha256": manifest.final_artifact.sha256.as_str(),
            "publication_manifest_sha256": manifest_digest.as_str(),
            "publication_pointer": publication.destination_path,
            "broker_publication": true,
            "external_edit_diverged": true,
            "exact_restore_clean": true,
            "coverage_complete": false,
            "project_graph_ready": false,
            "r16_closed": false
        }))
        .unwrap(),
    )
    .unwrap();

    let evidence = evidence_root.join("combined-native.json");
    fs::write(
        &evidence,
        serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "classification": "COMBINED_A_B_NATIVE_AV_CANDIDATE",
            "source_sha": std::env::var("GITHUB_SHA").ok(),
            "composition_source_sha": "7ab43f99f4cc62be2a9b0ce9ce1155283a429768",
            "audio_source_sha": B_AUDIO_SHA,
            "av_plan_sha256": manifest.av_plan_digest.as_str(),
            "pre_encode_audio_sha256": pre_encode_audio_digest.as_str(),
            "post_encode_audio_sha256": post_encode_audio_digest.as_str(),
            "master_mp4_sha256": manifest.final_artifact.sha256.as_str(),
            "audio_pre_encode_pass": true,
            "audio_post_encode_pass": true,
            "sync_full_scan_pass": true,
            "sync_exhaustive": manifest.sync.exhaustive,
            "sync_observations": manifest.sync.observations,
            "imported_audio_stages": imported,
            "native_dispatch_stages": coordinator
                .ledger()
                .iter()
                .filter(|entry| entry.origin == CompletionOrigin::NativeDispatch)
                .map(|entry| format!("{:?}", entry.stage))
                .collect::<Vec<_>>(),
            "r16_closed": false,
            "promotional_video": false
        }))
        .unwrap(),
    )
    .unwrap();

    broker.shutdown().await;
}
