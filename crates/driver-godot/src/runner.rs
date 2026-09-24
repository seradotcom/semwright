use crate::config::{ProjectConfig, RunnerConfig};
use semwright_driver_sdk::DriverExecutionContext;
use semwright_types::{Error, ErrorCode, JobArtifact, JobProgress, Result};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::Read,
    os::unix::process::CommandExt,
    path::{Component, Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{io::AsyncReadExt, process::Command};
use tokio_util::sync::CancellationToken;

const MAX_LOG: usize = 64 * 1024;

#[derive(Clone)]
pub struct Runner {
    config: RunnerConfig,
    projects: HashMap<String, PathBuf>,
}

impl Runner {
    pub fn new(config: RunnerConfig, projects: &[ProjectConfig]) -> Result<Self> {
        verify_file(&config.executable, &config.sha256)?;
        let projects = projects
            .iter()
            .map(|p| (p.project.clone(), p.root.clone()))
            .collect();
        Ok(Self { config, projects })
    }

    pub async fn execute(&self, command: &str, args: &Value) -> Result<Value> {
        self.execute_inner(command, args, None).await
    }

    pub async fn execute_with_context(
        &self,
        command: &str,
        args: &Value,
        context: &DriverExecutionContext,
    ) -> Result<Value> {
        context.check_cancelled()?;
        context.report_progress(
            JobProgress {
                completed: 0,
                total: Some(1),
                message: Some(format!(
                    "starting {}",
                    command.strip_prefix("driver.godot.").unwrap_or(command)
                )),
            },
            vec![],
        )?;
        let value = self
            .execute_inner(command, args, Some(context.cancellation()))
            .await?;
        let artifacts = self.artifacts_from_result(&value)?;
        context.report_progress(
            JobProgress {
                completed: 1,
                total: Some(1),
                message: Some(format!(
                    "completed {}",
                    command.strip_prefix("driver.godot.").unwrap_or(command)
                )),
            },
            artifacts,
        )?;
        Ok(value)
    }

    async fn execute_inner(
        &self,
        command: &str,
        args: &Value,
        cancellation: Option<CancellationToken>,
    ) -> Result<Value> {
        let project_id = args
            .get("project")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::invalid("Godot runner operation requires project"))?;
        let root = self
            .projects
            .get(project_id)
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "Godot project is not configured"))?;

        let (argv, artifact, timeout) = match command {
            "driver.godot.project.validate" => (
                vec![
                    "--headless".into(),
                    "--editor".into(),
                    "--path".into(),
                    root.display().to_string(),
                    "--quit-after".into(),
                    "3".into(),
                ],
                None,
                Duration::from_secs(30),
            ),
            "driver.godot.script.validate" => {
                let script = safe_res(args, "path", ".gd")?;
                (
                    vec![
                        "--headless".into(),
                        "--path".into(),
                        root.display().to_string(),
                        "--check-only".into(),
                        "--script".into(),
                        script,
                    ],
                    None,
                    Duration::from_secs(20),
                )
            }
            "driver.godot.project.run_test" => {
                let frames = args
                    .get("frames")
                    .and_then(Value::as_u64)
                    .unwrap_or(120)
                    .clamp(1, 3600);
                let mut argv = vec![
                    "--headless".into(),
                    "--path".into(),
                    root.display().to_string(),
                    "--quit-after".into(),
                    frames.to_string(),
                ];
                if let Some(scene) = args.get("scene").and_then(Value::as_str) {
                    validate_res(scene, ".tscn")?;
                    argv.push("--scene".into());
                    argv.push(scene.into());
                }
                (argv, None, Duration::from_secs(60))
            }
            "driver.godot.export.pack" => {
                let preset = bounded_arg(args, "preset", 128)?;
                let output = self.output_path(args, "output", &["pck", "zip"])?;
                (
                    vec![
                        "--headless".into(),
                        "--path".into(),
                        root.display().to_string(),
                        "--export-pack".into(),
                        preset,
                        output.display().to_string(),
                    ],
                    Some(output),
                    Duration::from_secs(120),
                )
            }
            "driver.godot.export.build" => {
                let preset = bounded_arg(args, "preset", 128)?;
                let output = self.output_path(args, "output", &[])?;
                let mode = match args.get("debug").and_then(Value::as_bool) {
                    Some(true) => "--export-debug",
                    _ => "--export-release",
                };
                (
                    vec![
                        "--headless".into(),
                        "--path".into(),
                        root.display().to_string(),
                        mode.into(),
                        preset,
                        output.display().to_string(),
                    ],
                    Some(output),
                    Duration::from_secs(180),
                )
            }
            "driver.godot.movie.capture" => {
                if self.config.display.is_none() {
                    return Err(Error::new(
                        ErrorCode::Unavailable,
                        "Godot movie capture requires an owner-configured X11 display",
                    ));
                }
                let output = self.output_path(args, "output", &["avi"])?;
                let frames = args
                    .get("frames")
                    .and_then(Value::as_u64)
                    .unwrap_or(120)
                    .clamp(1, 18_000);
                let fps = args
                    .get("fps")
                    .and_then(Value::as_u64)
                    .unwrap_or(30)
                    .clamp(1, 240);
                let mut argv = vec![
                    "--display-driver".into(),
                    "x11".into(),
                    "--audio-driver".into(),
                    "Dummy".into(),
                    "--path".into(),
                    root.display().to_string(),
                    "--write-movie".into(),
                    output.display().to_string(),
                    "--fixed-fps".into(),
                    fps.to_string(),
                    "--quit-after".into(),
                    frames.to_string(),
                ];
                if let Some(scene) = args.get("scene").and_then(Value::as_str) {
                    validate_res(scene, ".tscn")?;
                    argv.push("--scene".into());
                    argv.push(scene.into());
                }
                (argv, Some(output), Duration::from_secs(180))
            }
            _ => {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "Unsupported Godot runner operation",
                ));
            }
        };

        let process = self.run(root, &argv, timeout, cancellation).await?;
        let artifact = artifact
            .filter(|path| path.is_file())
            .map(|path| {
                path.strip_prefix(&self.config.output_root)
                    .unwrap_or(&path)
                    .display()
                    .to_string()
            })
            .unwrap_or_default();
        Ok(json!({
            "success": true,
            "exit_code": process.exit_code,
            "stdout": process.stdout,
            "stderr": process.stderr,
            "artifact": artifact,
        }))
    }

    fn artifacts_from_result(&self, value: &Value) -> Result<Vec<JobArtifact>> {
        let Some(relative) = value.get("artifact").and_then(Value::as_str) else {
            return Ok(vec![]);
        };
        if relative.is_empty() {
            return Ok(vec![]);
        }
        let path = self.config.output_root.join(relative);
        let metadata = std::fs::metadata(&path)?;
        if !metadata.is_file() {
            return Ok(vec![]);
        }
        let digest = file_digest(&path)?;
        let media_type = match path.extension().and_then(|value| value.to_str()) {
            Some("zip") => Some("application/zip".to_owned()),
            Some("avi") => Some("video/x-msvideo".to_owned()),
            Some("pck") => Some("application/octet-stream".to_owned()),
            _ => Some("application/octet-stream".to_owned()),
        };
        Ok(vec![JobArtifact {
            name: path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("godot-artifact")
                .to_owned(),
            reference: format!("artifact:godot:{relative}"),
            media_type,
            sha256: Some(digest),
            bytes: Some(metadata.len()),
        }])
    }

    async fn run(
        &self,
        root: &Path,
        argv: &[String],
        timeout: Duration,
        cancellation: Option<CancellationToken>,
    ) -> Result<ProcessOutput> {
        verify_file(&self.config.executable, &self.config.sha256)?;
        let home = self.config.output_root.join(".semwright-home");
        std::fs::create_dir_all(&home)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o700))?;
        }

        let mut command = Command::new(&self.config.executable);
        command
            .args(argv)
            .current_dir(root)
            .env_clear()
            .env("HOME", &home)
            .env("XDG_DATA_HOME", home.join("data"))
            .env("XDG_CONFIG_HOME", home.join("config"))
            .env("XDG_CACHE_HOME", home.join("cache"))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(display) = &self.config.display {
            command.env("DISPLAY", display);
        }
        command.as_std_mut().process_group(0);

        let mut child = command.spawn()?;
        let pid = child
            .id()
            .ok_or_else(|| Error::new(ErrorCode::Internal, "Godot child has no PID"))?
            as i32;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::new(ErrorCode::Internal, "Godot stdout pipe missing"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| Error::new(ErrorCode::Internal, "Godot stderr pipe missing"))?;
        let out_task = tokio::spawn(capture(stdout));
        let err_task = tokio::spawn(capture(stderr));

        enum WaitOutcome {
            Exited(std::io::Result<std::process::ExitStatus>),
            Cancelled,
            TimedOut,
        }
        let outcome = if let Some(cancellation) = cancellation {
            tokio::select! {
                result = child.wait() => WaitOutcome::Exited(result),
                _ = cancellation.cancelled() => WaitOutcome::Cancelled,
                _ = tokio::time::sleep(timeout) => WaitOutcome::TimedOut,
            }
        } else {
            match tokio::time::timeout(timeout, child.wait()).await {
                Ok(result) => WaitOutcome::Exited(result),
                Err(_) => WaitOutcome::TimedOut,
            }
        };
        let status = match outcome {
            WaitOutcome::Exited(result) => result?,
            WaitOutcome::Cancelled => {
                // SAFETY: PID belongs to the process group created for this child above.
                unsafe { libc::kill(-pid, libc::SIGKILL) };
                let _ = child.wait().await;
                return Err(Error::new(
                    ErrorCode::Cancelled,
                    "Godot runner observed cooperative cancellation",
                ));
            }
            WaitOutcome::TimedOut => {
                // SAFETY: PID belongs to the process group created for this child above.
                unsafe { libc::kill(-pid, libc::SIGKILL) };
                let _ = child.wait().await;
                return Err(Error::new(ErrorCode::Timeout, "Godot runner timed out"));
            }
        };
        let (stdout, out_overflow) = out_task
            .await
            .map_err(|_| Error::new(ErrorCode::Internal, "Godot stdout task failed"))??;
        let (stderr, err_overflow) = err_task
            .await
            .map_err(|_| Error::new(ErrorCode::Internal, "Godot stderr task failed"))??;
        if out_overflow || err_overflow {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Godot runner output exceeded limit",
            ));
        }
        if !status.success() {
            return Err(Error::new(
                ErrorCode::BackendFailed,
                "Godot runner exited with non-zero status",
            ));
        }
        Ok(ProcessOutput {
            exit_code: status.code().unwrap_or(0),
            stdout,
            stderr,
        })
    }

    fn output_path(&self, args: &Value, key: &str, extensions: &[&str]) -> Result<PathBuf> {
        let name = bounded_arg(args, key, 200)?;
        let rel = Path::new(&name);
        if rel.is_absolute()
            || rel.components().any(|c| !matches!(c, Component::Normal(_)))
            || name.contains('\\')
        {
            return Err(Error::invalid(
                "Godot output path must be a relative normal path",
            ));
        }
        if !extensions.is_empty()
            && !rel
                .extension()
                .and_then(|x| x.to_str())
                .is_some_and(|ext| extensions.contains(&ext))
        {
            return Err(Error::invalid("Godot output extension is not allowed"));
        }
        let path = self.config.output_root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
            if !parent.canonicalize()?.starts_with(&self.config.output_root) {
                return Err(Error::new(
                    ErrorCode::PermissionDenied,
                    "Godot output escaped its grant",
                ));
            }
        }
        Ok(path)
    }
}

struct ProcessOutput {
    exit_code: i32,
    stdout: String,
    stderr: String,
}

async fn capture<R: tokio::io::AsyncRead + Unpin>(mut reader: R) -> Result<(String, bool)> {
    let mut kept = Vec::new();
    let mut overflow = false;
    let mut buf = [0u8; 8192];
    loop {
        let read = reader.read(&mut buf).await?;
        if read == 0 {
            break;
        }
        let remaining = MAX_LOG.saturating_sub(kept.len());
        if read > remaining {
            kept.extend_from_slice(&buf[..remaining]);
            overflow = true;
        } else {
            kept.extend_from_slice(&buf[..read]);
        }
    }
    Ok((String::from_utf8_lossy(&kept).into_owned(), overflow))
}

fn file_digest(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        digest.update(&buf[..n]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn verify_file(path: &Path, expected: &str) -> Result<()> {
    if file_digest(path)? != expected {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Godot executable digest mismatch",
        ));
    }
    Ok(())
}

fn bounded_arg(args: &Value, key: &str, max: usize) -> Result<String> {
    let value = args
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::invalid("Godot runner argument is missing"))?;
    if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(Error::invalid("Godot runner argument exceeds bounds"));
    }
    Ok(value.to_owned())
}

fn safe_res(args: &Value, key: &str, suffix: &str) -> Result<String> {
    let value = bounded_arg(args, key, 240)?;
    validate_res(&value, suffix)?;
    Ok(value)
}

fn validate_res(value: &str, suffix: &str) -> Result<()> {
    if !value.starts_with("res://")
        || value.contains("..")
        || value.contains('\\')
        || !value.ends_with(suffix)
    {
        return Err(Error::invalid("Godot resource path is not allowed"));
    }
    Ok(())
}
