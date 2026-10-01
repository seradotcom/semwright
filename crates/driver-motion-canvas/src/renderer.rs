//! Motion Canvas render jobs shared by legacy async capabilities and Protocol v3 render.execute.
use crate::{
    Error, ErrorCode, Result,
    model::{ColorSpace, Project, RenderProfile},
    refs::{Kind, Reference},
    security,
    store::Snapshot,
    validate::RenderPlan,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{io::AsyncReadExt, process::Command, sync::Mutex};
use tokio_util::sync::CancellationToken;

const MAX_PROCESS_OUTPUT: u64 = 262_144;
const MAX_JOBS: usize = 64;
const NODE_RENDER_FLAGS: [&str; 2] = ["--disable-wasm-trap-handler", "--max-old-space-size=256"];

fn runtime_relative_path(path: &str) -> Result<()> {
    if path.is_empty()
        || path.len() > 512
        || path.split('/').count() > 12
        || path.contains(['\\', ':', '%', '\0'])
        || !path
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"/_-.@".contains(&byte))
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == ".." || part.len() > 128)
    {
        return Err(Error::invalid(
            "Runtime path must be bounded, relative and free of traversal or URLs",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct RendererRuntime {
    pub node: PathBuf,
    pub helper: PathBuf,
    pub browser: PathBuf,
    pub dependency_lock_sha256: String,
    pub font_resources_sha256: String,
}

impl RendererRuntime {
    pub fn from_root(root: &Path) -> Result<Self> {
        let bytes = fs::read(root.join("runtime.json")).map_err(|_| {
            Error::new(
                ErrorCode::Unavailable,
                "Pinned Motion Canvas runtime.json is unavailable",
            )
        })?;
        if bytes.len() > 16_384 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Runtime config exceeds byte budget",
            ));
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Tool {
            path: String,
            sha256: String,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Config {
            node: Tool,
            helper: Tool,
            browser: Tool,
            dependency_lock: Tool,
            font_resources: Vec<Tool>,
        }
        let config: Config = serde_json::from_slice(&bytes)?;
        let canonical_root = fs::canonicalize(root)?;
        if canonical_root != root {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "Runtime root must be canonical",
            ));
        }
        let resolve = |tool: &Tool| -> Result<PathBuf> {
            runtime_relative_path(&tool.path)?;
            if !security::digest(&tool.sha256) {
                return Err(Error::invalid("Runtime tool digest is malformed"));
            }
            let path = root.join(&tool.path);
            let canonical = fs::canonicalize(&path)?;
            if !canonical.starts_with(root) {
                return Err(Error::new(
                    ErrorCode::PermissionDenied,
                    "Runtime tool escapes owner-approved runtime root",
                ));
            }
            let meta = fs::symlink_metadata(&path)?;
            if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 536_870_912 {
                return Err(Error::new(
                    ErrorCode::PermissionDenied,
                    "Runtime tool must be a bounded regular non-symlink file",
                ));
            }
            let mut file = fs::File::open(&canonical)?;
            let mut hasher = Sha256::new();
            let mut buffer = [0u8; 65_536];
            loop {
                let read = file.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
            }
            let actual = format!("{:x}", hasher.finalize());
            if actual != tool.sha256 {
                return Err(Error::new(
                    ErrorCode::PermissionDenied,
                    "Pinned runtime tool digest mismatch",
                ));
            }
            Ok(canonical)
        };
        if config.dependency_lock.path != "package-lock.json" {
            return Err(Error::invalid(
                "Motion Canvas runtime dependency lock must be package-lock.json",
            ));
        }
        let dependency_lock = resolve(&config.dependency_lock)?;
        if fs::metadata(&dependency_lock)?.len() > 4 * 1024 * 1024 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Motion Canvas dependency lock exceeds 4 MiB",
            ));
        }
        if config.font_resources.is_empty() || config.font_resources.len() > 128 {
            return Err(Error::invalid(
                "Motion Canvas runtime must pin a bounded font resource set",
            ));
        }
        let mut font_resources = BTreeMap::new();
        for resource in &config.font_resources {
            let allowed = resource.path
                == "node_modules/@fontsource-variable/instrument-sans/index.css"
                || resource.path == "node_modules/@fontsource/ibm-plex-mono/400.css"
                || resource
                    .path
                    .starts_with("node_modules/@fontsource-variable/instrument-sans/files/")
                || resource
                    .path
                    .starts_with("node_modules/@fontsource/ibm-plex-mono/files/");
            if !allowed
                || !(resource.path.ends_with(".css") || resource.path.ends_with(".woff2"))
                || font_resources
                    .insert(resource.path.clone(), resource.sha256.clone())
                    .is_some()
            {
                return Err(Error::invalid(
                    "Unexpected or duplicate pinned font resource",
                ));
            }
            let path = resolve(resource)?;
            if fs::metadata(path)?.len() > 16 * 1024 * 1024 {
                return Err(Error::new(
                    ErrorCode::ResourceExhausted,
                    "Pinned font resource exceeds 16 MiB",
                ));
            }
        }
        let mut font_hasher = Sha256::new();
        for (path, digest) in &font_resources {
            font_hasher.update(path.as_bytes());
            font_hasher.update([0]);
            font_hasher.update(digest.as_bytes());
            font_hasher.update([0]);
        }
        Ok(Self {
            node: resolve(&config.node)?,
            helper: resolve(&config.helper)?,
            browser: resolve(&config.browser)?,
            dependency_lock_sha256: config.dependency_lock.sha256,
            font_resources_sha256: format!("{:x}", font_hasher.finalize()),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RenderState {
    Queued,
    Starting,
    Rendering,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArtifactSummary {
    pub directory: String,
    pub manifest: String,
    pub frame_count: u64,
    pub first_png: String,
    pub last_png: String,
    pub manifest_sha256: String,
    pub manifest_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JobView {
    pub job_ref: String,
    pub state: RenderState,
    pub error: Option<String>,
    pub artifact: Option<ArtifactSummary>,
}

struct Job {
    source_sha256: String,
    view: JobView,
    failure_code: Option<ErrorCode>,
    cancel: CancellationToken,
}

#[derive(Clone)]
pub struct RenderManager {
    jobs: Arc<Mutex<BTreeMap<String, Job>>>,
    runtime: Option<RendererRuntime>,
    output_root: PathBuf,
}

impl RenderManager {
    pub fn new(runtime: Option<RendererRuntime>, output_root: PathBuf) -> Self {
        Self {
            jobs: Arc::new(Mutex::new(BTreeMap::new())),
            runtime,
            output_root,
        }
    }

    pub fn available(&self) -> bool {
        self.runtime.is_some()
    }

    pub async fn active_count(&self) -> usize {
        self.jobs
            .lock()
            .await
            .values()
            .filter(|job| {
                matches!(
                    job.view.state,
                    RenderState::Queued | RenderState::Starting | RenderState::Rendering
                )
            })
            .count()
    }

    pub async fn start(&self, snapshot: &Snapshot, profile: RenderProfile) -> Result<JobView> {
        let runtime = self.runtime.clone().ok_or_else(|| {
            Error::new(
                ErrorCode::Unavailable,
                "Pinned Motion Canvas runtime is unavailable",
            )
        })?;
        let mut guard = self.jobs.lock().await;
        if guard.len() >= MAX_JOBS {
            guard.retain(|_, job| {
                matches!(
                    job.view.state,
                    RenderState::Queued | RenderState::Starting | RenderState::Rendering
                )
            });
        }
        let active = guard
            .values()
            .filter(|job| {
                matches!(
                    job.view.state,
                    RenderState::Queued | RenderState::Starting | RenderState::Rendering
                )
            })
            .count();
        if active >= 2 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "At most two Motion Canvas render jobs may run concurrently",
            ));
        }
        let plan = crate::validate::render_plan(&snapshot.project, &profile)?;
        let id = format!("render-{}", uuid::Uuid::new_v4().simple());
        let job_ref = Reference::new(
            &snapshot.project,
            &snapshot.source_sha256,
            Kind::RenderJob,
            &id,
        )
        .encode();
        let view = JobView {
            job_ref: job_ref.clone(),
            state: RenderState::Queued,
            error: None,
            artifact: None,
        };
        let cancel = CancellationToken::new();
        guard.insert(
            job_ref.clone(),
            Job {
                source_sha256: snapshot.source_sha256.clone(),
                view: view.clone(),
                failure_code: None,
                cancel: cancel.clone(),
            },
        );
        drop(guard);

        let jobs = self.jobs.clone();
        let output_root = self.output_root.clone();
        let generated = snapshot.generated_dir.clone().ok_or_else(|| {
            Error::new(
                ErrorCode::Unavailable,
                "Generated project has not been materialized for rendering",
            )
        })?;
        let project = snapshot.project.clone();
        let key = job_ref.clone();
        tokio::spawn(async move {
            let result = run_render(
                &runtime,
                &output_root,
                &generated,
                &project,
                &plan,
                &id,
                &cancel,
                &jobs,
                &key,
            )
            .await;
            let mut jobs = jobs.lock().await;
            if let Some(job) = jobs.get_mut(&key) {
                match result {
                    Ok(artifact) => {
                        job.view.state = RenderState::Succeeded;
                        job.view.artifact = Some(artifact);
                    }
                    Err(error) if error.code == ErrorCode::Cancelled => {
                        job.view.state = RenderState::Cancelled;
                        job.failure_code = Some(error.code);
                        job.view.error = Some(error.message);
                    }
                    Err(error) => {
                        job.view.state = RenderState::Failed;
                        job.failure_code = Some(error.code);
                        job.view.error = Some(error.message);
                    }
                }
            }
        });
        Ok(view)
    }

    pub async fn status(&self, job_ref: &str) -> Result<JobView> {
        self.jobs
            .lock()
            .await
            .get(job_ref)
            .map(|job| job.view.clone())
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "Unknown render job"))
    }

    pub(crate) async fn failure_code(&self, job_ref: &str) -> Option<ErrorCode> {
        self.jobs
            .lock()
            .await
            .get(job_ref)
            .and_then(|job| job.failure_code)
    }

    pub async fn cancel(&self, job_ref: &str) -> Result<JobView> {
        let token = self
            .jobs
            .lock()
            .await
            .get(job_ref)
            .map(|job| job.cancel.clone())
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "Unknown render job"))?;
        token.cancel();
        self.status(job_ref).await
    }

    pub async fn result(&self, job_ref: &str) -> Result<JobView> {
        let view = self.status(job_ref).await?;
        if matches!(
            view.state,
            RenderState::Queued | RenderState::Starting | RenderState::Rendering
        ) {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Render job is not terminal",
            ));
        }
        Ok(view)
    }
}

async fn set_state(jobs: &Arc<Mutex<BTreeMap<String, Job>>>, key: &str, state: RenderState) {
    if let Some(job) = jobs.lock().await.get_mut(key) {
        job.view.state = state;
    }
}

fn renderer_failure_code(stdout: &[u8]) -> ErrorCode {
    let Ok(text) = std::str::from_utf8(stdout) else {
        return ErrorCode::BackendFailed;
    };
    let Some(line) = text.lines().rev().find(|line| line.starts_with('{')) else {
        return ErrorCode::BackendFailed;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
        return ErrorCode::BackendFailed;
    };
    if value.get("ok") != Some(&serde_json::Value::Bool(false)) {
        return ErrorCode::BackendFailed;
    }
    match value.get("errorClass").and_then(serde_json::Value::as_str) {
        Some("vite_build") => ErrorCode::PluginProtocolError,
        Some("font_evidence" | "frame_export" | "observation") => ErrorCode::ProtocolMismatch,
        Some("browser_launch" | "page_load") => ErrorCode::Unavailable,
        Some("arguments") => ErrorCode::InvalidArgument,
        Some("project_stage") => ErrorCode::Internal,
        Some("finalize") => ErrorCode::ProtocolMismatch,
        Some("render_wait_timeout") => ErrorCode::Timeout,
        Some(
            "renderer_state_frame_clock"
            | "renderer_state_authoring_protocol"
            | "renderer_log_authoring_protocol"
            | "renderer_log_exporter_missing"
            | "renderer_log_async_property",
        ) => ErrorCode::ProtocolMismatch,
        Some(
            "renderer_state_model_invariant"
            | "renderer_log_model_invariant"
            | "renderer_state_authoring_model"
            | "renderer_log_authoring_model"
            | "renderer_state_range_error"
            | "renderer_log_range_error",
        ) => ErrorCode::InvalidArgument,
        Some("renderer_state_type_error" | "renderer_log_type_error") => {
            ErrorCode::PluginProtocolError
        }
        Some("renderer_state_error" | "renderer_log_error") => ErrorCode::Internal,
        Some("render_result_aborted") => ErrorCode::Cancelled,
        Some("render_result_error" | "render_wait") => ErrorCode::Internal,
        Some("render_result_unknown" | "render_nonzero") => ErrorCode::ProtocolMismatch,
        _ => ErrorCode::BackendFailed,
    }
}

fn renderer_process_failure_code(status: &std::process::ExitStatus, stdout: &[u8]) -> ErrorCode {
    let receipt = renderer_failure_code(stdout);
    if receipt != ErrorCode::BackendFailed {
        return receipt;
    }
    // A valid structured receipt may intentionally classify a controlled
    // renderer failure as BackendFailed. Detect receipt presence before
    // falling back to OS exit status.
    let structured = std::str::from_utf8(stdout)
        .ok()
        .and_then(|text| text.lines().rev().find(|line| line.starts_with('{')))
        .and_then(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .is_some_and(|value| value.get("ok") == Some(&serde_json::Value::Bool(false)));
    if structured {
        return ErrorCode::BackendFailed;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return match signal {
                libc::SIGKILL | libc::SIGXCPU | libc::SIGXFSZ => ErrorCode::ResourceExhausted,
                libc::SIGINT | libc::SIGTERM => ErrorCode::Cancelled,
                libc::SIGABRT | libc::SIGBUS | libc::SIGILL | libc::SIGSEGV => ErrorCode::Internal,
                _ => ErrorCode::Unavailable,
            };
        }
    }
    match status.code() {
        Some(137 | 152 | 153) => ErrorCode::ResourceExhausted,
        Some(130 | 143) => ErrorCode::Cancelled,
        Some(1 | 2) => ErrorCode::PluginProtocolError,
        Some(_) => ErrorCode::Internal,
        None => ErrorCode::Unavailable,
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_render(
    runtime: &RendererRuntime,
    output_root: &Path,
    generated: &Path,
    project: &Project,
    plan: &RenderPlan,
    id: &str,
    cancel: &CancellationToken,
    jobs: &Arc<Mutex<BTreeMap<String, Job>>>,
    job_ref: &str,
) -> Result<ArtifactSummary> {
    set_state(jobs, job_ref, RenderState::Starting).await;
    if std::env::var("SEMWRIGHT_DRIVER_SANDBOX").as_deref() != Ok("landlock-bwrap-v1") {
        return Err(Error::new(
            ErrorCode::SandboxDenied,
            "Rendering is only available inside the Semwright Driver Host sandbox",
        ));
    }
    if cancel.is_cancelled() {
        return Err(Error::new(
            ErrorCode::Cancelled,
            "Render cancelled before start",
        ));
    }
    fs::create_dir_all(output_root)?;
    let output = output_root.join(id);
    fs::create_dir(&output)?;
    let render_input_digest = semwright_semantic_composition::canonical_digest(&(
        project,
        plan,
        crate::authoring::COMPILER_EXTENSION_VERSION,
        security::sha256(&fs::read(&runtime.helper)?),
        &runtime.dependency_lock_sha256,
        &runtime.font_resources_sha256,
    ))
    .map_err(|e| Error::invalid(e.to_string()))?;
    let config = json!({
        "authoring": project.authoring.is_some(),
        "renderInputDigest":render_input_digest.as_str(),
        "fontResourcesDigest":runtime.font_resources_sha256,
        "name": "frames",
        "width": plan.width,
        "height": plan.height,
        "fps": f64::from(plan.fps) / f64::from(plan.fps_denominator),
        "fpsNum": plan.fps,
        "fpsDen": plan.fps_denominator,
        "firstFrame": plan.first_frame,
        "endFrameExclusive": plan.end_frame_exclusive,
        "colorSpace": match plan.color_space { ColorSpace::Srgb => "srgb", ColorSpace::DisplayP3 => "display-p3" },
        "background": project.settings.background,
        "alpha": plan.alpha,
        "timeoutMs": plan.timeout_ms,
    });
    let encoded = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&config)?);

    // The narrow Node helper owns the pinned Firefox process. Both inherit the
    // same process group plus Driver Host Bubblewrap + Landlock confinement.
    // No browser flags or executable paths come from agent input.
    let mut command = Command::new(&runtime.node);
    command
        .args(NODE_RENDER_FLAGS)
        .arg(&runtime.helper)
        .arg("--project")
        .arg(generated)
        .arg("--output")
        .arg(&output)
        .arg("--config")
        .arg(encoded)
        .arg("--browser")
        .arg(&runtime.browser)
        .kill_on_drop(true)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", "/home")
        .env("LANG", "C.UTF-8")
        .env("FONTCONFIG_PATH", "/etc/fonts")
        .env("FONTCONFIG_FILE", "fonts.conf")
        .env("TMPDIR", &output)
        .env("TMP", &output)
        .env("TEMP", &output)
        .env("XDG_CACHE_HOME", output.join(".cache"))
        .env("XDG_CONFIG_HOME", output.join(".config"))
        .env("XDG_DATA_HOME", output.join(".data"))
        .env("SEMWRIGHT_DRIVER_SANDBOX", "landlock-bwrap-v1");
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.as_std_mut().process_group(0);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let _ = fs::remove_dir_all(&output);
            return Err(Error::new(
                ErrorCode::Unavailable,
                format!("Failed to start pinned renderer helper: {error}"),
            ));
        }
    };
    let pid = child.id();
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            terminate_tree(pid, &mut child).await;
            let _ = fs::remove_dir_all(&output);
            return Err(Error::new(
                ErrorCode::Internal,
                "Renderer stdout unavailable",
            ));
        }
    };
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            terminate_tree(pid, &mut child).await;
            let _ = fs::remove_dir_all(&output);
            return Err(Error::new(
                ErrorCode::Internal,
                "Renderer stderr unavailable",
            ));
        }
    };
    let out_task = tokio::spawn(async move {
        let mut bytes = vec![];
        stdout
            .take(MAX_PROCESS_OUTPUT + 1)
            .read_to_end(&mut bytes)
            .await
            .map(|_| bytes)
    });
    let err_task = tokio::spawn(async move {
        let mut bytes = vec![];
        stderr
            .take(MAX_PROCESS_OUTPUT + 1)
            .read_to_end(&mut bytes)
            .await
            .map(|_| bytes)
    });
    set_state(jobs, job_ref, RenderState::Rendering).await;
    let status = tokio::select! {
        status = child.wait() => status?,
        _ = cancel.cancelled() => {
            terminate_tree(pid, &mut child).await;
            let _ = fs::remove_dir_all(&output);
            return Err(Error::new(ErrorCode::Cancelled, "Render cancelled"));
        }
        _ = tokio::time::sleep(Duration::from_millis(plan.timeout_ms)) => {
            terminate_tree(pid, &mut child).await;
            let _ = fs::remove_dir_all(&output);
            return Err(Error::new(ErrorCode::Timeout, "Render exceeded timeout"));
        }
    };
    let stdout = out_task
        .await
        .map_err(|_| Error::new(ErrorCode::Internal, "Renderer stdout task failed"))??;
    let stderr = err_task
        .await
        .map_err(|_| Error::new(ErrorCode::Internal, "Renderer stderr task failed"))??;
    if stdout.len() > MAX_PROCESS_OUTPUT as usize || stderr.len() > MAX_PROCESS_OUTPUT as usize {
        let _ = fs::remove_dir_all(&output);
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Renderer process output exceeded byte budget",
        ));
    }
    if !status.success() {
        let _ = fs::remove_dir_all(&output);
        let code = renderer_process_failure_code(&status, &stdout);
        return Err(Error::new(
            code,
            format!(
                "Pinned Motion Canvas renderer exited without a validated success receipt ({code:?})"
            ),
        ));
    }
    let stdout = String::from_utf8(stdout).map_err(|_| {
        Error::new(
            ErrorCode::PluginProtocolError,
            "Renderer returned non-UTF8 output",
        )
    })?;
    let result_line = stdout
        .lines()
        .rev()
        .find(|line| line.starts_with('{'))
        .ok_or_else(|| {
            Error::new(
                ErrorCode::PluginProtocolError,
                "Renderer returned no structured result",
            )
        })?;
    let value: serde_json::Value = serde_json::from_str(result_line).map_err(|_| {
        Error::new(
            ErrorCode::PluginProtocolError,
            "Renderer returned malformed result",
        )
    })?;
    if value.get("ok") != Some(&serde_json::Value::Bool(true)) {
        let _ = fs::remove_dir_all(&output);
        return Err(Error::new(
            ErrorCode::PluginProtocolError,
            "Renderer did not report success",
        ));
    }
    // Full-film validation decodes and hashes every rendered PNG. Keep that bounded
    // synchronous work off the current-thread protocol runtime so render.status and
    // cancellation requests remain responsive while large artifacts are certified.
    let validation_output = output.clone();
    let validation_plan = plan.clone();
    let validation_authoring = project.authoring.is_some();
    let validation_input_digest = render_input_digest.as_str().to_owned();
    let validation = tokio::task::spawn_blocking(move || {
        validate_artifacts(
            &validation_output,
            &validation_plan,
            validation_authoring,
            &validation_input_digest,
        )
    })
    .await
    .map_err(|_| {
        Error::new(
            ErrorCode::Internal,
            "Render artifact validation worker failed",
        )
    })?;
    match validation {
        Ok(artifact) => Ok(artifact),
        Err(error) => {
            let _ = fs::remove_dir_all(&output);
            if error.code == ErrorCode::BackendFailed {
                Err(Error::new(ErrorCode::ProtocolMismatch, error.message))
            } else {
                Err(error)
            }
        }
    }
}

#[cfg(unix)]
async fn terminate_tree(pid: Option<u32>, child: &mut tokio::process::Child) {
    if let Some(pid) = pid {
        // SAFETY: kill is called with a process-group id created for this owned render child.
        unsafe { libc::kill(-(pid as i32), libc::SIGTERM) };
    }
    if tokio::time::timeout(Duration::from_secs(2), child.wait())
        .await
        .is_err()
    {
        if let Some(pid) = pid {
            // SAFETY: same owned process group as above; SIGKILL is the bounded fallback.
            unsafe { libc::kill(-(pid as i32), libc::SIGKILL) };
        }
        let _ = child.wait().await;
    }
}

#[cfg(not(unix))]
async fn terminate_tree(_pid: Option<u32>, child: &mut tokio::process::Child) {
    let _ = child.kill().await;
    let _ = child.wait().await;
}

fn validate_artifacts(
    output: &Path,
    plan: &RenderPlan,
    authoring: bool,
    render_input_digest: &str,
) -> Result<ArtifactSummary> {
    let frames = output.join("frames");
    let meta = fs::symlink_metadata(&frames)?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err(Error::new(
            ErrorCode::BackendFailed,
            "Renderer frame directory is missing or unsafe",
        ));
    }
    let mut entries = fs::read_dir(&frames)?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    if entries.len() as u64 != plan.frame_count {
        return Err(Error::new(
            ErrorCode::BackendFailed,
            format!(
                "Renderer produced {} frames; expected {}",
                entries.len(),
                plan.frame_count
            ),
        ));
    }
    // Small renders are exhaustively decoded for pixel evidence. Long sequences keep
    // full filename/symlink/byte-hash/IHDR validation for every frame and deep-decode
    // five deterministic samples. This avoids spending the driver's entire CPU budget
    // re-decoding thousands of PNGs that came from the already-bounded exporter.
    const FULL_PIXEL_VALIDATION_LIMIT: usize = 60;
    let pixel_samples = if entries.len() <= FULL_PIXEL_VALIDATION_LIMIT {
        (0..entries.len()).collect::<Vec<_>>()
    } else {
        let mut samples = vec![
            0,
            entries.len() / 4,
            entries.len() / 2,
            entries.len() * 3 / 4,
            entries.len() - 1,
        ];
        samples.sort_unstable();
        samples.dedup();
        samples
    };
    let mut manifest = Vec::with_capacity(entries.len());
    let mut saw_transparency = false;
    for (index, entry) in entries.iter().enumerate() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let expected = format!("{:06}.png", plan.first_frame + index as u64);
        if name != expected || entry.file_type()?.is_symlink() {
            return Err(Error::new(
                ErrorCode::BackendFailed,
                "Unexpected render artifact",
            ));
        }
        let bytes = fs::read(entry.path())?;
        let deep = pixel_samples.contains(&index);
        let pixels = if deep {
            Some(security::inspect_png(&bytes)?)
        } else {
            None
        };
        let (width, height) = if let Some(png) = &pixels {
            (png.width, png.height)
        } else {
            let header = security::inspect_png_header(&bytes)?;
            (header.width, header.height)
        };
        if width != plan.width || height != plan.height {
            return Err(Error::new(
                ErrorCode::BackendFailed,
                "Rendered PNG dimensions do not match plan",
            ));
        }
        if let Some(png) = &pixels {
            saw_transparency |= png.min_alpha < 255;
        }
        manifest.push(json!({
            "index": index,
            "file": format!("frames/{name}"),
            "bytes": bytes.len(),
            "sha256": security::sha256(&bytes),
            "pixel_sha256": pixels.as_ref().map(|png| png.pixel_sha256.as_str()),
            "min_alpha": pixels.as_ref().map(|png| png.min_alpha),
            "max_alpha": pixels.as_ref().map(|png| png.max_alpha),
        }));
    }
    if plan.alpha && !saw_transparency {
        return Err(Error::new(
            ErrorCode::BackendFailed,
            "Transparent render produced no transparent pixels",
        ));
    }
    let native_observations = if authoring {
        let receipt_bytes =
            crate::store::read_granted_file(output, "native-observations-receipt.json", 4096)?;
        let receipt: serde_json::Value = serde_json::from_slice(&receipt_bytes)?;
        let data = crate::store::read_granted_file(
            output,
            "native-observations.ndjson",
            64 * 1024 * 1024,
        )?;
        if receipt
            .get("render_input_digest")
            .and_then(serde_json::Value::as_str)
            != Some(render_input_digest)
            || receipt.get("sha256").and_then(serde_json::Value::as_str)
                != Some(security::sha256(&data).as_str())
            || receipt.get("bytes").and_then(serde_json::Value::as_u64) != Some(data.len() as u64)
            || receipt.get("frames").and_then(serde_json::Value::as_u64) != Some(plan.frame_count)
            || !receipt
                .get("font_resources_sha256")
                .and_then(serde_json::Value::as_str)
                .is_some_and(security::digest)
        {
            return Err(Error::new(
                ErrorCode::BackendFailed,
                "native observation receipt mismatch",
            ));
        }
        Some(receipt)
    } else {
        None
    };
    let manifest_bytes = serde_json::to_vec_pretty(&json!({
        "native_observations":native_observations,
        "renderer": "motion-canvas-core-renderer-v3.17.2",
        "plan": plan,
        "pixel_validation": {
            "mode": if entries.len() <= FULL_PIXEL_VALIDATION_LIMIT { "all" } else { "sampled" },
            "sample_indices": pixel_samples,
        },
        "frames": manifest,
    }))?;
    let manifest_path = output.join("artifact-manifest.json");
    fs::write(&manifest_path, &manifest_bytes)?;
    std::fs::File::open(&manifest_path)?.sync_all()?;
    let first = entries
        .first()
        .unwrap()
        .file_name()
        .to_string_lossy()
        .into_owned();
    let last = entries
        .last()
        .unwrap()
        .file_name()
        .to_string_lossy()
        .into_owned();
    let directory = output.file_name().unwrap().to_string_lossy().into_owned();
    Ok(ArtifactSummary {
        directory: directory.clone(),
        manifest: format!("{directory}/artifact-manifest.json"),
        frame_count: plan.frame_count,
        first_png: format!("{directory}/frames/{first}"),
        last_png: format!("{directory}/frames/{last}"),
        manifest_sha256: security::sha256(&manifest_bytes),
        manifest_bytes: u64::try_from(manifest_bytes.len()).map_err(|_| {
            Error::new(ErrorCode::ResourceExhausted, "manifest byte count overflow")
        })?,
    })
}

/// Returned only for a completed, source-bound job. Paths never originate in capability args.
pub struct NativeObservationBundle {
    pub bytes: Vec<u8>,
    pub observation_sha256: String,
    pub render_input_digest: String,
    pub font_resources_sha256: String,
    pub artifact_sha256: String,
    pub plan: RenderPlan,
}
impl RenderManager {
    pub async fn native_observations(
        &self,
        job_ref: &str,
        expected_source: &str,
    ) -> Result<NativeObservationBundle> {
        let guard = self.jobs.lock().await;
        let job = guard
            .get(job_ref)
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "render job not found"))?;
        if job.source_sha256 != expected_source || job.view.state != RenderState::Succeeded {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "render is not completed for the current project source",
            ));
        }
        let artifact = job
            .view
            .artifact
            .clone()
            .ok_or_else(|| Error::new(ErrorCode::BackendFailed, "render artifact absent"))?;
        drop(guard);
        let bytes = crate::store::read_granted_file(
            &self.output_root,
            &artifact.manifest,
            8 * 1024 * 1024,
        )?;
        if security::sha256(&bytes) != artifact.manifest_sha256 {
            return Err(Error::new(
                ErrorCode::BackendFailed,
                "render manifest changed",
            ));
        }
        let manifest: serde_json::Value = serde_json::from_slice(&bytes)?;
        let receipt = manifest
            .get("native_observations")
            .ok_or_else(|| Error::invalid("not an instrumented authoring render"))?;
        let observation_sha256 = receipt
            .get("sha256")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| Error::invalid("native observation digest absent"))?
            .to_owned();
        let render_input_digest = receipt
            .get("render_input_digest")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| Error::invalid("native render input binding absent"))?
            .to_owned();
        let font_resources_sha256 = receipt
            .get("font_resources_sha256")
            .and_then(serde_json::Value::as_str)
            .filter(|value| security::digest(value))
            .ok_or_else(|| Error::invalid("native font evidence binding absent"))?
            .to_owned();
        let bytes = crate::store::read_granted_file(
            &self.output_root,
            &format!("{}/native-observations.ndjson", artifact.directory),
            64 * 1024 * 1024,
        )?;
        if security::sha256(&bytes) != observation_sha256 {
            return Err(Error::new(
                ErrorCode::BackendFailed,
                "native observation stream changed",
            ));
        }
        let plan = serde_json::from_value(
            manifest
                .get("plan")
                .cloned()
                .ok_or_else(|| Error::invalid("render plan missing"))?,
        )?;
        Ok(NativeObservationBundle {
            bytes,
            observation_sha256,
            render_input_digest,
            font_resources_sha256,
            artifact_sha256: artifact.manifest_sha256,
            plan,
        })
    }
}

#[cfg(test)]
mod runtime_path_tests {
    use super::*;

    #[test]
    fn pinned_runtime_paths_allow_npm_scopes_but_not_traversal_or_urls() {
        runtime_relative_path(".semwright-tools/node").unwrap();
        runtime_relative_path("node_modules/@fontsource-variable/instrument-sans/index.css")
            .unwrap();
        runtime_relative_path(
            "node_modules/playwright-core/.local-browsers/firefox-1532/firefox/firefox",
        )
        .unwrap();

        for hostile in [
            "../escape",
            "node_modules/../escape",
            "https://example.com/browser",
            "node_modules/@scope/pkg%2fescape",
            "/absolute/tool",
        ] {
            assert!(runtime_relative_path(hostile).is_err(), "{hostile}");
        }
    }

    #[test]
    fn renderer_failure_receipt_accepts_only_allowlisted_phase_classes() {
        assert_eq!(
            renderer_failure_code(br#"{"ok":false,"errorClass":"vite_build"}"#),
            ErrorCode::PluginProtocolError
        );
        assert_eq!(
            renderer_failure_code(br#"{"ok":false,"errorClass":"browser_launch"}"#),
            ErrorCode::Unavailable
        );
        assert_eq!(
            renderer_failure_code(br#"{"ok":false,"errorClass":"observation"}"#),
            ErrorCode::ProtocolMismatch
        );
        assert_eq!(
            renderer_failure_code(br#"{"ok":false,"errorClass":"render_wait"}"#),
            ErrorCode::Internal
        );
        assert_eq!(
            renderer_failure_code(br#"{"ok":false,"errorClass":"render_wait_timeout"}"#),
            ErrorCode::Timeout
        );
        assert_eq!(
            renderer_failure_code(br#"{"ok":false,"errorClass":"renderer_state_frame_clock"}"#),
            ErrorCode::ProtocolMismatch
        );
        assert_eq!(
            renderer_failure_code(br#"{"ok":false,"errorClass":"renderer_state_model_invariant"}"#),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            renderer_failure_code(br#"{"ok":false,"errorClass":"renderer_state_type_error"}"#),
            ErrorCode::PluginProtocolError
        );
        assert_eq!(
            renderer_failure_code(br#"{"ok":false,"errorClass":"renderer_state_range_error"}"#),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            renderer_failure_code(br#"{"ok":false,"errorClass":"renderer_state_authoring_model"}"#),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            renderer_failure_code(
                br#"{"ok":false,"errorClass":"renderer_state_authoring_protocol"}"#
            ),
            ErrorCode::ProtocolMismatch
        );
        assert_eq!(
            renderer_failure_code(br#"{"ok":false,"errorClass":"render_nonzero"}"#),
            ErrorCode::ProtocolMismatch
        );
        assert_eq!(
            renderer_failure_code(br#"{"ok":false,"errorClass":"render_result_error"}"#),
            ErrorCode::Internal
        );
        assert_eq!(
            renderer_failure_code(br#"{"ok":false,"errorClass":"renderer_log_exporter_missing"}"#),
            ErrorCode::ProtocolMismatch
        );
        assert_eq!(
            renderer_failure_code(br#"{"ok":false,"errorClass":"renderer_log_async_property"}"#),
            ErrorCode::ProtocolMismatch
        );
        assert_eq!(
            renderer_failure_code(br#"{"ok":false,"errorClass":"renderer_log_type_error"}"#),
            ErrorCode::PluginProtocolError
        );
        assert_eq!(
            renderer_failure_code(br#"{"ok":false,"errorClass":"renderer_log_range_error"}"#),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            renderer_failure_code(br#"{"ok":false,"errorClass":"renderer_log_authoring_model"}"#),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            renderer_failure_code(
                br#"{"ok":false,"errorClass":"renderer_log_authoring_protocol"}"#
            ),
            ErrorCode::ProtocolMismatch
        );
        assert_eq!(
            renderer_failure_code(br#"{"ok":false,"errorClass":"render_result_aborted"}"#),
            ErrorCode::Cancelled
        );
        for hostile in [
            br#"{"ok":false,"errorClass":"../../escape"}"#.as_slice(),
            br#"{"ok":true,"errorClass":"vite_build"}"#.as_slice(),
            b"not-json".as_slice(),
        ] {
            assert_eq!(
                renderer_failure_code(hostile),
                ErrorCode::BackendFailed,
                "{hostile:?}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn renderer_process_exit_without_receipt_is_safely_classified() {
        use std::os::unix::process::ExitStatusExt;

        let exit_one = std::process::ExitStatus::from_raw(1 << 8);
        assert_eq!(
            renderer_process_failure_code(&exit_one, b""),
            ErrorCode::PluginProtocolError
        );

        let killed = std::process::ExitStatus::from_raw(libc::SIGKILL);
        assert_eq!(
            renderer_process_failure_code(&killed, b""),
            ErrorCode::ResourceExhausted
        );

        let cpu = std::process::ExitStatus::from_raw(libc::SIGXCPU);
        assert_eq!(
            renderer_process_failure_code(&cpu, b""),
            ErrorCode::ResourceExhausted
        );

        let structured = br#"{"ok":false,"errorClass":"renderer_log_type_error"}"#;
        assert_eq!(
            renderer_process_failure_code(&exit_one, structured),
            ErrorCode::Internal
        );
    }

    #[tokio::test]
    async fn render_failure_code_stays_private_but_is_preserved_for_execute_context() {
        let manager = RenderManager::new(None, PathBuf::from("/tmp/not-used"));
        let job_ref = "job:test".to_owned();
        let public = JobView {
            job_ref: job_ref.clone(),
            state: RenderState::Failed,
            error: Some("redacted-in-protocol".into()),
            artifact: None,
        };
        manager.jobs.lock().await.insert(
            job_ref.clone(),
            Job {
                source_sha256: "a".repeat(64),
                view: public.clone(),
                failure_code: Some(ErrorCode::ResourceExhausted),
                cancel: CancellationToken::new(),
            },
        );
        assert_eq!(
            manager.failure_code(&job_ref).await,
            Some(ErrorCode::ResourceExhausted)
        );
        let wire = serde_json::to_value(public).unwrap();
        assert!(wire.get("failure_code").is_none());
    }
}
