//! Explicit owner diagnostic for the existing daemon-owned DriverProvider.
//! Feature and CLI opt-in are required. This is custody, never execution authority.
use clap::Args;
use semwright_driver_host::{
    HostStdinCapture, HostStdinCustody, HostStdinCustodyLosses, HostStdinCustodyReceiver,
    HostStdinWriteTerminal,
};
use semwright_driver_sdk::Manifest;
use semwright_types::{Error, ErrorCode, ProviderIdentity, Result};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

const MAX_RECORDS: usize = 64;
const MAX_INPUT: usize = 64 * 1024;
const MAX_TOTAL_INPUT: usize = MAX_RECORDS * MAX_INPUT;
const MAX_METADATA: usize = 8 * 1024;
const MAX_TERMINAL: usize = 2 * 1024;
const QUEUE: usize = 8;

#[derive(Args, Default)]
pub(super) struct Options {
    /// Existing empty private directory; sensitive originals remain local.
    #[arg(long)]
    host_stdin_custody_dir: Option<PathBuf>,
    /// Exact owner-configured provider identity, for example driver:motion-canvas.
    #[arg(long)]
    host_stdin_custody_provider: Option<String>,
    /// One existing Host-mediated tool in that provider's v7+ manifest.
    #[arg(long)]
    host_stdin_custody_tool: Option<String>,
}

struct Request {
    directory: PathBuf,
    provider: String,
    tool: String,
}

impl Options {
    fn request(&self) -> Result<Option<Request>> {
        match (
            &self.host_stdin_custody_dir,
            &self.host_stdin_custody_provider,
            &self.host_stdin_custody_tool,
        ) {
            (None, None, None) => Ok(None),
            (Some(directory), Some(provider), Some(tool)) => Ok(Some(Request {
                directory: directory.clone(),
                provider: provider.clone(),
                tool: tool.clone(),
            })),
            _ => Err(Error::invalid(
                "Host stdin custody requires directory, provider and tool together",
            )),
        }
    }

    #[cfg(test)]
    pub(super) fn disabled(&self) -> bool {
        matches!(self.request(), Ok(None))
    }
}

fn setup_error(error: Error) -> Error {
    let mut sanitized = Error::new(error.code, "Private Host stdin custody setup failed");
    sanitized.outcome_known = error.outcome_known;
    sanitized
}

struct Plan {
    manifests: Vec<(PathBuf, Manifest)>,
    selected: ProviderIdentity,
    driver_sha256: String,
    tool: String,
    tool_sha256: String,
}

impl Plan {
    fn select(manifests: Vec<(PathBuf, Manifest)>, request: &Request) -> Result<Self> {
        let mut matches = manifests.iter().filter_map(|(_, manifest)| {
            manifest
                .identity()
                .ok()
                .filter(|identity| identity.id == request.provider)
                .map(|identity| (manifest, identity))
        });
        let (manifest, selected) = matches
            .next()
            .ok_or_else(|| Error::invalid("Custody must select one configured provider"))?;
        if matches.next().is_some() {
            return Err(Error::invalid("Custody provider selection is ambiguous"));
        }
        if manifest.protocol < 7 || !manifest.interfaces.host_tools {
            return Err(Error::invalid("Custody requires an existing v7 Host tool"));
        }
        let tool = manifest
            .tools
            .iter()
            .find(|tool| tool.name == request.tool)
            .ok_or_else(|| Error::invalid("Custody tool is not configured"))?;
        let driver_sha256 = manifest.sha256.clone();
        let tool_sha256 = tool.sha256.clone();
        Ok(Self {
            manifests,
            selected,
            driver_sha256,
            tool: request.tool.clone(),
            tool_sha256,
        })
    }
}

struct PrivateDirectory {
    path: PathBuf,
    #[cfg(target_os = "linux")]
    root: semwright_platform_services::Root,
}

impl PrivateDirectory {
    fn open(path: &Path) -> Result<Self> {
        #[cfg(not(target_os = "linux"))]
        {
            let _ = path;
            Err(Error::new(
                ErrorCode::Unsupported,
                "Private no-replace Host stdin custody is supported only on Linux",
            ))
        }
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::MetadataExt;
            if !path.is_absolute()
                || path.parent().is_none()
                || std::fs::canonicalize(path)? != path
            {
                return Err(Error::invalid("Custody directory must be canonical"));
            }
            let uid = semwright_platform_services::current_uid();
            let metadata = std::fs::symlink_metadata(path)?;
            if !metadata.is_dir()
                || metadata.file_type().is_symlink()
                || metadata.uid() != uid
                || metadata.mode() & 0o777 != 0o700
            {
                return Err(Error::new(
                    ErrorCode::PermissionDenied,
                    "Custody directory must already be private and owner-held",
                ));
            }
            let parent = path
                .parent()
                .ok_or_else(|| Error::invalid("Custody parent missing"))?;
            let parent_metadata = std::fs::symlink_metadata(parent)?;
            if !parent_metadata.is_dir()
                || parent_metadata.file_type().is_symlink()
                || parent_metadata.uid() != uid
                || parent_metadata.mode() & 0o777 != 0o700
            {
                return Err(Error::new(
                    ErrorCode::PermissionDenied,
                    "Custody requires a dedicated private owner parent",
                ));
            }
            // Reject untrusted writable ancestors. This does not protect against
            // the trusted owner renaming its own tree during setup. Subsequent
            // writes use only the retained SDK Root.
            for ancestor in path.ancestors().skip(1) {
                let metadata = std::fs::symlink_metadata(ancestor)?;
                let sticky_root_directory = metadata.uid() == 0 && metadata.mode() & 0o1000 != 0;
                if !metadata.is_dir()
                    || metadata.file_type().is_symlink()
                    || (metadata.uid() != uid && metadata.uid() != 0)
                    || (metadata.mode() & 0o022 != 0 && !sticky_root_directory)
                {
                    return Err(Error::new(
                        ErrorCode::PermissionDenied,
                        "Custody ancestor is not protected from replacement",
                    ));
                }
            }
            if std::fs::read_dir(path)?.next().is_some() {
                return Err(Error::new(
                    ErrorCode::Conflict,
                    "Custody directory must be empty and dedicated to this process",
                ));
            }
            let root = semwright_platform_services::Root::open(path, true, true)?;
            Ok(Self {
                path: path.to_path_buf(),
                root,
            })
        }
    }

    fn write_new(&self, name: &str, bytes: &[u8]) -> Result<()> {
        // All callers supply fixed internal names; no provider/request/job ID
        // participates in a path. The SDK primitive retains the directory fd.
        if name.contains('/') || name.contains('\\') || name.is_empty() {
            return Err(Error::invalid("Invalid internal custody filename"));
        }
        #[cfg(target_os = "linux")]
        {
            self.root.write_new_atomic(Path::new(name), bytes)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = bytes;
            Err(Error::new(
                ErrorCode::Unsupported,
                "Private no-replace custody publication is unavailable",
            ))
        }
    }
}

struct StopOnDrop(CancellationToken);
impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

pub(super) struct Bootstrap {
    plan: Plan,
    directory: PathBuf,
    sink: HostStdinCustody,
    stop: StopOnDrop,
    worker: Option<oneshot::Receiver<()>>,
}

impl Bootstrap {
    pub(super) fn prepare(
        options: &Options,
        fake: bool,
        paths: &[PathBuf],
    ) -> Result<Option<Self>> {
        let Some(request) = options.request()? else {
            return Ok(None);
        };
        if fake {
            return Err(Error::invalid("Fake mode cannot enable Host stdin custody"));
        }
        Self::prepare_selected(request, paths)
            .map(Some)
            .map_err(setup_error)
    }

    fn prepare_selected(request: Request, paths: &[PathBuf]) -> Result<Self> {
        let manifests = paths
            .iter()
            .map(|path| {
                semwright_daemon::config::driver_manifest(path)
                    .map(|manifest| (path.clone(), manifest))
            })
            .collect::<Result<Vec<_>>>()?;
        // The exact validated manifests below are also used by the provider loop.
        // Re-reading a changed manifest after selection is not an admission path.
        let plan = Plan::select(manifests, &request)?;
        let directory = Arc::new(PrivateDirectory::open(&request.directory)?);
        let (sink, receiver) = HostStdinCustody::channel_for_tool(&plan.tool, QUEUE)?;
        let session = bounded_json(
            &json!({
                "schema": "semwright.host-stdin-custody/1",
                "provider": plan.selected,
                "driver_sha256": plan.driver_sha256,
                "tool": plan.tool,
                "tool_sha256": plan.tool_sha256,
                "queue_capacity": QUEUE,
                "max_records": MAX_RECORDS,
                "max_input_bytes": MAX_INPUT,
                "max_total_input_bytes": MAX_TOTAL_INPUT,
                "max_capture_metadata_bytes": MAX_METADATA,
                "max_terminal_metadata_bytes": MAX_TERMINAL,
                "scope": "original Host stdin and pipe write only; child consumption unobserved"
            }),
            MAX_METADATA,
        )?;
        // This is also the real no-replace capability preflight. No provider has
        // been launched by this bootstrap if it fails.
        directory.write_new("session.json", &session)?;
        let stop = CancellationToken::new();
        let runtime = tokio::runtime::Handle::try_current()
            .map_err(|_| Error::unavailable("Custody runtime unavailable"))?;
        let (finished, worker) = oneshot::channel();
        let output = directory.clone();
        let provider = plan.selected.clone();
        let tool = plan.tool.clone();
        let observed_sink = sink.clone();
        let observed_stop = stop.clone();
        // Exactly one private collector; reuse the current runtime's Handle.
        // This thread is not part of Tokio's blocking pool or native execution.
        let thread = std::thread::Builder::new()
            .name("semwright-stdin-custody".into())
            .spawn(move || {
                runtime.block_on(drain(
                    output,
                    provider,
                    tool,
                    receiver,
                    observed_sink,
                    observed_stop,
                ));
                let _ = finished.send(());
            })
            .map_err(|_| Error::unavailable("Custody collector unavailable"))?;
        drop(thread);
        Ok(Self {
            plan,
            directory: directory.path.clone(),
            sink,
            stop: StopOnDrop(stop),
            worker: Some(worker),
        })
    }

    pub(super) fn directory(&self) -> &Path {
        &self.directory
    }

    pub(super) fn manifest(&self, path: &Path) -> Result<Manifest> {
        self.plan
            .manifests
            .iter()
            .find(|(candidate, _)| candidate == path)
            .map(|(_, manifest)| manifest.clone())
            .ok_or_else(|| Error::invalid("Custody manifest plan changed"))
    }

    pub(super) fn sink_for(&self, manifest: &Manifest) -> Option<HostStdinCustody> {
        manifest
            .identity()
            .ok()
            .filter(|identity| identity == &self.plan.selected)
            .map(|_| self.sink.clone())
    }

    pub(super) async fn finish(&mut self) {
        self.stop.0.cancel();
        if let Some(mut worker) = self.worker.take() {
            // No thread join and no Tokio blocking task. A slow private writer
            // may outlive this wait until process exit; missing closure is
            // incomplete custody, never a native terminal.
            let _ = tokio::time::timeout(Duration::from_secs(2), &mut worker).await;
        }
    }
}

fn bounded_json(value: &Value, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec(value)
        .map_err(|_| Error::invalid("Custody metadata encoding failed"))?;
    bytes.push(b'\n');
    if bytes.len() > limit {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Custody metadata exceeds its fixed budget",
        ));
    }
    Ok(bytes)
}

#[derive(Default)]
struct Budget {
    records: usize,
    input_bytes: usize,
}

impl Budget {
    fn reserve(&mut self, input: usize, metadata: usize) -> Result<usize> {
        let total = self
            .input_bytes
            .checked_add(input)
            .ok_or_else(|| Error::invalid("Custody budget overflow"))?;
        if input > MAX_INPUT
            || metadata > MAX_METADATA
            || self.records >= MAX_RECORDS
            || total > MAX_TOTAL_INPUT
        {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Custody input or metadata budget exhausted",
            ));
        }
        self.records += 1;
        self.input_bytes = total;
        Ok(self.records)
    }
}

fn capture_metadata(
    capture: &HostStdinCapture,
    provider: &ProviderIdentity,
    tool: &str,
    process_id: Option<u32>,
) -> Result<Vec<u8>> {
    if capture.provider() != provider
        || capture.tool() != tool
        || capture.provider_process_id() == 0
        || capture.sha256().len() != 64
        || !capture
            .sha256()
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || process_id.is_some_and(|pid| pid != capture.provider_process_id())
        || [
            capture.job_id(),
            capture.request_id(),
            capture.parent_request_id(),
            capture.session_id(),
            capture.tool(),
        ]
        .iter()
        .any(|value| value.is_empty() || value.len() > 512 || value.contains('\0'))
    {
        return Err(Error::new(
            ErrorCode::StaleReference,
            "Custody record does not match the selected Host",
        ));
    }
    bounded_json(
        &json!({
            "provider": capture.provider(),
            "provider_process_id": capture.provider_process_id(),
            "job_id": capture.job_id(),
            "request_id": capture.request_id(),
            "parent_request_id": capture.parent_request_id(),
            "session_id": capture.session_id(),
            "tool": capture.tool(),
            "stdin_bytes": capture.original_stdin().len(),
            "stdin_sha256": capture.sha256(),
            "terminal_file_required": true
        }),
        MAX_METADATA,
    )
}

fn terminal_metadata(terminal: HostStdinWriteTerminal) -> Value {
    let (state, partial_possible, io_kind, raw_os_error) = match terminal {
        HostStdinWriteTerminal::NotObserved => ("not_observed", true, None, None),
        HostStdinWriteTerminal::NotAttempted => ("not_attempted", false, None, None),
        HostStdinWriteTerminal::EmptyInputNoWrite => ("empty_input_no_write", false, None, None),
        HostStdinWriteTerminal::WriteAllCompleted => ("write_all_completed", false, None, None),
        HostStdinWriteTerminal::WriteAllFailed { kind, raw_os_error } => (
            "write_all_failed",
            true,
            Some(format!("{kind:?}")),
            raw_os_error,
        ),
        HostStdinWriteTerminal::WriteAllInterrupted => ("write_all_interrupted", true, None, None),
    };
    json!({
        "write_state": state,
        "partial_delivery_possible": partial_possible,
        "io_error_kind": io_kind,
        "raw_os_error": raw_os_error,
        "child_consumption": "not_observed",
        "native_render_success": "not_inferred"
    })
}

fn losses_json(losses: HostStdinCustodyLosses) -> Value {
    json!({
        "capture_full": losses.capture_full,
        "capture_closed": losses.capture_closed,
        "capture_oversized": losses.capture_oversized,
        "terminal_receiver_lost": losses.terminal_receiver_lost
    })
}

fn publish(directory: Arc<PrivateDirectory>, name: String, bytes: Vec<u8>) -> Result<()> {
    directory.write_new(&name, &bytes)
}

async fn drain(
    directory: Arc<PrivateDirectory>,
    provider: ProviderIdentity,
    tool: String,
    mut receiver: HostStdinCustodyReceiver,
    sink: HostStdinCustody,
    stop: CancellationToken,
) {
    let mut budget = Budget::default();
    let mut process_id = None;
    let mut captured = 0usize;
    let mut terminals = 0usize;
    let mut incomplete = false;
    let mut reason = "owner_shutdown";
    loop {
        let capture = tokio::select! {
            biased;
            _ = stop.cancelled() => break,
            capture = receiver.recv() => match capture {
                Some(capture) => capture,
                None => { reason = "channel_closed"; break; }
            }
        };
        let metadata = match capture_metadata(&capture, &provider, &tool, process_id) {
            Ok(metadata) => metadata,
            Err(_) => {
                incomplete = true;
                reason = "identity_or_metadata_rejected";
                break;
            }
        };
        let sequence = match budget.reserve(capture.original_stdin().len(), metadata.len()) {
            Ok(sequence) => sequence,
            Err(_) => {
                incomplete = true;
                reason = "capture_budget_exhausted";
                break;
            }
        };
        process_id = Some(capture.provider_process_id());
        let output = directory.clone();
        let written = (|| {
            output.write_new(
                &format!("capture-{sequence:04}.stdin"),
                capture.original_stdin(),
            )?;
            output.write_new(&format!("capture-{sequence:04}.json"), &metadata)?;
            Ok::<HostStdinCapture, Error>(capture)
        })();
        let capture = match written {
            Ok(capture) => capture,
            _ => {
                incomplete = true;
                reason = "capture_publication_failed";
                break;
            }
        };
        captured += 1;
        let terminal = tokio::select! {
            biased;
            terminal = capture.terminal() => match terminal {
                Ok(terminal) => terminal_metadata(terminal),
                Err(_) => {
                    incomplete = true;
                    json!({"write_state":"terminal_receiver_lost",
                        "child_consumption":"not_observed","native_render_success":"not_inferred"})
                }
            },
            _ = stop.cancelled() => {
                incomplete = true;
                json!({"write_state":"observer_stopped_terminal_unavailable",
                    "child_consumption":"not_observed","native_render_success":"not_inferred"})
            }
        };
        let bytes = match bounded_json(&terminal, MAX_TERMINAL) {
            Ok(bytes) => bytes,
            Err(_) => {
                incomplete = true;
                reason = "terminal_metadata_rejected";
                break;
            }
        };
        if publish(
            directory.clone(),
            format!("capture-{sequence:04}.terminal.json"),
            bytes,
        )
        .is_err()
        {
            incomplete = true;
            reason = "terminal_publication_failed";
            break;
        }
        terminals += 1;
    }
    receiver.close();
    // Already-reserved permits can arrive after close. Empty is not disconnected.
    let mut queued_discarded = 0usize;
    let queue_disconnected = tokio::time::timeout(Duration::from_millis(200), async {
        while let Some(capture) = receiver.recv().await {
            queued_discarded += 1;
            drop(capture);
        }
    })
    .await
    .is_ok();
    drop(receiver);
    incomplete |= queued_discarded != 0 || !queue_disconnected;
    let losses = sink.losses();
    incomplete |= losses.capture_full != 0
        || losses.capture_closed != 0
        || losses.capture_oversized != 0
        || losses.terminal_receiver_lost != 0
        || captured != terminals;
    let report = bounded_json(
        &json!({
            "schema":"semwright.host-stdin-custody-close/1",
            "status": if incomplete { "incomplete" } else { "observer_closed" },
            "reason": reason,
            "capture_records_written": captured,
            "terminal_records_written": terminals,
            "input_bytes_reserved": budget.input_bytes,
            "queued_captures_discarded": queued_discarded,
            "queue_disconnection_proven": queue_disconnected,
            "loss_counters_at_observer_close": losses_json(losses),
            "expected_job_coverage": "caller_must_join_original_job_and_parent_ids",
            "child_consumption": "not_observed",
            "native_render_success": "not_inferred"
        }),
        MAX_METADATA,
    );
    if let Ok(bytes) = report {
        // No retry, replacement, or native error propagation if this fails.
        let _ = publish(directory, "closed.json".into(), bytes);
    }
}

#[cfg(test)]
#[path = "host_stdin_custody_tests.rs"]
mod tests;
