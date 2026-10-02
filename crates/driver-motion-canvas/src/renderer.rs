//! Motion Canvas render jobs backed exclusively by Driver Host runtime-tool jobs.
use crate::{
    Error, ErrorCode, Result,
    model::{ColorSpace, RenderProfile},
    refs::{Kind, Reference},
    security,
    store::Snapshot,
    validate::RenderPlan,
};
use schemars::JsonSchema;
use semwright_driver_sdk::{
    DriverExecutionContext, RuntimeToolArg, RuntimeToolCwd, RuntimeToolJob, RuntimeToolJobStatus,
    ToolExecutionOutput,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::Mutex;

const MAX_JOBS: usize = 64;
const HOST_TOOL: &str = "motion-node";
const RENDER_HELPER: &str = include_str!("../../../integrations/motion-canvas/runtime/render.mjs");
const RUNTIME_MOUNT: &str = "runtime";
const PROJECT_MOUNT: &str = "project";
const OUTPUT_MOUNT: &str = "output";
const FONTCONFIG_MOUNT: &str = "fontconfig";
const NODE_RENDER_FLAGS: [&str; 2] = ["--disable-wasm-trap-handler", "--max-old-space-size=512"];

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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RenderFailureClass {
    Arguments,
    FontEvidence,
    ProjectStage,
    ViteBuild,
    FrameExport,
    BrowserLaunch,
    PageLoad,
    RenderWait,
    RenderWaitTimeout,
    RendererStateFrameClock,
    RendererStateAuthoringProtocol,
    RendererLogAuthoringProtocol,
    RendererStatePlaybackProtocol,
    RendererLogPlaybackProtocol,
    RendererLogExporterMissing,
    RendererLogAsyncProperty,
    RendererStateWebglUnavailable,
    RendererLogWebglUnavailable,
    RendererStateModelInvariant,
    RendererLogModelInvariant,
    RendererStateAuthoringModel,
    RendererLogAuthoringModel,
    RendererStateInvalidScene,
    RendererLogInvalidScene,
    RendererStateRangeError,
    RendererLogRangeError,
    RendererStateTypeError,
    RendererLogTypeError,
    RendererStateSemwrightNative,
    RendererStateSemwrightExporter,
    RendererStateMotionCore,
    #[serde(rename = "renderer_state_motion_2d")]
    RendererStateMotion2d,
    RendererStateBeforeFirstFrame,
    RendererStateAfterFirstFrame,
    RendererStateError,
    RendererLogError,
    RenderResultAborted,
    RenderResultError,
    RenderResultUnknown,
    RenderNonzero,
    RuntimeModuleLoad,
    RuntimeSyntax,
    RuntimePermission,
    RuntimeOom,
    RuntimeKilled,
    RuntimeCpuLimit,
    RuntimeFileSizeLimit,
    RuntimeSignal,
    Observation,
    Finalize,
    Startup,
}

impl RenderFailureClass {
    fn code(self) -> ErrorCode {
        match self {
            Self::ViteBuild
            | Self::RendererStateTypeError
            | Self::RendererLogTypeError
            | Self::RuntimeModuleLoad
            | Self::RuntimeSyntax => ErrorCode::PluginProtocolError,
            Self::FontEvidence
            | Self::FrameExport
            | Self::Observation
            | Self::Finalize
            | Self::RendererStateFrameClock
            | Self::RendererStateAuthoringProtocol
            | Self::RendererLogAuthoringProtocol
            | Self::RendererStatePlaybackProtocol
            | Self::RendererLogPlaybackProtocol
            | Self::RendererLogExporterMissing
            | Self::RendererLogAsyncProperty
            | Self::RendererStateSemwrightExporter
            | Self::RendererStateBeforeFirstFrame
            | Self::RenderResultUnknown
            | Self::RenderNonzero => ErrorCode::ProtocolMismatch,
            Self::RuntimePermission => ErrorCode::SandboxDenied,
            Self::RuntimeOom
            | Self::RuntimeKilled
            | Self::RuntimeCpuLimit
            | Self::RuntimeFileSizeLimit => ErrorCode::ResourceExhausted,
            Self::BrowserLaunch
            | Self::PageLoad
            | Self::ProjectStage
            | Self::RendererStateWebglUnavailable
            | Self::RendererLogWebglUnavailable => ErrorCode::Unavailable,
            Self::Arguments
            | Self::RendererStateModelInvariant
            | Self::RendererLogModelInvariant
            | Self::RendererStateAuthoringModel
            | Self::RendererLogAuthoringModel
            | Self::RendererStateInvalidScene
            | Self::RendererLogInvalidScene
            | Self::RendererStateRangeError
            | Self::RendererLogRangeError
            | Self::RendererStateSemwrightNative => ErrorCode::InvalidArgument,
            Self::RenderWait | Self::RenderWaitTimeout => ErrorCode::Timeout,
            Self::RenderResultAborted => ErrorCode::Cancelled,
            Self::RendererStateMotion2d
            | Self::RendererStateAfterFirstFrame
            | Self::RenderResultError
            | Self::Startup => ErrorCode::BackendFailed,
            Self::RendererStateMotionCore
            | Self::RendererStateError
            | Self::RendererLogError
            | Self::RuntimeSignal => ErrorCode::Internal,
        }
    }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_class: Option<RenderFailureClass>,
    pub error: Option<String>,
    pub artifact: Option<ArtifactSummary>,
}

#[derive(Clone)]
struct Job {
    view: JobView,
    host_job: Option<RuntimeToolJob>,
    source_sha256: String,
    failure_code: Option<ErrorCode>,
    authoring: bool,
    render_input_digest: String,
    plan: RenderPlan,
    output: PathBuf,
}

#[derive(Clone)]
pub struct RenderManager {
    jobs: Arc<Mutex<BTreeMap<String, Job>>>,
    output_root: PathBuf,
    host_managed: bool,
}

impl RenderManager {
    pub fn new(host_managed: bool, output_root: PathBuf) -> Self {
        Self {
            jobs: Arc::new(Mutex::new(BTreeMap::new())),
            output_root,
            host_managed,
        }
    }

    pub fn available(&self) -> bool {
        self.host_managed
    }

    pub async fn failure_code(&self, job_ref: &str) -> Option<ErrorCode> {
        self.jobs
            .lock()
            .await
            .get(job_ref)
            .and_then(|job| job.failure_code)
    }

    pub async fn active_count(&self) -> usize {
        self.jobs
            .lock()
            .await
            .values()
            .filter(|job| active(&job.view.state))
            .count()
    }

    pub async fn start_host(
        &self,
        snapshot: &Snapshot,
        project_root: &Path,
        profile: RenderProfile,
        context: &DriverExecutionContext,
    ) -> Result<JobView> {
        if !self.host_managed {
            return Err(Error::new(
                ErrorCode::Unavailable,
                "Host-managed Motion Canvas runtime is unavailable",
            ));
        }
        context.check_cancelled()?;
        let generated = snapshot.generated_dir.as_ref().ok_or_else(|| {
            Error::new(
                ErrorCode::Unavailable,
                "Generated project has not been materialized for rendering",
            )
        })?;
        let generated_relative = generated.strip_prefix(project_root).map_err(|_| {
            Error::new(
                ErrorCode::PermissionDenied,
                "Generated project escaped the owner-granted project root",
            )
        })?;
        let generated_relative = portable_relative(generated_relative)?;
        let plan = crate::validate::render_plan(&snapshot.project, &profile)?;
        let inputs = render_inputs(snapshot, &plan)?;
        let id = format!("render-{}", uuid::Uuid::new_v4().simple());
        let job_ref = Reference::new(
            &snapshot.project,
            &snapshot.source_sha256,
            Kind::RenderJob,
            &id,
        )
        .encode();
        let output = self.output_root.join(&id);

        let view = JobView {
            job_ref: job_ref.clone(),
            state: RenderState::Starting,
            failure_class: None,
            error: None,
            artifact: None,
        };
        {
            let mut guard = self.jobs.lock().await;
            if guard.len() >= MAX_JOBS {
                guard.retain(|_, job| active(&job.view.state));
            }
            if guard.values().filter(|job| active(&job.view.state)).count() >= 2 {
                return Err(Error::new(
                    ErrorCode::ResourceExhausted,
                    "At most two Motion Canvas render jobs may run concurrently",
                ));
            }
            fs::create_dir_all(&self.output_root)?;
            fs::create_dir(&output)?;
            guard.insert(
                job_ref.clone(),
                Job {
                    view: view.clone(),
                    host_job: None,
                    source_sha256: snapshot.source_sha256.clone(),
                    failure_code: None,
                    authoring: snapshot.project.authoring.is_some(),
                    render_input_digest: inputs.digest.clone(),
                    plan: plan.clone(),
                    output: output.clone(),
                },
            );
        }

        let (args, program) = host_args(snapshot, &plan, &generated_relative, &id, &inputs)?;
        let host_job = match context
            .start_runtime_tool_job_args(
                HOST_TOOL,
                args,
                program,
                host_timeout(&plan),
                Some(RuntimeToolCwd {
                    mount: RUNTIME_MOUNT.into(),
                    relative: String::new(),
                }),
            )
            .await
        {
            Ok(job) => job,
            Err(error) => {
                self.jobs.lock().await.remove(&job_ref);
                let _ = fs::remove_dir_all(&output);
                return Err(error);
            }
        };
        let mut guard = self.jobs.lock().await;
        let reserved = guard.get_mut(&job_ref).ok_or_else(|| {
            Error::new(
                ErrorCode::Conflict,
                "Render job reservation disappeared before Host start completed",
            )
        })?;
        reserved.host_job = Some(host_job);
        Ok(reserved.view.clone())
    }

    pub async fn status_host(
        &self,
        job_ref: &str,
        context: &DriverExecutionContext,
    ) -> Result<JobView> {
        let host_job = {
            let guard = self.jobs.lock().await;
            let job = guard
                .get(job_ref)
                .ok_or_else(|| Error::new(ErrorCode::NotFound, "Unknown render job"))?;
            if !active(&job.view.state) {
                return Ok(job.view.clone());
            }
            job.host_job.clone().ok_or_else(|| {
                Error::new(
                    ErrorCode::Unavailable,
                    "Render job is still waiting for Host start acknowledgement",
                )
            })?
        };
        match context.runtime_tool_job_status(&host_job).await {
            Ok(status) => self.apply_host_status(job_ref, status).await,
            Err(error) if error.code == ErrorCode::NotFound => {
                let guard = self.jobs.lock().await;
                if let Some(job) = guard.get(job_ref)
                    && !active(&job.view.state)
                {
                    return Ok(job.view.clone());
                }
                Err(error)
            }
            Err(error) => Err(error),
        }
    }

    pub async fn cancel_host(
        &self,
        job_ref: &str,
        context: &DriverExecutionContext,
    ) -> Result<JobView> {
        let host_job = {
            let guard = self.jobs.lock().await;
            let job = guard
                .get(job_ref)
                .ok_or_else(|| Error::new(ErrorCode::NotFound, "Unknown render job"))?;
            if !active(&job.view.state) {
                return Ok(job.view.clone());
            }
            job.host_job.clone().ok_or_else(|| {
                Error::new(
                    ErrorCode::Unavailable,
                    "Render job is still waiting for Host start acknowledgement",
                )
            })?
        };
        let status = context.cancel_runtime_tool_job(&host_job).await?;
        self.apply_host_status(job_ref, status).await
    }

    pub async fn result_host(
        &self,
        job_ref: &str,
        context: &DriverExecutionContext,
    ) -> Result<JobView> {
        let view = self.status_host(job_ref, context).await?;
        if active(&view.state) {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Render job is not terminal",
            ));
        }
        Ok(view)
    }

    async fn apply_host_status(
        &self,
        job_ref: &str,
        status: RuntimeToolJobStatus,
    ) -> Result<JobView> {
        let (plan, output, current, authoring, render_input_digest) = {
            let guard = self.jobs.lock().await;
            let job = guard
                .get(job_ref)
                .ok_or_else(|| Error::new(ErrorCode::NotFound, "Unknown render job"))?;
            if !active(&job.view.state) {
                return Ok(job.view.clone());
            }
            (
                job.plan.clone(),
                job.output.clone(),
                job.view.clone(),
                job.authoring,
                job.render_input_digest.clone(),
            )
        };

        let failure_code = match &status {
            RuntimeToolJobStatus::Failed { error } => Some(error.code),
            RuntimeToolJobStatus::Succeeded { output } => {
                validate_host_result(output).err().map(|e| e.code)
            }
            _ => None,
        };
        let next = match status {
            RuntimeToolJobStatus::Running => JobView {
                state: RenderState::Rendering,
                ..current
            },
            RuntimeToolJobStatus::Cancelling => JobView {
                state: RenderState::Rendering,
                ..current
            },
            RuntimeToolJobStatus::Cancelled => JobView {
                state: RenderState::Cancelled,
                error: Some("Motion Canvas render cancelled".into()),
                ..current
            },
            RuntimeToolJobStatus::Failed { error } => JobView {
                state: RenderState::Failed,
                error: Some(error.message.chars().take(16_384).collect()),
                ..current
            },
            RuntimeToolJobStatus::Succeeded {
                output: tool_output,
            } => match validate_host_result(&tool_output) {
                Ok(()) => {
                    let validation_output = output.clone();
                    let validation_plan = plan.clone();
                    match tokio::task::spawn_blocking(move || {
                        validate_artifacts(
                            &validation_output,
                            &validation_plan,
                            authoring,
                            &render_input_digest,
                        )
                    })
                    .await
                    .map_err(|_| {
                        Error::new(
                            ErrorCode::BackendFailed,
                            "Render artifact validation worker failed",
                        )
                    })? {
                        Ok(artifact) => JobView {
                            state: RenderState::Succeeded,
                            error: None,
                            artifact: Some(artifact),
                            ..current
                        },
                        Err(error) => JobView {
                            state: RenderState::Failed,
                            failure_class: renderer_failure_class(&tool_output.stdout)
                                .or_else(|| renderer_stderr_failure_class(&tool_output.stderr)),
                            error: Some(error.message),
                            ..current
                        },
                    }
                }
                Err(error) => JobView {
                    state: RenderState::Failed,
                    error: Some(error.message),
                    ..current
                },
            },
        };

        let mut guard = self.jobs.lock().await;
        let job = guard
            .get_mut(job_ref)
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "Unknown render job"))?;
        if active(&job.view.state) {
            job.failure_code = failure_code;
            job.view = next;
        }
        Ok(job.view.clone())
    }
}

fn active(state: &RenderState) -> bool {
    matches!(
        state,
        RenderState::Queued | RenderState::Starting | RenderState::Rendering
    )
}

fn portable_relative(path: &Path) -> Result<String> {
    let mut parts = Vec::new();
    for component in path.components() {
        let Component::Normal(value) = component else {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "Generated project path is not a canonical relative path",
            ));
        };
        let value = value
            .to_str()
            .ok_or_else(|| Error::invalid("Generated project path must be Unicode"))?;
        if value.is_empty() || value.chars().any(char::is_control) {
            return Err(Error::invalid(
                "Generated project path component is invalid",
            ));
        }
        parts.push(value);
    }
    if parts.is_empty() {
        return Err(Error::invalid("Generated project path is empty"));
    }
    Ok(parts.join("/"))
}

fn literal(value: impl Into<String>) -> RuntimeToolArg {
    RuntimeToolArg::Literal {
        value: value.into(),
    }
}

fn mount(name: &str) -> RuntimeToolArg {
    RuntimeToolArg::MountPath {
        mount: name.into(),
        relative: String::new(),
    }
}

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

struct RenderInputs {
    digest: String,
    lock: String,
    fonts: String,
    pins: Vec<serde_json::Value>,
}
fn render_inputs(snapshot: &Snapshot, plan: &RenderPlan) -> Result<RenderInputs> {
    let runtime = semwright_driver_sdk::workspace_mount(RUNTIME_MOUNT)?;
    let lock_bytes =
        crate::store::read_granted_file(&runtime, "package-lock.json", 2 * 1024 * 1024)?;
    let lock = security::sha256(&lock_bytes);
    if lock
        != security::sha256(include_bytes!(
            "../../../integrations/motion-canvas/runtime/package-lock.json"
        ))
    {
        return Err(Error::new(
            ErrorCode::StaleReference,
            "Motion runtime dependency lock changed",
        ));
    }
    let mut resources = BTreeMap::new();
    for css_path in [
        "node_modules/@fontsource-variable/instrument-sans/index.css",
        "node_modules/@fontsource/ibm-plex-mono/400.css",
    ] {
        let bytes = crate::store::read_pinned_font_file(&runtime, css_path, 256 * 1024)?;
        let css = std::str::from_utf8(&bytes)
            .map_err(|_| Error::invalid("Font stylesheet is not UTF8"))?;
        resources.insert(css_path.to_owned(), security::sha256(&bytes));
        let mut count = 0;
        for part in css.split("url(").skip(1) {
            let reference = part
                .split(')')
                .next()
                .ok_or_else(|| Error::invalid("Font URL is unterminated"))?
                .trim_matches(['\'', '"']);
            if !reference.ends_with(".woff2") {
                continue;
            }
            let reference = reference.strip_prefix("./").unwrap_or(reference);
            runtime_relative_path(reference)?;
            let path = format!("{}/{}", css_path.rsplit_once('/').unwrap().0, reference);
            let bytes = crate::store::read_pinned_font_file(&runtime, &path, 16 * 1024 * 1024)?;
            if bytes.is_empty() {
                return Err(Error::invalid("Font resource is empty"));
            }
            resources.insert(path, security::sha256(&bytes));
            count += 1;
            if resources.len() > 128 {
                return Err(Error::new(
                    ErrorCode::ResourceExhausted,
                    "Font resource budget exceeded",
                ));
            }
        }
        if count == 0 {
            return Err(Error::invalid("Font stylesheet has no WOFF2 resources"));
        }
    }
    let mut hasher = Sha256::new();
    for (path, digest) in &resources {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(digest.as_bytes());
        hasher.update([0]);
    }
    let fonts = format!("{:x}", hasher.finalize());
    let digest = semwright_semantic_composition::canonical_digest(&(
        &snapshot.project,
        plan,
        crate::authoring::COMPILER_EXTENSION_VERSION,
        security::sha256(RENDER_HELPER.as_bytes()),
        &lock,
        &fonts,
    ))
    .map_err(|e| Error::invalid(e.to_string()))?
    .as_str()
    .to_owned();
    let pins = resources
        .into_iter()
        .map(|(path, sha256)| json!({"path":path,"sha256":sha256}))
        .collect();
    Ok(RenderInputs {
        digest,
        lock,
        fonts,
        pins,
    })
}

fn host_args(
    snapshot: &Snapshot,
    plan: &RenderPlan,
    generated_relative: &str,
    output_relative: &str,
    inputs: &RenderInputs,
) -> Result<(Vec<RuntimeToolArg>, Vec<u8>)> {
    security::relative_path(generated_relative)?;
    security::relative_path(output_relative)?;
    let config = json!({
        "authoring": snapshot.project.authoring.is_some(),
        "renderInputDigest": inputs.digest,
        "dependencyLockDigest": inputs.lock,
        "fontResourcesDigest": inputs.fonts,
        "fontResourcePins": inputs.pins,
        "name": "frames",
        "width": plan.width,
        "height": plan.height,
        "fps": f64::from(plan.fps) / f64::from(plan.fps_denominator),
        "fpsNum": plan.fps,
        "fpsDen": plan.fps_denominator,
        "firstFrame": plan.first_frame,
        "endFrameExclusive": plan.end_frame_exclusive,
        "colorSpace": match plan.color_space { ColorSpace::Srgb => "srgb", ColorSpace::DisplayP3 => "display-p3" },
        "background": snapshot.project.settings.background,
        "alpha": plan.alpha,
        "timeoutMs": plan.timeout_ms,
    });
    let program = format!(
        "globalThis.__SEMWRIGHT_RENDER_INPUT__ = {};\n{}",
        serde_json::to_string(&config)?,
        RENDER_HELPER
    )
    .into_bytes();
    if program.len() > 64 * 1024 {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Motion render program exceeds Host stdin budget",
        ));
    }
    let mut args = NODE_RENDER_FLAGS
        .into_iter()
        .map(literal)
        .collect::<Vec<_>>();
    args.extend([
        literal("--input-type=module"),
        literal("-"),
        literal("--project-root"),
        mount(PROJECT_MOUNT),
        literal("--project-relative"),
        literal(generated_relative),
        literal("--output-root"),
        mount(OUTPUT_MOUNT),
        literal("--output-relative"),
        literal(output_relative),
        literal("--fontconfig-root"),
        mount(FONTCONFIG_MOUNT),
        literal("--config"),
        literal(inputs.digest.clone()),
    ]);
    Ok((args, program))
}

fn host_timeout(plan: &RenderPlan) -> Duration {
    Duration::from_millis(plan.timeout_ms.saturating_add(30_000).min(3_600_000))
}

fn renderer_failure_class(stdout: &[u8]) -> Option<RenderFailureClass> {
    let text = std::str::from_utf8(stdout).ok()?;
    let line = text.lines().rev().find(|line| line.starts_with('{'))?;
    let value = serde_json::from_str::<serde_json::Value>(line).ok()?;
    if value.get("ok") != Some(&serde_json::Value::Bool(false)) {
        return None;
    }
    serde_json::from_value(value.get("errorClass")?.clone()).ok()
}

fn safe_renderer_detail(detail: &str) -> bool {
    if matches!(detail, "not_iterable" | "null_object" | "other") {
        return true;
    }
    ["read:", "set:", "not_function:"].iter().any(|prefix| {
        detail.strip_prefix(prefix).is_some_and(|name| {
            !name.is_empty()
                && name.len() <= 64
                && name.bytes().enumerate().all(|(index, byte)| {
                    byte == b'_'
                        || byte == b'$'
                        || byte.is_ascii_alphanumeric() && (index > 0 || !byte.is_ascii_digit())
                })
        })
    })
}

fn renderer_failure_detail(stdout: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(stdout).ok()?;
    let line = text.lines().rev().find(|line| line.starts_with('{'))?;
    let value = serde_json::from_str::<serde_json::Value>(line).ok()?;
    if value.get("ok") != Some(&serde_json::Value::Bool(false)) {
        return None;
    }
    let detail = value.get("detail")?.as_str()?;
    safe_renderer_detail(detail).then(|| detail.to_owned())
}

fn renderer_failure_code(stdout: &[u8]) -> ErrorCode {
    renderer_failure_class(stdout)
        .map(RenderFailureClass::code)
        .unwrap_or(ErrorCode::BackendFailed)
}

fn renderer_stderr_failure_class(stderr: &[u8]) -> Option<RenderFailureClass> {
    let text = std::str::from_utf8(stderr).ok()?;
    if text.contains("ERR_MODULE_NOT_FOUND")
        || text.contains("MODULE_NOT_FOUND")
        || text.contains("Cannot find package")
        || text.contains("Cannot find module")
        || text.contains("error while loading shared libraries")
    {
        return Some(RenderFailureClass::RuntimeModuleLoad);
    }
    if text.contains("SyntaxError") {
        return Some(RenderFailureClass::RuntimeSyntax);
    }
    if text.contains("EACCES")
        || text.contains("EPERM")
        || text.contains("Permission denied")
        || text.contains("Operation not permitted")
    {
        return Some(RenderFailureClass::RuntimePermission);
    }
    let lower = text.to_ascii_lowercase();
    if lower.contains("heap out of memory")
        || lower.contains("fatal process out of memory")
        || lower.contains("allocation failed")
    {
        return Some(RenderFailureClass::RuntimeOom);
    }
    None
}

#[cfg(test)]
fn renderer_status_failure_class(status: &std::process::ExitStatus) -> Option<RenderFailureClass> {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return Some(match signal {
                libc::SIGKILL => RenderFailureClass::RuntimeKilled,
                libc::SIGXCPU => RenderFailureClass::RuntimeCpuLimit,
                libc::SIGXFSZ => RenderFailureClass::RuntimeFileSizeLimit,
                _ => RenderFailureClass::RuntimeSignal,
            });
        }
    }
    match status.code() {
        Some(137) => Some(RenderFailureClass::RuntimeKilled),
        Some(152) => Some(RenderFailureClass::RuntimeCpuLimit),
        Some(153) => Some(RenderFailureClass::RuntimeFileSizeLimit),
        Some(134 | 135 | 136 | 139) => Some(RenderFailureClass::RuntimeSignal),
        _ => None,
    }
}

#[cfg(test)]
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

fn validate_host_result(output: &ToolExecutionOutput) -> Result<()> {
    let stdout = String::from_utf8(output.stdout.clone()).map_err(|_| {
        Error::new(
            ErrorCode::BackendFailed,
            "Renderer returned non-UTF8 output",
        )
    })?;
    let result_line = stdout.lines().rev().find(|line| line.starts_with('{'));
    let value = result_line
        .map(serde_json::from_str::<serde_json::Value>)
        .transpose()
        .map_err(|_| {
            Error::new(
                ErrorCode::BackendFailed,
                "Renderer returned malformed result",
            )
        })?;

    if output.exit_code != 0 {
        let detail = value
            .as_ref()
            .filter(|value| value.get("ok") == Some(&serde_json::Value::Bool(false)))
            .and_then(|value| value.get("error"))
            .and_then(serde_json::Value::as_str)
            .filter(|message| {
                !message.is_empty()
                    && message.len() <= 1024
                    && !message.chars().any(char::is_control)
            });
        let code = renderer_failure_code(&output.stdout);
        let safe_detail = renderer_failure_detail(&output.stdout);
        return Err(Error::new(
            code,
            match detail.or(safe_detail.as_deref()) {
                Some(detail) => format!(
                    "Motion Canvas renderer exited with code {}: {detail}",
                    output.exit_code
                ),
                None => format!(
                    "Motion Canvas renderer exited with code {}",
                    output.exit_code
                ),
            },
        ));
    }

    let value = value.ok_or_else(|| {
        Error::new(
            ErrorCode::BackendFailed,
            "Renderer returned no structured result",
        )
    })?;
    if value.get("ok") != Some(&serde_json::Value::Bool(true)) {
        return Err(Error::new(
            ErrorCode::BackendFailed,
            "Renderer did not report success",
        ));
    }
    Ok(())
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
mod tests {
    use super::*;

    #[test]
    fn host_result_accepts_success_and_preserves_bounded_structured_failure() {
        let success = ToolExecutionOutput {
            exit_code: 0,
            stdout: br#"{"ok":true}"#.to_vec(),
            stderr: vec![],
        };
        assert!(validate_host_result(&success).is_ok());

        let failure = ToolExecutionOutput {
            exit_code: 1,
            stdout: br#"{"ok":false,"error":"runtime bundle resolution failed"}"#.to_vec(),
            stderr: vec![],
        };
        let error = validate_host_result(&failure).unwrap_err();
        assert_eq!(error.code, ErrorCode::BackendFailed);
        assert!(error.message.contains("runtime bundle resolution failed"));

        let unstructured = ToolExecutionOutput {
            exit_code: 1,
            stdout: b"not-json".to_vec(),
            stderr: vec![],
        };
        let error = validate_host_result(&unstructured).unwrap_err();
        assert_eq!(error.message, "Motion Canvas renderer exited with code 1");
    }
}

#[cfg(test)]
mod runtime_path_tests {
    use super::*;

    #[test]
    fn production_node_heap_remains_bounded_but_supports_authoring_bundle() {
        assert_eq!(
            NODE_RENDER_FLAGS,
            ["--disable-wasm-trap-handler", "--max-old-space-size=512"]
        );
        let heap_mib = NODE_RENDER_FLAGS[1]
            .strip_prefix("--max-old-space-size=")
            .unwrap()
            .parse::<u64>()
            .unwrap();
        assert!((256..=512).contains(&heap_mib));
        assert!(
            heap_mib * 1024 * 1024 < 4_294_967_296,
            "V8 heap ceiling must remain strictly below Driver Host RLIMIT_AS"
        );
    }

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
            ErrorCode::Timeout
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
            ErrorCode::BackendFailed
        );
        assert_eq!(
            renderer_failure_code(br#"{"ok":false,"errorClass":"project_stage"}"#),
            ErrorCode::Unavailable
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
            renderer_failure_code(br#"{"ok":false,"errorClass":"renderer_log_webgl_unavailable"}"#),
            ErrorCode::Unavailable
        );
        assert_eq!(
            renderer_failure_code(br#"{"ok":false,"errorClass":"renderer_log_playback_protocol"}"#),
            ErrorCode::ProtocolMismatch
        );
        assert_eq!(
            renderer_failure_code(br#"{"ok":false,"errorClass":"renderer_log_invalid_scene"}"#),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            renderer_failure_code(
                br#"{"ok":false,"errorClass":"renderer_state_semwright_native"}"#
            ),
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            renderer_failure_code(
                br#"{"ok":false,"errorClass":"renderer_state_semwright_exporter"}"#
            ),
            ErrorCode::ProtocolMismatch
        );
        assert_eq!(
            renderer_failure_code(br#"{"ok":false,"errorClass":"renderer_state_motion_core"}"#),
            ErrorCode::Internal
        );
        assert_eq!(
            renderer_failure_code(br#"{"ok":false,"errorClass":"renderer_state_motion_2d"}"#),
            ErrorCode::BackendFailed
        );
        assert_eq!(
            renderer_failure_code(
                br#"{"ok":false,"errorClass":"renderer_state_before_first_frame"}"#
            ),
            ErrorCode::ProtocolMismatch
        );
        assert_eq!(
            renderer_failure_code(
                br#"{"ok":false,"errorClass":"renderer_state_after_first_frame"}"#
            ),
            ErrorCode::BackendFailed
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

    #[test]
    fn renderer_failure_detail_accepts_only_bounded_normalized_hints() {
        assert_eq!(
            renderer_failure_detail(
                br#"{"ok":false,"errorClass":"renderer_log_type_error","detail":"read:element"}"#
            ),
            Some("read:element".to_owned())
        );
        assert_eq!(
            renderer_failure_detail(
                br#"{"ok":false,"errorClass":"renderer_log_type_error","detail":"not_function:map"}"#
            ),
            Some("not_function:map".to_owned())
        );
        for hostile in [
            br#"{"ok":false,"detail":"../../secret"}"#.as_slice(),
            br#"{"ok":false,"detail":"read:/home/runner/private"}"#.as_slice(),
            br#"{"ok":false,"detail":"read:9starts_with_digit"}"#.as_slice(),
            br#"{"ok":true,"detail":"read:element"}"#.as_slice(),
        ] {
            assert_eq!(renderer_failure_detail(hostile), None, "{hostile:?}");
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
            ErrorCode::PluginProtocolError
        );
        assert_eq!(
            renderer_stderr_failure_class(b"Error [ERR_MODULE_NOT_FOUND]: hidden details"),
            Some(RenderFailureClass::RuntimeModuleLoad)
        );
        assert_eq!(
            renderer_stderr_failure_class(b"SyntaxError: hidden details"),
            Some(RenderFailureClass::RuntimeSyntax)
        );
        assert_eq!(
            renderer_stderr_failure_class(b"EACCES: hidden details"),
            Some(RenderFailureClass::RuntimePermission)
        );
        assert_eq!(
            renderer_stderr_failure_class(b"FATAL ERROR: JavaScript heap out of memory"),
            Some(RenderFailureClass::RuntimeOom)
        );
        assert_eq!(
            renderer_stderr_failure_class(b"secret arbitrary stderr"),
            None
        );
        assert_eq!(
            renderer_status_failure_class(&killed),
            Some(RenderFailureClass::RuntimeKilled)
        );
        assert_eq!(
            renderer_status_failure_class(&cpu),
            Some(RenderFailureClass::RuntimeCpuLimit)
        );
        let file_size = std::process::ExitStatus::from_raw(libc::SIGXFSZ);
        assert_eq!(
            renderer_status_failure_class(&file_size),
            Some(RenderFailureClass::RuntimeFileSizeLimit)
        );
    }

    #[tokio::test]
    async fn render_failure_code_stays_private_but_is_preserved_for_execute_context() {
        let manager = RenderManager::new(false, PathBuf::from("/tmp/not-used"));
        let job_ref = "job:test".to_owned();
        let public = JobView {
            job_ref: job_ref.clone(),
            state: RenderState::Failed,
            failure_class: Some(RenderFailureClass::RendererStateMotionCore),
            error: Some("Motion Canvas render failed".into()),
            artifact: None,
        };
        manager.jobs.lock().await.insert(
            job_ref.clone(),
            Job {
                source_sha256: "a".repeat(64),
                view: public.clone(),
                failure_code: Some(ErrorCode::ResourceExhausted),
                host_job: None,
                authoring: false,
                render_input_digest: "b".repeat(64),
                plan: serde_json::from_value(json!({"renderer":"motion-canvas-core-renderer-v3.17.2","project_duration_ms":34,"width":32,"height":32,"fps":30,"fps_denominator":1,"first_frame":0,"end_frame_exclusive":1,"frame_count":1,"alpha":false,"color_space":"srgb","timeout_ms":1000})).unwrap(),
                output: PathBuf::from("/tmp/not-used"),
            },
        );
        assert_eq!(
            manager.failure_code(&job_ref).await,
            Some(ErrorCode::ResourceExhausted)
        );
        let wire = serde_json::to_value(public).unwrap();
        assert_eq!(
            wire.get("failure_class")
                .and_then(serde_json::Value::as_str),
            Some("renderer_state_motion_core")
        );
        assert!(wire.get("failure_code").is_none());
    }
}
