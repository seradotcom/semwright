//! Sandboxed persistent application-driver host. Drivers are mounted into the broker as Providers.
#[cfg(any(unix, windows))]
mod loopback;

use async_trait::async_trait;
use semwright_backend_api::{
    Context, ProvidedCapability, Provider, ProviderInterfaces, ProviderSignal,
};
use semwright_driver_sdk::{
    DriverInterfaces, DriverRequestContext, Manifest, Request, Response, capabilities_digest,
    descriptor_digest,
};
use semwright_platform_api::launch::{SandboxCpuAccounting, SandboxStdin, SandboxStdout};
use semwright_policy::FilesystemGrant;
use semwright_protocol::{read_frame, write_frame};
use semwright_types::{
    CapabilityStatus, CommandDescriptor, Error, ErrorCode, Feature, NativeTarget, ProviderIdentity,
    Result, unique_id,
};
use serde::Serialize;
use serde_json::{Value, json};
#[cfg(all(test, unix))]
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::os::{
    fd::AsRawFd,
    unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, RwLock as StdRwLock},
    time::Duration,
};
#[cfg(target_os = "linux")]
use std::{ffi::CString, os::fd::FromRawFd};
#[cfg(all(test, unix))]
use tokio::process::Command;
use tokio::sync::{Mutex, broadcast, oneshot};
use tokio_util::sync::CancellationToken;

struct StagedFile(PathBuf);
impl Drop for StagedFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(unix)]
struct SealedTool {
    name: String,
    // Own the immutable sealed memfd for the full provider lifetime.
    _file: std::fs::File,
    // Read-only descriptor inherited only by Bubblewrap so --file can
    // materialize the verified bytes at a fixed executable path inside the
    // private mount namespace.
    data_file: std::fs::File,
}
#[cfg(unix)]
impl SealedTool {
    fn sandbox_mount(&self) -> semwright_platform_api::launch::SealedToolMount {
        let _sealed_owner_fd = self._file.as_raw_fd();
        semwright_platform_api::launch::SealedToolMount {
            fd: self.data_file.as_raw_fd(),
            name: self.name.clone(),
        }
    }
}

#[cfg(target_os = "linux")]
fn seal_verified_tool(path: &Path, digest: &str, name: &str) -> Result<SealedTool> {
    let bytes = semwright_platform_services::verify_executable(path, digest)?;
    let label = CString::new(format!("semwright-tool-{name}"))
        .map_err(|_| Error::invalid("Invalid tool name"))?;
    // SAFETY: label is a live NUL-terminated CString and flags contain no pointers.
    let fd =
        unsafe { libc::memfd_create(label.as_ptr(), libc::MFD_ALLOW_SEALING | libc::MFD_CLOEXEC) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: memfd_create returned a new owned descriptor on success.
    let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
    file.write_all(&bytes)?;
    file.sync_all()?;
    // SAFETY: fd is live and fchmod only reads scalar arguments.
    if unsafe { libc::fchmod(fd, 0o500) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let seals = libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL;
    // SAFETY: fd is a live memfd created with MFD_ALLOW_SEALING.
    if unsafe { libc::fcntl(fd, libc::F_ADD_SEALS, seals) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: F_GET_SEALS returns scalar seal bits for a live memfd.
    let actual = unsafe { libc::fcntl(fd, libc::F_GET_SEALS) };
    if actual < 0 || actual & seals != seals {
        return Err(Error::new(
            ErrorCode::SandboxDenied,
            "Driver tool memfd could not be sealed",
        ));
    }
    let proc_path = CString::new(format!("/proc/self/fd/{fd}"))
        .map_err(|_| Error::invalid("Invalid sealed tool descriptor path"))?;
    // Re-open the sealed inode read-only. This descriptor deliberately omits
    // O_CLOEXEC so Bubblewrap can consume it with --file; the original
    // read-write memfd stays CLOEXEC and sealed.
    // SAFETY: proc_path is a live NUL-terminated path to the sealed memfd.
    let data_fd = unsafe { libc::open(proc_path.as_ptr(), libc::O_RDONLY) };
    if data_fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: open returned a new owned read-only descriptor on success.
    let data_file = unsafe { std::fs::File::from_raw_fd(data_fd) };
    // SAFETY: F_GETFD reads scalar descriptor flags only.
    let fd_flags = unsafe { libc::fcntl(data_fd, libc::F_GETFD) };
    // SAFETY: F_GETFL reads scalar file status flags only.
    let status_flags = unsafe { libc::fcntl(data_fd, libc::F_GETFL) };
    if fd_flags < 0
        || status_flags < 0
        || fd_flags & libc::FD_CLOEXEC != 0
        || status_flags & libc::O_ACCMODE != libc::O_RDONLY
    {
        return Err(Error::new(
            ErrorCode::SandboxDenied,
            "Driver tool materialization descriptor is not read-only/inheritable",
        ));
    }
    Ok(SealedTool {
        name: name.to_owned(),
        _file: file,
        data_file,
    })
}

#[cfg(all(unix, not(target_os = "linux")))]
fn seal_verified_tool(_path: &Path, _digest: &str, _name: &str) -> Result<SealedTool> {
    Err(Error::new(
        ErrorCode::Unsupported,
        "Sealed Driver Host tools are not implemented on this platform",
    ))
}

fn verify_owned_executable(path: &Path, digest: &str) -> Result<Vec<u8>> {
    semwright_platform_services::verify_executable(path, digest)
}

#[cfg(target_os = "linux")]
fn one_process_cpu_ticks(pid: u32) -> Result<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .map_err(|_| Error::unavailable("Driver process CPU accounting disappeared"))?;
    let end = stat
        .rfind(')')
        .ok_or_else(|| Error::new(ErrorCode::Internal, "Malformed /proc process stat"))?;
    let fields = stat[end + 1..].split_whitespace().collect::<Vec<_>>();
    if fields.len() <= 14 {
        return Err(Error::new(
            ErrorCode::Internal,
            "Malformed /proc process CPU fields",
        ));
    }
    let mut total = 0u64;
    // After the command name, indexes 11..=14 are utime, stime, cutime and cstime.
    // Including waited-for children plus live descendants prevents secondary runners from
    // escaping a per-operation CPU budget.
    for field in &fields[11..=14] {
        total = total
            .checked_add(
                field
                    .parse::<u64>()
                    .map_err(|_| Error::new(ErrorCode::Internal, "Malformed process CPU ticks"))?,
            )
            .ok_or_else(|| Error::new(ErrorCode::ResourceExhausted, "Process CPU tick overflow"))?;
    }
    Ok(total)
}

#[cfg(target_os = "linux")]
fn process_children(pid: u32) -> Result<Vec<u32>> {
    let task_dir = match std::fs::read_dir(format!("/proc/{pid}/task")) {
        Ok(task_dir) => task_dir,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut tids = 0usize;
    let mut children = std::collections::BTreeSet::new();
    for entry in task_dir {
        let entry = entry?;
        let Some(tid) = entry
            .file_name()
            .to_str()
            .and_then(|value| value.parse::<u32>().ok())
        else {
            continue;
        };
        tids += 1;
        if tids > 256 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Driver thread count exceeded CPU accounting bounds",
            ));
        }
        let path = format!("/proc/{pid}/task/{tid}/children");
        let text = std::fs::read_to_string(path).unwrap_or_default();
        for child in text.split_whitespace() {
            if let Ok(child) = child.parse::<u32>() {
                children.insert(child);
                if children.len() > 256 {
                    return Err(Error::new(
                        ErrorCode::ResourceExhausted,
                        "Driver child count exceeded CPU accounting bounds",
                    ));
                }
            }
        }
    }
    Ok(children.into_iter().collect())
}

#[cfg(target_os = "linux")]
fn process_tree_cpu_ticks(root: u32) -> Result<u64> {
    let mut pending = vec![root];
    let mut seen = std::collections::BTreeSet::new();
    let mut total = 0u64;
    while let Some(pid) = pending.pop() {
        if !seen.insert(pid) {
            continue;
        }
        if seen.len() > 256 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Driver process tree exceeded CPU accounting bounds",
            ));
        }
        let ticks = match one_process_cpu_ticks(pid) {
            Ok(ticks) => ticks,
            Err(error) if pid != root && error.code == ErrorCode::Unavailable => continue,
            Err(error) => return Err(error),
        };
        total = total
            .checked_add(ticks)
            .ok_or_else(|| Error::new(ErrorCode::ResourceExhausted, "Process CPU tick overflow"))?;
        pending.extend(process_children(pid)?);
    }
    Ok(total)
}

#[cfg(target_os = "linux")]
fn cpu_ticks_per_second() -> Result<u64> {
    // SAFETY: sysconf with _SC_CLK_TCK has no pointer arguments or memory-safety preconditions.
    let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    u64::try_from(ticks)
        .ok()
        .filter(|ticks| *ticks > 0)
        .ok_or_else(|| Error::new(ErrorCode::Unavailable, "CPU clock tick rate is unavailable"))
}

#[cfg(target_os = "linux")]
async fn wait_for_operation_cpu_budget(
    pid: u32,
    _cpu_accounting: Option<Arc<dyn SandboxCpuAccounting>>,
    seconds: u64,
) -> Result<()> {
    if seconds == 0 {
        return std::future::pending::<Result<()>>().await;
    }
    let start = process_tree_cpu_ticks(pid)?;
    let limit = seconds
        .checked_mul(cpu_ticks_per_second()?)
        .ok_or_else(|| Error::new(ErrorCode::ResourceExhausted, "CPU budget overflow"))?;
    loop {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let current = process_tree_cpu_ticks(pid)?;
        if current.saturating_sub(start) >= limit {
            return Ok(());
        }
    }
}

#[cfg(target_os = "windows")]
async fn wait_for_operation_cpu_budget(
    _pid: u32,
    cpu_accounting: Option<Arc<dyn SandboxCpuAccounting>>,
    seconds: u64,
) -> Result<()> {
    if seconds == 0 {
        return std::future::pending::<Result<()>>().await;
    }
    let accounting = cpu_accounting.ok_or_else(|| {
        Error::new(
            ErrorCode::Unsupported,
            "Windows sandbox does not expose cumulative Job CPU accounting",
        )
    })?;
    let start = accounting.total_cpu_time()?;
    let limit = Duration::from_secs(seconds);
    loop {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let current = accounting.total_cpu_time()?;
        let used = current.checked_sub(start).ok_or_else(|| {
            Error::new(
                ErrorCode::BackendFailed,
                "Windows sandbox CPU accounting moved backwards",
            )
        })?;
        if used >= limit {
            return Ok(());
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
async fn wait_for_operation_cpu_budget(
    _pid: u32,
    _cpu_accounting: Option<Arc<dyn SandboxCpuAccounting>>,
    seconds: u64,
) -> Result<()> {
    if seconds == 0 {
        std::future::pending::<Result<()>>().await
    } else {
        Err(Error::new(
            ErrorCode::Unsupported,
            "Per-operation driver CPU accounting is unavailable on this platform",
        ))
    }
}

fn validate_secret_source(path: &Path) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        semwright_platform_services::verify_private_data_file(path, 4096)
    }

    #[cfg(not(target_os = "windows"))]
    {
        if std::fs::canonicalize(path)? != path {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Driver secret source must be canonical",
            ));
        }
        let metadata = std::fs::symlink_metadata(path)?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > 4096 {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Driver secret source must be a small regular file",
            ));
        }
        #[cfg(unix)]
        {
            if metadata.uid() != semwright_platform_services::current_uid()
                || metadata.mode() & 0o077 != 0
                || metadata.nlink() != 1
            {
                return Err(Error::new(
                    ErrorCode::PermissionDenied,
                    "Driver secret source must be owner-only and single-linked",
                ));
            }
        }
        Ok(())
    }
}

fn validate_owner_permissions(
    manifest: &Manifest,
    roots: &[FilesystemGrant],
    allow_network: bool,
) -> Result<()> {
    manifest.validate()?;
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    if manifest.resources.operation_cpu_seconds != 0 {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "Per-operation driver CPU accounting is unavailable on this platform",
        ));
    }
    if manifest.network && !allow_network {
        return Err(Error::new(
            ErrorCode::PolicyDenied,
            "Driver requests network but owner configuration denies it",
        ));
    }
    for mount in &manifest.mounts {
        let grant = roots
            .iter()
            .find(|grant| grant.name == mount.root)
            .ok_or_else(|| {
                Error::new(ErrorCode::PolicyDenied, "Driver mount has no owner grant")
            })?;
        if !grant.read || (!mount.read_only && !grant.write) {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Driver mount exceeds its filesystem grant",
            ));
        }
        #[cfg(unix)]
        if std::fs::canonicalize(&grant.path)? != grant.path {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Driver filesystem grants must be canonical paths",
            ));
        }
        #[cfg(target_os = "windows")]
        if !grant.path.is_absolute() {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Windows driver filesystem grants must use absolute paths",
            ));
        }
    }
    for mount in &manifest.system_config {
        let grant = roots
            .iter()
            .find(|grant| grant.name == mount.root)
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::PolicyDenied,
                    "Driver system config mount has no owner grant",
                )
            })?;
        if !grant.read {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Driver system config mount requires a readable owner grant",
            ));
        }
        #[cfg(unix)]
        if std::fs::canonicalize(&grant.path)? != grant.path {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Driver system config mount requires a canonical owner grant",
            ));
        }
        #[cfg(target_os = "windows")]
        if !grant.path.is_absolute() {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Windows driver system config grant must use an absolute path",
            ));
        }
    }
    for secret in &manifest.secrets {
        let grant = roots
            .iter()
            .find(|grant| grant.name == secret.root)
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::PolicyDenied,
                    "Driver secret mount has no owner grant",
                )
            })?;
        if !grant.read {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Driver secret mount requires readable owner grant",
            ));
        }
        validate_secret_source(&grant.path)?;
    }
    for tool in &manifest.tools {
        let grant = roots
            .iter()
            .find(|grant| grant.name == tool.root)
            .ok_or_else(|| Error::new(ErrorCode::PolicyDenied, "Driver tool has no owner grant"))?;
        if !grant.read || std::fs::canonicalize(&grant.path)? != grant.path {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Driver tool requires canonical readable owner grant",
            ));
        }
    }
    if manifest.protocol == 1
        && (manifest.interfaces.dynamic_capabilities
            || manifest.interfaces.cooperative_cancellation
            || manifest.interfaces.events
            || manifest.interfaces.progress
            || manifest.interfaces.artifacts)
    {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "Driver protocol v1 cannot negotiate dynamic/events/progress/artifacts/cancellation",
        ));
    }
    if manifest.interfaces.artifacts && !manifest.interfaces.progress {
        return Err(Error::invalid(
            "Driver artifact reporting requires negotiated progress reporting",
        ));
    }
    Ok(())
}

struct Io {
    input: SandboxStdin,
    output: SandboxStdout,
}

struct V2Io {
    input: Mutex<SandboxStdin>,
    pending: Arc<Mutex<BTreeMap<String, oneshot::Sender<Response>>>>,
}

#[cfg_attr(target_os = "windows", allow(dead_code))]
enum ProtocolIo {
    V1(Mutex<Io>),
    V2(V2Io),
}

impl V2Io {
    async fn begin(&self, request: &Request, id: &str) -> Result<oneshot::Receiver<Response>> {
        let (sender, receiver) = oneshot::channel();
        {
            let mut pending = self.pending.lock().await;
            if pending.insert(id.to_owned(), sender).is_some() {
                return Err(Error::new(
                    ErrorCode::Conflict,
                    "Driver protocol request ID is already pending",
                ));
            }
        }
        let result = {
            let mut input = self.input.lock().await;
            write_frame(&mut *input, request).await
        };
        if let Err(error) = result {
            self.pending.lock().await.remove(id);
            return Err(error);
        }
        Ok(receiver)
    }

    async fn request(&self, request: &Request, id: &str, timeout: Duration) -> Result<Response> {
        let receiver = self.begin(request, id).await?;
        match tokio::time::timeout(timeout, receiver).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => Err(Error::unavailable("Driver response channel closed")),
            Err(_) => {
                self.pending.lock().await.remove(id);
                Err(Error::new(
                    ErrorCode::Timeout,
                    "Driver protocol request timed out",
                ))
            }
        }
    }
}

fn provider_interfaces(interfaces: DriverInterfaces) -> ProviderInterfaces {
    ProviderInterfaces {
        dynamic_capabilities: interfaces.dynamic_capabilities,
        cooperative_cancellation: interfaces.cooperative_cancellation,
        events: interfaces.events,
        progress: interfaces.progress,
        artifacts: interfaces.artifacts,
        health: interfaces.health,
        native_refs: interfaces.native_refs,
    }
}

fn response_id(response: &Response) -> Option<&str> {
    match response {
        Response::Interfaces { id, .. }
        | Response::Capabilities { id, .. }
        | Response::Result { id, .. }
        | Response::Failure { id, .. }
        | Response::Cancelled { id, .. }
        | Response::Validated { id }
        | Response::Healthy { id, .. }
        | Response::Shutdown { id } => Some(id),
        Response::Ready { .. }
        | Response::Event { .. }
        | Response::CapabilitiesChanged
        | Response::Progress { .. } => None,
    }
}

fn spawn_v2_reader(
    mut output: SandboxStdout,
    pending: Arc<Mutex<BTreeMap<String, oneshot::Sender<Response>>>>,
    signals: broadcast::Sender<ProviderSignal>,
    interfaces: DriverInterfaces,
    closed: CancellationToken,
    terminate: CancellationToken,
) {
    tokio::spawn(async move {
        loop {
            let response = match read_frame::<_, Response>(&mut output).await {
                Ok(response) => response,
                Err(_) => {
                    break;
                }
            };
            match response {
                Response::Event { kind, payload } => {
                    if !interfaces.events {
                        terminate.cancel();
                        closed.cancel();
                        break;
                    }
                    let _ = signals.send(ProviderSignal::Event { kind, payload });
                }
                Response::CapabilitiesChanged => {
                    if !interfaces.dynamic_capabilities {
                        terminate.cancel();
                        closed.cancel();
                        break;
                    }
                    let _ = signals.send(ProviderSignal::CapabilitiesChanged);
                }
                Response::Progress {
                    id,
                    progress,
                    artifacts,
                } => {
                    if !interfaces.progress
                        || (!artifacts.is_empty() && !interfaces.artifacts)
                        || progress.validate().is_err()
                        || artifacts.len() > 32
                        || artifacts
                            .iter()
                            .any(|artifact| artifact.validate().is_err())
                    {
                        terminate.cancel();
                        closed.cancel();
                        break;
                    }
                    let _ = signals.send(ProviderSignal::Progress {
                        request_id: id,
                        progress,
                        artifacts,
                    });
                }
                other => {
                    let Some(id) = response_id(&other).map(str::to_owned) else {
                        terminate.cancel();
                        closed.cancel();
                        break;
                    };
                    let sender = pending.lock().await.remove(&id);
                    let Some(sender) = sender else {
                        terminate.cancel();
                        closed.cancel();
                        break;
                    };
                    let _ = sender.send(other);
                }
            }
        }
        terminate.cancel();
        closed.cancel();
        pending.lock().await.clear();
        let _ = signals.send(ProviderSignal::Disconnected);
    });
}

async fn request<T: Serialize>(io: &mut Io, request: &T, timeout: Duration) -> Result<Response> {
    tokio::time::timeout(timeout, async {
        write_frame(&mut io.input, request).await?;
        read_frame::<_, Response>(&mut io.output).await
    })
    .await
    .map_err(|_| Error::new(ErrorCode::Timeout, "Driver protocol request timed out"))?
}

#[cfg(unix)]
fn sandbox_spec(
    manifest: &Manifest,
    staged: &Path,
    helper: &Path,
    roots: &[FilesystemGrant],
    loopback_directory: Option<&Path>,
    sealed_tools: &[SealedTool],
) -> Result<semwright_platform_api::launch::SandboxSpec> {
    use semwright_platform_api::launch::{
        Mount, MountClass, ResourceLimits, SandboxKind, SandboxSpec,
    };
    let lookup = |name: &str| -> Result<&FilesystemGrant> {
        roots
            .iter()
            .find(|g| g.name == name)
            .ok_or_else(|| Error::new(ErrorCode::PolicyDenied, "Driver grant disappeared"))
    };
    let mut mounts = manifest
        .mounts
        .iter()
        .map(|m| {
            Ok(Mount {
                source: lookup(&m.root)?.path.clone(),
                class: MountClass::Workspace,
                logical_name: m.root.clone(),
                read_only: m.read_only,
                execute: m.execute,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    mounts.extend(
        manifest
            .system_config
            .iter()
            .map(|m| {
                let relative = m.destination.strip_prefix("/etc").map_err(|_| {
                    Error::new(
                        ErrorCode::PolicyDenied,
                        "Driver system config destination must remain under /etc",
                    )
                })?;
                let logical_name = relative
                    .to_str()
                    .ok_or_else(|| {
                        Error::invalid("Driver system config destination must be UTF-8")
                    })?
                    .trim_start_matches('/')
                    .to_owned();
                Ok(Mount {
                    source: lookup(&m.root)?.path.clone(),
                    class: MountClass::SystemConfig,
                    logical_name,
                    read_only: true,
                    execute: false,
                })
            })
            .collect::<Result<Vec<_>>>()?,
    );
    mounts.extend(
        manifest
            .secrets
            .iter()
            .map(|secret| {
                Ok(Mount {
                    source: lookup(&secret.root)?.path.clone(),
                    class: MountClass::Secret,
                    logical_name: secret.name.clone(),
                    read_only: true,
                    execute: false,
                })
            })
            .collect::<Result<Vec<_>>>()?,
    );
    let mut environment = Vec::new();
    if let Some(directory) = loopback_directory {
        mounts.push(Mount {
            source: directory.to_path_buf(),
            class: MountClass::Workspace,
            logical_name: loopback::MOUNT_NAME.into(),
            read_only: false,
            execute: false,
        });
        environment.push((
            "SEMWRIGHT_DRIVER_LOOPBACK_SOCKET".into(),
            loopback::SANDBOX_SOCKET.into(),
        ));
    }
    Ok(SandboxSpec {
        kind: SandboxKind::Driver,
        staged_executable: staged.into(),
        helper: helper.into(),
        mounts,
        args: vec![],
        environment,
        sealed_tools: sealed_tools.iter().map(SealedTool::sandbox_mount).collect(),
        network: manifest.network,
        limits: Some(ResourceLimits {
            open_files: manifest.resources.open_files,
            processes: manifest.resources.processes,
            cpu_seconds: manifest.resources.cpu_seconds,
            address_space_bytes: manifest.resources.address_space_bytes,
            file_size_bytes: manifest.resources.file_size_bytes,
        }),
    })
}

#[cfg(all(test, unix))]
fn sandbox_command(
    manifest: &Manifest,
    staged: &Path,
    helper: &Path,
    roots: &[FilesystemGrant],
    loopback_directory: Option<&Path>,
    sealed_tools: &[SealedTool],
) -> Result<Command> {
    semwright_platform_services::sandbox_command(&sandbox_spec(
        manifest,
        staged,
        helper,
        roots,
        loopback_directory,
        sealed_tools,
    )?)
}

#[cfg(target_os = "windows")]
fn sandbox_spec_windows(
    manifest: &Manifest,
    staged: &Path,
    helper: &Path,
    roots: &[FilesystemGrant],
    loopback_pipe: Option<&Path>,
) -> Result<semwright_platform_api::launch::SandboxSpec> {
    use semwright_platform_api::launch::{
        Mount, MountClass, ResourceLimits, SandboxKind, SandboxSpec,
    };

    if !manifest.tools.is_empty() {
        return Err(Error::new(
            ErrorCode::SandboxDenied,
            "Windows tool grants remain fail-closed until their immutable-executable contract is proven",
        ));
    }
    let lookup = |name: &str| -> Result<&FilesystemGrant> {
        roots
            .iter()
            .find(|grant| grant.name == name)
            .ok_or_else(|| Error::new(ErrorCode::PolicyDenied, "Driver grant disappeared"))
    };
    let mut mounts = manifest
        .mounts
        .iter()
        .map(|mount| {
            let grant = lookup(&mount.root)?;
            Ok(Mount {
                source: grant.path.clone(),
                class: MountClass::Workspace,
                logical_name: mount.root.clone(),
                read_only: mount.read_only,
                execute: mount.execute,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    mounts.extend(
        manifest
            .system_config
            .iter()
            .map(|mount| {
                let relative = mount.destination.strip_prefix("/etc").map_err(|_| {
                    Error::new(
                        ErrorCode::PolicyDenied,
                        "Driver system config destination must remain under /etc",
                    )
                })?;
                let logical_name = relative
                    .to_str()
                    .ok_or_else(|| {
                        Error::invalid("Driver system config destination must be UTF-8")
                    })?
                    .trim_start_matches('/')
                    .to_owned();
                let grant = lookup(&mount.root)?;
                Ok(Mount {
                    source: grant.path.clone(),
                    class: MountClass::SystemConfig,
                    logical_name,
                    read_only: true,
                    execute: false,
                })
            })
            .collect::<Result<Vec<_>>>()?,
    );
    mounts.extend(
        manifest
            .secrets
            .iter()
            .map(|secret| {
                let grant = lookup(&secret.root)?;
                Ok(Mount {
                    source: grant.path.clone(),
                    class: MountClass::Secret,
                    logical_name: secret.name.clone(),
                    read_only: true,
                    execute: false,
                })
            })
            .collect::<Result<Vec<_>>>()?,
    );
    let environment = loopback_pipe
        .map(|path| {
            path.to_str()
                .ok_or_else(|| Error::invalid("Windows loopback pipe path must be Unicode"))
                .map(|value| vec![(loopback::SANDBOX_PIPE_ENV.into(), value.to_owned())])
        })
        .transpose()?
        .unwrap_or_default();
    Ok(SandboxSpec {
        kind: SandboxKind::Driver,
        staged_executable: staged.into(),
        helper: helper.into(),
        mounts,
        args: vec![],
        environment,
        sealed_tools: vec![],
        network: manifest.network,
        limits: Some(ResourceLimits {
            open_files: manifest.resources.open_files,
            processes: manifest.resources.processes,
            cpu_seconds: manifest.resources.cpu_seconds,
            address_space_bytes: manifest.resources.address_space_bytes,
            file_size_bytes: manifest.resources.file_size_bytes,
        }),
    })
}

pub struct DriverProvider {
    identity: ProviderIdentity,
    manifest: Manifest,
    capabilities: StdRwLock<Vec<ProvidedCapability>>,
    descriptor_digests: StdRwLock<BTreeMap<String, String>>,
    io: ProtocolIo,
    signals: Option<broadcast::Sender<ProviderSignal>>,
    interfaces: ProviderInterfaces,
    closed: CancellationToken,
    terminate: CancellationToken,
    process_id: u32,
    cpu_accounting: Option<Arc<dyn SandboxCpuAccounting>>,
    operation_cpu_gate: Mutex<()>,
    _staged: Arc<StagedFile>,
    #[cfg(any(unix, windows))]
    _loopback: Option<Arc<loopback::LoopbackProxy>>,
    #[cfg(unix)]
    _tools: Vec<SealedTool>,
}

impl DriverProvider {
    pub async fn connect(
        manifest: Manifest,
        state: &Path,
        helper: &Path,
        roots: &[FilesystemGrant],
        allow_network: bool,
    ) -> Result<Arc<Self>> {
        validate_owner_permissions(&manifest, roots, allow_network)?;
        #[cfg(target_os = "windows")]
        {
            semwright_protocol::private_directory(state)?;
            let identity = manifest.identity()?;
            let bytes = verify_owned_executable(&manifest.executable, &manifest.sha256)?;
            let staged_path = state.join(format!("driver-{}.exe", unique_id()));
            let staged = Arc::new(StagedFile(staged_path.clone()));
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&staged_path)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);

            // Bind trust checks to the exact staged file that will execute.
            let _ = verify_owned_executable(&staged_path, &manifest.sha256)?;
            let loopback = match manifest.loopback_port {
                Some(port) => Some(loopback::start(state, port).await?),
                None => None,
            };
            let spec = sandbox_spec_windows(
                &manifest,
                &staged_path,
                helper,
                roots,
                loopback.as_deref().map(loopback::LoopbackProxy::pipe_path),
            )?;
            let mut child = semwright_platform_services::sandbox_spawn(&spec)?;
            let process_id = child
                .id()
                .ok_or_else(|| Error::new(ErrorCode::Internal, "Driver child has no PID"))?;
            if let Some(proxy) = &loopback
                && let Err(error) = proxy.activate(process_id).await
            {
                let _ = child.kill().await;
                return Err(error);
            }
            let cpu_accounting = child.cpu_accounting();
            #[cfg(target_os = "windows")]
            if manifest.resources.operation_cpu_seconds != 0 && cpu_accounting.is_none() {
                let _ = child.kill().await;
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "Windows Driver Host requires Job CPU accounting for per-operation budgets",
                ));
            }
            let input = child.take_stdin()?;
            let output = child.take_stdout()?;
            let mut io = Io { input, output };
            let timeout = Duration::from_millis(manifest.request_timeout_ms.min(30_000));

            let hello = request(
                &mut io,
                &Request::Hello {
                    protocol: manifest.protocol,
                    provider: identity.clone(),
                    executable_sha256: manifest.sha256.to_ascii_lowercase(),
                },
                timeout,
            )
            .await?;
            match hello {
                Response::Ready {
                    protocol,
                    id,
                    version,
                } if protocol == manifest.protocol
                    && id == manifest.id
                    && version == manifest.version => {}
                _ => {
                    let _ = child.kill().await;
                    return Err(Error::new(
                        ErrorCode::ProtocolMismatch,
                        "Driver handshake did not attest the expected identity/version",
                    ));
                }
            }

            if manifest.protocol >= 2 {
                let interfaces_id = unique_id();
                match request(
                    &mut io,
                    &Request::Interfaces {
                        id: interfaces_id.clone(),
                    },
                    timeout,
                )
                .await?
                {
                    Response::Interfaces { id, interfaces }
                        if id == interfaces_id && interfaces == manifest.interfaces => {}
                    _ => {
                        let _ = child.kill().await;
                        return Err(Error::new(
                            ErrorCode::ProtocolMismatch,
                            "Driver interface negotiation did not match the owner manifest",
                        ));
                    }
                }
            }

            let id = unique_id();
            let response =
                request(&mut io, &Request::Capabilities { id: id.clone() }, timeout).await?;
            let capabilities = match response {
                Response::Capabilities {
                    id: response_id,
                    capabilities,
                    digest,
                } if response_id == id && digest == capabilities_digest(&capabilities)? => {
                    capabilities
                }
                _ => {
                    let _ = child.kill().await;
                    return Err(Error::new(
                        ErrorCode::ProtocolMismatch,
                        "Driver capability attestation mismatch",
                    ));
                }
            };
            if capabilities.is_empty() || capabilities.len() > 2048 {
                let _ = child.kill().await;
                return Err(Error::invalid(
                    "Driver returned an invalid capability count",
                ));
            }
            let mut provided = Vec::with_capacity(capabilities.len());
            let mut digests = BTreeMap::new();
            for capability in capabilities {
                capability.validate_for(&identity)?;
                let digest = descriptor_digest(&capability.descriptor)?;
                if digests
                    .insert(capability.descriptor.name.clone(), digest)
                    .is_some()
                {
                    let _ = child.kill().await;
                    return Err(Error::new(
                        ErrorCode::Conflict,
                        "Driver returned duplicate capability names",
                    ));
                }
                provided.push(ProvidedCapability {
                    descriptor: capability.descriptor,
                    aliases: capability.aliases,
                    tags: capability.tags,
                    object_types: capability.object_types,
                });
            }

            let health_id = unique_id();
            match request(
                &mut io,
                &Request::Health {
                    id: health_id.clone(),
                },
                timeout,
            )
            .await?
            {
                Response::Healthy { id, .. } if id == health_id => {}
                _ => {
                    let _ = child.kill().await;
                    return Err(Error::new(
                        ErrorCode::ProtocolMismatch,
                        "Driver health handshake failed",
                    ));
                }
            }

            let closed = CancellationToken::new();
            let terminate = CancellationToken::new();
            let interfaces = if manifest.protocol >= 2 {
                provider_interfaces(manifest.interfaces)
            } else {
                ProviderInterfaces {
                    health: manifest.interfaces.health,
                    ..Default::default()
                }
            };
            let (protocol_io, signals) = if manifest.protocol >= 2 {
                let (signals, _) = broadcast::channel(128);
                let pending = Arc::new(Mutex::new(BTreeMap::new()));
                let Io { input, output } = io;
                spawn_v2_reader(
                    output,
                    pending.clone(),
                    signals.clone(),
                    manifest.interfaces,
                    closed.clone(),
                    terminate.clone(),
                );
                (
                    ProtocolIo::V2(V2Io {
                        input: Mutex::new(input),
                        pending,
                    }),
                    Some(signals),
                )
            } else {
                (ProtocolIo::V1(Mutex::new(io)), None)
            };
            let monitor_closed = closed.clone();
            let monitor_terminate = terminate.clone();
            let monitor_staged = staged.clone();
            let monitor_loopback = loopback.clone();
            tokio::spawn(async move {
                tokio::select! {
                    _ = monitor_terminate.cancelled() => {
                        let _ = child.kill().await;
                    }
                    _ = child.wait() => {}
                }
                if let Some(proxy) = monitor_loopback {
                    proxy.shutdown();
                }
                monitor_closed.cancel();
                drop(monitor_staged);
            });

            Ok(Arc::new(Self {
                identity,
                manifest,
                capabilities: StdRwLock::new(provided),
                descriptor_digests: StdRwLock::new(digests),
                io: protocol_io,
                signals,
                interfaces,
                closed,
                terminate,
                process_id,
                cpu_accounting,
                operation_cpu_gate: Mutex::new(()),
                _staged: staged,
                _loopback: loopback,
            }))
        }
        #[cfg(unix)]
        {
            semwright_protocol::private_directory(state)?;
            let identity = manifest.identity()?;
            let bytes = verify_owned_executable(&manifest.executable, &manifest.sha256)?;
            let staged_path = state.join(format!("driver-{}", unique_id()));
            let staged = Arc::new(StagedFile(staged_path.clone()));
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&staged_path)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            file.set_permissions(std::fs::Permissions::from_mode(0o500))?;
            drop(file);

            let sealed_tools = manifest
                .tools
                .iter()
                .map(|tool| {
                    let grant = roots
                        .iter()
                        .find(|grant| grant.name == tool.root)
                        .ok_or_else(|| {
                            Error::new(ErrorCode::PolicyDenied, "Driver tool grant disappeared")
                        })?;
                    seal_verified_tool(&grant.path, &tool.sha256, &tool.name)
                })
                .collect::<Result<Vec<_>>>()?;
            let loopback = match manifest.loopback_port {
                Some(port) => Some(loopback::start(state, port).await?),
                None => None,
            };
            let spec = sandbox_spec(
                &manifest,
                &staged_path,
                helper,
                roots,
                loopback.as_deref().map(loopback::LoopbackProxy::directory),
                &sealed_tools,
            )?;
            let mut child = semwright_platform_services::sandbox_spawn(&spec)?;
            let process_id = child
                .id()
                .ok_or_else(|| Error::new(ErrorCode::Internal, "Driver child has no PID"))?;
            let cpu_accounting = child.cpu_accounting();
            #[cfg(target_os = "windows")]
            if manifest.resources.operation_cpu_seconds != 0 && cpu_accounting.is_none() {
                let _ = child.kill().await;
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "Windows Driver Host requires Job CPU accounting for per-operation budgets",
                ));
            }
            let input = child.take_stdin()?;
            let output = child.take_stdout()?;
            let mut io = Io { input, output };
            let timeout = Duration::from_millis(manifest.request_timeout_ms.min(30_000));
            let hello = request(
                &mut io,
                &Request::Hello {
                    protocol: manifest.protocol,
                    provider: identity.clone(),
                    executable_sha256: manifest.sha256.to_ascii_lowercase(),
                },
                timeout,
            )
            .await?;
            match hello {
                Response::Ready {
                    protocol,
                    id,
                    version,
                } if protocol == manifest.protocol
                    && id == manifest.id
                    && version == manifest.version => {}
                _ => {
                    let _ = child.kill().await;
                    return Err(Error::new(
                        ErrorCode::ProtocolMismatch,
                        "Driver handshake did not attest the expected identity/version",
                    ));
                }
            }

            if manifest.protocol >= 2 {
                let interfaces_id = unique_id();
                match request(
                    &mut io,
                    &Request::Interfaces {
                        id: interfaces_id.clone(),
                    },
                    timeout,
                )
                .await?
                {
                    Response::Interfaces { id, interfaces }
                        if id == interfaces_id && interfaces == manifest.interfaces => {}
                    _ => {
                        let _ = child.kill().await;
                        return Err(Error::new(
                            ErrorCode::ProtocolMismatch,
                            "Driver interface negotiation did not match the owner manifest",
                        ));
                    }
                }
            }

            let id = unique_id();
            let response =
                request(&mut io, &Request::Capabilities { id: id.clone() }, timeout).await?;
            let capabilities = match response {
                Response::Capabilities {
                    id: response_id,
                    capabilities,
                    digest,
                } if response_id == id && digest == capabilities_digest(&capabilities)? => {
                    capabilities
                }
                _ => {
                    let _ = child.kill().await;
                    return Err(Error::new(
                        ErrorCode::ProtocolMismatch,
                        "Driver capability attestation mismatch",
                    ));
                }
            };
            if capabilities.is_empty() || capabilities.len() > 2048 {
                let _ = child.kill().await;
                return Err(Error::invalid(
                    "Driver returned an invalid capability count",
                ));
            }
            let mut provided = Vec::with_capacity(capabilities.len());
            let mut digests = BTreeMap::new();
            for capability in capabilities {
                capability.validate_for(&identity)?;
                let digest = descriptor_digest(&capability.descriptor)?;
                if digests
                    .insert(capability.descriptor.name.clone(), digest)
                    .is_some()
                {
                    let _ = child.kill().await;
                    return Err(Error::new(
                        ErrorCode::Conflict,
                        "Driver returned duplicate capability names",
                    ));
                }
                provided.push(ProvidedCapability {
                    descriptor: capability.descriptor,
                    aliases: capability.aliases,
                    tags: capability.tags,
                    object_types: capability.object_types,
                });
            }

            let health_id = unique_id();
            match request(
                &mut io,
                &Request::Health {
                    id: health_id.clone(),
                },
                timeout,
            )
            .await?
            {
                Response::Healthy { id, .. } if id == health_id => {}
                _ => {
                    let _ = child.kill().await;
                    return Err(Error::new(
                        ErrorCode::ProtocolMismatch,
                        "Driver health handshake failed",
                    ));
                }
            }

            let closed = CancellationToken::new();
            let terminate = CancellationToken::new();
            let interfaces = if manifest.protocol >= 2 {
                provider_interfaces(manifest.interfaces)
            } else {
                ProviderInterfaces {
                    health: manifest.interfaces.health,
                    ..Default::default()
                }
            };
            let (protocol_io, signals) = if manifest.protocol >= 2 {
                let (signals, _) = broadcast::channel(128);
                let pending = Arc::new(Mutex::new(BTreeMap::new()));
                let Io { input, output } = io;
                spawn_v2_reader(
                    output,
                    pending.clone(),
                    signals.clone(),
                    manifest.interfaces,
                    closed.clone(),
                    terminate.clone(),
                );
                (
                    ProtocolIo::V2(V2Io {
                        input: Mutex::new(input),
                        pending,
                    }),
                    Some(signals),
                )
            } else {
                (ProtocolIo::V1(Mutex::new(io)), None)
            };
            let monitor_closed = closed.clone();
            let monitor_terminate = terminate.clone();
            let monitor_staged = staged.clone();
            let monitor_loopback = loopback.clone();
            tokio::spawn(async move {
                tokio::select! {
                    _ = monitor_terminate.cancelled() => {
                        let _ = child.kill().await;
                    }
                    _ = child.wait() => {}
                }
                if let Some(proxy) = monitor_loopback {
                    proxy.shutdown();
                }
                monitor_closed.cancel();
                drop(monitor_staged);
            });

            Ok(Arc::new(Self {
                identity,
                manifest,
                capabilities: StdRwLock::new(provided),
                descriptor_digests: StdRwLock::new(digests),
                io: protocol_io,
                signals,
                interfaces,
                closed,
                terminate,
                process_id,
                cpu_accounting,
                operation_cpu_gate: Mutex::new(()),
                _staged: staged,
                _loopback: loopback,
                _tools: sealed_tools,
            }))
        }
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    async fn exchange(
        &self,
        request_value: Request,
        id: &str,
        timeout: Duration,
    ) -> Result<Response> {
        match &self.io {
            ProtocolIo::V1(io) => {
                let mut io = io.lock().await;
                request(&mut io, &request_value, timeout).await
            }
            ProtocolIo::V2(io) => io.request(&request_value, id, timeout).await,
        }
    }

    fn decode_capabilities(
        &self,
        response: Response,
        id: &str,
    ) -> Result<(Vec<ProvidedCapability>, BTreeMap<String, String>)> {
        let capabilities = match response {
            Response::Capabilities {
                id: response_id,
                capabilities,
                digest,
            } if response_id == id && digest == capabilities_digest(&capabilities)? => capabilities,
            Response::Failure {
                id: response_id,
                error,
            } if response_id == id => {
                return Err(Error::new(
                    error.code,
                    "Driver capability refresh failed; child details were redacted",
                ));
            }
            _ => {
                return Err(Error::new(
                    ErrorCode::ProtocolMismatch,
                    "Driver capability response did not match its request",
                ));
            }
        };
        if capabilities.is_empty() || capabilities.len() > 2048 {
            return Err(Error::invalid(
                "Driver returned an invalid capability count",
            ));
        }
        let mut provided = Vec::with_capacity(capabilities.len());
        let mut digests = BTreeMap::new();
        for capability in capabilities {
            capability.validate_for(&self.identity)?;
            let digest = descriptor_digest(&capability.descriptor)?;
            if digests
                .insert(capability.descriptor.name.clone(), digest)
                .is_some()
            {
                return Err(Error::new(
                    ErrorCode::Conflict,
                    "Driver returned duplicate capability names",
                ));
            }
            provided.push(ProvidedCapability {
                descriptor: capability.descriptor,
                aliases: capability.aliases,
                tags: capability.tags,
                object_types: capability.object_types,
            });
        }
        Ok((provided, digests))
    }

    async fn refresh_capabilities(&self) -> Result<Vec<ProvidedCapability>> {
        let id = unique_id();
        let response = self
            .exchange(
                Request::Capabilities { id: id.clone() },
                &id,
                Duration::from_millis(self.manifest.request_timeout_ms.min(10_000)),
            )
            .await?;
        let (provided, digests) = self.decode_capabilities(response, &id)?;
        *self
            .capabilities
            .write()
            .map_err(|_| Error::new(ErrorCode::Internal, "Driver capability cache poisoned"))? =
            provided.clone();
        *self
            .descriptor_digests
            .write()
            .map_err(|_| Error::new(ErrorCode::Internal, "Driver digest cache poisoned"))? =
            digests;
        Ok(provided)
    }

    fn execution_result(response: Response, id: &str) -> Result<Value> {
        match response {
            Response::Result {
                id: response_id,
                value,
            } if response_id == id => Ok(value),
            Response::Failure {
                id: response_id,
                error,
            } if response_id == id => Err(Error::new(
                error.code,
                "Driver reported an error; untrusted payload was redacted",
            )
            .uncertain()),
            _ => Err(Error::new(
                ErrorCode::ProtocolMismatch,
                "Driver returned an unexpected execution response",
            )
            .uncertain()),
        }
    }
}

#[async_trait]
impl Provider for DriverProvider {
    fn identity(&self) -> &ProviderIdentity {
        &self.identity
    }
    fn supports(&self, command: &str) -> bool {
        self.descriptor_digests
            .read()
            .map(|digests| digests.contains_key(command))
            .unwrap_or(false)
    }
    async fn capabilities(&self) -> Result<Vec<ProvidedCapability>> {
        if self.interfaces.dynamic_capabilities {
            self.refresh_capabilities().await
        } else {
            self.capabilities
                .read()
                .map(|capabilities| capabilities.clone())
                .map_err(|_| Error::new(ErrorCode::Internal, "Driver capability cache poisoned"))
        }
    }
    async fn probe(&self) -> Vec<Feature> {
        let capabilities = self
            .capabilities
            .read()
            .map(|capabilities| capabilities.clone())
            .unwrap_or_default();
        capabilities
            .iter()
            .map(|capability| Feature {
                backend: self.identity.id.clone(),
                capability: capability.descriptor.name.clone(),
                status: if self.closed.is_cancelled() {
                    CapabilityStatus::Unavailable
                } else {
                    CapabilityStatus::Supported
                },
                reason: if self.closed.is_cancelled() {
                    "Driver process disconnected".into()
                } else {
                    "Pinned sandboxed driver is connected".into()
                },
                remediation: if self.closed.is_cancelled() {
                    "Restart the owner-configured driver".into()
                } else {
                    String::new()
                },
            })
            .collect()
    }
    fn operation_feature(&self, command: &str) -> Option<String> {
        self.supports(command).then(|| command.to_owned())
    }
    fn interfaces(&self) -> ProviderInterfaces {
        self.interfaces
    }
    fn events(&self) -> Option<broadcast::Receiver<ProviderSignal>> {
        self.signals.as_ref().map(broadcast::Sender::subscribe)
    }
    fn closed(&self) -> Option<CancellationToken> {
        Some(self.closed.clone())
    }
    async fn execute(
        &self,
        context: &Context,
        descriptor: &CommandDescriptor,
        args: &Value,
    ) -> Result<Value> {
        if self.closed.is_cancelled() {
            return Err(Error::unavailable("Driver process is disconnected"));
        }
        let expected = self
            .descriptor_digests
            .read()
            .map_err(|_| Error::new(ErrorCode::Internal, "Driver digest cache poisoned"))?
            .get(&descriptor.name)
            .cloned()
            .ok_or_else(|| {
                Error::new(ErrorCode::NotFound, "Driver capability is not registered")
            })?;
        let actual = descriptor_digest(descriptor)?;
        if actual != expected {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Pinned driver capability descriptor changed",
            ));
        }
        // CPU accounting is process-boundary-wide (Job Object on Windows and
        // process-tree accounting on Linux), not request-tagged. Serialize only Drivers that
        // opt into a per-operation budget so concurrent requests cannot charge each other.
        let _operation_cpu_guard = if self.manifest.resources.operation_cpu_seconds != 0 {
            Some(tokio::select! {
                guard = self.operation_cpu_gate.lock() => guard,
                _ = context.cancellation.cancelled() => {
                    return Err(Error::new(
                        ErrorCode::Cancelled,
                        "Driver execution cancelled while waiting for its CPU-budget turn",
                    ));
                }
            })
        } else {
            None
        };
        let id = if context.request_id.is_empty() {
            unique_id()
        } else {
            context.request_id.clone()
        };
        let timeout =
            Duration::from_millis(descriptor.timeout_ms.min(self.manifest.request_timeout_ms));
        let mut child_args = args.clone();
        let native_target = if self.interfaces.native_refs {
            child_args
                .as_object_mut()
                .and_then(|object| object.remove("_target"))
                .map(serde_json::from_value::<NativeTarget>)
                .transpose()?
        } else {
            None
        };
        let request_context = (self.manifest.protocol >= 3).then(|| DriverRequestContext {
            session: context.session.clone(),
            native_target,
        });
        let execute = Request::Execute {
            id: id.clone(),
            command: descriptor.name.clone(),
            descriptor_sha256: actual,
            args: child_args,
            context: request_context,
        };

        let response_future = async {
            match &self.io {
                ProtocolIo::V1(io) => {
                    let mut io = io.lock().await;
                    tokio::select! {
                        biased;
                        _ = context.cancellation.cancelled() => {
                            self.terminate.cancel();
                            Err(Error::new(
                                ErrorCode::Cancelled,
                                "Driver v1 execution cancelled by terminating its isolated process",
                            ).uncertain())
                        }
                        response = request(&mut io, &execute, timeout) => response
                    }
                }
                ProtocolIo::V2(io) => {
                    let receiver = io.begin(&execute, &id).await?;
                    let response_future = async {
                        match tokio::time::timeout(timeout, receiver).await {
                            Ok(Ok(response)) => Ok(response),
                            Ok(Err(_)) => Err(Error::unavailable("Driver response channel closed")),
                            Err(_) => {
                                io.pending.lock().await.remove(&id);
                                Err(Error::new(ErrorCode::Timeout, "Driver execution timed out"))
                            }
                        }
                    };
                    tokio::pin!(response_future);
                    if self.interfaces.cooperative_cancellation {
                        tokio::select! {
                            response = &mut response_future => response,
                            _ = context.cancellation.cancelled() => {
                                let cancel_id = unique_id();
                                let ack = io.request(
                                    &Request::Cancel {
                                        id: cancel_id.clone(),
                                        target: id.clone(),
                                    },
                                    &cancel_id,
                                    Duration::from_secs(2),
                                ).await?;
                                match ack {
                                    Response::Cancelled { id: response_id, target, .. }
                                        if response_id == cancel_id && target == id => {}
                                    _ => {
                                        self.terminate.cancel();
                                        return Err(Error::new(
                                            ErrorCode::ProtocolMismatch,
                                            "Driver cancellation acknowledgement mismatch",
                                        ).uncertain());
                                    }
                                }
                                match tokio::time::timeout(Duration::from_secs(3), &mut response_future).await {
                                    Ok(response) => response,
                                    Err(_) => {
                                        self.terminate.cancel();
                                        Err(Error::new(
                                            ErrorCode::Cancelled,
                                            "Driver accepted cancellation but did not terminate the request",
                                        ).uncertain())
                                    }
                                }
                            }
                        }
                    } else {
                        tokio::select! {
                            response = &mut response_future => response,
                            _ = context.cancellation.cancelled() => {
                                self.terminate.cancel();
                                Err(Error::new(
                                    ErrorCode::Cancelled,
                                    "Driver execution cancelled; child has no cooperative cancellation contract",
                                ).uncertain())
                            }
                        }
                    }
                }
            }
        };
        tokio::pin!(response_future);
        let cpu_watch = wait_for_operation_cpu_budget(
            self.process_id,
            self.cpu_accounting.clone(),
            self.manifest.resources.operation_cpu_seconds,
        );
        tokio::pin!(cpu_watch);
        let response = tokio::select! {
            biased;
            budget = &mut cpu_watch => {
                if let ProtocolIo::V2(io) = &self.io {
                    io.pending.lock().await.remove(&id);
                }
                self.terminate.cancel();
                let terminated = tokio::time::timeout(
                    Duration::from_secs(2),
                    self.closed.cancelled(),
                )
                .await
                .is_ok();
                match budget {
                    Ok(()) if terminated => {
                        return Err(Error::new(
                            ErrorCode::ResourceExhausted,
                            "Driver exceeded its per-operation CPU budget",
                        ).uncertain());
                    }
                    Ok(()) => {
                        return Err(Error::new(
                            ErrorCode::ResourceExhausted,
                            "Driver exceeded its per-operation CPU budget and termination did not complete",
                        ).uncertain());
                    }
                    Err(error) => return Err(error.uncertain()),
                }
            }
            response = &mut response_future => response,
        };

        match response {
            Ok(response) => Self::execution_result(response, &id),
            Err(error) => {
                self.terminate.cancel();
                Err(error.uncertain())
            }
        }
    }
    fn emits_native_refs(&self) -> bool {
        self.interfaces.native_refs
    }
    async fn validate(&self, target: &NativeTarget) -> Result<()> {
        if !self.interfaces.native_refs || self.manifest.protocol < 3 {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "Driver did not negotiate native reference validation",
            ));
        }
        let id = unique_id();
        let timeout = Duration::from_millis(self.manifest.request_timeout_ms.min(5_000));
        let request = Request::Validate {
            id: id.clone(),
            target: target.clone(),
        };
        let response = match &self.io {
            ProtocolIo::V1(_) => {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "Driver protocol v1 cannot validate native references",
                ));
            }
            ProtocolIo::V2(io) => io.request(&request, &id, timeout).await?,
        };
        match response {
            Response::Validated { id: response_id } if response_id == id => Ok(()),
            Response::Failure {
                id: response_id,
                error,
            } if response_id == id => Err(Error::new(
                error.code,
                "Driver rejected native reference; child details were redacted",
            )),
            _ => Err(Error::new(
                ErrorCode::ProtocolMismatch,
                "Driver native-reference validation response mismatch",
            )),
        }
    }
    async fn shutdown(&self) -> Result<()> {
        if self.closed.is_cancelled() {
            return Ok(());
        }
        let id = unique_id();
        let response = self
            .exchange(
                Request::Shutdown { id: id.clone() },
                &id,
                Duration::from_secs(3),
            )
            .await?;
        if !matches!(response, Response::Shutdown { id: response_id } if response_id == id) {
            return Err(Error::new(
                ErrorCode::ProtocolMismatch,
                "Driver shutdown acknowledgement mismatch",
            ));
        }
        if tokio::time::timeout(Duration::from_secs(3), self.closed.cancelled())
            .await
            .is_err()
        {
            self.terminate.cancel();
            tokio::time::timeout(Duration::from_secs(2), self.closed.cancelled())
                .await
                .map_err(|_| Error::new(ErrorCode::Timeout, "Driver could not be terminated"))?;
            return Err(Error::new(
                ErrorCode::Timeout,
                "Driver did not exit after shutdown acknowledgement",
            ));
        }
        Ok(())
    }
}

impl Drop for DriverProvider {
    fn drop(&mut self) {
        self.terminate.cancel();
    }
}

#[derive(Debug, Serialize)]
pub struct ConformanceReport {
    pub provider: String,
    pub namespace: String,
    pub version: String,
    pub capabilities: usize,
    pub sandboxed: bool,
    pub persistent_process: bool,
    pub health: bool,
    pub executed_read_only: bool,
    pub shutdown: bool,
}

pub async fn conformance(
    manifest: Manifest,
    state: &Path,
    helper: &Path,
    roots: &[FilesystemGrant],
    allow_network: bool,
) -> Result<ConformanceReport> {
    let provider = DriverProvider::connect(manifest, state, helper, roots, allow_network).await?;
    let capabilities = Provider::capabilities(provider.as_ref()).await?;
    let provider_id = provider.identity.id.clone();
    let namespace = provider.identity.namespace.clone();
    let version = provider.identity.version.clone();
    let safe = capabilities.iter().find(|capability| {
        capability.descriptor.risk == semwright_types::Risk::ReadOnly
            && capability.descriptor.dry_run
            && capability
                .descriptor
                .input_schema
                .get("type")
                .and_then(Value::as_str)
                == Some("object")
            && capability
                .descriptor
                .input_schema
                .get("required")
                .and_then(Value::as_array)
                .is_none_or(Vec::is_empty)
    });
    let executed_read_only = if let Some(capability) = safe {
        let context = Context {
            session: "driver-conformance".into(),
            request_id: semwright_types::unique_id(),
            cancellation: CancellationToken::new(),
        };
        Provider::execute(
            provider.as_ref(),
            &context,
            &capability.descriptor,
            &json!({}),
        )
        .await?;
        true
    } else {
        false
    };
    Provider::shutdown(provider.as_ref()).await?;
    Ok(ConformanceReport {
        provider: provider_id,
        namespace,
        version,
        capabilities: capabilities.len(),
        sandboxed: true,
        persistent_process: true,
        health: true,
        executed_read_only,
        shutdown: true,
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    fn manifest() -> Manifest {
        Manifest {
            manifest_version: 1,
            protocol: 1,
            id: "fixture".into(),
            version: "1".into(),
            publisher: "tests".into(),
            executable: "/tmp/fixture".into(),
            sha256: "a".repeat(64),
            application: semwright_driver_sdk::ApplicationMatch {
                desktop_id: Some("org.example.Fixture".into()),
                ..Default::default()
            },
            transport: semwright_driver_sdk::Transport::StdioV1,
            mounts: vec![],
            system_config: vec![],
            secrets: vec![],
            tools: vec![],
            network: false,
            loopback_port: None,
            resources: semwright_driver_sdk::DriverResources::default(),
            request_timeout_ms: 1000,
            interfaces: semwright_driver_sdk::DriverInterfaces::default(),
        }
    }

    #[test]
    fn owner_network_consent_and_interface_negotiation_fail_closed() {
        let mut network = manifest();
        network.network = true;
        assert!(matches!(
            validate_owner_permissions(&network, &[], false),
            Err(error) if error.code == ErrorCode::PolicyDenied
        ));
        assert!(validate_owner_permissions(&network, &[], true).is_ok());

        let mut dynamic = manifest();
        dynamic.interfaces.dynamic_capabilities = true;
        assert!(matches!(
            validate_owner_permissions(&dynamic, &[], false),
            Err(error) if error.code == ErrorCode::Unsupported
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn sealed_tool_memfd_is_write_sealed() {
        let source = Path::new("/usr/bin/true");
        let digest = format!("{:x}", Sha256::digest(std::fs::read(source).unwrap()));
        let tool = seal_verified_tool(source, &digest, "probe").unwrap();
        let fd = tool._file.as_raw_fd();
        let data_fd = tool.data_file.as_raw_fd();
        // SAFETY: F_GETFD reads scalar flags from the live sealed memfd descriptor.
        let owner_flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        assert!(owner_flags >= 0);
        assert_ne!(owner_flags & libc::FD_CLOEXEC, 0);
        let owner_metadata = tool._file.metadata().unwrap();
        let data_metadata = tool.data_file.metadata().unwrap();
        assert_eq!(owner_metadata.dev(), data_metadata.dev());
        assert_eq!(owner_metadata.ino(), data_metadata.ino());
        assert_eq!(data_metadata.permissions().mode() & 0o777, 0o500);
        // SAFETY: F_GETFD reads scalar descriptor flags from the live read-only descriptor.
        let data_fd_flags = unsafe { libc::fcntl(data_fd, libc::F_GETFD) };
        // SAFETY: F_GETFL reads scalar file status flags from the live read-only descriptor.
        let data_status_flags = unsafe { libc::fcntl(data_fd, libc::F_GETFL) };
        assert!(data_fd_flags >= 0);
        assert!(data_status_flags >= 0);
        assert_eq!(data_fd_flags & libc::FD_CLOEXEC, 0);
        assert_eq!(data_status_flags & libc::O_ACCMODE, libc::O_RDONLY);
        let required =
            libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL;
        // SAFETY: fd is a live memfd retained by tool.
        let actual = unsafe { libc::fcntl(fd, libc::F_GET_SEALS) };
        assert_eq!(actual & required, required);

        let byte = b"x";
        // SAFETY: pwrite reads one byte from a live static buffer and targets the live memfd.
        let written = unsafe { libc::pwrite(fd, byte.as_ptr().cast(), 1, 0) };
        assert_eq!(written, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::EPERM)
        );
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn operation_cpu_watchdog_counts_busy_descendants_from_worker_threads() {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (stop_tx, stop_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let mut child = std::process::Command::new("/usr/bin/yes")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap();
            ready_tx.send(child.id()).unwrap();
            let _ = stop_rx.recv_timeout(Duration::from_secs(5));
            let _ = child.kill();
            let _ = child.wait();
        });

        let child_pid = ready_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("worker-thread child did not start");
        let root = std::process::id();
        assert!(
            process_children(root).unwrap().contains(&child_pid),
            "child spawned by a secondary thread must remain visible to CPU accounting"
        );

        tokio::time::timeout(
            Duration::from_secs(5),
            wait_for_operation_cpu_budget(root, None, 1),
        )
        .await
        .expect("CPU watchdog did not fire")
        .expect("CPU accounting failed");

        let _ = stop_tx.send(());
        worker.join().unwrap();
    }

    #[test]
    fn system_config_requires_an_explicit_readable_owner_grant() {
        let dir = tempfile::tempdir().unwrap();
        let canonical = std::fs::canonicalize(dir.path()).unwrap();
        let mut candidate = manifest();
        candidate.system_config = vec![semwright_driver_sdk::SystemConfigMount {
            root: "app-config".into(),
            destination: "/etc/example-app".into(),
        }];

        assert!(matches!(
            validate_owner_permissions(&candidate, &[], false),
            Err(error) if error.code == ErrorCode::PolicyDenied
        ));

        let unreadable = FilesystemGrant {
            name: "app-config".into(),
            path: canonical.clone(),
            read: false,
            write: false,
        };
        assert!(matches!(
            validate_owner_permissions(&candidate, &[unreadable], false),
            Err(error) if error.code == ErrorCode::PolicyDenied
        ));

        let readable = FilesystemGrant {
            name: "app-config".into(),
            path: canonical,
            read: true,
            write: false,
        };
        validate_owner_permissions(&candidate, &[readable], false).unwrap();
    }

    #[test]
    fn secret_mount_requires_private_small_regular_owner_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let secret_path = dir.path().join("pairing");
        std::fs::write(&secret_path, b"0123456789abcdef").unwrap();
        std::fs::set_permissions(&secret_path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let secret_path = std::fs::canonicalize(secret_path).unwrap();

        let mut candidate = manifest();
        candidate.secrets = vec![semwright_driver_sdk::DriverSecretMount {
            root: "pairing-secret".into(),
            name: "pairing".into(),
        }];

        assert!(matches!(
            validate_owner_permissions(&candidate, &[], false),
            Err(error) if error.code == ErrorCode::PolicyDenied
        ));

        let unreadable = FilesystemGrant {
            name: "pairing-secret".into(),
            path: secret_path.clone(),
            read: false,
            write: false,
        };
        assert!(matches!(
            validate_owner_permissions(&candidate, &[unreadable], false),
            Err(error) if error.code == ErrorCode::PolicyDenied
        ));

        let readable = FilesystemGrant {
            name: "pairing-secret".into(),
            path: secret_path.clone(),
            read: true,
            write: false,
        };
        validate_owner_permissions(&candidate, std::slice::from_ref(&readable), false).unwrap();

        std::fs::set_permissions(&secret_path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(
            validate_owner_permissions(&candidate, &[readable], false),
            Err(error) if error.code == ErrorCode::PermissionDenied
        ));
    }

    #[test]
    fn sandbox_includes_read_only_system_alternatives_when_available() {
        if !Path::new("/etc/alternatives").is_dir() || !Path::new("/usr/bin/bwrap").is_file() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let staged = dir.path().join("driver");
        let helper = dir.path().join("sandbox");
        std::fs::write(&staged, b"driver").unwrap();
        std::fs::write(&helper, b"helper").unwrap();

        let command = sandbox_command(&manifest(), &staged, &helper, &[], None, &[]).unwrap();
        let args: Vec<_> = command
            .as_std()
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(
            args.windows(3).any(|window| {
                window
                    == [
                        "--ro-bind".to_owned(),
                        "/etc/alternatives".to_owned(),
                        "/etc/alternatives".to_owned(),
                    ]
            }),
            "sandbox must preserve read-only update-alternatives symlink resolution"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn sealed_tools_are_materialized_as_private_files_not_deleted_bind_data() {
        if !Path::new("/usr/bin/bwrap").is_file() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let staged = dir.path().join("driver");
        let helper = dir.path().join("sandbox");
        std::fs::write(&staged, b"driver").unwrap();
        std::fs::write(&helper, b"helper").unwrap();

        let source = Path::new("/usr/bin/true");
        let digest = format!("{:x}", Sha256::digest(std::fs::read(source).unwrap()));
        let tool = seal_verified_tool(source, &digest, "probe").unwrap();
        let command = sandbox_command(&manifest(), &staged, &helper, &[], None, &[tool]).unwrap();
        let args = command
            .as_std()
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert!(
            args.windows(5).any(|window| {
                window[0] == "--perms"
                    && window[1] == "0500"
                    && window[2] == "--file"
                    && window[4] == "/plugin/tools/probe"
            }),
            "sealed tool must be copied from the sealed Host fd into the private sandbox root"
        );
        assert!(
            !args.iter().any(|arg| arg == "--ro-bind-data"),
            "deleted backing bind-data must not be used for executable sealed tools"
        );
    }

    #[test]
    fn writable_or_wrong_digest_executable_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("driver");
        let body = b"ELFfixture";
        std::fs::write(&path, body).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o660)).unwrap();
        assert!(verify_owned_executable(&path, &format!("{:x}", Sha256::digest(body))).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(verify_owned_executable(&path, &"0".repeat(64)).is_err());
    }
}
