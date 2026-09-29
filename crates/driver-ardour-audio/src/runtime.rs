use crate::{
    native::ArdourSnapshot,
    script::{self, NativeMutation, RESULT_PREFIX},
};
use semwright_audio_domain::wav::WaveReader;
use semwright_driver_sdk::{DriverExecutionContext, tool_path, workspace_mount};
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
pub struct AllowedPlugin {
    pub id: String,
    pub native_name: String,
    pub kind: String,
    #[serde(default)]
    pub preset: String,
    #[serde(default)]
    pub unique_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeConfig {
    pub schema_version: u32,
    pub ardour_version: String,
    #[serde(default)]
    pub allowed_plugins: Vec<AllowedPlugin>,
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
pub struct ArdourRuntimeProbe {
    pub ardour_version: String,
    pub lua_banner: String,
    pub create_banner: String,
    pub export_banner: String,
    pub create_self_test: bool,
    pub create_diagnostic_class: String,
    pub create_diagnostic_prefix: String,
    pub reopen_self_test: bool,
    pub reopen_diagnostic_class: String,
    pub reopen_diagnostic_prefix: String,
    pub snapshot_self_test: bool,
    pub snapshot_diagnostic_class: String,
    pub snapshot_diagnostic_prefix: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct ToolRun {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    exit_code: i32,
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
        if config.schema_version != 1
            || config.ardour_version != "8.4.0"
            || config.allowed_plugins.len() > 64
        {
            return Err(Error::new(
                ErrorCode::ProtocolMismatch,
                "Unsupported pinned Ardour runtime manifest",
            ));
        }
        let mut plugin_ids = std::collections::BTreeSet::new();
        for plugin in &config.allowed_plugins {
            if !valid_slug(&plugin.id)
                || !plugin_ids.insert(plugin.id.as_str())
                || plugin.native_name.is_empty()
                || plugin.native_name.len() > 1024
                || plugin.native_name.chars().any(char::is_control)
                || !matches!(plugin.kind.as_str(), "lua" | "lv2")
                || plugin.preset.len() > 1024
                || plugin.preset.chars().any(char::is_control)
                || plugin.unique_id.as_ref().is_some_and(|value| {
                    value.is_empty() || value.len() > 1024 || value.chars().any(char::is_control)
                })
            {
                return Err(Error::invalid("Invalid Ardour plugin allowlist entry"));
            }
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

    pub fn allowed_plugin(&self, id: &str) -> Result<&AllowedPlugin> {
        self.config
            .allowed_plugins
            .iter()
            .find(|plugin| plugin.id == id)
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::PolicyDenied,
                    "Plugin is outside the owner-pinned Ardour allowlist",
                )
            })
    }

    pub fn allowed_plugin_ids(&self) -> Vec<String> {
        self.config
            .allowed_plugins
            .iter()
            .map(|plugin| plugin.id.clone())
            .collect()
    }

    pub async fn probe(
        &self,
        context: Option<&DriverExecutionContext>,
    ) -> Result<ArdourRuntimeProbe> {
        let lua = self
            .run_tool(context, &self.lua_tool, &["-V".into()])
            .await?;
        let create = self
            .run_tool(context, &self.create_tool, &["-V".into()])
            .await?;
        let export = self
            .run_tool(context, &self.export_tool, &["-V".into()])
            .await?;
        let lua_banner = version_banner(&lua.stdout, "ardour-lua")?;
        let create_banner = version_banner(&create.stdout, "ardour-utils")?;
        let export_banner = version_banner(&export.stdout, "ardour-utils")?;
        for banner in [&lua_banner, &create_banner, &export_banner] {
            if !banner.contains("8.4") {
                return Err(Error::new(
                    ErrorCode::ProtocolMismatch,
                    "Pinned Ardour utility version differs from the managed 8.4 baseline",
                ));
            }
        }
        let probe_root = tempfile::Builder::new()
            .prefix("semwright-ardour-create-probe-")
            .tempdir()?;
        let probe_state = "Probe";
        let probe_session = probe_root.path().join("managed-session");
        let probe_args = vec![
            "-s".into(),
            "48000".into(),
            probe_session.to_string_lossy().into_owned(),
            probe_state.into(),
        ];
        let probe_run = self
            .run_tool_capture(context, &self.create_tool, &probe_args)
            .await?;
        let probe_state_file = probe_session.join(format!("{probe_state}.ardour"));
        let (create_self_test, create_diagnostic_class, create_diagnostic_prefix) = if probe_run
            .exit_code
            == 0
            && probe_state_file.is_file()
        {
            (
                true,
                "ok".to_string(),
                bounded_text_diagnostic("native session artifact created"),
            )
        } else {
            let classified = if probe_run.exit_code != 0 {
                classify_tool_failure(&probe_run.stdout, &probe_run.stderr)
            } else {
                classify_create_failure(&probe_run.stdout, &probe_run.stderr)
            };
            (
                false,
                format!("{:?}", classified.code),
                bounded_text_diagnostic(&bounded_diagnostic(&probe_run.stdout, &probe_run.stderr)),
            )
        };
        let (reopen_self_test, reopen_diagnostic_class, reopen_diagnostic_prefix) =
            if create_self_test {
                let script_dir = tempfile::Builder::new()
                    .prefix("semwright-ardour-reopen-probe-")
                    .tempdir()?;
                let script = script_dir.path().join("probe.lua");
                fs::write(
                    &script,
                    r#"local dir = arg[1]
local state = arg[2]
load_session(dir, state)
if not Session then error("reopen failed") end
print("SEMWRIGHT_ARDOUR_REOPEN_OK")
close_session()
"#,
                )?;
                let reopen_args = vec![
                    script.to_string_lossy().into_owned(),
                    probe_session.to_string_lossy().into_owned(),
                    probe_state.into(),
                ];
                let reopen_run = self
                    .run_tool_capture(context, &self.lua_tool, &reopen_args)
                    .await?;
                let stdout = String::from_utf8_lossy(&reopen_run.stdout);
                if reopen_run.exit_code == 0 && stdout.contains("SEMWRIGHT_ARDOUR_REOPEN_OK") {
                    (
                        true,
                        "ok".to_string(),
                        bounded_text_diagnostic("native session reopened through ardour-lua"),
                    )
                } else {
                    let classified = if reopen_run.exit_code != 0 {
                        classify_tool_failure(&reopen_run.stdout, &reopen_run.stderr)
                    } else {
                        Error::new(
                            ErrorCode::ProtocolMismatch,
                            "Ardour reopen probe did not emit the expected marker",
                        )
                    };
                    (
                        false,
                        format!("{:?}", classified.code),
                        bounded_text_diagnostic(&bounded_diagnostic(
                            &reopen_run.stdout,
                            &reopen_run.stderr,
                        )),
                    )
                }
            } else {
                (
                    false,
                    "create_prerequisite_failed".to_string(),
                    String::new(),
                )
            };
        let (snapshot_self_test, snapshot_diagnostic_class, snapshot_diagnostic_prefix) =
            if reopen_self_test {
                let script_dir = tempfile::Builder::new()
                    .prefix("semwright-ardour-snapshot-probe-")
                    .tempdir()?;
                let script_path = script_dir.path().join("semwright-ardour.lua");
                fs::write(&script_path, script::source())?;
                let snapshot_args = vec![
                    script_path.to_string_lossy().into_owned(),
                    probe_session.to_string_lossy().into_owned(),
                    probe_state.into(),
                    self.config.ardour_version.clone(),
                    "inspect".into(),
                ];
                let snapshot_run = self
                    .run_tool_capture(context, &self.lua_tool, &snapshot_args)
                    .await?;
                if snapshot_run.exit_code != 0 {
                    let classified =
                        classify_tool_failure(&snapshot_run.stdout, &snapshot_run.stderr);
                    (
                        false,
                        format!("{:?}", classified.code),
                        bounded_text_diagnostic(&bounded_diagnostic(
                            &snapshot_run.stdout,
                            &snapshot_run.stderr,
                        )),
                    )
                } else {
                    match parse_snapshot(&snapshot_run.stdout) {
                        Ok(_) => (
                            true,
                            "ok".to_string(),
                            bounded_text_diagnostic(
                                "fixed Ardour semantic snapshot parsed and validated",
                            ),
                        ),
                        Err(error) => (
                            false,
                            format!("{:?}", error.code),
                            bounded_text_diagnostic(&error.message),
                        ),
                    }
                }
            } else {
                (
                    false,
                    "reopen_prerequisite_failed".to_string(),
                    String::new(),
                )
            };
        Ok(ArdourRuntimeProbe {
            ardour_version: self.config.ardour_version.clone(),
            lua_banner,
            create_banner,
            export_banner,
            create_self_test,
            create_diagnostic_class,
            create_diagnostic_prefix,
            reopen_self_test,
            reopen_diagnostic_class,
            reopen_diagnostic_prefix,
            snapshot_self_test,
            snapshot_diagnostic_class,
            snapshot_diagnostic_prefix,
        })
    }

    pub async fn inspect(
        &self,
        context: Option<&DriverExecutionContext>,
        state: &str,
    ) -> Result<ArdourSnapshot> {
        self.run_lua(context, state, &["inspect".to_string()]).await
    }

    pub async fn mutate(
        &self,
        context: Option<&DriverExecutionContext>,
        state: &str,
        mutation: &NativeMutation,
    ) -> Result<ArdourSnapshot> {
        let args = mutation.argv().map_err(domain_error)?;
        self.run_lua(context, state, &args).await
    }

    pub async fn create(
        &self,
        context: Option<&DriverExecutionContext>,
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
        let session_dir = self.managed_session_dir();
        let state_file = self.state_path(state);
        if session_dir.try_exists()? {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Managed Ardour session directory already exists",
            ));
        }
        let args = vec![
            "-s".into(),
            sample_rate.to_string(),
            session_dir.to_string_lossy().into_owned(),
            state.into(),
        ];
        let creation = self.run_tool(context, &self.create_tool, &args).await?;
        // Ardour 8.4's utility has internal error paths that still return zero.
        // The native state artifact and a clean reopen are the acceptance signal.
        if !state_file.try_exists()? {
            return Err(classify_create_failure(&creation.stdout, &creation.stderr));
        }
        regular(&state_file, 64 * 1024 * 1024)?;
        let snapshot = self
            .mutate(
                context,
                state,
                &NativeMutation::MasterCreate {
                    channels: master_channels,
                },
            )
            .await?;
        let stereo_master = snapshot.routes.iter().any(|route| {
            route.kind == crate::native::RouteKind::Master && route.channels == master_channels
        });
        if !stereo_master {
            return Err(Error::new(
                ErrorCode::BackendFailed,
                "Created Ardour session did not reopen with the required stereo master",
            ));
        }
        Ok(snapshot)
    }

    pub async fn save_as(
        &self,
        context: Option<&DriverExecutionContext>,
        source_state: &str,
        candidate_state: &str,
    ) -> Result<ArdourSnapshot> {
        validate_state(source_state)?;
        validate_state(candidate_state)?;
        if source_state == candidate_state {
            return Err(Error::invalid(
                "Ardour save-as requires a distinct snapshot name",
            ));
        }
        let candidate = self.state_path(candidate_state);
        if candidate.try_exists()? {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Ardour save-as target already exists",
            ));
        }
        self.mutate(
            context,
            source_state,
            &NativeMutation::SaveAs {
                state: candidate_state.into(),
            },
        )
        .await?;
        regular(&candidate, 64 * 1024 * 1024)?;
        // Reopen the candidate independently; success is not inferred from Lua return alone.
        self.inspect(context, candidate_state).await
    }

    pub async fn export_wav(
        &self,
        context: Option<&DriverExecutionContext>,
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
            self.managed_session_dir().to_string_lossy().into_owned(),
            state.into(),
        ];
        let export_run = self.run_tool(context, &self.export_tool, &args).await?;
        // export.cc in Ardour 8.4 does not propagate export_session() failure through main().
        if !output.try_exists()? {
            return Err(classify_export_failure(
                &export_run.stdout,
                &export_run.stderr,
            ));
        }
        regular(&output, MAX_ARTIFACT_BYTES)?;
        let sha256 = file_sha256(&output, MAX_ARTIFACT_BYTES)?;
        let bytes = fs::metadata(&output)?.len();
        let reader =
            WaveReader::open(File::open(&output)?, MAX_ARTIFACT_BYTES).map_err(domain_error)?;
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

    async fn run_lua(
        &self,
        context: Option<&DriverExecutionContext>,
        state: &str,
        operation_args: &[String],
    ) -> Result<ArdourSnapshot> {
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
                self.managed_session_dir().to_string_lossy().into_owned(),
                state.into(),
                self.config.ardour_version.clone(),
            ],
            operation_args.to_vec(),
        ]
        .concat();
        let run = self.run_tool(context, &self.lua_tool, &args).await?;
        parse_snapshot(&run.stdout)
    }

    async fn run_tool(
        &self,
        context: Option<&DriverExecutionContext>,
        tool: &Path,
        args: &[String],
    ) -> Result<ToolRun> {
        let run = self.run_tool_capture(context, tool, args).await?;
        if run.exit_code != 0 {
            return Err(classify_tool_failure(&run.stdout, &run.stderr));
        }
        Ok(run)
    }

    async fn run_tool_capture(
        &self,
        context: Option<&DriverExecutionContext>,
        tool: &Path,
        args: &[String],
    ) -> Result<ToolRun> {
        if let Some(context) = context {
            context.check_cancelled()?;
        }
        let temp_home = TempDir::new()?;
        let mut command = Command::new(tool);
        command
            .args(args)
            .current_dir(temp_home.path())
            .env_clear()
            .env("HOME", temp_home.path())
            .env("XDG_CACHE_HOME", temp_home.path().join(".cache"))
            .env("XDG_CONFIG_HOME", temp_home.path().join(".config"))
            .env("XDG_DATA_HOME", temp_home.path().join(".local/share"))
            .env("PATH", "/usr/bin:/bin")
            .env("LANG", "C.UTF-8")
            .env("LC_ALL", "C.UTF-8")
            .env("LD_LIBRARY_PATH", "/usr/lib/ardour8")
            .env("ARDOUR_DATA_PATH", "/usr/share/ardour8")
            .env("ARDOUR_CONFIG_PATH", "/etc/ardour8")
            .env("ARDOUR_DLL_PATH", "/usr/lib/ardour8")
            .env("VAMP_PATH", "/usr/lib/ardour8/vamp")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        for relative in [".cache", ".config", ".local/share"] {
            fs::create_dir_all(temp_home.path().join(relative))?;
        }
        let mut child = command.spawn().map_err(|error| match error.kind() {
            std::io::ErrorKind::PermissionDenied => Error::new(
                ErrorCode::SandboxDenied,
                "Sandbox denied execution of the pinned Ardour utility",
            ),
            std::io::ErrorKind::NotFound => Error::new(
                ErrorCode::Unavailable,
                "Pinned Ardour utility or dynamic loader is unavailable",
            ),
            _ => Error::new(
                ErrorCode::BackendFailed,
                "Could not start the pinned Ardour utility",
            ),
        })?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::unavailable("Ardour stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| Error::unavailable("Ardour stderr"))?;
        let execution = async {
            let (stdout, stderr, status) = tokio::try_join!(
                read_bounded(stdout, MAX_STDOUT_BYTES),
                read_bounded(stderr, MAX_STDERR_BYTES),
                async { child.wait().await.map_err(Error::from) }
            )?;
            Ok::<ToolRun, Error>(ToolRun {
                stdout,
                stderr,
                exit_code: status.code().unwrap_or(-1),
            })
        };
        let result = if let Some(context) = context {
            let cancellation = context.cancellation();
            tokio::select! {
                _ = cancellation.cancelled() => {
                    Err(Error::new(ErrorCode::Cancelled, "Ardour native operation cancelled"))
                }
                result = timeout(RUN_TIMEOUT, execution) => {
                    result.map_err(|_| {
                        Error::new(
                            ErrorCode::Timeout,
                            "Ardour native tool exceeded runtime budget",
                        )
                    })?
                }
            }
        } else {
            timeout(RUN_TIMEOUT, execution).await.map_err(|_| {
                Error::new(
                    ErrorCode::Timeout,
                    "Ardour native tool exceeded runtime budget",
                )
            })?
        };
        if result.is_err() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        result
    }

    fn managed_session_dir(&self) -> PathBuf {
        self.session_root.join("managed-session")
    }

    fn state_path(&self, state: &str) -> PathBuf {
        self.managed_session_dir().join(format!("{state}.ardour"))
    }
}

async fn read_bounded(reader: impl tokio::io::AsyncRead + Unpin, max: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take((max + 1) as u64)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() > max {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Ardour tool output exceeded bounded capture",
        ));
    }
    Ok(bytes)
}

fn bounded_diagnostic(stdout: &[u8], stderr: &[u8]) -> String {
    let mut text = String::from_utf8_lossy(stderr).into_owned();
    if !stdout.is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&String::from_utf8_lossy(stdout));
    }
    text.truncate(4096);
    text
}

fn bounded_text_diagnostic(value: &str) -> String {
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

fn classify_tool_failure(stdout: &[u8], stderr: &[u8]) -> Error {
    let text = bounded_diagnostic(stdout, stderr);
    if text.contains("Permission denied") || text.contains("Read-only file system") {
        Error::new(
            ErrorCode::SandboxDenied,
            "Ardour utility was denied required sandbox filesystem access",
        )
    } else if text.contains("error while loading shared libraries")
        || text.contains("cannot open shared object file")
        || text.contains("No such file or directory")
    {
        Error::new(
            ErrorCode::Unavailable,
            "Pinned Ardour runtime dependency is unavailable",
        )
    } else if text.contains("Cannot create Audio/MIDI engine")
        || text.contains("Cannot start Audio/MIDI engine")
        || text.contains("Cannot set session's samplerate")
    {
        Error::new(
            ErrorCode::Unavailable,
            "Ardour Dummy audio engine could not initialize inside the managed runtime",
        )
    } else if text.contains("Session file exists") {
        Error::new(ErrorCode::Conflict, "Ardour session state already exists")
    } else {
        Error::new(ErrorCode::BackendFailed, "Pinned Ardour utility failed")
    }
}

fn classify_create_failure(stdout: &[u8], stderr: &[u8]) -> Error {
    let text = bounded_diagnostic(stdout, stderr);
    if text.contains("Cannot create Audio/MIDI engine")
        || text.contains("Cannot start Audio/MIDI engine")
        || text.contains("Cannot set session's samplerate")
    {
        Error::new(
            ErrorCode::Unavailable,
            "Ardour managed-session engine initialization failed",
        )
    } else if text.contains("Permission denied") || text.contains("Read-only file system") {
        Error::new(
            ErrorCode::SandboxDenied,
            "Ardour managed-session creation was denied by confinement",
        )
    } else if text.contains("Session file exists") {
        Error::new(ErrorCode::Conflict, "Ardour session state already exists")
    } else {
        Error::new(
            ErrorCode::BackendFailed,
            "Ardour create utility returned without a native session artifact",
        )
    }
}

fn classify_export_failure(stdout: &[u8], stderr: &[u8]) -> Error {
    let text = bounded_diagnostic(stdout, stderr);
    if text.contains("Permission denied") || text.contains("Read-only file system") {
        Error::new(
            ErrorCode::SandboxDenied,
            "Ardour export was denied by confinement",
        )
    } else if text.contains("Cannot") || text.contains("failed") || text.contains("error") {
        Error::new(
            ErrorCode::BackendFailed,
            "Ardour export utility reported failure",
        )
    } else {
        Error::new(
            ErrorCode::BackendFailed,
            "Ardour export utility returned without an artifact",
        )
    }
}

fn parse_snapshot(stdout: &[u8]) -> Result<ArdourSnapshot> {
    let text = std::str::from_utf8(stdout).map_err(|_| {
        Error::new(
            ErrorCode::ProtocolMismatch,
            "Ardour adapter output is not UTF-8",
        )
    })?;
    let mut results = text
        .lines()
        .filter_map(|line| line.strip_prefix(RESULT_PREFIX));
    let encoded = results.next().ok_or_else(|| {
        Error::new(
            ErrorCode::ProtocolMismatch,
            "Ardour adapter result marker is absent",
        )
    })?;
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

fn version_banner(stdout: &[u8], prefix: &str) -> Result<String> {
    let text = std::str::from_utf8(stdout).map_err(|_| {
        Error::new(
            ErrorCode::ProtocolMismatch,
            "Ardour utility version output is not UTF-8",
        )
    })?;
    let line = text.lines().next().unwrap_or_default().trim();
    if line.is_empty()
        || line.len() > 160
        || !line.starts_with(prefix)
        || line.chars().any(char::is_control)
    {
        return Err(Error::new(
            ErrorCode::ProtocolMismatch,
            "Ardour utility version banner has an unexpected shape",
        ));
    }
    Ok(line.to_owned())
}

fn valid_slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
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
        return Err(Error::invalid(format!(
            "{label} grant must be a real directory"
        )));
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
        return Err(Error::invalid(
            "Expected bounded regular Ardour runtime file",
        ));
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
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::ResourceExhausted,
                    "Ardour artifact hash budget exceeded",
                )
            })?;
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
            groups: vec![],
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
