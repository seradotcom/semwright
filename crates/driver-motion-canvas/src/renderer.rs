//! Driver-local Motion Canvas render jobs for Driver Protocol v1.
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

#[derive(Debug, Clone)]
pub struct RendererRuntime {
    pub node: PathBuf,
    pub helper: PathBuf,
    pub browser: PathBuf,
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
            security::relative_path(&tool.path)?;
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
        Ok(Self {
            node: resolve(&config.node)?,
            helper: resolve(&config.helper)?,
            browser: resolve(&config.browser)?,
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
    view: JobView,
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
                view: view.clone(),
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
                        job.view.error = Some(error.message);
                    }
                    Err(error) => {
                        job.view.state = RenderState::Failed;
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
    let config = json!({
        "name": "frames",
        "width": plan.width,
        "height": plan.height,
        "fps": plan.fps,
        "firstFrame": plan.first_frame,
        "endFrameExclusive": plan.end_frame_exclusive,
        "colorSpace": match plan.color_space { ColorSpace::Srgb => "srgb", ColorSpace::DisplayP3 => "display-p3" },
        "background": project.settings.background,
        "alpha": plan.alpha,
        "timeoutMs": plan.timeout_ms,
    });
    let encoded = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&config)?);
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
        // Keep all renderer/browser ephemeral state inside this job's writable,
        // owner-granted output directory. This is required because Chromium's
        // Playwright profile must remain visible across its subprocesses inside
        // the outer Bubblewrap + Landlock sandbox.
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
    let mut child = command.spawn().map_err(|error| {
        Error::new(
            ErrorCode::BackendFailed,
            format!("Failed to start pinned renderer: {error}"),
        )
    })?;
    let pid = child.id();
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::new(ErrorCode::Internal, "Renderer stdout unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| Error::new(ErrorCode::Internal, "Renderer stderr unavailable"))?;
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
        .map_err(|_| Error::new(ErrorCode::BackendFailed, "Renderer stdout task failed"))??;
    let stderr = err_task
        .await
        .map_err(|_| Error::new(ErrorCode::BackendFailed, "Renderer stderr task failed"))??;
    if stdout.len() > MAX_PROCESS_OUTPUT as usize || stderr.len() > MAX_PROCESS_OUTPUT as usize {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Renderer process output exceeded byte budget",
        ));
    }
    if !status.success() {
        let message = String::from_utf8_lossy(&stderr);
        return Err(Error::new(
            ErrorCode::BackendFailed,
            format!(
                "Motion Canvas renderer failed: {}",
                message.chars().take(2000).collect::<String>()
            ),
        ));
    }
    let stdout = String::from_utf8(stdout).map_err(|_| {
        Error::new(
            ErrorCode::BackendFailed,
            "Renderer returned non-UTF8 output",
        )
    })?;
    let result_line = stdout
        .lines()
        .rev()
        .find(|line| line.starts_with('{'))
        .ok_or_else(|| {
            Error::new(
                ErrorCode::BackendFailed,
                "Renderer returned no structured result",
            )
        })?;
    let value: serde_json::Value = serde_json::from_str(result_line).map_err(|_| {
        Error::new(
            ErrorCode::BackendFailed,
            "Renderer returned malformed result",
        )
    })?;
    if value.get("ok") != Some(&serde_json::Value::Bool(true)) {
        return Err(Error::new(
            ErrorCode::BackendFailed,
            "Renderer did not report success",
        ));
    }
    validate_artifacts(&output, plan)
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

fn validate_artifacts(output: &Path, plan: &RenderPlan) -> Result<ArtifactSummary> {
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
        let png = security::inspect_png(&bytes)?;
        if png.width != plan.width || png.height != plan.height {
            return Err(Error::new(
                ErrorCode::BackendFailed,
                "Rendered PNG dimensions do not match plan",
            ));
        }
        saw_transparency |= png.min_alpha < 255;
        manifest.push(json!({
            "index": index,
            "file": format!("frames/{name}"),
            "bytes": bytes.len(),
            "sha256": security::sha256(&bytes),
            "pixel_sha256": png.pixel_sha256,
            "min_alpha": png.min_alpha,
            "max_alpha": png.max_alpha,
        }));
    }
    if plan.alpha && !saw_transparency {
        return Err(Error::new(
            ErrorCode::BackendFailed,
            "Transparent render produced no transparent pixels",
        ));
    }
    let manifest_bytes = serde_json::to_vec_pretty(&json!({
        "renderer": "motion-canvas-core-renderer-v3.17.2",
        "plan": plan,
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
    })
}
