use crate::{
    faust::{
        INSTRUMENT_TRANSLATOR_VERSION, TRANSLATOR_VERSION, VOICE_POLICY, translate,
        translate_instrument, translate_sample_player,
    },
    runtime::{MidiRuntimeEvent, PolyRenderSpec, Runtime, SampleRenderSpec},
};
use async_trait::async_trait;
use semwright_audio_domain::{
    backend::{BackendContract, BackendIdentity, ProjectionFidelity},
    model::{AudioProfile, AudioProject, MidiEvent, MidiPhrase, Sample, Synth},
    presets::{self, SfxPreset},
    render::{
        AudioFormat, BitDepth, DitherPolicy, RENDER_CONTRACT_VERSION, RenderIntent, RenderSource,
        ResampleQuality,
    },
    support::{AudioOperation, OperationSupport},
    time::{SampleRange, SampleRate},
};
use semwright_driver_sdk::{
    Capability, Driver, DriverExecutionContext, DriverInterfaces, descriptor_digest,
};
use semwright_types::{CommandDescriptor, Error, ErrorCode, Idempotency, Result, Risk};
use serde_json::{Value, json};

const DRIVER_ID: &str = "faust-audio";
const DRIVER_SCOPE: &str = "driver:faust-audio";
const MAX_SYNTH_JSON: usize = 524_288;
const MAX_SAMPLE_JSON: usize = 262_144;
const MAX_MIDI_JSON: usize = 1_048_576;

pub struct FaustAudioDriver {
    runtime: Option<Runtime>,
    runtime_reason: String,
}

impl FaustAudioDriver {
    pub fn production() -> Result<Self> {
        match Runtime::load_production() {
            Ok(Some(runtime)) => Ok(Self {
                runtime: Some(runtime),
                runtime_reason: "owner-pinned Faust runtime is available".into(),
            }),
            Ok(None) => Ok(Self {
                runtime: None,
                runtime_reason:
                    "owner-pinned Faust interpreter and faust-libraries mount are absent".into(),
            }),
            Err(error) => Ok(Self {
                runtime: None,
                runtime_reason: format!("{}", error),
            }),
        }
    }

    fn contract(&self) -> Result<BackendContract> {
        BackendContract::from_support(
            BackendIdentity {
                backend_id: "faust".into(),
                backend_version: None,
                adapter_id: format!("semwright-faust/{TRANSLATOR_VERSION}"),
            },
            ProjectionFidelity::Exact,
            |operation| {
                let support = match operation {
                    AudioOperation::SynthCreate
                    | AudioOperation::SynthRemove
                    | AudioOperation::SignalAdd
                    | AudioOperation::SignalRemove
                    | AudioOperation::SignalConnect
                    | AudioOperation::SignalDisconnect
                    | AudioOperation::SfxPresetMaterialize
                    | AudioOperation::RenderPlan
                    | AudioOperation::RenderStart => OperationSupport::RenderOnly,
                    _ => OperationSupport::Unsupported,
                };
                let reason = (support == OperationSupport::Unsupported).then(|| {
                    "Faust backend is DSP/render oriented, not a DAW/session backend".into()
                });
                (support, reason)
            },
        )
        .map_err(map_domain)
    }

    fn sample_from_synth(
        &self,
        synth: &Synth,
        sample: &Sample,
        sample_rate: u32,
        duration_frames: u64,
        channels: u16,
    ) -> Result<crate::faust::SampleProgram> {
        translate_sample_player(
            synth,
            sample,
            SampleRate::new(sample_rate).map_err(map_domain)?,
            duration_frames,
            channels,
        )
        .map_err(map_domain)
    }

    fn instrument_from_synth(
        &self,
        synth: &Synth,
        sample_rate: u32,
        channels: u16,
        reference_midi_note: u8,
    ) -> Result<crate::faust::InstrumentProgram> {
        translate_instrument(
            synth,
            SampleRate::new(sample_rate).map_err(map_domain)?,
            channels,
            reference_midi_note,
        )
        .map_err(map_domain)
    }

    fn source_from_synth(
        &self,
        synth: &Synth,
        sample_rate: u32,
        duration_frames: u64,
        channels: u16,
    ) -> Result<crate::faust::FaustProgram> {
        translate(
            synth,
            SampleRate::new(sample_rate).map_err(map_domain)?,
            duration_frames,
            channels,
        )
        .map_err(map_domain)
    }
}

fn map_domain(error: semwright_audio_domain::Error) -> Error {
    let code = match error.code {
        "Unsupported" => ErrorCode::Unsupported,
        "Unavailable" => ErrorCode::Unavailable,
        "PermissionDenied" => ErrorCode::PermissionDenied,
        "ConsentRequired" => ErrorCode::ConsentRequired,
        "PolicyDenied" => ErrorCode::PolicyDenied,
        "NotFound" => ErrorCode::NotFound,
        "StaleReference" => ErrorCode::StaleReference,
        "Timeout" => ErrorCode::Timeout,
        "InvalidArgument" => ErrorCode::InvalidArgument,
        "SandboxDenied" => ErrorCode::SandboxDenied,
        "Conflict" => ErrorCode::Conflict,
        "Cancelled" => ErrorCode::Cancelled,
        "ProtocolMismatch" => ErrorCode::ProtocolMismatch,
        "ResourceExhausted" => ErrorCode::ResourceExhausted,
        _ => ErrorCode::BackendFailed,
    };
    let mut value = Error::new(code, error.message);
    value.outcome_known = error.outcome_known;
    value
}

#[derive(Clone, Copy)]
struct Op {
    name: &'static str,
    description: &'static str,
    input: fn() -> Value,
    output: fn() -> Value,
    risk: Risk,
    idempotency: Idempotency,
}

pub fn capability_catalog() -> Vec<Capability> {
    let ops = [
        Op {
            name: "doctor",
            description: "Inspect deterministic Faust runtime availability without executing DSP",
            input: empty_schema,
            output: doctor_schema,
            risk: Risk::ReadOnly,
            idempotency: Idempotency::ReadOnly,
        },
        Op {
            name: "runtime.probe",
            description: "Execute only the pinned sealed Faust helper version probe through Driver Host confinement",
            input: empty_schema,
            output: runtime_probe_schema,
            risk: Risk::ReadOnly,
            idempotency: Idempotency::ReadOnly,
        },
        Op {
            name: "backend.contract",
            description: "Inspect the backend-neutral audio operation support contract",
            input: empty_schema,
            output: contract_schema,
            risk: Risk::ReadOnly,
            idempotency: Idempotency::ReadOnly,
        },
        Op {
            name: "sfx.preset.list",
            description: "List curated deterministic semantic SFX presets",
            input: empty_schema,
            output: preset_list_schema,
            risk: Risk::ReadOnly,
            idempotency: Idempotency::ReadOnly,
        },
        Op {
            name: "sfx.source",
            description: "Translate one curated SFX preset to bounded deterministic Faust source",
            input: preset_source_input_schema,
            output: source_output_schema,
            risk: Risk::ReadOnly,
            idempotency: Idempotency::ReadOnly,
        },
        Op {
            name: "synth.source",
            description: "Translate typed semantic Synth JSON to bounded deterministic Faust source",
            input: synth_input_schema,
            output: source_output_schema,
            risk: Risk::ReadOnly,
            idempotency: Idempotency::ReadOnly,
        },
        Op {
            name: "synth.validate",
            description: "Compile generated semantic Faust source with the owner-pinned compiler",
            input: synth_input_schema,
            output: validate_output_schema,
            risk: Risk::Mutating,
            idempotency: Idempotency::NonIdempotent,
        },
        Op {
            name: "sfx.render",
            description: "Render a curated semantic SFX preset to a new WAV or FLAC artifact",
            input: preset_render_input_schema,
            output: render_output_schema,
            risk: Risk::Mutating,
            idempotency: Idempotency::NonIdempotent,
        },
        Op {
            name: "synth.render",
            description: "Render typed semantic Synth JSON to a new WAV or FLAC artifact",
            input: synth_render_input_schema,
            output: render_output_schema,
            risk: Risk::Mutating,
            idempotency: Idempotency::NonIdempotent,
        },
        Op {
            name: "sample.render",
            description: "Render one hash-pinned owner-granted WAV/FLAC Sample through a typed monophonic Faust graph with exact-rate mono-average input mapping",
            input: sample_render_input_schema,
            output: sample_render_output_schema,
            risk: Risk::Mutating,
            idempotency: Idempotency::NonIdempotent,
        },
        Op {
            name: "instrument.render",
            description: "Render a typed semantic Synth and MidiPhrase through the official Faust polyphonic interpreter with sample-frame event timing",
            input: instrument_render_input_schema,
            output: instrument_render_output_schema,
            risk: Risk::Mutating,
            idempotency: Idempotency::NonIdempotent,
        },
    ];
    ops.into_iter()
        .map(|op| Capability {
            descriptor: CommandDescriptor {
                name: format!("driver.faust-audio.{}", op.name),
                version: "1".into(),
                description: op.description.into(),
                input_schema: (op.input)(),
                output_schema: (op.output)(),
                requires: vec![DRIVER_SCOPE.into()],
                risk: op.risk,
                idempotency: op.idempotency,
                timeout_ms: if op.name.ends_with(".render") {
                    180_000
                } else {
                    30_000
                },
                dry_run: op.risk == Risk::ReadOnly,
                interactive_consent: false,
                backends: vec![DRIVER_SCOPE.into()],
            },
            aliases: vec![],
            tags: vec!["audio".into(), "faust".into(), "deterministic".into()],
            object_types: match op.name {
                "sample.render" => vec!["audio-synth".into(), "audio-sample".into()],
                "instrument.render" => {
                    vec!["audio-synth".into(), "audio-midi-phrase".into()]
                }
                _ => vec!["audio-synth".into()],
            },
        })
        .collect()
}

fn capability(command: &str) -> Result<Capability> {
    capability_catalog()
        .into_iter()
        .find(|value| value.descriptor.name == command)
        .ok_or_else(|| Error::new(ErrorCode::NotFound, "Faust audio capability not registered"))
}

#[async_trait]
impl Driver for FaustAudioDriver {
    fn id(&self) -> &str {
        DRIVER_ID
    }
    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    async fn capabilities(&mut self) -> Result<Vec<Capability>> {
        Ok(capability_catalog())
    }

    fn interfaces(&self) -> DriverInterfaces {
        DriverInterfaces {
            cooperative_cancellation: true,
            progress: true,
            artifacts: true,
            host_tools: std::env::var_os("SEMWRIGHT_DRIVER_HOST_TOOLS").is_some(),
            health: true,
            ..Default::default()
        }
    }
    async fn execute(&mut self, command: &str, digest: &str, args: Value) -> Result<Value> {
        self.execute_inner(command, digest, args, None).await
    }
    async fn execute_with_context(
        &mut self,
        command: &str,
        digest: &str,
        args: Value,
        context: DriverExecutionContext,
    ) -> Result<Value> {
        context.check_cancelled()?;
        self.execute_inner(command, digest, args, Some(context))
            .await
    }
    async fn health(&mut self) -> Result<Value> {
        let version = self.runtime.as_ref().map(Runtime::version);
        Ok(json!({
            "healthy": true,
            "runtime_available": self.runtime.is_some(),
            "runtime_reason": self.runtime_reason,
            "faust_version": version,
            "translator_version": TRANSLATOR_VERSION,
            "arbitrary_faust_source": false,
            "arbitrary_compiler_flags": false,
            "shell_execution": false,
            "network": false
        }))
    }
}

impl FaustAudioDriver {
    async fn execute_inner(
        &mut self,
        command: &str,
        descriptor_sha256: &str,
        args: Value,
        context: Option<DriverExecutionContext>,
    ) -> Result<Value> {
        let cap = capability(command)?;
        if descriptor_digest(&cap.descriptor)? != descriptor_sha256 {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Faust audio capability descriptor changed",
            ));
        }
        match command.strip_prefix("driver.faust-audio.") {
            Some("doctor") => self.health().await,
            Some("runtime.probe") => {
                let runtime = self.runtime.as_ref().ok_or_else(|| {
                    Error::new(
                        ErrorCode::Unavailable,
                        "Pinned Faust runtime is unavailable",
                    )
                })?;
                let context = context.as_ref().ok_or_else(|| {
                    Error::new(
                        ErrorCode::Unsupported,
                        "Driver Host execution context is required",
                    )
                })?;
                let probe = runtime.probe(context).await?;
                Ok(json!({
                    "runtime_available": probe["runtime_available"],
                    "compiler_version": probe["compiler_version"],
                    "sealed_helper_executed": probe["sealed_helper_executed"],
                    "library_mount": probe["library_mount"],
                    "stdlib_regular": probe["stdlib_regular"],
                    "interpreter_compile": probe["interpreter_compile"],
                    "diagnostic_class": probe["diagnostic_class"],
                    "diagnostic_prefix": probe["diagnostic_prefix"]
                }))
            }
            Some("backend.contract") => {
                let contract = self.contract()?;
                Ok(json!({
                    "contract_version": contract.contract_version,
                    "backend_id": contract.identity.backend_id,
                    "adapter_id": contract.identity.adapter_id,
                    "semantic_model_version": contract.semantic_model_version,
                    "projection_fidelity": "exact",
                    "operations": contract.operations.iter().map(|value| json!({
                        "operation": value.operation.as_str(),
                        "support": support_name(value.support),
                        "reason": value.reason,
                    })).collect::<Vec<_>>()
                }))
            }
            Some("sfx.preset.list") => Ok(json!({
                "presets": SfxPreset::ALL.iter().map(|value| value.as_str()).collect::<Vec<_>>(),
                "deterministic": true
            })),
            Some("sfx.source") => {
                let request = SourceRequest::from_preset(&args)?;
                let program = self.source_from_synth(
                    &request.synth,
                    request.sample_rate,
                    request.duration_frames,
                    request.channels,
                )?;
                Ok(source_json(&program))
            }
            Some("synth.source") => {
                let request = SourceRequest::from_synth(&args)?;
                let program = self.source_from_synth(
                    &request.synth,
                    request.sample_rate,
                    request.duration_frames,
                    request.channels,
                )?;
                Ok(source_json(&program))
            }
            Some("synth.validate") => {
                let request = SourceRequest::from_synth(&args)?;
                let program = self.source_from_synth(
                    &request.synth,
                    request.sample_rate,
                    request.duration_frames,
                    request.channels,
                )?;
                let runtime = self.runtime.as_ref().ok_or_else(|| {
                    Error::new(
                        ErrorCode::Unavailable,
                        "Pinned Faust runtime is unavailable",
                    )
                })?;
                runtime
                    .validate_program(
                        context.as_ref().ok_or_else(|| {
                            Error::new(
                                ErrorCode::Unsupported,
                                "Driver Host execution context is required",
                            )
                        })?,
                        &program,
                    )
                    .await?;
                Ok(json!({
                    "valid": true,
                    "source_sha256": program.source_sha256,
                    "translator_version": TRANSLATOR_VERSION,
                    "compiler_version": runtime.version()
                }))
            }
            Some("sfx.render") => {
                let request = RenderRequest::from_preset(&args)?;
                self.render_request(
                    request,
                    context.as_ref().ok_or_else(|| {
                        Error::new(
                            ErrorCode::Unsupported,
                            "Driver Host execution context is required",
                        )
                    })?,
                )
                .await
            }
            Some("synth.render") => {
                let request = RenderRequest::from_synth(&args)?;
                self.render_request(
                    request,
                    context.as_ref().ok_or_else(|| {
                        Error::new(
                            ErrorCode::Unsupported,
                            "Driver Host execution context is required",
                        )
                    })?,
                )
                .await
            }
            Some("sample.render") => {
                let request = SampleRenderRequest::from_value(&args)?;
                self.render_sample_request(
                    request,
                    context.as_ref().ok_or_else(|| {
                        Error::new(
                            ErrorCode::Unsupported,
                            "Driver Host execution context is required",
                        )
                    })?,
                )
                .await
            }
            Some("instrument.render") => {
                let request = InstrumentRenderRequest::from_value(&args)?;
                self.render_instrument_request(
                    request,
                    context.as_ref().ok_or_else(|| {
                        Error::new(
                            ErrorCode::Unsupported,
                            "Driver Host execution context is required",
                        )
                    })?,
                )
                .await
            }
            _ => Err(Error::new(
                ErrorCode::NotFound,
                "Faust audio capability not registered",
            )),
        }
    }

    async fn render_sample_request(
        &self,
        request: SampleRenderRequest,
        context: &DriverExecutionContext,
    ) -> Result<Value> {
        let runtime = self.runtime.as_ref().ok_or_else(|| {
            Error::new(
                ErrorCode::Unavailable,
                "Pinned Faust runtime is unavailable",
            )
        })?;
        let sample_program = self.sample_from_synth(
            &request.synth,
            &request.sample,
            request.sample_rate,
            request.duration_frames,
            request.channels,
        )?;
        let artifact = runtime
            .render_sample(
                context,
                &sample_program.program,
                &request.sample,
                &request.expected_sha256,
                SampleRenderSpec {
                    sample_rate: request.sample_rate,
                    frames: request.duration_frames,
                    channels: request.channels,
                    format: request.format,
                    bit_depth: request.bit_depth,
                    file_name: request.output_file.clone(),
                    looped: sample_program.looped,
                },
            )
            .await?;
        context.report_progress(
            semwright_types::JobProgress {
                completed: artifact.native.frames,
                total: Some(artifact.native.frames),
                message: Some("Native Faust sample-backed render published".into()),
            },
            vec![semwright_types::JobArtifact {
                name: "audio".into(),
                reference: format!("artifact:sha256:{}", artifact.sha256),
                media_type: Some(
                    match artifact.format {
                        AudioFormat::Wav => "audio/wav",
                        AudioFormat::Flac => "audio/flac",
                    }
                    .into(),
                ),
                sha256: Some(artifact.sha256.clone()),
                bytes: Some(artifact.bytes),
            }],
        )?;
        Ok(json!({
            "artifact":{
                "file":artifact.file_name,
                "sha256":artifact.sha256,
                "bytes":artifact.bytes,
                "format":match artifact.format { AudioFormat::Wav=>"wav", AudioFormat::Flac=>"flac" }
            },
            "input":{
                "sample_id":sample_program.sample_id,
                "sha256":artifact.input_sha256,
                "looped":sample_program.looped,
                "resampling":"exact_only",
                "channel_mapping":"mono_average"
            },
            "source_sha256":sample_program.program.source_sha256,
            "translator_version":TRANSLATOR_VERSION,
            "deterministic":true,
            "native_receipt":artifact.native,
            "libraries_sha256":artifact.libraries_sha256
        }))
    }

    async fn render_instrument_request(
        &self,
        request: InstrumentRenderRequest,
        context: &DriverExecutionContext,
    ) -> Result<Value> {
        let runtime = self.runtime.as_ref().ok_or_else(|| {
            Error::new(
                ErrorCode::Unavailable,
                "Pinned Faust runtime is unavailable",
            )
        })?;
        let instrument = self.instrument_from_synth(
            &request.synth,
            request.sample_rate,
            request.channels,
            request.reference_midi_note,
        )?;
        let mut project = AudioProject::new(AudioProfile {
            sample_rate: SampleRate(request.sample_rate),
            channels: request.channels,
            ..AudioProfile::default()
        })
        .map_err(map_domain)?;
        project
            .synths
            .insert(request.synth.id.clone(), request.synth.clone());
        project.midi_phrases.push(request.phrase.clone());
        project.validate().map_err(map_domain)?;
        let events = request.runtime_events()?;
        let last_note_off = events
            .iter()
            .filter_map(|event| match event {
                MidiRuntimeEvent::NoteOff { frame, .. } => Some(*frame),
                _ => None,
            })
            .max()
            .ok_or_else(|| Error::invalid("Polyphonic render requires at least one MIDI note"))?;
        let required_end = last_note_off
            .checked_add(instrument.tail_frames)
            .ok_or_else(|| {
                Error::new(ErrorCode::ResourceExhausted, "MIDI tail horizon overflow")
            })?;
        if required_end > request.duration_frames {
            return Err(Error::invalid(
                "duration_frames must include the final note-off plus the deterministic instrument tail",
            ));
        }
        let artifact = runtime
            .render_poly(
                context,
                &instrument.program,
                &events,
                PolyRenderSpec {
                    sample_rate: request.sample_rate,
                    frames: request.duration_frames,
                    channels: request.channels,
                    format: request.format,
                    bit_depth: request.bit_depth,
                    file_name: request.output_file.clone(),
                    polyphony: request.synth.polyphony,
                    tail_frames: instrument.tail_frames,
                },
            )
            .await?;
        context.report_progress(
            semwright_types::JobProgress {
                completed: artifact.native.frames,
                total: Some(artifact.native.frames),
                message: Some("Native Faust polyphonic MIDI render published".into()),
            },
            vec![semwright_types::JobArtifact {
                name: "audio".into(),
                reference: format!("artifact:sha256:{}", artifact.sha256),
                media_type: Some(
                    match artifact.format {
                        AudioFormat::Wav => "audio/wav",
                        AudioFormat::Flac => "audio/flac",
                    }
                    .into(),
                ),
                sha256: Some(artifact.sha256.clone()),
                bytes: Some(artifact.bytes),
            }],
        )?;
        Ok(json!({
            "artifact":{
                "file":artifact.file_name,
                "sha256":artifact.sha256,
                "bytes":artifact.bytes,
                "format":match artifact.format { AudioFormat::Wav=>"wav", AudioFormat::Flac=>"flac" }
            },
            "source_sha256":instrument.program.source_sha256,
            "translator_version":TRANSLATOR_VERSION,
            "instrument_translator_version":INSTRUMENT_TRANSLATOR_VERSION,
            "reference_midi_note":instrument.reference_midi_note,
            "voice_policy":VOICE_POLICY,
            "schedule_sha256":artifact.schedule_sha256,
            "deterministic":true,
            "native_receipt":artifact.native,
            "libraries_sha256":artifact.libraries_sha256
        }))
    }

    async fn render_request(
        &self,
        request: RenderRequest,
        context: &DriverExecutionContext,
    ) -> Result<Value> {
        let runtime = self.runtime.as_ref().ok_or_else(|| {
            Error::new(
                ErrorCode::Unavailable,
                "Pinned Faust runtime is unavailable",
            )
        })?;
        let program = self.source_from_synth(
            &request.synth,
            request.sample_rate,
            request.duration_frames,
            request.channels,
        )?;
        context.check_cancelled()?;

        let profile = AudioProfile {
            sample_rate: SampleRate(request.sample_rate),
            channels: request.channels,
            ..AudioProfile::default()
        };
        let mut project = AudioProject::new(profile).map_err(map_domain)?;
        project
            .synths
            .insert(request.synth.id.clone(), request.synth.clone());
        project.validate().map_err(map_domain)?;
        let intent = RenderIntent {
            contract_version: RENDER_CONTRACT_VERSION,
            source: RenderSource::Synth {
                synth: request.synth.id.clone(),
            },
            range: Some(SampleRange::new(0, request.duration_frames).map_err(map_domain)?),
            format: request.format,
            sample_rate: SampleRate(request.sample_rate),
            channels: request.channels,
            bit_depth: request.bit_depth,
            normalize_lufs_milli: None,
            resample_quality: ResampleQuality::High,
            dither: DitherPolicy::None,
        };
        let artifact = runtime
            .render(context, &program, &intent, &project, &request.output_file)
            .await?;
        context.report_progress(
            semwright_types::JobProgress {
                completed: artifact.native.frames,
                total: Some(artifact.native.frames),
                message: Some("Native Faust render published".into()),
            },
            vec![semwright_types::JobArtifact {
                name: "audio".into(),
                reference: format!("artifact:sha256:{}", artifact.sha256),
                media_type: Some(
                    match artifact.format {
                        AudioFormat::Wav => "audio/wav",
                        AudioFormat::Flac => "audio/flac",
                    }
                    .into(),
                ),
                sha256: Some(artifact.sha256.clone()),
                bytes: Some(artifact.bytes),
            }],
        )?;
        Ok(json!({
            "artifact": {
                "file": artifact.file_name,
                "sha256": artifact.sha256,
                "bytes": artifact.bytes,
                "format": match artifact.format { AudioFormat::Wav => "wav", AudioFormat::Flac => "flac" }
            },
            "source_sha256": program.source_sha256,
            "translator_version": TRANSLATOR_VERSION,
            "deterministic": true,
            "native_receipt": artifact.native,
            "libraries_sha256": artifact.libraries_sha256
        }))
    }
}

struct SourceRequest {
    synth: Synth,
    sample_rate: u32,
    duration_frames: u64,
    channels: u16,
}
impl SourceRequest {
    fn from_synth(value: &Value) -> Result<Self> {
        let synth_text = required_str(value, "synth_json", MAX_SYNTH_JSON)?;
        let synth: Synth = serde_json::from_str(synth_text).map_err(|_| {
            Error::new(
                ErrorCode::InvalidArgument,
                "synth_json is not a valid semantic Synth",
            )
        })?;
        Ok(Self {
            synth,
            sample_rate: required_u32(value, "sample_rate")?,
            duration_frames: required_u64(value, "duration_frames")?,
            channels: required_u16(value, "channels")?,
        })
    }
    fn from_preset(value: &Value) -> Result<Self> {
        let sample_rate = required_u32(value, "sample_rate")?;
        let duration_frames = required_u64(value, "duration_frames")?;
        let seed = required_u64(value, "seed")?;
        let synth = presets::synth_for(
            parse_preset(required_str(value, "preset", 32)?)?,
            "preset",
            SampleRate::new(sample_rate).map_err(map_domain)?,
            duration_frames,
            seed,
        )
        .map_err(map_domain)?;
        Ok(Self {
            synth,
            sample_rate,
            duration_frames,
            channels: required_u16(value, "channels")?,
        })
    }
}

struct RenderRequest {
    synth: Synth,
    sample_rate: u32,
    duration_frames: u64,
    channels: u16,
    format: AudioFormat,
    bit_depth: BitDepth,
    output_file: String,
}
impl RenderRequest {
    fn from_synth(value: &Value) -> Result<Self> {
        let source = SourceRequest::from_synth(value)?;
        Self::finish(value, source, required_u64(value, "seed")?)
    }
    fn from_preset(value: &Value) -> Result<Self> {
        let source = SourceRequest::from_preset(value)?;
        Self::finish(value, source, required_u64(value, "seed")?)
    }
    fn finish(value: &Value, source: SourceRequest, _seed: u64) -> Result<Self> {
        let format = match required_str(value, "format", 16)? {
            "wav" => AudioFormat::Wav,
            "flac" => AudioFormat::Flac,
            _ => return Err(Error::invalid("format must be wav or flac")),
        };
        let bit_depth = match required_u64(value, "bit_depth")? {
            16 => BitDepth::Pcm16,
            24 => BitDepth::Pcm24,
            32 => BitDepth::Pcm32,
            _ => return Err(Error::invalid("bit_depth must be 16, 24 or 32")),
        };
        Ok(Self {
            synth: source.synth,
            sample_rate: source.sample_rate,
            duration_frames: source.duration_frames,
            channels: source.channels,
            format,
            bit_depth,
            output_file: required_str(value, "output_file", 240)?.to_owned(),
        })
    }
}

struct SampleRenderRequest {
    synth: Synth,
    sample: Sample,
    expected_sha256: String,
    sample_rate: u32,
    duration_frames: u64,
    channels: u16,
    format: AudioFormat,
    bit_depth: BitDepth,
    output_file: String,
}

impl SampleRenderRequest {
    fn from_value(value: &Value) -> Result<Self> {
        let synth: Synth = serde_json::from_str(required_str(value, "synth_json", MAX_SYNTH_JSON)?)
            .map_err(|_| Error::invalid("synth_json is not a valid semantic Synth"))?;
        let sample: Sample =
            serde_json::from_str(required_str(value, "sample_json", MAX_SAMPLE_JSON)?)
                .map_err(|_| Error::invalid("sample_json is not a valid semantic Sample"))?;
        let format = match required_str(value, "format", 16)? {
            "wav" => AudioFormat::Wav,
            "flac" => AudioFormat::Flac,
            _ => return Err(Error::invalid("format must be wav or flac")),
        };
        let bit_depth = match required_u64(value, "bit_depth")? {
            16 => BitDepth::Pcm16,
            24 => BitDepth::Pcm24,
            32 => BitDepth::Pcm32,
            _ => return Err(Error::invalid("bit_depth must be 16, 24 or 32")),
        };
        Ok(Self {
            synth,
            sample,
            expected_sha256: required_str(value, "expected_sha256", 64)?.to_owned(),
            sample_rate: required_u32(value, "sample_rate")?,
            duration_frames: required_u64(value, "duration_frames")?,
            channels: required_u16(value, "channels")?,
            format,
            bit_depth,
            output_file: required_str(value, "output_file", 240)?.to_owned(),
        })
    }
}

struct InstrumentRenderRequest {
    synth: Synth,
    phrase: MidiPhrase,
    reference_midi_note: u8,
    sample_rate: u32,
    duration_frames: u64,
    channels: u16,
    format: AudioFormat,
    bit_depth: BitDepth,
    output_file: String,
}

impl InstrumentRenderRequest {
    fn from_value(value: &Value) -> Result<Self> {
        let synth: Synth = serde_json::from_str(required_str(value, "synth_json", MAX_SYNTH_JSON)?)
            .map_err(|_| Error::invalid("synth_json is not a valid semantic Synth"))?;
        let phrase: MidiPhrase =
            serde_json::from_str(required_str(value, "midi_phrase_json", MAX_MIDI_JSON)?).map_err(
                |_| Error::invalid("midi_phrase_json is not a valid semantic MidiPhrase"),
            )?;
        if phrase.instrument_synth.as_deref() != Some(synth.id.as_str()) {
            return Err(Error::invalid(
                "MidiPhrase instrument_synth must reference the rendered Synth",
            ));
        }
        if phrase.events.len() > 50_000 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "MidiPhrase exceeds polyphonic runtime event budget",
            ));
        }
        let format = match required_str(value, "format", 16)? {
            "wav" => AudioFormat::Wav,
            "flac" => AudioFormat::Flac,
            _ => return Err(Error::invalid("format must be wav or flac")),
        };
        let bit_depth = match required_u64(value, "bit_depth")? {
            16 => BitDepth::Pcm16,
            24 => BitDepth::Pcm24,
            32 => BitDepth::Pcm32,
            _ => return Err(Error::invalid("bit_depth must be 16, 24 or 32")),
        };
        Ok(Self {
            synth,
            phrase,
            reference_midi_note: u8::try_from(required_u64(value, "reference_midi_note")?)
                .ok()
                .filter(|note| *note <= 127)
                .ok_or_else(|| Error::invalid("reference_midi_note must be 0..=127"))?,
            sample_rate: required_u32(value, "sample_rate")?,
            duration_frames: required_u64(value, "duration_frames")?,
            channels: required_u16(value, "channels")?,
            format,
            bit_depth,
            output_file: required_str(value, "output_file", 240)?.to_owned(),
        })
    }

    fn runtime_events(&self) -> Result<Vec<MidiRuntimeEvent>> {
        let mut events = Vec::with_capacity(self.phrase.events.len().saturating_mul(2));
        let mut notes = Vec::new();
        for event in &self.phrase.events {
            match event {
                MidiEvent::Note {
                    start,
                    duration_frames,
                    channel,
                    note,
                    velocity,
                    ..
                } => {
                    if *velocity == 0 {
                        return Err(Error::invalid(
                            "Polyphonic Faust note velocity must be nonzero",
                        ));
                    }
                    let end = start
                        .0
                        .checked_add(*duration_frames)
                        .filter(|end| *end <= self.duration_frames)
                        .ok_or_else(|| {
                            Error::invalid("MIDI note exceeds requested render duration")
                        })?;
                    notes.push((start.0, end, *note));
                    events.push(MidiRuntimeEvent::NoteOn {
                        frame: start.0,
                        channel: *channel,
                        note: *note,
                        velocity: *velocity,
                    });
                    events.push(MidiRuntimeEvent::NoteOff {
                        frame: end,
                        channel: *channel,
                        note: *note,
                        velocity: 0,
                    });
                }
                MidiEvent::Control {
                    frame,
                    channel,
                    controller,
                    value,
                    ..
                } => {
                    if frame.0 > self.duration_frames {
                        return Err(Error::invalid(
                            "MIDI control exceeds requested render duration",
                        ));
                    }
                    if !matches!(*controller, 120 | 123) || *channel != 0 {
                        return Err(Error::new(
                            ErrorCode::Unsupported,
                            "Faust polyphonic runtime maps only global MIDI CC 120/123 on channel 0",
                        ));
                    }
                    events.push(MidiRuntimeEvent::Control {
                        frame: frame.0,
                        channel: *channel,
                        controller: *controller,
                        value: *value,
                    });
                }
            }
        }
        if notes.is_empty() || events.len() > 100_000 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Polyphonic MIDI schedule has no notes or exceeds runtime budget",
            ));
        }
        notes.sort_unstable();
        let mut last_end_by_pitch = [0u64; 128];
        let mut seen_pitch = [false; 128];
        for (start, end, pitch) in notes {
            let index = usize::from(pitch);
            if seen_pitch[index] && start < last_end_by_pitch[index] {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "Overlapping notes of the same pitch are ambiguous in Faust dsp_poly keyOff semantics",
                ));
            }
            seen_pitch[index] = true;
            last_end_by_pitch[index] = end;
        }
        Ok(events)
    }
}

fn parse_preset(value: &str) -> Result<SfxPreset> {
    SfxPreset::ALL
        .iter()
        .copied()
        .find(|preset| preset.as_str() == value)
        .ok_or_else(|| Error::invalid("Unknown semantic SFX preset"))
}
fn required_str<'a>(value: &'a Value, key: &str, max: usize) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| {
            !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
        })
        .ok_or_else(|| Error::invalid(format!("Invalid {key}")))
}
fn required_u64(value: &Value, key: &str) -> Result<u64> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| Error::invalid(format!("Invalid {key}")))
}
fn required_u32(value: &Value, key: &str) -> Result<u32> {
    u32::try_from(required_u64(value, key)?).map_err(|_| Error::invalid(format!("Invalid {key}")))
}
fn required_u16(value: &Value, key: &str) -> Result<u16> {
    u16::try_from(required_u64(value, key)?).map_err(|_| Error::invalid(format!("Invalid {key}")))
}
fn support_name(value: OperationSupport) -> &'static str {
    match value {
        OperationSupport::SafeRoundtrip => "safe_roundtrip",
        OperationSupport::MetadataRisk => "metadata_risk",
        OperationSupport::RenderOnly => "render_only",
        OperationSupport::Unsupported => "unsupported",
    }
}
fn source_json(program: &crate::faust::FaustProgram) -> Value {
    json!({
        "source": program.source,
        "source_sha256": program.source_sha256,
        "translator_version": program.translator_version,
        "outputs": program.outputs,
        "arbitrary_source": false
    })
}

fn empty_schema() -> Value {
    json!({"type":"object","properties":{},"additionalProperties":false})
}
fn base_source_properties(include_preset: bool) -> serde_json::Map<String, Value> {
    let mut props = serde_json::Map::new();
    if include_preset {
        props.insert("preset".into(), json!({"type":"string","enum":["click","whoosh","impact","riser","sweep","notification"]}));
        props.insert(
            "seed".into(),
            json!({"type":"integer","minimum":0,"maximum":18446744073709551615u64}),
        );
    } else {
        props.insert(
            "synth_json".into(),
            json!({"type":"string","minLength":2,"maxLength":MAX_SYNTH_JSON}),
        );
    }
    props.insert(
        "sample_rate".into(),
        json!({"type":"integer","minimum":8000,"maximum":384000}),
    );
    props.insert(
        "duration_frames".into(),
        json!({"type":"integer","minimum":1,"maximum":33177600000u64}),
    );
    props.insert(
        "channels".into(),
        json!({"type":"integer","minimum":1,"maximum":16}),
    );
    props
}
fn preset_source_input_schema() -> Value {
    json!({"type":"object","properties":base_source_properties(true),"required":["preset","seed","sample_rate","duration_frames","channels"],"additionalProperties":false})
}
fn synth_input_schema() -> Value {
    json!({"type":"object","properties":base_source_properties(false),"required":["synth_json","sample_rate","duration_frames","channels"],"additionalProperties":false})
}
fn render_properties(include_preset: bool) -> serde_json::Map<String, Value> {
    let mut props = base_source_properties(include_preset);
    if !include_preset {
        props.insert(
            "seed".into(),
            json!({"type":"integer","minimum":0,"maximum":18446744073709551615u64}),
        );
    }
    props.insert(
        "format".into(),
        json!({"type":"string","enum":["wav","flac"]}),
    );
    props.insert(
        "bit_depth".into(),
        json!({"type":"integer","enum":[16,24,32]}),
    );
    props.insert("output_file".into(), json!({"type":"string","minLength":5,"maxLength":240,"pattern":"^[A-Za-z0-9][A-Za-z0-9._-]*\\.(wav|flac)$"}));
    props
}
fn preset_render_input_schema() -> Value {
    json!({"type":"object","properties":render_properties(true),"required":["preset","seed","sample_rate","duration_frames","channels","format","bit_depth","output_file"],"additionalProperties":false})
}
fn synth_render_input_schema() -> Value {
    json!({"type":"object","properties":render_properties(false),"required":["synth_json","seed","sample_rate","duration_frames","channels","format","bit_depth","output_file"],"additionalProperties":false})
}
fn sample_render_input_schema() -> Value {
    json!({
        "type":"object",
        "properties":{
            "synth_json":{"type":"string","minLength":2,"maxLength":MAX_SYNTH_JSON},
            "sample_json":{"type":"string","minLength":2,"maxLength":MAX_SAMPLE_JSON},
            "expected_sha256":{"type":"string","pattern":"^[0-9a-f]{64}$"},
            "sample_rate":{"type":"integer","minimum":8000,"maximum":192000},
            "duration_frames":{"type":"integer","minimum":1,"maximum":57600000},
            "channels":{"type":"integer","minimum":1,"maximum":16},
            "format":{"type":"string","enum":["wav","flac"]},
            "bit_depth":{"type":"integer","enum":[16,24,32]},
            "output_file":{"type":"string","minLength":5,"maxLength":240,"pattern":"^[A-Za-z0-9][A-Za-z0-9._-]*\\.(wav|flac)$"}
        },
        "required":["synth_json","sample_json","expected_sha256","sample_rate","duration_frames","channels","format","bit_depth","output_file"],
        "additionalProperties":false
    })
}

fn sample_render_output_schema() -> Value {
    json!({
        "type":"object",
        "properties":{
            "artifact":{"type":"object"},
            "input":{
                "type":"object",
                "properties":{
                    "sample_id":{"type":"string","minLength":1,"maxLength":128},
                    "sha256":{"type":"string","pattern":"^[0-9a-f]{64}$"},
                    "looped":{"type":"boolean"},
                    "resampling":{"const":"exact_only"},
                    "channel_mapping":{"const":"mono_average"}
                },
                "required":["sample_id","sha256","looped","resampling","channel_mapping"],
                "additionalProperties":false
            },
            "source_sha256":{"type":"string","pattern":"^[0-9a-f]{64}$"},
            "translator_version":{"const":TRANSLATOR_VERSION},
            "deterministic":{"const":true},
            "libraries_sha256":{"type":"string","pattern":"^[0-9a-f]{64}$"},
            "native_receipt":{
                "type":"object",
                "properties":{
                    "schema_version":{"const":1},
                    "frames":{"type":"integer","minimum":1,"maximum":57600000},
                    "sample_rate":{"type":"integer","minimum":8000,"maximum":192000},
                    "channels":{"type":"integer","minimum":1,"maximum":16},
                    "clipped_output_samples":{"type":"integer","minimum":0},
                    "compiler_version":{"enum":["2.37.3","2.70.3"]},
                    "engine":{"const":"faust-sample-interpreter"},
                    "dither":{"const":"none"},
                    "source_frames":{"type":"integer","minimum":1,"maximum":57600000},
                    "source_sample_rate":{"type":"integer","minimum":8000,"maximum":192000},
                    "source_channels":{"type":"integer","minimum":1,"maximum":64},
                    "looped":{"type":"boolean"},
                    "resampling":{"const":"exact_only"},
                    "channel_mapping":{"const":"mono_average"}
                },
                "required":["schema_version","frames","sample_rate","channels","clipped_output_samples","compiler_version","engine","dither","source_frames","source_sample_rate","source_channels","looped","resampling","channel_mapping"],
                "additionalProperties":false
            }
        },
        "required":["artifact","input","source_sha256","translator_version","deterministic","native_receipt","libraries_sha256"],
        "additionalProperties":false
    })
}

fn instrument_render_input_schema() -> Value {
    json!({
        "type":"object",
        "properties":{
            "synth_json":{"type":"string","minLength":2,"maxLength":MAX_SYNTH_JSON},
            "midi_phrase_json":{"type":"string","minLength":2,"maxLength":MAX_MIDI_JSON},
            "reference_midi_note":{"type":"integer","minimum":0,"maximum":127},
            "sample_rate":{"type":"integer","minimum":8000,"maximum":192000},
            "duration_frames":{"type":"integer","minimum":1,"maximum":57600000},
            "channels":{"type":"integer","minimum":1,"maximum":16},
            "format":{"type":"string","enum":["wav","flac"]},
            "bit_depth":{"type":"integer","enum":[16,24,32]},
            "output_file":{"type":"string","minLength":5,"maxLength":240,"pattern":"^[A-Za-z0-9][A-Za-z0-9._-]*\\.(wav|flac)$"}
        },
        "required":["synth_json","midi_phrase_json","reference_midi_note","sample_rate","duration_frames","channels","format","bit_depth","output_file"],
        "additionalProperties":false
    })
}

fn instrument_render_output_schema() -> Value {
    json!({
        "type":"object",
        "properties":{
            "artifact":{"type":"object","properties":{
                "file":{"type":"string","maxLength":240},
                "sha256":{"type":"string","pattern":"^[0-9a-f]{64}$"},
                "bytes":{"type":"integer","minimum":1},
                "format":{"type":"string","enum":["wav","flac"]}
            },"required":["file","sha256","bytes","format"],"additionalProperties":false},
            "source_sha256":{"type":"string","pattern":"^[0-9a-f]{64}$"},
            "translator_version":{"const":TRANSLATOR_VERSION},
            "instrument_translator_version":{"const":INSTRUMENT_TRANSLATOR_VERSION},
            "reference_midi_note":{"type":"integer","minimum":0,"maximum":127},
            "voice_policy":{"const":VOICE_POLICY},
            "schedule_sha256":{"type":"string","pattern":"^[0-9a-f]{64}$"},
            "deterministic":{"const":true},
            "libraries_sha256":{"type":"string","pattern":"^[0-9a-f]{64}$"},
            "native_receipt":{"type":"object","properties":{
                "schema_version":{"const":1},
                "frames":{"type":"integer","minimum":1,"maximum":57600000},
                "sample_rate":{"type":"integer","minimum":8000,"maximum":192000},
                "channels":{"type":"integer","minimum":1,"maximum":16},
                "clipped_input_samples":{"type":"integer","minimum":0},
                "compiler_version":{"enum":["2.37.3","2.70.3"]},
                "engine":{"const":"faust-poly-interpreter"},
                "dither":{"const":"none"},
                "polyphony":{"type":"integer","minimum":1,"maximum":64},
                "midi_events":{"type":"integer","minimum":1,"maximum":100000},
                "tail_frames":{"type":"integer","minimum":1},
                "voice_policy":{"const":VOICE_POLICY}
            },"required":["schema_version","frames","sample_rate","channels","clipped_input_samples","compiler_version","engine","dither","polyphony","midi_events","tail_frames","voice_policy"],"additionalProperties":false}
        },
        "required":["artifact","source_sha256","translator_version","instrument_translator_version","reference_midi_note","voice_policy","schedule_sha256","deterministic","native_receipt","libraries_sha256"],
        "additionalProperties":false
    })
}

fn source_output_schema() -> Value {
    json!({"type":"object","properties":{
        "source":{"type":"string","maxLength":1048576},
        "source_sha256":{"type":"string","pattern":"^[0-9a-f]{64}$"},
        "translator_version":{"const":TRANSLATOR_VERSION},
        "outputs":{"type":"integer","minimum":1,"maximum":16},
        "arbitrary_source":{"const":false}
    },"required":["source","source_sha256","translator_version","outputs","arbitrary_source"],"additionalProperties":false})
}
fn runtime_probe_schema() -> Value {
    json!({
        "type":"object",
        "properties":{
            "runtime_available":{"type":"boolean"},
            "compiler_version":{"type":"string","minLength":1,"maxLength":128},
            "sealed_helper_executed":{"type":"boolean"},
            "library_mount":{"type":"boolean"},
            "stdlib_regular":{"type":"boolean"},
            "interpreter_compile":{"type":"boolean"},
            "diagnostic_class":{"type":"string","minLength":1,"maxLength":64},
            "diagnostic_prefix":{"type":"string","maxLength":512}
        },
        "required":["runtime_available","compiler_version","sealed_helper_executed","library_mount","stdlib_regular","interpreter_compile","diagnostic_class","diagnostic_prefix"],
        "additionalProperties":false
    })
}

fn doctor_schema() -> Value {
    json!({"type":"object","properties":{
        "healthy":{"const":true},
        "runtime_available":{"type":"boolean"},
        "runtime_reason":{"type":"string","maxLength":2048},
        "faust_version":{"type":["string","null"],"maxLength":1024},
        "translator_version":{"const":TRANSLATOR_VERSION},
        "arbitrary_faust_source":{"const":false},
        "arbitrary_compiler_flags":{"const":false},
        "shell_execution":{"const":false},
        "network":{"const":false}
    },"required":["healthy","runtime_available","runtime_reason","faust_version","translator_version","arbitrary_faust_source","arbitrary_compiler_flags","shell_execution","network"],"additionalProperties":false})
}
fn contract_schema() -> Value {
    json!({"type":"object","properties":{
        "contract_version":{"const":1},
        "backend_id":{"const":"faust"},
        "adapter_id":{"type":"string","maxLength":128},
        "semantic_model_version":{"const":1},
        "projection_fidelity":{"const":"exact"},
        "operations":{"type":"array","minItems":AudioOperation::ALL.len(),"maxItems":AudioOperation::ALL.len(),"items":{"type":"object","properties":{
            "operation":{"type":"string","maxLength":128},
            "support":{"type":"string","enum":["safe_roundtrip","metadata_risk","render_only","unsupported"]},
            "reason":{"type":["string","null"],"maxLength":2048}
        },"required":["operation","support","reason"],"additionalProperties":false}}
    },"required":["contract_version","backend_id","adapter_id","semantic_model_version","projection_fidelity","operations"],"additionalProperties":false})
}
fn preset_list_schema() -> Value {
    json!({"type":"object","properties":{
        "presets":{"type":"array","minItems":6,"maxItems":6,"items":{"type":"string","enum":["click","whoosh","impact","riser","sweep","notification"]}},
        "deterministic":{"const":true}
    },"required":["presets","deterministic"],"additionalProperties":false})
}
fn validate_output_schema() -> Value {
    json!({"type":"object","properties":{
        "valid":{"const":true},
        "source_sha256":{"type":"string","pattern":"^[0-9a-f]{64}$"},
        "translator_version":{"const":TRANSLATOR_VERSION},
        "compiler_version":{"type":"string","maxLength":1024}
    },"required":["valid","source_sha256","translator_version","compiler_version"],"additionalProperties":false})
}
fn render_output_schema() -> Value {
    json!({"type":"object","properties":{
        "artifact":{"type":"object","properties":{
            "file":{"type":"string","maxLength":240},
            "sha256":{"type":"string","pattern":"^[0-9a-f]{64}$"},
            "bytes":{"type":"integer","minimum":1},
            "format":{"type":"string","enum":["wav","flac"]}
        },"required":["file","sha256","bytes","format"],"additionalProperties":false},
        "source_sha256":{"type":"string","pattern":"^[0-9a-f]{64}$"},
        "translator_version":{"const":TRANSLATOR_VERSION},
        "deterministic":{"const":true},
        "libraries_sha256":{"type":"string","pattern":"^[0-9a-f]{64}$"},
        "native_receipt":{"type":"object","properties":{
            "schema_version":{"const":1},"frames":{"type":"integer","minimum":1},
            "sample_rate":{"type":"integer","minimum":8000,"maximum":192000},
            "channels":{"type":"integer","minimum":1,"maximum":16},
            "clipped_input_samples":{"type":"integer","minimum":0},
            "compiler_version":{"enum":["2.37.3","2.70.3"]},"engine":{"const":"faust-interpreter"},"dither":{"const":"none"}
        },"required":["schema_version","frames","sample_rate","channels","clipped_input_samples","compiler_version","engine","dither"],"additionalProperties":false}
    },"required":["artifact","source_sha256","translator_version","deterministic","native_receipt","libraries_sha256"],"additionalProperties":false})
}

#[cfg(test)]
mod midi_contract_tests {
    use super::*;
    use semwright_audio_domain::{
        model::{Envelope, Oscillator, Signal, SignalNodeKind, Waveform},
        time::SampleFrame,
        units::{MilliHz, Permille},
    };

    fn synth() -> Synth {
        Synth {
            id: "instrument".into(),
            name: "instrument".into(),
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
                            amplitude: Permille(800),
                            phase_millidegrees: 0,
                            seed: None,
                        },
                    },
                },
                Signal {
                    id: "env".into(),
                    inputs: vec!["osc".into()],
                    node: SignalNodeKind::Envelope {
                        envelope: Envelope {
                            attack_frames: 48,
                            decay_frames: 96,
                            sustain: Permille(700),
                            release_frames: 2_400,
                        },
                    },
                },
            ],
            output: "env".into(),
        }
    }

    fn request(events: Vec<MidiEvent>) -> InstrumentRenderRequest {
        InstrumentRenderRequest {
            synth: synth(),
            phrase: MidiPhrase {
                id: "phrase".into(),
                name: "phrase".into(),
                instrument_synth: Some("instrument".into()),
                events,
            },
            reference_midi_note: 69,
            sample_rate: 48_000,
            duration_frames: 20_000,
            channels: 2,
            format: AudioFormat::Wav,
            bit_depth: BitDepth::Pcm16,
            output_file: "proof.wav".into(),
        }
    }

    #[test]
    fn same_pitch_overlap_is_rejected_even_across_midi_channels() {
        let value = request(vec![
            MidiEvent::Note {
                id: "a".into(),
                start: SampleFrame(0),
                duration_frames: 5_000,
                channel: 0,
                note: 60,
                velocity: 100,
            },
            MidiEvent::Note {
                id: "b".into(),
                start: SampleFrame(1_000),
                duration_frames: 2_000,
                channel: 1,
                note: 60,
                velocity: 100,
            },
        ]);
        let error = value.runtime_events().unwrap_err();
        assert_eq!(error.code, ErrorCode::Unsupported);
    }

    #[test]
    fn only_global_all_notes_controls_are_admitted() {
        let unsupported = request(vec![
            MidiEvent::Note {
                id: "note".into(),
                start: SampleFrame(0),
                duration_frames: 1_000,
                channel: 0,
                note: 60,
                velocity: 100,
            },
            MidiEvent::Control {
                id: "mod".into(),
                frame: SampleFrame(500),
                channel: 0,
                controller: 1,
                value: 64,
            },
        ]);
        assert_eq!(
            unsupported.runtime_events().unwrap_err().code,
            ErrorCode::Unsupported
        );

        let wrong_channel = request(vec![
            MidiEvent::Note {
                id: "note".into(),
                start: SampleFrame(0),
                duration_frames: 1_000,
                channel: 0,
                note: 60,
                velocity: 100,
            },
            MidiEvent::Control {
                id: "all".into(),
                frame: SampleFrame(500),
                channel: 1,
                controller: 123,
                value: 0,
            },
        ]);
        assert_eq!(
            wrong_channel.runtime_events().unwrap_err().code,
            ErrorCode::Unsupported
        );

        let supported = request(vec![
            MidiEvent::Note {
                id: "note".into(),
                start: SampleFrame(0),
                duration_frames: 1_000,
                channel: 0,
                note: 60,
                velocity: 100,
            },
            MidiEvent::Control {
                id: "all".into(),
                frame: SampleFrame(500),
                channel: 0,
                controller: 123,
                value: 0,
            },
        ]);
        assert!(supported.runtime_events().is_ok());
    }
}

#[cfg(test)]
mod descriptor_tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn catalog_is_unique_pinned_and_accepts_semantics_not_compiler_authority() {
        let capabilities = capability_catalog();
        let mut names = BTreeSet::new();
        for capability in &capabilities {
            assert!(names.insert(capability.descriptor.name.clone()));
            assert!(
                capability
                    .descriptor
                    .name
                    .starts_with("driver.faust-audio.")
            );
            assert_eq!(
                capability.descriptor.backends,
                vec![DRIVER_SCOPE.to_string()]
            );
            assert_eq!(descriptor_digest(&capability.descriptor).unwrap().len(), 64);

            let input = capability.descriptor.input_schema.to_string();
            for forbidden in [
                "faust_source",
                "source_code",
                "compiler_flags",
                "executable",
                "shell",
                "host",
                "url",
            ] {
                assert!(
                    !input.contains(forbidden),
                    "{}: {forbidden}",
                    capability.descriptor.name
                );
            }
        }
    }
}
