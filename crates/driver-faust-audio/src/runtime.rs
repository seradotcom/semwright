//! Production uses immutable owner-pinned tools inside the Linux Driver Host sandbox.
use crate::faust::FaustProgram;
use semwright_audio_domain::{
    model::AudioProject,
    render::{AudioFormat, BitDepth, DitherPolicy, RenderIntent},
};
use semwright_driver_sdk::{DriverExecutionContext, tool_path, workspace_mount};
use semwright_types::{Error, ErrorCode, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};

pub const HELPER_NAME: &str = "faust-interpreter";
const MAX_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;
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
        // The Host independently verifies the executable and seals it for execution.
        // File presence is availability only, never an attestation of authority.
        regular(&tool_path(HELPER_NAME)?, 64 * 1024 * 1024)?;
        Ok(Some(result))
    }
    pub fn version(&self) -> &str {
        &self.config.compiler_version
    }
    pub async fn probe(&self, context: &DriverExecutionContext) -> Result<serde_json::Value> {
        context.check_cancelled()?;
        self.verify_libraries()?;
        let output = run_sealed_tool(
            context,
            vec![
                "probe".into(),
                self.library_root.to_string_lossy().into_owned(),
            ],
            &[],
        )
        .await?;
        if output.exit_code != 0 {
            return Err(classify_tool_exit(&output.stderr));
        }
        let value: serde_json::Value = serde_json::from_slice(&output.stdout)?;
        if value["schema_version"] != 1
            || value["engine"] != "faust-interpreter"
            || value["compiler_version"] != self.config.compiler_version
            || !value["library_mount"].is_boolean()
            || !value["stdlib_regular"].is_boolean()
            || !value["stdlib_compile"].is_boolean()
            || value["diagnostic_class"].as_str().is_none()
            || value["diagnostic_prefix"].as_str().is_none()
        {
            return Err(Error::new(
                ErrorCode::ProtocolMismatch,
                "Faust runtime probe receipt differs from the pinned runtime",
            ));
        }
        Ok(value)
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

async fn bounded_output<R: tokio::io::AsyncRead + Unpin>(reader: R) -> Result<Vec<u8>> {
    let mut result = Vec::new();
    reader.take(262145).read_to_end(&mut result).await?;
    if result.len() > 262144 {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Native tool output exceeded limit",
        ));
    }
    Ok(result)
}
async fn run_sealed_tool(
    context: &DriverExecutionContext,
    args: Vec<String>,
    source: &[u8],
) -> Result<semwright_driver_sdk::ToolExecutionOutput> {
    if !cfg!(target_os = "linux") {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "Audio runtime grants are not implemented for this platform's tool broker",
        ));
    }
    context.check_cancelled()?;
    // Fixed tool identity, materialized and made immutable by the real Driver Host.
    // No input field can select an executable, environment, shell or script.
    let library_env = args.get(1).cloned();
    let mut command = Command::new(tool_path(HELPER_NAME)?);
    command
        .args(args)
        .env_clear()
        .env("HOME", "/home")
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .env("LC_ALL", "C.UTF-8")
        .env("TMPDIR", "/tmp");
    if let Some(library_root) = library_env {
        command.env("FAUST_LIB_PATH", library_root);
    }
    let mut child = command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::PermissionDenied => Error::new(
                ErrorCode::SandboxDenied,
                "Sandbox denied the pinned Faust helper execution",
            ),
            std::io::ErrorKind::NotFound => Error::new(
                ErrorCode::Unavailable,
                "Pinned Faust helper or its loader is unavailable",
            ),
            _ => Error::new(
                ErrorCode::BackendFailed,
                "Could not start the pinned Faust helper",
            ),
        })?;
    let mut input = child
        .stdin
        .take()
        .ok_or_else(|| Error::unavailable("tool stdin"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::unavailable("tool stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| Error::unavailable("tool stderr"))?;
    let cancellation = context.cancellation();
    let result = {
        let execution = async {
            let write_input = async {
                input.write_all(source).await?;
                input.shutdown().await?;
                drop(input);
                Ok::<(), Error>(())
            };
            let wait = async { child.wait().await.map_err(Error::from) };
            let (_, stdout, stderr, status) = tokio::try_join!(
                write_input,
                bounded_output(stdout),
                bounded_output(stderr),
                wait
            )?;
            Ok(semwright_driver_sdk::ToolExecutionOutput {
                exit_code: status.code().unwrap_or(-1),
                stdout,
                stderr,
            })
        };
        tokio::select! {
            _ = cancellation.cancelled() => Err(Error::new(ErrorCode::Cancelled, "Native tool cancelled")),
            result = tokio::time::timeout(Duration::from_secs(30), execution) =>
                result.unwrap_or_else(|_| Err(Error::new(ErrorCode::Timeout, "Native tool timed out"))),
        }
    };
    if result.is_err() {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    result
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
