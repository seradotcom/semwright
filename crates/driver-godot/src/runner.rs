use crate::{
    authoring::{
        native_observation::{
            MAX_OBSERVATION_BYTES, NATIVE_VERSION, NativeEvidenceBinding, NativeObservation,
            NativeRequest, NativeVerification, NativeVerifyRequest, NativeVerifyResult,
            PROBE_SOURCE, ProbeMode, decode_observation, persistence_value,
        },
        runtime::NativePlanContext,
        store::{Snapshot, Store},
        validate,
    },
    config::{AuthoringConfig, ProjectConfig, RunnerConfig},
};
use semwright_driver_sdk::DriverExecutionContext;
use semwright_types::{Error, ErrorCode, JobArtifact, JobProgress, Result, unique_id};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    io::{Read, Write},
    os::unix::process::CommandExt,
    path::{Component, Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{io::AsyncReadExt, process::Command};
use tokio_util::sync::CancellationToken;

const MAX_LOG: usize = 64 * 1024;

struct StagedManagedProject {
    path: PathBuf,
    snapshot: Snapshot,
    expected: BTreeMap<String, semwright_semantic_composition::Digest>,
}
impl StagedManagedProject {
    fn verify_sources(&self) -> Result<()> {
        for (relative, expected) in &self.expected {
            let path = self.path.join(relative);
            let metadata = std::fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(Error::new(
                    ErrorCode::Conflict,
                    "Native execution changed a managed source type",
                ));
            }
            if file_digest(&path)? != expected.as_str() {
                return Err(Error::new(
                    ErrorCode::Conflict,
                    "Native execution changed managed source bytes",
                ));
            }
        }
        Ok(())
    }
}
impl Drop for StagedManagedProject {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[derive(Clone)]
pub struct Runner {
    config: RunnerConfig,
    projects: HashMap<String, PathBuf>,
    authoring: Option<AuthoringConfig>,
}

impl Runner {
    pub fn new(
        config: RunnerConfig,
        projects: &[ProjectConfig],
        authoring: Option<&AuthoringConfig>,
    ) -> Result<Self> {
        verify_file(&config.executable, &config.sha256)?;
        let projects = projects
            .iter()
            .map(|p| (p.project.clone(), p.root.clone()))
            .collect();
        Ok(Self {
            config,
            projects,
            authoring: authoring.cloned(),
        })
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

    pub(crate) async fn execute_native_verification(
        &self,
        request: NativeVerifyRequest,
        binding: NativePlanContext,
        context: &DriverExecutionContext,
    ) -> Result<NativeVerifyResult> {
        context.check_cancelled()?;
        let result = self
            .native_verify(
                request,
                binding,
                context.request_id().to_owned(),
                Some(context.cancellation()),
            )
            .await;
        if let Err(error) = &result {
            self.write_private_native_diagnostic(error);
        }
        result
    }

    fn write_private_native_diagnostic(&self, error: &Error) {
        let Some(authoring) = self.authoring.as_ref() else {
            return;
        };
        let gate = authoring.state_root.join(".enable-native-diagnostics");
        let Ok(gate_meta) = std::fs::symlink_metadata(&gate) else {
            return;
        };
        if !gate_meta.file_type().is_file() || gate_meta.len() > 32 {
            return;
        }
        let path = authoring.state_root.join(".native-verify-error");
        let mut message = error.message.replace(['\r', '\n'], " ");
        message.truncate(2048);
        let Ok(mut file) = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
        else {
            return;
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
        }
        let _ = writeln!(file, "code={:?}", error.code);
        let _ = writeln!(file, "message={message}");
        let _ = file.sync_all();
    }

    async fn execute_inner(
        &self,
        command: &str,
        args: &Value,
        cancellation: Option<CancellationToken>,
    ) -> Result<Value> {
        let source_root = self.project_root(args)?;
        let staged = args
            .get("managed_project")
            .and_then(Value::as_str)
            .map(|project| self.stage_managed_project(project))
            .transpose()?;
        let root = staged
            .as_ref()
            .map(|project| project.path.as_path())
            .unwrap_or(source_root.as_path());

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

    fn project_root(&self, args: &Value) -> Result<PathBuf> {
        let paired = args.get("project").and_then(Value::as_str);
        let managed = args.get("managed_project").and_then(Value::as_str);
        match (paired, managed) {
            (Some(project), None) => {
                self.projects.get(project).cloned().ok_or_else(|| {
                    Error::new(ErrorCode::NotFound, "Godot project is not configured")
                })
            }
            (None, Some(project)) => {
                validate::id(project).map_err(|error| Error::invalid(error.to_string()))?;
                let authoring = self.authoring.as_ref().ok_or_else(|| {
                    Error::new(
                        ErrorCode::PermissionDenied,
                        "Managed Godot project execution requires an authoring output grant",
                    )
                })?;
                let snapshot = Store::new(authoring.clone())?.snapshot(project)?;
                if snapshot.status != "IN_SYNC" {
                    return Err(Error::new(
                        ErrorCode::Conflict,
                        format!(
                            "Managed Godot project must be IN_SYNC before native execution ({})",
                            snapshot.status
                        ),
                    ));
                }
                let root = authoring.output_root.join(project);
                let metadata = std::fs::symlink_metadata(&root).map_err(|error| {
                    if error.kind() == std::io::ErrorKind::NotFound {
                        Error::new(ErrorCode::NotFound, "Managed Godot project does not exist")
                    } else {
                        error.into()
                    }
                })?;
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(Error::new(
                        ErrorCode::PermissionDenied,
                        "Managed Godot project root must be an owned directory",
                    ));
                }
                let canonical = root.canonicalize()?;
                if canonical != root || !canonical.starts_with(&authoring.output_root) {
                    return Err(Error::new(
                        ErrorCode::PermissionDenied,
                        "Managed Godot project escaped its authoring output grant",
                    ));
                }
                Ok(root)
            }
            _ => Err(Error::invalid(
                "Godot runner requires exactly one of project or managed_project",
            )),
        }
    }

    fn stage_managed_project(&self, project: &str) -> Result<StagedManagedProject> {
        let authoring = self.authoring.as_ref().ok_or_else(|| {
            Error::new(
                ErrorCode::PermissionDenied,
                "Managed Godot project execution requires an authoring output grant",
            )
        })?;
        let store = Store::new(authoring.clone())?;
        let (snapshot, files) = store.execution_files(project)?;
        let path = self
            .config
            .output_root
            .join(format!(".sw-project-{}", unique_id()));
        std::fs::create_dir(&path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))?;
        }
        let mut expected = BTreeMap::new();
        let result = (|| -> Result<()> {
            for (relative, bytes) in files {
                let rel = Path::new(&relative);
                if rel.is_absolute()
                    || relative.contains('\\')
                    || rel
                        .components()
                        .any(|component| !matches!(component, Component::Normal(_)))
                {
                    return Err(Error::new(
                        ErrorCode::Internal,
                        "Provider-owned managed path is not portable",
                    ));
                }
                let target = path.join(rel);
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&target, &bytes)?;
                expected.insert(
                    relative,
                    semwright_semantic_composition::Digest::of_bytes(&bytes),
                );
            }
            Ok(())
        })();
        if let Err(error) = result {
            let _ = std::fs::remove_dir_all(&path);
            return Err(error);
        }
        let staged = StagedManagedProject {
            path,
            snapshot,
            expected,
        };
        staged.verify_sources()?;
        Ok(staged)
    }

    async fn native_verify(
        &self,
        request: NativeVerifyRequest,
        binding: NativePlanContext,
        request_id: String,
        cancellation: Option<CancellationToken>,
    ) -> Result<NativeVerifyResult> {
        validate::id(&request.scene).map_err(|error| Error::invalid(error.to_string()))?;
        let staged = self.stage_managed_project(&binding.project)?;
        let record = staged.snapshot.record().ok_or_else(|| {
            Error::new(ErrorCode::Conflict, "Managed derivation record is missing")
        })?;
        if record.project != binding.project_id
            || record.slug != binding.project
            || record.intent_digest != binding.intent_digest
        {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Native verification project binding changed after plan resolution",
            ));
        }
        let actions: BTreeSet<String> = record
            .intent
            .inputs
            .iter()
            .map(|input| input.id.clone())
            .collect();
        let scene_id = request.scene.clone();
        let scene = format!("res://scenes/{scene_id}.tscn");
        let source = staged.snapshot.fingerprint.clone();
        let evidence_binding = NativeEvidenceBinding {
            owner: binding.owner,
            request_id,
            project: binding.project_id,
            slug: binding.project,
            scene: scene_id,
            plan_digest: binding.plan_digest,
            intent_digest: binding.intent_digest,
            source_fingerprint: source.clone(),
        };
        let private = self
            .config
            .output_root
            .join(format!(".sw-native-{}", unique_id()));
        std::fs::create_dir(&private)?;
        let helper = private.join("native_observer.gd");
        std::fs::write(&helper, PROBE_SOURCE.as_bytes())?;
        let import_argv = vec![
            "--headless".into(),
            "--path".into(),
            staged.path.display().to_string(),
            "--import".into(),
        ];
        let import_result = self
            .run(
                &staged.path,
                &import_argv,
                Duration::from_secs(90),
                cancellation.clone(),
            )
            .await;
        if let Err(error) = import_result {
            let _ = std::fs::remove_dir_all(&private);
            return Err(error);
        }
        staged.verify_sources()?;

        let result = async {
            match request.verification {
                NativeVerification::Inspect => {
                    let native = NativeRequest {
                        version: NATIVE_VERSION,
                        nonce: format!("native_{}", unique_id()),
                        source_fingerprint: source,
                        mode: ProbeMode::Inspect,
                        scene,
                        ticks: 0,
                        inputs: vec![],
                        checkpoints: vec![],
                        variables: vec![],
                        capture: false,
                    };
                    native.validate(&actions)?;
                    let observation = self
                        .run_native_probe(&staged, &private, &helper, &native, cancellation)
                        .await?;
                    Ok(NativeVerifyResult::Inspect {
                        binding: evidence_binding,
                        observation,
                    })
                }
                NativeVerification::Persistence => {
                    let writer_request = NativeRequest {
                        version: NATIVE_VERSION,
                        nonce: format!("native_save_{}", unique_id()),
                        source_fingerprint: source.clone(),
                        mode: ProbeMode::SaveCandidate,
                        scene: scene.clone(),
                        ticks: 0,
                        inputs: vec![],
                        checkpoints: vec![],
                        variables: vec![],
                        capture: false,
                    };
                    writer_request.validate(&actions)?;
                    let writer = self
                        .run_native_probe(
                            &staged,
                            &private,
                            &helper,
                            &writer_request,
                            cancellation.clone(),
                        )
                        .await?;
                    let reader_request = NativeRequest {
                        version: NATIVE_VERSION,
                        nonce: format!("native_reopen_{}", unique_id()),
                        source_fingerprint: source,
                        mode: ProbeMode::ReopenCandidate,
                        scene,
                        ticks: 0,
                        inputs: vec![],
                        checkpoints: vec![],
                        variables: vec![],
                        capture: false,
                    };
                    reader_request.validate(&actions)?;
                    let reader = self
                        .run_native_probe(&staged, &private, &helper, &reader_request, cancellation)
                        .await?;
                    let evidence = persistence_value(&writer, &reader)?;
                    Ok(NativeVerifyResult::Persistence {
                        binding: evidence_binding,
                        writer,
                        reader,
                        evidence,
                    })
                }
                NativeVerification::Play {
                    ticks,
                    inputs,
                    checkpoints,
                    variables,
                    capture,
                } => {
                    let native = NativeRequest {
                        version: NATIVE_VERSION,
                        nonce: format!("native_play_{}", unique_id()),
                        source_fingerprint: source,
                        mode: ProbeMode::Play,
                        scene,
                        ticks,
                        inputs,
                        checkpoints,
                        variables,
                        capture,
                    };
                    native.validate(&actions)?;
                    let observation = self
                        .run_native_probe(&staged, &private, &helper, &native, cancellation)
                        .await?;
                    Ok(NativeVerifyResult::Play {
                        binding: evidence_binding,
                        observation,
                    })
                }
            }
        }
        .await;
        let _ = std::fs::remove_dir_all(&private);
        result
    }

    async fn run_native_probe(
        &self,
        staged: &StagedManagedProject,
        private: &Path,
        helper: &Path,
        request: &NativeRequest,
        cancellation: Option<CancellationToken>,
    ) -> Result<NativeObservation> {
        let token = unique_id();
        let request_path = private.join(format!("request-{token}.json"));
        let output_path = private.join(format!("observation-{token}.json"));
        let request_bytes = semwright_semantic_composition::canonical_bytes(request)
            .map_err(|error| Error::invalid(error.to_string()))?;
        std::fs::write(&request_path, request_bytes)?;
        let argv = vec![
            "--headless".into(),
            "--path".into(),
            staged.path.display().to_string(),
            "--script".into(),
            helper.display().to_string(),
            "--".into(),
            "--request".into(),
            request_path.display().to_string(),
            "--output".into(),
            output_path.display().to_string(),
        ];
        self.run(&staged.path, &argv, Duration::from_secs(90), cancellation)
            .await?;
        staged.verify_sources()?;
        let metadata = std::fs::metadata(&output_path)?;
        if !metadata.is_file() || metadata.len() as usize > MAX_OBSERVATION_BYTES {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Native Godot observation exceeded its output budget",
            ));
        }
        let bytes = std::fs::read(&output_path)?;
        decode_observation(&bytes, request)
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn test_runner() -> (tempfile::TempDir, Runner) {
        let root = tempfile::tempdir().unwrap();
        for name in ["managed", "state", "artifacts", "input"] {
            std::fs::create_dir(root.path().join(name)).unwrap();
        }
        std::fs::write(
            root.path().join("input/start_cue.wav"),
            include_bytes!("../tests/fixtures/authoring/start_cue.wav"),
        )
        .unwrap();
        std::fs::set_permissions(
            root.path().join("state"),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        let executable = PathBuf::from("/usr/bin/true").canonicalize().unwrap();
        let config = RunnerConfig {
            sha256: file_digest(&executable).unwrap(),
            executable,
            output_root: root.path().join("artifacts").canonicalize().unwrap(),
            display: None,
        };
        let authoring = AuthoringConfig {
            output_root: root.path().join("managed").canonicalize().unwrap(),
            state_root: root.path().join("state").canonicalize().unwrap(),
            input_root: Some(root.path().join("input").canonicalize().unwrap()),
        };
        let spec =
            validate::decode(include_bytes!("../tests/fixtures/authoring/two_d.json")).unwrap();
        let store = Store::new(authoring.clone()).unwrap();
        let prepared = store.prepare(&spec, false, false).unwrap();
        store.apply(&prepared, || Ok(())).unwrap();
        let runner = Runner::new(config, &[], Some(&authoring)).unwrap();
        (root, runner)
    }

    #[test]
    fn managed_project_selector_is_owner_rooted_and_exclusive() {
        let (root, runner) = test_runner();
        let project = runner
            .project_root(&json!({"managed_project":"technical_two"}))
            .unwrap();
        assert_eq!(
            project,
            root.path()
                .join("managed/technical_two")
                .canonicalize()
                .unwrap()
        );
        assert!(
            runner
                .project_root(&json!({
                    "project":"a".repeat(64),
                    "managed_project":"technical_two"
                }))
                .is_err()
        );
        assert!(
            runner
                .project_root(&json!({"managed_project":"../escape"}))
                .is_err()
        );
    }

    #[test]
    fn managed_project_rejects_source_drift_before_native_execution() {
        let (root, runner) = test_runner();
        std::fs::write(
            root.path().join("managed/technical_two/project.godot"),
            "config_version=5\n# external edit\n",
        )
        .unwrap();
        let error = runner
            .project_root(&json!({"managed_project":"technical_two"}))
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Conflict);
    }
}
