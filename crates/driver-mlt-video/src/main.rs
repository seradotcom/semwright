use async_trait::async_trait;
use semwright_driver_sdk::{
    Capability, Driver, DriverExecutionContext, DriverInterfaces, RuntimeToolArg, RuntimeToolJob,
    RuntimeToolJobStatus, artifact_input_tag, artifact_output_tag, serve, workspace_mount,
};
use semwright_mlt_video::{
    app::{App, RenderRequest},
    catalog, color,
    fs::{Artifact, PrivateDir, Root},
    hash::{random_id, reader_hash, sha256},
    jobs::{
        self, JobSnapshot, MAX_ACTIVE_JOBS, MAX_ARTIFACT_BYTES, MAX_JOB_INPUT_BYTES,
        MAX_MEDIA_BYTES, MAX_RETAINED_JOBS, RenderedOutput, State,
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
    observation_request: Option<Vec<u8>>,
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

// Original results from this Driver's existing Host calls. Raw data remains in
// the already-granted private output Root and never enters public errors.
struct AvHostCustody {
    scope: AvHostScope,
    output: std::sync::Arc<Root>,
    output_path: String,
    request: Vec<u8>,
}

fn finish_ffv1_host_original<T>(original: Result<T>, custody: Result<()>) -> Result<T> {
    match original {
        Err(error) => Err(error),
        Ok(value) => custody.map(|()| value).map_err(Error::uncertain),
    }
}

impl AvHostCustody {
    fn new(app: &App, command: &str, descriptor: &str, args: &Value) -> Result<Self> {
        use semwright_mlt_video::runtime as rt;
        if command != "driver.mlt-video.frames.encode" {
            return Err(Error::invalid("FFV1 custody is closed to frames.encode"));
        }
        let output_path = args
            .get("output_path")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::invalid("FFV1 original output is absent"))?
            .to_owned();
        let output =
            app.roots.get("output").cloned().ok_or_else(|| {
                Error::new(ErrorCode::Unavailable, "Original output grant is absent")
            })?;
        rt::preflight_ffv1_custody(&output, &output_path, true).map_err(map_error)?;
        // This one serialized Vec is retained now and cloned unchanged into the
        // existing start_runtime_tool_job_args call. It is not reconstructed.
        let request = serde_json::to_vec(&serde_json::json!({
            "command":command,"descriptor_sha256":descriptor,"args":args
        }))?;
        rt::retain_ffv1_original(
            &output,
            &format!("{output_path}.ffv1-host.stdin.original.bin"),
            &request,
            rt::FFV1_CUSTODY_STDIN,
        )
        .map_err(map_error)?;
        Ok(Self {
            output,
            output_path,
            request,
            scope: AvHostScope::Ffv1,
        })
    }

    fn catalog_stdin(&self, input: &[u8]) -> Result<()> {
        if !input.is_empty() {
            return Err(Error::invalid("Original discover stdin changed"));
        }
        self.retain(
            &format!(
                "{}.{}-catalog.stdin.original.bin",
                self.output_path,
                self.namespace()
            ),
            input,
            self.stdin_limit(),
        )
        .map_err(map_error)?;
        Ok(())
    }

    fn record(
        &self,
        family: &str,
        job: Option<&RuntimeToolJob>,
        state: &str,
        output: Option<(i32, &[u8], &[u8])>,
        original_error: Option<&Error>,
    ) -> Result<()> {
        if !matches!(family, "host" | "catalog")
            || !matches!(
                state,
                "start-returned-error"
                    | "wait-returned-error"
                    | "host-returned-error"
                    | "completed"
            )
        {
            return Err(Error::invalid(format!(
                "{} custody terminal label changed",
                self.label()
            )));
        }
        if let Some(job) = job {
            job.validate()?;
        }
        let mut custody_error = None;
        let mut retain = |name: &str, bytes: &[u8]| {
            let prefix = &bytes[..bytes.len().min(self.stream_limit())];
            let path = format!(
                "{}.{}-{family}.{name}.original.bin",
                self.output_path,
                self.namespace()
            );
            let retained = match self.retain(&path, prefix, self.stream_limit()) {
                Ok(_) => true,
                Err(error) => {
                    if custody_error.is_none() {
                        custody_error = Some(map_error(error));
                    }
                    false
                }
            };
            if bytes.len() > prefix.len() && custody_error.is_none() {
                custody_error = Some(
                    Error::new(
                        ErrorCode::ResourceExhausted,
                        format!(
                            "Original Host buffer exceeds {} custody limit",
                            self.label()
                        ),
                    )
                    .uncertain(),
                );
            }
            serde_json::json!({"root":"output","path":path,"sha256":sha256(prefix),
                "bytes":prefix.len(),"delivered_buffer_bytes":bytes.len(),
                "delivered_buffer_sha256":sha256(bytes),"diagnostic_prefix_truncated":bytes.len()>prefix.len(),
                "retained":retained,"stream_eof_attested":false})
        };
        let (exit_code, stdout, stderr) = match output {
            Some((exit, stdout, stderr)) => (
                Some(exit),
                Some(retain("stdout", stdout)),
                Some(retain("stderr", stderr)),
            ),
            None => (None, None, None),
        };
        let stdin = if family == "catalog" {
            &[][..]
        } else {
            self.request.as_slice()
        };
        let document = serde_json::json!({
            "schema":self.schema(),"host_stage":family,"state":state,
            "output_path":self.output_path,"runtime_job_id":job.map(|job|job.id.as_str()),
            "runtime_job_id_exposed":job.is_some(),"host_exit_code":exit_code,
            "stdin":{"root":"output","path":format!("{}.{}-{family}.stdin.original.bin",self.output_path,self.namespace()),
                "sha256":sha256(stdin),"bytes":stdin.len()},
            "stdin_scope":if family=="catalog" {"same-empty-Vec-supplied-to-existing-discover-call"}
                else {"same-serialized-Vec-supplied-to-existing-AV-job-start"},
            "stdin_delivery_confirmed":if output.is_some() {Some(true)} else {None},
            "stdout":stdout,"stderr":stderr,"original_error":original_error,
            "stream_eof_attested":false,"native_execution_retried":false,"physical_acceptance":false,
            "capture_scope":"same-existing-Host-return-before-parse-and-public-redaction"
        });
        let bytes = serde_json::to_vec(&document)?;
        if let Err(error) = self.retain(
            &format!(
                "{}.{}-{family}.terminal.original.json",
                self.output_path,
                self.namespace()
            ),
            &bytes,
            self.metadata_limit(),
        ) && custody_error.is_none()
        {
            custody_error = Some(map_error(error));
        }
        match custody_error {
            Some(error) => Err(error.uncertain()),
            None => Ok(()),
        }
    }
}

#[derive(Clone, Copy)]
enum AvHostScope {
    Ffv1,
    AudioDerivative,
}

impl AvHostCustody {
    fn namespace(&self) -> &'static str {
        match self.scope {
            AvHostScope::Ffv1 => "ffv1",
            AvHostScope::AudioDerivative => "audio-derive",
        }
    }

    fn label(&self) -> &'static str {
        match self.scope {
            AvHostScope::Ffv1 => "FFV1",
            AvHostScope::AudioDerivative => "Audio derivative",
        }
    }

    fn schema(&self) -> &'static str {
        match self.scope {
            AvHostScope::Ffv1 => "semwright.ffv1-host-original/1",
            AvHostScope::AudioDerivative => "semwright.audio-derivative-host-original/1",
        }
    }

    fn stdin_limit(&self) -> usize {
        match self.scope {
            AvHostScope::Ffv1 => semwright_mlt_video::runtime::FFV1_CUSTODY_STDIN,
            AvHostScope::AudioDerivative => {
                semwright_mlt_video::runtime::AUDIO_DERIVATIVE_STDIN_LIMIT
            }
        }
    }

    fn stream_limit(&self) -> usize {
        match self.scope {
            AvHostScope::Ffv1 => semwright_mlt_video::runtime::FFV1_CUSTODY_HOST_STREAM,
            AvHostScope::AudioDerivative => {
                semwright_mlt_video::runtime::AUDIO_DERIVATIVE_STREAM_LIMIT
            }
        }
    }

    fn metadata_limit(&self) -> usize {
        match self.scope {
            AvHostScope::Ffv1 => semwright_mlt_video::runtime::FFV1_CUSTODY_METADATA,
            AvHostScope::AudioDerivative => {
                semwright_mlt_video::runtime::AUDIO_DERIVATIVE_METADATA_LIMIT
            }
        }
    }

    fn retain(
        &self,
        path: &str,
        bytes: &[u8],
        maximum: usize,
    ) -> semwright_mlt_video::Result<Artifact> {
        match self.scope {
            AvHostScope::Ffv1 => semwright_mlt_video::runtime::retain_ffv1_original(
                &self.output,
                path,
                bytes,
                maximum,
            ),
            AvHostScope::AudioDerivative => {
                semwright_mlt_video::runtime::retain_audio_derivative_original(
                    &self.output,
                    path,
                    bytes,
                    maximum,
                )
            }
        }
    }

    fn new_audio(app: &App, command: &str, descriptor: &str, args: &Value) -> Result<Self> {
        use semwright_mlt_video::runtime as rt;
        if command != rt::AUDIO_DERIVATIVE_COMMAND {
            return Err(Error::invalid("Audio custody is closed to audio.derive"));
        }
        let output_path = args
            .get("output_path")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::invalid("Audio original output is absent"))?
            .to_owned();
        let output =
            app.roots.get("output").cloned().ok_or_else(|| {
                Error::new(ErrorCode::Unavailable, "Original output grant is absent")
            })?;
        rt::preflight_audio_derivative_custody(&output, &output_path, true).map_err(map_error)?;
        let request = serde_json::to_vec(&serde_json::json!({
            "command":command,"descriptor_sha256":descriptor,"args":args
        }))?;
        let custody = Self {
            output,
            output_path,
            request,
            scope: AvHostScope::AudioDerivative,
        };
        custody
            .retain(
                &format!(
                    "{}.audio-derive-host.stdin.original.bin",
                    custody.output_path
                ),
                &custody.request,
                custody.stdin_limit(),
            )
            .map_err(map_error)?;
        Ok(custody)
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as u64)
}

const NATIVE_HOST_CUSTODY_LIMIT: usize = 16_384;
struct OriginalHostTerminal<'a> {
    render_job: &'a str,
    revision: &'a str,
    output_path: &'a str,
    runtime_job: &'a RuntimeToolJob,
    exit_code: i32,
    request: &'a [u8],
    stdout: &'a [u8],
}
fn host_custody_readback_uncertain() -> Error {
    let mut error = Error::new(
        ErrorCode::BackendFailed,
        "Original Host custody was published but its readback is uncertain",
    );
    error.outcome_known = false;
    error
}
// Called once from the original Host completion. No renderer, reconstructed
// request, process inspection, argv/environment capture, or retry exists here.
fn retain_original_host_custody(
    work: &Root,
    output: &Root,
    original: &OriginalHostTerminal<'_>,
) -> Result<Artifact> {
    original.runtime_job.validate()?;
    let job = original
        .render_job
        .strip_prefix("render:")
        .filter(|s| {
            !s.is_empty()
                && s.len() <= 73
                && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
        .ok_or_else(host_render_protocol_error)?;
    let _ = job;
    if original.exit_code != 0
        || !semwright_mlt_video::hash::valid_digest(original.revision)
        || original.request.is_empty()
        || original.request.len() > color::NATIVE_OBSERVATION_REQUEST_LIMIT
    {
        return Err(host_render_protocol_error());
    }
    let descriptor = host_render_result(original.stdout, "partial.mkv", Some(original.request))?
        .ok_or_else(host_render_protocol_error)?;
    let bound = bind_host_native_observation(work, original.request, &descriptor)?;
    if bound.request.output_path != original.output_path {
        return Err(host_render_protocol_error());
    }
    let request_file = Artifact {
        root: "output".into(),
        path: format!("{}.native-host-request.json", original.output_path),
        sha256: sha256(original.request),
        bytes: original.request.len() as u64,
    };
    let terminal_file = Artifact {
        root: "output".into(),
        path: format!("{}.native-host-terminal.json", original.output_path),
        sha256: sha256(original.stdout),
        bytes: original.stdout.len() as u64,
    };
    let original_observation = Artifact {
        root: "output".into(),
        path: format!("{}.native-host-observation.json", original.output_path),
        sha256: sha256(&bound.report),
        bytes: bound.report.len() as u64,
    };
    let planned_input = Artifact {
        root: "output".into(),
        path: format!("{}.native-input.mlt", original.output_path),
        sha256: bound.request.project_sha256.clone(),
        bytes: bound.request.project_bytes,
    };
    let planned_report = Artifact {
        root: "output".into(),
        path: format!("{}.color-observation.json", original.output_path),
        sha256: original_observation.sha256.clone(),
        bytes: original_observation.bytes,
    };
    let planned_video = Artifact {
        root: "output".into(),
        path: original.output_path.into(),
        sha256: bound.video_sha256.clone(),
        bytes: bound.video_bytes,
    };
    let document = serde_json::json!({
        "schema":"semwright.mlt-native-host-custody/1",
        "render_job":original.render_job, "project_revision":original.revision,
        "profile":"lossless-video-only", "output_path":original.output_path,
        "runtime_job_id":original.runtime_job.id, "host_exit_code":original.exit_code,
        "stage":"original-host-terminal-before-final-publication",
        "request":from_internal(request_file.json())?,
        "terminal":from_internal(terminal_file.json())?,
        "observation":from_internal(descriptor.json())?,
        "original_observation":from_internal(original_observation.json())?,
        "publication_plan":{
            "scope":"planned-destinations-not-commit-observations",
            "native_input":from_internal(planned_input.json())?,
            "color_observation":from_internal(planned_report.json())?,
            "video":from_internal(planned_video.json())?,
            "decoded_frame_count":bound.request.frames,
            "decoded_frame_prefix":format!("{}.decoded-",original.output_path),
            "decoded_frame_suffix":".png", "decoded_frame_index_width":6,
            "frame_descriptors":"original_observation.frames"
        },
        "final_video_commit_observed":false, "native_execution_retried":false
    });
    let document = serde_json::to_vec(&document)?;
    if document.len() > NATIVE_HOST_CUSTODY_LIMIT {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Original Host custody metadata exceeds its closed bound",
        ));
    }
    // Admission precedes every write; descriptor-relative no-replace/fsync
    // retains partial originals independently of the final native effect.
    for (file, bytes, maximum) in [
        (
            &request_file,
            original.request,
            color::NATIVE_OBSERVATION_REQUEST_LIMIT as u64,
        ),
        (&terminal_file, original.stdout, 4096u64),
        (
            &original_observation,
            bound.report.as_slice(),
            color::REPORT_LIMIT as u64,
        ),
    ] {
        output
            .publish_verified(
                "output",
                &file.path,
                bytes,
                maximum,
                &file.sha256,
                file.bytes,
            )
            .map_err(map_error)?;
        let actual = output
            .read(&file.path, maximum as usize)
            .map_err(|_| host_custody_readback_uncertain())?;
        if actual != bytes {
            return Err(host_custody_readback_uncertain());
        }
    }
    let path = format!("{}.native-host-custody.json", original.output_path);
    let retained = output
        .publish_verified(
            "output",
            &path,
            document.as_slice(),
            NATIVE_HOST_CUSTODY_LIMIT as u64,
            &sha256(&document),
            document.len() as u64,
        )
        .map_err(map_error)?;
    if output
        .read(&path, NATIVE_HOST_CUSTODY_LIMIT)
        .map_err(|_| host_custody_readback_uncertain())?
        != document
    {
        return Err(host_custody_readback_uncertain());
    }
    Ok(retained)
}
// Retention never replaces the original native error or turns UNKNOWN into cancel.
fn record_original_host_custody(snapshot: &mut JobSnapshot, result: Result<Artifact>) {
    snapshot.native_host_custody = Some(match result {
        Ok(file) => jobs::NativeHostCustody::Retained(file),
        Err(error) => jobs::NativeHostCustody::Unretained(to_internal_error(error)),
    });
}
// Previous partial or uncertain originals remain; a new native attempt needs
// a new output destination. This function never removes or replays them.
fn preflight_native_host_custody(output: &Root, path: &str) -> Result<()> {
    semwright_mlt_video::fs::validate_relative(path).map_err(map_error)?;
    for suffix in [
        ".native-host-request.json",
        ".native-host-terminal.json",
        ".native-host-observation.json",
        ".native-host-custody.json",
    ] {
        if output
            .exists(&format!("{path}{suffix}"))
            .map_err(map_error)?
        {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Original Host custody already exists; use a new output destination",
            ));
        }
    }
    let reserve = color::REPORT_LIMIT as u64
        + color::NATIVE_OBSERVATION_REQUEST_LIMIT as u64
        + 4096
        + NATIVE_HOST_CUSTODY_LIMIT as u64;
    if output.available_bytes_for(path).map_err(map_error)? < reserve {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Output lacks the bounded original Host custody reserve",
        ));
    }
    Ok(())
}

fn host_render_protocol_error() -> Error {
    Error::new(
        ErrorCode::PluginProtocolError,
        "MLT render result differs from the original Host request",
    )
}

// Accepted only for this original Host job. Stand-alone JSON is not authority.
fn host_render_result(
    bytes: &[u8],
    expected_output: &str,
    original_request: Option<&[u8]>,
) -> Result<Option<Artifact>> {
    if bytes.len() > 4096 {
        return Err(host_render_protocol_error());
    }
    semwright_mlt_video::json::parse(bytes).map_err(|_| host_render_protocol_error())?;
    let value: Value = serde_json::from_slice(bytes).map_err(|_| host_render_protocol_error())?;
    let object = value.as_object().ok_or_else(host_render_protocol_error)?;
    let fields = if original_request.is_some() { 5 } else { 3 };
    if object.len() != fields
        || value["schema"].as_u64() != Some(1)
        || value["operation"].as_str() != Some("render")
        || value["output"].as_str() != Some(expected_output)
    {
        return Err(host_render_protocol_error());
    }
    let Some(request) = original_request else {
        return Ok(None);
    };
    if value["request_sha256"].as_str() != Some(sha256(request).as_str()) {
        return Err(host_render_protocol_error());
    }
    let descriptor = value["observation"]
        .as_object()
        .ok_or_else(host_render_protocol_error)?;
    let digest = descriptor
        .get("sha256")
        .and_then(Value::as_str)
        .filter(|s| semwright_mlt_video::hash::valid_digest(s))
        .ok_or_else(host_render_protocol_error)?;
    let size = descriptor
        .get("bytes")
        .and_then(Value::as_u64)
        .filter(|n| *n > 0 && *n <= color::REPORT_LIMIT as u64)
        .ok_or_else(host_render_protocol_error)?;
    if descriptor.len() != 4
        || descriptor.get("root").and_then(Value::as_str) != Some("scratch")
        || descriptor.get("path").and_then(Value::as_str) != Some(color::NATIVE_OBSERVATION_FILE)
    {
        return Err(host_render_protocol_error());
    }
    Ok(Some(Artifact {
        root: "scratch".into(),
        path: color::NATIVE_OBSERVATION_FILE.into(),
        sha256: digest.into(),
        bytes: size,
    }))
}

fn verify_host_native_request(
    work: &Root,
    original: &[u8],
) -> Result<color::NativeObservationRequest> {
    let actual = work
        .read(
            color::NATIVE_OBSERVATION_REQUEST_FILE,
            color::NATIVE_OBSERVATION_REQUEST_LIMIT,
        )
        .map_err(map_error)?;
    if actual != original {
        return Err(Error::new(
            ErrorCode::StaleReference,
            "Native observation request changed after Host staging",
        ));
    }
    let request = color::NativeObservationRequest::parse(original).map_err(map_error)?;
    request.verify_staged(work).map_err(map_error)?;
    Ok(request)
}

struct BoundHostObservation {
    request: color::NativeObservationRequest,
    report: Vec<u8>,
    video_sha256: String,
    video_bytes: u64,
}

fn bind_host_native_observation(
    work: &Root,
    original: &[u8],
    descriptor: &Artifact,
) -> Result<BoundHostObservation> {
    let request = verify_host_native_request(work, original)?;
    if descriptor.root != "scratch"
        || descriptor.path != color::NATIVE_OBSERVATION_FILE
        || descriptor.bytes == 0
        || descriptor.bytes > color::REPORT_LIMIT as u64
        || !semwright_mlt_video::hash::valid_digest(&descriptor.sha256)
    {
        return Err(host_render_protocol_error());
    }
    let report = work
        .read(color::NATIVE_OBSERVATION_FILE, color::REPORT_LIMIT)
        .map_err(map_error)?;
    if report.len() as u64 != descriptor.bytes || sha256(&report) != descriptor.sha256 {
        return Err(Error::new(
            ErrorCode::StaleReference,
            "Complete observation differs from the original terminal Host result",
        ));
    }
    let parsed =
        color::parse_complete_observation(&report).map_err(|error| map_error(error.into()))?;
    request.verify_report(&parsed).map_err(map_error)?;
    let doc = parsed.document();
    let video_sha256 = doc["artifact"]["sha256"]
        .as_str()
        .ok_or_else(host_render_protocol_error)?
        .to_owned();
    let video_bytes = doc["artifact"]["bytes"]
        .as_u64()
        .ok_or_else(host_render_protocol_error)?;
    Ok(BoundHostObservation {
        request,
        report,
        video_sha256,
        video_bytes,
    })
}

// The existing token is checked during the final copy. After commit,
// cancellation cannot erase an UNKNOWN durability or readback outcome.
struct HostRenderReader<F> {
    file: std::fs::File,
    cancelled: F,
}
impl<F: Fn() -> bool> Read for HostRenderReader<F> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        if (self.cancelled)() {
            return Err(std::io::Error::other("Host render publication cancelled"));
        }
        self.file.read(bytes)
    }
}

fn host_render_failure_state(error: &semwright_mlt_video::Error) -> State {
    if !error.outcome_known {
        State::Unknown
    } else if error.code == "Cancelled" {
        State::Cancelled
    } else {
        State::Failed
    }
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
        custody: Option<&AvHostCustody>,
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
        let request = match custody {
            Some(custody) => custody.request.clone(),
            None => serde_json::to_vec(&serde_json::json!({
                "command":command,"descriptor_sha256":descriptor,"args":args
            }))?,
        };
        let started = context
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
                request,
                Duration::from_secs(300),
                None,
            )
            .await;
        let job = match started {
            Ok(job) => job,
            Err(original) => {
                let retained = custody
                    .map(|c| c.record("host", None, "start-returned-error", None, Some(&original)))
                    .unwrap_or(Ok(()));
                return finish_ffv1_host_original(Err(original), retained);
            }
        };
        let result = match context.wait_runtime_tool_job(&job).await {
            Ok(result) => result,
            Err(original) => {
                let retained = custody
                    .map(|c| {
                        c.record(
                            "host",
                            Some(&job),
                            "wait-returned-error",
                            None,
                            Some(&original),
                        )
                    })
                    .unwrap_or(Ok(()));
                return finish_ffv1_host_original(Err(original), retained);
            }
        };
        let retained = custody
            .map(|c| {
                c.record(
                    "host",
                    Some(&job),
                    "completed",
                    Some((result.exit_code, &result.stdout, &result.stderr)),
                    None,
                )
            })
            .unwrap_or(Ok(()));
        let original = (|| -> Result<Value> {
            if result.exit_code != 0 {
                let mut error = host_av_operation_failure(&result.stdout);
                error.outcome_known = matches!(
                    command,
                    "driver.mlt-video.sync.probe" | "driver.mlt-video.av.caption_pixels.verify"
                );
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
        })();
        finish_ffv1_host_original(original, retained)
    }

    async fn host_probe_staged(
        &self,
        directory: &str,
        name: &str,
        context: &DriverExecutionContext,
    ) -> Result<MediaInfo> {
        self.host_probe_staged_observed(directory, name, false, context)
            .await
            .map(|(media, _)| media)
    }

    async fn host_probe_staged_observed(
        &self,
        directory: &str,
        name: &str,
        count_video_frames: bool,
        context: &DriverExecutionContext,
    ) -> Result<(MediaInfo, Vec<u8>)> {
        let mut args = vec![
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
        ];
        if count_video_frames {
            args.push(RuntimeToolArg::Literal {
                value: "--count-video-frames".into(),
            });
        }
        let output = if count_video_frames {
            // Full frame decoding is bounded but may exceed 30 seconds for
            // 36k FFV1 frames. Protocol v7 synchronous tools have a strict
            // 30-second ceiling; use its *existing* Host-owned detached job
            // with cancellation and reap-on-error semantics. Never weaken
            // the protocol or run a consumer-side ffmpeg process.
            let job = context
                .start_runtime_tool_job_args(
                    "mlt-runner",
                    args,
                    Vec::new(),
                    std::time::Duration::from_secs(150),
                    None,
                )
                .await?;
            context.wait_runtime_tool_job(&job).await?
        } else {
            context
                .execute_runtime_tool_args(
                    "mlt-runner",
                    args,
                    Vec::new(),
                    std::time::Duration::from_secs(30),
                    None,
                )
                .await?
        };
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
            .and_then(|v| v.to_str())
            .filter(|v| !v.is_empty() && v.len() <= 128)
            .ok_or_else(|| Error::new(ErrorCode::Internal, "MLT scratch directory is invalid"))?
            .to_owned();
        let sequence = request
            .project
            .sequence(&request.sequence)
            .map_err(map_error)?
            .clone();
        let complete = request.profile.id == "lossless-video-only";
        let output_root = self
            .app
            .roots
            .get("output")
            .ok_or_else(|| Error::new(ErrorCode::Unavailable, "Output mount disappeared"))?;
        if complete {
            preflight_native_host_custody(output_root, &request.output)?;
            color::preflight_observation_output(
                output_root,
                &request.output,
                request
                    .profile
                    .width
                    .unwrap_or(request.project.profile.width),
                request
                    .profile
                    .height
                    .unwrap_or(request.project.profile.height),
                sequence.duration(),
                true,
            )
            .map_err(map_error)?;
        }
        let (asset_ids, _) = jobs::required(&request.project, &sequence).map_err(map_error)?;
        let input_root = Root::open(scratch.path(), true, false).map_err(map_error)?;
        let mut staged = BTreeMap::new();
        let mut staged_sources = Vec::new();
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
            let mut copied = 0u64;
            let mut buffer = [0u8; 65536];
            loop {
                context.check_cancelled()?;
                let n = source.read(&mut buffer).map_err(|_| {
                    Error::new(
                        ErrorCode::BackendFailed,
                        "Failed to read bounded MLT render input",
                    )
                })?;
                if n == 0 {
                    break;
                }
                copied = copied.checked_add(n as u64).ok_or_else(|| {
                    Error::new(ErrorCode::ResourceExhausted, "MLT input size overflow")
                })?;
                total = total.checked_add(n as u64).ok_or_else(|| {
                    Error::new(ErrorCode::ResourceExhausted, "MLT input total overflow")
                })?;
                if copied > MAX_MEDIA_BYTES || total > MAX_JOB_INPUT_BYTES {
                    return Err(Error::new(
                        ErrorCode::ResourceExhausted,
                        "Staged MLT render inputs exceed their byte budget",
                    ));
                }
                destination.write_all(&buffer[..n]).map_err(|_| {
                    Error::new(
                        ErrorCode::BackendFailed,
                        "Failed to stage bounded MLT render input",
                    )
                })?;
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
            if complete {
                let actual = reader_hash(
                    input_root
                        .read_file(&name, MAX_MEDIA_BYTES)
                        .map_err(map_error)?,
                    MAX_MEDIA_BYTES,
                )
                .map_err(map_error)?;
                if actual.1 != copied {
                    return Err(Error::new(
                        ErrorCode::StaleReference,
                        "Staged native input length changed before Host request",
                    ));
                }
                staged_sources.push((
                    Artifact {
                        root: root_name,
                        path,
                        sha256: actual.0,
                        bytes: actual.1,
                    },
                    name.clone(),
                ));
            }
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
        let observation_request = if complete {
            let bytes = color::native_observation_request(
                &request.output,
                prepared.xml.as_bytes(),
                &prepared.expected_profile,
                prepared.frames,
                &staged_sources,
            )
            .map_err(map_error)?;
            let budget = color::observation_budget(
                prepared.expected_profile.width,
                prepared.expected_profile.height,
                prepared.frames,
            )
            .map_err(|error| map_error(error.into()))?;
            if input_root
                .available_bytes_for("decoded-000000.png")
                .map_err(map_error)?
                < budget.peak_free_bytes + 2 * semwright_mlt_video::xml::MAX_XML as u64
            {
                return Err(Error::new(
                    ErrorCode::ResourceExhausted,
                    "Host scratch lacks complete observation free-space budget",
                ));
            }
            let mut f = scratch
                .create(color::NATIVE_OBSERVATION_REQUEST_FILE)
                .map_err(map_error)?;
            f.write_all(&bytes).map_err(|_| {
                Error::new(
                    ErrorCode::BackendFailed,
                    "Failed to stage native observation request",
                )
            })?;
            f.sync_all().map_err(|_| {
                Error::new(
                    ErrorCode::BackendFailed,
                    "Failed to sync native observation request",
                )
            })?;
            drop(f);
            scratch
                .seal(color::NATIVE_OBSERVATION_REQUEST_FILE)
                .map_err(map_error)?;
            verify_host_native_request(&input_root, &bytes)?;
            Some(bytes)
        } else {
            None
        };
        let literal = |value: &str| RuntimeToolArg::Literal {
            value: value.into(),
        };
        let tool = |name: &str| RuntimeToolArg::ToolPath { tool: name.into() };
        let mount = |name: &str| RuntimeToolArg::MountPath {
            mount: name.into(),
            relative: String::new(),
        };
        let mut argv = vec![
            literal("render"),
            literal("--runtime-root"),
            mount("mlt-runtime"),
            literal("--melt-sealed"),
            tool("melt"),
            literal("--scratch-root"),
            mount("scratch"),
            literal("--directory"),
            literal(&directory),
            literal("--profile"),
            literal(request.profile.id),
        ];
        if complete {
            argv.extend([
                literal("--complete-observation"),
                literal("--ffprobe-sealed"),
                tool("ffprobe"),
                literal("--ffmpeg-sealed"),
                tool("ffmpeg"),
            ]);
        }
        // Allocate fallible local state before starting the Host job.
        let id = format!("render:{}", random_id().map_err(map_error)?);
        context.check_cancelled()?;
        let host_job = context
            .start_runtime_tool_job_args(
                "mlt-runner",
                argv,
                Vec::new(),
                // Existing Host CPU ceiling; one native render+observation deadline.
                Duration::from_secs(300),
                None,
            )
            .await?;
        let now = now_ms();
        let output_file = format!("partial.{}", request.profile.extension);
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
            native_input: None,
            native_input_sources: Vec::new(),
            color_observation: None,
            native_host_custody: None,
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
                observation_request,
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
        observation: Option<(&[u8], &Artifact)>,
        context: &DriverExecutionContext,
    ) -> Result<RenderedOutput> {
        let scratch_path = diagnostic.scratch_path;
        context.check_cancelled()?;
        let source_root = Root::open(scratch_path, true, false).map_err(map_error)?;
        let native = match (profile.id == "lossless-video-only", observation) {
            (true, Some((request, descriptor))) => {
                let bound = bind_host_native_observation(&source_root, request, descriptor)?;
                if bound.request.output_path != output_path
                    || bound.request.width != expected_profile.width
                    || bound.request.height != expected_profile.height
                    || bound.request.fps_num != expected_profile.fps.num
                    || bound.request.fps_den != expected_profile.fps.den
                    || bound.request.frames != frames
                {
                    return Err(host_render_protocol_error());
                }
                Some(bound)
            }
            (false, None) => None,
            _ => return Err(host_render_protocol_error()),
        };
        let (mut media, raw_probe) = self
            .host_probe_staged_observed(
                diagnostic.directory,
                output_file,
                profile.id == "lossless-video-only",
                context,
            )
            .await?;
        if profile.id == "lossless-video-only" {
            // The Host passed the exact --count-video-frames argument to
            // FFprobe in the *same* confined runner. Require nb_read_frames:
            // ordinary Matroska nb_frames is not proof of a full decode.
            // Independently inspect all streams before publishing anything.
            let envelope: Value = serde_json::from_slice(&raw_probe).map_err(|_| {
                Error::new(
                    ErrorCode::PluginProtocolError,
                    "MLT decoded frame envelope is malformed",
                )
            })?;
            let observation = envelope.get("media").ok_or_else(|| {
                Error::new(
                    ErrorCode::PluginProtocolError,
                    "MLT decoder has no complete stream evidence",
                )
            })?;
            let bytes = serde_json::to_vec(observation)?;
            let observed =
                semwright_mlt_video::runtime::parse_exact_ffv1_decoded_frame_count(&bytes)
                    .map_err(map_error)?;
            media.frames = Some(observed);
        }
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
        let output_root = self
            .app
            .roots
            .get("output")
            .ok_or_else(|| Error::new(ErrorCode::Unavailable, "Output mount disappeared"))?;
        let mut native_input = None;
        let mut native_input_sources = Vec::new();
        let mut color_observation = None;
        if let Some(bound) = &native {
            context.check_cancelled()?;
            bound
                .request
                .verify_staged(&source_root)
                .map_err(map_error)?;
            let retained = output_root
                .publish_verified(
                    "output",
                    &format!("{output_path}.native-input.mlt"),
                    source_root
                        .read_file("project.mlt", semwright_mlt_video::xml::MAX_XML as u64)
                        .map_err(map_error)?,
                    semwright_mlt_video::xml::MAX_XML as u64,
                    &bound.request.project_sha256,
                    bound.request.project_bytes,
                )
                .map_err(map_error)?;
            if output_root
                .digest(&retained.path, semwright_mlt_video::xml::MAX_XML as u64)
                .map_err(map_error)?
                != retained.sha256
            {
                return Err(Error::new(
                    ErrorCode::StaleReference,
                    "Retained native XML changed before video publication",
                ));
            }
            color_observation = Some(
                color::publish_complete_observation_checked(
                    &bound.report,
                    &source_root,
                    output_root,
                    output_path,
                    || context.check_cancelled().map_err(to_internal_error),
                )
                .map_err(map_error)?,
            );
            native_input_sources = bound.request.sources();
            native_input = Some(retained);
            // Recheck original request and sources immediately before final publication.
            verify_host_native_request(
                &source_root,
                observation.ok_or_else(host_render_protocol_error)?.0,
            )?;
        }
        context.check_cancelled()?;
        let token = context.cancellation();
        let source = HostRenderReader {
            file: source_root
                .read_file(output_file, MAX_ARTIFACT_BYTES)
                .map_err(map_error)?,
            cancelled: || token.is_cancelled(),
        };
        let publication = match &native {
            Some(bound) => output_root.publish_verified(
                "output",
                output_path,
                source,
                MAX_ARTIFACT_BYTES,
                &bound.video_sha256,
                bound.video_bytes,
            ),
            None => output_root.publish("output", output_path, source, MAX_ARTIFACT_BYTES),
        };
        let artifact = match publication {
            Ok(artifact) => artifact,
            Err(error) if !error.outcome_known => return Err(map_error(error)),
            Err(error) => {
                context.check_cancelled()?;
                return Err(map_error(error));
            }
        };
        // No cancellation check after final-name commit. Preserve UNKNOWN.
        if native.is_some() {
            color::verify_published_video(output_root, &artifact).map_err(map_error)?;
        }
        Ok(RenderedOutput {
            artifact,
            media,
            native_input,
            native_input_sources,
            color_observation,
        })
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
            observation_request,
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
                job.observation_request.clone(),
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
                    host_render_result(&output.stdout, &output_file, observation_request.as_deref())
                };

                if let Some(request) = observation_request.as_deref() {
                    let custody = if output.exit_code == 0
                        && terminal.as_ref().is_ok_and(|value| value.is_some())
                    {
                        (|| -> Result<Artifact> {
                            let work = Root::open(&scratch_path, true, false).map_err(map_error)?;
                            let destination = self.app.roots.get("output").ok_or_else(|| {
                                Error::new(ErrorCode::Unavailable, "Output mount disappeared")
                            })?;
                            retain_original_host_custody(
                                &work,
                                destination,
                                &OriginalHostTerminal {
                                    render_job: &next.id,
                                    revision: &next.revision,
                                    output_path: &next.output,
                                    runtime_job: &host_job,
                                    exit_code: output.exit_code,
                                    request,
                                    stdout: &output.stdout,
                                },
                            )
                        })()
                    } else {
                        Err(Error::new(
                            ErrorCode::Unavailable,
                            "No closed successful original Host terminal is available",
                        ))
                    };
                    record_original_host_custody(&mut next, custody);
                }

                let finalized = match terminal {
                    Ok(descriptor) => {
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
                            observation_request.as_deref().zip(descriptor.as_ref()),
                            context,
                        )
                        .await
                    }
                    Err(error) => Err(error),
                };
                match finalized {
                    Ok(rendered) => {
                        next.state = State::Succeeded;
                        next.completed_at = Some(now_ms());
                        next.artifact = Some(rendered.artifact);
                        next.media = Some(rendered.media);
                        next.native_input = rendered.native_input;
                        next.native_input_sources = rendered.native_input_sources;
                        next.color_observation = rendered.color_observation;
                        next.error = None;
                    }
                    Err(error) => {
                        let internal = to_internal_error(error);
                        next.state = host_render_failure_state(&internal);
                        next.completed_at = Some(now_ms());
                        next.error = Some(internal);
                    }
                }
            }
        }

        if observation_request.is_some()
            && next.state.terminal()
            && next.native_host_custody.is_none()
        {
            record_original_host_custody(
                &mut next,
                Err(Error::new(
                    ErrorCode::Unavailable,
                    "No closed successful original Host terminal is available",
                )),
            );
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

    async fn ensure_host_catalog(
        &mut self,
        context: &DriverExecutionContext,
        custody: Option<&AvHostCustody>,
    ) -> Result<()> {
        if self.host_catalog_verified || std::env::var_os("SEMWRIGHT_DRIVER_HOST_TOOLS").is_none() {
            return Ok(());
        }
        let legacy_catalog = self
            .app
            .runtime
            .as_ref()
            .map(|runtime| runtime.catalog.clone());
        let input = Vec::new();
        if let Some(custody) = custody {
            custody.catalog_stdin(&input)?;
        }
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
                input,
                std::time::Duration::from_secs(30),
                None,
            )
            .await;
        let output = match output {
            Ok(output) => output,
            Err(original) => {
                let retained = custody
                    .map(|c| {
                        c.record(
                            "catalog",
                            None,
                            "host-returned-error",
                            None,
                            Some(&original),
                        )
                    })
                    .unwrap_or(Ok(()));
                return finish_ffv1_host_original(Err(original), retained);
            }
        };
        let retained = custody
            .map(|c| {
                c.record(
                    "catalog",
                    None,
                    "completed",
                    Some((output.exit_code, &output.stdout, &output.stderr)),
                    None,
                )
            })
            .unwrap_or(Ok(()));
        let original = (|| -> Result<()> {
            if output.exit_code != 0 {
                let detail = host_runner_failure(&output.stdout);
                return Err(Error::new(
                    ErrorCode::BackendFailed,
                    match detail {
                        Some(detail) => {
                            format!("Host-mediated MLT discovery runner failed: {detail}")
                        }
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
        })();
        finish_ffv1_host_original(original, retained)
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
        let custody = if command == "driver.mlt-video.frames.encode" {
            Some(AvHostCustody::new(
                &self.app,
                command,
                descriptor_sha256,
                &args,
            )?)
        } else if command == semwright_mlt_video::runtime::AUDIO_DERIVATIVE_COMMAND {
            Some(AvHostCustody::new_audio(
                &self.app,
                command,
                descriptor_sha256,
                &args,
            )?)
        } else {
            None
        };
        self.ensure_host_catalog(&context, custody.as_ref()).await?;
        if matches!(
            command,
            "driver.mlt-video.frames.encode"
                | "driver.mlt-video.av.mux"
                | semwright_mlt_video::runtime::AUDIO_DERIVATIVE_COMMAND
                | "driver.mlt-video.av.caption_pixels.verify"
                | "driver.mlt-video.sync.probe"
        ) {
            return self
                .host_av_operation(
                    command,
                    descriptor_sha256,
                    &args,
                    &context,
                    custody.as_ref(),
                )
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

#[cfg(test)]
mod host_native_observation_tests {
    use super::*;
    use serde_json::json;

    fn original_request_fixture() -> (PrivateDir, Root, Vec<u8>) {
        let directory = PrivateDir::new(std::path::Path::new("/tmp")).unwrap();
        let work = Root::open(directory.path(), true, true).unwrap();
        let xml = b"<mlt>one</mlt>";
        let source = b"original-native-source";
        let staged = format!("asset-{}.bin", "a".repeat(24));
        work.write_new("scratch", "project.mlt", xml).unwrap();
        work.write_new("scratch", &staged, source).unwrap();
        let request = serde_json::to_vec(&json!({
            "schema":"semwright.mlt-native-observation-request/1",
            "output_path":"fixture.mkv", "project_sha256":sha256(xml),
            "project_bytes":xml.len(), "width":16, "height":16,
            "fps_num":30, "fps_den":1, "frames":1,
            "staged_sources":[{"root":"media","path":"source.bin",
                "sha256":sha256(source),"bytes":source.len(),"staged_name":staged}]
        }))
        .unwrap();
        work.write_new("scratch", color::NATIVE_OBSERVATION_REQUEST_FILE, &request)
            .unwrap();
        (directory, work, request)
    }

    // Synthetic observed-data fixture, never evidence of native execution.
    // Production publication still decodes the actual PNG bytes.
    fn complete_report_fixture(work: &Root, original: &[u8]) -> (Vec<u8>, Artifact) {
        complete_report_fixture_with_png(work, original, None)
    }

    fn complete_report_fixture_with_png(
        work: &Root,
        original: &[u8],
        png: Option<&[u8]>,
    ) -> (Vec<u8>, Artifact) {
        let request = verify_host_native_request(work, original).unwrap();
        let mut stream = json!({"index":0,"codec_type":"video","codec_name":"ffv1",
            "width":16,"height":16,"pix_fmt":"yuv444p","r_frame_rate":"30/1",
            "avg_frame_rate":"30/1","time_base":"1/1000",
            "sample_aspect_ratio":"1:1","field_order":"progressive"});
        let mut frame = json!({"media_type":"video","stream_index":0,"pts":0,"duration":33,
            "width":16,"height":16,"pix_fmt":"yuv444p","sample_aspect_ratio":"1:1",
            "interlaced_frame":0,"top_field_first":0,"repeat_pict":0,
            "crop_top":0,"crop_bottom":0,"crop_left":0,"crop_right":0});
        for target in [&mut stream, &mut frame] {
            target.as_object_mut().unwrap().extend(
                json!({"color_range":"tv",
                "color_space":"bt709","color_primaries":"bt709","color_transfer":"bt709",
                "side_data_list":[]})
                .as_object()
                .unwrap()
                .clone(),
            );
        }
        let raw = serde_json::to_vec(&json!({"streams":[stream],"frames":[frame],
            "program_version":{"version":"fixture-only-no-native-execution"},
            "library_versions":[{"name":"libavcodec","major":60,"minor":31,"micro":102}]
        }))
        .unwrap();
        let probe = color::parse_probe_profile(
            color::ObservationProfile::MltVisualFfv1,
            &raw,
            16,
            16,
            1,
            30,
            1,
        )
        .unwrap();
        let frames = if let Some(png) = png {
            let actual = color::inspect_decoded_png(png, 0, 16, 16).unwrap();
            work.write_new("scratch", &actual.file, png).unwrap();
            vec![actual]
        } else {
            vec![color::DecodedPng {
                file: "decoded-000000.png".into(),
                bytes: 100,
                sha256: sha256(&[0u8; 100]),
                rgba_sha256: "b".repeat(64),
                alpha_min: 255,
                alpha_max: 255,
            }]
        };
        let retained_png_bytes: u64 = frames.iter().map(|frame| frame.bytes).sum();
        let rgba_roster_sha256 = color::rgba_roster_digest(&frames).unwrap();
        let observed = semwright_mlt_video::runtime::EncodedFramesObservation {
            video_sha256: "c".repeat(64),
            video_bytes: 256,
            probe,
            frames,
            retained_png_bytes,
            rgba_roster_sha256,
            ffmpeg_version: "ffmpeg version fixture-only-no-native-execution".into(),
            ffmpeg_sha256: "d".repeat(64),
            ffprobe_sha256: "e".repeat(64),
        };
        let sources = request.sources();
        let report = color::build_observation_report(
            &observed,
            &request.output_path,
            request.source_binding(),
            Some(&sources),
            16,
            16,
            30,
            1,
        )
        .unwrap();
        let descriptor = work
            .write_new("scratch", color::NATIVE_OBSERVATION_FILE, &report)
            .unwrap();
        (report, descriptor)
    }

    #[test]
    fn host_terminal_receipt_requires_the_original_request_and_closed_descriptor() {
        let request = b"original-host-request";
        let legacy = json!({"schema":1,"operation":"render","output":"partial.mkv"});
        let legacy_bytes = serde_json::to_vec(&legacy).unwrap();
        assert!(
            host_render_result(&legacy_bytes, "partial.mkv", None)
                .unwrap()
                .is_none()
        );
        assert!(host_render_result(&legacy_bytes, "partial.mkv", Some(request)).is_err());
        let mut complete = legacy;
        complete["request_sha256"] = json!(sha256(request));
        complete["observation"] = json!({"root":"scratch","path":color::NATIVE_OBSERVATION_FILE,
            "sha256":"a".repeat(64),"bytes":100});
        let encoded = serde_json::to_vec(&complete).unwrap();
        assert!(
            host_render_result(&encoded, "partial.mkv", Some(request))
                .unwrap()
                .is_some()
        );
        assert!(host_render_result(&encoded, "partial.mkv", None).is_err());
        assert!(host_render_result(&encoded, "partial.mkv", Some(b"another-host-job")).is_err());
        assert!(host_render_result(&encoded, "other.mkv", Some(request)).is_err());
        for (field, bad) in [
            ("root", json!("output")),
            ("path", json!("../report.json")),
            ("sha256", json!("not-a-digest")),
            ("bytes", json!(0)),
            ("bytes", json!(color::REPORT_LIMIT as u64 + 1)),
        ] {
            let mut altered = complete.clone();
            altered["observation"][field] = bad;
            assert!(
                host_render_result(
                    &serde_json::to_vec(&altered).unwrap(),
                    "partial.mkv",
                    Some(request)
                )
                .is_err()
            );
        }
        complete["extra"] = json!(true);
        assert!(
            host_render_result(
                &serde_json::to_vec(&complete).unwrap(),
                "partial.mkv",
                Some(request)
            )
            .is_err()
        );
        assert!(
            host_render_result(
                br#"{"schema":1,"schema":1,"operation":"render","output":"partial.mkv"}"#,
                "partial.mkv",
                None
            )
            .is_err()
        );
    }

    #[test]
    fn host_original_request_rejects_xml_media_and_request_substitution() {
        for target in ["xml", "media", "request"] {
            let (directory, work, original) = original_request_fixture();
            verify_host_native_request(&work, &original).unwrap();
            let name = match target {
                "xml" => "project.mlt".to_owned(),
                "media" => format!("asset-{}.bin", "a".repeat(24)),
                _ => color::NATIVE_OBSERVATION_REQUEST_FILE.to_owned(),
            };
            let mut bytes = work
                .read(&name, color::NATIVE_OBSERVATION_REQUEST_LIMIT)
                .unwrap();
            let i = bytes.len() - 1;
            bytes[i] ^= 1;
            std::fs::write(directory.path().join(&name), &bytes).unwrap();
            let error = verify_host_native_request(&work, &original).err().unwrap();
            assert_eq!(error.code, ErrorCode::StaleReference, "{target}");
            assert!(!work.exists("fixture.mkv").unwrap());
        }
    }

    #[test]
    fn host_binder_accepts_the_exact_complete_report_and_original_sources() {
        let (_directory, work, original) = original_request_fixture();
        let (report, descriptor) = complete_report_fixture(&work, &original);
        let bound = bind_host_native_observation(&work, &original, &descriptor).unwrap();
        assert_eq!(bound.report, report);
        assert_eq!(bound.video_sha256, "c".repeat(64));
        assert_eq!(bound.video_bytes, 256);
        assert_eq!(bound.request.sources().len(), 1);
        let document = color::parse_complete_observation(&report).unwrap();
        assert!(document.document()["stream"]["chroma_location"].is_null());
        assert_eq!(document.document()["stream"]["side_data_types"], json!([]));
    }

    #[test]
    fn host_binder_rejects_swapped_terminal_report_payload() {
        let (_directory, work, original) = original_request_fixture();
        let (_report, mut descriptor) = complete_report_fixture(&work, &original);
        descriptor.sha256 = sha256(b"a-different-terminal-payload");
        let error = bind_host_native_observation(&work, &original, &descriptor)
            .err()
            .unwrap();
        assert_eq!(error.code, ErrorCode::StaleReference);
    }

    #[test]
    fn host_binder_rejects_self_consistent_report_from_other_source() {
        let (directory, work, original) = original_request_fixture();
        let (report, mut descriptor) = complete_report_fixture(&work, &original);
        let mut doc: Value = serde_json::from_slice(&report).unwrap();
        doc["source_binding"]["sha256"] = json!("f".repeat(64));
        let replacement = serde_json::to_vec(&doc).unwrap();
        // Internally valid; only original Host source identity rejects it.
        color::parse_complete_observation(&replacement).unwrap();
        std::fs::write(
            directory.path().join(color::NATIVE_OBSERVATION_FILE),
            &replacement,
        )
        .unwrap();
        descriptor.sha256 = sha256(&replacement);
        descriptor.bytes = replacement.len() as u64;
        let error = bind_host_native_observation(&work, &original, &descriptor)
            .err()
            .unwrap();
        assert_eq!(error.code, ErrorCode::StaleReference);
        assert!(!work.exists("fixture.mkv").unwrap());
    }

    #[test]
    fn complete_report_cannot_replace_actual_png_decoding() {
        let (_directory, work, original) = original_request_fixture();
        let (report, _descriptor) = complete_report_fixture(&work, &original);
        // SHA/size matches, but the payload has no decoded PNG pixels.
        work.write_new("scratch", "decoded-000000.png", &[0u8; 100])
            .unwrap();
        assert!(
            color::publish_complete_observation_checked(
                &report,
                &work,
                &work,
                "fixture.mkv",
                || Ok(())
            )
            .is_err()
        );
        assert!(!work.exists("fixture.mkv.decoded-000000.png").unwrap());
        assert!(!work.exists("fixture.mkv.color-observation.json").unwrap());
    }

    #[test]
    fn host_copy_cancellation_is_not_an_interrupted_retry_loop() {
        let (_directory, work, _original) = original_request_fixture();
        let mut reader = HostRenderReader {
            file: work.read_file("project.mlt", 1024).unwrap(),
            cancelled: || true,
        };
        let error = std::io::copy(&mut reader, &mut Vec::new()).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::Other);
    }

    #[test]
    fn unknown_publication_precedes_cancellation_state() {
        let mut error = semwright_mlt_video::Error::new("Cancelled", "fixture");
        assert_eq!(host_render_failure_state(&error), State::Cancelled);
        error.outcome_known = false;
        assert_eq!(host_render_failure_state(&error), State::Unknown);
    }

    fn custody_terminal(request: &[u8], descriptor: &Artifact) -> Vec<u8> {
        let mut bytes = serde_json::to_vec_pretty(&json!({
            "schema":1,"operation":"render","output":"partial.mkv",
            "request_sha256":sha256(request),
            "observation":from_internal(descriptor.json()).unwrap()
        }))
        .unwrap();
        bytes.push(b'\n');
        bytes
    }
    fn retain_fixture_custody(
        work: &Root,
        output: &Root,
        request: &[u8],
        stdout: &[u8],
    ) -> Result<Artifact> {
        let runtime = RuntimeToolJob {
            id: "original-runtime-job-1".into(),
        };
        let revision = "a".repeat(64);
        retain_original_host_custody(
            work,
            output,
            &OriginalHostTerminal {
                render_job: "render:original-custody",
                revision: &revision,
                output_path: "fixture.mkv",
                runtime_job: &runtime,
                exit_code: 0,
                request,
                stdout,
            },
        )
    }
    fn custody_snapshot() -> JobSnapshot {
        JobSnapshot {
            id: "render:original-custody".into(),
            revision: "a".repeat(64),
            profile: "lossless-video-only".into(),
            output: "fixture.mkv".into(),
            state: State::Succeeded,
            created_at: 1,
            started_at: Some(2),
            completed_at: Some(3),
            artifact: None,
            media: None,
            native_input: None,
            native_input_sources: vec![],
            color_observation: None,
            native_host_custody: None,
            error: None,
            cancellation_requested: false,
        }
    }
    #[test]
    fn host_custody_retains_exact_original_request_and_terminal() {
        let (_work_directory, work, request) = original_request_fixture();
        let (report, descriptor) = complete_report_fixture(&work, &request);
        let stdout = custody_terminal(&request, &descriptor);
        let output_directory = PrivateDir::new(std::path::Path::new("/tmp")).unwrap();
        let output = Root::open(output_directory.path(), true, true).unwrap();
        let retained = retain_fixture_custody(&work, &output, &request, &stdout).unwrap();
        assert_eq!(
            output
                .read(
                    "fixture.mkv.native-host-request.json",
                    color::NATIVE_OBSERVATION_REQUEST_LIMIT
                )
                .unwrap(),
            request
        );
        assert_eq!(
            output
                .read("fixture.mkv.native-host-terminal.json", 4096)
                .unwrap(),
            stdout
        );
        assert_eq!(
            output
                .read(
                    "fixture.mkv.native-host-observation.json",
                    color::REPORT_LIMIT
                )
                .unwrap(),
            report
        );
        let raw = output
            .read(&retained.path, NATIVE_HOST_CUSTODY_LIMIT)
            .unwrap();
        assert_eq!(retained.sha256, sha256(&raw));
        assert_eq!(retained.bytes, raw.len() as u64);
        let value: Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(value["schema"], "semwright.mlt-native-host-custody/1");
        assert_eq!(value["runtime_job_id"], "original-runtime-job-1");
        assert_eq!(value["render_job"], "render:original-custody");
        assert_eq!(value["request"]["sha256"], sha256(&request));
        assert_eq!(value["terminal"]["sha256"], sha256(&stdout));
        assert_eq!(value["original_observation"]["sha256"], sha256(&report));
        assert_eq!(
            value["stage"],
            "original-host-terminal-before-final-publication"
        );
        assert_eq!(value["final_video_commit_observed"], false);
        assert_eq!(value["native_execution_retried"], false);
        assert_eq!(
            value["publication_plan"]["scope"],
            "planned-destinations-not-commit-observations"
        );
        assert_eq!(value["publication_plan"]["decoded_frame_count"], 1);
        assert_eq!(
            value["publication_plan"]["frame_descriptors"],
            "original_observation.frames"
        );
        for forbidden in ["pid", "argv", "environment", "stderr"] {
            assert!(value.get(forbidden).is_none());
        }
        assert!(!output.exists("fixture.mkv").unwrap());
    }
    #[test]
    fn host_custody_rejects_substituted_request_or_terminal_before_writes() {
        let (_work_directory, work, request) = original_request_fixture();
        let (_report, descriptor) = complete_report_fixture(&work, &request);
        let stdout = custody_terminal(&request, &descriptor);
        let mut changed_request = request.clone();
        changed_request.push(b' ');
        let mut bad_hash: Value = serde_json::from_slice(&stdout).unwrap();
        bad_hash["request_sha256"] = json!("f".repeat(64));
        let mut extra: Value = serde_json::from_slice(&stdout).unwrap();
        extra["argv"] = json!(["forbidden"]);
        let cases = [
            (changed_request, stdout.clone()),
            (request.clone(), serde_json::to_vec(&bad_hash).unwrap()),
            (request.clone(), serde_json::to_vec(&extra).unwrap()),
        ];
        for (input, terminal) in cases {
            let directory = PrivateDir::new(std::path::Path::new("/tmp")).unwrap();
            let output = Root::open(directory.path(), true, true).unwrap();
            assert!(retain_fixture_custody(&work, &output, &input, &terminal).is_err());
            for suffix in ["request", "terminal", "observation", "custody"] {
                assert!(
                    !output
                        .exists(&format!("fixture.mkv.native-host-{suffix}.json"))
                        .unwrap()
                );
            }
        }
    }
    #[test]
    fn host_custody_never_replaces_existing_originals() {
        let (_work_directory, work, request) = original_request_fixture();
        let (_report, descriptor) = complete_report_fixture(&work, &request);
        let stdout = custody_terminal(&request, &descriptor);
        let directory = PrivateDir::new(std::path::Path::new("/tmp")).unwrap();
        let output = Root::open(directory.path(), true, true).unwrap();
        retain_fixture_custody(&work, &output, &request, &stdout).unwrap();
        let paths = ["request", "terminal", "observation", "custody"];
        let before = paths
            .iter()
            .map(|suffix| {
                output
                    .read(
                        &format!("fixture.mkv.native-host-{suffix}.json"),
                        color::REPORT_LIMIT,
                    )
                    .unwrap()
            })
            .collect::<Vec<_>>();
        let error = retain_fixture_custody(&work, &output, &request, &stdout).unwrap_err();
        assert!(error.outcome_known);
        for (suffix, bytes) in paths.iter().zip(before) {
            assert_eq!(
                output
                    .read(
                        &format!("fixture.mkv.native-host-{suffix}.json"),
                        color::REPORT_LIMIT
                    )
                    .unwrap(),
                bytes
            );
        }
        assert!(preflight_native_host_custody(&output, "fixture.mkv").is_err());
    }
    #[test]
    fn host_custody_failure_preserves_original_native_terminal() {
        for state in [
            State::Succeeded,
            State::Failed,
            State::Cancelled,
            State::Unknown,
        ] {
            for known in [false, true] {
                let mut snapshot = custody_snapshot();
                snapshot.state = state;
                if state != State::Succeeded {
                    let mut native = semwright_mlt_video::Error::new(
                        if state == State::Cancelled {
                            "Cancelled"
                        } else {
                            "BackendFailed"
                        },
                        "the original native cause",
                    );
                    native.outcome_known = state != State::Unknown;
                    snapshot.error = Some(native);
                }
                let before = from_internal(snapshot.json()).unwrap();
                let mut secondary = Error::new(ErrorCode::BackendFailed, "secondary custody cause");
                secondary.outcome_known = known;
                record_original_host_custody(&mut snapshot, Err(secondary));
                let mut after = from_internal(snapshot.json()).unwrap();
                let retained = after
                    .as_object_mut()
                    .unwrap()
                    .remove("native_host_custody")
                    .unwrap();
                assert_eq!(after, before);
                assert_eq!(
                    retained["state"],
                    if known {
                        "failed_known"
                    } else {
                        "outcome_unknown"
                    }
                );
                assert_eq!(retained["error"]["outcome_known"], known);
                assert_eq!(retained["artifact"], Value::Null);
                assert!(retained["error"].get("message").is_none());
            }
        }
    }
    #[test]
    fn host_custody_is_absent_from_other_profiles() {
        for profile in ["lossless", "h264-high", "lossless-video-only"] {
            let mut snapshot = custody_snapshot();
            snapshot.profile = profile.into();
            let value = from_internal(snapshot.json()).unwrap();
            assert!(value.get("native_host_custody").is_none());
            assert!(value.get("native_input").is_some());
            assert!(value.get("color_observation").is_some());
        }
    }
    #[test]
    fn host_custody_keeps_partial_publication_and_requires_new_destination() {
        let (_work_directory, work, request) = original_request_fixture();
        // A real 16 x 16 RGBA8 PNG exercises the production decoder before
        // cancellation; synthetic metadata alone cannot publish this artifact.
        let png = hex::decode(
            "89504e470d0a1a0a0000000d49484452000000100000001008060000001ff3ff610000001949444154789c631094d5fe4f0966183560d4805103868b010080e75810a5d6b5980000000049454e44ae426082"
        ).unwrap();
        assert_eq!(
            sha256(&png),
            "ce8cde8f22d50a5da9396a1b3ec383a725b8cd2c8f0b83d4030758f685921687"
        );
        let (report, descriptor) = complete_report_fixture_with_png(&work, &request, Some(&png));
        let stdout = custody_terminal(&request, &descriptor);
        let directory = PrivateDir::new(std::path::Path::new("/tmp")).unwrap();
        let output = Root::open(directory.path(), true, true).unwrap();
        let retained = retain_fixture_custody(&work, &output, &request, &stdout).unwrap();
        let xml = work
            .read("project.mlt", semwright_mlt_video::xml::MAX_XML)
            .unwrap();
        output
            .write_new("output", "fixture.mkv.native-input.mlt", &xml)
            .unwrap();
        let mut checks = 0usize;
        let error = color::publish_complete_observation_checked(
            &report,
            &work,
            &output,
            "fixture.mkv",
            || {
                checks += 1;
                if checks == 3 {
                    Err(semwright_mlt_video::Error::new(
                        "Cancelled",
                        "original cancellation",
                    ))
                } else {
                    Ok(())
                }
            },
        )
        .unwrap_err();
        assert_eq!(checks, 3);
        assert_eq!(error.code, "Cancelled");
        let frame_path = "fixture.mkv.decoded-000000.png";
        let published_png = output.read(frame_path, color::PNG_LIMIT).unwrap();
        assert_eq!(published_png, png);
        let report_value: Value = serde_json::from_slice(&report).unwrap();
        assert_eq!(
            report_value["frames"][0]["png_sha256"],
            sha256(&published_png)
        );
        assert_eq!(
            report_value["frames"][0]["png_bytes"],
            published_png.len() as u64
        );
        assert_eq!(
            report_value["frames"][0]["rgba_sha256"],
            "f3da8dcf7a5442f1b542259a2f47d5e2cfaa970faec152c8c41db7efe1679a1e"
        );
        assert_eq!(
            output
                .read(
                    "fixture.mkv.native-input.mlt",
                    semwright_mlt_video::xml::MAX_XML
                )
                .unwrap(),
            xml
        );
        let capsule: Value = serde_json::from_slice(
            &output
                .read(&retained.path, NATIVE_HOST_CUSTODY_LIMIT)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            capsule["publication_plan"]["native_input"]["sha256"],
            sha256(&xml)
        );
        assert!(!output.exists("fixture.mkv").unwrap());
        assert!(!output.exists("fixture.mkv.color-observation.json").unwrap());
        assert!(preflight_native_host_custody(&output, "fixture.mkv").is_err());
        assert!(
            color::preflight_observation_output(&output, "fixture.mkv", 16, 16, 1, true).is_err()
        );
        preflight_native_host_custody(&output, "different-destination.mkv").unwrap();
        color::preflight_observation_output(&output, "different-destination.mkv", 16, 16, 1, true)
            .unwrap();
        assert_eq!(output.read(frame_path, color::PNG_LIMIT).unwrap(), png);
        assert_eq!(
            output
                .read(
                    "fixture.mkv.native-input.mlt",
                    semwright_mlt_video::xml::MAX_XML
                )
                .unwrap(),
            xml
        );
    }
}

#[cfg(test)]
mod ffv1_host_original_tests {
    use super::*;
    fn fixture() -> (PrivateDir, AvHostCustody) {
        let directory = PrivateDir::new(std::path::Path::new("/tmp")).unwrap();
        let output = std::sync::Arc::new(Root::open(directory.path(), true, true).unwrap());
        let custody = AvHostCustody {
            output,
            output_path: "first.mkv".into(),
            request: b"original-prepared-input".to_vec(),
            scope: AvHostScope::Ffv1,
        };
        (directory, custody)
    }
    #[test]
    fn ffv1_host_original_unknown_is_primary_when_custody_also_fails() {
        let original = Error::new(ErrorCode::BackendFailed, "original-native-failure").uncertain();
        let error = finish_ffv1_host_original::<()>(
            Err(original),
            Err(Error::invalid("secondary-custody")),
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::BackendFailed);
        assert_eq!(error.message, "original-native-failure");
        assert!(!error.outcome_known);
    }
    #[test]
    fn ffv1_host_original_parser_error_is_not_replaced_by_custody() {
        let error = finish_ffv1_host_original::<()>(
            Err(Error::new(
                ErrorCode::PluginProtocolError,
                "original-json-refusal",
            )),
            Err(Error::invalid("secondary-custody")),
        )
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::PluginProtocolError);
        assert_eq!(error.message, "original-json-refusal");
    }
    #[test]
    fn ffv1_host_success_with_unretained_originals_is_uncertain() {
        let error =
            finish_ffv1_host_original(Ok(1), Err(Error::invalid("custody-refusal"))).unwrap_err();
        assert!(!error.outcome_known);
    }
    #[test]
    fn ffv1_discover_keeps_actual_empty_input_and_binary_buffers_before_parse() {
        let (_directory, custody) = fixture();
        custody.catalog_stdin(b"").unwrap();
        custody
            .record(
                "catalog",
                None,
                "completed",
                Some((2, b"\xff\0bad-json", b"\0native-error")),
                None,
            )
            .unwrap();
        assert_eq!(
            custody
                .output
                .read("first.mkv.ffv1-catalog.stdin.original.bin", 0)
                .unwrap(),
            b""
        );
        assert_eq!(
            custody
                .output
                .read("first.mkv.ffv1-catalog.stdout.original.bin", 262144)
                .unwrap(),
            b"\xff\0bad-json"
        );
        let raw = custody
            .output
            .read("first.mkv.ffv1-catalog.terminal.original.json", 65536)
            .unwrap();
        let saved: Value = serde_json::from_slice(&raw).unwrap();
        assert!(saved["runtime_job_id"].is_null());
        assert_eq!(saved["runtime_job_id_exposed"], false);
        assert_eq!(saved["host_exit_code"], 2);
        assert_eq!(saved["stream_eof_attested"], false);
    }
    #[test]
    fn ffv1_unobserved_host_failure_does_not_invent_empty_streams_or_exit() {
        let (_directory, custody) = fixture();
        let original = Error::new(ErrorCode::BackendFailed, "original-host-error").uncertain();
        custody
            .record(
                "catalog",
                None,
                "host-returned-error",
                None,
                Some(&original),
            )
            .unwrap();
        let raw = custody
            .output
            .read("first.mkv.ffv1-catalog.terminal.original.json", 65536)
            .unwrap();
        let saved: Value = serde_json::from_slice(&raw).unwrap();
        assert!(saved["stdout"].is_null());
        assert!(saved["stderr"].is_null());
        assert!(saved["host_exit_code"].is_null());
        assert!(
            !custody
                .output
                .exists("first.mkv.ffv1-catalog.stdout.original.bin")
                .unwrap()
        );
        assert_eq!(saved["original_error"]["outcome_known"], false);
    }
}

#[cfg(test)]
mod audio_derivative_host_tests {
    use super::*;

    fn fixture() -> (PrivateDir, App, Value) {
        let directory = PrivateDir::new(std::path::Path::new("/tmp")).unwrap();
        let output = std::sync::Arc::new(Root::open(directory.path(), true, true).unwrap());
        let app = App::new(BTreeMap::from([("output".into(), output)]), None).unwrap();
        let args = serde_json::json!({
            "profile":"pcm16-stereo48-to-mono16-v1",
            "source":{"root":"media","path":"source.wav","sha256":"a".repeat(64),
                "size_bytes":4844,"sample_rate":48000,"channels":2,"sample_frames":1200},
            "output_path":"voice.wav"
        });
        (directory, app, args)
    }

    #[test]
    fn audio_host_retains_the_same_request_and_has_no_ffv1_namespace() {
        let (_directory, app, args) = fixture();
        let descriptor = app
            .capabilities
            .iter()
            .find(|cap| cap.name == "driver.mlt-video.audio.derive")
            .unwrap()
            .digest
            .clone();
        let custody =
            AvHostCustody::new_audio(&app, "driver.mlt-video.audio.derive", &descriptor, &args)
                .unwrap();
        let actual = custody
            .output
            .read("voice.wav.audio-derive-host.stdin.original.bin", 65536)
            .unwrap();
        assert_eq!(actual, custody.request);
        let request: Value = serde_json::from_slice(&actual).unwrap();
        assert_eq!(request["command"], "driver.mlt-video.audio.derive");
        assert_eq!(request["descriptor_sha256"], descriptor);
        assert_eq!(request["args"], args);
        assert_eq!(custody.namespace(), "audio-derive");
        assert!(
            !custody
                .output
                .exists("voice.wav.ffv1-host.stdin.original.bin")
                .unwrap()
        );
        assert!(
            AvHostCustody::new_audio(&app, "driver.mlt-video.frames.encode", &descriptor, &args)
                .is_err()
        );
    }

    #[test]
    fn audio_host_preserves_binary_pipes_and_nonzero_before_any_json_parse() {
        let (_directory, app, args) = fixture();
        let custody = AvHostCustody::new_audio(
            &app,
            "driver.mlt-video.audio.derive",
            &"a".repeat(64),
            &args,
        )
        .unwrap();
        custody
            .record(
                "host",
                None,
                "completed",
                Some((2, b"\xff\0not-json", b"original stderr\0")),
                None,
            )
            .unwrap();
        assert_eq!(
            custody
                .output
                .read("voice.wav.audio-derive-host.stdout.original.bin", 262144)
                .unwrap(),
            b"\xff\0not-json"
        );
        assert_eq!(
            custody
                .output
                .read("voice.wav.audio-derive-host.stderr.original.bin", 262144)
                .unwrap(),
            b"original stderr\0"
        );
        let original: Value = serde_json::from_slice(
            &custody
                .output
                .read("voice.wav.audio-derive-host.terminal.original.json", 65536)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            original["schema"],
            "semwright.audio-derivative-host-original/1"
        );
        assert_eq!(original["host_exit_code"], 2);
        assert_eq!(original["stream_eof_attested"], false);
        assert_eq!(original["native_execution_retried"], false);
    }

    #[test]
    fn absent_host_result_stays_absent_and_keeps_the_original_unknown_error() {
        let (_directory, app, args) = fixture();
        let custody = AvHostCustody::new_audio(
            &app,
            "driver.mlt-video.audio.derive",
            &"a".repeat(64),
            &args,
        )
        .unwrap();
        let original =
            Error::new(ErrorCode::BackendFailed, "original interrupted Host wait").uncertain();
        let before = serde_json::to_value(&original).unwrap();
        let retained = custody.record("host", None, "wait-returned-error", None, Some(&original));
        let error = finish_ffv1_host_original::<()>(Err(original), retained).unwrap_err();
        assert_eq!(serde_json::to_value(&error).unwrap(), before);
        let terminal: Value = serde_json::from_slice(
            &custody
                .output
                .read("voice.wav.audio-derive-host.terminal.original.json", 65536)
                .unwrap(),
        )
        .unwrap();
        assert!(terminal["stdout"].is_null());
        assert!(terminal["stderr"].is_null());
        assert!(terminal["host_exit_code"].is_null());
        assert_eq!(terminal["original_error"], before);
        assert!(
            !custody
                .output
                .exists("voice.wav.audio-derive-host.stdout.original.bin")
                .unwrap()
        );
    }

    #[test]
    fn failed_terminal_publication_does_not_replace_original_unknown_or_overwrite() {
        let (_directory, app, args) = fixture();
        let custody = AvHostCustody::new_audio(
            &app,
            "driver.mlt-video.audio.derive",
            &"a".repeat(64),
            &args,
        )
        .unwrap();
        custody
            .output
            .write_new(
                "output",
                "voice.wav.audio-derive-host.terminal.original.json",
                b"kept original",
            )
            .unwrap();
        let original = Error::new(ErrorCode::Cancelled, "original cancellation").uncertain();
        let before = serde_json::to_value(&original).unwrap();
        let retained = custody.record(
            "host",
            None,
            "completed",
            Some((7, b"", b"native failure")),
            Some(&original),
        );
        assert!(retained.is_err());
        let error = finish_ffv1_host_original::<()>(Err(original), retained).unwrap_err();
        assert_eq!(serde_json::to_value(&error).unwrap(), before);
        assert_eq!(
            custody
                .output
                .read("voice.wav.audio-derive-host.terminal.original.json", 65536)
                .unwrap(),
            b"kept original"
        );
        assert_eq!(
            custody
                .output
                .read("voice.wav.audio-derive-host.stdout.original.bin", 262144)
                .unwrap(),
            b""
        );
        assert_eq!(
            custody
                .output
                .read("voice.wav.audio-derive-host.stderr.original.bin", 262144)
                .unwrap(),
            b"native failure"
        );
    }

    #[test]
    fn actual_sdk_catalog_keeps_audio_derivation_mutating_and_non_idempotent() {
        let caps = sdk_capabilities().unwrap();
        let cap = caps
            .iter()
            .find(|cap| cap.descriptor.name == "driver.mlt-video.audio.derive")
            .unwrap();
        let descriptor = serde_json::to_value(&cap.descriptor).unwrap();
        assert_eq!(descriptor["risk"], "mutating");
        assert_eq!(descriptor["idempotency"], "non_idempotent");
        assert_eq!(
            descriptor["backends"],
            serde_json::json!(["driver:mlt-video"])
        );
        assert_eq!(descriptor["timeout_ms"], 300000);
    }
}
