//! Motion Canvas render jobs backed exclusively by Driver Host runtime-tool jobs.
use crate::{
    Error, ErrorCode, Result,
    model::{ColorSpace, RenderProfile},
    refs::{Kind, Reference},
    security,
    store::Snapshot,
    validate::RenderPlan,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use schemars::JsonSchema;
use semwright_driver_sdk::{
    DriverExecutionContext, RuntimeToolArg, RuntimeToolCwd, RuntimeToolJob, RuntimeToolJobStatus,
    ToolExecutionOutput,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
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
const NODE_RENDER_FLAGS: [&str; 2] = ["--disable-wasm-trap-handler", "--max-old-space-size=256"];

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

#[derive(Clone)]
struct Job {
    view: JobView,
    host_job: Option<RuntimeToolJob>,
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
                    plan: plan.clone(),
                    output: output.clone(),
                },
            );
        }

        let args = host_args(snapshot, &plan, &generated_relative, &id)?;
        let host_job = match context
            .start_runtime_tool_job_args(
                HOST_TOOL,
                args,
                RENDER_HELPER.as_bytes().to_vec(),
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
        let (plan, output, current) = {
            let guard = self.jobs.lock().await;
            let job = guard
                .get(job_ref)
                .ok_or_else(|| Error::new(ErrorCode::NotFound, "Unknown render job"))?;
            if !active(&job.view.state) {
                return Ok(job.view.clone());
            }
            (job.plan.clone(), job.output.clone(), job.view.clone())
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
                        validate_artifacts(&validation_output, &validation_plan)
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

fn host_args(
    snapshot: &Snapshot,
    plan: &RenderPlan,
    generated_relative: &str,
    output_relative: &str,
) -> Result<Vec<RuntimeToolArg>> {
    security::relative_path(generated_relative)?;
    security::relative_path(output_relative)?;
    let config = json!({
        "name": "frames",
        "width": plan.width,
        "height": plan.height,
        "fps": plan.fps,
        "firstFrame": plan.first_frame,
        "endFrameExclusive": plan.end_frame_exclusive,
        "colorSpace": match plan.color_space { ColorSpace::Srgb => "srgb", ColorSpace::DisplayP3 => "display-p3" },
        "background": snapshot.project.settings.background,
        "alpha": plan.alpha,
        "timeoutMs": plan.timeout_ms,
    });
    let encoded = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&config)?);
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
        literal(encoded),
    ]);
    Ok(args)
}

fn host_timeout(plan: &RenderPlan) -> Duration {
    Duration::from_millis(plan.timeout_ms.saturating_add(30_000).min(3_600_000))
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
        return Err(Error::new(
            ErrorCode::BackendFailed,
            match detail {
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
    let manifest_bytes = serde_json::to_vec_pretty(&json!({
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
    })
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
