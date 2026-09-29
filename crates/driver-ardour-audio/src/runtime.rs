use crate::{
    native::ArdourSnapshot,
    script::{self, NativeMutation, RESULT_PREFIX},
};
use semwright_audio_domain::wav::WaveReader;
use semwright_driver_sdk::{tool_path, workspace_mount};
use semwright_types::{Error, ErrorCode, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tempfile::TempDir;
use tokio::{io::AsyncReadExt, process::Command, time::timeout};

pub const LUA_TOOL: &str = "ardour-lua";
pub const CREATE_TOOL: &str = "ardour-new-session";
pub const EXPORT_TOOL: &str = "ardour-export";

const MAX_CONFIG_BYTES: u64 = 32 * 1024;
const MAX_TOOL_BYTES: u64 = 512 * 1024 * 1024;
const MAX_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;
const MAX_STDOUT_BYTES: usize = 2 * 1024 * 1024;
const MAX_STDERR_BYTES: usize = 256 * 1024;
const RUN_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeConfig {
    pub schema_version: u32,
    pub ardour_version: String,
}

#[derive(Clone, Debug)]
pub struct DeepRuntime {
    config: RuntimeConfig,
    session_root: PathBuf,
    output_root: PathBuf,
    lua_tool: PathBuf,
    create_tool: PathBuf,
    export_tool: PathBuf,
}

#[derive(Clone, Debug, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExportReceipt {
    pub file_name: String,
    pub sha256: String,
    pub bytes: u64,
    pub sample_rate: u32,
    pub channels: u16,
    pub frames: u64,
    pub bit_depth: u16,
    pub source_state: String,
    pub ardour_version: String,
}

impl DeepRuntime {
    #[cfg(unix)]
    pub fn load_production() -> Result<Option<Self>> {
        let runtime_root = match workspace_mount("ardour-runtime") {
            Ok(path) => path,
            Err(error) if error.code == ErrorCode::Unavailable => return Ok(None),
            Err(error) => return Err(error),
        };
        let config_path = runtime_root.join("semwright-runtime.json");
        if !config_path.try_exists()? {
            return Ok(None);
        }
        regular(&config_path, MAX_CONFIG_BYTES)?;
        let bytes = fs::read(&config_path)?;
        let config: RuntimeConfig = serde_json::from_slice(&bytes)?;
        Self::from_config(
            config,
            workspace_mount("ardour-project")?,
            workspace_mount("ardour-output")?,
            tool_path(LUA_TOOL)?,
            tool_path(CREATE_TOOL)?,
            tool_path(EXPORT_TOOL)?,
        )
        .map(Some)
    }

    #[cfg(not(unix))]
    pub fn load_production() -> Result<Option<Self>> {
        Ok(None)
    }

    fn from_config(
        config: RuntimeConfig,
        session_root: PathBuf,
        output_root: PathBuf,
        lua_tool: PathBuf,
        create_tool: PathBuf,
        export_tool: PathBuf,
    ) -> Result<Self> {
        if config.schema_version != 1 || config.ardour_version != "8.4.0" {
            return Err(Error::new(
                ErrorCode::ProtocolMismatch,
                "Unsupported pinned Ardour runtime manifest",
            ));
        }
        directory(&session_root, "Ardour project")?;
        directory(&output_root, "Ardour output")?;
        for tool in [&lua_tool, &create_tool, &export_tool] {
            regular(tool, MAX_TOOL_BYTES)?;
        }
        Ok(Self {
            config,
            session_root,
            output_root,
            lua_tool,
            create_tool,
            export_tool,
        })
    }

    pub fn version(&self) -> &str {
        &self.config.ardour_version
    }

    pub async fn inspect(&self, state: &str) -> Result<ArdourSnapshot> {
        self.run_lua(state, &["inspect".to_string()]).await
    }

    pub async fn mutate(&self, state: &str, mutation: &NativeMutation) -> Result<ArdourSnapshot> {
        let args = mutation.argv().map_err(domain_error)?;
        self.run_lua(state, &args).await
    }

    pub async fn create(
        &self,
        state: &str,
        sample_rate: u32,
        master_channels: u16,
    ) -> Result<ArdourSnapshot> {
        validate_state(state)?;
        if !(8_000..=192_000).contains(&sample_rate) || master_channels != 2 {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "Ardour 8.4 new_empty_session does not expose master-channel selection; managed creation is stereo",
            ));
        }
        let state_file = self.state_path(state);
        if state_file.try_exists()? {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Ardour session state already exists",
            ));
        }
        let args = vec![
            "-s".into(),
            sample_rate.to_string(),
            self.session_root.to_string_lossy().into_owned(),
            state.into(),
        ];
        self.run_tool(&self.create_tool, &args).await?;
        // Ardour 8.4's utility has internal error paths that still return zero.
        // The native state artifact and a clean reopen are the acceptance signal.
        regular(&state_file, 64 * 1024 * 1024)?;
        let snapshot = self.inspect(state).await?;
        let stereo_master = snapshot
            .routes
            .iter()
            .any(|route| route.kind == crate::native::RouteKind::Master && route.channels == 2);
        if !stereo_master {
            return Err(Error::new(
                ErrorCode::BackendFailed,
                "Created Ardour session did not reopen with the required stereo master",
            ));
        }
        Ok(snapshot)
    }

    pub async fn save_as(&self, source_state: &str, candidate_state: &str) -> Result<ArdourSnapshot> {
        validate_state(source_state)?;
        validate_state(candidate_state)?;
        if source_state == candidate_state {
            return Err(Error::invalid("Ardour save-as requires a distinct snapshot name"));
        }
        let candidate = self.state_path(candidate_state);
        if candidate.try_exists()? {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Ardour save-as target already exists",
            ));
        }
        self.mutate(
            source_state,
            &NativeMutation::SaveAs {
                state: candidate_state.into(),
            },
        )
        .await?;
        regular(&candidate, 64 * 1024 * 1024)?;
        // Reopen the candidate independently; success is not inferred from Lua return alone.
        self.inspect(candidate_state).await
    }

    pub async fn export_wav(
        &self,
        state: &str,
        file_name: &str,
        sample_rate: u32,
        bit_depth: u16,
    ) -> Result<ExportReceipt> {
        validate_state(state)?;
        validate_output_name(file_name)?;
        if !(8_000..=192_000).contains(&sample_rate) || !matches!(bit_depth, 16 | 24 | 32) {
            return Err(Error::invalid("Invalid Ardour WAV export profile"));
        }
        let output = self.output_root.join(file_name);
        if output.try_exists()? {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Ardour export target already exists",
            ));
        }
        let args = vec![
            "-b".into(),
            bit_depth.to_string(),
            "-s".into(),
            sample_rate.to_string(),
            "-o".into(),
            output.to_string_lossy().into_owned(),
            self.session_root.to_string_lossy().into_owned(),
            state.into(),
        ];
        self.run_tool(&self.export_tool, &args).await?;
        // export.cc in Ardour 8.4 does not propagate export_session() failure through main().
        regular(&output, MAX_ARTIFACT_BYTES)?;
        let sha256 = file_sha256(&output, MAX_ARTIFACT_BYTES)?;
        let bytes = fs::metadata(&output)?.len();
        let reader = WaveReader::open(File::open(&output)?, MAX_ARTIFACT_BYTES).map_err(domain_error)?;
        let info = reader.info().clone();
        if info.sample_rate.0 != sample_rate
            || info.frames == 0
            || !(1..=64).contains(&info.channels)
            || info.ieee_float
            || info.valid_bits != bit_depth
        {
            return Err(Error::new(
                ErrorCode::BackendFailed,
                "Ardour export artifact does not match requested WAV profile",
            ));
        }
        Ok(ExportReceipt {
            file_name: file_name.into(),
            sha256,
            bytes,
            sample_rate: info.sample_rate.0,
            channels: info.channels,
            frames: info.frames,
            bit_depth,
            source_state: state.into(),
            ardour_version: self.config.ardour_version.clone(),
        })
    }

    async fn run_lua(&self, state: &str, operation_args: &[String]) -> Result<ArdourSnapshot> {
        validate_state(state)?;
        if operation_args.is_empty() || operation_args.len() > 8 {
            return Err(Error::invalid("Invalid Ardour adapter argument count"));
        }
        let temp = TempDir::new()?;
        let script_path = temp.path().join("semwright-ardour.lua");
        fs::write(&script_path, script::source())?;
        let args = [
            vec![
                script_path.to_string_lossy().into_owned(),
                self.session_root.to_string_lossy().into_owned(),
                state.into(),
                self.config.ardour_version.clone(),
            ],
            operation_args.to_vec(),
        ]
        .concat();
        let stdout = self.run_tool(&self.lua_tool, &args).await?;
        parse_snapshot(&stdout)
    }

    async fn run_tool(&self, tool: &Path, args: &[String]) -> Result<Vec<u8>> {
        let temp_home = TempDir::new()?;
        let mut command = Command::new(tool);
        command
            .args(args)
            .env_clear()
            .env("HOME", temp_home.path())
            .env("LANG", "C.UTF-8")
            .env("LC_ALL", "C.UTF-8")
            .env("PATH", "/usr/bin:/bin")
            .env("LD_LIBRARY_PATH", "/usr/lib/ardour8")
            .env("ARDOUR_DATA_PATH", "/usr/share/ardour8")
            .env("ARDOUR_CONFIG_PATH", "/etc/ardour8")
            .env("ARDOUR_DLL_PATH", "/usr/lib/ardour8")
            .env("VAMP_PATH", "/usr/lib/ardour8")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn()?;
        let stdout = child.stdout.take().ok_or_else(|| Error::unavailable("Ardour stdout"))?;
        let stderr = child.stderr.take().ok_or_else(|| Error::unavailable("Ardour stderr"))?;
        let execution = async {
            let (stdout, stderr, status) = tokio::try_join!(
                read_bounded(stdout, MAX_STDOUT_BYTES),
                read_bounded(stderr, MAX_STDERR_BYTES),
                async { child.wait().await.map_err(Error::from) }
            )?;
            if !status.success() {
                return Err(Error::new(
                    ErrorCode::BackendFailed,
                    format!("Pinned Ardour tool exited unsuccessfully ({} stderr bytes)", stderr.len()),
                ));
            }
            Ok::<Vec<u8>, Error>(stdout)
        };
        timeout(RUN_TIMEOUT, execution)
            .await
            .map_err(|_| Error::new(ErrorCode::Timeout, "Ardour native tool exceeded runtime budget"))?
    }

    fn state_path(&self, state: &str) -> PathBuf {
        self.session_root.join(format!("{state}.ardour"))
    }
}

async fn read_bounded(reader: impl tokio::io::AsyncRead + Unpin, max: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take((max + 1) as u64).read_to_end(&mut bytes).await?;
    if bytes.len() > max {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Ardour tool output exceeded bounded capture",
        ));
    }
    Ok(bytes)
}

fn parse_snapshot(stdout: &[u8]) -> Result<ArdourSnapshot> {
    let text = std::str::from_utf8(stdout)
        .map_err(|_| Error::new(ErrorCode::ProtocolMismatch, "Ardour adapter output is not UTF-8"))?;
    let mut results = text.lines().filter_map(|line| line.strip_prefix(RESULT_PREFIX));
    let encoded = results
        .next()
        .ok_or_else(|| Error::new(ErrorCode::ProtocolMismatch, "Ardour adapter result marker is absent"))?;
    if results.next().is_some() {
        return Err(Error::new(
            ErrorCode::ProtocolMismatch,
            "Ardour adapter emitted multiple semantic snapshots",
        ));
    }
    let snapshot: ArdourSnapshot = serde_json::from_str(encoded)?;
    snapshot.validate().map_err(domain_error)?;
    Ok(snapshot)
}

fn validate_state(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || matches!(value, "." | "..")
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(Error::invalid("Invalid Ardour session state name"));
    }
    Ok(())
}
fn validate_output_name(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 200
        || value.starts_with('.')
        || value.contains("..")
        || !value.ends_with(".wav")
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    {
        return Err(Error::invalid("Invalid Ardour export filename"));
    }
    Ok(())
}
fn directory(path: &Path, label: &str) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(Error::invalid(format!("{label} grant must be a real directory")));
    }
    Ok(())
}
fn regular(path: &Path, limit: u64) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() == 0
        || metadata.len() > limit
    {
        return Err(Error::invalid("Expected bounded regular Ardour runtime file"));
    }
    Ok(())
}
fn file_sha256(path: &Path, limit: u64) -> Result<String> {
    let mut file = File::open(path)?.take(limit + 1);
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut size = 0u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        size = size
            .checked_add(read as u64)
            .filter(|value| *value <= limit)
            .ok_or_else(|| Error::new(ErrorCode::ResourceExhausted, "Ardour artifact hash budget exceeded"))?;
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}
fn domain_error(error: semwright_audio_domain::Error) -> Error {
    let code = match error.code {
        "InvalidArgument" => ErrorCode::InvalidArgument,
        "Unsupported" => ErrorCode::Unsupported,
        "ResourceExhausted" => ErrorCode::ResourceExhausted,
        "NotFound" => ErrorCode::NotFound,
        "StaleReference" => ErrorCode::StaleReference,
        _ => ErrorCode::BackendFailed,
    };
    Error::new(code, error.message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::{NativeRoute, RouteKind, SNAPSHOT_VERSION};

    fn fixture() -> ArdourSnapshot {
        ArdourSnapshot {
            snapshot_version: SNAPSHOT_VERSION,
            ardour_version: "8.4.0".into(),
            session_name: "fixture".into(),
            sample_rate: 48_000,
            session_start: 0,
            session_end: 0,
            routes: vec![NativeRoute {
                id: "master1".into(),
                name: "Master".into(),
                kind: RouteKind::Master,
                channels: 2,
                muted: false,
                soloed: false,
                gain_millidb: 0,
                pan_milli: 0,
                regions: vec![],
                sends: vec![],
                plugins: vec![],
                routing_complete: false,
                sends_complete: false,
                plugins_complete: false,
            }],
            warnings: vec![],
        }
    }

    #[test]
    fn strict_runtime_config_and_names_fail_closed() {
        assert!(validate_state("../session").is_err());
        assert!(validate_state("session/name").is_err());
        assert!(validate_output_name("../mix.wav").is_err());
        assert!(validate_output_name("mix.flac").is_err());
    }

    #[test]
    fn duplicate_result_markers_are_protocol_errors() {
        let json = serde_json::to_string(&fixture()).unwrap();
        let body = format!("{RESULT_PREFIX}{json}\n{RESULT_PREFIX}{json}\n");
        assert!(parse_snapshot(body.as_bytes()).is_err());
    }

    #[test]
    fn snapshot_marker_parses_valid_native_state() {
        let json = serde_json::to_string(&fixture()).unwrap();
        let body = format!("noise\n{RESULT_PREFIX}{json}\n");
        assert_eq!(parse_snapshot(body.as_bytes()).unwrap(), fixture());
    }
}
