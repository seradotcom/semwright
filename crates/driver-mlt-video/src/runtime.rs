//! Pinned ELF tools and bounded subprocesses. Hosted drivers reuse DriverProvider isolation;
//! standalone execution adds Bubblewrap. No production path invokes a shell.
use crate::{
    Error, Result,
    fs::PrivateDir,
    hash::{reader_hash, valid_digest},
    json::{self, Value, array, obj},
    model::Profile,
    sync::{self, CueWindow, ProbeResult},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::unix::{
        fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
        process::{CommandExt, ExitStatusExt},
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
const RETAIN_LOG: usize = 16_384;
const TOTAL_LOG: usize = 262_144;
pub const NATIVE_DIAGNOSTIC_FILE: &str = "native-render-diagnostic.json";
pub const VALIDATION_DIAGNOSTIC_FILE: &str = "native-validation-diagnostic.json";
pub const NATIVE_DIAGNOSTIC_LIMIT: usize = 65_536;

/// Raw native logs belong only to a bounded local receipt, never a public envelope.
pub fn native_render_diagnostic(
    project: &[u8],
    args: &[OsString],
    result: &ProcessResult,
) -> Result<Vec<u8>> {
    if project.len() > crate::xml::MAX_XML || args.len() > 32 {
        return Err(Error::limit("Native diagnostic input exceeds bounds"));
    }
    let argv = args
        .iter()
        .map(|arg| {
            arg.to_str()
                .filter(|arg| arg.len() <= 4096)
                .map(str::to_owned)
                .ok_or_else(|| Error::invalid("Native diagnostic argv differs"))
        })
        .collect::<Result<Vec<_>>>()?;
    let hex = |bytes: &[u8], maximum: usize| {
        bytes
            .iter()
            .take(maximum)
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    let value = serde_json::json!({
        "schema":1,"operation":"mlt-native-render-observation",
        "project_xml_sha256":crate::hash::sha256(project),"project_xml_bytes":project.len(),
        "argv":argv,"exit_code":result.exit_code,"term_signal":result.term_signal,
        "cancelled":result.cancelled,"timed_out":result.timed_out,"output_exceeded":result.output_exceeded,
        "stdout_encoding":"hex","stdout_hex":hex(&result.stdout,4096),
        "stderr_encoding":"hex","stderr_hex":hex(&result.stderr,16384),
        "stdout_retained_bytes":result.stdout.len(),"stderr_retained_bytes":result.stderr.len(),
        "stdout_diagnostic_truncated":result.stdout.len()>4096,
        "stderr_diagnostic_truncated":result.stderr.len()>16384,
        "capture_scope":"runtime capture retains prefixes; this local receipt is not video validation or publication"
    });
    let bytes = serde_json::to_vec(&value)
        .map_err(|_| Error::invalid("Native diagnostic serialization failed"))?;
    if bytes.len() > NATIVE_DIAGNOSTIC_LIMIT {
        return Err(Error::limit("Native diagnostic receipt exceeds bounds"));
    }
    Ok(bytes)
}

pub fn native_failure_filename(output: &str, job: &str) -> Result<String> {
    crate::fs::validate_relative(output)?;
    let hex = job
        .strip_prefix("render:")
        .ok_or_else(|| Error::invalid("Native diagnostic job identity differs"))?;
    if hex.len() != 32
        || !hex
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err(Error::invalid("Native diagnostic job identity differs"));
    }
    let name = format!("{output}.native-failure-{hex}.json");
    crate::fs::validate_relative(&name)?;
    Ok(name)
}

/// Retain exact scalar fields from the first video stream, independent of audio packets.
pub fn raw_video_probe_observation(bytes: &[u8]) -> Result<serde_json::Value> {
    if bytes.len() > 262_144 {
        return Err(Error::limit("Native probe diagnostic exceeds bounds"));
    }
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|_| Error::invalid("Native probe diagnostic is malformed"))?;
    if value.get("schema").and_then(|v| v.as_u64()) != Some(1)
        || value.get("operation").and_then(|v| v.as_str()) != Some("probe")
    {
        return Err(Error::invalid("Native probe diagnostic envelope differs"));
    }
    let media = value
        .get("media")
        .ok_or_else(|| Error::invalid("Native probe media absent"))?;
    let streams = media
        .get("streams")
        .and_then(|v| v.as_array())
        .filter(|v| !v.is_empty() && v.len() <= 16)
        .ok_or_else(|| Error::invalid("Native probe streams differ"))?;
    let video = streams
        .iter()
        .find(|s| s.get("codec_type").and_then(|v| v.as_str()) == Some("video"))
        .ok_or_else(|| Error::invalid("Native probe video absent"))?;
    let select = |object: &serde_json::Value, fields: &[&str]| -> Result<serde_json::Value> {
        let mut out = serde_json::Map::new();
        for field in fields {
            if let Some(v) = object.get(*field) {
                if v.is_array() || v.is_object() || v.as_str().is_some_and(|s| s.len() > 512) {
                    return Err(Error::limit(
                        "Native probe diagnostic scalar exceeds bounds",
                    ));
                }
                out.insert((*field).to_owned(), v.clone());
            }
        }
        Ok(serde_json::Value::Object(out))
    };
    Ok(serde_json::json!({
        "raw_probe_sha256":crate::hash::sha256(bytes),"raw_probe_bytes":bytes.len(),
        "first_video_stream":select(video,&["codec_type","codec_name","width","height","nb_frames",
            "r_frame_rate","avg_frame_rate","duration","duration_ts","time_base","start_time","start_pts","pix_fmt"] )?,
        "format":select(media.get("format").unwrap_or(&serde_json::Value::Null),
            &["duration","start_time","size","format_name","format_long_name"] )?
    }))
}

// Bounded originals from the existing FFV1 Runtime. These records never grant
// authority, run another process, change arguments, or prove stream EOF.
pub const FFV1_CUSTODY_STREAM_PREFIX: usize = 16_384;
// The complete probe already captures this many bytes per stream. Retention
// must not truncate those accepted original buffers a second time.
pub const FFV1_CUSTODY_PROBE_STREAM: usize = crate::color::REPORT_LIMIT;

fn ffv1_stream_original_limit(stage: &str) -> usize {
    if stage == "complete-probe" {
        FFV1_CUSTODY_PROBE_STREAM
    } else {
        FFV1_CUSTODY_STREAM_PREFIX
    }
}
pub const FFV1_CUSTODY_HOST_STREAM: usize = 262_144;
pub const FFV1_CUSTODY_STDIN: usize = 65_536;
pub const FFV1_CUSTODY_METADATA: usize = 65_536;
pub const FFV1_CUSTODY_STAGES: [&str; 6] = [
    "zscale-help",
    "encode",
    "media-probe",
    "complete-probe",
    "decoder-version",
    "decode",
];
pub const FFV1_CUSTODY_HOST_SUFFIXES: [&str; 8] = [
    ".ffv1-host.stdin.original.bin",
    ".ffv1-host.stdout.original.bin",
    ".ffv1-host.stderr.original.bin",
    ".ffv1-host.terminal.original.json",
    ".ffv1-catalog.stdin.original.bin",
    ".ffv1-catalog.stdout.original.bin",
    ".ffv1-catalog.stderr.original.bin",
    ".ffv1-catalog.terminal.original.json",
];
const FFV1_CUSTODY_SUMMARY: &str = ".ffv1-process-custody.original.json";

pub fn ffv1_custody_paths(output_path: &str, host: bool) -> Result<Vec<String>> {
    crate::fs::validate_relative(output_path)?;
    if !output_path.ends_with(".mkv") {
        return Err(Error::invalid(
            "FFV1 custody requires the original .mkv destination",
        ));
    }
    let mut paths = Vec::with_capacity(27);
    if host {
        paths.extend(
            FFV1_CUSTODY_HOST_SUFFIXES
                .iter()
                .map(|s| format!("{output_path}{s}")),
        );
    }
    for stage in FFV1_CUSTODY_STAGES {
        for suffix in [
            "stdout.original.bin",
            "stderr.original.bin",
            "terminal.original.json",
        ] {
            paths.push(format!("{output_path}.ffv1-{stage}.{suffix}"));
        }
    }
    paths.push(format!("{output_path}{FFV1_CUSTODY_SUMMARY}"));
    for path in &paths {
        crate::fs::validate_relative(path)?;
    }
    Ok(paths)
}

pub fn preflight_ffv1_custody(output: &crate::fs::Root, path: &str, host: bool) -> Result<()> {
    if !output.readable || !output.writable {
        return Err(Error::new(
            "PermissionDenied",
            "FFV1 custody requires the existing read/write output grant",
        ));
    }
    for member in ffv1_custody_paths(path, host)? {
        if output.exists(&member)? {
            return Err(Error::new(
                "Conflict",
                "Original FFV1 custody exists; preserve it and use a new destination",
            ));
        }
    }
    // Read-only space check, never a reservation or a replacement scheduler.
    let native = 5 * 2 * FFV1_CUSTODY_STREAM_PREFIX
        + 2 * FFV1_CUSTODY_PROBE_STREAM
        + 7 * FFV1_CUSTODY_METADATA;
    let host_bytes = if host {
        2 * (FFV1_CUSTODY_STDIN + 2 * FFV1_CUSTODY_HOST_STREAM + FFV1_CUSTODY_METADATA)
    } else {
        0
    };
    if output.available_bytes_for(path)? < (native + host_bytes) as u64 {
        return Err(Error::limit("Output lacks the closed FFV1 custody budget"));
    }
    Ok(())
}

fn ffv1_custody_uncertain(message: &str) -> Error {
    let mut error = Error::new("BackendFailed", message);
    error.outcome_known = false;
    error
}

/// Root::write_new uses the existing FD-relative NOREPLACE/fsync publisher and
/// admits empty originals. publish_verified intentionally rejects zero bytes.
pub fn retain_ffv1_original(
    output: &crate::fs::Root,
    path: &str,
    bytes: &[u8],
    maximum: usize,
) -> Result<crate::fs::Artifact> {
    retain_process_original(output, path, bytes, maximum, "FFV1")
}

fn retain_process_original(
    output: &crate::fs::Root,
    path: &str,
    bytes: &[u8],
    maximum: usize,
    scope: &str,
) -> Result<crate::fs::Artifact> {
    if bytes.len() > maximum {
        return Err(Error::limit(format!(
            "{scope} original exceeds its closed retention budget"
        )));
    }
    let uncertain = |detail: &str| {
        let mut error = Error::new("BackendFailed", format!("Original {scope} {detail}"));
        error.outcome_known = false;
        error
    };
    let file = output.write_new("output", path, bytes)?;
    if file.bytes != bytes.len() as u64 || file.sha256 != crate::hash::sha256(bytes) {
        return Err(uncertain("publication identity differs"));
    }
    let actual = output
        .read(path, maximum)
        .map_err(|_| uncertain("custody readback is uncertain"))?;
    if actual != bytes {
        return Err(uncertain("custody readback differs"));
    }
    Ok(file)
}

fn ffv1_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn ffv1_spec_original(spec: &ProcessSpec) -> Result<serde_json::Value> {
    process_spec_original(spec, "FFV1")
}

fn process_spec_original(spec: &ProcessSpec, scope: &str) -> Result<serde_json::Value> {
    use std::os::unix::ffi::OsStrExt;
    if spec.args.len() > 128 {
        return Err(Error::limit(format!("{scope} original argv count")));
    }
    let mut total = 0usize;
    let mut argv = Vec::with_capacity(spec.args.len());
    for arg in &spec.args {
        let bytes = arg.as_os_str().as_bytes();
        total = total
            .checked_add(bytes.len())
            .ok_or_else(|| Error::limit(format!("{scope} argv overflow")))?;
        if bytes.len() > 4096 || total > 16_384 {
            return Err(Error::limit(format!("{scope} original argv byte budget")));
        }
        argv.push(ffv1_hex(bytes));
    }
    let executable = spec.executable.as_os_str().as_bytes();
    let cwd = spec.cwd.as_os_str().as_bytes();
    if executable.len() > 4096 || cwd.len() > 4096 {
        return Err(Error::limit(format!(
            "{scope} original process path budget"
        )));
    }
    Ok(serde_json::json!({
        "encoding":"unix-os-bytes-hex", "executable":ffv1_hex(executable),
        "argv":argv, "cwd":ffv1_hex(cwd), "timeout_millis":spec.timeout.as_millis(),
        "cpu_seconds":spec.cpu_seconds, "address_space_bytes":spec.address_space_bytes,
        "environment_included":false, "native_execution_retried":false
    }))
}

#[doc(hidden)]
pub struct Ffv1ProcessCustody<'a> {
    kind: crate::originals::VideoOriginalKind,
    prefix: String,
    work: Option<&'a crate::fs::Root>,
    output: &'a crate::fs::Root,
    output_path: &'a str,
    manifest_sha256: &'a str,
    stages: std::cell::RefCell<Vec<&'static str>>,
    failures: std::cell::RefCell<Vec<&'static str>>,
    retention_errors: std::cell::RefCell<Vec<serde_json::Value>>,
}

impl<'a> Ffv1ProcessCustody<'a> {
    #[cfg(test)]
    pub(crate) fn new(
        output: &'a crate::fs::Root,
        output_path: &'a str,
        manifest_sha256: &'a str,
    ) -> Result<Self> {
        preflight_ffv1_custody(output, output_path, false)?;
        Self::make(
            output,
            output_path,
            manifest_sha256,
            crate::originals::VideoOriginalKind::MotionFrames,
            "ffv1".into(),
            None,
        )
    }

    pub fn with_owned_work(
        output: &'a crate::fs::Root,
        output_path: &'a str,
        source_sha256: &'a str,
        kind: crate::originals::VideoOriginalKind,
        work: &'a crate::fs::Root,
    ) -> Result<Self> {
        Self::with_owned_work_prefix(
            output,
            output_path,
            source_sha256,
            kind,
            kind.prefix().into(),
            work,
        )
    }

    pub fn for_input_probe(
        output: &'a crate::fs::Root,
        output_path: &'a str,
        source_sha256: &'a str,
        slot: usize,
        work: &'a crate::fs::Root,
    ) -> Result<Self> {
        if slot >= 1_000_000 {
            return Err(Error::limit("Input-probe slot exceeds its original roster"));
        }
        Self::with_owned_work_prefix(
            output,
            output_path,
            source_sha256,
            crate::originals::VideoOriginalKind::InputProbe,
            format!("native-input-probe-{slot:06}"),
            work,
        )
    }

    fn with_owned_work_prefix(
        output: &'a crate::fs::Root,
        output_path: &'a str,
        source_sha256: &'a str,
        kind: crate::originals::VideoOriginalKind,
        prefix: String,
        work: &'a crate::fs::Root,
    ) -> Result<Self> {
        // The shared VideoOriginals preflight already declared and checked all
        // output destinations and the complete raw-custody peak. Work backup
        // names are closed by the same internal role roster.
        if !work.readable || !work.writable {
            return Err(Error::new(
                "PermissionDenied",
                "Owned work Root lacks custody access",
            ));
        }
        for stage in kind.stages() {
            for suffix in [
                "stdout.original.bin",
                "stderr.original.bin",
                "terminal.original.json",
            ] {
                if work.exists(&format!("{prefix}-{stage}.{suffix}"))? {
                    return Err(Error::new(
                        "Conflict",
                        "Original native process backup already exists",
                    ));
                }
            }
        }
        if work.exists(&format!("{prefix}-process-custody.original.json"))? {
            return Err(Error::new(
                "Conflict",
                "Original native process summary already exists",
            ));
        }
        Self::make(output, output_path, source_sha256, kind, prefix, Some(work))
    }

    fn make(
        output: &'a crate::fs::Root,
        output_path: &'a str,
        manifest_sha256: &'a str,
        kind: crate::originals::VideoOriginalKind,
        prefix: String,
        work: Option<&'a crate::fs::Root>,
    ) -> Result<Self> {
        if !valid_digest(manifest_sha256) {
            return Err(Error::invalid("Native custody source identity"));
        }
        Ok(Self {
            kind,
            prefix,
            work,
            output,
            output_path,
            manifest_sha256,
            stages: std::cell::RefCell::new(Vec::new()),
            failures: std::cell::RefCell::new(Vec::new()),
            retention_errors: std::cell::RefCell::new(Vec::new()),
        })
    }

    fn retain_work(&self, basename: &str, bytes: &[u8], maximum: usize) {
        if let Some(work) = self.work {
            let retained = (|| -> Result<()> {
                if bytes.len() > maximum {
                    return Err(Error::limit(
                        "Owned process original exceeds its closed bound",
                    ));
                }
                let artifact = work.write_new("owned-work", basename, bytes)?;
                if artifact.bytes != bytes.len() as u64
                    || artifact.sha256 != crate::hash::sha256(bytes)
                    || work.read(basename, maximum)? != bytes
                {
                    return Err(ffv1_custody_uncertain(
                        "Owned process original readback differs",
                    ));
                }
                Ok(())
            })();
            if let Err(error) = retained {
                self.failed_error("owned-work-backup", &error);
            }
        }
    }

    pub fn original_capture_complete(&self) -> bool {
        self.failures.borrow().is_empty()
    }

    fn failed(&self, code: &'static str) {
        let mut failures = self.failures.borrow_mut();
        if failures.len() < 32 {
            failures.push(code);
        }
    }

    pub fn failed_error(&self, phase: &'static str, error: &Error) {
        self.failed(phase);
        // Calls are bounded by the closed process stage roster. Keep the real
        // error rather than reducing a publication/readback failure to bool.
        self.retention_errors.borrow_mut().push(serde_json::json!({
            "phase":phase,"code":error.code,"message":error.message,
            "outcome_known":error.outcome_known,
        }));
    }

    pub fn secondary_report(&self) -> Option<serde_json::Value> {
        if self.failures.borrow().is_empty() {
            return None;
        }
        Some(serde_json::json!({
            "member_prefix":self.prefix,"source_sha256":self.manifest_sha256,
            "secondary_failures":*self.failures.borrow(),
            "retention_errors":*self.retention_errors.borrow(),
            "observed_after_summary_retention":true,
        }))
    }

    fn stream(&self, stage: &str, name: &str, bytes: &[u8]) -> serde_json::Value {
        let limit = ffv1_stream_original_limit(stage);
        let basename = format!("{}-{stage}.{name}.original.bin", self.prefix);
        self.retain_work(&basename, bytes, limit);
        let path = format!("{}.{basename}", self.output_path);
        let retention = retain_ffv1_original(self.output, &path, bytes, limit);
        if let Err(error) = &retention {
            self.failed_error("stream-retention", error);
        }
        let retained = retention.is_ok();
        serde_json::json!({
            "root":"output", "path":path, "sha256":crate::hash::sha256(bytes),
            "bytes":bytes.len(), "returned_buffer_bytes":bytes.len(),
            "returned_buffer_sha256":crate::hash::sha256(bytes),
            "diagnostic_prefix_truncated":false, "retention_limit":limit, "retained":retained,
            "upstream_stream_eof_attested":false
        })
    }

    #[cfg(test)]
    fn record(
        &self,
        stage: &'static str,
        spec: &ProcessSpec,
        result: &Result<ProcessResult>,
        retained_limit: usize,
        total_limit: usize,
    ) {
        self.record_observed(stage, spec, result, retained_limit, total_limit, None, None);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_observed(
        &self,
        stage: &'static str,
        spec: &ProcessSpec,
        result: &Result<ProcessResult>,
        retained_limit: usize,
        total_limit: usize,
        observation: Option<&ProcessObservation>,
        file_size_limit: Option<u64>,
    ) {
        let next = self.stages.borrow().len();
        if self.kind.stages().get(next).copied() != Some(stage) {
            self.failed("stage-order");
            return;
        }
        self.stages.borrow_mut().push(stage);
        if let Some(observed) = observation {
            let failed_pipe =
                |pipe: &CapturedPipe| pipe.panicked || pipe.stopped || pipe.read_error.is_some();
            if observed.drain_stop_reason.is_some()
                || observed.stdout.as_ref().is_some_and(failed_pipe)
                || observed.stderr.as_ref().is_some_and(failed_pipe)
                || (observed.spawn_attempted
                    && (observed.stdout.is_none() || observed.stderr.is_none()))
            {
                self.failed("observed-stream-capture-incomplete");
            }
        }
        let spec = match ffv1_spec_original(spec) {
            Ok(spec) => spec,
            Err(_) => {
                self.failed("spec-retention");
                serde_json::Value::Null
            }
        };
        let terminal = match result {
            Ok(result) => serde_json::json!({
                "process_result_received":true, "exit_code":result.exit_code,
                "term_signal":result.term_signal, "cancelled":result.cancelled,
                "timed_out":result.timed_out, "output_exceeded":result.output_exceeded,
                "stdout":self.stream(stage,"stdout",&result.stdout),
                "stderr":self.stream(stage,"stderr",&result.stderr), "original_error":null
            }),
            Err(error) => serde_json::json!({
                "process_result_received":false, "exit_code":null, "term_signal":null,
                "cancelled":null, "timed_out":null, "output_exceeded":null,
                "stdout":observation.and_then(|value| value.stdout.as_ref()).map(|stream| self.stream(stage,"stdout",&stream.retained)),
                "stderr":observation.and_then(|value| value.stderr.as_ref()).map(|stream| self.stream(stage,"stderr",&stream.retained)),
                "original_error":{"code":error.code,"message":error.message,"outcome_known":error.outcome_known}
            }),
        };
        let document = serde_json::json!({
            "schema":if matches!(self.kind,crate::originals::VideoOriginalKind::MotionFrames) {"semwright.ffv1-process-original/1"} else {"semwright.native-video-process-original/1"}, "stage":stage,"role":self.kind.prefix(),"member_prefix":self.prefix,
            "output_path":self.output_path,"source_manifest_sha256":if matches!(self.kind,crate::originals::VideoOriginalKind::MotionFrames) {Some(self.manifest_sha256)} else {None},
            "source_sha256":self.manifest_sha256,
            "source_kind":match self.kind {
                crate::originals::VideoOriginalKind::MotionFrames => "motion-frame-manifest-bytes",
                crate::originals::VideoOriginalKind::NativeRender
                | crate::originals::VideoOriginalKind::HostNativeRender => "native-render-xml-bytes",
                crate::originals::VideoOriginalKind::InputProbe => "staged-input-bytes",
                crate::originals::VideoOriginalKind::AvAudioDecode => "staged-av-master-bytes",
            },
            "source_capture_retained_limit":retained_limit,"source_capture_total_limit":total_limit,
            "diagnostic_prefix_limit":ffv1_stream_original_limit(stage),"spec":spec,"terminal":terminal,
            "same_spawn_observation":observation.map(ProcessObservation::metadata),
            "file_size_limit_bytes":file_size_limit,
            "native_execution_retried":false,"stream_eof_attested":false,
            "capture_scope":"same-spawn-retained-buffers-before-checked-or-parse",
            "physical_acceptance":false
        });
        let bytes = match serde_json::to_vec(&document) {
            Ok(bytes) => bytes,
            Err(_) => {
                self.failed("terminal-serialization");
                return;
            }
        };
        let basename = format!("{}-{stage}.terminal.original.json", self.prefix);
        self.retain_work(&basename, &bytes, FFV1_CUSTODY_METADATA);
        let path = format!("{}.{basename}", self.output_path);
        if let Err(error) = retain_ffv1_original(self.output, &path, &bytes, FFV1_CUSTODY_METADATA)
        {
            self.failed_error("terminal-retention", &error);
        }
    }

    pub fn complete<T>(&self, original: Result<T>) -> Result<T> {
        if original.is_ok() && self.stages.borrow().as_slice() != self.kind.stages() {
            self.failed("stage-coverage");
        }
        let document = serde_json::json!({
            "schema":if matches!(self.kind,crate::originals::VideoOriginalKind::MotionFrames) {"semwright.ffv1-process-custody/1"} else {"semwright.native-video-process-custody/1"}, "output_path":self.output_path,"role":self.kind.prefix(),"member_prefix":self.prefix,
            "source_manifest_sha256":if matches!(self.kind,crate::originals::VideoOriginalKind::MotionFrames) {Some(self.manifest_sha256)} else {None},
            "source_sha256":self.manifest_sha256,
            "source_kind":match self.kind {
                crate::originals::VideoOriginalKind::MotionFrames => "motion-frame-manifest-bytes",
                crate::originals::VideoOriginalKind::NativeRender
                | crate::originals::VideoOriginalKind::HostNativeRender => "native-render-xml-bytes",
                crate::originals::VideoOriginalKind::InputProbe => "staged-input-bytes",
                crate::originals::VideoOriginalKind::AvAudioDecode => "staged-av-master-bytes",
            },
            "observed_stages":*self.stages.borrow(), "secondary_failures":*self.failures.borrow(),
            "retention_errors":*self.retention_errors.borrow(),
            "observation_boundary":"before-this-summary-retention",
            "original_operation_succeeded":original.is_ok(),
            "original_error":original.as_ref().err().map(|e| serde_json::json!({"code":e.code,"message":e.message,"outcome_known":e.outcome_known})),
            "stream_eof_attested":false,"native_execution_retried":false,"physical_acceptance":false
        });
        let basename = format!("{}-process-custody.original.json", self.prefix);
        let retained = serde_json::to_vec(&document)
            .map_err(|error| {
                Error::invalid(format!("Process summary serialization failed: {error}"))
            })
            .and_then(|bytes| {
                self.retain_work(&basename, &bytes, FFV1_CUSTODY_METADATA);
                retain_ffv1_original(
                    self.output,
                    &format!("{}.{basename}", self.output_path),
                    &bytes,
                    FFV1_CUSTODY_METADATA,
                )
            });
        if let Err(error) = retained {
            self.failed_error("summary-retention", &error);
        }
        // A custody failure must never replace the original native/domain error.
        match original {
            Err(error) => Err(error),
            Ok(value) if self.failures.borrow().is_empty() => Ok(value),
            Ok(_) => Err(ffv1_custody_uncertain(
                "FFV1 execution returned but original custody is incomplete",
            )),
        }
    }
}

#[cfg(test)]
fn checked_ffv1_original(
    custody: Option<&Ffv1ProcessCustody<'_>>,
    stage: &'static str,
    spec: &ProcessSpec,
    result: Result<ProcessResult>,
    retained_limit: usize,
    total_limit: usize,
) -> Result<ProcessResult> {
    if let Some(custody) = custody {
        custody.record(stage, spec, &result, retained_limit, total_limit);
    }
    result?.checked()
}

// The original process is executed exactly once. Its independently captured
// partial pipes are available even when the supervisor returns Err rather than
// a ProcessResult. Existing native error classification remains primary.
fn run_checked_ffv1_original(
    custody: Option<&Ffv1ProcessCustody<'_>>,
    stage: &'static str,
    spec: &ProcessSpec,
    cancel: &AtomicBool,
    retained_limit: usize,
    total_limit: usize,
    file_size_limit: u64,
) -> Result<ProcessResult> {
    let mut observation = ProcessObservation::default();
    let result = run_with_file_limit_observed(
        spec,
        cancel,
        retained_limit,
        total_limit,
        file_size_limit,
        custody.map(|_| &mut observation),
    );
    if let Some(custody) = custody {
        custody.record_observed(
            stage,
            spec,
            &result,
            retained_limit,
            total_limit,
            Some(&observation),
            Some(file_size_limit),
        );
    }
    checked_ffv1_process_observation(result, &observation)
}

#[doc(hidden)]
pub fn checked_ffv1_process_observation(
    result: Result<ProcessResult>,
    observation: &ProcessObservation,
) -> Result<ProcessResult> {
    let result = result?;
    // Preserve the legacy wait -> stdout panic -> stderr panic -> native flags
    // error precedence after retaining the actual same-spawn observations.
    if observation
        .stdout
        .as_ref()
        .is_some_and(|pipe| pipe.panicked)
    {
        return Err(Error::new("Internal", "stdout reader failed"));
    }
    if observation
        .stderr
        .as_ref()
        .is_some_and(|pipe| pipe.panicked)
    {
        return Err(Error::new("Internal", "stderr reader failed"));
    }
    result.checked()
}

pub const AUDIO_DERIVATIVE_STDIN_LIMIT: usize = 65_536;
pub const AUDIO_DERIVATIVE_METADATA_LIMIT: usize = 65_536;
pub const AUDIO_DERIVATIVE_STREAM_LIMIT: usize = 262_144;
pub const AUDIO_DERIVATIVE_COMMAND: &str = "driver.mlt-video.audio.derive";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AudioDerivativeProfile {
    Stereo48ToMono16V1,
    Mono16ToDualMono48V1,
}

impl AudioDerivativeProfile {
    pub(crate) fn parse(value: &str) -> Result<Self> {
        match value {
            "pcm16-stereo48-to-mono16-v1" => Ok(Self::Stereo48ToMono16V1),
            "pcm16-mono16-to-dualmono48-v1" => Ok(Self::Mono16ToDualMono48V1),
            _ => Err(Error::unsupported(
                "Audio derivative profile is not supported",
            )),
        }
    }

    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::Stereo48ToMono16V1 => "pcm16-stereo48-to-mono16-v1",
            Self::Mono16ToDualMono48V1 => "pcm16-mono16-to-dualmono48-v1",
        }
    }

    pub(crate) fn input_limit(self) -> usize {
        match self {
            Self::Stereo48ToMono16V1 => 2 << 20,
            Self::Mono16ToDualMono48V1 => 6 << 20,
        }
    }

    pub(crate) fn output_limit(self) -> usize {
        match self {
            Self::Stereo48ToMono16V1 => 1 << 20,
            Self::Mono16ToDualMono48V1 => 40 << 20,
        }
    }

    pub(crate) fn source_layout(self) -> (u32, u16) {
        match self {
            Self::Stereo48ToMono16V1 => (48_000, 2),
            Self::Mono16ToDualMono48V1 => (16_000, 1),
        }
    }

    pub(crate) fn target_layout(self) -> (u32, u16) {
        match self {
            Self::Stereo48ToMono16V1 => (16_000, 1),
            Self::Mono16ToDualMono48V1 => (48_000, 2),
        }
    }

    pub(crate) fn output_frames(self, source_frames: u64) -> Result<u64> {
        match self {
            Self::Stereo48ToMono16V1
                if (1_200..=384_000).contains(&source_frames)
                    && source_frames.is_multiple_of(3) =>
            {
                Ok(source_frames / 3)
            }
            Self::Mono16ToDualMono48V1 if (16_000..=2_880_000).contains(&source_frames) => {
                source_frames
                    .checked_mul(3)
                    .ok_or_else(|| Error::limit("Audio frame count overflow"))
            }
            _ => Err(Error::unsupported(
                "Audio derivative duration or integral sample clock differs",
            )),
        }
    }

    fn filter(self) -> &'static str {
        match self {
            Self::Stereo48ToMono16V1 => {
                "pan=mono|c0=0.5*c0+0.5*c1,aresample=16000:async=0:dither_method=none"
            }
            Self::Mono16ToDualMono48V1 => {
                "pan=stereo|c0=c0|c1=c0,aresample=48000:async=0:dither_method=none"
            }
        }
    }

    pub(crate) fn recipe_sha256(self) -> Result<String> {
        let args = audio_derivative_args(self, Path::new("SOURCE_PCM"), Path::new("TARGET_PCM"));
        let template = args
            .iter()
            .map(|arg| {
                arg.to_str()
                    .ok_or_else(|| Error::invalid("Audio recipe template is not UTF-8"))
            })
            .collect::<Result<Vec<_>>>()?;
        let bytes = serde_json::to_vec(&serde_json::json!({
            "schema":"semwright.mlt-audio-recipe/1", "profile":self.id(), "argv_template":template
        }))
        .map_err(|_| Error::invalid("Audio recipe serialization failed"))?;
        Ok(crate::hash::sha256(&bytes))
    }

    pub(crate) fn inspect_source(self, bytes: &[u8], frames: u64) -> Result<AudioPcmObservation> {
        self.output_frames(frames)?;
        inspect_audio_pcm16(
            bytes,
            self.input_limit(),
            self.source_layout(),
            frames,
            false,
        )
    }

    pub(crate) fn inspect_target(
        self,
        bytes: &[u8],
        source_frames: u64,
    ) -> Result<AudioPcmObservation> {
        let frames = self.output_frames(source_frames)?;
        inspect_audio_pcm16(
            bytes,
            self.output_limit(),
            self.target_layout(),
            frames,
            self == Self::Mono16ToDualMono48V1,
        )
    }
}

#[derive(Clone, Debug)]
pub(crate) struct AudioPcmObservation {
    pub(crate) sample_rate: u32,
    pub(crate) channels: u16,
    pub(crate) channel_mask: Option<u32>,
    pub(crate) sample_frames: u64,
}

impl AudioPcmObservation {
    pub(crate) fn json(&self) -> Value {
        obj([
            ("sample_rate", u64::from(self.sample_rate).into()),
            ("channels", u64::from(self.channels).into()),
            (
                "channel_mask",
                self.channel_mask
                    .map_or(Value::Null, |mask| u64::from(mask).into()),
            ),
            ("container_bits", 16u64.into()),
            ("valid_bits", 16u64.into()),
            ("ieee_float", false.into()),
            ("sample_frames", self.sample_frames.into()),
        ])
    }
}

fn inspect_audio_pcm16(
    bytes: &[u8],
    maximum: usize,
    layout: (u32, u16),
    expected_frames: u64,
    dual_mono: bool,
) -> Result<AudioPcmObservation> {
    use semwright_audio_domain::wav::WaveReader;
    if bytes.len() < 44 || bytes.len() > maximum {
        return Err(Error::limit("Audio derivative WAV byte budget differs"));
    }
    let mut reader = WaveReader::open(std::io::Cursor::new(bytes), maximum as u64)
        .map_err(|_| Error::invalid("Audio derivative source is not a bounded WAV"))?;
    let info = reader.info().clone();
    let mask = if layout.1 == 1 { 0x4 } else { 0x3 };
    if info.sample_rate.0 != layout.0
        || info.channels != layout.1
        || info.container_bits != 16
        || info.valid_bits != 16
        || info.ieee_float
        || info.frames != expected_frames
        || info.channel_mask.is_some_and(|observed| observed != mask)
    {
        return Err(Error::new(
            "Conflict",
            "Audio WAV bytes do not match the closed PCM16 layout and sample clock",
        ));
    }
    let channels = usize::from(info.channels);
    let mut observed_frames = 0u64;
    loop {
        let samples = reader
            .read_frames(16_384)
            .map_err(|_| Error::invalid("Audio derivative PCM payload cannot be read"))?;
        if samples.is_empty() {
            break;
        }
        if samples.len() % channels != 0 {
            return Err(Error::invalid("Audio derivative PCM frame is incomplete"));
        }
        observed_frames = observed_frames
            .checked_add((samples.len() / channels) as u64)
            .filter(|count| *count <= expected_frames)
            .ok_or_else(|| {
                Error::limit("Audio derivative PCM frame count exceeds its source clock")
            })?;
        for frame in samples.chunks_exact(channels) {
            for value in frame {
                let sample = value * 32768.0;
                if !sample.is_finite()
                    || sample.fract() != 0.0
                    || !(-32768.0..=32767.0).contains(&sample)
                {
                    return Err(Error::invalid(
                        "Audio derivative contains a non-PCM16 sample",
                    ));
                }
            }
            if dual_mono && (channels != 2 || frame[0] != frame[1]) {
                return Err(Error::new(
                    "Conflict",
                    "Derived stereo channels are not the declared dual mono",
                ));
            }
        }
    }
    if observed_frames != expected_frames {
        return Err(Error::new(
            "Conflict",
            "Decoded PCM sample count differs from the exact audio clock",
        ));
    }
    Ok(AudioPcmObservation {
        sample_rate: info.sample_rate.0,
        channels: info.channels,
        channel_mask: info.channel_mask,
        sample_frames: observed_frames,
    })
}

fn audio_derivative_args(
    profile: AudioDerivativeProfile,
    input: &Path,
    output: &Path,
) -> Vec<OsString> {
    let (rate, channels) = profile.target_layout();
    vec![
        "-v".into(),
        "error".into(),
        "-nostdin".into(),
        "-n".into(),
        "-i".into(),
        input.as_os_str().to_owned(),
        "-map".into(),
        "0:a:0".into(),
        "-vn".into(),
        "-sn".into(),
        "-dn".into(),
        "-map_metadata".into(),
        "-1".into(),
        "-af".into(),
        profile.filter().into(),
        "-ac".into(),
        channels.to_string().into(),
        "-ar".into(),
        rate.to_string().into(),
        "-c:a".into(),
        "pcm_s16le".into(),
        "-fflags".into(),
        "+bitexact".into(),
        "-flags:a".into(),
        "+bitexact".into(),
        "-f".into(),
        "wav".into(),
        output.as_os_str().to_owned(),
    ]
}

pub fn audio_derivative_custody_paths(output_path: &str, host: bool) -> Result<Vec<String>> {
    crate::fs::validate_relative(output_path)?;
    if !output_path.ends_with(".wav") || output_path.len() > 3072 {
        return Err(Error::invalid(
            "Audio derivative requires a bounded .wav destination",
        ));
    }
    let mut paths = vec![
        output_path.to_owned(),
        format!("{output_path}.audio-derive.source.original.wav"),
        format!("{output_path}.audio-derive.output.original.wav"),
        format!("{output_path}.audio-derive.ffmpeg.argv.original.json"),
        format!("{output_path}.audio-derive.ffmpeg.stdout.original.bin"),
        format!("{output_path}.audio-derive.ffmpeg.stderr.original.bin"),
        format!("{output_path}.audio-derive.ffmpeg.terminal.original.json"),
        format!("{output_path}.audio-derive.operation.original.json"),
    ];
    if host {
        for family in ["host", "catalog"] {
            for suffix in [
                "stdin.original.bin",
                "stdout.original.bin",
                "stderr.original.bin",
                "terminal.original.json",
            ] {
                paths.push(format!("{output_path}.audio-derive-{family}.{suffix}"));
            }
        }
    }
    for path in &paths {
        crate::fs::validate_relative(path)?;
    }
    Ok(paths)
}

pub fn preflight_audio_derivative_custody(
    output: &crate::fs::Root,
    path: &str,
    host: bool,
) -> Result<()> {
    if !output.readable || !output.writable {
        return Err(Error::new(
            "PermissionDenied",
            "Audio derivative requires the existing read/write output grant",
        ));
    }
    for name in audio_derivative_custody_paths(path, host)? {
        if output.exists(&name)? {
            return Err(Error::new(
                "Conflict",
                "Audio derivative destination or original custody already exists",
            ));
        }
    }
    // Upper bound for the two declared profiles, original candidate + final WAV,
    // retained source and bounded process/Host originals. This is only a
    // free-space preflight; concurrency and quotas remain owned by Driver Host.
    if output.available_bytes_for(path)? < 90 * 1024 * 1024 {
        return Err(Error::limit(
            "Output lacks the closed audio derivative custody budget",
        ));
    }
    Ok(())
}

pub fn retain_audio_derivative_original(
    output: &crate::fs::Root,
    path: &str,
    bytes: &[u8],
    maximum: usize,
) -> Result<crate::fs::Artifact> {
    retain_process_original(output, path, bytes, maximum, "Audio derivative")
}

pub(crate) fn finish_audio_derivative_original<T>(
    original: Result<T>,
    retained: Result<()>,
) -> Result<T> {
    match original {
        Err(error) => Err(error),
        Ok(value) => retained.map(|()| value).map_err(|mut error| {
            error.outcome_known = false;
            error
        }),
    }
}

pub(crate) struct AudioDerivativeExecution<'a> {
    pub(crate) inputs: &'a Path,
    pub(crate) work: &'a Path,
    pub(crate) output: &'a crate::fs::Root,
    pub(crate) output_path: &'a str,
    pub(crate) source_sha256: &'a str,
    pub(crate) profile: AudioDerivativeProfile,
}

pub(crate) struct AudioDerivativeRun {
    pub(crate) recipe_sha256: String,
    pub(crate) runtime_sha256: String,
    pub(crate) originals: Vec<crate::fs::Artifact>,
}

fn checked_audio_derivative_process(
    original: Result<ProcessResult>,
    observation: &ProcessObservation,
) -> Result<ProcessResult> {
    let result = original?.checked()?;
    if !observation.complete() {
        let mut error = Error::new(
            "BackendFailed",
            "Audio process returned without complete original stream/terminal capture",
        );
        error.outcome_known = false;
        return Err(error);
    }
    Ok(result)
}

pub(crate) fn validate_audio_derivative_result(value: &Value) -> Result<()> {
    let profile = AudioDerivativeProfile::parse(value.str("profile")?)?;
    let source = value.get("source")?;
    let target = value.get("target")?;
    let source_frames = source.uint("sample_frames")?;
    let target_frames = profile.output_frames(source_frames)?;
    let layout = |value: &Value, expected: (u32, u16), frames: u64| -> Result<()> {
        let expected_mask = if expected.1 == 1 { 4 } else { 3 };
        let mask = value.get("channel_mask")?;
        if value.uint("sample_rate")? != u64::from(expected.0)
            || value.uint("channels")? != u64::from(expected.1)
            || value.uint("sample_frames")? != frames
            || value.uint("container_bits")? != 16
            || value.uint("valid_bits")? != 16
            || value.get("ieee_float")? != &Value::Bool(false)
            || (mask != &Value::Null && mask.u64()? != expected_mask)
        {
            return Err(Error::new(
                "BackendFailed",
                "Audio derivative result layout or sample clock differs",
            ));
        }
        Ok(())
    };
    layout(source, profile.source_layout(), source_frames)?;
    layout(target, profile.target_layout(), target_frames)?;
    if !valid_digest(value.str("source_sha256")?)
        || !valid_digest(value.str("runtime_sha256")?)
        || value.str("recipe_sha256")? != profile.recipe_sha256()?
        || value.get("process_capture_complete")? != &Value::Bool(true)
    {
        return Err(Error::new(
            "BackendFailed",
            "Audio derivative result identity differs",
        ));
    }
    let artifact = value.get("artifact")?;
    let path = artifact.str("path")?;
    crate::fs::validate_relative(path)?;
    if !path.ends_with(".wav") || path.len() > 3072 {
        return Err(Error::new(
            "BackendFailed",
            "Audio derivative result destination differs",
        ));
    }
    let validate_artifact =
        |file: &Value, expected_path: &str, minimum: u64, maximum: u64| -> Result<()> {
            if file.str("root")? != "output"
                || file.str("path")? != expected_path
                || !valid_digest(file.str("sha256")?)
                || !(minimum..=maximum).contains(&file.uint("bytes")?)
                || (file.uint("bytes")? == 0 && file.str("sha256")? != crate::hash::sha256(&[]))
            {
                return Err(Error::new(
                    "BackendFailed",
                    "Audio derivative original Artifact differs",
                ));
            }
            Ok(())
        };
    let source_minimum = source_frames * u64::from(profile.source_layout().1) * 2 + 44;
    let target_minimum = target_frames * u64::from(profile.target_layout().1) * 2 + 44;
    validate_artifact(
        artifact,
        path,
        target_minimum,
        profile.output_limit() as u64,
    )?;
    let original_source = value.get("original_source")?;
    validate_artifact(
        original_source,
        &format!("{path}.audio-derive.source.original.wav"),
        source_minimum,
        profile.input_limit() as u64,
    )?;
    let original_output = value.get("original_output")?;
    validate_artifact(
        original_output,
        &format!("{path}.audio-derive.output.original.wav"),
        target_minimum,
        profile.output_limit() as u64,
    )?;
    if original_source.str("sha256")? != value.str("source_sha256")?
        || original_output.str("sha256")? != artifact.str("sha256")?
        || original_output.uint("bytes")? != artifact.uint("bytes")?
    {
        return Err(Error::new(
            "BackendFailed",
            "Audio derivative originals do not bind the published WAV",
        ));
    }
    let originals = value.get("process_originals")?.as_array()?;
    if originals.len() != 4 {
        return Err(Error::new(
            "BackendFailed",
            "Audio derivative process originals are incomplete",
        ));
    }
    for (file, suffix) in originals.iter().zip([
        "argv.original.json",
        "stdout.original.bin",
        "stderr.original.bin",
        "terminal.original.json",
    ]) {
        let stream = suffix.ends_with(".bin");
        validate_artifact(
            file,
            &format!("{path}.audio-derive.ffmpeg.{suffix}"),
            if stream { 0 } else { 1 },
            (if stream {
                AUDIO_DERIVATIVE_STREAM_LIMIT
            } else {
                AUDIO_DERIVATIVE_METADATA_LIMIT
            }) as u64,
        )?;
    }
    Ok(())
}

#[repr(C)]
struct Limit {
    current: u64,
    maximum: u64,
}
unsafe extern "C" {
    fn prctl(option: i32, ...) -> i32;
    fn getppid() -> i32;
    fn kill(pid: i32, sig: i32) -> i32;
    fn setrlimit(resource: u32, limit: *const Limit) -> i32;
    fn getrlimit(resource: u32, limit: *mut Limit) -> i32;
    fn getuid() -> u32;
}
#[derive(Clone, Debug)]
pub struct ProcessSpec {
    pub executable: PathBuf,
    pub args: Vec<OsString>,
    pub cwd: PathBuf,
    pub timeout: Duration,
    pub cpu_seconds: u64,
    pub address_space_bytes: u64,
    pub environment: BTreeMap<String, String>,
}
#[derive(Clone, Debug)]
pub struct ProcessResult {
    pub exit_code: Option<i32>,
    pub term_signal: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub cancelled: bool,
    pub timed_out: bool,
    pub output_exceeded: bool,
}
impl ProcessResult {
    pub fn success(&self) -> bool {
        self.exit_code == Some(0) && !self.cancelled && !self.timed_out && !self.output_exceeded
    }
    pub fn checked(self) -> Result<Self> {
        if self.cancelled {
            return Err(Error::new("Cancelled", "Operation cancelled"));
        }
        if self.timed_out {
            return Err(Error::new("Timeout", "Tool wall-clock deadline exceeded"));
        }
        if self.output_exceeded {
            return Err(Error::limit("Tool output exceeded total byte budget"));
        }
        if self.exit_code != Some(0) {
            return Err(Error::new(
                "BackendFailed",
                format!(
                    "Tool failed with exit code {:?}, signal {:?}; raw logs are not exposed",
                    self.exit_code, self.term_signal
                ),
            ));
        }
        Ok(self)
    }
}
#[derive(Clone, Debug, Default)]
struct CapturedPipe {
    retained: Vec<u8>,
    observed_bytes: u64,
    eof: bool,
    read_error: Option<serde_json::Value>,
    panicked: bool,
    stopped: bool,
}

impl CapturedPipe {
    fn complete(&self) -> bool {
        self.eof
            && self.read_error.is_none()
            && !self.panicked
            && !self.stopped
            && self.observed_bytes == self.retained.len() as u64
    }

    fn metadata(&self) -> serde_json::Value {
        serde_json::json!({
            "observed_bytes": self.observed_bytes,
            "retained_bytes": self.retained.len(),
            "retained_sha256": crate::hash::sha256(&self.retained),
            "eof_observed": self.eof,
            "read_error": self.read_error,
            "reader_panicked": self.panicked,
            "drain_stopped": self.stopped,
            "truncated": self.observed_bytes != self.retained.len() as u64,
            "complete": self.complete()
        })
    }
}

struct CaptureThread {
    state: Arc<std::sync::Mutex<CapturedPipe>>,
    handle: Option<thread::JoinHandle<()>>,
    stop: Arc<AtomicBool>,
    interruptible: bool,
}

impl CaptureThread {
    fn finished(&self) -> bool {
        self.handle
            .as_ref()
            .is_none_or(thread::JoinHandle::is_finished)
    }

    fn finish(mut self) -> CapturedPipe {
        let panicked = self
            .handle
            .take()
            .is_some_and(|handle| handle.join().is_err());
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.panicked |= panicked;
        state.clone()
    }
}

impl Drop for CaptureThread {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            self.stop.store(true, Ordering::Release);
            if self.interruptible {
                // Only the observed path sets O_NONBLOCK before starting this
                // reader. Closing its stop flag cannot wait on an inherited
                // descendant pipe. The legacy path retains its prior policy.
                let _ = handle.join();
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct ProcessObservation {
    spawn_attempted: bool,
    pid: Option<u32>,
    terminal_observed: bool,
    exit_code: Option<i32>,
    term_signal: Option<i32>,
    cancelled: Option<bool>,
    timed_out: Option<bool>,
    output_exceeded: Option<bool>,
    drain_stop_reason: Option<&'static str>,
    stdout: Option<CapturedPipe>,
    stderr: Option<CapturedPipe>,
    supervisor_error: Option<serde_json::Value>,
}

impl ProcessObservation {
    fn complete(&self) -> bool {
        self.terminal_observed
            && self.supervisor_error.is_none()
            && self.drain_stop_reason.is_none()
            && self.cancelled == Some(false)
            && self.timed_out == Some(false)
            && self.output_exceeded == Some(false)
            && self.stdout.as_ref().is_some_and(CapturedPipe::complete)
            && self.stderr.as_ref().is_some_and(CapturedPipe::complete)
    }

    fn metadata(&self) -> serde_json::Value {
        serde_json::json!({
            "spawn_attempted": self.spawn_attempted,
            "pid": self.pid,
            "terminal_observed": self.terminal_observed,
            "exit_code": self.exit_code,
            "term_signal": self.term_signal,
            "cancelled": self.cancelled,
            "timed_out": self.timed_out,
            "output_exceeded": self.output_exceeded,
            "drain_stop_reason": self.drain_stop_reason,
            "stdout": self.stdout.as_ref().map(CapturedPipe::metadata),
            "stderr": self.stderr.as_ref().map(CapturedPipe::metadata),
            "supervisor_error": self.supervisor_error,
            "capture_complete": self.complete(),
            "native_execution_retried": false
        })
    }
}

fn capture_nonblocking(pipe: &impl std::os::fd::AsRawFd) -> Result<()> {
    let fd = pipe.as_raw_fd();
    // SAFETY: fd is borrowed from the still-owned read pipe. These calls only
    // inspect/change this read end's status flags; they never close the fd.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: the same live borrowed descriptor is used with the existing flags.
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

fn capture(
    mut reader: impl Read + Send + 'static,
    overflow: Arc<AtomicBool>,
    retained_limit: usize,
    total_limit: usize,
    stop: Arc<AtomicBool>,
    interruptible: bool,
) -> Result<CaptureThread> {
    let state = Arc::new(std::sync::Mutex::new(CapturedPipe::default()));
    let observed = state.clone();
    let requested_stop = stop.clone();
    let handle = thread::Builder::new().spawn(move || {
        let mut buf = [0; 4096];
        loop {
            if requested_stop.load(Ordering::Acquire) {
                observed.lock().unwrap_or_else(|e| e.into_inner()).stopped = true;
                break;
            }
            match reader.read(&mut buf) {
                Ok(0) => {
                    observed.lock().unwrap_or_else(|e| e.into_inner()).eof = true;
                    break;
                }
                Ok(n) => {
                    let mut observed = observed.lock().unwrap_or_else(|e| e.into_inner());
                    observed.observed_bytes = observed.observed_bytes.saturating_add(n as u64);
                    let keep = n.min(retained_limit.saturating_sub(observed.retained.len()));
                    observed.retained.extend_from_slice(&buf[..keep]);
                    if observed.observed_bytes > total_limit as u64 {
                        overflow.store(true, Ordering::Release);
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) if interruptible && error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => {
                    observed
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .read_error = Some(serde_json::json!({
                        "kind": format!("{:?}", error.kind()),
                        "raw_os_error": error.raw_os_error(),
                        "message": error.to_string().chars().take(256).collect::<String>()
                    }));
                    break;
                }
            }
        }
    })?;
    Ok(CaptureThread {
        state,
        handle: Some(handle),
        stop,
        interruptible,
    })
}
// This drain uses the same executor deadline. A stop after a known leader
// terminal is recorded separately; it never changes that leader's wait flags.
fn drain_observed_capture(
    readers: [&CaptureThread; 2],
    stop: &AtomicBool,
    start: Instant,
    timeout: Duration,
    cancel: &AtomicBool,
    overflow: &AtomicBool,
    wait_failed: bool,
) -> Option<&'static str> {
    if wait_failed {
        stop.store(true, Ordering::Release);
        return Some("supervisor_error");
    }
    while readers.iter().any(|reader| !reader.finished()) {
        let reason = if start.elapsed() >= timeout {
            Some("deadline")
        } else if cancel.load(Ordering::Acquire) {
            Some("cancelled")
        } else if overflow.load(Ordering::Acquire) {
            Some("output_exceeded")
        } else {
            None
        };
        if reason.is_some() {
            stop.store(true, Ordering::Release);
            return reason;
        }
        thread::sleep(Duration::from_millis(10));
    }
    None
}

/// Owns the child on every error path, including pipe setup and wait failures.
struct ChildGuard {
    child: std::process::Child,
    group: i32,
    armed: bool,
}
impl std::ops::Deref for ChildGuard {
    type Target = std::process::Child;
    fn deref(&self) -> &Self::Target {
        &self.child
    }
}
impl std::ops::DerefMut for ChildGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.child
    }
}
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if self.armed {
            // SAFETY: group belongs to the child created by this supervisor, not a caller-supplied PID.
            unsafe { kill(-self.group, 9) };
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
/// Testable supervisor. Only Runtime builds production specs; no public capability accepts argv.
pub fn run(spec: &ProcessSpec, cancel: &AtomicBool) -> Result<ProcessResult> {
    run_with_output_limits(spec, cancel, RETAIN_LOG, TOTAL_LOG)
}

fn run_with_output_limits(
    spec: &ProcessSpec,
    cancel: &AtomicBool,
    retained_limit: usize,
    total_limit: usize,
) -> Result<ProcessResult> {
    run_with_output_limits_observed(spec, cancel, retained_limit, total_limit, None)
}

fn run_with_output_limits_observed(
    spec: &ProcessSpec,
    cancel: &AtomicBool,
    retained_limit: usize,
    total_limit: usize,
    observation: Option<&mut ProcessObservation>,
) -> Result<ProcessResult> {
    run_with_file_limit_observed(
        spec,
        cancel,
        retained_limit,
        total_limit,
        crate::jobs::MAX_ARTIFACT_BYTES,
        observation,
    )
}

#[doc(hidden)]
pub fn run_with_file_limit_observed(
    spec: &ProcessSpec,
    cancel: &AtomicBool,
    retained_limit: usize,
    total_limit: usize,
    file_size_limit: u64,
    mut observation: Option<&mut ProcessObservation>,
) -> Result<ProcessResult> {
    let result = run_with_output_limits_inner(
        spec,
        cancel,
        retained_limit,
        total_limit,
        file_size_limit,
        observation.as_deref_mut(),
    );
    if let (Some(observation), Err(error)) = (observation, result.as_ref()) {
        observation.supervisor_error = Some(serde_json::json!({
            "code": error.code, "message": error.message,
            "outcome_known": error.outcome_known
        }));
    }
    result
}

fn run_with_output_limits_inner(
    spec: &ProcessSpec,
    cancel: &AtomicBool,
    retained_limit: usize,
    total_limit: usize,
    file_size_limit: u64,
    mut observation: Option<&mut ProcessObservation>,
) -> Result<ProcessResult> {
    if retained_limit == 0 || retained_limit > total_limit || total_limit > 8 * 1024 * 1024 {
        return Err(Error::limit("Process output capture budget is invalid"));
    }
    if file_size_limit == 0 || file_size_limit > crate::jobs::MAX_ARTIFACT_BYTES {
        return Err(Error::limit(
            "Process file-size limit is outside the existing bound",
        ));
    }
    if !spec.executable.is_absolute()
        || !spec.cwd.is_absolute()
        || spec.timeout.is_zero()
        || spec.timeout > Duration::from_secs(3600)
        || !(134_217_728..=4_294_967_296).contains(&spec.address_space_bytes)
    {
        return Err(Error::invalid("Invalid process specification"));
    }
    if cancel.load(Ordering::Acquire) {
        return Err(Error::new("Cancelled", "Cancelled before spawn"));
    }
    let mut command = Command::new(&spec.executable);
    command
        .args(&spec.args)
        .current_dir(&spec.cwd)
        .env_clear()
        .envs(&spec.environment)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let parent = std::process::id() as i32;
    let cpu = spec.cpu_seconds.clamp(1, 300);
    let address_space = spec.address_space_bytes;
    // SAFETY: the closure only uses async-signal-safe Linux syscalls and stack values after fork.
    unsafe {
        command.pre_exec(move || {
            if prctl(1, 9, 0, 0, 0) != 0 || getppid() != parent {
                return Err(std::io::Error::last_os_error());
            }
            // Reduce, never raise, inherited hard limits. Limits are per process, not a cgroup.
            for (resource, budget) in [
                (4u32, 0u64),
                (0, cpu),
                (1, file_size_limit),
                (7, 128),
                (9, address_space),
                // RLIMIT_NPROC is intentionally owned by the outer DriverProvider sandbox.
                // Linux accounts it against the real UID, so imposing a second fixed limit here
                // can reject legitimate child workers when the host UID already has many tasks.
            ] {
                let mut current = Limit {
                    current: 0,
                    maximum: 0,
                };
                if getrlimit(resource, &mut current) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                let maximum = current.maximum.min(budget);
                let limit = Limit {
                    current: current.current.min(maximum),
                    maximum,
                };
                if setrlimit(resource, &limit) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
    if let Some(observation) = observation.as_deref_mut() {
        observation.spawn_attempted = true;
    }
    let child = command.spawn()?;
    let pgid = child.id() as i32;
    if let Some(observation) = observation.as_deref_mut() {
        observation.pid = Some(child.id());
    }
    let mut child = ChildGuard {
        child,
        group: pgid,
        armed: true,
    };
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::new("Internal", "stdout pipe missing"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| Error::new("Internal", "stderr pipe missing"))?;
    let observing = observation.is_some();
    if observing {
        capture_nonblocking(&stdout)?;
        capture_nonblocking(&stderr)?;
    }
    let overflow = Arc::new(AtomicBool::new(false));
    let stop = Arc::new(AtomicBool::new(false));
    let a = capture(
        stdout,
        overflow.clone(),
        retained_limit,
        total_limit,
        stop.clone(),
        observing,
    )?;
    let b = match capture(
        stderr,
        overflow.clone(),
        retained_limit,
        total_limit,
        stop.clone(),
        observing,
    ) {
        Ok(reader) => reader,
        Err(original) => {
            // End the one owned child before dropping the first reader. In the
            // observed path its nonblocking read end also honors this stop.
            drop(child);
            stop.store(true, Ordering::Release);
            if let Some(observation) = observation.as_deref_mut() {
                observation.drain_stop_reason = Some("reader_start_failed");
                observation.stdout = Some(a.finish());
            } else {
                drop(a);
            }
            return Err(original);
        }
    };
    let start = Instant::now();
    let mut cancelled = false;
    let mut timed_out = false;
    let mut status = None;
    let waited = (|| -> Result<()> {
        loop {
            if let Some(observed) = child.try_wait()? {
                status = Some(observed);
                break;
            }
            cancelled = cancel.load(Ordering::Acquire);
            timed_out = start.elapsed() >= spec.timeout;
            if cancelled || timed_out || overflow.load(Ordering::Acquire) {
                // SAFETY: the negative PID is the still-owned child group.
                unsafe { kill(-pgid, 15) };
                let until = Instant::now() + Duration::from_millis(300);
                while Instant::now() < until {
                    if let Some(observed) = child.try_wait()? {
                        status = Some(observed);
                        break;
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                // SAFETY: same owned group; this is the existing termination policy.
                unsafe { kill(-pgid, 9) };
                status = Some(child.wait()?);
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    })();
    if let Some(observation) = observation.as_deref_mut() {
        // Preserve the actual terminal BEFORE either join. A later wait/read/
        // custody error does not turn a known nonzero exit into a spawn failure.
        observation.terminal_observed = status.is_some();
        observation.exit_code = status.as_ref().and_then(std::process::ExitStatus::code);
        observation.term_signal = status.as_ref().and_then(ExitStatusExt::signal);
        observation.cancelled = Some(cancelled);
        observation.timed_out = Some(timed_out);
    }
    // SAFETY: preserve the executor's cleanup of residual same-group descendants.
    unsafe { kill(-pgid, 9) };
    if waited.is_ok() {
        child.armed = false;
    }
    drop(child);
    if !observing && waited.is_err() {
        // Legacy errors retain their classification and prior detach behavior.
        return waited.and_then(|()| Err(Error::new("Internal", "Unreachable wait state")));
    }
    if let Some(observation) = observation.as_deref_mut() {
        observation.drain_stop_reason = drain_observed_capture(
            [&a, &b],
            &stop,
            start,
            spec.timeout,
            cancel,
            &overflow,
            waited.is_err(),
        );
    }
    let stdout = a.finish();
    let stderr = b.finish();
    if let Some(observation) = observation {
        observation.stdout = Some(stdout.clone());
        observation.stderr = Some(stderr.clone());
        observation.output_exceeded = Some(overflow.load(Ordering::Acquire));
    }
    waited?;
    if !observing && stdout.panicked {
        return Err(Error::new("Internal", "stdout reader failed"));
    }
    if !observing && stderr.panicked {
        return Err(Error::new("Internal", "stderr reader failed"));
    }
    let status = status.ok_or_else(|| Error::new("Internal", "Process terminal missing"))?;
    Ok(ProcessResult {
        exit_code: status.code(),
        term_signal: status.signal(),
        stdout: stdout.retained,
        stderr: stderr.retained,
        cancelled,
        timed_out,
        output_exceeded: overflow.load(Ordering::Acquire),
    })
}

fn overflow_uid() -> Option<u32> {
    std::fs::read_to_string("/proc/sys/kernel/overflowuid")
        .ok()?
        .trim()
        .parse()
        .ok()
}

fn trusted_tool_owner(path: &Path, owner: u32, current: u32, overflow: Option<u32>) -> bool {
    owner == 0
        || owner == current
        || (overflow == Some(owner) && path.starts_with(Path::new("/usr")))
}

#[derive(Clone, Debug)]
pub struct Tool {
    pub path: PathBuf,
    pub sha256: String,
}
impl Tool {
    fn parse(v: &Value) -> Result<Self> {
        v.strict(&["path", "sha256"], &["path", "sha256"])?;
        let tool = Self {
            path: v.str("path")?.into(),
            sha256: v.str("sha256")?.into(),
        };
        tool.verify()?;
        Ok(tool)
    }
    pub fn verify(&self) -> Result<File> {
        if !self.path.is_absolute()
            || !valid_digest(&self.sha256)
            || std::fs::canonicalize(&self.path)? != self.path
        {
            return Err(Error::new(
                "PermissionDenied",
                "Tool must have a canonical absolute path and lowercase SHA-256 pin",
            ));
        }
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(0o400000 | 0o2000000 | 0o4000)
            .open(&self.path)?;
        let m = file.metadata()?;
        // SAFETY: getuid has no arguments or memory preconditions.
        let uid = unsafe { getuid() };
        if !trusted_tool_owner(&self.path, m.uid(), uid, overflow_uid()) {
            return Err(Error::new(
                "PermissionDenied",
                "Pinned tool owner must be root, the sandbox uid, or the kernel overflow uid for read-only /usr",
            ));
        }
        if !m.is_file() {
            return Err(Error::new(
                "PermissionDenied",
                "Pinned tool must be a regular file",
            ));
        }
        if m.nlink() != 1 {
            return Err(Error::new(
                "PermissionDenied",
                "Pinned tool must have exactly one hard link",
            ));
        }
        if m.mode() & 0o022 != 0 {
            return Err(Error::new(
                "PermissionDenied",
                "Pinned tool must not be group- or other-writable",
            ));
        }
        if m.mode() & 0o111 == 0 {
            return Err(Error::new(
                "PermissionDenied",
                "Pinned tool must have an executable mode bit",
            ));
        }
        if m.len() > 64 * 1024 * 1024 {
            return Err(Error::new(
                "PermissionDenied",
                "Pinned tool must not exceed 64 MiB",
            ));
        }
        let mut elf = [0; 4];
        file.read_exact(&mut elf)?;
        if &elf != b"\x7fELF" {
            return Err(Error::invalid("Pinned tool is not ELF"));
        }
        use std::io::{Seek, SeekFrom};
        file.seek(SeekFrom::Start(0))?;
        let (actual, _) = reader_hash(&mut file, 64 * 1024 * 1024)?;
        if actual != self.sha256 {
            return Err(Error::new("PermissionDenied", "Tool digest changed"));
        }
        file.seek(SeekFrom::Start(0))?;
        Ok(file)
    }
}
#[derive(Clone, Debug, Default)]
pub struct ServiceCatalog {
    pub version: String,
    pub groups: BTreeMap<String, BTreeSet<String>>,
}
impl ServiceCatalog {
    pub fn has(&self, group: &str, id: &str) -> bool {
        self.groups.get(group).is_some_and(|s| s.contains(id))
    }
    pub fn parse_list(text: &str) -> Result<BTreeSet<String>> {
        if text.len() > RETAIN_LOG {
            return Err(Error::limit("Service catalog text too large"));
        }
        let mut values = BTreeSet::new();
        for line in text.lines() {
            if let Some(token) = line.trim().strip_prefix("- ") {
                let token = token.trim().trim_matches('"').trim_matches('\'');
                if token.len() <= 128
                    && !token.is_empty()
                    && token.bytes().all(|b| {
                        b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b':')
                    })
                {
                    values.insert(token.into());
                }
            }
            if values.len() > 2048 {
                return Err(Error::limit("Runtime service catalog budget exceeded"));
            }
        }
        Ok(values)
    }
}
pub fn constrained_environment() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("LC_ALL".into(), "C".into()),
        ("HOME".into(), "/home".into()),
        ("PATH".into(), "/usr/bin:/bin".into()),
        ("QT_QPA_PLATFORM".into(), "offscreen".into()),
        ("XDG_CONFIG_HOME".into(), "/tmp/config".into()),
        ("XDG_CACHE_HOME".into(), "/tmp/cache".into()),
        ("MLT_NO_VDPAU".into(), "1".into()),
    ])
}

fn close_frame_readback(
    reader: impl Read,
    video_sha256: &str,
    video_bytes: u64,
    frames: &[crate::color::DecodedPng],
    deadline: Instant,
    cancel: &AtomicBool,
) -> Result<String> {
    let (after_sha256, after_bytes) = reader_hash(reader, crate::jobs::MAX_ARTIFACT_BYTES)?;
    if after_sha256 != video_sha256 || after_bytes != video_bytes {
        return Err(Error::new(
            "StaleReference",
            "Encoded artifact changed during complete decode",
        ));
    }
    let roster = crate::color::rgba_roster_digest(frames)?;
    remaining_color_budget(deadline)?;
    if cancel.load(Ordering::Acquire) {
        return Err(Error::new("Cancelled", "Complete frame readback cancelled"));
    }
    Ok(roster)
}

fn remaining_color_budget(deadline: Instant) -> Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|v| !v.is_zero())
        .ok_or_else(|| Error::new("Timeout", "Shared encode/readback deadline exceeded"))
}

#[derive(Debug)]
pub struct EncodedFramesObservation {
    pub video_sha256: String,
    pub video_bytes: u64,
    pub probe: crate::color::Probe,
    pub frames: Vec<crate::color::DecodedPng>,
    pub retained_png_bytes: u64,
    pub rgba_roster_sha256: String,
    pub ffmpeg_version: String,
    pub ffmpeg_sha256: String,
    pub ffprobe_sha256: String,
}
pub struct ValidatedOutput {
    pub media: MediaInfo,
    pub observation: Option<EncodedFramesObservation>,
}
/// Existing opt-in native routes; unrelated legacy profiles keep their contract.
pub fn complete_observation_profile(
    profile: &RenderProfile,
) -> Option<crate::color::ObservationProfile> {
    match profile.id {
        "lossless-video-only" => Some(crate::color::ObservationProfile::MltVisualFfv1),
        "av-h264-aac" => Some(crate::color::ObservationProfile::MltH264AacMaster),
        _ => None,
    }
}

pub struct EncodedFrames {
    pub media: MediaInfo,
    pub observation: EncodedFramesObservation,
}

pub struct Runtime {
    pub melt: Tool,
    pub ffprobe: Tool,
    pub ffmpeg: Tool,
    pub bubblewrap: Tool,
    pub timeout: Duration,
    pub catalog: ServiceCatalog,
    tools: PrivateDir,
    host_sandboxed: bool,
}
impl Runtime {
    /// Construct the bounded media engine inside the Semwright-owned Host tool.
    /// The helper receives these paths only as sealed ToolPath dependencies, then
    /// verifies the matching read-only executable runtime entrypoints. Caller
    /// capability arguments never select an executable or a runtime configuration.
    pub fn from_host_tools(melt: Tool, ffprobe: Tool, ffmpeg: Tool) -> Result<Self> {
        melt.verify()?;
        ffprobe.verify()?;
        ffmpeg.verify()?;
        let mut runtime = Self {
            melt,
            ffprobe: ffprobe.clone(),
            ffmpeg,
            // No nested sandbox is invoked: the Host owns this tool's isolation.
            bubblewrap: ffprobe,
            // Keep the media operation bounded while leaving room for a full
            // two-thread 1080p master under the 300-CPU-second Host ceiling.
            timeout: Duration::from_secs(180),
            catalog: ServiceCatalog::default(),
            tools: PrivateDir::new(Path::new("/tmp"))?,
            host_sandboxed: true,
        };
        runtime.discover()?;
        Ok(runtime)
    }
    pub fn load(v: &Value) -> Result<Self> {
        v.strict(
            &[
                "schema",
                "melt",
                "ffprobe",
                "ffmpeg",
                "bubblewrap",
                "timeout_seconds",
            ],
            &[
                "schema",
                "melt",
                "ffprobe",
                "ffmpeg",
                "bubblewrap",
                "timeout_seconds",
            ],
        )?;
        if v.uint("schema")? != 1 {
            return Err(Error::unsupported("Runtime configuration schema"));
        }
        let melt = Tool::parse(v.get("melt")?)?;
        let ffprobe = Tool::parse(v.get("ffprobe")?)?;
        let ffmpeg = Tool::parse(v.get("ffmpeg")?)?;
        let bubblewrap = Tool::parse(v.get("bubblewrap")?)?;
        let timeout = v.uint("timeout_seconds")?;
        if !(1..=3600).contains(&timeout) {
            return Err(Error::invalid("Render timeout must be 1..3600 seconds"));
        }
        let tools = PrivateDir::new(Path::new("/tmp"))?;
        // Stage the media tools so the bytes executed later are exactly the pinned
        // bytes we verified. Bubblewrap is different: Linux/AppArmor installations can
        // grant user-namespace permission specifically to its canonical system path.
        // Keep that root-owned, non-writable path and re-verify its digest before use.
        for (name, tool) in [("melt", &melt), ("ffprobe", &ffprobe), ("ffmpeg", &ffmpeg)] {
            let mut source = tool.verify()?;
            let mut destination = tools.create(name)?;
            let mut hash = crate::hash::Sha256::new();
            let mut size = 0usize;
            let mut buffer = [0; 65536];
            loop {
                let count = source.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                size += count;
                if size > 64 * 1024 * 1024 {
                    return Err(Error::limit("Tool changed beyond staged byte budget"));
                }
                destination.write_all(&buffer[..count])?;
                hash.update(&buffer[..count]);
            }
            if hash.finish() != tool.sha256 {
                return Err(Error::new(
                    "PermissionDenied",
                    "Tool bytes changed while staging",
                ));
            }
            destination.sync_all()?;
            std::fs::set_permissions(
                tools.path().join(name),
                std::fs::Permissions::from_mode(0o500),
            )?;
        }
        let mut runtime = Self {
            melt,
            ffprobe,
            ffmpeg,
            bubblewrap,
            timeout: Duration::from_secs(timeout),
            catalog: ServiceCatalog::default(),
            tools,
            host_sandboxed: std::env::var("SEMWRIGHT_DRIVER_SANDBOX").as_deref()
                == Ok("landlock-bwrap-v1")
                && Path::new("/plugin/bin").is_file()
                && Path::new("/plugin/sandbox").is_file(),
        };
        runtime.discover()?;
        Ok(runtime)
    }
    fn environment() -> BTreeMap<String, String> {
        constrained_environment()
    }

    fn input_path(&self, inputs: &Path, name: &str) -> PathBuf {
        if self.host_sandboxed {
            inputs.join(name)
        } else {
            PathBuf::from(format!("/inputs/{name}"))
        }
    }

    fn work_path(&self, work: &Path, name: &str) -> PathBuf {
        if self.host_sandboxed {
            work.join(name)
        } else {
            PathBuf::from(format!("/work/{name}"))
        }
    }

    pub fn input_reference(&self, inputs: &Path, name: &str) -> String {
        self.input_path(inputs, name).to_string_lossy().into_owned()
    }

    fn spec(
        &self,
        tool: &str,
        args: Vec<OsString>,
        inputs: &Path,
        work: &Path,
        timeout: Duration,
    ) -> Result<ProcessSpec> {
        if self.host_sandboxed {
            // Driver Host deliberately keeps writable scratch roots non-executable.
            // Execute the original owner-pinned tool from the host's read-only+exec
            // system/runtime surface instead of the staged /tmp copy used by the
            // standalone nested-Bubblewrap path. Re-verify the digest immediately
            // before every spawn so a stale runtime pin fails closed.
            let pinned = match tool {
                "melt" => &self.melt,
                "ffprobe" => &self.ffprobe,
                "ffmpeg" => &self.ffmpeg,
                _ => return Err(Error::invalid("Unknown pinned runtime tool")),
            };
            pinned.verify()?;
            let (cpu_seconds, address_space_bytes) = match tool {
                // Curated 1080p H.264 renders are bounded by the outer Driver Host
                // at the same 4 GiB / 300 CPU-second ceilings. These are limits,
                // not reservations; ffprobe keeps the smaller probe budget.
                "melt" => (300, 4_294_967_296),
                "ffprobe" => (timeout.as_secs().clamp(30, 120), 1_073_741_824),
                "ffmpeg" => (300, 4_294_967_296),
                _ => return Err(Error::invalid("Unknown pinned runtime tool")),
            };
            return Ok(ProcessSpec {
                executable: pinned.path.clone(),
                args,
                cwd: work.into(),
                timeout,
                cpu_seconds,
                address_space_bytes,
                environment: Self::environment(),
            });
        }

        self.bubblewrap.verify()?;
        let mut argv: Vec<OsString> = [
            "--die-with-parent",
            "--new-session",
            "--unshare-all",
            "--clearenv",
            "--cap-drop",
            "ALL",
            "--proc",
            "/proc",
            "--dev",
            "/dev",
            "--tmpfs",
            "/tmp",
            "--dir",
            "/home",
            "--dir",
            "/etc",
            "--dir",
            "/tools",
            "--dir",
            "/inputs",
            "--dir",
            "/work",
        ]
        .into_iter()
        .map(Into::into)
        .collect();
        for path in ["/usr", "/lib", "/lib64"] {
            if Path::new(path).exists() {
                argv.extend(["--ro-bind".into(), path.into(), path.into()]);
            }
        }
        if Path::new("/etc/ld.so.cache").exists() {
            argv.extend([
                "--ro-bind".into(),
                "/etc/ld.so.cache".into(),
                "/etc/ld.so.cache".into(),
            ]);
        }
        argv.extend([
            "--ro-bind".into(),
            self.tools.path().as_os_str().into(),
            "/tools".into(),
            "--ro-bind".into(),
            inputs.as_os_str().into(),
            "/inputs".into(),
            "--bind".into(),
            work.as_os_str().into(),
            "/work".into(),
        ]);
        for (key, value) in Self::environment() {
            argv.extend(["--setenv".into(), key.into(), value.into()]);
        }
        argv.extend([
            "--chdir".into(),
            "/work".into(),
            "--".into(),
            format!("/tools/{tool}").into(),
        ]);
        argv.extend(args);
        let (cpu_seconds, address_space_bytes) = match tool {
            "melt" => (300, 4_294_967_296),
            "ffprobe" => (timeout.as_secs().clamp(30, 120), 1_073_741_824),
            "ffmpeg" => (300, 4_294_967_296),
            _ => return Err(Error::invalid("Unknown pinned runtime tool")),
        };
        Ok(ProcessSpec {
            executable: self.bubblewrap.path.clone(),
            args: argv,
            cwd: work.into(),
            timeout,
            cpu_seconds,
            address_space_bytes,
            environment: BTreeMap::from([("LC_ALL".into(), "C".into())]),
        })
    }
    fn discover(&mut self) -> Result<()> {
        let inputs = PrivateDir::new(Path::new("/tmp"))?;
        let work = PrivateDir::new(Path::new("/tmp"))?;
        let cancel = AtomicBool::new(false);
        let result = run(
            &self.spec(
                "melt",
                vec!["-version".into()],
                inputs.path(),
                work.path(),
                Duration::from_secs(2),
            )?,
            &cancel,
        )?
        .checked()
        .map_err(|error| {
            Error::new(
                error.code,
                format!("melt -version discovery failed: {}", error.message),
            )
        })?;
        let text = format!(
            "{} {}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        self.catalog.version = json::display(
            text.lines()
                .find(|l| l.to_ascii_lowercase().contains("melt"))
                .unwrap_or("Version output did not identify melt"),
        );
        for group in [
            "producers",
            "filters",
            "transitions",
            "consumers",
            "video_codecs",
            "audio_codecs",
        ] {
            let result = run(
                &self.spec(
                    "melt",
                    vec!["-query".into(), group.into()],
                    inputs.path(),
                    work.path(),
                    Duration::from_secs(2),
                )?,
                &cancel,
            )?
            .checked()
            .map_err(|error| {
                Error::new(
                    error.code,
                    format!("melt -query {group} discovery failed: {}", error.message),
                )
            })?;
            let text = format!(
                "{}\n{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
            self.catalog
                .groups
                .insert(group.into(), ServiceCatalog::parse_list(&text)?);
        }
        Ok(())
    }
    pub fn probe(
        &self,
        inputs: &Path,
        work: &Path,
        name: &str,
        cancel: &AtomicBool,
    ) -> Result<MediaInfo> {
        self.probe_observed(inputs, work, name, cancel, None)
    }

    pub(crate) fn probe_observed(
        &self,
        inputs: &Path,
        work: &Path,
        name: &str,
        cancel: &AtomicBool,
        custody: Option<&Ffv1ProcessCustody<'_>>,
    ) -> Result<MediaInfo> {
        self.probe_with_timeout(inputs, work, name, cancel, Duration::from_secs(5), custody)
    }

    fn probe_before_deadline(
        &self,
        inputs: &Path,
        work: &Path,
        name: &str,
        cancel: &AtomicBool,
        deadline: Instant,
        custody: Option<&Ffv1ProcessCustody<'_>>,
    ) -> Result<MediaInfo> {
        let timeout = remaining_color_budget(deadline)?.min(Duration::from_secs(5));
        let media = self.probe_with_timeout(inputs, work, name, cancel, timeout, custody)?;
        remaining_color_budget(deadline)?;
        if cancel.load(Ordering::Acquire) {
            return Err(Error::new("Cancelled", "Complete media probe cancelled"));
        }
        Ok(media)
    }

    fn probe_with_timeout(
        &self,
        inputs: &Path,
        work: &Path,
        name: &str,
        cancel: &AtomicBool,
        timeout: Duration,
        custody: Option<&Ffv1ProcessCustody<'_>>,
    ) -> Result<MediaInfo> {
        crate::fs::validate_relative(name)?;
        if name.contains('/') {
            return Err(Error::invalid("Probe accepts one staged basename"));
        }
        let args = [
            "-v",
            "error",
            "-show_streams",
            "-show_format",
            "-of",
            "json",
        ]
        .into_iter()
        .map(OsString::from)
        .chain(std::iter::once(
            self.input_path(inputs, name).into_os_string(),
        ))
        .collect();
        let spec = self.spec("ffprobe", args, inputs, work, timeout)?;
        let r = run_checked_ffv1_original(
            custody,
            "media-probe",
            &spec,
            cancel,
            RETAIN_LOG,
            TOTAL_LOG,
            crate::jobs::MAX_ARTIFACT_BYTES,
        )?;
        MediaInfo::parse(&r.stdout)
    }
    pub fn sync_probe(
        &self,
        inputs: &Path,
        work: &Path,
        name: &str,
        cues: &[CueWindow],
        window_us: u64,
        full_scan: bool,
        cancel: &AtomicBool,
    ) -> Result<ProbeResult> {
        crate::fs::validate_relative(name)?;
        if name.contains('/')
            || cues.is_empty()
            || cues.len() > sync::MAX_SYNC_CUES
            || !(sync::MIN_WINDOW_US..=sync::MAX_WINDOW_US).contains(&window_us)
        {
            return Err(Error::invalid("Invalid bounded sync probe request"));
        }
        let input = self.input_path(inputs, name);
        let input_text = input.to_string_lossy();
        if !input_text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'_' | b'-' | b'.'))
        {
            return Err(Error::new(
                "Internal",
                "Private sync probe path is not safely representable",
            ));
        }
        let seconds = |micros: u64| format!("{}.{:06}", micros / 1_000_000, micros % 1_000_000);
        let run_probe = |filter: String, tag: &str, retain_full_scan: bool| -> Result<Vec<u8>> {
            let entries = format!("frame=best_effort_timestamp_time,pts_time:frame_tags={tag}");
            let args = vec![
                "-v".into(),
                "error".into(),
                "-f".into(),
                "lavfi".into(),
                "-i".into(),
                filter.into(),
                "-show_frames".into(),
                "-show_entries".into(),
                entries.into(),
                "-of".into(),
                "json".into(),
            ];
            let spec = self.spec(
                "ffprobe",
                args,
                inputs,
                work,
                if retain_full_scan {
                    Duration::from_secs(120)
                } else {
                    Duration::from_secs(10)
                },
            )?;
            let result = if retain_full_scan {
                run_with_output_limits(
                    &spec,
                    cancel,
                    sync::MAX_SYNC_METADATA_BYTES,
                    sync::MAX_SYNC_METADATA_BYTES,
                )?
            } else {
                run(&spec, cancel)?
            }
            .checked()?;
            let limit = if retain_full_scan {
                sync::MAX_SYNC_METADATA_BYTES
            } else {
                RETAIN_LOG
            };
            if result.stdout.is_empty() || result.stdout.len() >= limit {
                return Err(Error::limit(
                    "Sync probe metadata was empty or reached the retained-output budget",
                ));
            }
            Ok(result.stdout)
        };

        let (full_video, full_audio) = if full_scan {
            let info = self.probe(inputs, work, name, cancel)?;
            if !info.video || !info.audio || info.duration_num == 0 || info.duration_den == 0 {
                return Err(Error::unsupported(
                    "Full sync scan requires measurable audio and video streams",
                ));
            }
            let duration_us = u128::from(info.duration_num) * 1_000_000;
            let max_us = u128::from(sync::MAX_FULL_SCAN_US) * u128::from(info.duration_den);
            if duration_us > max_us {
                return Err(Error::limit(
                    "Full sync scan is bounded to sixty seconds of decoded media",
                ));
            }
            (
                Some(run_probe(
                    format!("movie=filename='{input_text}',signalstats"),
                    "lavfi.signalstats.YAVG",
                    true,
                )?),
                Some(run_probe(
                    format!(
                        "amovie=filename='{input_text}',asetnsamples=n=512:p=0,astats=metadata=1:reset=1"
                    ),
                    "lavfi.astats.Overall.Peak_level",
                    true,
                )?),
            )
        } else {
            (None, None)
        };

        let mut flashes = Vec::new();
        let mut impulses = Vec::new();
        let mut missing_video = Vec::new();
        let mut missing_audio = Vec::new();
        for cue in cues {
            if !sync::valid_cue_id(&cue.id) || cue.expected_us > 600_000_000 {
                return Err(Error::invalid("Invalid sync cue"));
            }
            if cancel.load(Ordering::Acquire) {
                return Err(Error::new("Cancelled", "Sync probe cancelled"));
            }
            let start = cue.expected_us.saturating_sub(window_us);
            let end = cue
                .expected_us
                .checked_add(window_us)
                .ok_or_else(|| Error::limit("Sync cue window overflow"))?
                .min(600_000_000);
            if end <= start {
                return Err(Error::invalid("Sync cue window is empty"));
            }
            let video_window;
            let video = if let Some(video) = &full_video {
                video.as_slice()
            } else {
                let video_filter = format!(
                    "movie=filename='{}',trim=start={}:end={},signalstats",
                    input_text,
                    seconds(start),
                    seconds(end)
                );
                video_window = run_probe(video_filter, "lavfi.signalstats.YAVG", false)?;
                video_window.as_slice()
            };
            match sync::flash(cue, video, window_us)? {
                Some(value) => flashes.push(value),
                None => missing_video.push(cue.id.clone()),
            }

            let audio_window;
            let audio = if let Some(audio) = &full_audio {
                audio.as_slice()
            } else {
                let audio_filter = format!(
                    "amovie=filename='{}',atrim=start={}:end={},asetnsamples=n=512:p=0,astats=metadata=1:reset=1",
                    input_text,
                    seconds(start),
                    seconds(end)
                );
                audio_window = run_probe(audio_filter, "lavfi.astats.Overall.Peak_level", false)?;
                audio_window.as_slice()
            };
            match sync::impulse(cue, audio, window_us)? {
                Some(value) => impulses.push(value),
                None => missing_audio.push(cue.id.clone()),
            }
        }
        Ok(ProbeResult {
            flashes,
            impulses,
            missing_video,
            missing_audio,
            exhaustive_video: full_scan,
            exhaustive_audio: full_scan,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn encode_frames(
        &self,
        inputs: &Path,
        work: &Path,
        first_frame: u64,
        frame_count: u64,
        fps_num: u32,
        fps_den: u32,
        width: u32,
        height: u32,
        cancel: &AtomicBool,
    ) -> Result<EncodedFrames> {
        self.encode_frames_observed(
            inputs,
            work,
            first_frame,
            frame_count,
            fps_num,
            fps_den,
            width,
            height,
            cancel,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn encode_frames_observed(
        &self,
        inputs: &Path,
        work: &Path,
        first_frame: u64,
        frame_count: u64,
        fps_num: u32,
        fps_den: u32,
        width: u32,
        height: u32,
        cancel: &AtomicBool,
        custody: Option<&Ffv1ProcessCustody<'_>>,
    ) -> Result<EncodedFrames> {
        let deadline = Instant::now() + self.timeout;
        if frame_count == 0
            || frame_count > 36_000
            || fps_num == 0
            || fps_den == 0
            || fps_num > 120_000
            || fps_den > 1001
            || width == 0
            || height == 0
            || width > 4096
            || height > 4096
            || u64::from(width) * u64::from(height) > 8_847_360
        {
            return Err(Error::invalid(
                "Invalid bounded Motion frame encode profile",
            ));
        }
        if cancel.load(Ordering::Acquire) {
            return Err(Error::new(
                "Cancelled",
                "Frame encode cancelled before start",
            ));
        }
        crate::color::observation_budget(width, height, frame_count)?;
        let spec = self.spec(
            "ffmpeg",
            vec!["-hide_banner".into(), "-h".into(), "filter=zscale".into()],
            inputs,
            work,
            remaining_color_budget(deadline)?.min(Duration::from_secs(5)),
        )?;
        let help = run_checked_ffv1_original(
            custody,
            "zscale-help",
            &spec,
            cancel,
            RETAIN_LOG,
            TOTAL_LOG,
            crate::jobs::MAX_ARTIFACT_BYTES,
        )?;
        if !String::from_utf8_lossy(&help.stdout).contains("Filter zscale") {
            return Err(Error::new(
                "Unsupported",
                "Pinned FFmpeg lacks reviewed zscale conversion",
            ));
        }
        let pattern = self.input_path(inputs, "%06d.png");
        let output = self.work_path(work, "mezzanine.mkv");
        let args = vec![
            "-v".into(),
            "error".into(),
            "-nostdin".into(),
            "-n".into(),
            "-framerate".into(),
            format!("{fps_num}/{fps_den}").into(),
            "-start_number".into(),
            first_frame.to_string().into(),
            "-i".into(),
            pattern.into_os_string(),
            "-frames:v".into(),
            frame_count.to_string().into(),
            "-an".into(),
            "-filter_threads".into(),
            "1".into(),
            "-vf".into(),
            crate::color::ENCODE_FILTER.into(),
            "-colorspace".into(),
            "bt709".into(),
            "-color_primaries".into(),
            "bt709".into(),
            "-color_trc".into(),
            "bt709".into(),
            "-color_range".into(),
            "tv".into(),
            "-map_metadata".into(),
            "-1".into(),
            // Declare output geometry; strict readback must still observe its SAR.
            "-aspect".into(),
            format!("{width}:{height}").into(),
            "-c:v".into(),
            "ffv1".into(),
            "-level".into(),
            "3".into(),
            "-g".into(),
            "1".into(),
            "-pix_fmt".into(),
            "yuv444p".into(),
            "-threads".into(),
            "2".into(),
            "-f".into(),
            "matroska".into(),
            output.into_os_string(),
        ];
        let spec = self.spec(
            "ffmpeg",
            args,
            inputs,
            work,
            remaining_color_budget(deadline)?,
        )?;
        run_checked_ffv1_original(
            custody,
            "encode",
            &spec,
            cancel,
            RETAIN_LOG,
            TOTAL_LOG,
            crate::jobs::MAX_ARTIFACT_BYTES,
        )?;
        let info =
            self.probe_before_deadline(work, work, "mezzanine.mkv", cancel, deadline, custody)?;
        if !info.video
            || info.audio
            || info.width != Some(width)
            || info.height != Some(height)
            || info.frames.is_some_and(|frames| frames != frame_count)
            || info.duration_num == 0
            || info.duration_den == 0
        {
            return Err(Error::new(
                "BackendFailed",
                "FFV1 mezzanine does not match the verified Motion frame plan",
            ));
        }
        let actual = u128::from(info.duration_num) * u128::from(fps_num);
        let wanted = u128::from(frame_count) * u128::from(fps_den) * u128::from(info.duration_den);
        let tolerance = u128::from(fps_den) * u128::from(info.duration_den);
        if actual.abs_diff(wanted) > tolerance {
            return Err(Error::new(
                "BackendFailed",
                "FFV1 mezzanine duration differs by more than one source frame",
            ));
        }
        let observation = self.observe_video_file_observed(
            work,
            "mezzanine.mkv",
            crate::color::ObservationProfile::MotionCanvasFfv1,
            (width, height),
            frame_count,
            fps_num,
            fps_den,
            deadline,
            cancel,
            custody,
        )?;
        Ok(EncodedFrames {
            media: info,
            observation,
        })
    }

    /// Complete post-encode observation. Reuses this Runtime's sealed tools,
    /// supervisor, limits and custody; no second decoder runtime or scheduler.
    /// Reused by the existing Host render runner; observations confer no authority.
    pub fn observe_video_file(
        &self,
        work: &Path,
        name: &str,
        profile: crate::color::ObservationProfile,
        dimensions: (u32, u32),
        frame_count: u64,
        fps_num: u32,
        fps_den: u32,
        deadline: Instant,
        cancel: &AtomicBool,
    ) -> Result<EncodedFramesObservation> {
        self.observe_video_file_observed(
            work,
            name,
            profile,
            dimensions,
            frame_count,
            fps_num,
            fps_den,
            deadline,
            cancel,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    #[doc(hidden)]
    pub fn observe_video_file_observed(
        &self,
        work: &Path,
        name: &str,
        profile: crate::color::ObservationProfile,
        dimensions: (u32, u32),
        frame_count: u64,
        fps_num: u32,
        fps_den: u32,
        deadline: Instant,
        cancel: &AtomicBool,
        custody: Option<&Ffv1ProcessCustody<'_>>,
    ) -> Result<EncodedFramesObservation> {
        use crate::{color, fs::Root};
        let (width, height) = dimensions;
        crate::fs::validate_relative(name)?;
        if name.contains('/') {
            return Err(Error::invalid(
                "Complete readback requires an owned basename",
            ));
        }
        let budget = color::observation_budget(width, height, frame_count)?;
        let root = Root::open(work, true, false)?;
        let (video_sha256, video_bytes) = reader_hash(
            root.read_file(name, crate::jobs::MAX_ARTIFACT_BYTES)?,
            crate::jobs::MAX_ARTIFACT_BYTES,
        )?;
        let entries = "stream=index,codec_type,codec_name,width,height,pix_fmt,chroma_location,sample_fmt,sample_rate,channels,channel_layout,r_frame_rate,avg_frame_rate,time_base,start_pts,duration_ts,sample_aspect_ratio,field_order,color_range,color_space,color_transfer,color_primaries:frame=media_type,stream_index,pts,duration,nb_samples,sample_fmt,channels,channel_layout,width,height,pix_fmt,chroma_location,sample_aspect_ratio,interlaced_frame,top_field_first,repeat_pict,crop_top,crop_bottom,crop_left,crop_right,color_range,color_space,color_transfer,color_primaries:frame_side_data=side_data_type:stream_side_data=side_data_type";
        let args = [
            "-v",
            "error",
            "-show_program_version",
            "-show_library_versions",
            "-show_streams",
            "-show_frames",
            "-show_entries",
            entries,
            "-of",
            "json",
        ]
        .into_iter()
        .map(OsString::from)
        .chain(std::iter::once(
            self.input_path(work, name).into_os_string(),
        ))
        .collect();
        let spec = self.spec(
            "ffprobe",
            args,
            work,
            work,
            remaining_color_budget(deadline)?,
        )?;
        let result = run_checked_ffv1_original(
            custody,
            "complete-probe",
            &spec,
            cancel,
            color::REPORT_LIMIT,
            color::REPORT_LIMIT,
            crate::jobs::MAX_ARTIFACT_BYTES,
        )?;
        let probe = color::parse_probe_profile(
            profile,
            &result.stdout,
            width,
            height,
            frame_count,
            fps_num,
            fps_den,
        )?;
        let readback_filter = color::readback_filter(
            profile,
            probe.stream["pixel_format"]
                .as_str()
                .ok_or_else(|| Error::invalid("Observed pixel format"))?,
            probe.stream["chroma_location"].as_str(),
        )?;
        let spec = self.spec(
            "ffmpeg",
            vec!["-version".into()],
            work,
            work,
            remaining_color_budget(deadline)?.min(Duration::from_secs(5)),
        )?;
        let version = run_checked_ffv1_original(
            custody,
            "decoder-version",
            &spec,
            cancel,
            RETAIN_LOG,
            TOTAL_LOG,
            crate::jobs::MAX_ARTIFACT_BYTES,
        )?;
        let version = std::str::from_utf8(&version.stdout)
            .ok()
            .and_then(|v| v.lines().next())
            .filter(|v| v.starts_with("ffmpeg version ") && v.len() <= 256)
            .ok_or_else(|| Error::new("BackendFailed", "Decoder version observation missing"))?
            .to_owned();
        // No output-fps rewriting. +1 detects a disagreement with the independent
        // complete ffprobe decode; normal completion reaches EOF before that limit.
        let args = vec![
            "-v".into(),
            "error".into(),
            "-nostdin".into(),
            "-n".into(),
            "-threads".into(),
            "2".into(),
            "-i".into(),
            self.input_path(work, name).into_os_string(),
            "-map".into(),
            "0:v:0".into(),
            "-an".into(),
            "-sn".into(),
            "-dn".into(),
            "-map_metadata".into(),
            "-1".into(),
            "-vsync".into(),
            "0".into(),
            "-filter_threads".into(),
            "1".into(),
            "-vf".into(),
            readback_filter.into(),
            "-frames:v".into(),
            (frame_count + 1).to_string().into(),
            "-c:v".into(),
            "png".into(),
            "-pix_fmt".into(),
            "rgba".into(),
            "-compression_level".into(),
            "9".into(),
            "-threads".into(),
            "2".into(),
            "-start_number".into(),
            "0".into(),
            "-f".into(),
            "image2".into(),
            self.work_path(work, "decoded-%06d.png").into_os_string(),
        ];
        let spec = self.spec(
            "ffmpeg",
            args,
            work,
            work,
            remaining_color_budget(deadline)?,
        )?;
        run_checked_ffv1_original(
            custody,
            "decode",
            &spec,
            cancel,
            RETAIN_LOG,
            TOTAL_LOG,
            budget.png_bytes_upper_bound,
        )?;
        if root.exists(&format!("decoded-{frame_count:06}.png"))? {
            return Err(Error::new(
                "BackendFailed",
                "FFmpeg decoded more frames than the complete observation",
            ));
        }
        let mut frames = Vec::with_capacity(frame_count as usize);
        let mut total = 0u64;
        for index in 0..frame_count {
            remaining_color_budget(deadline)?;
            if cancel.load(Ordering::Acquire) {
                return Err(Error::new("Cancelled", "Complete frame readback cancelled"));
            }
            let png = root.read(&format!("decoded-{index:06}.png"), color::PNG_LIMIT)?;
            let observed = color::inspect_decoded_png(&png, index, width, height)?;
            if observed.bytes > budget.png_bytes_upper_bound {
                return Err(Error::limit("Decoded PNG exceeds reviewed encoder bound"));
            }
            total = total
                .checked_add(observed.bytes)
                .ok_or_else(|| Error::limit("PNG total overflow"))?;
            if total > color::RETAINED_PNG_LIMIT {
                return Err(Error::limit(
                    "Decoded PNG aggregate exceeds observation budget",
                ));
            }
            frames.push(observed);
        }
        let roster = close_frame_readback(
            root.read_file(name, crate::jobs::MAX_ARTIFACT_BYTES)?,
            &video_sha256,
            video_bytes,
            &frames,
            deadline,
            cancel,
        )?;
        Ok(EncodedFramesObservation {
            video_sha256,
            video_bytes,
            probe,
            frames,
            retained_png_bytes: total,
            rgba_roster_sha256: roster,
            ffmpeg_version: version,
            ffmpeg_sha256: self.ffmpeg.sha256.clone(),
            ffprobe_sha256: self.ffprobe.sha256.clone(),
        })
    }

    /// Strict original decoded-frame readback using the same owner-pinned
    /// FFmpeg binary and confined runtime as FFV1 encoding / AV mux.
    /// No caller-provided filters, argv, absolute paths or output filename.
    pub fn decoded_native_rgb_frame(
        &self,
        inputs: &Path,
        video: &str,
        frame: u64,
        width: u32,
        height: u32,
        cancel: &AtomicBool,
    ) -> Result<Vec<u8>> {
        crate::fs::validate_relative(video)?;
        if !matches!(video, "original.mkv" | "master.mp4")
            || frame >= crate::caption_pixels::MAX_FRAMES
            || width == 0
            || width > crate::caption_pixels::MAX_WIDTH
            || height == 0
            || height > crate::caption_pixels::MAX_HEIGHT
        {
            return Err(Error::invalid(
                "Native caption frame decoder input is unsupported",
            ));
        }
        let size = usize::try_from(u64::from(width) * u64::from(height) * 3)
            .map_err(|_| Error::limit("Decoded RGB size overflow"))?;
        let work = PrivateDir::new(Path::new("/tmp"))?;
        let frame_ref = self.input_path(inputs, video);
        let target = self.work_path(work.path(), "caption.rgb");
        let args = vec![
            "-v".into(),
            "error".into(),
            "-nostdin".into(),
            "-i".into(),
            frame_ref.into_os_string(),
            "-an".into(),
            "-sn".into(),
            "-dn".into(),
            "-vf".into(),
            format!("select=eq(n\\,{frame})").into(),
            "-vsync".into(),
            "0".into(),
            "-frames:v".into(),
            "1".into(),
            "-pix_fmt".into(),
            "rgb24".into(),
            "-f".into(),
            "rawvideo".into(),
            target.into_os_string(),
        ];
        run(
            &self.spec("ffmpeg", args, inputs, work.path(), Duration::from_secs(25))?,
            cancel,
        )?
        .checked()?;
        let output = crate::fs::Root::open(work.path(), true, false)?;
        let frame = output.read("caption.rgb", size + 1)?;
        if frame.len() != size {
            return Err(Error::new(
                "Conflict",
                "Decoded original media frame was absent or did not match output geometry",
            ));
        }
        Ok(frame)
    }

    pub(crate) fn derive_audio(
        &self,
        request: AudioDerivativeExecution<'_>,
        cancel: &AtomicBool,
    ) -> Result<AudioDerivativeRun> {
        let recipe_sha256 = request.profile.recipe_sha256()?;
        let input = self.input_path(request.inputs, "source.wav");
        let output = self.work_path(request.work, "derived.wav");
        let spec = self.spec(
            "ffmpeg",
            audio_derivative_args(request.profile, &input, &output),
            request.inputs,
            request.work,
            self.timeout,
        );
        let mut observation = ProcessObservation::default();
        let mut originals = Vec::new();
        let process = match spec {
            Ok(spec) => {
                let argv = process_spec_original(&spec, "Audio derivative")
                    .and_then(|value| {
                        serde_json::to_vec(&value)
                            .map_err(|_| Error::invalid("Audio argv serialization failed"))
                    })
                    .and_then(|bytes| {
                        retain_audio_derivative_original(
                            request.output,
                            &format!(
                                "{}.audio-derive.ffmpeg.argv.original.json",
                                request.output_path
                            ),
                            &bytes,
                            AUDIO_DERIVATIVE_METADATA_LIMIT,
                        )
                    });
                match argv {
                    Ok(artifact) => {
                        originals.push(artifact);
                        run_with_output_limits_observed(
                            &spec,
                            cancel,
                            AUDIO_DERIVATIVE_STREAM_LIMIT,
                            AUDIO_DERIVATIVE_STREAM_LIMIT,
                            Some(&mut observation),
                        )
                    }
                    Err(original) => Err(original),
                }
            }
            Err(original) => Err(original),
        };
        if observation.supervisor_error.is_none()
            && let Err(error) = &process
        {
            observation.supervisor_error = Some(serde_json::json!({
                "code":error.code,"message":error.message,"outcome_known":error.outcome_known
            }));
        }
        let mut custody_error = None;
        for (name, stream) in [
            ("stdout", &observation.stdout),
            ("stderr", &observation.stderr),
        ] {
            if let Some(stream) = stream {
                match retain_audio_derivative_original(
                    request.output,
                    &format!(
                        "{}.audio-derive.ffmpeg.{name}.original.bin",
                        request.output_path
                    ),
                    &stream.retained,
                    AUDIO_DERIVATIVE_STREAM_LIMIT,
                ) {
                    Ok(artifact) => originals.push(artifact),
                    Err(error) => {
                        if custody_error.is_none() {
                            custody_error = Some(error);
                        }
                    }
                }
            }
        }
        let terminal = serde_json::json!({
            "schema":"semwright.audio-derivative-process-original/1",
            "profile":request.profile.id(),"source_sha256":request.source_sha256,
            "recipe_sha256":recipe_sha256,"ffmpeg_sha256":self.ffmpeg.sha256,
            "process":observation.metadata(),
            "process_result_received":process.is_ok(),
            "retained_prefix_limit":AUDIO_DERIVATIVE_STREAM_LIMIT,
            "per_stream_total_limit":AUDIO_DERIVATIVE_STREAM_LIMIT,
            "custody_error":custody_error.as_ref().map(|error| serde_json::json!({
                "code":error.code,"message":error.message,"outcome_known":error.outcome_known
            })),
            "capture_scope":"same-FFmpeg-spawn-before-checked-and-WAV-validation",
            "physical_acceptance":false
        });
        let retained = serde_json::to_vec(&terminal)
            .map_err(|_| Error::invalid("Audio process terminal serialization failed"))
            .and_then(|bytes| {
                retain_audio_derivative_original(
                    request.output,
                    &format!(
                        "{}.audio-derive.ffmpeg.terminal.original.json",
                        request.output_path
                    ),
                    &bytes,
                    AUDIO_DERIVATIVE_METADATA_LIMIT,
                )
            });
        match retained {
            Ok(artifact) => originals.push(artifact),
            Err(error) => {
                if custody_error.is_none() {
                    custody_error = Some(error);
                }
            }
        }
        // Actual wait/cancel/nonzero classification stays primary. Only a known
        // successful process can be rejected next for incomplete capture.
        let original =
            checked_audio_derivative_process(process, &observation).map(|_| AudioDerivativeRun {
                recipe_sha256,
                runtime_sha256: self.ffmpeg.sha256.clone(),
                originals,
            });
        finish_audio_derivative_original(original, custody_error.map_or(Ok(()), Err))
    }

    pub fn extract_audio(
        &self,
        inputs: &Path,
        work: &Path,
        name: &str,
        sample_rate: u32,
        channels: u16,
        cancel: &AtomicBool,
    ) -> Result<MediaInfo> {
        self.extract_audio_observed(
            inputs,
            work,
            name,
            sample_rate,
            channels,
            cancel,
            crate::jobs::MAX_ARTIFACT_BYTES,
            None,
        )
    }

    pub(crate) fn extract_audio_observed(
        &self,
        inputs: &Path,
        work: &Path,
        name: &str,
        sample_rate: u32,
        channels: u16,
        cancel: &AtomicBool,
        maximum_bytes: u64,
        custody: Option<&Ffv1ProcessCustody<'_>>,
    ) -> Result<MediaInfo> {
        crate::fs::validate_relative(name)?;
        if name.contains('/')
            || sample_rate != 48_000
            || channels != 2
            || maximum_bytes == 0
            || maximum_bytes > crate::jobs::MAX_ARTIFACT_BYTES
            || cancel.load(Ordering::Acquire)
        {
            return Err(Error::invalid(
                "Final AV audio extraction requires one bounded 48 kHz stereo input",
            ));
        }
        let input = self.input_path(inputs, name);
        let output = self.work_path(work, "decoded-audio.wav");
        let args = vec![
            "-v".into(),
            "error".into(),
            "-nostdin".into(),
            "-i".into(),
            input.into_os_string(),
            "-map".into(),
            "0:a:0".into(),
            "-vn".into(),
            "-ac".into(),
            channels.to_string().into(),
            "-ar".into(),
            sample_rate.to_string().into(),
            "-c:a".into(),
            "pcm_s16le".into(),
            "-f".into(),
            "wav".into(),
            output.into_os_string(),
        ];
        run_checked_ffv1_original(
            custody,
            "audio-extract",
            &self.spec("ffmpeg", args, inputs, work, self.timeout)?,
            cancel,
            RETAIN_LOG,
            TOTAL_LOG,
            maximum_bytes,
        )?;
        let info = self.probe_observed(work, work, "decoded-audio.wav", cancel, custody)?;
        if !info.audio
            || info.video
            || info.sample_rate != Some(sample_rate)
            || info.channels != Some(channels)
            || info.audio_sample_frames.is_none()
        {
            return Err(Error::new(
                "BackendFailed",
                "Decoded final audio WAV does not match the certified delivery layout",
            ));
        }
        Ok(info)
    }

    pub fn render(
        &self,
        inputs: &Path,
        work: &Path,
        profile: &RenderProfile,
        cancel: &AtomicBool,
        deadline: Instant,
    ) -> Result<ProcessResult> {
        self.render_observed(inputs, work, profile, cancel, deadline, None)
    }

    pub(crate) fn render_observed(
        &self,
        inputs: &Path,
        work: &Path,
        profile: &RenderProfile,
        cancel: &AtomicBool,
        deadline: Instant,
        custody: Option<&Ffv1ProcessCustody<'_>>,
    ) -> Result<ProcessResult> {
        let args = render_argv(
            self.input_path(inputs, "project.mlt").into_os_string(),
            self.work_path(work, &format!("partial.{}", profile.extension))
                .into_os_string(),
            profile,
        );
        let spec = self.spec(
            "melt",
            args,
            inputs,
            work,
            remaining_color_budget(deadline)?,
        )?;
        run_checked_ffv1_original(
            custody,
            "render",
            &spec,
            cancel,
            RETAIN_LOG,
            TOTAL_LOG,
            crate::jobs::MAX_ARTIFACT_BYTES,
        )
    }
    pub fn validate_output(
        &self,
        work: &Path,
        profile: &RenderProfile,
        expected: &Profile,
        frames: u64,
        cancel: &AtomicBool,
        deadline: Instant,
    ) -> Result<ValidatedOutput> {
        self.validate_output_observed(work, profile, expected, frames, cancel, deadline, None)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn validate_output_observed(
        &self,
        work: &Path,
        profile: &RenderProfile,
        expected: &Profile,
        frames: u64,
        cancel: &AtomicBool,
        deadline: Instant,
        custody: Option<&Ffv1ProcessCustody<'_>>,
    ) -> Result<ValidatedOutput> {
        remaining_color_budget(deadline)?;
        let filename = format!("partial.{}", profile.extension);
        let mut info =
            self.probe_before_deadline(work, work, &filename, cancel, deadline, custody)?;
        remaining_color_budget(deadline)?;
        let observation = if let Some(selected) = complete_observation_profile(profile) {
            let observed = self.observe_video_file_observed(
                work,
                &filename,
                selected,
                (expected.width, expected.height),
                frames,
                expected.fps.num,
                expected.fps.den,
                deadline,
                cancel,
                custody,
            )?;
            info.frames = Some(observed.frames.len() as u64);
            Some(observed)
        } else {
            None
        };
        validate_render_media(&info, profile, expected, frames)?;
        Ok(ValidatedOutput {
            media: info,
            observation,
        })
    }
}

/// Check exactly one independently decoded video stream, never trusting
/// the estimated Matroska duration or its often-missing nb_frames metadata.
/// This returns only a count; raw media paths and decoder text stay private.
pub fn parse_exact_ffv1_decoded_frame_count(bytes: &[u8]) -> Result<u64> {
    if bytes.is_empty() || bytes.len() > 4096 {
        return Err(Error::limit(
            "Decoded video-only frame evidence is absent or unbounded",
        ));
    }
    let doc: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|_| Error::invalid("Decoded video-only frame evidence is malformed"))?;
    let streams = doc
        .get("streams")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| Error::invalid("Decoded video-only frame evidence has no stream list"))?;
    if streams.len() != 1 {
        return Err(Error::new(
            "BackendFailed",
            "Decoded video-only evidence must have one stream",
        ));
    }
    let stream = &streams[0];
    if stream.get("codec_type").and_then(serde_json::Value::as_str) != Some("video")
        || stream.get("codec_name").and_then(serde_json::Value::as_str) != Some("ffv1")
        || !stream
            .get("width")
            .and_then(serde_json::Value::as_u64)
            .is_some_and(|n| (16..=8192).contains(&n))
        || !stream
            .get("height")
            .and_then(serde_json::Value::as_u64)
            .is_some_and(|n| (16..=8192).contains(&n))
    {
        return Err(Error::new(
            "BackendFailed",
            "Decoded video-only stream codec or geometry differs",
        ));
    }
    let text = stream
        .get("nb_read_frames")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| Error::new("BackendFailed", "Decoder did not count video frames"))?;
    if text.is_empty() || text.len() > 6 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Error::invalid("Decoded frame count is not bounded decimal"));
    }
    let frames: u64 = text
        .parse()
        .map_err(|_| Error::invalid("Decoded frame count cannot be parsed"))?;
    if !(1..=36_000).contains(&frames) {
        return Err(Error::limit(
            "Decoded frame count is outside the render budget",
        ));
    }
    Ok(frames)
}

pub fn validate_render_media(
    info: &MediaInfo,
    profile: &RenderProfile,
    expected: &Profile,
    frames: u64,
) -> Result<()> {
    if profile.video_codec.is_some() {
        if info.width != Some(expected.width) || info.height != Some(expected.height) {
            return Err(Error::new(
                "BackendFailed",
                "Rendered dimensions do not match plan",
            ));
        }
        if let Some(observed_frames) = info.frames
            && observed_frames != frames
        {
            return Err(Error::new(
                "BackendFailed",
                format!(
                    "Rendered frame count differs from plan: observed {observed_frames}, expected {frames}"
                ),
            ));
        }
    }
    if info.duration_num == 0 || info.duration_den == 0 {
        return Err(Error::new(
            "BackendFailed",
            "Rendered artifact has no measurable duration",
        ));
    }
    let actual = u128::from(info.duration_num) * u128::from(expected.fps.num);
    let wanted = u128::from(frames) * u128::from(expected.fps.den) * u128::from(info.duration_den);
    let tolerance = u128::from(expected.fps.den) * u128::from(info.duration_den);
    if actual.abs_diff(wanted) > tolerance {
        return Err(Error::new(
            "BackendFailed",
            format!(
                "Rendered duration differs by more than one project frame: observed {}/{}, expected {} frames at {}/{} fps",
                info.duration_num, info.duration_den, frames, expected.fps.num, expected.fps.den
            ),
        ));
    }
    match profile.audio_codec {
        Some(_) if !info.audio => {
            return Err(Error::new(
                "BackendFailed",
                "Curated audio-bearing render profile requires an audio stream",
            ));
        }
        None if info.audio => {
            return Err(Error::new(
                "BackendFailed",
                "Video-only lossless render unexpectedly contains audio",
            ));
        }
        _ => {}
    }
    if profile.id == "lossless-video-only"
        && (!info.video
            || info.frames != Some(frames)
            || info.codecs.len() != 1
            || info.codecs[0] != "ffv1")
    {
        return Err(Error::new(
            "BackendFailed",
            "Video-only lossless render must contain only FFV1 video",
        ));
    }
    Ok(())
}

pub fn render_argv(
    project: impl Into<OsString>,
    output: impl Into<OsString>,
    profile: &RenderProfile,
) -> Vec<OsString> {
    let mut args = vec![
        project.into(),
        "-silent".into(),
        "-consumer".into(),
        format!("avformat:{}", output.into().to_string_lossy()).into(),
        format!("f={}", profile.container).into(),
    ];
    args.extend(render_processing_args(profile));
    if let Some(codec) = profile.video_codec {
        args.push(format!("vcodec={codec}").into());
        if codec == "libx264" {
            args.extend(h264_encoding_args());
        }
    } else {
        args.push("vn=1".into());
    }
    if let Some(codec) = profile.audio_codec {
        args.push(format!("acodec={codec}").into());
    } else {
        // MLT avformat consumer: omit audio frames altogether. This is not
        // the same as an encoded silent PCM stream and is checked post-render.
        args.push("an=1".into());
    }
    args
}

fn h264_encoding_args() -> Vec<OsString> {
    // The 52-second launch-film render demonstrated that libx264 medium can exceed
    // the bounded CI wall-clock budget. Keep acceleration inside the encoder only:
    // CRF 18 preserves high-quality motion graphics while veryfast trades file size
    // for throughput without changing timeline semantics or MLT processing topology.
    vec![
        "pix_fmt=yuv420p".into(),
        "crf=18".into(),
        "preset=veryfast".into(),
    ]
}

fn render_processing_args(profile: &RenderProfile) -> Vec<OsString> {
    // MLT real_time=-1 still runs asynchronous frame workers. That path repeatedly
    // SIGSEGV'd for the 1080p H.264 launch-film graph on Ubuntu's MLT 7.22, so H.264
    // stays synchronous at the MLT layer. Keep libx264 at two codec threads: a full
    // 1,560-frame CI film render completed quickly with this setting, while the later
    // threads=0 experiment caused the bounded 50-frame live conformance render to hit
    // its 120-second wall-clock deadline.
    if profile.video_codec == Some("libx264") {
        vec!["real_time=0".into(), "threads=2".into()]
    } else {
        vec!["real_time=-1".into(), "threads=2".into()]
    }
}

#[cfg(test)]
mod native_diagnostic_tests {
    use super::*;

    fn process() -> ProcessResult {
        ProcessResult {
            exit_code: Some(0),
            term_signal: None,
            stdout: vec![0, 255, 10],
            stderr: vec![b'x'; 20000],
            cancelled: false,
            timed_out: false,
            output_exceeded: false,
        }
    }
    fn probe() -> serde_json::Value {
        serde_json::json!({"schema":1,"operation":"probe","media":{
            "streams":[{"codec_type":"audio","nb_frames":"2695"},
                {"codec_type":"video","nb_frames":"1345","avg_frame_rate":"25/1",
                 "r_frame_rate":"25/1","duration":"53.800000","duration_ts":688640,
                 "time_base":"1/12800","start_time":"0.000000","start_pts":0}],
            "format":{"duration":"53.800000","size":"1234"}}})
    }

    #[test]
    fn logs_are_lossless_hex_and_bounded_local_receipt() {
        let bytes =
            native_render_diagnostic(b"<mlt/>", &["project.mlt".into()], &process()).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(bytes.len() <= NATIVE_DIAGNOSTIC_LIMIT);
        assert_eq!(value["stdout_hex"], "00ff0a");
        assert_eq!(value["stderr_hex"].as_str().unwrap().len(), 32768);
        assert_eq!(value["stderr_retained_bytes"], 20000);
        assert_eq!(value["stderr_diagnostic_truncated"], true);
        assert_eq!(value["project_xml_sha256"], crate::hash::sha256(b"<mlt/>"));
    }

    #[test]
    fn diagnostic_argv_and_xml_caps_fail_closed() {
        assert!(native_render_diagnostic(b"x", &vec!["a".into(); 33], &process()).is_err());
        assert!(native_render_diagnostic(b"x", &["a".repeat(4097).into()], &process()).is_err());
        assert!(
            native_render_diagnostic(&vec![0; crate::xml::MAX_XML + 1], &[], &process()).is_err()
        );
    }

    #[test]
    fn failure_filename_requires_scoped_output_and_exact_job() {
        let job = "render:0123456789abcdef0123456789abcdef";
        assert_eq!(
            native_failure_filename("output/master.mp4", job).unwrap(),
            "output/master.mp4.native-failure-0123456789abcdef0123456789abcdef.json"
        );
        for output in [
            "/tmp/foreign.mp4",
            "../foreign.mp4",
            "output/../foreign.mp4",
        ] {
            assert!(native_failure_filename(output, job).is_err());
        }
        for bad in [
            "0123456789abcdef0123456789abcdef",
            "render:../foreign",
            "render:ABCDEF0123456789abcdef0123456789a",
        ] {
            assert!(native_failure_filename("output/master.mp4", bad).is_err());
        }
    }

    #[test]
    fn raw_probe_selects_video_and_preserves_clock_without_audio_packet_confusion() {
        let bytes = serde_json::to_vec(&probe()).unwrap();
        let value = raw_video_probe_observation(&bytes).unwrap();
        assert_eq!(value["first_video_stream"]["nb_frames"], "1345");
        assert_eq!(value["first_video_stream"]["avg_frame_rate"], "25/1");
        assert_eq!(value["first_video_stream"]["r_frame_rate"], "25/1");
        assert_eq!(value["first_video_stream"]["duration"], "53.800000");
        assert_eq!(value["first_video_stream"]["start_pts"], 0);
        assert_eq!(value["raw_probe_sha256"], crate::hash::sha256(&bytes));
    }

    #[test]
    fn raw_probe_envelope_streams_and_scalars_are_bounded() {
        for value in [
            serde_json::json!({}),
            serde_json::json!({"schema":1,"operation":"probe","media":{"streams":[]}}),
        ] {
            assert!(raw_video_probe_observation(&serde_json::to_vec(&value).unwrap()).is_err());
        }
        let mut value = probe();
        value["media"]["streams"][1]["duration"] = serde_json::json!("x".repeat(513));
        assert!(raw_video_probe_observation(&serde_json::to_vec(&value).unwrap()).is_err());
        assert!(raw_video_probe_observation(&vec![0; 262145]).is_err());
    }

    #[test]
    fn local_receipt_write_is_single_link_0600_and_never_replaces() {
        let scratch = crate::fs::PrivateDir::new(Path::new("/tmp")).unwrap();
        let root = crate::fs::Root::open(scratch.path(), true, true).unwrap();
        root.write_new("scratch", NATIVE_DIAGNOSTIC_FILE, b"{}")
            .unwrap();
        assert!(
            root.write_new("scratch", NATIVE_DIAGNOSTIC_FILE, b"changed")
                .is_err()
        );
        let metadata = std::fs::metadata(scratch.path().join(NATIVE_DIAGNOSTIC_FILE)).unwrap();
        assert_eq!(metadata.mode() & 0o777, 0o600);
        assert_eq!(metadata.nlink(), 1);
        assert_eq!(root.read(NATIVE_DIAGNOSTIC_FILE, 2).unwrap(), b"{}");
    }
}

#[derive(Clone, Debug)]
pub struct RenderProfile {
    pub id: &'static str,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub video_codec: Option<&'static str>,
    /// None explicitly disables the MLT avformat consumer's audio stream.
    pub audio_codec: Option<&'static str>,
    pub container: &'static str,
    pub extension: &'static str,
}
impl RenderProfile {
    pub fn all() -> Vec<Self> {
        vec![
            Self {
                id: "h264-1080p",
                width: Some(1920),
                height: Some(1080),
                video_codec: Some("libx264"),
                audio_codec: Some("aac"),
                container: "mp4",
                extension: "mp4",
            },
            Self {
                id: "h264-720p",
                width: Some(1280),
                height: Some(720),
                video_codec: Some("libx264"),
                audio_codec: Some("aac"),
                container: "mp4",
                extension: "mp4",
            },
            Self {
                id: "lossless",
                width: None,
                height: None,
                video_codec: Some("ffv1"),
                audio_codec: Some("pcm_s16le"),
                container: "matroska",
                extension: "mkv",
            },
            Self {
                // A separate, explicitly opt-in profile for semantic MLT
                // cuts that will become the video input to native av.mux.
                // The existing "lossless" PCM transport output is unchanged.
                id: "lossless-video-only",
                width: None,
                height: None,
                video_codec: Some("ffv1"),
                audio_codec: None,
                container: "matroska",
                extension: "mkv",
            },
            Self {
                id: "audio-wav",
                width: None,
                height: None,
                video_codec: None,
                audio_codec: Some("pcm_s16le"),
                container: "wav",
                extension: "wav",
            },
        ]
    }

    /// Project this backend profile into the shared semantic render contract.
    /// Native encoder names stay private to this driver.
    pub fn semantic(&self) -> semwright_video_domain::render::RenderPreset {
        semwright_video_domain::render::RenderPreset {
            id: self.id.to_owned(),
            width: self.width,
            height: self.height,
            video_codec: self.video_codec.map(|codec| {
                if codec == "libx264" {
                    "h264".to_owned()
                } else {
                    codec.to_owned()
                }
            }),
            audio_codec: self.audio_codec.map(str::to_owned),
            container: self.container.to_owned(),
            extension: self.extension.to_owned(),
        }
    }

    pub fn get(id: &str) -> Result<Self> {
        Self::all()
            .into_iter()
            .find(|p| p.id == id)
            .ok_or_else(|| Error::invalid("Unknown render profile"))
    }
    pub fn available(&self, catalog: &ServiceCatalog) -> bool {
        catalog.has("consumers", "avformat")
            && self
                .audio_codec
                .is_none_or(|codec| catalog.has("audio_codecs", codec))
            && self
                .video_codec
                .is_none_or(|v| catalog.has("video_codecs", v))
    }
    pub fn json(&self, catalog: Option<&ServiceCatalog>) -> Value {
        obj([
            ("id", self.id.into()),
            (
                "available",
                catalog.is_some_and(|c| self.available(c)).into(),
            ),
            (
                "width",
                self.width.map_or(Value::Null, |v| u64::from(v).into()),
            ),
            (
                "height",
                self.height.map_or(Value::Null, |v| u64::from(v).into()),
            ),
            (
                "video_codec",
                self.video_codec.map_or(Value::Null, Into::into),
            ),
            (
                "audio_codec",
                self.audio_codec.map_or(Value::Null, Into::into),
            ),
            ("container", self.container.into()),
            ("extension", self.extension.into()),
        ])
    }
}
#[derive(Clone, Debug, Default)]
pub struct MediaInfo {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub frames: Option<u64>,
    pub duration_num: u64,
    pub duration_den: u64,
    pub audio: bool,
    pub video: bool,
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
    pub audio_sample_frames: Option<u64>,
    pub codecs: Vec<String>,
}
impl MediaInfo {
    /// Capacity expressed in project frames, rounded up once at the last partial frame.
    pub fn frame_capacity(&self, rate: crate::time::FrameRate) -> Result<Option<u64>> {
        rate.validate()?;
        if self.duration_num == 0 || self.duration_den == 0 {
            return Ok(None);
        }
        let numerator = u128::from(self.duration_num) * u128::from(rate.num);
        let denominator = u128::from(self.duration_den) * u128::from(rate.den);
        let frames = numerator.div_ceil(denominator);
        Ok(Some(u64::try_from(frames).map_err(|_| {
            Error::limit("Media duration exceeds frame capacity")
        })?))
    }

    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let v = json::parse(bytes)?;
        let streams = v.get("streams")?.as_array()?;
        if streams.is_empty() || streams.len() > 16 {
            return Err(Error::limit("Probe stream count outside budget"));
        }
        let mut info = Self::default();
        for s in streams {
            let kind = s.str("codec_type")?;
            if let Some(codec) = s.opt("codec_name").and_then(|v| v.string().ok()) {
                info.codecs.push(json::display(codec));
            }
            if kind == "video" && !info.video {
                info.video = true;
                info.width = s
                    .opt("width")
                    .map(Value::u64)
                    .transpose()?
                    .and_then(|n| u32::try_from(n).ok());
                info.height = s
                    .opt("height")
                    .map(Value::u64)
                    .transpose()?
                    .and_then(|n| u32::try_from(n).ok());
                info.frames = s
                    .opt("nb_frames")
                    .and_then(|v| v.string().ok())
                    .and_then(|s| s.parse().ok());
            }
            if kind == "audio" {
                info.audio = true;
                if info.sample_rate.is_none() {
                    info.sample_rate = s
                        .opt("sample_rate")
                        .and_then(|value| value.string().ok())
                        .and_then(|value| value.parse::<u32>().ok());
                }
                if info.channels.is_none() {
                    info.channels = s
                        .opt("channels")
                        .and_then(|value| value.u64().ok())
                        .and_then(|value| u16::try_from(value).ok());
                }
                if info.audio_sample_frames.is_none()
                    && let (Some(sample_rate), Some(duration_ts), Some(time_base)) = (
                        info.sample_rate,
                        s.opt("duration_ts").and_then(|value| value.u64().ok()),
                        s.opt("time_base").and_then(|value| value.string().ok()),
                    )
                    && let Some((numerator, denominator)) = time_base.split_once('/')
                    && numerator == "1"
                    && denominator.parse::<u32>().ok() == Some(sample_rate)
                {
                    info.audio_sample_frames = Some(duration_ts);
                }
            }
            if info.duration_num == 0 {
                if let (Some(ts), Some(tb)) = (
                    s.opt("duration_ts").and_then(|v| v.u64().ok()),
                    s.opt("time_base").and_then(|v| v.string().ok()),
                ) {
                    if let Some((a, b)) = tb.split_once('/') {
                        let a = a
                            .parse::<u64>()
                            .map_err(|_| Error::invalid("Probe time base numerator"))?;
                        let b = b
                            .parse::<u64>()
                            .map_err(|_| Error::invalid("Probe time base denominator"))?;
                        if b > 0 {
                            info.duration_num = ts
                                .checked_mul(a)
                                .ok_or_else(|| Error::invalid("Probe duration overflow"))?;
                            info.duration_den = b;
                        }
                    }
                }
            }
        }
        if info.duration_num == 0 {
            if let Some(text) = v
                .opt("format")
                .and_then(|f| f.opt("duration"))
                .and_then(|v| v.string().ok())
            {
                let (a, b) = text.split_once('.').unwrap_or((text, ""));
                if b.len() > 9 || !a.bytes().chain(b.bytes()).all(|c| c.is_ascii_digit()) {
                    return Err(Error::invalid("Invalid decimal probe duration"));
                }
                let den = 10u64.pow(b.len() as u32);
                let n = a
                    .parse::<u64>()
                    .map_err(|_| Error::invalid("Probe duration integer"))?
                    .checked_mul(den)
                    .and_then(|n| n.checked_add(b.parse::<u64>().unwrap_or(0)))
                    .ok_or_else(|| Error::invalid("Probe duration overflow"))?;
                info.duration_num = n;
                info.duration_den = den;
            }
        }
        Ok(info)
    }
    pub fn json(&self) -> Value {
        obj([
            (
                "width",
                self.width.map_or(Value::Null, |n| u64::from(n).into()),
            ),
            (
                "height",
                self.height.map_or(Value::Null, |n| u64::from(n).into()),
            ),
            ("frames", self.frames.map_or(Value::Null, Into::into)),
            ("duration_num", self.duration_num.into()),
            ("duration_den", self.duration_den.into()),
            ("audio", self.audio.into()),
            ("video", self.video.into()),
            ("codecs", array(self.codecs.iter().cloned().map(Into::into))),
        ])
    }
}

#[cfg(test)]
mod tool_owner_tests {
    use super::{RenderProfile, h264_encoding_args, render_processing_args, trusted_tool_owner};
    use std::path::Path;

    #[test]
    fn video_only_profile_is_opt_in_and_preserves_legacy_audio_profiles() {
        let only = RenderProfile::get("lossless-video-only").unwrap();
        assert_eq!(only.video_codec, Some("ffv1"));
        assert_eq!(only.audio_codec, None);
        assert_eq!(only.semantic().audio_codec, None);
        let command = super::render_argv("project.mlt", "intermediate.mkv", &only)
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(command.iter().any(|value| value == "an=1"));
        assert!(command.iter().any(|value| value == "vcodec=ffv1"));
        assert!(!command.iter().any(|value| value.starts_with("acodec=")));

        let legacy = RenderProfile::get("lossless").unwrap();
        assert_eq!(legacy.audio_codec, Some("pcm_s16le"));
        assert_eq!(legacy.semantic().audio_codec.as_deref(), Some("pcm_s16le"));
        let original = super::render_argv("project.mlt", "original.mkv", &legacy)
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(original.iter().any(|value| value == "acodec=pcm_s16le"));
        assert!(!original.iter().any(|value| value == "an=1"));
    }

    #[test]
    fn strict_video_only_receipts_refuse_pcm_and_forged_profiles() {
        let profile = RenderProfile::get("lossless-video-only").unwrap();
        let original = RenderProfile::get("lossless").unwrap();
        let expected = crate::model::Profile {
            width: 160,
            height: 90,
            fps: crate::time::FrameRate::new(25, 1).unwrap(),
            progressive: true,
            sample_aspect: (1, 1),
            display_aspect: (16, 9),
            colorspace: 709,
            audio_channels: 2,
        };
        let mut measured = super::MediaInfo {
            width: Some(160),
            height: Some(90),
            frames: Some(50),
            duration_num: 2,
            duration_den: 1,
            video: true,
            audio: false,
            codecs: vec!["ffv1".into()],
            ..Default::default()
        };
        assert!(super::validate_render_media(&measured, &profile, &expected, 50).is_ok());
        assert!(super::validate_render_media(&measured, &original, &expected, 50).is_err());
        measured.audio = true;
        measured.codecs.push("pcm_s16le".into());
        assert!(super::validate_render_media(&measured, &profile, &expected, 50).is_err());
        assert!(super::validate_render_media(&measured, &original, &expected, 50).is_ok());
        measured.audio = false;
        measured.codecs = vec!["aac".into()];
        assert!(super::validate_render_media(&measured, &profile, &expected, 50).is_err());
        measured.codecs = vec!["ffv1".into()];
        measured.frames = Some(49);
        assert!(super::validate_render_media(&measured, &profile, &expected, 50).is_err());
        measured.frames = None;
        assert!(super::validate_render_media(&measured, &profile, &expected, 50).is_err());
        measured.frames = Some(50);
        measured.duration_num = 1;
        assert!(super::validate_render_media(&measured, &profile, &expected, 50).is_err());
        measured.duration_num = 2;
        measured.width = Some(1920);
        assert!(super::validate_render_media(&measured, &profile, &expected, 50).is_err());
    }

    #[test]
    fn decoded_frame_counter_requires_exact_bounded_ffv1_observation() {
        let fixture = serde_json::json!({
            "streams": [{"codec_type":"video","codec_name":"ffv1",
                         "width":160,"height":90,"nb_read_frames":"50"}]
        });
        let correct = serde_json::to_vec(&fixture).unwrap();
        assert_eq!(
            super::parse_exact_ffv1_decoded_frame_count(&correct).unwrap(),
            50
        );
        let mut mutated = fixture;
        for (field, value) in [
            ("nb_read_frames", serde_json::json!("N/A")),
            ("nb_read_frames", serde_json::json!("0")),
            ("nb_read_frames", serde_json::json!("36001")),
            ("nb_read_frames", serde_json::Value::Null),
            ("codec_name", serde_json::json!("h264")),
            ("width", serde_json::json!(0)),
            ("height", serde_json::json!(9000)),
        ] {
            mutated["streams"][0][field] = value;
            assert!(
                super::parse_exact_ffv1_decoded_frame_count(&serde_json::to_vec(&mutated).unwrap())
                    .is_err(),
                "bad field {field} passed"
            );
            mutated = serde_json::json!({
                "streams": [{"codec_type":"video","codec_name":"ffv1",
                             "width":160,"height":90,"nb_read_frames":"50"}]
            });
        }
        let duplicate = mutated["streams"][0].clone();
        mutated["streams"].as_array_mut().unwrap().push(duplicate);
        assert!(
            super::parse_exact_ffv1_decoded_frame_count(&serde_json::to_vec(&mutated).unwrap())
                .is_err()
        );
        assert!(super::parse_exact_ffv1_decoded_frame_count(&vec![b'x'; 4097]).is_err());
        assert!(super::parse_exact_ffv1_decoded_frame_count(b"{}").is_err());
    }

    #[test]
    fn h264_profile_uses_bounded_fast_high_quality_encoding() {
        let args = h264_encoding_args();
        let args = args.iter().map(|v| v.to_string_lossy()).collect::<Vec<_>>();
        assert_eq!(args, ["pix_fmt=yuv420p", "crf=18", "preset=veryfast"]);
    }

    #[test]
    fn h264_uses_synchronous_mlt_but_other_profiles_keep_certified_async_mode() {
        for id in ["h264-1080p", "h264-720p"] {
            let args = render_processing_args(&RenderProfile::get(id).unwrap());
            let args = args.iter().map(|v| v.to_string_lossy()).collect::<Vec<_>>();
            assert_eq!(args, ["real_time=0", "threads=2"], "profile {id}");
        }
        for id in ["lossless", "lossless-video-only", "audio-wav"] {
            let args = render_processing_args(&RenderProfile::get(id).unwrap());
            let args = args.iter().map(|v| v.to_string_lossy()).collect::<Vec<_>>();
            assert_eq!(args, ["real_time=-1", "threads=2"], "profile {id}");
        }
    }

    #[test]
    fn owner_policy_allows_only_explicit_trust_cases() {
        assert!(trusted_tool_owner(
            Path::new("/workspace/tool"),
            1000,
            1000,
            Some(65534)
        ));
        assert!(trusted_tool_owner(
            Path::new("/workspace/tool"),
            0,
            1000,
            Some(65534)
        ));
        assert!(trusted_tool_owner(
            Path::new("/usr/bin/melt"),
            65534,
            1000,
            Some(65534)
        ));
        assert!(!trusted_tool_owner(
            Path::new("/workspace/tool"),
            65534,
            1000,
            Some(65534)
        ));
        assert!(!trusted_tool_owner(
            Path::new("/opt/tool"),
            65534,
            1000,
            Some(65534)
        ));
        assert!(!trusted_tool_owner(
            Path::new("/usr/bin/melt"),
            4242,
            1000,
            Some(65534)
        ));
        assert!(!trusted_tool_owner(
            Path::new("/usr/bin/melt"),
            65534,
            1000,
            None
        ));
    }
}

#[cfg(all(test, feature = "test-tools"))]
pub(crate) mod native_closure_test_tools {
    use super::*;

    pub(crate) const DELAY_FIXTURE: &[u8] = b"semwright-probe-delay-fixture/1\n";

    pub(crate) fn runtime() -> (Runtime, PrivateDir) {
        // These are the existing test-tools binaries, built before the library tests.
        // Copies retain the production single-link and owner-executable requirements.
        let executable = std::env::current_exe().unwrap();
        let directory = executable.parent().unwrap().parent().unwrap();
        let tools = PrivateDir::new(Path::new("/tmp")).unwrap();
        let pinned = |name: &str| {
            let source = std::fs::canonicalize(directory.join(name)).unwrap();
            let path = tools.path().join(name);
            std::fs::copy(source, &path).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o500)).unwrap();
            let (sha256, _) = reader_hash(File::open(&path).unwrap(), 64 * 1024 * 1024).unwrap();
            Tool { path, sha256 }
        };
        let melt = pinned("fake-melt");
        let ffprobe = pinned("fake-ffprobe");
        let runtime = Runtime::from_host_tools(melt.clone(), ffprobe, melt).unwrap();
        (runtime, tools)
    }
}

#[cfg(test)]
mod native_closure_tests {
    use super::*;
    use std::io::Cursor;

    fn frames() -> Vec<crate::color::DecodedPng> {
        vec![crate::color::DecodedPng {
            file: "decoded-000000.png".into(),
            bytes: 100,
            sha256: "b".repeat(64),
            rgba_sha256: "a".repeat(64),
            alpha_min: 255,
            alpha_max: 255,
        }]
    }

    struct AtEof<R, F> {
        reader: R,
        effect: Option<F>,
    }
    impl<R: Read, F: FnOnce()> Read for AtEof<R, F> {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let size = self.reader.read(buffer)?;
            if size == 0
                && let Some(effect) = self.effect.take()
            {
                effect();
            }
            Ok(size)
        }
    }

    #[test]
    fn final_readback_preserves_unchanged_artifact_roster() {
        let data = b"observed artifact";
        let frames = frames();
        let roster = close_frame_readback(
            Cursor::new(data),
            &crate::hash::sha256(data),
            data.len() as u64,
            &frames,
            Instant::now() + Duration::from_secs(2),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(roster, crate::color::rgba_roster_digest(&frames).unwrap());
    }

    #[test]
    fn final_readback_preserves_changed_artifact_rejection() {
        let data = b"observed artifact";
        let error = close_frame_readback(
            Cursor::new(b"replaced artifact"),
            &crate::hash::sha256(data),
            data.len() as u64,
            &frames(),
            Instant::now() + Duration::from_secs(2),
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert_eq!(error.code, "StaleReference");
    }

    #[test]
    fn final_readback_rejects_cancellation_arriving_at_hash_eof() {
        let data = b"observed artifact";
        let cancel = AtomicBool::new(false);
        let reader = AtEof {
            reader: Cursor::new(data),
            effect: Some(|| cancel.store(true, Ordering::Release)),
        };
        let result = close_frame_readback(
            reader,
            &crate::hash::sha256(data),
            data.len() as u64,
            &frames(),
            Instant::now() + Duration::from_secs(2),
            &cancel,
        );
        assert!(cancel.load(Ordering::Acquire));
        assert_eq!(result.unwrap_err().code, "Cancelled");
    }

    #[test]
    fn final_readback_rejects_deadline_expiring_at_hash_eof() {
        let data = b"observed artifact";
        let deadline = Instant::now() + Duration::from_millis(50);
        let reader = AtEof {
            reader: Cursor::new(data),
            effect: Some(|| {
                thread::sleep(deadline.saturating_duration_since(Instant::now()));
                assert!(Instant::now() >= deadline);
            }),
        };
        let result = close_frame_readback(
            reader,
            &crate::hash::sha256(data),
            data.len() as u64,
            &frames(),
            deadline,
            &AtomicBool::new(false),
        );
        assert_eq!(result.unwrap_err().code, "Timeout");
    }

    #[cfg(feature = "test-tools")]
    #[test]
    fn probe_fixture_completion_is_observable_with_its_own_budget() {
        let (runtime, _tools) = native_closure_test_tools::runtime();
        let work = tempfile::tempdir().unwrap();
        let input = work.path().join("partial.mkv");
        std::fs::write(&input, native_closure_test_tools::DELAY_FIXTURE).unwrap();
        let media = runtime
            .probe(
                work.path(),
                work.path(),
                "partial.mkv",
                &AtomicBool::new(false),
            )
            .unwrap();
        assert!(media.video && media.audio);
        assert!(input.with_extension("probe-finished").is_file());
    }

    #[cfg(feature = "test-tools")]
    #[test]
    fn output_probe_cannot_complete_past_the_shared_deadline() {
        let (runtime, _tools) = native_closure_test_tools::runtime();
        let work = tempfile::tempdir().unwrap();
        let input = work.path().join("partial.mkv");
        std::fs::write(&input, native_closure_test_tools::DELAY_FIXTURE).unwrap();
        let expected = Profile {
            width: 160,
            height: 90,
            fps: crate::time::FrameRate::new(25, 1).unwrap(),
            progressive: true,
            sample_aspect: (1, 1),
            display_aspect: (16, 9),
            colorspace: 709,
            audio_channels: 2,
        };
        let result = runtime.validate_output(
            work.path(),
            &RenderProfile::get("lossless").unwrap(),
            &expected,
            50,
            &AtomicBool::new(false),
            Instant::now() + Duration::from_millis(500),
        );
        assert_eq!(
            result.err().expect("shared deadline must expire").code,
            "Timeout"
        );
        // The fixture completes after 2 s. Observe its effect rather than impose
        // a fragile wall-clock threshold on a loaded CI runner.
        assert!(
            !input.with_extension("probe-finished").exists(),
            "the media probe completed after the shared deadline",
        );
    }
}

#[cfg(test)]
mod ffv1_original_custody_tests {
    use super::*;
    use crate::fs::Root;

    fn fixture() -> (PrivateDir, Root) {
        let directory = PrivateDir::new(Path::new("/tmp")).unwrap();
        let root = Root::open(directory.path(), true, true).unwrap();
        (directory, root)
    }
    fn spec(directory: &Path) -> ProcessSpec {
        ProcessSpec {
            executable: "/never-executed/ffmpeg".into(),
            args: vec!["-version".into()],
            cwd: directory.into(),
            timeout: Duration::from_secs(1),
            cpu_seconds: 1,
            address_space_bytes: 134_217_728,
            environment: BTreeMap::new(),
        }
    }
    fn result(code: i32) -> ProcessResult {
        ProcessResult {
            exit_code: Some(code),
            term_signal: None,
            stdout: vec![b'o', 0xff, 0],
            stderr: vec![b'e', 0, 0xfe],
            cancelled: false,
            timed_out: false,
            output_exceeded: false,
        }
    }
    fn document(root: &Root, name: &str) -> serde_json::Value {
        serde_json::from_slice(&root.read(name, FFV1_CUSTODY_METADATA).unwrap()).unwrap()
    }
    #[test]
    fn ffv1_binary_originals_precede_native_nonzero_classification() {
        let (directory, root) = fixture();
        let pin = "a".repeat(64);
        let custody = Ffv1ProcessCustody::new(&root, "first.mkv", &pin).unwrap();
        let original = checked_ffv1_original(
            Some(&custody),
            "zscale-help",
            &spec(directory.path()),
            Ok(result(7)),
            RETAIN_LOG,
            TOTAL_LOG,
        );
        let error = custody.complete(original).unwrap_err();
        assert_eq!(error.code, "BackendFailed");
        assert!(error.message.contains("Some(7)"));
        assert_eq!(
            root.read("first.mkv.ffv1-zscale-help.stdout.original.bin", 16384)
                .unwrap(),
            vec![b'o', 0xff, 0]
        );
        assert_eq!(
            root.read("first.mkv.ffv1-zscale-help.stderr.original.bin", 16384)
                .unwrap(),
            vec![b'e', 0, 0xfe]
        );
        let saved = document(&root, "first.mkv.ffv1-zscale-help.terminal.original.json");
        assert_eq!(saved["terminal"]["exit_code"], 7);
        assert_eq!(saved["stream_eof_attested"], false);
        assert_eq!(saved["source_capture_retained_limit"], 16384);
        assert_eq!(saved["source_capture_total_limit"], 262144);
    }
    #[test]
    fn ffv1_custody_conflict_does_not_replace_original_error_or_original_file() {
        let (directory, root) = fixture();
        let pin = "b".repeat(64);
        let custody = Ffv1ProcessCustody::new(&root, "first.mkv", &pin).unwrap();
        root.write_new(
            "output",
            "first.mkv.ffv1-zscale-help.stdout.original.bin",
            b"existing",
        )
        .unwrap();
        let error = custody
            .complete(checked_ffv1_original(
                Some(&custody),
                "zscale-help",
                &spec(directory.path()),
                Ok(result(9)),
                RETAIN_LOG,
                TOTAL_LOG,
            ))
            .unwrap_err();
        assert_eq!(error.code, "BackendFailed");
        assert!(error.message.contains("Some(9)"));
        assert_eq!(
            root.read("first.mkv.ffv1-zscale-help.stdout.original.bin", 16384)
                .unwrap(),
            b"existing"
        );
        assert_eq!(
            document(&root, "first.mkv.ffv1-zscale-help.terminal.original.json")["terminal"]["stdout"]
                ["retained"],
            false
        );
    }
    #[test]
    fn ffv1_missing_process_result_never_fabricates_exit_or_empty_stream() {
        let (directory, root) = fixture();
        let pin = "c".repeat(64);
        let custody = Ffv1ProcessCustody::new(&root, "first.mkv", &pin).unwrap();
        let mut original = Error::new("BackendFailed", "original-spawn-or-wait-error");
        original.outcome_known = false;
        let error = custody
            .complete(checked_ffv1_original(
                Some(&custody),
                "zscale-help",
                &spec(directory.path()),
                Err(original),
                RETAIN_LOG,
                TOTAL_LOG,
            ))
            .unwrap_err();
        assert_eq!(error.message, "original-spawn-or-wait-error");
        assert!(!error.outcome_known);
        let saved = document(&root, "first.mkv.ffv1-zscale-help.terminal.original.json");
        assert_eq!(saved["terminal"]["process_result_received"], false);
        assert!(saved["terminal"]["exit_code"].is_null());
        assert!(saved["terminal"]["stdout"].is_null());
        assert!(
            !root
                .exists("first.mkv.ffv1-zscale-help.stdout.original.bin")
                .unwrap()
        );
    }
    #[test]
    fn ffv1_cancel_timeout_and_overflow_keep_their_original_cause() {
        for (field, want) in [(0, "Cancelled"), (1, "Timeout"), (2, "ResourceExhausted")] {
            let (directory, root) = fixture();
            let pin = "d".repeat(64);
            let custody = Ffv1ProcessCustody::new(&root, "first.mkv", &pin).unwrap();
            let mut observed = result(0);
            match field {
                0 => observed.cancelled = true,
                1 => observed.timed_out = true,
                _ => observed.output_exceeded = true,
            }
            let error = custody
                .complete(checked_ffv1_original(
                    Some(&custody),
                    "zscale-help",
                    &spec(directory.path()),
                    Ok(observed),
                    RETAIN_LOG,
                    TOTAL_LOG,
                ))
                .unwrap_err();
            assert_eq!(error.code, want);
        }
    }
    #[test]
    fn ffv1_probe_bytes_are_retained_before_a_later_parser_error() {
        let (directory, root) = fixture();
        let pin = "e".repeat(64);
        let custody = Ffv1ProcessCustody::new(&root, "first.mkv", &pin).unwrap();
        let mut observed = result(0);
        observed.stdout = b"not-json\0\xff".to_vec();
        let returned = checked_ffv1_original(
            Some(&custody),
            "zscale-help",
            &spec(directory.path()),
            Ok(observed),
            RETAIN_LOG,
            TOTAL_LOG,
        )
        .unwrap();
        let parsed: Result<serde_json::Value> = serde_json::from_slice(&returned.stdout)
            .map_err(|_| Error::invalid("original-parser-refusal"));
        assert_eq!(
            custody.complete(parsed).unwrap_err().message,
            "original-parser-refusal"
        );
        assert_eq!(
            root.read("first.mkv.ffv1-zscale-help.stdout.original.bin", 16384)
                .unwrap(),
            b"not-json\0\xff"
        );
    }
    #[test]
    fn ffv1_six_closed_stages_preserve_exact_returned_buffers_without_execution() {
        let (directory, root) = fixture();
        let pin = "f".repeat(64);
        let custody = Ffv1ProcessCustody::new(&root, "first.mkv", &pin).unwrap();
        for stage in FFV1_CUSTODY_STAGES {
            let output = checked_ffv1_original(
                Some(&custody),
                stage,
                &spec(directory.path()),
                Ok(result(0)),
                RETAIN_LOG,
                TOTAL_LOG,
            )
            .unwrap();
            assert_eq!(output.stdout, vec![b'o', 0xff, 0]);
        }
        assert_eq!(custody.complete(Ok(37)).unwrap(), 37);
        let saved = document(&root, "first.mkv.ffv1-process-custody.original.json");
        assert_eq!(
            saved["observed_stages"],
            serde_json::json!(FFV1_CUSTODY_STAGES)
        );
        assert_eq!(saved["physical_acceptance"], false);
    }
    #[test]
    fn ffv1_complete_probe_retains_every_accepted_original_byte() {
        let (directory, root) = fixture();
        let pin = "a".repeat(64);
        let custody = Ffv1ProcessCustody::new(&root, "first.mkv", &pin).unwrap();
        for stage in ["zscale-help", "encode", "media-probe"] {
            checked_ffv1_original(
                Some(&custody),
                stage,
                &spec(directory.path()),
                Ok(result(0)),
                RETAIN_LOG,
                TOTAL_LOG,
            )
            .unwrap();
        }
        let bytes = vec![0xff; FFV1_CUSTODY_STREAM_PREFIX + 17];
        let mut observed = result(0);
        observed.stdout = bytes.clone();
        checked_ffv1_original(
            Some(&custody),
            "complete-probe",
            &spec(directory.path()),
            Ok(observed),
            FFV1_CUSTODY_PROBE_STREAM,
            FFV1_CUSTODY_PROBE_STREAM,
        )
        .unwrap();
        assert_eq!(
            root.read(
                "first.mkv.ffv1-complete-probe.stdout.original.bin",
                FFV1_CUSTODY_PROBE_STREAM
            )
            .unwrap(),
            bytes
        );
        let saved = document(
            &root,
            "first.mkv.ffv1-complete-probe.terminal.original.json",
        );
        assert_eq!(saved["terminal"]["stdout"]["bytes"], 16401);
        assert_eq!(
            saved["terminal"]["stdout"]["diagnostic_prefix_truncated"],
            false
        );
        assert_eq!(
            saved["terminal"]["stdout"]["upstream_stream_eof_attested"],
            false
        );
        assert_eq!(saved["source_capture_retained_limit"], 8_388_608);
        assert_eq!(saved["source_capture_total_limit"], 8_388_608);
    }

    #[test]
    fn ffv1_supervisor_error_keeps_observed_partial_originals_and_primary_error() {
        let (directory, root) = fixture();
        let pin = "a".repeat(64);
        let custody = Ffv1ProcessCustody::new(&root, "first.mkv", &pin).unwrap();
        let observation = ProcessObservation {
            spawn_attempted: true,
            pid: Some(123),
            stdout: Some(CapturedPipe {
                retained: vec![0, 255, 7],
                observed_bytes: 3,
                stopped: true,
                ..CapturedPipe::default()
            }),
            stderr: Some(CapturedPipe {
                retained: vec![254, 0],
                observed_bytes: 2,
                eof: true,
                ..CapturedPipe::default()
            }),
            ..ProcessObservation::default()
        };
        let mut error = Error::new("BackendFailed", "original-supervisor-error");
        error.outcome_known = false;
        let original: Result<ProcessResult> = Err(error);
        custody.record_observed(
            "zscale-help",
            &spec(directory.path()),
            &original,
            RETAIN_LOG,
            TOTAL_LOG,
            Some(&observation),
            Some(crate::jobs::MAX_ARTIFACT_BYTES),
        );
        let error = custody.complete(original).unwrap_err();
        assert_eq!(error.message, "original-supervisor-error");
        assert!(!error.outcome_known);
        assert_eq!(
            root.read("first.mkv.ffv1-zscale-help.stdout.original.bin", RETAIN_LOG)
                .unwrap(),
            vec![0, 255, 7]
        );
        assert_eq!(
            root.read("first.mkv.ffv1-zscale-help.stderr.original.bin", RETAIN_LOG)
                .unwrap(),
            vec![254, 0]
        );
        let saved = document(&root, "first.mkv.ffv1-zscale-help.terminal.original.json");
        assert_eq!(
            saved["same_spawn_observation"]["stdout"]["drain_stopped"],
            true
        );
        assert_eq!(saved["same_spawn_observation"]["capture_complete"], false);
        assert!(saved["terminal"]["exit_code"].is_null());
    }

    #[test]
    fn ffv1_existing_sidecar_refuses_preflight_without_overwrite() {
        let (_directory, root) = fixture();
        root.write_new(
            "output",
            "first.mkv.ffv1-host.stdin.original.bin",
            b"old-unknown",
        )
        .unwrap();
        assert!(preflight_ffv1_custody(&root, "first.mkv", true).is_err());
        assert_eq!(
            root.read("first.mkv.ffv1-host.stdin.original.bin", 64)
                .unwrap(),
            b"old-unknown"
        );
        assert!(ffv1_custody_paths("../outside.mkv", true).is_err());
        assert_eq!(ffv1_custody_paths("first.mkv", true).unwrap().len(), 27);
    }
    #[test]
    fn ffv1_empty_original_uses_existing_atomic_root_api() {
        let (_directory, root) = fixture();
        let file = retain_ffv1_original(&root, "empty.original.bin", b"", 0).unwrap();
        assert_eq!(file.bytes, 0);
        assert_eq!(file.sha256, crate::hash::sha256(b""));
        assert_eq!(root.read("empty.original.bin", 0).unwrap(), b"");
    }
    #[test]
    fn ffv1_success_does_not_hide_incomplete_custody() {
        let (directory, root) = fixture();
        let pin = "a".repeat(64);
        let custody = Ffv1ProcessCustody::new(&root, "first.mkv", &pin).unwrap();
        root.write_new(
            "output",
            "first.mkv.ffv1-zscale-help.stdout.original.bin",
            b"retained-before",
        )
        .unwrap();
        for stage in FFV1_CUSTODY_STAGES {
            checked_ffv1_original(
                Some(&custody),
                stage,
                &spec(directory.path()),
                Ok(result(0)),
                RETAIN_LOG,
                TOTAL_LOG,
            )
            .unwrap();
        }
        let error = custody.complete(Ok(())).unwrap_err();
        assert!(!error.outcome_known);
        assert_eq!(
            root.read("first.mkv.ffv1-zscale-help.stdout.original.bin", 64)
                .unwrap(),
            b"retained-before"
        );
    }

    #[test]
    fn ffv1_observed_reader_panics_keep_the_legacy_primary_error_order() {
        let mut observation = ProcessObservation {
            stdout: Some(CapturedPipe {
                panicked: true,
                ..CapturedPipe::default()
            }),
            stderr: Some(CapturedPipe {
                panicked: true,
                ..CapturedPipe::default()
            }),
            ..ProcessObservation::default()
        };
        let error = checked_ffv1_process_observation(Ok(result(7)), &observation).unwrap_err();
        assert_eq!(error.code, "Internal");
        assert_eq!(error.message, "stdout reader failed");
        observation.stdout.as_mut().unwrap().panicked = false;
        let error = checked_ffv1_process_observation(Ok(result(7)), &observation).unwrap_err();
        assert_eq!(error.message, "stderr reader failed");
        let error = checked_ffv1_process_observation(
            Err(Error::new("BackendFailed", "original-wait-error")),
            &observation,
        )
        .unwrap_err();
        assert_eq!(error.message, "original-wait-error");
        observation.stderr.as_mut().unwrap().panicked = false;
        observation.stdout.as_mut().unwrap().observed_bytes = 100;
        // Intentional legacy prefix capture is not reclassified as a panic.
        assert!(checked_ffv1_process_observation(Ok(result(0)), &observation).is_ok());
    }

    #[test]
    fn decode_file_size_limit_is_enforced_before_spawn() {
        const MARKER: &str = "SEMWRIGHT_TEST_FSIZE_ORIGINAL_CHILD";
        if std::env::var(MARKER).ok().as_deref() == Some("1") {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open("bounded.original.bin")
                .unwrap();
            let result = file.write_all(&[0x61; 8192]);
            // SIGXFSZ may terminate first. If the inherited disposition ignores
            // it, Linux still returns EFBIG and the file cannot exceed the cap.
            assert!(result.is_err());
            assert!(file.metadata().unwrap().len() <= 4096);
            return;
        }
        let (directory, _root) = fixture();
        let spec = ProcessSpec {
            executable: std::env::current_exe().unwrap(),
            args: vec!["--exact".into(),
                "runtime::ffv1_original_custody_tests::decode_file_size_limit_is_enforced_before_spawn".into(),
                "--nocapture".into(), "--test-threads=1".into()],
            cwd: directory.path().into(), timeout: Duration::from_secs(10),
            cpu_seconds: 5, address_space_bytes: 536_870_912,
            environment: BTreeMap::from([(MARKER.into(), "1".into())]),
        };
        let mut observation = ProcessObservation::default();
        let result = run_with_file_limit_observed(
            &spec,
            &AtomicBool::new(false),
            RETAIN_LOG,
            TOTAL_LOG,
            4096,
            Some(&mut observation),
        )
        .unwrap();
        let actual = std::fs::metadata(directory.path().join("bounded.original.bin")).unwrap();
        assert_eq!(actual.len(), 4096);
        assert!(result.exit_code == Some(0) || result.term_signal == Some(libc::SIGXFSZ));
        assert!(observation.terminal_observed);
        assert!(!result.timed_out && !result.cancelled && !result.output_exceeded);
    }

    #[test]
    fn incomplete_observed_drain_does_not_claim_complete_original_custody() {
        let (directory, root) = fixture();
        let pin = "a".repeat(64);
        let custody = Ffv1ProcessCustody::new(&root, "first.mkv", &pin).unwrap();
        let observation = ProcessObservation {
            spawn_attempted: true,
            terminal_observed: true,
            exit_code: Some(0),
            drain_stop_reason: Some("deadline"),
            stdout: Some(CapturedPipe {
                stopped: true,
                ..CapturedPipe::default()
            }),
            stderr: Some(CapturedPipe {
                eof: true,
                ..CapturedPipe::default()
            }),
            ..ProcessObservation::default()
        };
        custody.record_observed(
            "zscale-help",
            &spec(directory.path()),
            &Ok(result(0)),
            RETAIN_LOG,
            TOTAL_LOG,
            Some(&observation),
            Some(crate::jobs::MAX_ARTIFACT_BYTES),
        );
        for stage in [
            "encode",
            "media-probe",
            "complete-probe",
            "decoder-version",
            "decode",
        ] {
            checked_ffv1_original(
                Some(&custody),
                stage,
                &spec(directory.path()),
                Ok(result(0)),
                RETAIN_LOG,
                TOTAL_LOG,
            )
            .unwrap();
        }
        let error = custody.complete(Ok(())).unwrap_err();
        assert!(!error.outcome_known);
        assert!(!custody.original_capture_complete());
        let report = document(&root, "first.mkv.ffv1-process-custody.original.json");
        assert!(
            report["secondary_failures"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item == "observed-stream-capture-incomplete")
        );
    }

    #[test]
    fn input_probe_custody_keeps_the_real_slot_source_and_error() {
        let (directory, root) = fixture();
        let pin = "b".repeat(64);
        let custody =
            Ffv1ProcessCustody::for_input_probe(&root, "first.mkv", &pin, 4, &root).unwrap();
        let original = checked_ffv1_original(
            Some(&custody),
            "media-probe",
            &spec(directory.path()),
            Ok(result(7)),
            RETAIN_LOG,
            TOTAL_LOG,
        );
        let error = custody.complete(original).unwrap_err();
        assert!(error.message.contains("Some(7)"));
        let stem = "first.mkv.native-input-probe-000004-media-probe";
        assert_eq!(
            root.read(&format!("{stem}.stdout.original.bin"), RETAIN_LOG)
                .unwrap(),
            vec![b'o', 255, 0]
        );
        assert_eq!(
            root.read(
                "native-input-probe-000004-media-probe.stdout.original.bin",
                RETAIN_LOG
            )
            .unwrap(),
            vec![b'o', 255, 0]
        );
        let report = document(&root, &format!("{stem}.terminal.original.json"));
        assert_eq!(report["source_sha256"], pin);
        assert_eq!(report["source_kind"], "staged-input-bytes");
        assert!(report["source_manifest_sha256"].is_null());
        assert_eq!(report["member_prefix"], "native-input-probe-000004");
        assert_eq!(report["role"], "native-input-probe");
    }

    #[test]
    fn summary_retention_error_remains_available_after_the_primary_error() {
        let (directory, root) = fixture();
        let (_work_directory, work_root) = fixture();
        let pin = "b".repeat(64);
        let custody =
            Ffv1ProcessCustody::for_input_probe(&root, "first.mkv", &pin, 0, &work_root).unwrap();
        let result = checked_ffv1_original(
            Some(&custody),
            "media-probe",
            &spec(directory.path()),
            Ok(result(7)),
            RETAIN_LOG,
            TOTAL_LOG,
        );
        let primary_message = result.as_ref().unwrap_err().message.clone();
        let summary = "first.mkv.native-input-probe-000000-process-custody.original.json";
        root.write_new("output", summary, b"prior-unknown-summary")
            .unwrap();
        let error = custody.complete(result).unwrap_err();
        assert_eq!(error.message, primary_message);
        let report = custody.secondary_report().unwrap();
        assert_eq!(report["observed_after_summary_retention"], true);
        let errors = report["retention_errors"].as_array().unwrap();
        let secondary = errors
            .iter()
            .find(|error| error["phase"] == "summary-retention")
            .unwrap();
        assert_eq!(secondary["code"], "Conflict");
        assert!(!secondary["message"].as_str().unwrap().is_empty());
        assert_eq!(secondary["outcome_known"], true);
        assert!(!custody.original_capture_complete());
        assert_eq!(root.read(summary, 64).unwrap(), b"prior-unknown-summary");
        let before = document(
            &work_root,
            "native-input-probe-000000-process-custody.original.json",
        );
        assert_eq!(
            before["observation_boundary"],
            "before-this-summary-retention"
        );
        assert_eq!(before["original_error"]["message"], primary_message);
    }
}

#[cfg(test)]
mod audio_derivative_tests {
    use super::*;
    use std::io::Cursor;

    fn wav_data(
        rate: u32,
        channels: u16,
        bits: u16,
        floating: bool,
        mask: Option<u32>,
        data: &[u8],
    ) -> Vec<u8> {
        let mut fmt = Vec::new();
        let tag: u16 = if mask.is_some() {
            0xfffe
        } else if floating {
            3
        } else {
            1
        };
        fmt.extend_from_slice(&tag.to_le_bytes());
        fmt.extend_from_slice(&channels.to_le_bytes());
        fmt.extend_from_slice(&rate.to_le_bytes());
        let block = channels * (bits / 8);
        fmt.extend_from_slice(&(rate * u32::from(block)).to_le_bytes());
        fmt.extend_from_slice(&block.to_le_bytes());
        fmt.extend_from_slice(&bits.to_le_bytes());
        if let Some(mask) = mask {
            fmt.extend_from_slice(&22u16.to_le_bytes());
            fmt.extend_from_slice(&bits.to_le_bytes());
            fmt.extend_from_slice(&mask.to_le_bytes());
            fmt.extend_from_slice(&[
                1, 0, 0, 0, 0, 0, 0x10, 0, 0x80, 0, 0, 0xaa, 0, 0x38, 0x9b, 0x71,
            ]);
        }
        let mut bytes = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        bytes.extend_from_slice(&(fmt.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&fmt);
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(data);
        if !data.len().is_multiple_of(2) {
            bytes.push(0);
        }
        let riff = (bytes.len() - 8) as u32;
        bytes[4..8].copy_from_slice(&riff.to_le_bytes());
        bytes
    }

    fn wav(rate: u32, channels: u16, frames: u32, mask: Option<u32>, unequal: bool) -> Vec<u8> {
        let mut data = Vec::new();
        for frame in 0..frames {
            for channel in 0..channels {
                let sample =
                    ((frame % 101) as i16 - 50) * 127 + if unequal && channel == 1 { 1 } else { 0 };
                data.extend_from_slice(&sample.to_le_bytes());
            }
        }
        wav_data(rate, channels, 16, false, mask, &data)
    }

    #[test]
    fn source_clock_boundaries_are_integral_and_profiles_are_closed() {
        let down = AudioDerivativeProfile::Stereo48ToMono16V1;
        assert_eq!(down.output_frames(1_200).unwrap(), 400);
        assert_eq!(down.output_frames(384_000).unwrap(), 128_000);
        for invalid in [0, 1_199, 1_201, 384_001, 384_003, u64::MAX] {
            assert!(down.output_frames(invalid).is_err(), "{invalid}");
        }
        let up = AudioDerivativeProfile::Mono16ToDualMono48V1;
        assert_eq!(up.output_frames(16_000).unwrap(), 48_000);
        assert_eq!(up.output_frames(2_880_000).unwrap(), 8_640_000);
        for invalid in [0, 15_999, 2_880_001, u64::MAX] {
            assert!(up.output_frames(invalid).is_err());
        }
        assert!(AudioDerivativeProfile::parse("pcm16-mono16-to-stereo48-v1").is_err());
        assert!(AudioDerivativeProfile::parse("caller-filter").is_err());
    }

    #[test]
    fn wave_reader_accepts_original_pcm_chunks_and_observes_every_frame() {
        let profile = AudioDerivativeProfile::Stereo48ToMono16V1;
        let mut bytes = wav(48_000, 2, 1_200, None, true);
        bytes.splice(36..36, b"JUNK\x04\0\0\0keep".iter().copied());
        let size = (bytes.len() - 8) as u32;
        bytes[4..8].copy_from_slice(&size.to_le_bytes());
        let before = bytes.clone();
        let observation = profile.inspect_source(&bytes, 1_200).unwrap();
        assert_eq!(observation.sample_frames, 1_200);
        assert_eq!(observation.channels, 2);
        assert_eq!(bytes, before);
        assert!(profile.inspect_source(&bytes, 1_203).is_err());
        let last = bytes.len() - 1;
        assert!(profile.inspect_source(&bytes[..last], 1_200).is_err());
    }

    #[test]
    fn channel_masks_are_observed_instead_of_inferred_from_channel_count() {
        let down = AudioDerivativeProfile::Stereo48ToMono16V1;
        assert!(
            down.inspect_source(&wav(48_000, 2, 1_200, Some(3), true), 1_200)
                .is_ok()
        );
        for mask in [0, 4, 0x30] {
            assert!(
                down.inspect_source(&wav(48_000, 2, 1_200, Some(mask), true), 1_200)
                    .is_err()
            );
        }
        let up = AudioDerivativeProfile::Mono16ToDualMono48V1;
        assert!(
            up.inspect_source(&wav(16_000, 1, 16_000, Some(4), false), 16_000)
                .is_ok()
        );
        for mask in [0, 1, 3] {
            assert!(
                up.inspect_source(&wav(16_000, 1, 16_000, Some(mask), false), 16_000)
                    .is_err()
            );
        }
    }

    #[test]
    fn well_formed_float_and_pcm24_sources_do_not_enter_the_pcm16_profile() {
        let floats = (0..16_000)
            .flat_map(|_| 0.25f32.to_le_bytes())
            .collect::<Vec<_>>();
        let wave = wav_data(16_000, 1, 32, true, None, &floats);
        assert!(semwright_audio_domain::wav::WaveReader::open(Cursor::new(&wave), 1 << 20).is_ok());
        assert!(
            AudioDerivativeProfile::Mono16ToDualMono48V1
                .inspect_source(&wave, 16_000)
                .is_err()
        );
        let pcm24 = vec![0u8; 1_200 * 2 * 3];
        let wave = wav_data(48_000, 2, 24, false, None, &pcm24);
        assert!(semwright_audio_domain::wav::WaveReader::open(Cursor::new(&wave), 1 << 20).is_ok());
        assert!(
            AudioDerivativeProfile::Stereo48ToMono16V1
                .inspect_source(&wave, 1_200)
                .is_err()
        );
    }

    #[test]
    fn derived_dual_mono_checks_both_samples_and_exact_target_count() {
        let profile = AudioDerivativeProfile::Mono16ToDualMono48V1;
        let source_frames = 16_000;
        let valid = wav(48_000, 2, 48_000, Some(3), false);
        assert!(profile.inspect_target(&valid, source_frames).is_ok());
        assert!(
            profile
                .inspect_target(&wav(48_000, 2, 48_000, None, true), source_frames)
                .is_err()
        );
        assert!(
            profile
                .inspect_target(&wav(48_000, 2, 48_001, None, false), source_frames)
                .is_err()
        );
        assert!(
            profile
                .inspect_target(&wav(48_000, 1, 48_000, None, false), source_frames)
                .is_err()
        );
        assert!(
            profile
                .inspect_target(&wav(16_000, 2, 48_000, None, false), source_frames)
                .is_err()
        );
    }

    #[test]
    fn recipes_are_fixed_and_do_not_trim_pad_or_normalize_original_samples() {
        for profile in [
            AudioDerivativeProfile::Stereo48ToMono16V1,
            AudioDerivativeProfile::Mono16ToDualMono48V1,
        ] {
            let args = audio_derivative_args(
                profile,
                Path::new("/input/source.wav"),
                Path::new("/work/derived.wav"),
            );
            let strings = args
                .iter()
                .map(|arg| arg.to_str().unwrap())
                .collect::<Vec<_>>();
            assert_eq!(strings.iter().filter(|arg| **arg == "-i").count(), 1);
            assert!(strings.windows(2).any(|a| a == ["-c:a", "pcm_s16le"]));
            assert!(strings.windows(2).any(|a| a == ["-af", profile.filter()]));
            assert!(strings.contains(&"-n"));
            for forbidden in ["-ss", "-t", "-to", "-shortest", "-frames:a"] {
                assert!(!strings.contains(&forbidden));
            }
            assert!(!profile.filter().contains("apad"));
            assert!(!profile.filter().contains("atrim"));
            assert!(!profile.filter().contains("loudnorm"));
            assert!(valid_digest(&profile.recipe_sha256().unwrap()));
        }
        assert_ne!(
            AudioDerivativeProfile::Stereo48ToMono16V1
                .recipe_sha256()
                .unwrap(),
            AudioDerivativeProfile::Mono16ToDualMono48V1
                .recipe_sha256()
                .unwrap()
        );
    }

    struct PrefixFailure {
        first: bool,
        panic: bool,
    }
    impl Read for PrefixFailure {
        fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
            if self.first {
                self.first = false;
                bytes[..6].copy_from_slice(b"prefix");
                return Ok(6);
            }
            assert!(!self.panic, "controlled reader panic after original prefix");
            Err(std::io::Error::from_raw_os_error(libc::EIO))
        }
    }

    fn read_fixture(
        reader: impl Read + Send + 'static,
        retained: usize,
        total: usize,
    ) -> CapturedPipe {
        capture(
            reader,
            Arc::new(AtomicBool::new(false)),
            retained,
            total,
            Arc::new(AtomicBool::new(false)),
            false,
        )
        .unwrap()
        .finish()
    }

    #[test]
    fn original_prefix_survives_read_error_without_fabricating_eof() {
        let pipe = read_fixture(
            PrefixFailure {
                first: true,
                panic: false,
            },
            64,
            64,
        );
        assert_eq!(pipe.retained, b"prefix");
        assert_eq!(pipe.observed_bytes, 6);
        assert!(pipe.read_error.is_some());
        assert!(!pipe.eof);
        assert!(!pipe.complete());
    }

    #[test]
    fn original_prefix_survives_reader_panic_without_becoming_an_empty_pipe() {
        let pipe = read_fixture(
            PrefixFailure {
                first: true,
                panic: true,
            },
            64,
            64,
        );
        assert_eq!(pipe.retained, b"prefix");
        assert!(pipe.panicked);
        assert!(!pipe.eof);
        assert!(!pipe.complete());
    }

    #[test]
    fn empty_eof_and_a_truncated_prefix_have_different_observations() {
        let empty = read_fixture(Cursor::new(Vec::<u8>::new()), 4, 8);
        assert!(empty.eof && empty.complete());
        assert!(empty.retained.is_empty());
        let prefix = read_fixture(Cursor::new(b"123456".to_vec()), 4, 8);
        assert_eq!(prefix.retained, b"1234");
        assert_eq!(prefix.observed_bytes, 6);
        assert!(prefix.eof);
        assert!(!prefix.complete());
        assert_eq!(prefix.metadata()["truncated"], true);
    }

    #[test]
    fn an_open_descendant_pipe_can_stop_without_a_blocking_join_or_fake_eof() {
        use std::os::unix::net::UnixStream;
        let (reader, _still_open_writer) = UnixStream::pair().unwrap();
        reader.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let handle = capture(
            reader,
            Arc::new(AtomicBool::new(false)),
            64,
            64,
            stop.clone(),
            true,
        )
        .unwrap();
        stop.store(true, Ordering::Release);
        let pipe = handle.finish();
        assert!(pipe.stopped);
        assert!(!pipe.eof);
        assert!(!pipe.complete());
    }

    #[test]
    fn known_terminal_remains_separate_from_a_later_drain_stop() {
        use std::os::unix::net::UnixStream;
        for reason in ["deadline", "cancelled", "output_exceeded"] {
            let (reader, writer) = UnixStream::pair().unwrap();
            capture_nonblocking(&reader).unwrap();
            let stop = Arc::new(AtomicBool::new(false));
            let overflow = Arc::new(AtomicBool::new(reason == "output_exceeded"));
            let cancel = AtomicBool::new(reason == "cancelled");
            let stdout = capture(reader, overflow.clone(), 64, 64, stop.clone(), true).unwrap();
            let stderr = capture(
                Cursor::new(Vec::<u8>::new()),
                overflow.clone(),
                64,
                64,
                stop.clone(),
                true,
            )
            .unwrap();
            let start = if reason == "deadline" {
                Instant::now().checked_sub(Duration::from_secs(1)).unwrap()
            } else {
                Instant::now()
            };
            let mut observation = ProcessObservation {
                terminal_observed: true,
                exit_code: Some(0),
                cancelled: Some(false),
                timed_out: Some(false),
                output_exceeded: Some(overflow.load(Ordering::Acquire)),
                ..ProcessObservation::default()
            };
            observation.drain_stop_reason = drain_observed_capture(
                [&stdout, &stderr],
                &stop,
                start,
                Duration::from_millis(500),
                &cancel,
                &overflow,
                false,
            );
            observation.stdout = Some(stdout.finish());
            observation.stderr = Some(stderr.finish());
            // The original writer stays open throughout the actual drain and join.
            assert!(observation.stdout.as_ref().unwrap().stopped);
            assert!(!observation.stdout.as_ref().unwrap().eof);
            assert_eq!(observation.drain_stop_reason, Some(reason));
            assert_eq!(observation.exit_code, Some(0));
            assert_eq!(observation.cancelled, Some(false));
            assert_eq!(observation.timed_out, Some(false));
            assert!(!observation.complete());
            let metadata = observation.metadata();
            assert_eq!(metadata["drain_stop_reason"], reason);
            assert_eq!(metadata["exit_code"], 0);
            assert_eq!(metadata["timed_out"], false);
            // Even if EOF races the stop request, the observed drain stop cannot
            // be erased by subsequently complete reader states.
            observation.stdout = Some(CapturedPipe {
                eof: true,
                ..CapturedPipe::default()
            });
            observation.stderr = Some(CapturedPipe {
                eof: true,
                ..CapturedPipe::default()
            });
            observation.output_exceeded = Some(false);
            assert!(!observation.complete());
            drop(writer);
        }
    }

    fn terminal_fixture(
        code: i32,
        stdout: Option<CapturedPipe>,
    ) -> (ProcessResult, ProcessObservation) {
        let stderr = read_fixture(Cursor::new(b"original stderr".to_vec()), 64, 64);
        let result = ProcessResult {
            exit_code: Some(code),
            term_signal: None,
            stdout: stdout
                .as_ref()
                .map_or_else(Vec::new, |pipe| pipe.retained.clone()),
            stderr: stderr.retained.clone(),
            cancelled: false,
            timed_out: false,
            output_exceeded: false,
        };
        let observation = ProcessObservation {
            spawn_attempted: true,
            pid: None,
            terminal_observed: true,
            exit_code: Some(code),
            term_signal: None,
            cancelled: Some(false),
            timed_out: Some(false),
            output_exceeded: Some(false),
            stdout,
            stderr: Some(stderr),
            supervisor_error: None,
            drain_stop_reason: None,
        };
        (result, observation)
    }

    #[test]
    fn nonzero_terminal_stays_primary_for_reader_error_panic_or_missing_pipe() {
        let cases = [
            Some(read_fixture(
                PrefixFailure {
                    first: true,
                    panic: false,
                },
                64,
                64,
            )),
            Some(read_fixture(
                PrefixFailure {
                    first: true,
                    panic: true,
                },
                64,
                64,
            )),
            None,
        ];
        for pipe in cases {
            let (result, observation) = terminal_fixture(7, pipe);
            let error = checked_audio_derivative_process(Ok(result), &observation).unwrap_err();
            assert_eq!(error.code, "BackendFailed");
            assert!(error.message.contains("Some(7)"));
            assert_eq!(observation.exit_code, Some(7));
            assert!(!observation.complete());
        }
    }

    #[test]
    fn successful_exit_with_incomplete_capture_is_unknown_instead_of_a_derivative() {
        let pipe = read_fixture(
            PrefixFailure {
                first: true,
                panic: false,
            },
            64,
            64,
        );
        let (result, observation) = terminal_fixture(0, Some(pipe));
        let error = checked_audio_derivative_process(Ok(result), &observation).unwrap_err();
        assert!(!error.outcome_known);
        assert_eq!(observation.exit_code, Some(0));
    }

    #[test]
    fn custody_failure_keeps_the_original_error_object_and_unknown_flag() {
        let mut original = Error::new("Cancelled", "retained primary cancellation");
        original.outcome_known = false;
        let message = original.message.as_ptr();
        let error = finish_audio_derivative_original::<()>(
            Err(original),
            Err(Error::new("Internal", "secondary write failure")),
        )
        .unwrap_err();
        assert_eq!(error.code, "Cancelled");
        assert_eq!(error.message.as_ptr(), message);
        assert!(!error.outcome_known);
        let success =
            finish_audio_derivative_original(Ok(()), Err(Error::new("Internal", "write failure")))
                .unwrap_err();
        assert!(!success.outcome_known);
    }

    #[test]
    fn empty_original_stream_is_published_and_never_overwritten() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().canonicalize().unwrap();
        let root = crate::fs::Root::open(&path, true, true).unwrap();
        let original = retain_audio_derivative_original(&root, "empty.bin", &[], 8).unwrap();
        assert_eq!(original.bytes, 0);
        assert_eq!(original.sha256, crate::hash::sha256(&[]));
        assert_eq!(root.read("empty.bin", 8).unwrap(), b"");
        assert!(retain_audio_derivative_original(&root, "empty.bin", b"replacement", 16).is_err());
        assert_eq!(root.read("empty.bin", 8).unwrap(), b"");
    }

    #[test]
    fn custody_roster_is_audio_specific_closed_and_disjoint_from_ffv1() {
        let paths = audio_derivative_custody_paths("voice.wav", true).unwrap();
        assert_eq!(paths.len(), 16);
        let unique = paths.iter().collect::<BTreeSet<_>>();
        assert_eq!(unique.len(), paths.len());
        assert!(paths.iter().all(|path| !path.contains("ffv1")));
        assert!(audio_derivative_custody_paths("../voice.wav", true).is_err());
        assert!(audio_derivative_custody_paths("voice.mp4", true).is_err());
    }

    // A controlled test-binary child exercises the SAME supervisor. It is not
    // FFmpeg, a render, or evidence of the runtime audio profiles.
    #[test]
    fn audio_capture_child_fixture() {
        if std::env::var("SEMWRIGHT_AUDIO_CAPTURE_FIXTURE").as_deref() == Ok("one-child") {
            std::io::stdout()
                .write_all(b"audio-derive-one-child\n")
                .unwrap();
            std::io::stdout().flush().unwrap();
            std::io::stderr()
                .write_all(b"audio-derive-original-stderr\n")
                .unwrap();
            std::io::stderr().flush().unwrap();
            std::process::exit(7);
        }
    }

    #[test]
    fn same_supervisor_retains_real_nonzero_exit_and_complete_original_pipes() {
        let work = tempfile::tempdir().unwrap();
        let spec = ProcessSpec {
            executable: std::env::current_exe().unwrap(),
            args: [
                "--exact",
                "runtime::audio_derivative_tests::audio_capture_child_fixture",
                "--nocapture",
            ]
            .into_iter()
            .map(Into::into)
            .collect(),
            cwd: work.path().canonicalize().unwrap(),
            timeout: Duration::from_secs(10),
            cpu_seconds: 10,
            address_space_bytes: 1_073_741_824,
            environment: BTreeMap::from([(
                "SEMWRIGHT_AUDIO_CAPTURE_FIXTURE".into(),
                "one-child".into(),
            )]),
        };
        let mut observation = ProcessObservation::default();
        let result = run_with_output_limits_observed(
            &spec,
            &AtomicBool::new(false),
            65_536,
            65_536,
            Some(&mut observation),
        )
        .unwrap();
        assert_eq!(result.exit_code, Some(7));
        assert!(observation.pid.is_some());
        assert!(observation.complete());
        assert_eq!(observation.exit_code, Some(7));
        assert_eq!(result.stdout, observation.stdout.as_ref().unwrap().retained);
        assert_eq!(result.stderr, observation.stderr.as_ref().unwrap().retained);
        assert_eq!(
            String::from_utf8_lossy(&result.stdout)
                .matches("audio-derive-one-child\n")
                .count(),
            1
        );
        assert!(String::from_utf8_lossy(&result.stderr).contains("audio-derive-original-stderr"));
        assert_eq!(
            checked_audio_derivative_process(Ok(result), &observation)
                .unwrap_err()
                .code,
            "BackendFailed"
        );
    }
}
