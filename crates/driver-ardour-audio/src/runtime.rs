use crate::{
    native::ArdourSnapshot,
    script::{self, NativeMutation, RESULT_PREFIX},
};
use semwright_audio_domain::{Error, Result};
use serde::Deserialize;
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

const CONFIG_PATH: &str = "/workspace/runtime/ardour.json";
const SESSION_ROOT: &str = "/workspace/project";
const MAX_CONFIG_BYTES: u64 = 32 * 1024;
const MAX_EXECUTABLE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_STDOUT_BYTES: usize = 2 * 1024 * 1024;
const MAX_STDERR_BYTES: usize = 128 * 1024;
const RUN_TIMEOUT: Duration = Duration::from_secs(45);

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PinnedExecutable {
    pub path: PathBuf,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeConfig {
    pub ardour_lua: PinnedExecutable,
    pub ardour_version: String,
}

#[derive(Clone, Debug)]
pub struct DeepRuntime {
    config: RuntimeConfig,
    session_root: PathBuf,
}

impl DeepRuntime {
    #[cfg(unix)]
    pub fn load_production() -> Result<Option<Self>> {
        let path = Path::new(CONFIG_PATH);
        if !path.exists() {
            return Ok(None);
        }
        let metadata = fs::symlink_metadata(path)
            .map_err(|_| Error::new("Unavailable", "Could not inspect Ardour runtime config"))?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() > MAX_CONFIG_BYTES
        {
            return Err(Error::invalid(
                "Ardour runtime config is not a bounded regular file",
            ));
        }
        reject_group_or_other_writable(&metadata, "Ardour runtime config")?;
        let bytes = fs::read(path)
            .map_err(|_| Error::new("Unavailable", "Could not read Ardour runtime config"))?;
        let config: RuntimeConfig = serde_json::from_slice(&bytes)
            .map_err(|_| Error::invalid("Malformed Ardour runtime config"))?;
        Self::from_config(config, PathBuf::from(SESSION_ROOT)).map(Some)
    }

    #[cfg(not(unix))]
    pub fn load_production() -> Result<Option<Self>> {
        Ok(None)
    }

    fn from_config(config: RuntimeConfig, session_root: PathBuf) -> Result<Self> {
        validate_version(&config.ardour_version)?;
        verify_executable(&config.ardour_lua)?;
        let metadata = fs::symlink_metadata(&session_root)
            .map_err(|_| Error::new("Unavailable", "Ardour session mount is absent"))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(Error::invalid(
                "Ardour session mount must be a real directory",
            ));
        }
        Ok(Self {
            config,
            session_root,
        })
    }

    pub fn version(&self) -> &str {
        &self.config.ardour_version
    }

    pub async fn inspect(&self, state: &str) -> Result<ArdourSnapshot> {
        self.run(state, &["inspect".to_string()]).await
    }

    pub async fn mutate(&self, state: &str, mutation: &NativeMutation) -> Result<ArdourSnapshot> {
        let args = mutation.argv()?;
        self.run(state, &args).await
    }

    async fn run(&self, state: &str, operation_args: &[String]) -> Result<ArdourSnapshot> {
        validate_state(state)?;
        if operation_args.is_empty() || operation_args.len() > 8 {
            return Err(Error::invalid("Invalid Ardour adapter argument count"));
        }

        let temp = TempDir::new().map_err(|_| {
            Error::new(
                "BackendFailed",
                "Could not create Ardour adapter staging dir",
            )
        })?;
        let script_path = temp.path().join("semwright-ardour.lua");
        fs::write(&script_path, script::source())
            .map_err(|_| Error::new("BackendFailed", "Could not stage fixed Ardour Lua adapter"))?;

        let mut command = Command::new(&self.config.ardour_lua.path);
        command
            .arg(&script_path)
            .arg(&self.session_root)
            .arg(state)
            .arg(&self.config.ardour_version)
            .args(operation_args)
            .env_clear()
            .env("HOME", temp.path())
            .env("LANG", "C.UTF-8")
            .env("LC_ALL", "C.UTF-8")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        #[cfg(unix)]
        command.env("PATH", "/usr/bin:/bin");

        let mut child = command
            .spawn()
            .map_err(|_| Error::new("Unavailable", "Could not start pinned Ardour Lua runtime"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::new("BackendFailed", "Ardour stdout pipe is unavailable"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| Error::new("BackendFailed", "Ardour stderr pipe is unavailable"))?;

        let execution = async move {
            let out_task = read_bounded(stdout, MAX_STDOUT_BYTES);
            let err_task = read_bounded(stderr, MAX_STDERR_BYTES);
            let wait_task = child.wait();
            let (stdout, stderr, status) = tokio::join!(out_task, err_task, wait_task);
            let stdout = stdout?;
            let _stderr = stderr?;
            let status = status.map_err(|_| {
                Error::new("BackendFailed", "Could not wait for Ardour Lua runtime")
            })?;
            if !status.success() {
                return Err(Error::new(
                    "BackendFailed",
                    "Pinned Ardour Lua adapter exited unsuccessfully",
                ));
            }
            Ok::<Vec<u8>, Error>(stdout)
        };

        let stdout = timeout(RUN_TIMEOUT, execution)
            .await
            .map_err(|_| Error::new("Timeout", "Ardour Lua adapter exceeded runtime budget"))??;
        parse_snapshot(&stdout)
    }
}

async fn read_bounded(reader: impl tokio::io::AsyncRead + Unpin, max: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take((max + 1) as u64)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| Error::new("BackendFailed", "Could not read Ardour adapter output"))?;
    if bytes.len() > max {
        return Err(Error::limit(
            "Ardour adapter output exceeded bounded capture",
        ));
    }
    Ok(bytes)
}

fn parse_snapshot(stdout: &[u8]) -> Result<ArdourSnapshot> {
    let text = std::str::from_utf8(stdout)
        .map_err(|_| Error::new("ProtocolMismatch", "Ardour adapter output is not UTF-8"))?;
    let mut results = text
        .lines()
        .filter_map(|line| line.strip_prefix(RESULT_PREFIX));
    let encoded = results
        .next()
        .ok_or_else(|| Error::new("ProtocolMismatch", "Ardour adapter result marker is absent"))?;
    if results.next().is_some() {
        return Err(Error::new(
            "ProtocolMismatch",
            "Ardour adapter emitted multiple semantic snapshots",
        ));
    }
    if encoded.len() > MAX_STDOUT_BYTES {
        return Err(Error::limit("Ardour semantic snapshot exceeds budget"));
    }
    let snapshot: ArdourSnapshot = serde_json::from_str(encoded)
        .map_err(|_| Error::new("ProtocolMismatch", "Ardour semantic snapshot is malformed"))?;
    snapshot.validate()?;
    Ok(snapshot)
}

fn validate_state(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 255
        || matches!(value, "." | "..")
        || value.contains('/')
        || value.contains('\\')
        || value.chars().any(char::is_control)
    {
        return Err(Error::invalid("Invalid Ardour session state name"));
    }
    Ok(())
}

fn validate_version(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
        return Err(Error::invalid("Invalid Ardour runtime version"));
    }
    Ok(())
}

fn verify_executable(executable: &PinnedExecutable) -> Result<()> {
    if !executable.path.is_absolute()
        || executable.sha256.len() != 64
        || !executable
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(Error::invalid(
            "Invalid pinned Ardour executable descriptor",
        ));
    }
    let metadata = fs::symlink_metadata(&executable.path)
        .map_err(|_| Error::new("Unavailable", "Pinned Ardour executable is absent"))?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() == 0
        || metadata.len() > MAX_EXECUTABLE_BYTES
    {
        return Err(Error::invalid(
            "Pinned Ardour executable is not a bounded regular file",
        ));
    }
    reject_group_or_other_writable(&metadata, "Pinned Ardour executable")?;

    let actual = file_sha256(&executable.path)?;
    if actual != executable.sha256 {
        return Err(Error::new(
            "PermissionDenied",
            "Pinned Ardour executable digest mismatch",
        ));
    }
    Ok(())
}

fn file_sha256(path: &Path) -> Result<String> {
    let mut file = File::open(path)
        .map_err(|_| Error::new("Unavailable", "Could not open pinned Ardour executable"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| Error::new("BackendFailed", "Could not hash pinned Ardour executable"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn reject_group_or_other_writable(metadata: &fs::Metadata, label: &str) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o022 != 0 {
            return Err(Error::new(
                "PermissionDenied",
                format!("{label} must not be group/other writable"),
            ));
        }
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::native::{NativeRoute, RouteKind, SNAPSHOT_VERSION};
    use std::os::unix::fs::PermissionsExt;

    fn fixture() -> ArdourSnapshot {
        ArdourSnapshot {
            snapshot_version: SNAPSHOT_VERSION,
            ardour_version: "test".into(),
            session_name: "fixture".into(),
            sample_rate: 48_000,
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

    fn fake_runtime(temp: &Path) -> DeepRuntime {
        let snapshot = serde_json::to_string(&fixture()).unwrap();
        let executable = temp.join("fake-ardour-lua");
        let program = format!(
            "#!/bin/sh\nprintf '%s\\n' '{}{}'\n",
            RESULT_PREFIX, snapshot
        );
        fs::write(&executable, program).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let descriptor = PinnedExecutable {
            path: executable.clone(),
            sha256: file_sha256(&executable).unwrap(),
        };
        let session = temp.join("session");
        fs::create_dir(&session).unwrap();
        DeepRuntime::from_config(
            RuntimeConfig {
                ardour_lua: descriptor,
                ardour_version: "test".into(),
            },
            session,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn fixed_runtime_parses_one_bounded_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let runtime = fake_runtime(temp.path());
        assert_eq!(runtime.inspect("fixture").await.unwrap(), fixture());
    }

    #[test]
    fn traversal_and_digest_mismatch_fail_closed() {
        assert!(validate_state("../session").is_err());
        let temp = tempfile::tempdir().unwrap();
        let executable = temp.path().join("fake");
        fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let result = DeepRuntime::from_config(
            RuntimeConfig {
                ardour_lua: PinnedExecutable {
                    path: executable,
                    sha256: "0".repeat(64),
                },
                ardour_version: "test".into(),
            },
            temp.path().to_path_buf(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn duplicate_result_markers_are_protocol_errors() {
        let json = serde_json::to_string(&fixture()).unwrap();
        let body = format!("{RESULT_PREFIX}{json}\n{RESULT_PREFIX}{json}\n");
        assert!(parse_snapshot(body.as_bytes()).is_err());
    }
}
