//! Production uses immutable owner-pinned tools inside the Linux Driver Host sandbox.
use crate::faust::FaustProgram;
use semwright_audio_domain::{
    model::{AudioProject, Sample, SampleSource},
    render::{AudioFormat, BitDepth, DitherPolicy, RenderIntent},
};
use semwright_driver_sdk::{DriverExecutionContext, workspace_mount};
use semwright_types::{Error, ErrorCode, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

pub const HELPER_NAME: &str = "faust-interpreter";
const MAX_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;
const MAX_SAMPLE_ASSET_BYTES: u64 = 512 * 1024 * 1024;
const MAX_RUNTIME_CONFIG_BYTES: u64 = 512 * 1024;
const MAX_LIBRARY_FILES: usize = 2048;
const MAX_LIBRARY_DEPTH: usize = 32;
const MAX_LIBRARY_TOTAL_BYTES: u64 = 256 * 1024 * 1024;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeConfig {
    pub schema_version: u32,
    pub compiler_version: String,
    pub libraries: BTreeMap<String, String>,
}
#[derive(Debug, Clone)]
pub struct Runtime {
    library_root: PathBuf,
    config: RuntimeConfig,
    library_digest: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeReceipt {
    pub schema_version: u32,
    pub frames: u64,
    pub sample_rate: u32,
    pub channels: u16,
    pub clipped_input_samples: u64,
    pub compiler_version: String,
    pub engine: String,
    pub dither: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolyNativeReceipt {
    pub schema_version: u32,
    pub frames: u64,
    pub sample_rate: u32,
    pub channels: u16,
    pub clipped_input_samples: u64,
    pub compiler_version: String,
    pub engine: String,
    pub dither: String,
    pub polyphony: u16,
    pub midi_events: u64,
    pub tail_frames: u64,
    pub voice_policy: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MidiRuntimeEvent {
    NoteOn {
        frame: u64,
        channel: u8,
        note: u8,
        velocity: u8,
    },
    NoteOff {
        frame: u64,
        channel: u8,
        note: u8,
        velocity: u8,
    },
    Control {
        frame: u64,
        channel: u8,
        controller: u8,
        value: u8,
    },
}

impl MidiRuntimeEvent {
    fn tuple(&self) -> (u64, u8, u8, u8, u8) {
        match *self {
            Self::NoteOff {
                frame,
                channel,
                note,
                velocity,
            } => (frame, 0, channel, note, velocity),
            Self::Control {
                frame,
                channel,
                controller,
                value,
            } => (frame, 1, channel, controller, value),
            Self::NoteOn {
                frame,
                channel,
                note,
                velocity,
            } => (frame, 2, channel, note, velocity),
        }
    }
    fn wire_kind(&self) -> u8 {
        match self {
            Self::NoteOn { .. } => 1,
            Self::NoteOff { .. } => 2,
            Self::Control { .. } => 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SampleNativeReceipt {
    pub schema_version: u32,
    pub frames: u64,
    pub sample_rate: u32,
    pub channels: u16,
    pub clipped_output_samples: u64,
    pub compiler_version: String,
    pub engine: String,
    pub dither: String,
    pub source_frames: u64,
    pub source_sample_rate: u32,
    pub source_channels: u16,
    pub looped: bool,
    pub resampling: String,
    pub channel_mapping: String,
}

#[derive(Debug, Clone)]
pub struct SampleArtifact {
    pub file_name: String,
    pub sha256: String,
    pub bytes: u64,
    pub format: AudioFormat,
    pub native: SampleNativeReceipt,
    pub libraries_sha256: String,
    pub input_sha256: String,
}

#[derive(Debug, Clone)]
pub struct SampleRenderSpec {
    pub sample_rate: u32,
    pub frames: u64,
    pub channels: u16,
    pub format: AudioFormat,
    pub bit_depth: BitDepth,
    pub file_name: String,
    pub looped: bool,
}

#[derive(Debug, Clone)]
pub struct PolyArtifact {
    pub file_name: String,
    pub sha256: String,
    pub bytes: u64,
    pub format: AudioFormat,
    pub native: PolyNativeReceipt,
    pub libraries_sha256: String,
    pub schedule_sha256: String,
}

#[derive(Debug, Clone)]
pub struct PolyRenderSpec {
    pub sample_rate: u32,
    pub frames: u64,
    pub channels: u16,
    pub format: AudioFormat,
    pub bit_depth: BitDepth,
    pub file_name: String,
    pub polyphony: u16,
    pub tail_frames: u64,
}

#[derive(Debug, Clone)]
pub struct Artifact {
    pub file_name: String,
    pub sha256: String,
    pub bytes: u64,
    pub format: AudioFormat,
    pub native: NativeReceipt,
    pub libraries_sha256: String,
}
impl Runtime {
    pub fn load_production() -> Result<Option<Self>> {
        if !cfg!(target_os = "linux") {
            return Ok(None);
        }
        let library_root = workspace_mount("faust-libraries")?;
        let config_path = library_root.join("semwright-runtime.json");
        if !config_path.try_exists()? {
            return Ok(None);
        }
        regular(&config_path, MAX_RUNTIME_CONFIG_BYTES)?;
        let bytes = fs::read(&config_path)?;
        let config: RuntimeConfig = serde_json::from_slice(&bytes)?;
        if config.schema_version != 1
            || !matches!(config.compiler_version.as_str(), "2.37.3" | "2.70.3")
            || config.libraries.is_empty()
            || config.libraries.len() > MAX_LIBRARY_FILES
            || !config.libraries.contains_key("stdfaust.lib")
        {
            return Err(Error::invalid("Unsupported Faust runtime manifest"));
        }
        let library_digest = format!("{:x}", Sha256::digest(serde_json::to_vec(&config)?));
        let result = Self {
            library_root,
            config,
            library_digest,
        };
        result.verify_libraries()?;
        // Executable authority and availability are checked by the shared SDK/Host
        // when the fixed logical tool is invoked, never by a private path resolver.
        Ok(Some(result))
    }
    pub fn version(&self) -> &str {
        &self.config.compiler_version
    }
    pub async fn probe(&self, context: &DriverExecutionContext) -> Result<serde_json::Value> {
        context.check_cancelled()?;
        let base = |sealed_helper_executed: bool,
                    library_mount: bool,
                    stdlib_regular: bool,
                    interpreter_compile: bool,
                    diagnostic_class: String,
                    diagnostic_prefix: String| {
            serde_json::json!({
                "schema_version":1,
                "engine":"faust-interpreter",
                "compiler_version":self.config.compiler_version,
                "runtime_available":true,
                "sealed_helper_executed":sealed_helper_executed,
                "library_mount":library_mount,
                "stdlib_regular":stdlib_regular,
                "interpreter_compile":interpreter_compile,
                "diagnostic_class":diagnostic_class,
                "diagnostic_prefix":diagnostic_prefix
            })
        };
        if let Err(error) = self.verify_libraries() {
            return Ok(base(
                false,
                false,
                false,
                false,
                format!("{:?}", error.code),
                bounded_diagnostic(&error.message),
            ));
        }
        let output = match run_sealed_tool(
            context,
            vec![
                "probe".into(),
                self.library_root.to_string_lossy().into_owned(),
            ],
            &[],
        )
        .await
        {
            Ok(output) => output,
            Err(error) => {
                return Ok(base(
                    false,
                    true,
                    true,
                    false,
                    format!("{:?}", error.code),
                    bounded_diagnostic(&error.message),
                ));
            }
        };
        if output.exit_code != 0 {
            let classified = classify_tool_exit(&output.stderr);
            return Ok(base(
                true,
                true,
                true,
                false,
                format!("{:?}", classified.code),
                bounded_diagnostic(&String::from_utf8_lossy(&output.stderr)),
            ));
        }
        let value: serde_json::Value = match serde_json::from_slice(&output.stdout) {
            Ok(value) => value,
            Err(_) => {
                return Ok(base(
                    true,
                    true,
                    true,
                    false,
                    "ProtocolMismatch".into(),
                    "helper_stdout_not_typed_json".into(),
                ));
            }
        };
        if value["schema_version"] != 1
            || value["engine"] != "faust-interpreter"
            || value["compiler_version"] != self.config.compiler_version
            || !value["library_mount"].is_boolean()
            || !value["stdlib_regular"].is_boolean()
            || !value["interpreter_compile"].is_boolean()
            || value["diagnostic_class"].as_str().is_none()
            || value["diagnostic_prefix"].as_str().is_none()
        {
            return Ok(base(
                true,
                true,
                true,
                false,
                "ProtocolMismatch".into(),
                "helper_probe_receipt_shape_mismatch".into(),
            ));
        }
        Ok(serde_json::json!({
            "schema_version":1,
            "engine":"faust-interpreter",
            "compiler_version":self.config.compiler_version,
            "runtime_available":true,
            "sealed_helper_executed":true,
            "library_mount":value["library_mount"],
            "stdlib_regular":value["stdlib_regular"],
            "interpreter_compile":value["interpreter_compile"],
            "diagnostic_class":value["diagnostic_class"],
            "diagnostic_prefix":value["diagnostic_prefix"]
        }))
    }

    fn verify_libraries(&self) -> Result<()> {
        let metadata = fs::symlink_metadata(&self.library_root)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(Error::invalid(
                "Faust libraries must be a real read-only mount",
            ));
        }
        let present = discover_library_files(&self.library_root)?;
        let expected: BTreeSet<String> = self.config.libraries.keys().cloned().collect();
        if present.keys().cloned().collect::<BTreeSet<_>>() != expected {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Faust library inventory changed",
            ));
        }
        for (name, digest) in &self.config.libraries {
            validate_library_relative(name)?;
            if digest.len() != 64
                || !digest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            {
                return Err(Error::invalid("Invalid Faust library digest"));
            }
            let path = present
                .get(name)
                .ok_or_else(|| Error::new(ErrorCode::Conflict, "Faust library is missing"))?;
            if hash_file(path, 8 * 1024 * 1024)? != *digest {
                return Err(Error::new(
                    ErrorCode::Conflict,
                    "Faust library digest changed",
                ));
            }
        }
        Ok(())
    }
    async fn call(
        &self,
        context: &DriverExecutionContext,
        args: Vec<String>,
        source: &[u8],
    ) -> Result<serde_json::Value> {
        context.check_cancelled()?;
        self.verify_libraries()?;
        if source.is_empty() || source.len() > 60000 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Faust source exceeds sealed-tool budget",
            ));
        }
        let output = run_sealed_tool(context, args, source).await?;
        if output.exit_code != 0 {
            return Err(classify_tool_exit(&output.stderr));
        }
        context.check_cancelled()?;
        let value: serde_json::Value = serde_json::from_slice(&output.stdout)?;
        if value["compiler_version"] != self.config.compiler_version || value["schema_version"] != 1
        {
            return Err(Error::new(
                ErrorCode::ProtocolMismatch,
                "Faust native receipt version differs from the pinned runtime",
            ));
        }
        Ok(value)
    }
    pub async fn validate_program(
        &self,
        context: &DriverExecutionContext,
        program: &FaustProgram,
    ) -> Result<()> {
        let value = self
            .call(
                context,
                vec![
                    "validate".into(),
                    self.library_root.to_string_lossy().into_owned(),
                ],
                program.source.as_bytes(),
            )
            .await?;
        if value["valid"] != true || value["outputs"] != program.outputs {
            return Err(Error::new(
                ErrorCode::BackendFailed,
                "Faust validation receipt has an unexpected shape",
            ));
        }
        Ok(())
    }
    pub async fn render(
        &self,
        context: &DriverExecutionContext,
        program: &FaustProgram,
        intent: &RenderIntent,
        project: &AudioProject,
        file_name: &str,
    ) -> Result<Artifact> {
        let frames = intent
            .validate_against(project)
            .map_err(|e| Error::invalid(e.message))?;
        validate_file_name(file_name, intent.format)?;
        if frames > 57_600_000
            || intent.sample_rate.0 > 192_000
            || intent.channels != program.outputs
            || intent.sample_rate != project.profile.sample_rate
            || intent.normalize_lufs_milli.is_some()
            || intent.dither != DitherPolicy::None
            || intent.range.is_some_and(|r| r.start.0 != 0)
            || (intent.format == AudioFormat::Flac && intent.bit_depth == BitDepth::Pcm32)
        {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "Render intent exceeds the explicit interpreter contract",
            ));
        }
        let output_root = workspace_mount("output")?;
        let metadata = fs::symlink_metadata(&output_root)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(Error::invalid("Output grant must be a real directory"));
        }
        let scratch = tempfile::Builder::new()
            .prefix(".faust-candidate-")
            .tempdir_in(&output_root)?;
        let bits = match intent.bit_depth {
            BitDepth::Pcm16 => 16,
            BitDepth::Pcm24 => 24,
            BitDepth::Pcm32 => 32,
        };
        let value = self
            .call(
                context,
                vec![
                    "render".into(),
                    self.library_root.to_string_lossy().into_owned(),
                    scratch.path().to_string_lossy().into_owned(),
                    intent.sample_rate.0.to_string(),
                    frames.to_string(),
                    intent.extension().into(),
                    bits.to_string(),
                ],
                program.source.as_bytes(),
            )
            .await?;
        let native: NativeReceipt = serde_json::from_value(value)?;
        if native.frames != frames
            || native.sample_rate != intent.sample_rate.0
            || native.channels != intent.channels
            || native.engine != "faust-interpreter"
            || native.dither != "none"
        {
            return Err(Error::new(
                ErrorCode::BackendFailed,
                "Native render does not match the requested media shape",
            ));
        }
        let staged = scratch
            .path()
            .join(format!("render.{}", intent.extension()));
        regular(&staged, MAX_ARTIFACT_BYTES)?;
        let bytes = fs::metadata(&staged)?.len();
        let sha256 = hash_file(&staged, MAX_ARTIFACT_BYTES)?;
        context.check_cancelled()?;
        // Link within one granted filesystem publishes complete bytes without replacing
        // an existing name. This is single-file publication, not a DAW transaction.
        fs::hard_link(&staged, output_root.join(file_name)).map_err(|_| {
            Error::new(
                ErrorCode::Conflict,
                "Output exists or no-clobber publication failed",
            )
        })?;
        Ok(Artifact {
            file_name: file_name.into(),
            sha256,
            bytes,
            format: intent.format,
            native,
            libraries_sha256: self.library_digest.clone(),
        })
    }

    pub async fn render_sample(
        &self,
        context: &DriverExecutionContext,
        program: &FaustProgram,
        sample: &Sample,
        expected_sha256: &str,
        spec: SampleRenderSpec,
    ) -> Result<SampleArtifact> {
        context.check_cancelled()?;
        sample
            .validate()
            .map_err(|error| Error::invalid(error.message))?;
        validate_file_name(&spec.file_name, spec.format)?;
        validate_sha256(expected_sha256)?;
        if spec.frames == 0
            || spec.frames > 57_600_000
            || !(8_000..=192_000).contains(&spec.sample_rate)
            || !(1..=16).contains(&spec.channels)
            || spec.channels != program.outputs
            || sample.sample_rate.0 != spec.sample_rate
            || sample.frames > 57_600_000
            || (spec.format == AudioFormat::Flac && spec.bit_depth == BitDepth::Pcm32)
        {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "Sample render exceeds the explicit exact-rate interpreter contract",
            ));
        }

        let asset_root = workspace_mount("audio-assets")?;
        let asset_metadata = fs::symlink_metadata(&asset_root)?;
        if asset_metadata.file_type().is_symlink() || !asset_metadata.is_dir() {
            return Err(Error::invalid("Audio asset grant must be a real directory"));
        }
        let relative = match &sample.source {
            SampleSource::RelativePath { path } => path.as_str(),
            SampleSource::Artifact { .. } => {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "Artifact-backed samples require explicit materialization before Faust playback",
                ));
            }
            SampleSource::Silence => {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "Silence samples do not require native sample playback",
                ));
            }
        };
        validate_asset_relative(relative)?;

        let output_root = workspace_mount("output")?;
        let output_metadata = fs::symlink_metadata(&output_root)?;
        if output_metadata.file_type().is_symlink() || !output_metadata.is_dir() {
            return Err(Error::invalid("Output grant must be a real directory"));
        }
        let scratch = tempfile::Builder::new()
            .prefix(".faust-sample-candidate-")
            .tempdir_in(&output_root)?;
        let staged_input =
            snapshot_sample_asset(&asset_root, relative, expected_sha256, scratch.path())?;
        let input_sha256 = hash_file(&staged_input, MAX_SAMPLE_ASSET_BYTES)?;
        let input_name = staged_input
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| Error::invalid("Staged sample filename is not UTF-8"))?;

        let bits = match spec.bit_depth {
            BitDepth::Pcm16 => 16,
            BitDepth::Pcm24 => 24,
            BitDepth::Pcm32 => 32,
        };
        let extension = match spec.format {
            AudioFormat::Wav => "wav",
            AudioFormat::Flac => "flac",
        };
        let value = self
            .call(
                context,
                vec![
                    "render-sample".into(),
                    self.library_root.to_string_lossy().into_owned(),
                    scratch.path().to_string_lossy().into_owned(),
                    input_name.to_owned(),
                    spec.sample_rate.to_string(),
                    spec.frames.to_string(),
                    extension.into(),
                    bits.to_string(),
                    if spec.looped { "1" } else { "0" }.into(),
                ],
                program.source.as_bytes(),
            )
            .await?;
        let native: SampleNativeReceipt = serde_json::from_value(value)?;
        if native.frames != spec.frames
            || native.sample_rate != spec.sample_rate
            || native.channels != spec.channels
            || native.engine != "faust-sample-interpreter"
            || native.dither != "none"
            || native.source_frames != sample.frames
            || native.source_sample_rate != sample.sample_rate.0
            || native.source_channels != sample.channels
            || native.looped != spec.looped
            || native.resampling != "exact_only"
            || native.channel_mapping != "mono_average"
        {
            return Err(Error::new(
                ErrorCode::BackendFailed,
                "Native sample render receipt does not match the semantic Sample contract",
            ));
        }
        let staged = scratch.path().join(format!("render.{extension}"));
        regular(&staged, MAX_ARTIFACT_BYTES)?;
        let bytes = fs::metadata(&staged)?.len();
        let sha256 = hash_file(&staged, MAX_ARTIFACT_BYTES)?;
        context.check_cancelled()?;
        fs::hard_link(&staged, output_root.join(&spec.file_name)).map_err(|_| {
            Error::new(
                ErrorCode::Conflict,
                "Output exists or no-clobber publication failed",
            )
        })?;
        Ok(SampleArtifact {
            file_name: spec.file_name,
            sha256,
            bytes,
            format: spec.format,
            native,
            libraries_sha256: self.library_digest.clone(),
            input_sha256,
        })
    }

    pub async fn render_poly(
        &self,
        context: &DriverExecutionContext,
        program: &FaustProgram,
        events: &[MidiRuntimeEvent],
        spec: PolyRenderSpec,
    ) -> Result<PolyArtifact> {
        context.check_cancelled()?;
        validate_file_name(&spec.file_name, spec.format)?;
        if spec.frames == 0
            || spec.frames > 57_600_000
            || !(8_000..=192_000).contains(&spec.sample_rate)
            || !(1..=16).contains(&spec.channels)
            || spec.channels != program.outputs
            || !(1..=64).contains(&spec.polyphony)
            || spec.tail_frames == 0
            || spec.tail_frames > u64::from(spec.sample_rate) * 30
            || events.is_empty()
            || events.len() > 100_000
            || (spec.format == AudioFormat::Flac && spec.bit_depth == BitDepth::Pcm32)
        {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "Polyphonic render request exceeds the explicit interpreter contract",
            ));
        }
        let output_root = workspace_mount("output")?;
        let metadata = fs::symlink_metadata(&output_root)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(Error::invalid("Output grant must be a real directory"));
        }
        let scratch = tempfile::Builder::new()
            .prefix(".faust-poly-candidate-")
            .tempdir_in(&output_root)?;
        let schedule = scratch.path().join("midi-events.bin");
        write_midi_schedule(&schedule, events, spec.frames)?;
        let schedule_sha256 = hash_file(&schedule, 2 * 1024 * 1024)?;
        let bits = match spec.bit_depth {
            BitDepth::Pcm16 => 16,
            BitDepth::Pcm24 => 24,
            BitDepth::Pcm32 => 32,
        };
        let extension = match spec.format {
            AudioFormat::Wav => "wav",
            AudioFormat::Flac => "flac",
        };
        let value = self
            .call(
                context,
                vec![
                    "render-poly".into(),
                    self.library_root.to_string_lossy().into_owned(),
                    scratch.path().to_string_lossy().into_owned(),
                    spec.sample_rate.to_string(),
                    spec.frames.to_string(),
                    extension.into(),
                    bits.to_string(),
                    spec.polyphony.to_string(),
                    spec.tail_frames.to_string(),
                ],
                program.source.as_bytes(),
            )
            .await?;
        let native: PolyNativeReceipt = serde_json::from_value(value)?;
        if native.frames != spec.frames
            || native.sample_rate != spec.sample_rate
            || native.channels != spec.channels
            || native.engine != "faust-poly-interpreter"
            || native.dither != "none"
            || native.polyphony != spec.polyphony
            || native.midi_events != events.len() as u64
            || native.tail_frames != spec.tail_frames
            || native.voice_policy != "faust_first_free_then_oldest_release_then_oldest_playing"
        {
            return Err(Error::new(
                ErrorCode::BackendFailed,
                "Polyphonic native render receipt does not match the requested schedule",
            ));
        }
        let staged = scratch.path().join(format!("render.{extension}"));
        regular(&staged, MAX_ARTIFACT_BYTES)?;
        let bytes = fs::metadata(&staged)?.len();
        let sha256 = hash_file(&staged, MAX_ARTIFACT_BYTES)?;
        context.check_cancelled()?;
        fs::hard_link(&staged, output_root.join(&spec.file_name)).map_err(|_| {
            Error::new(
                ErrorCode::Conflict,
                "Output exists or no-clobber publication failed",
            )
        })?;
        Ok(PolyArtifact {
            file_name: spec.file_name,
            sha256,
            bytes,
            format: spec.format,
            native,
            libraries_sha256: self.library_digest.clone(),
            schedule_sha256,
        })
    }
}
fn validate_sha256(value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(Error::invalid("Invalid expected SHA-256 digest"));
    }
    Ok(())
}

fn validate_asset_relative(value: &str) -> Result<()> {
    let path = Path::new(value);
    if value.is_empty()
        || value.len() > 512
        || value.contains('\\')
        || value.chars().any(char::is_control)
        || path.is_absolute()
        || path.components().count() > 16
        || !matches!(
            path.extension().and_then(|value| value.to_str()),
            Some("wav" | "flac")
        )
        || path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(Error::invalid("Invalid bounded audio asset relative path"));
    }
    Ok(())
}

fn snapshot_sample_asset(
    root: &Path,
    relative: &str,
    expected_sha256: &str,
    scratch: &Path,
) -> Result<PathBuf> {
    validate_asset_relative(relative)?;
    validate_sha256(expected_sha256)?;
    let relative_path = Path::new(relative);
    let mut cursor = root.to_path_buf();
    let components = relative_path.components().collect::<Vec<_>>();
    for (index, component) in components.iter().enumerate() {
        let std::path::Component::Normal(part) = component else {
            return Err(Error::invalid("Audio asset path is not normalized"));
        };
        cursor.push(part);
        let metadata = fs::symlink_metadata(&cursor)?;
        if metadata.file_type().is_symlink() {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "Audio asset path cannot traverse symlinks",
            ));
        }
        if index + 1 < components.len() {
            if !metadata.is_dir() {
                return Err(Error::invalid("Audio asset parent is not a directory"));
            }
        } else if !metadata.is_file()
            || metadata.len() == 0
            || metadata.len() > MAX_SAMPLE_ASSET_BYTES
        {
            return Err(Error::invalid("Audio asset is not a bounded regular file"));
        }
    }

    let canonical_root = fs::canonicalize(root)?;
    let canonical_source = fs::canonicalize(&cursor)?;
    if !canonical_source.starts_with(&canonical_root) {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Audio asset canonical path escaped its owner grant",
        ));
    }
    let before_path = fs::symlink_metadata(&cursor)?;
    let extension = relative_path
        .extension()
        .and_then(|value| value.to_str())
        .ok_or_else(|| Error::invalid("Audio asset extension is unavailable"))?;
    let staged = scratch.join(format!("sample-input.{extension}"));
    let mut input = File::open(&cursor)?;
    let before = input.metadata()?;
    if !before.is_file() || before.len() == 0 || before.len() > MAX_SAMPLE_ASSET_BYTES {
        return Err(Error::invalid(
            "Opened audio asset is not a bounded regular file",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if before_path.dev() != before.dev()
            || before_path.ino() != before.ino()
            || before.nlink() != 1
        {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Audio asset identity changed or is hard-linked",
            ));
        }
    }
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&staged)?;
    let mut digest = Sha256::new();
    let mut copied = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        copied = copied
            .checked_add(count as u64)
            .filter(|value| *value <= MAX_SAMPLE_ASSET_BYTES)
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::ResourceExhausted,
                    "Audio asset snapshot exceeded size budget",
                )
            })?;
        output.write_all(&buffer[..count])?;
        digest.update(&buffer[..count]);
    }
    output.flush()?;
    output.sync_all()?;
    drop(output);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&staged, fs::Permissions::from_mode(0o400))?;
    }
    #[cfg(not(unix))]
    {
        let mut permissions = fs::metadata(&staged)?.permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&staged, permissions)?;
    }
    let after = input.metadata()?;
    if copied != before.len() || after.len() != before.len() {
        return Err(Error::new(
            ErrorCode::Conflict,
            "Audio asset changed while taking its immutable snapshot",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if after.dev() != before.dev() || after.ino() != before.ino() || after.nlink() != 1 {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Audio asset identity changed during immutable snapshot",
            ));
        }
    }
    let actual = format!("{:x}", digest.finalize());
    if actual != expected_sha256 {
        return Err(Error::new(
            ErrorCode::Conflict,
            "Audio asset SHA-256 does not match the requested immutable identity",
        ));
    }
    regular(&staged, MAX_SAMPLE_ASSET_BYTES)?;
    Ok(staged)
}

fn validate_library_relative(value: &str) -> Result<()> {
    let path = Path::new(value);
    if value.is_empty()
        || value.len() > 512
        || !value.ends_with(".lib")
        || path.is_absolute()
        || path.components().count() > MAX_LIBRARY_DEPTH + 1
        || path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
        || value.split('/').any(|part| {
            part.is_empty()
                || part == "."
                || part == ".."
                || part.len() > 128
                || !part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        })
    {
        return Err(Error::invalid("Invalid Faust library relative path"));
    }
    Ok(())
}

fn discover_library_files(root: &Path) -> Result<BTreeMap<String, PathBuf>> {
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    let mut found = BTreeMap::new();
    let mut total_bytes = 0u64;
    while let Some((directory, depth)) = stack.pop() {
        if depth > MAX_LIBRARY_DEPTH {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Faust library directory depth exceeded limit",
            ));
        }
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() {
                return Err(Error::new(
                    ErrorCode::PermissionDenied,
                    "Faust library tree cannot contain symlinks",
                ));
            }
            if metadata.is_dir() {
                stack.push((path, depth + 1));
                continue;
            }
            if !metadata.is_file() || path.extension().is_none_or(|ext| ext != "lib") {
                continue;
            }
            let relative = path
                .strip_prefix(root)
                .map_err(|_| Error::invalid("Faust library escaped its mount"))?
                .to_string_lossy()
                .replace('\\', "/");
            validate_library_relative(&relative)?;
            regular(&path, 8 * 1024 * 1024)?;
            total_bytes = total_bytes
                .checked_add(metadata.len())
                .filter(|value| *value <= MAX_LIBRARY_TOTAL_BYTES)
                .ok_or_else(|| {
                    Error::new(
                        ErrorCode::ResourceExhausted,
                        "Faust library aggregate size exceeded limit",
                    )
                })?;
            if found.insert(relative, path).is_some() || found.len() > MAX_LIBRARY_FILES {
                return Err(Error::new(
                    ErrorCode::ResourceExhausted,
                    "Faust library inventory exceeded limit or repeated a path",
                ));
            }
        }
    }
    if found.is_empty() {
        return Err(Error::invalid("Faust library inventory is empty"));
    }
    Ok(found)
}

fn regular(path: &Path, limit: u64) -> Result<()> {
    let m = fs::symlink_metadata(path)?;
    if !m.is_file() || m.file_type().is_symlink() || m.len() == 0 || m.len() > limit {
        return Err(Error::invalid(
            "Expected a bounded regular audio runtime file",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if m.nlink() != 1 || m.mode() & 0o022 != 0 {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "Runtime file must have one link and no untrusted writers",
            ));
        }
    }
    Ok(())
}
fn hash_file(path: &Path, limit: u64) -> Result<String> {
    let mut file = File::open(path)?.take(limit + 1);
    let mut digest = Sha256::new();
    let mut buffer = [0; 65536];
    let mut size = 0;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        size += read as u64;
        if size > limit {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "File hashing budget exceeded",
            ));
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}
fn write_midi_schedule(path: &Path, events: &[MidiRuntimeEvent], frames: u64) -> Result<()> {
    if events.is_empty() || events.len() > 100_000 {
        return Err(Error::invalid("MIDI schedule event count is invalid"));
    }
    let mut rows = events.to_vec();
    rows.sort_by_key(MidiRuntimeEvent::tuple);
    for event in &rows {
        let (frame, _, channel, data1, data2) = event.tuple();
        if frame > frames || channel > 15 || data1 > 127 || data2 > 127 {
            return Err(Error::invalid("MIDI schedule event exceeds runtime bounds"));
        }
        if matches!(event, MidiRuntimeEvent::NoteOn { velocity: 0, .. }) {
            return Err(Error::invalid("MIDI note-on velocity must be nonzero"));
        }
    }
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(b"SWMIDI1\x00")?;
    file.write_all(&(rows.len() as u32).to_le_bytes())?;
    for event in &rows {
        let (frame, _, channel, data1, data2) = event.tuple();
        file.write_all(&frame.to_le_bytes())?;
        file.write_all(&[event.wire_kind(), channel, data1, data2])?;
    }
    file.flush()?;
    file.sync_all()?;
    regular(path, 2 * 1024 * 1024)?;
    Ok(())
}

fn validate_file_name(value: &str, format: AudioFormat) -> Result<()> {
    let extension = match format {
        AudioFormat::Wav => ".wav",
        AudioFormat::Flac => ".flac",
    };
    if value.is_empty()
        || value.len() > 200
        || value.starts_with('.')
        || value.contains("..")
        || !value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
        || !value.ends_with(extension)
    {
        return Err(Error::invalid("Invalid audio output filename"));
    }
    Ok(())
}

fn bounded_diagnostic(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_control() && ch != '\n' && ch != '\t' {
                '?'
            } else {
                ch
            }
        })
        .take(512)
        .collect()
}

fn classify_tool_exit(stderr: &[u8]) -> Error {
    let text = String::from_utf8_lossy(stderr);
    if text.contains("Permission denied") || text.contains("Operation not permitted") {
        Error::new(
            ErrorCode::SandboxDenied,
            "Sandbox denied an operation required by the pinned Faust helper",
        )
    } else if text.contains("error while loading shared libraries")
        || text.contains("No such file or directory")
    {
        Error::new(
            ErrorCode::Unavailable,
            "Pinned Faust helper runtime dependencies are unavailable",
        )
    } else if text.contains("library mount")
        || text.contains("standard library import")
        || text.contains("external Faust mechanism")
    {
        Error::new(
            ErrorCode::ProtocolMismatch,
            "Pinned Faust helper rejected the staged runtime contract",
        )
    } else {
        Error::new(
            ErrorCode::BackendFailed,
            "Pinned Faust helper exited unsuccessfully",
        )
    }
}

async fn run_sealed_tool(
    context: &DriverExecutionContext,
    args: Vec<String>,
    source: &[u8],
) -> Result<semwright_driver_sdk::ToolExecutionOutput> {
    if !cfg!(target_os = "linux") {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "Audio runtime grants are not implemented for this platform",
        ));
    }
    context.check_cancelled()?;
    let operation = args.first().map(String::as_str).unwrap_or_default();
    let wall_timeout = if matches!(operation, "render" | "render-sample" | "render-poly") {
        Duration::from_secs(180)
    } else {
        Duration::from_secs(30)
    };
    // Keep the existing v4 aggregate CPU budget. The shared one-shot SDK
    // applies its stricter 30-second wall ceiling and reaps cancellation.
    let output = context
        .execute_runtime_tool(
            HELPER_NAME,
            args,
            source.to_vec(),
            wall_timeout.min(Duration::from_secs(30)),
        )
        .await?;
    if output.stdout.len() > 262144 || output.stderr.len() > 262144 {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Native tool output exceeded limit",
        ));
    }
    Ok(output)
}

#[cfg(test)]
mod library_inventory_tests {
    use super::*;

    #[test]
    fn relative_library_paths_are_strict_and_nested() {
        assert!(validate_library_relative("stdfaust.lib").is_ok());
        assert!(validate_library_relative("physmodels/mesh.lib").is_ok());
        assert!(validate_library_relative("../outside.lib").is_err());
        assert!(validate_library_relative("/absolute.lib").is_err());
        assert!(validate_library_relative("nested//broken.lib").is_err());
        assert!(validate_library_relative("nested/not-a-library.txt").is_err());
    }

    #[test]
    fn midi_schedule_is_versioned_sorted_and_bounded() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("midi-events.bin");
        let events = vec![
            MidiRuntimeEvent::NoteOn {
                frame: 100,
                channel: 0,
                note: 64,
                velocity: 100,
            },
            MidiRuntimeEvent::Control {
                frame: 100,
                channel: 0,
                controller: 1,
                value: 32,
            },
            MidiRuntimeEvent::NoteOff {
                frame: 100,
                channel: 0,
                note: 60,
                velocity: 0,
            },
        ];
        write_midi_schedule(&path, &events, 1000).unwrap();
        let bytes = fs::read(path).unwrap();
        assert_eq!(&bytes[..8], b"SWMIDI1\x00");
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), 3);
        assert_eq!(bytes[20], 2);
        assert_eq!(bytes[32], 3);
        assert_eq!(bytes[44], 1);

        let invalid = root.path().join("invalid.bin");
        assert!(
            write_midi_schedule(
                &invalid,
                &[MidiRuntimeEvent::NoteOn {
                    frame: 1,
                    channel: 0,
                    note: 60,
                    velocity: 0,
                }],
                100
            )
            .is_err()
        );
    }

    #[test]
    #[cfg(unix)]
    fn sample_snapshot_seals_private_permissions_independent_of_umask() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let scratch = tempfile::tempdir().unwrap();
        let source = root.path().join("sample.wav");
        fs::write(&source, b"bounded-sample-fixture").unwrap();
        let expected = format!("{:x}", Sha256::digest(fs::read(&source).unwrap()));

        let staged =
            snapshot_sample_asset(root.path(), "sample.wav", &expected, scratch.path()).unwrap();
        assert_eq!(
            fs::metadata(staged).unwrap().permissions().mode() & 0o777,
            0o400
        );
    }

    #[test]
    #[cfg(unix)]
    fn recursive_inventory_accepts_materialized_files_and_rejects_symlinks() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let root = tempfile::tempdir().unwrap();
        let nested = root.path().join("nested");
        fs::create_dir(&nested).unwrap();
        fs::write(root.path().join("stdfaust.lib"), b"main").unwrap();
        fs::write(nested.join("dependency.lib"), b"dep").unwrap();
        fs::set_permissions(
            root.path().join("stdfaust.lib"),
            fs::Permissions::from_mode(0o400),
        )
        .unwrap();
        fs::set_permissions(
            nested.join("dependency.lib"),
            fs::Permissions::from_mode(0o400),
        )
        .unwrap();

        let inventory = discover_library_files(root.path()).unwrap();
        assert_eq!(
            inventory.keys().cloned().collect::<Vec<_>>(),
            vec!["nested/dependency.lib", "stdfaust.lib"]
        );

        symlink(
            nested.join("dependency.lib"),
            root.path().join("linked.lib"),
        )
        .unwrap();
        assert!(discover_library_files(root.path()).is_err());
    }
}
