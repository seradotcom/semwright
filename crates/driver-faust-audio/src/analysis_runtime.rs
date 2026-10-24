use semwright_audio_domain::{
    analysis::LoudnessAnalysis, signal_analysis::SignalStatistics, wav::WaveReader,
};
use semwright_driver_sdk::{
    DriverExecutionContext, RuntimeToolArg, RuntimeToolMode, workspace_mount,
};
use semwright_types::{Error, ErrorCode, Result};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};
use tempfile::TempDir;

pub const METER_NAME: &str = "audio-meter";
pub(crate) const MAX_INPUT_BYTES: u64 = 512 * 1024 * 1024;
const MAX_OUTPUT_BYTES: usize = 128 * 1024;

#[derive(Debug)]
pub struct AnalysisRuntime {
    input_root: PathBuf,
    host_snapshot_meter: bool,
}
#[derive(Debug)]
pub struct AnalysisReceipt {
    pub file_name: String,
    pub sha256: String,
    pub bytes: u64,
    pub format: &'static str,
    pub loudness: LoudnessAnalysis,
    pub statistics: Option<SignalStatistics>,
}
impl AnalysisRuntime {
    pub fn load() -> Result<Option<Self>> {
        if !cfg!(target_os = "linux") {
            return Ok(None);
        }
        let input_root = workspace_mount("analysis-input")?;
        let metadata = fs::symlink_metadata(&input_root)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(Error::invalid(
                "Audio analysis input grant must be a real directory",
            ));
        }
        Ok(Some(Self {
            input_root,
            host_snapshot_meter: false,
        }))
    }

    pub(crate) fn enable_host_snapshot_meter(&mut self) {
        self.host_snapshot_meter = true;
    }

    pub(crate) fn snapshot_alignment(
        &self,
        file_name: &str,
    ) -> Result<(TempDir, PathBuf, String, u64)> {
        validate_name(file_name)?;
        if !file_name.ends_with(".wav") {
            return Err(Error::invalid("Alignment input must be WAV"));
        }
        snapshot_input_bounded(
            &self.input_root.join(file_name),
            file_name,
            crate::alignment_candidate::MAX_WAV,
        )
    }

    pub async fn measure(
        &self,
        context: Option<&DriverExecutionContext>,
        file_name: &str,
        expected_sha256: &str,
        layout: &str,
    ) -> Result<AnalysisReceipt> {
        validate_name(file_name)?;
        validate_digest(expected_sha256)?;
        if !matches!(layout, "mono" | "stereo") {
            return Err(Error::invalid(
                "Audio analysis layout must be mono or stereo",
            ));
        }
        if let Some(context) = context {
            context.check_cancelled()?;
        }
        let source = self.input_root.join(file_name);
        let (temp, staged, sha256, bytes) = snapshot_input(&source, file_name)?;
        if sha256 != expected_sha256 {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Audio analysis input digest changed",
            ));
        }
        let loudness = self
            .run_meter(context, &staged, file_name, &sha256, bytes, layout)
            .await?;
        let statistics = if file_name.ends_with(".wav") {
            let file = File::open(&staged)?;
            let reader = WaveReader::open(file, MAX_INPUT_BYTES).map_err(domain_error)?;
            if reader.info().sample_rate.0 != loudness.sample_rate
                || reader.info().channels != loudness.channels
                || reader.info().frames != loudness.frames
            {
                return Err(Error::new(
                    ErrorCode::BackendFailed,
                    "Independent WAVE decode disagrees with loudness media shape",
                ));
            }
            Some(
                reader
                    .analyze(-90_000, (u64::from(loudness.sample_rate) / 100).max(1))
                    .map_err(domain_error)?,
            )
        } else {
            None
        };
        drop(temp);
        Ok(AnalysisReceipt {
            file_name: file_name.into(),
            sha256,
            bytes,
            format: if file_name.ends_with(".wav") {
                "wav"
            } else {
                "flac"
            },
            loudness,
            statistics,
        })
    }

    async fn run_meter(
        &self,
        context: Option<&DriverExecutionContext>,
        staged: &Path,
        file_name: &str,
        expected_sha256: &str,
        expected_bytes: u64,
        layout: &str,
    ) -> Result<LoudnessAnalysis> {
        let context = context.ok_or_else(|| {
            Error::new(
                ErrorCode::Unsupported,
                "Audio analysis requires an authenticated Driver execution context",
            )
        })?;
        context.check_cancelled()?;
        let output = if self.host_snapshot_meter
            && context.runtime_tool_mode(METER_NAME)? == RuntimeToolMode::HostMediated
        {
            context
                .execute_runtime_tool_args(
                    crate::meter_snapshot::TOOL_NAME,
                    vec![
                        RuntimeToolArg::Literal {
                            value: "--input-root".into(),
                        },
                        RuntimeToolArg::MountPath {
                            mount: "analysis-input".into(),
                            relative: String::new(),
                        },
                        RuntimeToolArg::Literal {
                            value: "--meter".into(),
                        },
                        RuntimeToolArg::ToolPath {
                            tool: METER_NAME.into(),
                        },
                        RuntimeToolArg::Literal {
                            value: format!("--file-name={file_name}"),
                        },
                        RuntimeToolArg::Literal {
                            value: format!("--expected-sha256={expected_sha256}"),
                        },
                        RuntimeToolArg::Literal {
                            value: format!("--expected-bytes={expected_bytes}"),
                        },
                        RuntimeToolArg::Literal {
                            value: format!("--layout={layout}"),
                        },
                    ],
                    Vec::new(),
                    Duration::from_secs(30),
                    None,
                )
                .await?
        } else {
            context
                .execute_runtime_tool(
                    METER_NAME,
                    vec![
                        "analyze".into(),
                        staged.to_string_lossy().into_owned(),
                        layout.into(),
                    ],
                    Vec::new(),
                    Duration::from_secs(30),
                )
                .await?
        };
        if output.stdout.len() > MAX_OUTPUT_BYTES || output.stderr.len() > MAX_OUTPUT_BYTES {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Audio meter output exceeded limit",
            ));
        }
        if output.exit_code != 0 {
            return Err(Error::new(
                ErrorCode::BackendFailed,
                format!(
                    "Pinned audio meter failed with {} stderr bytes",
                    output.stderr.len()
                ),
            ));
        }
        let value: LoudnessAnalysis = serde_json::from_slice(&output.stdout)?;
        value.validate().map_err(domain_error)?;
        Ok(value)
    }
}

fn snapshot_input(source: &Path, file_name: &str) -> Result<(TempDir, PathBuf, String, u64)> {
    snapshot_input_bounded(source, file_name, MAX_INPUT_BYTES)
}

pub(crate) fn snapshot_input_bounded(
    source: &Path,
    file_name: &str,
    max_bytes: u64,
) -> Result<(TempDir, PathBuf, String, u64)> {
    let before = fs::symlink_metadata(source)?;
    regular_metadata(&before, max_bytes)?;
    let mut input = File::open(source)?;
    let opened = input.metadata()?;
    same_file(&before, &opened)?;
    let temp = tempfile::Builder::new()
        .prefix("semwright-audio-analysis-")
        .tempdir()?;
    let staged = temp.path().join(file_name);
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&staged)?;
    let mut hash = Sha256::new();
    let mut copied = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        copied = copied
            .checked_add(count as u64)
            .filter(|size| *size <= max_bytes)
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::ResourceExhausted,
                    "Audio input copy exceeded limit",
                )
            })?;
        output.write_all(&buffer[..count])?;
        hash.update(&buffer[..count]);
    }
    output.sync_all()?;
    if copied != opened.len() {
        return Err(Error::new(
            ErrorCode::Conflict,
            "Audio input changed while taking an immutable snapshot",
        ));
    }
    regular(&staged, max_bytes)?;
    Ok((temp, staged, hex::encode(hash.finalize()), copied))
}

fn same_file(before: &fs::Metadata, opened: &fs::Metadata) -> Result<()> {
    if before.len() != opened.len() {
        return Err(Error::new(
            ErrorCode::Conflict,
            "Audio input changed before snapshot",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if before.dev() != opened.dev() || before.ino() != opened.ino() {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Audio input identity changed",
            ));
        }
    }
    Ok(())
}

fn regular(path: &Path, limit: u64) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    regular_metadata(&metadata, limit)
}
fn regular_metadata(metadata: &fs::Metadata, limit: u64) -> Result<()> {
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() == 0
        || metadata.len() > limit
    {
        return Err(Error::invalid("Expected bounded regular audio input"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "Audio input hardlinks are not accepted",
            ));
        }
    }
    Ok(())
}
pub(crate) fn validate_name(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 200
        || value.starts_with('.')
        || value.contains("..")
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        || !(value.ends_with(".wav") || value.ends_with(".flac"))
    {
        return Err(Error::invalid("Invalid audio analysis filename"));
    }
    Ok(())
}
pub(crate) fn validate_digest(value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(Error::invalid("Invalid expected audio digest"));
    }
    Ok(())
}
fn domain_error(error: semwright_audio_domain::Error) -> Error {
    let code = match error.code {
        "InvalidArgument" => ErrorCode::InvalidArgument,
        "Unsupported" => ErrorCode::Unsupported,
        "ResourceExhausted" => ErrorCode::ResourceExhausted,
        _ => ErrorCode::BackendFailed,
    };
    Error::new(code, error.message)
}
