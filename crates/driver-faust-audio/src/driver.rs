use crate::{
    faust::{TRANSLATOR_VERSION, translate},
    runtime::Runtime,
};
use async_trait::async_trait;
use semwright_audio_domain::{
    backend::{BackendContract, BackendIdentity, ProjectionFidelity},
    model::{AudioProfile, AudioProject, Synth},
    presets::{self, SfxPreset},
    render::{AudioFormat, BitDepth, RENDER_CONTRACT_VERSION, RenderIntent, RenderSource},
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

pub struct FaustAudioDriver {
    runtime: Option<Runtime>,
    runtime_reason: String,
}

impl FaustAudioDriver {
    pub fn production() -> Result<Self> {
        match Runtime::load_production()? {
            Some(runtime) => Ok(Self {
                runtime: Some(runtime),
                runtime_reason: "owner-pinned Faust runtime is available".into(),
            }),
            None => Ok(Self {
                runtime: None,
                runtime_reason:
                    "owner-pinned Faust interpreter and faust-libraries mount are absent".into(),
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

fn capabilities() -> Vec<Capability> {
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
            object_types: vec!["audio-synth".into()],
        })
        .collect()
}

fn capability(command: &str) -> Result<Capability> {
    capabilities()
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
        Ok(capabilities())
    }

    fn interfaces(&self) -> DriverInterfaces {
        DriverInterfaces {
            cooperative_cancellation: true,
            progress: true,
            artifacts: true,
            host_tools: true,
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
        let version = match &self.runtime {
            Some(runtime) => Some(runtime.version()),
            None => None,
        };
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
                                "Protocol-v4 Host context is required",
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
                            "Protocol-v4 Host context is required",
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
                            "Protocol-v4 Host context is required",
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
            dither_seed: None,
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
fn source_output_schema() -> Value {
    json!({"type":"object","properties":{
        "source":{"type":"string","maxLength":1048576},
        "source_sha256":{"type":"string","pattern":"^[0-9a-f]{64}$"},
        "translator_version":{"const":TRANSLATOR_VERSION},
        "outputs":{"type":"integer","minimum":1,"maximum":16},
        "arbitrary_source":{"const":false}
    },"required":["source","source_sha256","translator_version","outputs","arbitrary_source"],"additionalProperties":false})
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
            "compiler_version":{"const":"2.70.3"},"engine":{"const":"faust-interpreter"},"dither":{"const":"none"}
        },"required":["schema_version","frames","sample_rate","channels","clipped_input_samples","compiler_version","engine","dither"],"additionalProperties":false}
    },"required":["artifact","source_sha256","translator_version","deterministic","native_receipt","libraries_sha256"],"additionalProperties":false})
}

#[cfg(test)]
mod descriptor_tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn catalog_is_unique_pinned_and_accepts_semantics_not_compiler_authority() {
        let capabilities = capabilities();
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
