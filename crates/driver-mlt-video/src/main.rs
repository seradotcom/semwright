use async_trait::async_trait;
use semwright_driver_sdk::{
    Capability, Driver, DriverExecutionContext, DriverInterfaces, RuntimeToolArg, RuntimeToolJob,
    RuntimeToolJobStatus, artifact_input_tag, artifact_output_tag, serve, workspace_mount,
};
use semwright_mlt_video::{
    app::{App, RenderRequest},
    catalog,
    fs::{PrivateDir, Root},
    hash::{random_id, sha256},
    jobs::{
        self, JobSnapshot, MAX_ACTIVE_JOBS, MAX_ARTIFACT_BYTES, MAX_JOB_INPUT_BYTES,
        MAX_MEDIA_BYTES, MAX_RETAINED_JOBS, State,
    },
    model::Profile,
    runtime::{
        MediaInfo, NATIVE_DIAGNOSTIC_FILE, NATIVE_DIAGNOSTIC_LIMIT, RenderProfile, ServiceCatalog,
        VALIDATION_DIAGNOSTIC_FILE, native_failure_filename, raw_video_probe_observation,
        validate_render_media,
    },
};
use semwright_types::{Error, ErrorCode, Result};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    io::{Read, Write},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

struct HostRenderJob {
    snapshot: JobSnapshot,
    host_job: RuntimeToolJob,
    scratch: PrivateDir,
    profile: RenderProfile,
    expected_profile: Profile,
    frames: u64,
    output_file: String,
}

struct HostRenderDiagnostic<'a> {
    job_id: &'a str,
    revision: &'a str,
    scratch_path: &'a std::path::Path,
    directory: &'a str,
}

#[derive(Default)]
struct HostRenderJobs {
    entries: BTreeMap<String, HostRenderJob>,
    order: VecDeque<String>,
}

impl HostRenderJobs {
    fn prepare_start(&mut self) -> Result<()> {
        if self
            .entries
            .values()
            .filter(|job| !job.snapshot.state.terminal())
            .count()
            >= MAX_ACTIVE_JOBS
        {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "At most two MLT render jobs may be active",
            ));
        }
        while self.entries.len() >= MAX_RETAINED_JOBS {
            let index = self
                .order
                .iter()
                .position(|id| {
                    self.entries
                        .get(id)
                        .is_some_and(|job| job.snapshot.state.terminal())
                })
                .ok_or_else(|| {
                    Error::new(
                        ErrorCode::ResourceExhausted,
                        "MLT render job retention is full",
                    )
                })?;
            if let Some(id) = self.order.remove(index) {
                self.entries.remove(&id);
            }
        }
        Ok(())
    }

    fn insert(&mut self, id: String, job: HostRenderJob) {
        self.order.push_back(id.clone());
        self.entries.insert(id, job);
    }
}

struct MltVideoDriver {
    app: App,
    host_catalog_verified: bool,
    host_render_jobs: HostRenderJobs,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as u64)
}

fn host_render_result(bytes: &[u8], expected_output: &str) -> Result<()> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| {
        Error::new(
            ErrorCode::PluginProtocolError,
            "MLT runtime runner returned invalid render JSON",
        )
    })?;
    let object = value.as_object().ok_or_else(|| {
        Error::new(
            ErrorCode::PluginProtocolError,
            "MLT runtime runner render result must be an object",
        )
    })?;
    if object.len() != 3
        || object.get("schema").and_then(Value::as_u64) != Some(1)
        || object.get("operation").and_then(Value::as_str) != Some("render")
        || object.get("output").and_then(Value::as_str) != Some(expected_output)
    {
        return Err(Error::new(
            ErrorCode::PluginProtocolError,
            "MLT runtime runner render envelope is invalid",
        ));
    }
    Ok(())
}

fn internal_job_error(
    code: &'static str,
    message: impl Into<String>,
) -> semwright_mlt_video::Error {
    semwright_mlt_video::Error::new(code, message)
}

fn to_internal_error(error: Error) -> semwright_mlt_video::Error {
    let code = match error.code {
        ErrorCode::Unsupported => "Unsupported",
        ErrorCode::Unavailable => "Unavailable",
        ErrorCode::PermissionDenied => "PermissionDenied",
        ErrorCode::ConsentRequired => "ConsentRequired",
        ErrorCode::PolicyDenied => "PolicyDenied",
        ErrorCode::NotFound => "NotFound",
        ErrorCode::AmbiguousTarget => "AmbiguousTarget",
        ErrorCode::StaleReference => "StaleReference",
        ErrorCode::Timeout => "Timeout",
        ErrorCode::InvalidArgument => "InvalidArgument",
        ErrorCode::SandboxDenied => "SandboxDenied",
        ErrorCode::Conflict => "Conflict",
        ErrorCode::Cancelled => "Cancelled",
        ErrorCode::ProtocolMismatch => "ProtocolMismatch",
        ErrorCode::ResourceExhausted => "ResourceExhausted",
        _ => "BackendFailed",
    };
    let mut mapped = semwright_mlt_video::Error::new(code, error.message);
    mapped.outcome_known = error.outcome_known;
    mapped
}

fn map_error(error: semwright_mlt_video::Error) -> Error {
    let code = match error.code {
        "Unsupported" => ErrorCode::Unsupported,
        "Unavailable" => ErrorCode::Unavailable,
        "PermissionDenied" => ErrorCode::PermissionDenied,
        "ConsentRequired" => ErrorCode::ConsentRequired,
        "PolicyDenied" => ErrorCode::PolicyDenied,
        "NotFound" => ErrorCode::NotFound,
        "AmbiguousTarget" => ErrorCode::AmbiguousTarget,
        "StaleReference" => ErrorCode::StaleReference,
        "Timeout" => ErrorCode::Timeout,
        "InvalidArgument" => ErrorCode::InvalidArgument,
        "SandboxDenied" => ErrorCode::SandboxDenied,
        "Conflict" => ErrorCode::Conflict,
        "Cancelled" => ErrorCode::Cancelled,
        "ProtocolMismatch" => ErrorCode::ProtocolMismatch,
        "ResourceExhausted" => ErrorCode::ResourceExhausted,
        _ => ErrorCode::BackendFailed,
    };
    let mut mapped = Error::new(code, error.message);
    mapped.outcome_known = error.outcome_known;
    mapped
}

fn from_internal(value: semwright_mlt_video::json::Value) -> Result<Value> {
    serde_json::from_str(&value.encode()).map_err(|_| {
        Error::new(
            ErrorCode::PluginProtocolError,
            "MLT driver produced invalid JSON",
        )
    })
}

fn to_internal(value: &Value) -> Result<semwright_mlt_video::json::Value> {
    let bytes = serde_json::to_vec(value)?;
    semwright_mlt_video::json::parse(&bytes).map_err(map_error)
}

fn sdk_capabilities() -> Result<Vec<Capability>> {
    catalog::capabilities()
        .map_err(map_error)?
        .into_iter()
        .map(|capability| {
            let mut capability: Capability =
                serde_json::from_str(&capability.wire).map_err(|_| {
                    Error::new(
                        ErrorCode::PluginProtocolError,
                        "MLT capability catalog is incompatible with the Driver SDK",
                    )
                })?;
            match capability.descriptor.name.as_str() {
                "driver.mlt-video.asset.import" => {
                    capability.tags.push(artifact_input_tag("video/clip")?);
                    capability.tags.push(artifact_input_tag("audio/sample")?);
                    capability.tags.push(artifact_input_tag("image/raster")?);
                }
                "driver.mlt-video.render.result" => {
                    capability.tags.push(artifact_output_tag("video/clip")?);
                }
                _ => {}
            }
            Ok(capability)
        })
        .collect()
}

fn host_runner_failure(bytes: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    let object = value.as_object()?;
    if object.len() != 3
        || object.get("schema").and_then(Value::as_u64) != Some(1)
        || object.get("operation").and_then(Value::as_str) != Some("error")
    {
        return None;
    }
    object
        .get("error")
        .and_then(Value::as_str)
        .filter(|message| {
            !message.is_empty() && message.len() <= 1024 && !message.chars().any(char::is_control)
        })
        .map(ToOwned::to_owned)
}

// The sealed AV helper emits bounded domain failures through the existing error
// envelope. Preserve stale-input classification; never infer completed effects
// from the helper's error text or accept a caller-supplied outcome flag.
fn host_av_operation_failure(bytes: &[u8]) -> Error {
    let message = host_runner_failure(bytes).unwrap_or_else(|| "Host AV operation failed".into());
    let code = match message.strip_prefix("StaleReference: ") {
        Some(detail) if !detail.trim().is_empty() => ErrorCode::StaleReference,
        _ => ErrorCode::BackendFailed,
    };
    let mut error = Error::new(code, message);
    error.outcome_known = false;
    error
}

fn host_media_info(bytes: &[u8]) -> Result<MediaInfo> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| {
        Error::new(
            ErrorCode::PluginProtocolError,
            "MLT runtime runner returned invalid probe JSON",
        )
    })?;
    let object = value.as_object().ok_or_else(|| {
        Error::new(
            ErrorCode::PluginProtocolError,
            "MLT runtime runner probe result must be an object",
        )
    })?;
    if object.len() != 3
        || object.get("schema").and_then(Value::as_u64) != Some(1)
        || object.get("operation").and_then(Value::as_str) != Some("probe")
    {
        return Err(Error::new(
            ErrorCode::PluginProtocolError,
            "MLT runtime runner probe envelope is invalid",
        ));
    }
    let media = object.get("media").ok_or_else(|| {
        Error::new(
            ErrorCode::PluginProtocolError,
            "MLT runtime runner probe omitted media",
        )
    })?;
    let encoded = serde_json::to_vec(media)?;
    MediaInfo::parse(&encoded).map_err(map_error)
}

fn host_catalog(bytes: &[u8]) -> Result<ServiceCatalog> {
    const GROUPS: [&str; 6] = [
        "producers",
        "filters",
        "transitions",
        "consumers",
        "video_codecs",
        "audio_codecs",
    ];
    let value: Value = serde_json::from_slice(bytes).map_err(|_| {
        Error::new(
            ErrorCode::PluginProtocolError,
            "MLT runtime runner returned invalid JSON",
        )
    })?;
    let object = value.as_object().ok_or_else(|| {
        Error::new(
            ErrorCode::PluginProtocolError,
            "MLT runtime runner result must be an object",
        )
    })?;
    if object.len() != 4
        || object.get("schema").and_then(Value::as_u64) != Some(1)
        || object.get("operation").and_then(Value::as_str) != Some("discover")
    {
        return Err(Error::new(
            ErrorCode::PluginProtocolError,
            "MLT runtime runner discovery envelope is invalid",
        ));
    }
    let version = object
        .get("version")
        .and_then(Value::as_str)
        .filter(|version| !version.is_empty() && version.len() <= 512)
        .ok_or_else(|| {
            Error::new(
                ErrorCode::PluginProtocolError,
                "MLT runtime runner version is invalid",
            )
        })?
        .to_owned();
    let raw_groups = object
        .get("groups")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            Error::new(
                ErrorCode::PluginProtocolError,
                "MLT runtime runner groups are invalid",
            )
        })?;
    if raw_groups.len() != GROUPS.len()
        || GROUPS.iter().any(|group| !raw_groups.contains_key(*group))
    {
        return Err(Error::new(
            ErrorCode::PluginProtocolError,
            "MLT runtime runner returned an unexpected service group set",
        ));
    }

    let mut groups = BTreeMap::new();
    for group in GROUPS {
        let values = raw_groups[group].as_array().ok_or_else(|| {
            Error::new(
                ErrorCode::PluginProtocolError,
                "MLT runtime runner service group is not an array",
            )
        })?;
        if values.len() > 2048 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "MLT runtime runner service group exceeds its bound",
            ));
        }
        let mut services = BTreeSet::new();
        for value in values {
            let service = value
                .as_str()
                .filter(|service| {
                    !service.is_empty()
                        && service.len() <= 128
                        && service.bytes().all(|byte| {
                            byte.is_ascii_alphanumeric()
                                || matches!(byte, b'_' | b'-' | b'.' | b':')
                        })
                })
                .ok_or_else(|| {
                    Error::new(
                        ErrorCode::PluginProtocolError,
                        "MLT runtime runner service token is invalid",
                    )
                })?;
            if !services.insert(service.to_owned()) {
                return Err(Error::new(
                    ErrorCode::PluginProtocolError,
                    "MLT runtime runner returned a duplicate service",
                ));
            }
        }
        groups.insert(group.to_owned(), services);
    }
    Ok(ServiceCatalog { version, groups })
}

impl MltVideoDriver {
    async fn host_av_operation(
        &self,
        command: &str,
        descriptor: &str,
        args: &Value,
        context: &DriverExecutionContext,
    ) -> Result<Value> {
        let literal = |value: &str| RuntimeToolArg::Literal {
            value: value.to_owned(),
        };
        let mount = |name: &str| RuntimeToolArg::MountPath {
            mount: name.to_owned(),
            relative: String::new(),
        };
        let tool = |name: &str| RuntimeToolArg::ToolPath {
            tool: name.to_owned(),
        };
        let job = context
            .start_runtime_tool_job_args(
                "mlt-runner",
                vec![
                    literal("av-operation"),
                    literal("--runtime-root"),
                    mount("mlt-runtime"),
                    literal("--melt-sealed"),
                    tool("melt"),
                    literal("--ffprobe-sealed"),
                    tool("ffprobe"),
                    literal("--ffmpeg-sealed"),
                    tool("ffmpeg"),
                    literal("--project-root"),
                    mount("project"),
                    literal("--media-root"),
                    mount("media"),
                    literal("--output-root"),
                    mount("output"),
                ],
                serde_json::to_vec(&serde_json::json!({
                    "command": command, "descriptor_sha256": descriptor, "args": args,
                }))?,
                Duration::from_secs(300),
                None,
            )
            .await?;
        let result = context.wait_runtime_tool_job(&job).await?;
        if result.exit_code != 0 {
            let mut error = host_av_operation_failure(&result.stdout);
            // The tool may have published before an error or cancellation. The
            // original artifact is reconciled by its digest; never assume rollback.
            error.outcome_known = command == "driver.mlt-video.sync.probe";
            return Err(error);
        }
        let value: Value = serde_json::from_slice(&result.stdout).map_err(|_| {
            Error::new(
                ErrorCode::PluginProtocolError,
                "Host AV tool returned malformed JSON",
            )
        })?;
        self.app
            .validate_output(command, &to_internal(&value)?)
            .map_err(map_error)?;
        Ok(value)
    }

    async fn host_probe_staged(
        &self,
        directory: &str,
        name: &str,
        context: &DriverExecutionContext,
    ) -> Result<MediaInfo> {
        self.host_probe_staged_observed(directory, name, context)
            .await
            .map(|(media, _)| media)
    }

    async fn host_probe_staged_observed(
        &self,
        directory: &str,
        name: &str,
        context: &DriverExecutionContext,
    ) -> Result<(MediaInfo, Vec<u8>)> {
        let output = context
            .execute_runtime_tool_args(
                "mlt-runner",
                vec![
                    RuntimeToolArg::Literal {
                        value: "probe".into(),
                    },
                    RuntimeToolArg::Literal {
                        value: "--runtime-root".into(),
                    },
                    RuntimeToolArg::MountPath {
                        mount: "mlt-runtime".into(),
                        relative: String::new(),
                    },
                    RuntimeToolArg::Literal {
                        value: "--ffprobe-sealed".into(),
                    },
                    RuntimeToolArg::ToolPath {
                        tool: "ffprobe".into(),
                    },
                    RuntimeToolArg::Literal {
                        value: "--scratch-root".into(),
                    },
                    RuntimeToolArg::MountPath {
                        mount: "scratch".into(),
                        relative: String::new(),
                    },
                    RuntimeToolArg::Literal {
                        value: "--directory".into(),
                    },
                    RuntimeToolArg::Literal {
                        value: directory.to_owned(),
                    },
                    RuntimeToolArg::Literal {
                        value: "--name".into(),
                    },
                    RuntimeToolArg::Literal {
                        value: name.to_owned(),
                    },
                ],
                Vec::new(),
                std::time::Duration::from_secs(30),
                None,
            )
            .await?;
        if output.exit_code != 0 {
            let detail = host_runner_failure(&output.stdout);
            return Err(Error::new(
                ErrorCode::BackendFailed,
                match detail {
                    Some(detail) => format!("Host-mediated MLT probe failed: {detail}"),
                    None => "Host-mediated MLT probe failed".into(),
                },
            ));
        }
        let media = host_media_info(&output.stdout)?;
        Ok((media, output.stdout))
    }

    async fn host_probe_for(
        &self,
        command: &str,
        args: &Value,
        context: &DriverExecutionContext,
    ) -> Result<Option<MediaInfo>> {
        let needs_probe = match command {
            "driver.mlt-video.asset.import" => {
                args.get("kind").and_then(Value::as_str) != Some("color")
            }
            "driver.mlt-video.asset.relink" => true,
            _ => false,
        };
        if !needs_probe {
            return Ok(None);
        }

        let root_name = args
            .get("root")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new(ErrorCode::InvalidArgument, "Media root is required"))?;
        let path = args
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new(ErrorCode::InvalidArgument, "Media path is required"))?;
        if !matches!(root_name, "project" | "media" | "output") {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "Media root is not granted for probing",
            ));
        }
        let root = self.app.roots.get(root_name).ok_or_else(|| {
            Error::new(
                ErrorCode::PermissionDenied,
                "Media root is not mounted for probing",
            )
        })?;
        let mut source = root.read_file(path, MAX_MEDIA_BYTES).map_err(map_error)?;

        let scratch_root = workspace_mount("scratch")?;
        if !scratch_root.is_dir() {
            return Err(Error::new(
                ErrorCode::Unavailable,
                "MLT scratch mount is unavailable",
            ));
        }
        let scratch = PrivateDir::new(&scratch_root).map_err(map_error)?;
        let directory = scratch
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty() && value.len() <= 128)
            .ok_or_else(|| {
                Error::new(ErrorCode::Internal, "MLT scratch directory name is invalid")
            })?
            .to_owned();
        let mut destination = scratch.create("probe.bin").map_err(map_error)?;
        let copied = std::io::copy(
            &mut (&mut source).take(MAX_MEDIA_BYTES.saturating_add(1)),
            &mut destination,
        )
        .map_err(|_| {
            Error::new(
                ErrorCode::BackendFailed,
                "Failed to stage bounded MLT probe input",
            )
        })?;
        if copied > MAX_MEDIA_BYTES {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "MLT probe input exceeds media byte budget",
            ));
        }
        destination.flush().map_err(|_| {
            Error::new(
                ErrorCode::BackendFailed,
                "Failed to flush bounded MLT probe input",
            )
        })?;
        destination.sync_all().map_err(|_| {
            Error::new(
                ErrorCode::BackendFailed,
                "Failed to sync bounded MLT probe input",
            )
        })?;
        drop(destination);
        scratch.seal("probe.bin").map_err(map_error)?;

        self.host_probe_staged(&directory, "probe.bin", context)
            .await
            .map(Some)
    }

    async fn start_host_render(
        &mut self,
        request: RenderRequest,
        context: &DriverExecutionContext,
    ) -> Result<JobSnapshot> {
        self.host_render_jobs.prepare_start()?;
        let scratch_root = workspace_mount("scratch")?;
        if !scratch_root.is_dir() {
            return Err(Error::new(
                ErrorCode::Unavailable,
                "MLT scratch mount is unavailable",
            ));
        }
        let scratch = PrivateDir::new(&scratch_root).map_err(map_error)?;
        let directory = scratch
            .path()
            .file_name()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty() && value.len() <= 128)
            .ok_or_else(|| Error::new(ErrorCode::Internal, "MLT scratch directory is invalid"))?
            .to_owned();

        let sequence = request
            .project
            .sequence(&request.sequence)
            .map_err(map_error)?
            .clone();
        let (asset_ids, _) = jobs::required(&request.project, &sequence).map_err(map_error)?;
        let mut staged = BTreeMap::new();
        let mut total = 0u64;
        for asset_id in asset_ids {
            context.check_cancelled()?;
            let asset =
                request.project.assets.get(&asset_id).ok_or_else(|| {
                    Error::new(ErrorCode::InvalidArgument, "Render asset is missing")
                })?;
            if matches!(
                asset.resource,
                semwright_mlt_video::model::Resource::Color(_)
            ) {
                continue;
            }
            let (root_name, path) =
                jobs::media_location(&request.project, asset).map_err(map_error)?;
            let root =
                self.app.roots.get(&root_name).ok_or_else(|| {
                    Error::new(ErrorCode::PermissionDenied, "Media mount is absent")
                })?;
            let mut source = root.read_file(&path, MAX_MEDIA_BYTES).map_err(map_error)?;
            let name = format!("asset-{}.bin", &sha256(asset_id.as_bytes())[..24]);
            let mut destination = scratch.create(&name).map_err(map_error)?;
            let copied = std::io::copy(
                &mut (&mut source).take(MAX_MEDIA_BYTES.saturating_add(1)),
                &mut destination,
            )
            .map_err(|_| {
                Error::new(
                    ErrorCode::BackendFailed,
                    "Failed to stage bounded MLT render input",
                )
            })?;
            total = total.saturating_add(copied);
            if copied > MAX_MEDIA_BYTES || total > MAX_JOB_INPUT_BYTES {
                return Err(Error::new(
                    ErrorCode::ResourceExhausted,
                    "Staged MLT render inputs exceed their byte budget",
                ));
            }
            destination.flush().map_err(|_| {
                Error::new(
                    ErrorCode::BackendFailed,
                    "Failed to flush bounded MLT render input",
                )
            })?;
            destination.sync_all().map_err(|_| {
                Error::new(
                    ErrorCode::BackendFailed,
                    "Failed to sync bounded MLT render input",
                )
            })?;
            drop(destination);
            scratch.seal(&name).map_err(map_error)?;
            let info = self.host_probe_staged(&directory, &name, context).await?;
            jobs::validate_staged_asset(&request.project, &sequence, &asset_id, &info)
                .map_err(map_error)?;
            staged.insert(asset_id, name);
        }

        let prepared = jobs::prepare_render_document(
            request.project,
            &request.sequence,
            &request.profile,
            &staged,
        )
        .map_err(map_error)?;
        let mut project_file = scratch.create("project.mlt").map_err(map_error)?;
        project_file
            .write_all(prepared.xml.as_bytes())
            .map_err(|_| Error::new(ErrorCode::BackendFailed, "Failed to stage project.mlt"))?;
        project_file
            .sync_all()
            .map_err(|_| Error::new(ErrorCode::BackendFailed, "Failed to sync project.mlt"))?;
        drop(project_file);
        scratch.seal("project.mlt").map_err(map_error)?;

        let output_file = format!("partial.{}", request.profile.extension);
        let host_job = context
            .start_runtime_tool_job_args(
                "mlt-runner",
                vec![
                    RuntimeToolArg::Literal {
                        value: "render".into(),
                    },
                    RuntimeToolArg::Literal {
                        value: "--runtime-root".into(),
                    },
                    RuntimeToolArg::MountPath {
                        mount: "mlt-runtime".into(),
                        relative: String::new(),
                    },
                    RuntimeToolArg::Literal {
                        value: "--melt-sealed".into(),
                    },
                    RuntimeToolArg::ToolPath {
                        tool: "melt".into(),
                    },
                    RuntimeToolArg::Literal {
                        value: "--scratch-root".into(),
                    },
                    RuntimeToolArg::MountPath {
                        mount: "scratch".into(),
                        relative: String::new(),
                    },
                    RuntimeToolArg::Literal {
                        value: "--directory".into(),
                    },
                    RuntimeToolArg::Literal {
                        value: directory.clone(),
                    },
                    RuntimeToolArg::Literal {
                        value: "--profile".into(),
                    },
                    RuntimeToolArg::Literal {
                        value: request.profile.id.into(),
                    },
                ],
                Vec::new(),
                // The Host derives the inherited CPU ceiling from this tool
                // deadline. A two-thread 1080p render can consume 150 CPU
                // seconds before its independent native wall limit.
                // Match the declared 300-CPU-second driver ceiling; the native
                // runner retains its independent 180-second wall deadline.
                Duration::from_secs(300),
                None,
            )
            .await?;

        let id = format!("render:{}", random_id().map_err(map_error)?);
        let now = now_ms();
        let snapshot = JobSnapshot {
            id: id.clone(),
            revision: request.revision,
            profile: request.profile.id.into(),
            output: request.output,
            state: State::Starting,
            created_at: now,
            started_at: Some(now),
            completed_at: None,
            artifact: None,
            media: None,
            error: None,
            cancellation_requested: false,
        };
        self.host_render_jobs.insert(
            id,
            HostRenderJob {
                snapshot: snapshot.clone(),
                host_job,
                scratch,
                profile: request.profile,
                expected_profile: prepared.expected_profile,
                frames: prepared.frames,
                output_file,
            },
        );
        Ok(snapshot)
    }

    async fn finalize_host_render(
        &self,
        diagnostic: &HostRenderDiagnostic<'_>,
        output_file: &str,
        output_path: &str,
        profile: &RenderProfile,
        expected_profile: &Profile,
        frames: u64,
        context: &DriverExecutionContext,
    ) -> Result<(semwright_mlt_video::fs::Artifact, MediaInfo)> {
        let scratch_path = diagnostic.scratch_path;
        let (media, raw_probe) = self
            .host_probe_staged_observed(diagnostic.directory, output_file, context)
            .await?;
        if let Err(error) = validate_render_media(&media, profile, expected_profile, frames) {
            // Evidence is an additional bounded JSON sibling inside the existing output grant.
            // Failure to retain it must not erase or soften the original validation failure.
            let _ = self.persist_native_validation_failure(
                diagnostic,
                output_path,
                profile,
                expected_profile,
                frames,
                &media,
                &raw_probe,
                &error,
            );
            return Err(map_error(error));
        }
        let source_root = Root::open(scratch_path, true, false).map_err(map_error)?;
        let source = source_root
            .read_file(output_file, MAX_ARTIFACT_BYTES)
            .map_err(map_error)?;
        let output_root = self
            .app
            .roots
            .get("output")
            .ok_or_else(|| Error::new(ErrorCode::Unavailable, "Output mount disappeared"))?;
        let artifact = output_root
            .publish("output", output_path, source, MAX_ARTIFACT_BYTES)
            .map_err(map_error)?;
        Ok((artifact, media))
    }

    fn persist_native_validation_failure(
        &self,
        diagnostic: &HostRenderDiagnostic<'_>,
        output_path: &str,
        profile: &RenderProfile,
        expected_profile: &Profile,
        frames: u64,
        media: &MediaInfo,
        raw_probe: &[u8],
        error: &semwright_mlt_video::Error,
    ) -> Result<()> {
        let job_id = diagnostic.job_id;
        let revision = diagnostic.revision;
        let scratch_path = diagnostic.scratch_path;
        let directory = diagnostic.directory;
        let diagnostic_path = native_failure_filename(output_path, job_id).map_err(map_error)?;
        let source = Root::open(scratch_path, true, true).map_err(map_error)?;
        let xml = source
            .read("project.mlt", semwright_mlt_video::xml::MAX_XML)
            .map_err(map_error)?;
        let native_bytes = source
            .read(NATIVE_DIAGNOSTIC_FILE, NATIVE_DIAGNOSTIC_LIMIT)
            .map_err(map_error)?;
        let native: Value = serde_json::from_slice(&native_bytes).map_err(|_| {
            Error::new(
                ErrorCode::PluginProtocolError,
                "Native diagnostic JSON differs",
            )
        })?;
        let xml_sha = sha256(&xml);
        if native.get("schema").and_then(Value::as_u64) != Some(1)
            || native.get("operation").and_then(Value::as_str)
                != Some("mlt-native-render-observation")
            || native.get("project_xml_sha256").and_then(Value::as_str) != Some(xml_sha.as_str())
        {
            return Err(Error::new(
                ErrorCode::PluginProtocolError,
                "Native diagnostic staged XML binding differs",
            ));
        }
        let value = serde_json::json!({
            "schema":1,"operation":"mlt-native-validation-failure","job_id":job_id,
            "revision":revision,"directory":directory,"output":output_path,"profile":profile.id,
            "expected_frames":frames,"expected_profile":from_internal(expected_profile.json())?,
            "observed_media":from_internal(media.json())?,
            "raw_probe":raw_video_probe_observation(raw_probe).map_err(map_error)?,
            "staged_xml":{"sha256":xml_sha,"bytes":xml.len()},
            "native_render_receipt_sha256":sha256(&native_bytes),"native_render":native,
            "validation_error":{"code":error.code,"message":error.message,"outcome_known":error.outcome_known},
            "video_artifact_published":false,
            "scope":"local bounded forensic evidence; raw logs never enter the public JobSnapshot or audit"
        });
        let bytes = serde_json::to_vec(&value)?;
        if bytes.len() > NATIVE_DIAGNOSTIC_LIMIT {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Native failure diagnostic exceeds bounds",
            ));
        }
        source
            .write_new("scratch", VALIDATION_DIAGNOSTIC_FILE, &bytes)
            .map_err(map_error)?;
        self.app
            .roots
            .get("output")
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::Unavailable,
                    "Diagnostic output mount disappeared",
                )
            })?
            .write_new("output", &diagnostic_path, &bytes)
            .map_err(map_error)?;
        Ok(())
    }

    async fn update_host_render(
        &mut self,
        job_id: &str,
        context: &DriverExecutionContext,
        cancel: bool,
    ) -> Result<JobSnapshot> {
        let (
            host_job,
            current,
            scratch_path,
            directory,
            profile,
            expected_profile,
            frames,
            output_file,
        ) = {
            let job = self.host_render_jobs.entries.get(job_id).ok_or_else(|| {
                Error::new(
                    ErrorCode::StaleReference,
                    "Render job belongs to another driver lifetime or was evicted",
                )
            })?;
            if job.snapshot.state.terminal() {
                return Ok(job.snapshot.clone());
            }
            let directory = job
                .scratch
                .path()
                .file_name()
                .and_then(|value| value.to_str())
                .ok_or_else(|| Error::new(ErrorCode::Internal, "MLT scratch job is invalid"))?
                .to_owned();
            (
                job.host_job.clone(),
                job.snapshot.clone(),
                job.scratch.path().to_path_buf(),
                directory,
                job.profile.clone(),
                job.expected_profile.clone(),
                job.frames,
                job.output_file.clone(),
            )
        };

        let status = if cancel {
            context.cancel_runtime_tool_job(&host_job).await?
        } else {
            context.runtime_tool_job_status(&host_job).await?
        };
        let mut next = current;
        if cancel {
            next.cancellation_requested = true;
        }
        match status {
            RuntimeToolJobStatus::Running => {
                next.state = State::Running;
            }
            RuntimeToolJobStatus::Cancelling => {
                next.state = State::Running;
                next.cancellation_requested = true;
            }
            RuntimeToolJobStatus::Cancelled => {
                next.state = State::Cancelled;
                next.completed_at = Some(now_ms());
                next.error = Some(internal_job_error("Cancelled", "Render cancelled"));
            }
            RuntimeToolJobStatus::Failed { error } => {
                next.state = if error.outcome_known {
                    State::Failed
                } else {
                    State::Unknown
                };
                next.completed_at = Some(now_ms());
                next.error = Some(to_internal_error(error));
            }
            RuntimeToolJobStatus::Succeeded { output } => {
                let terminal = if output.exit_code != 0 {
                    let detail = host_runner_failure(&output.stdout);
                    Err(Error::new(
                        ErrorCode::BackendFailed,
                        match detail {
                            Some(detail) => format!("Host-mediated MLT render failed: {detail}"),
                            None => format!(
                                "Host-mediated MLT render exited with code {}",
                                output.exit_code
                            ),
                        },
                    ))
                } else {
                    host_render_result(&output.stdout, &output_file)
                };
                let finalized = match terminal {
                    Ok(()) => {
                        self.finalize_host_render(
                            &HostRenderDiagnostic {
                                job_id: &next.id,
                                revision: &next.revision,
                                scratch_path: &scratch_path,
                                directory: &directory,
                            },
                            &output_file,
                            &next.output,
                            &profile,
                            &expected_profile,
                            frames,
                            context,
                        )
                        .await
                    }
                    Err(error) => Err(error),
                };
                match finalized {
                    Ok((artifact, media)) => {
                        next.state = State::Succeeded;
                        next.completed_at = Some(now_ms());
                        next.artifact = Some(artifact);
                        next.media = Some(media);
                        next.error = None;
                    }
                    Err(error) => {
                        let internal = to_internal_error(error);
                        next.state = if internal.outcome_known {
                            State::Failed
                        } else {
                            State::Unknown
                        };
                        next.completed_at = Some(now_ms());
                        next.error = Some(internal);
                    }
                }
            }
        }

        let job = self
            .host_render_jobs
            .entries
            .get_mut(job_id)
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::StaleReference,
                    "Render job disappeared during status update",
                )
            })?;
        if !job.snapshot.state.terminal() {
            job.snapshot = next;
        }
        Ok(job.snapshot.clone())
    }

    async fn ensure_host_catalog(&mut self, context: &DriverExecutionContext) -> Result<()> {
        if self.host_catalog_verified || std::env::var_os("SEMWRIGHT_DRIVER_HOST_TOOLS").is_none() {
            return Ok(());
        }
        let legacy_catalog = self
            .app
            .runtime
            .as_ref()
            .map(|runtime| runtime.catalog.clone());
        let output = context
            .execute_runtime_tool_args(
                "mlt-runner",
                vec![
                    RuntimeToolArg::Literal {
                        value: "discover".into(),
                    },
                    RuntimeToolArg::Literal {
                        value: "--runtime-root".into(),
                    },
                    RuntimeToolArg::MountPath {
                        mount: "mlt-runtime".into(),
                        relative: String::new(),
                    },
                    RuntimeToolArg::Literal {
                        value: "--melt-sealed".into(),
                    },
                    RuntimeToolArg::ToolPath {
                        tool: "melt".into(),
                    },
                ],
                Vec::new(),
                std::time::Duration::from_secs(30),
                None,
            )
            .await?;
        if output.exit_code != 0 {
            let detail = host_runner_failure(&output.stdout);
            return Err(Error::new(
                ErrorCode::BackendFailed,
                match detail {
                    Some(detail) => format!("Host-mediated MLT discovery runner failed: {detail}"),
                    None => "Host-mediated MLT discovery runner failed".into(),
                },
            ));
        }
        let observed = host_catalog(&output.stdout)?;
        if let Some(legacy) = legacy_catalog
            && (observed.version != legacy.version || observed.groups != legacy.groups)
        {
            return Err(Error::new(
                ErrorCode::BackendFailed,
                "Host-mediated and legacy MLT discovery catalogs diverged",
            ));
        }
        self.app.catalog = Some(observed);
        self.app.runtime_reason = if self.app.runtime.is_some() {
            "Host-mediated MLT catalog verified against the transitional legacy runtime".into()
        } else {
            "Host-mediated MLT catalog verified from owner-pinned runtime tools".into()
        };
        self.host_catalog_verified = true;
        Ok(())
    }
}

#[async_trait]
impl Driver for MltVideoDriver {
    fn id(&self) -> &str {
        "mlt-video"
    }

    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    fn interfaces(&self) -> DriverInterfaces {
        DriverInterfaces {
            health: true,
            host_tools: std::env::var_os("SEMWRIGHT_DRIVER_HOST_TOOLS").is_some(),
            ..DriverInterfaces::default()
        }
    }

    async fn capabilities(&mut self) -> Result<Vec<Capability>> {
        sdk_capabilities()
    }

    async fn execute(
        &mut self,
        command: &str,
        descriptor_sha256: &str,
        args: Value,
    ) -> Result<Value> {
        let value = self
            .app
            .execute(command, descriptor_sha256, to_internal(&args)?)
            .map_err(map_error)?;
        from_internal(value)
    }

    async fn execute_with_context(
        &mut self,
        command: &str,
        descriptor_sha256: &str,
        args: Value,
        context: DriverExecutionContext,
    ) -> Result<Value> {
        context.check_cancelled()?;
        let internal = to_internal(&args)?;
        self.app
            .validate_call(command, descriptor_sha256, &internal)
            .map_err(map_error)?;
        self.ensure_host_catalog(&context).await?;
        if matches!(
            command,
            "driver.mlt-video.frames.encode"
                | "driver.mlt-video.av.mux"
                | "driver.mlt-video.sync.probe"
        ) {
            return self
                .host_av_operation(command, descriptor_sha256, &args, &context)
                .await;
        }
        let value = match command {
            "driver.mlt-video.render.start" => {
                let request = self
                    .app
                    .prepare_render_request(&internal)
                    .map_err(map_error)?;
                self.start_host_render(request, &context).await?.json()
            }
            "driver.mlt-video.render.status"
            | "driver.mlt-video.render.cancel"
            | "driver.mlt-video.render.result" => {
                let job_id = args.get("job").and_then(Value::as_str).ok_or_else(|| {
                    Error::new(ErrorCode::InvalidArgument, "Render job is required")
                })?;
                let snapshot = self
                    .update_host_render(
                        job_id,
                        &context,
                        command == "driver.mlt-video.render.cancel",
                    )
                    .await?;
                if command == "driver.mlt-video.render.result" && snapshot.state != State::Succeeded
                {
                    return Err(Error::new(
                        ErrorCode::Unavailable,
                        "No validated successful artifact is available",
                    ));
                }
                snapshot.json()
            }
            _ => {
                let probe = self.host_probe_for(command, &args, &context).await?;
                return from_internal(
                    self.app
                        .execute_with_probe(command, descriptor_sha256, internal, probe)
                        .map_err(map_error)?,
                );
            }
        };
        self.app
            .validate_output(command, &value)
            .map_err(map_error)?;
        from_internal(value)
    }

    async fn health(&mut self) -> Result<Value> {
        from_internal(self.app.doctor())
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let result = App::production()
        .map(|app| MltVideoDriver {
            app,
            host_catalog_verified: false,
            host_render_jobs: HostRenderJobs::default(),
        })
        .map_err(map_error);
    let result = match result {
        Ok(driver) => serve(driver).await,
        Err(error) => Err(error),
    };
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(error.exit_code());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_runner_failure_is_strict_and_bounded() {
        let valid = serde_json::json!({
            "schema": 1,
            "operation": "error",
            "error": "BackendFailed: melt -version discovery failed"
        });
        assert_eq!(
            host_runner_failure(&serde_json::to_vec(&valid).unwrap()).as_deref(),
            Some("BackendFailed: melt -version discovery failed")
        );

        let mut extra = valid.clone();
        extra["extra"] = serde_json::json!(true);
        assert!(host_runner_failure(&serde_json::to_vec(&extra).unwrap()).is_none());

        let control = serde_json::json!({
            "schema": 1,
            "operation": "error",
            "error": "bad\nmessage"
        });
        assert!(host_runner_failure(&serde_json::to_vec(&control).unwrap()).is_none());
    }

    #[test]
    fn host_av_failure_preserves_stale_without_promoting_effect_outcome() {
        let envelope = |message: &str| {
            serde_json::to_vec(&serde_json::json!({
                "schema": 1, "operation": "error", "error": message
            }))
            .unwrap()
        };
        let stale = host_av_operation_failure(&envelope(
            "StaleReference: Reference or expected revision is stale; observe again",
        ));
        assert_eq!(stale.code, ErrorCode::StaleReference);
        assert!(!stale.outcome_known);
        for message in [
            "BackendFailed: native tool failed",
            "Conflict: StaleReference: embedded text",
            "StaleReference: ",
            "StaleReference: bad\nmessage",
        ] {
            let error = host_av_operation_failure(&envelope(message));
            assert_eq!(error.code, ErrorCode::BackendFailed);
            assert!(!error.outcome_known);
        }
        let extra = serde_json::to_vec(&serde_json::json!({
            "schema": 1, "operation": "error", "error": "StaleReference: stale",
            "outcome_known": true
        }))
        .unwrap();
        let rejected = host_av_operation_failure(&extra);
        assert_eq!(rejected.code, ErrorCode::BackendFailed);
        assert!(!rejected.outcome_known);
    }

    #[test]
    fn host_catalog_parser_is_strict_and_bounded() {
        let valid = serde_json::json!({
            "schema": 1,
            "operation": "discover",
            "version": "melt 7.32.0",
            "groups": {
                "producers": ["avformat"],
                "filters": ["volume"],
                "transitions": ["mix"],
                "consumers": ["avformat"],
                "video_codecs": ["libx264"],
                "audio_codecs": ["aac"]
            }
        });
        let parsed = host_catalog(&serde_json::to_vec(&valid).unwrap()).unwrap();
        assert_eq!(parsed.version, "melt 7.32.0");
        assert!(parsed.has("filters", "volume"));

        let mut missing = valid.clone();
        missing["groups"].as_object_mut().unwrap().remove("filters");
        assert!(host_catalog(&serde_json::to_vec(&missing).unwrap()).is_err());

        let mut duplicate = valid;
        duplicate["groups"]["filters"] = serde_json::json!(["volume", "volume"]);
        assert!(host_catalog(&serde_json::to_vec(&duplicate).unwrap()).is_err());
    }

    #[test]
    fn sdk_catalog_declares_generic_artifact_ports() {
        let caps = sdk_capabilities().unwrap();
        let import = caps
            .iter()
            .find(|cap| cap.descriptor.name == "driver.mlt-video.asset.import")
            .unwrap();
        assert!(
            import
                .tags
                .iter()
                .any(|tag| tag == "artifact-in:video/clip")
        );
        assert!(
            import
                .tags
                .iter()
                .any(|tag| tag == "artifact-in:audio/sample")
        );
        assert!(
            import
                .tags
                .iter()
                .any(|tag| tag == "artifact-in:image/raster")
        );

        let render = caps
            .iter()
            .find(|cap| cap.descriptor.name == "driver.mlt-video.render.result")
            .unwrap();
        assert!(
            render
                .tags
                .iter()
                .any(|tag| tag == "artifact-out:video/clip")
        );
    }
}
