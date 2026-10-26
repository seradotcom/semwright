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
    let native =
        6 * (2 * FFV1_CUSTODY_STREAM_PREFIX + FFV1_CUSTODY_METADATA) + FFV1_CUSTODY_METADATA;
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
    if bytes.len() > maximum {
        return Err(Error::limit(
            "FFV1 original exceeds its closed retention budget",
        ));
    }
    let file = output.write_new("output", path, bytes)?;
    if file.bytes != bytes.len() as u64 || file.sha256 != crate::hash::sha256(bytes) {
        return Err(ffv1_custody_uncertain(
            "Original FFV1 publication identity differs",
        ));
    }
    let actual = output
        .read(path, maximum)
        .map_err(|_| ffv1_custody_uncertain("Original FFV1 custody readback is uncertain"))?;
    if actual != bytes {
        return Err(ffv1_custody_uncertain(
            "Original FFV1 custody readback differs",
        ));
    }
    Ok(file)
}

fn ffv1_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn ffv1_spec_original(spec: &ProcessSpec) -> Result<serde_json::Value> {
    use std::os::unix::ffi::OsStrExt;
    if spec.args.len() > 128 {
        return Err(Error::limit("FFV1 original argv count"));
    }
    let mut total = 0usize;
    let mut argv = Vec::with_capacity(spec.args.len());
    for arg in &spec.args {
        let bytes = arg.as_os_str().as_bytes();
        total = total
            .checked_add(bytes.len())
            .ok_or_else(|| Error::limit("FFV1 argv overflow"))?;
        if bytes.len() > 4096 || total > 16_384 {
            return Err(Error::limit("FFV1 original argv byte budget"));
        }
        argv.push(ffv1_hex(bytes));
    }
    let executable = spec.executable.as_os_str().as_bytes();
    let cwd = spec.cwd.as_os_str().as_bytes();
    if executable.len() > 4096 || cwd.len() > 4096 {
        return Err(Error::limit("FFV1 original process path budget"));
    }
    Ok(serde_json::json!({
        "encoding":"unix-os-bytes-hex", "executable":ffv1_hex(executable),
        "argv":argv, "cwd":ffv1_hex(cwd), "timeout_millis":spec.timeout.as_millis(),
        "cpu_seconds":spec.cpu_seconds, "address_space_bytes":spec.address_space_bytes,
        "environment_included":false, "native_execution_retried":false
    }))
}

pub(crate) struct Ffv1ProcessCustody<'a> {
    output: &'a crate::fs::Root,
    output_path: &'a str,
    manifest_sha256: &'a str,
    stages: std::cell::RefCell<Vec<&'static str>>,
    failures: std::cell::RefCell<Vec<&'static str>>,
}

impl<'a> Ffv1ProcessCustody<'a> {
    pub(crate) fn new(
        output: &'a crate::fs::Root,
        output_path: &'a str,
        manifest_sha256: &'a str,
    ) -> Result<Self> {
        if !valid_digest(manifest_sha256) {
            return Err(Error::invalid("FFV1 custody source manifest identity"));
        }
        preflight_ffv1_custody(output, output_path, false)?;
        Ok(Self {
            output,
            output_path,
            manifest_sha256,
            stages: std::cell::RefCell::new(Vec::new()),
            failures: std::cell::RefCell::new(Vec::new()),
        })
    }

    fn failed(&self, code: &'static str) {
        let mut failures = self.failures.borrow_mut();
        if failures.len() < 32 {
            failures.push(code);
        }
    }

    fn stream(&self, stage: &str, name: &str, bytes: &[u8]) -> serde_json::Value {
        let prefix = &bytes[..bytes.len().min(FFV1_CUSTODY_STREAM_PREFIX)];
        let path = format!("{}.ffv1-{stage}.{name}.original.bin", self.output_path);
        let retained =
            retain_ffv1_original(self.output, &path, prefix, FFV1_CUSTODY_STREAM_PREFIX).is_ok();
        if !retained {
            self.failed("stream-retention");
        }
        serde_json::json!({
            "root":"output", "path":path, "sha256":crate::hash::sha256(prefix),
            "bytes":prefix.len(), "returned_buffer_bytes":bytes.len(),
            "returned_buffer_sha256":crate::hash::sha256(bytes),
            "diagnostic_prefix_truncated":bytes.len()>prefix.len(), "retained":retained,
            "upstream_stream_eof_attested":false
        })
    }

    fn record(
        &self,
        stage: &'static str,
        spec: &ProcessSpec,
        result: &Result<ProcessResult>,
        retained_limit: usize,
        total_limit: usize,
    ) {
        let next = self.stages.borrow().len();
        if FFV1_CUSTODY_STAGES.get(next).copied() != Some(stage) {
            self.failed("stage-order");
            return;
        }
        self.stages.borrow_mut().push(stage);
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
                "stdout":null, "stderr":null,
                "original_error":{"code":error.code,"message":error.message,"outcome_known":error.outcome_known}
            }),
        };
        let document = serde_json::json!({
            "schema":"semwright.ffv1-process-original/1", "stage":stage,
            "output_path":self.output_path,"source_manifest_sha256":self.manifest_sha256,
            "source_capture_retained_limit":retained_limit,"source_capture_total_limit":total_limit,
            "diagnostic_prefix_limit":FFV1_CUSTODY_STREAM_PREFIX,"spec":spec,"terminal":terminal,
            "native_execution_retried":false,"stream_eof_attested":false,
            "capture_scope":"existing-ProcessResult-before-checked-or-parse",
            "physical_acceptance":false
        });
        let bytes = match serde_json::to_vec(&document) {
            Ok(bytes) => bytes,
            Err(_) => {
                self.failed("terminal-serialization");
                return;
            }
        };
        let path = format!("{}.ffv1-{stage}.terminal.original.json", self.output_path);
        if retain_ffv1_original(self.output, &path, &bytes, FFV1_CUSTODY_METADATA).is_err() {
            self.failed("terminal-retention");
        }
    }

    pub(crate) fn complete<T>(&self, original: Result<T>) -> Result<T> {
        if original.is_ok() && self.stages.borrow().as_slice() != FFV1_CUSTODY_STAGES {
            self.failed("stage-coverage");
        }
        let document = serde_json::json!({
            "schema":"semwright.ffv1-process-custody/1", "output_path":self.output_path,
            "source_manifest_sha256":self.manifest_sha256,
            "observed_stages":*self.stages.borrow(), "secondary_failures":*self.failures.borrow(),
            "original_operation_succeeded":original.is_ok(),
            "original_error":original.as_ref().err().map(|e| serde_json::json!({"code":e.code,"message":e.message,"outcome_known":e.outcome_known})),
            "stream_eof_attested":false,"native_execution_retried":false,"physical_acceptance":false
        });
        let retained = serde_json::to_vec(&document).ok().is_some_and(|bytes| {
            retain_ffv1_original(
                self.output,
                &format!("{}{FFV1_CUSTODY_SUMMARY}", self.output_path),
                &bytes,
                FFV1_CUSTODY_METADATA,
            )
            .is_ok()
        });
        if !retained {
            self.failed("summary-retention");
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
fn capture(
    mut reader: impl Read + Send + 'static,
    overflow: Arc<AtomicBool>,
    retained_limit: usize,
    total_limit: usize,
) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut retained = Vec::new();
        let mut bytes = 0usize;
        let mut buf = [0; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    bytes = bytes.saturating_add(n);
                    let keep = n.min(retained_limit.saturating_sub(retained.len()));
                    retained.extend_from_slice(&buf[..keep]);
                    if bytes > total_limit {
                        overflow.store(true, Ordering::Release);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
        retained
    })
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
    if retained_limit == 0 || retained_limit > total_limit || total_limit > 8 * 1024 * 1024 {
        return Err(Error::limit("Process output capture budget is invalid"));
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
                (1, 1073741824),
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
    let child = command.spawn()?;
    let pgid = child.id() as i32;
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
    let overflow = Arc::new(AtomicBool::new(false));
    let a = capture(stdout, overflow.clone(), retained_limit, total_limit);
    let b = capture(stderr, overflow.clone(), retained_limit, total_limit);
    let start = Instant::now();
    let mut cancelled = false;
    let mut timed_out = false;
    let status;
    loop {
        if let Some(s) = child.try_wait()? {
            status = s;
            break;
        }
        cancelled = cancel.load(Ordering::Acquire);
        timed_out = start.elapsed() >= spec.timeout;
        if cancelled || timed_out || overflow.load(Ordering::Acquire) {
            // SAFETY: this process group was created for this still-owned child; negative PID targets it.
            unsafe { kill(-pgid, 15) };
            let until = Instant::now() + Duration::from_millis(300);
            while Instant::now() < until {
                if child.try_wait()?.is_some() {
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
            // SAFETY: kill all remaining members of the same owned process group before reaping.
            unsafe { kill(-pgid, 9) };
            status = child.wait()?;
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    // SAFETY: clean residual same-group descendants even after a successful parent exit.
    // A production bubblewrap PID namespace also contains descendants that create another group.
    unsafe { kill(-pgid, 9) };
    child.armed = false;
    let stdout = a
        .join()
        .map_err(|_| Error::new("Internal", "stdout reader failed"))?;
    let stderr = b
        .join()
        .map_err(|_| Error::new("Internal", "stderr reader failed"))?;
    Ok(ProcessResult {
        exit_code: status.code(),
        term_signal: status.signal(),
        stdout,
        stderr,
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
        self.probe_with_timeout(inputs, work, name, cancel, Duration::from_secs(5), None)
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
        let r = checked_ffv1_original(
            custody,
            "media-probe",
            &spec,
            run(&spec, cancel),
            RETAIN_LOG,
            TOTAL_LOG,
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
        let help = checked_ffv1_original(
            custody,
            "zscale-help",
            &spec,
            run(&spec, cancel),
            RETAIN_LOG,
            TOTAL_LOG,
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
        checked_ffv1_original(
            custody,
            "encode",
            &spec,
            run(&spec, cancel),
            RETAIN_LOG,
            TOTAL_LOG,
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
    fn observe_video_file_observed(
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
        let result = checked_ffv1_original(
            custody,
            "complete-probe",
            &spec,
            run_with_output_limits(&spec, cancel, color::REPORT_LIMIT, color::REPORT_LIMIT),
            color::REPORT_LIMIT,
            color::REPORT_LIMIT,
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
        let version = checked_ffv1_original(
            custody,
            "decoder-version",
            &spec,
            run(&spec, cancel),
            RETAIN_LOG,
            TOTAL_LOG,
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
        checked_ffv1_original(
            custody,
            "decode",
            &spec,
            run(&spec, cancel),
            RETAIN_LOG,
            TOTAL_LOG,
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

    pub fn extract_audio(
        &self,
        inputs: &Path,
        work: &Path,
        name: &str,
        sample_rate: u32,
        channels: u16,
        cancel: &AtomicBool,
    ) -> Result<MediaInfo> {
        crate::fs::validate_relative(name)?;
        if name.contains('/')
            || sample_rate != 48_000
            || channels != 2
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
        run(
            &self.spec("ffmpeg", args, inputs, work, self.timeout)?,
            cancel,
        )?
        .checked()?;
        let info = self.probe(work, work, "decoded-audio.wav", cancel)?;
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
        let args = render_argv(
            self.input_path(inputs, "project.mlt").into_os_string(),
            self.work_path(work, &format!("partial.{}", profile.extension))
                .into_os_string(),
            profile,
        );
        run(
            &self.spec(
                "melt",
                args,
                inputs,
                work,
                remaining_color_budget(deadline)?,
            )?,
            cancel,
        )?
        .checked()
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
        remaining_color_budget(deadline)?;
        let filename = format!("partial.{}", profile.extension);
        let mut info = self.probe_before_deadline(work, work, &filename, cancel, deadline, None)?;
        remaining_color_budget(deadline)?;
        let observation = if let Some(selected) = complete_observation_profile(profile) {
            let observed = self.observe_video_file(
                work,
                &filename,
                selected,
                (expected.width, expected.height),
                frames,
                expected.fps.num,
                expected.fps.den,
                deadline,
                cancel,
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
    fn ffv1_prefix_is_explicit_and_never_claims_upstream_eof() {
        let (directory, root) = fixture();
        let pin = "a".repeat(64);
        let custody = Ffv1ProcessCustody::new(&root, "first.mkv", &pin).unwrap();
        let mut observed = result(0);
        observed.stdout = vec![0xff; FFV1_CUSTODY_STREAM_PREFIX + 1];
        let output = checked_ffv1_original(
            Some(&custody),
            "zscale-help",
            &spec(directory.path()),
            Ok(observed),
            32768,
            65536,
        )
        .unwrap();
        assert_eq!(output.stdout.len(), 16385);
        let saved = document(&root, "first.mkv.ffv1-zscale-help.terminal.original.json");
        assert_eq!(saved["terminal"]["stdout"]["bytes"], 16384);
        assert_eq!(
            saved["terminal"]["stdout"]["diagnostic_prefix_truncated"],
            true
        );
        assert_eq!(
            saved["terminal"]["stdout"]["upstream_stream_eof_attested"],
            false
        );
        assert_eq!(saved["source_capture_retained_limit"], 32768);
        assert_eq!(saved["source_capture_total_limit"], 65536);
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
}
