use crate::faust::FaustProgram;
use semwright_audio_domain::{
    Error, Result,
    render::{AudioFormat, BitDepth, RenderIntent},
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tempfile::TempDir;
use tokio::{process::Command, time::timeout};

const MAX_CONFIG_BYTES: u64 = 32_768;
const MAX_TOOL_BYTES: u64 = 512 * 1024 * 1024;
const MAX_LOG_BYTES: usize = 128 * 1024;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PinnedFile {
    pub path: PathBuf,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeConfig {
    pub faust: PinnedFile,
    pub cxx: PinnedFile,
    pub pkg_config: PinnedFile,
    pub sndfile_architecture: PinnedFile,
    pub ffmpeg: Option<PinnedFile>,
    pub library_path: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct Runtime {
    config: RuntimeConfig,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Artifact {
    pub file_name: String,
    pub sha256: String,
    pub bytes: u64,
    pub format: AudioFormat,
}

impl Runtime {
    pub fn load_production() -> Result<Option<Self>> {
        let path = Path::new("/workspace/runtime/runtime.json");
        if !path.is_file() {
            return Ok(None);
        }
        let metadata = fs::symlink_metadata(path)
            .map_err(|_| Error::new("Unavailable", "Could not inspect Faust runtime config"))?;
        if metadata.file_type().is_symlink() || metadata.len() > MAX_CONFIG_BYTES {
            return Err(Error::invalid(
                "Faust runtime config is not a bounded regular file",
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o022 != 0 {
                return Err(Error::new(
                    "PermissionDenied",
                    "Faust runtime config must not be group/other writable",
                ));
            }
        }
        let bytes = fs::read(path)
            .map_err(|_| Error::new("Unavailable", "Could not read Faust runtime config"))?;
        let config: RuntimeConfig = serde_json::from_slice(&bytes)
            .map_err(|_| Error::invalid("Malformed Faust runtime config"))?;
        Self::from_config(config, true).map(Some)
    }

    pub fn from_config(config: RuntimeConfig, require_workspace_runtime: bool) -> Result<Self> {
        for value in [
            &config.faust,
            &config.cxx,
            &config.pkg_config,
            &config.sndfile_architecture,
        ] {
            verify_pinned(value, require_workspace_runtime)?;
        }
        if let Some(ffmpeg) = &config.ffmpeg {
            verify_pinned(ffmpeg, require_workspace_runtime)?;
        }
        if let Some(path) = &config.library_path {
            validate_absolute(path, require_workspace_runtime)?;
            let metadata = fs::symlink_metadata(path)
                .map_err(|_| Error::new("Unavailable", "Faust library path is unavailable"))?;
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err(Error::invalid(
                    "Faust library path must be a real directory",
                ));
            }
        }
        Ok(Self { config })
    }

    pub async fn version(&self) -> Result<String> {
        let output = self
            .run_bounded(
                &self.config.faust.path,
                ["-version"],
                None,
                Duration::from_secs(10),
            )
            .await?;
        let value = String::from_utf8_lossy(&output.stdout);
        let value = sanitize_line(&value);
        if value.is_empty() {
            return Err(Error::new(
                "BackendFailed",
                "Faust returned an empty version",
            ));
        }
        Ok(value)
    }

    pub async fn validate_program(&self, program: &FaustProgram) -> Result<()> {
        let temp = private_temp()?;
        let dsp = temp.path().join("semantic.dsp");
        let cpp = temp.path().join("semantic.cpp");
        fs::write(&dsp, &program.source)
            .map_err(|_| Error::new("BackendFailed", "Could not stage Faust source"))?;

        let mut args = vec!["-i".to_string(), "-json".to_string()];
        if let Some(path) = &self.config.library_path {
            args.extend(["-I".into(), path.display().to_string()]);
        }
        args.extend([
            dsp.display().to_string(),
            "-o".into(),
            cpp.display().to_string(),
        ]);
        self.run_bounded(
            &self.config.faust.path,
            args,
            Some(temp.path()),
            Duration::from_secs(20),
        )
        .await?;
        if !cpp.is_file() {
            return Err(Error::new(
                "BackendFailed",
                "Faust validation did not produce bounded compiler output",
            ));
        }
        Ok(())
    }

    pub async fn render(
        &self,
        program: &FaustProgram,
        intent: &RenderIntent,
        project: &semwright_audio_domain::model::AudioProject,
        output_dir: &Path,
        file_name: &str,
    ) -> Result<Artifact> {
        let frames = intent.validate_against(project)?;
        validate_file_name(file_name, intent.format)?;
        if program.outputs != intent.channels {
            return Err(Error::invalid(
                "Faust program output count differs from render intent",
            ));
        }

        let temp = private_temp()?;
        let dsp = temp.path().join("semantic.dsp");
        let cpp = temp.path().join("semantic.cpp");
        let renderer = temp.path().join("semantic-renderer");
        let wav = temp.path().join("render.wav");
        fs::write(&dsp, &program.source)
            .map_err(|_| Error::new("BackendFailed", "Could not stage Faust source"))?;

        self.compile_renderer(temp.path(), &dsp, &cpp, &renderer)
            .await?;

        let bit_depth = match intent.bit_depth {
            BitDepth::Pcm16 => 16,
            BitDepth::Pcm24 => 24,
            BitDepth::Pcm32 => 32,
        };
        let args = vec![
            wav.display().to_string(),
            "-bd".into(),
            bit_depth.to_string(),
            "-sr".into(),
            intent.sample_rate.0.to_string(),
            "-s".into(),
            frames.to_string(),
        ];
        self.run_bounded(&renderer, args, Some(temp.path()), Duration::from_secs(120))
            .await?;
        ensure_nonempty_regular(&wav)?;

        let staged = match intent.format {
            AudioFormat::Wav => wav,
            AudioFormat::Flac => {
                let ffmpeg = self.config.ffmpeg.as_ref().ok_or_else(|| {
                    Error::new(
                        "Unavailable",
                        "Pinned FFmpeg is required for deterministic FLAC export",
                    )
                })?;
                let flac = temp.path().join("render.flac");
                let args = vec![
                    "-hide_banner".into(),
                    "-loglevel".into(),
                    "error".into(),
                    "-nostdin".into(),
                    "-fflags".into(),
                    "+bitexact".into(),
                    "-i".into(),
                    wav.display().to_string(),
                    "-map_metadata".into(),
                    "-1".into(),
                    "-c:a".into(),
                    "flac".into(),
                    "-flags:a".into(),
                    "+bitexact".into(),
                    flac.display().to_string(),
                ];
                self.run_bounded(
                    &ffmpeg.path,
                    args,
                    Some(temp.path()),
                    Duration::from_secs(120),
                )
                .await?;
                ensure_nonempty_regular(&flac)?;
                flac
            }
        };

        fs::create_dir_all(output_dir)
            .map_err(|_| Error::new("BackendFailed", "Could not open audio output directory"))?;
        let final_path = output_dir.join(file_name);
        let mut input = File::open(&staged)
            .map_err(|_| Error::new("BackendFailed", "Could not open staged audio artifact"))?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&final_path)
            .map_err(|_| Error::new("Conflict", "Audio output file already exists or is unsafe"))?;
        let bytes = std::io::copy(&mut input, &mut output)
            .map_err(|_| Error::new("BackendFailed", "Could not publish audio artifact"))?;
        output
            .flush()
            .map_err(|_| Error::new("BackendFailed", "Could not flush audio artifact"))?;
        let digest = hash_file(&final_path)?;
        Ok(Artifact {
            file_name: file_name.into(),
            sha256: digest,
            bytes,
            format: intent.format,
        })
    }

    async fn compile_renderer(
        &self,
        cwd: &Path,
        dsp: &Path,
        cpp: &Path,
        renderer: &Path,
    ) -> Result<()> {
        let mut faust_args = vec![
            "-i".into(),
            "-a".into(),
            self.config.sndfile_architecture.path.display().to_string(),
        ];
        if let Some(path) = &self.config.library_path {
            faust_args.extend(["-I".into(), path.display().to_string()]);
        }
        faust_args.extend([
            dsp.display().to_string(),
            "-o".into(),
            cpp.display().to_string(),
        ]);
        self.run_bounded(
            &self.config.faust.path,
            faust_args,
            Some(cwd),
            Duration::from_secs(30),
        )
        .await?;
        ensure_nonempty_regular(cpp)?;

        let flags = self.pkg_config_sndfile(cwd).await?;
        let mut args = vec![
            "-std=c++17".into(),
            "-O2".into(),
            "-DFILE_MODE=OUTPUT_FILE".into(),
            cpp.display().to_string(),
        ];
        args.extend(flags);
        args.extend(["-o".into(), renderer.display().to_string()]);
        self.run_bounded(
            &self.config.cxx.path,
            args,
            Some(cwd),
            Duration::from_secs(60),
        )
        .await?;
        ensure_nonempty_regular(renderer)
    }

    async fn pkg_config_sndfile(&self, cwd: &Path) -> Result<Vec<String>> {
        let output = self
            .run_bounded(
                &self.config.pkg_config.path,
                ["--cflags", "--libs", "sndfile"],
                Some(cwd),
                Duration::from_secs(10),
            )
            .await?;
        let text = String::from_utf8(output.stdout)
            .map_err(|_| Error::new("BackendFailed", "pkg-config output was not UTF-8"))?;
        if text.len() > 16_384 {
            return Err(Error::limit("pkg-config output exceeds budget"));
        }
        let mut out = Vec::new();
        for token in text.split_ascii_whitespace() {
            let safe = token == "-pthread"
                || token.starts_with("-I/")
                || token.starts_with("-L/")
                || token.starts_with("-l")
                || token.starts_with("-D");
            if !safe
                || token.len() > 4096
                || token.chars().any(char::is_control)
                || token.contains([';', '&', '|', '`', '$'])
            {
                return Err(Error::new(
                    "SandboxDenied",
                    "Unexpected compiler flag from pinned pkg-config",
                ));
            }
            out.push(token.to_owned());
        }
        if !out.iter().any(|value| value == "-lsndfile") {
            return Err(Error::new(
                "Unavailable",
                "Pinned pkg-config did not resolve libsndfile",
            ));
        }
        Ok(out)
    }

    async fn run_bounded<I, S>(
        &self,
        executable: &Path,
        args: I,
        cwd: Option<&Path>,
        timeout_for: Duration,
    ) -> Result<std::process::Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        let mut command = Command::new(executable);
        command
            .args(args)
            .env_clear()
            .env("LANG", "C")
            .env("LC_ALL", "C")
            .env("HOME", cwd.unwrap_or(Path::new("/tmp")))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }
        let child = command
            .spawn()
            .map_err(|_| Error::new("Unavailable", "Could not spawn pinned audio runtime tool"))?;
        let output = timeout(timeout_for, child.wait_with_output())
            .await
            .map_err(|_| Error::new("Timeout", "Pinned audio runtime tool timed out"))?
            .map_err(|_| Error::new("BackendFailed", "Pinned audio runtime tool failed to join"))?;
        if output.stdout.len() > MAX_LOG_BYTES || output.stderr.len() > MAX_LOG_BYTES {
            return Err(Error::limit("Pinned audio runtime log budget exceeded"));
        }
        if !output.status.success() {
            return Err(Error::new(
                "BackendFailed",
                format!(
                    "Pinned audio runtime exited unsuccessfully: {}",
                    sanitize_line(&String::from_utf8_lossy(&output.stderr))
                ),
            ));
        }
        Ok(output)
    }
}

fn private_temp() -> Result<TempDir> {
    tempfile::Builder::new()
        .prefix("semwright-faust-")
        .tempdir()
        .map_err(|_| Error::new("BackendFailed", "Could not create private Faust workspace"))
}

fn verify_pinned(value: &PinnedFile, require_workspace_runtime: bool) -> Result<()> {
    validate_absolute(&value.path, require_workspace_runtime)?;
    if value.sha256.len() != 64 || !value.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::invalid("Pinned runtime SHA-256 is invalid"));
    }
    let metadata = fs::symlink_metadata(&value.path)
        .map_err(|_| Error::new("Unavailable", "Pinned audio runtime file is unavailable"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > MAX_TOOL_BYTES {
        return Err(Error::invalid(
            "Pinned audio runtime path is not a bounded regular file",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o022 != 0 {
            return Err(Error::new(
                "PermissionDenied",
                "Pinned audio runtime file is group/other writable",
            ));
        }
    }
    if hash_file(&value.path)? != value.sha256.to_ascii_lowercase() {
        return Err(Error::new(
            "Conflict",
            "Pinned audio runtime digest mismatch",
        ));
    }
    Ok(())
}

fn validate_absolute(path: &Path, require_workspace_runtime: bool) -> Result<()> {
    if !path.is_absolute()
        || path.as_os_str().len() > 4096
        || (require_workspace_runtime && !path.starts_with("/workspace/runtime"))
    {
        return Err(Error::invalid(
            "Audio runtime path is outside owner-pinned runtime",
        ));
    }
    Ok(())
}

fn hash_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)
        .map_err(|_| Error::new("Unavailable", "Could not open pinned audio runtime file"))?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| Error::new("BackendFailed", "Could not hash audio runtime file"))?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(read as u64)
            .ok_or_else(|| Error::limit("Audio runtime file size overflow"))?;
        if total > MAX_TOOL_BYTES {
            return Err(Error::limit("Audio runtime file exceeds hashing budget"));
        }
        hash.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn validate_file_name(value: &str, format: AudioFormat) -> Result<()> {
    if value.is_empty()
        || value.len() > 240
        || value.starts_with('.')
        || value.contains("..")
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return Err(Error::invalid("Audio output filename is invalid"));
    }
    let expected = match format {
        AudioFormat::Wav => ".wav",
        AudioFormat::Flac => ".flac",
    };
    if !value.to_ascii_lowercase().ends_with(expected) {
        return Err(Error::invalid(
            "Audio output extension does not match render format",
        ));
    }
    Ok(())
}

fn ensure_nonempty_regular(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| Error::new("BackendFailed", "Expected audio runtime output is absent"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() == 0 {
        return Err(Error::new(
            "BackendFailed",
            "Audio runtime output is not a non-empty regular file",
        ));
    }
    Ok(())
}

fn sanitize_line(value: &str) -> String {
    value
        .chars()
        .filter(|value| !value.is_control() || *value == ' ')
        .take(1024)
        .collect::<String>()
        .trim()
        .to_owned()
}
