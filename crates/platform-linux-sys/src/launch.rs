use async_trait::async_trait;
use semwright_platform_api::launch::WorkspaceMountBinding;
use semwright_platform_api::launch::{
    ExecutableVerifier, MaterializedMount, MaterializedTool, Mount, MountClass,
    SANDBOX_HOST_TOOL_CHILD_ENV, SANDBOX_HOST_TOOL_CWD_ENV, SANDBOX_HOST_TOOL_TYPED_ARGS_ENV,
    SANDBOX_MOUNTS_ENV, SANDBOX_TOOLS_ENV, SandboxChildControl, SandboxKind, SandboxLauncher,
    SandboxProcess, SandboxSpec, SandboxStderrObserver, SandboxStderrTerminal, SandboxStdin,
    SandboxStdout, SealedToolSource, encode_materialized_mounts, encode_materialized_tools,
    resolve_host_tool_args,
};
use semwright_types::{Error, ErrorCode, Result};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::{
    io::Read,
    os::{
        fd::{AsFd, AsRawFd, FromRawFd, OwnedFd},
        unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt},
    },
    path::Path,
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::process::{Child, Command};

const MAX_PROVIDER_EXECUTABLE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SEALED_TOOL_EXECUTABLE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_APPLICATION_EXECUTABLE_BYTES: u64 = 512 * 1024 * 1024;
const SANDBOX_BWRAP_INFO_FD_ENV: &str = "SEMWRIGHT_INTERNAL_BWRAP_INFO_FD";
const MAX_BWRAP_INFO_BYTES: usize = 16 * 1024;
const NVIDIA_PROC_PATH: &str = "/proc/driver/nvidia";

fn is_root_owned_character_device(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_char_device() && metadata.uid() == 0
}

fn validated_nvidia_compute_sources() -> Result<(Vec<(&'static str, std::fs::File)>, std::fs::File)>
{
    use semwright_platform_api::launch::NVIDIA_COMPUTE_DEVICE_PATHS;
    let mut sources = Vec::new();
    for (index, path) in NVIDIA_COMPUTE_DEVICE_PATHS.iter().enumerate() {
        let opened = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path);
        match opened {
            Ok(file) if is_root_owned_character_device(&file.metadata()?) => {
                sources.push((*path, file))
            }
            Err(error) if index == 3 && error.kind() == std::io::ErrorKind::NotFound => {}
            _ => {
                return Err(Error::new(
                    ErrorCode::SandboxDenied,
                    "NVIDIA compute requires fixed root-owned character device nodes without symlinks",
                ));
            }
        }
    }
    let proc = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(NVIDIA_PROC_PATH)?;
    let metadata = proc.metadata()?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() || metadata.uid() != 0 {
        return Err(Error::new(
            ErrorCode::SandboxDenied,
            "NVIDIA compute proc metadata must be a root-owned directory",
        ));
    }
    Ok((sources, proc))
}
const BWRAP_INFO_TIMEOUT: Duration = Duration::from_secs(5);

fn observe_stderr(
    observer: &dyn SandboxStderrObserver,
    event: impl FnOnce(&dyn SandboxStderrObserver),
) -> bool {
    // An owner observer failure must not close the child's pipe or replace a
    // native error. The drain continues even if a callback panics.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| event(observer))).is_ok()
}

struct StderrDrainTerminal {
    observer: Arc<dyn SandboxStderrObserver>,
    finished: bool,
    observer_failed: bool,
}

impl StderrDrainTerminal {
    fn finish(&mut self, terminal: SandboxStderrTerminal) {
        self.finished = true;
        let terminal = if self.observer_failed {
            SandboxStderrTerminal::Interrupted
        } else {
            terminal
        };
        observe_stderr(self.observer.as_ref(), |observer| {
            observer.terminal(terminal)
        });
    }
}

impl Drop for StderrDrainTerminal {
    fn drop(&mut self) {
        if !self.finished {
            observe_stderr(self.observer.as_ref(), |observer| {
                observer.terminal(SandboxStderrTerminal::Interrupted);
            });
        }
    }
}

fn private_stderr_pipe(observer: Arc<dyn SandboxStderrObserver>) -> Result<Stdio> {
    let mut fds = [-1i32; 2];
    // SAFETY: pipe2 initializes both descriptors on success. Only the pipe
    // writer is installed as child stderr; no custody-file descriptor is shared.
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: both descriptors were newly returned by pipe2.
    let mut reader = unsafe { std::fs::File::from_raw_fd(fds[0]) };
    // SAFETY: this is the distinct newly-owned writer returned by pipe2.
    let writer = unsafe { OwnedFd::from_raw_fd(fds[1]) };
    let mut terminal = StderrDrainTerminal {
        observer,
        finished: false,
        observer_failed: false,
    };
    let thread = std::thread::Builder::new()
        .name("semwright-startup-stderr".into())
        .spawn(move || {
            terminal.observer_failed =
                !observe_stderr(terminal.observer.as_ref(), |observer| observer.started());
            let mut chunk = [0u8; 8192];
            loop {
                match reader.read(&mut chunk) {
                    Ok(0) => {
                        terminal.finish(SandboxStderrTerminal::Eof);
                        break;
                    }
                    Ok(count) => {
                        if !terminal.observer_failed {
                            terminal.observer_failed =
                                !observe_stderr(terminal.observer.as_ref(), |observer| {
                                    observer.bytes(&chunk[..count]);
                                });
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) => {
                        terminal.finish(SandboxStderrTerminal::ReadFailed {
                            kind: error.kind(),
                            raw_os_error: error.raw_os_error(),
                        });
                        break;
                    }
                }
            }
        })
        .map_err(|_| Error::unavailable("Private stderr drain unavailable"))?;
    // This independent thread already runs before synchronous Bubblewrap
    // metadata collection. It owns the reader until EOF, including post-Hello
    // writes and cleanup. Dropping this handle never cancels the drain.
    drop(thread);
    Ok(Stdio::from(writer))
}

fn parse_bwrap_child_pid(bytes: &[u8]) -> Result<u32> {
    if bytes.is_empty() || bytes.len() > MAX_BWRAP_INFO_BYTES {
        return Err(Error::new(
            ErrorCode::SandboxDenied,
            "Bubblewrap child metadata is missing or exceeds its bound",
        ));
    }
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| {
        Error::new(
            ErrorCode::SandboxDenied,
            "Bubblewrap child metadata is not valid JSON",
        )
    })?;
    let pid = value
        .get("child-pid")
        .and_then(serde_json::Value::as_u64)
        .filter(|pid| (1..=i32::MAX as u64).contains(pid))
        .ok_or_else(|| {
            Error::new(
                ErrorCode::SandboxDenied,
                "Bubblewrap child metadata lacks a valid host child PID",
            )
        })?;
    Ok(pid as u32)
}

fn bwrap_info_pipe() -> Result<(OwnedFd, OwnedFd)> {
    let mut fds = [-1i32; 2];
    // SAFETY: fds points to two writable integers and pipe2 initializes both on success.
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: pipe2 returned two newly-owned descriptors.
    let read_fd = unsafe { OwnedFd::from_raw_fd(fds[0]) };
    // SAFETY: pipe2 returned two newly-owned descriptors.
    let write_fd = unsafe { OwnedFd::from_raw_fd(fds[1]) };

    // Keep only the Bubblewrap writer inheritable across exec.
    // SAFETY: F_GETFD reads scalar descriptor flags only.
    let write_flags = unsafe { libc::fcntl(write_fd.as_raw_fd(), libc::F_GETFD) };
    if write_flags < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: F_SETFD writes scalar descriptor flags only.
    let set_write_flags = unsafe {
        libc::fcntl(
            write_fd.as_raw_fd(),
            libc::F_SETFD,
            write_flags & !libc::FD_CLOEXEC,
        )
    };
    if set_write_flags != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // Make the parent read side nonblocking so startup remains bounded if Bubblewrap fails.
    // SAFETY: F_GETFL reads scalar status flags only.
    let read_flags = unsafe { libc::fcntl(read_fd.as_raw_fd(), libc::F_GETFL) };
    if read_flags < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: F_SETFL writes scalar status flags only.
    let set_read_flags = unsafe {
        libc::fcntl(
            read_fd.as_raw_fd(),
            libc::F_SETFL,
            read_flags | libc::O_NONBLOCK,
        )
    };
    if set_read_flags != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok((read_fd, write_fd))
}

fn read_bwrap_child_pid(read_fd: &OwnedFd, monitor: &mut Child) -> Result<u32> {
    let deadline = Instant::now() + BWRAP_INFO_TIMEOUT;
    let mut bytes = Vec::new();
    loop {
        let mut chunk = [0u8; 4096];
        // SAFETY: chunk is live writable storage and read_fd is an owned readable pipe endpoint.
        let read =
            unsafe { libc::read(read_fd.as_raw_fd(), chunk.as_mut_ptr().cast(), chunk.len()) };
        if read > 0 {
            bytes.extend_from_slice(&chunk[..read as usize]);
            if bytes.len() > MAX_BWRAP_INFO_BYTES {
                return Err(Error::new(
                    ErrorCode::SandboxDenied,
                    "Bubblewrap child metadata exceeds its bound",
                ));
            }
            if let Ok(pid) = parse_bwrap_child_pid(&bytes) {
                return Ok(pid);
            }
        } else if read == 0 {
            return parse_bwrap_child_pid(&bytes);
        } else {
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::WouldBlock {
                return Err(error.into());
            }
        }

        if monitor.try_wait()?.is_some() {
            return Err(Error::new(
                ErrorCode::SandboxDenied,
                "Bubblewrap exited before reporting the sandbox child PID",
            ));
        }
        if Instant::now() >= deadline {
            let _ = monitor.start_kill();
            return Err(Error::new(
                ErrorCode::Timeout,
                "Bubblewrap did not report the sandbox child PID in time",
            ));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn duplicate_pidfd(pidfd: &OwnedFd) -> Result<OwnedFd> {
    // SAFETY: F_DUPFD_CLOEXEC duplicates one live descriptor using scalar arguments only.
    let fd = unsafe { libc::fcntl(pidfd.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 3) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: fcntl returned a new owned descriptor on success.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn wait_pidfd_exit(pidfd: &OwnedFd, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(Error::new(
                ErrorCode::Timeout,
                "Linux sandbox child did not terminate after Host signal",
            ));
        }
        let timeout_ms = i32::try_from(remaining.as_millis())
            .unwrap_or(i32::MAX)
            .max(1);
        let mut pollfd = libc::pollfd {
            fd: pidfd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: pollfd points to one live pollfd entry for the duration of the call.
        let result = unsafe { libc::poll(&mut pollfd, 1, timeout_ms) };
        if result > 0 {
            if pollfd.revents & libc::POLLIN != 0 {
                return Ok(());
            }
            if pollfd.revents & (libc::POLLERR | libc::POLLNVAL) != 0 {
                return Err(Error::new(
                    ErrorCode::SandboxDenied,
                    "Linux sandbox child pidfd became invalid while reaping",
                ));
            }
            continue;
        }
        if result == 0 {
            return Err(Error::new(
                ErrorCode::Timeout,
                "Linux sandbox child did not terminate after Host signal",
            ));
        }
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::Interrupted {
            return Err(error.into());
        }
    }
}

fn open_pidfd(pid: u32) -> Result<Option<OwnedFd>> {
    // SAFETY: pidfd_open takes a numeric PID and flags only.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    if fd < 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            // Bubblewrap may report a very short-lived child that exits before Host opens
            // its pidfd. Treat that as an already-terminal child, not a sandbox denial.
            return Ok(None);
        }
        return Err(Error::new(
            ErrorCode::SandboxDenied,
            "pidfd_open is required for Linux sandbox child lifecycle safety",
        ));
    }
    // SAFETY: successful pidfd_open returns a newly-owned descriptor.
    Ok(Some(unsafe { OwnedFd::from_raw_fd(fd as i32) }))
}

fn require_live_mount_process(pidfd: &OwnedFd) -> Result<()> {
    let mut entry = libc::pollfd {
        fd: pidfd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: one initialized entry and a live owned pidfd; nonblocking poll.
    let result = unsafe { libc::poll(&mut entry, 1, 0) };
    if result == 0 {
        Ok(())
    } else {
        Err(Error::new(
            ErrorCode::StaleReference,
            "The sandbox instance for this workspace binding is no longer live",
        ))
    }
}

struct LinuxWorkspaceMountBinding {
    mount: Mount,
    mounted_root: std::fs::File,
    child_root_path: std::path::PathBuf,
    child_pidfd: OwnedFd,
}
impl LinuxWorkspaceMountBinding {
    fn open_instance(path: &Path) -> Result<std::fs::File> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)?;
        let metadata = file.metadata()?;
        if metadata.file_type().is_symlink() || (!metadata.is_dir() && !metadata.is_file()) {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Workspace binding requires a regular file or directory instance",
            ));
        }
        Ok(file)
    }
    fn capture(mount: &Mount, child_pid: u32, child_pidfd: &OwnedFd) -> Result<Self> {
        mount.validate()?;
        if mount.class != MountClass::Workspace {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Only workspace mounts can be exposed through this binding",
            ));
        }
        require_live_mount_process(child_pidfd)?;
        let destination = materialized_destination(mount)?;
        let relative = Path::new(&destination).strip_prefix("/").map_err(|_| {
            Error::new(
                ErrorCode::Internal,
                "Sandbox workspace destination is not absolute",
            )
        })?;
        let child_root_path =
            std::path::PathBuf::from(format!("/proc/{child_pid}/root")).join(relative);
        let mounted_root = Self::open_instance(&child_root_path)?;
        let binding = Self {
            mount: mount.clone(),
            mounted_root,
            child_root_path,
            child_pidfd: duplicate_pidfd(child_pidfd)?,
        };
        binding.validate_current()?;
        Ok(binding)
    }
    fn same_instance(&self, file: &std::fs::File) -> Result<bool> {
        let expected = self.mounted_root.metadata()?;
        let observed = file.metadata()?;
        Ok(expected.dev() == observed.dev()
            && expected.ino() == observed.ino()
            && expected.file_type() == observed.file_type())
    }
}
impl WorkspaceMountBinding for LinuxWorkspaceMountBinding {
    fn mount(&self) -> &Mount {
        &self.mount
    }
    fn root_instance_identity(&self) -> Result<String> {
        self.validate_current()?;
        let identity = crate::filesystem::retained_directory_identity(self.mounted_root.as_fd())?;
        self.validate_current()?;
        Ok(identity)
    }
    fn validate_current(&self) -> Result<()> {
        require_live_mount_process(&self.child_pidfd)?;
        if std::fs::canonicalize(&self.mount.source)? != self.mount.source {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Owner workspace root now traverses a different path",
            ));
        }
        let current_owner = Self::open_instance(&self.mount.source)?;
        let current_sandbox = Self::open_instance(&self.child_root_path)?;
        if !self.same_instance(&current_owner)? || !self.same_instance(&current_sandbox)? {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Owner path or sandbox mount instance was replaced",
            ));
        }
        require_live_mount_process(&self.child_pidfd)
    }
}

struct LinuxSandboxChild {
    workspace_mounts: Vec<Mount>,
    monitor: Child,
    child_pid: u32,
    child_pidfd: Option<OwnedFd>,
    exit_code: Option<i32>,
}

#[async_trait]
impl SandboxChildControl for LinuxSandboxChild {
    fn id(&self) -> Option<u32> {
        Some(self.child_pid)
    }

    fn workspace_mount_bindings(&self) -> Result<Vec<Arc<dyn WorkspaceMountBinding>>> {
        let pidfd = self.child_pidfd.as_ref().ok_or_else(|| {
            Error::new(
                ErrorCode::StaleReference,
                "Sandbox process has already exited",
            )
        })?;
        self.workspace_mounts
            .iter()
            .map(|mount| {
                Ok(Arc::new(LinuxWorkspaceMountBinding::capture(
                    mount,
                    self.child_pid,
                    pidfd,
                )?) as Arc<dyn WorkspaceMountBinding>)
            })
            .collect()
    }

    async fn kill(&mut self) -> Result<()> {
        if let Some(child_pidfd) = &self.child_pidfd {
            // Kill the exact Bubblewrap child first. This keeps Host-owned stdio open while the
            // runtime is terminated, so a persistent child cannot interpret pipe EOF as a clean exit.
            // SAFETY: pidfd pins the exact sandbox child and no user pointers are passed.
            let signalled = unsafe {
                libc::syscall(
                    libc::SYS_pidfd_send_signal,
                    child_pidfd.as_raw_fd(),
                    libc::SIGKILL,
                    std::ptr::null::<libc::siginfo_t>(),
                    0,
                )
            };
            if signalled != 0 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() != Some(libc::ESRCH) {
                    return Err(error.into());
                }
            } else {
                let wait_pidfd = duplicate_pidfd(child_pidfd)?;
                tokio::task::spawn_blocking(move || {
                    wait_pidfd_exit(&wait_pidfd, Duration::from_secs(2))
                })
                .await
                .map_err(|_| {
                    Error::new(
                        ErrorCode::Internal,
                        "Linux sandbox child reaper task failed",
                    )
                })??;
            }
        }

        if self.monitor.try_wait()?.is_none() {
            let _ = self.monitor.start_kill();
        }
        Ok(())
    }

    async fn wait(&mut self) -> Result<()> {
        let status = self.monitor.wait().await?;
        self.exit_code = status.code();
        Ok(())
    }

    fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }
}

fn verify_executable_bounded(path: &Path, digest: &str, max_bytes: u64) -> Result<Vec<u8>> {
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let meta = file.metadata()?;
    // SAFETY: getuid has no pointer arguments or preconditions.
    let uid = unsafe { libc::getuid() };
    if !meta.is_file()
        || meta.len() > max_bytes
        || meta.mode() & 0o022 != 0
        || !(meta.uid() == uid || meta.uid() == 0)
    {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Executable must be owned/root, regular, bounded, and not writable by others",
        ));
    }
    let mut bytes = Vec::new();
    std::io::Read::by_ref(&mut file)
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max_bytes
        || hex::encode(Sha256::digest(&bytes)) != digest.to_ascii_lowercase()
    {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Executable digest mismatch",
        ));
    }
    // Preserve baseline format semantics; do not newly accept scripts/interpreters.
    if !bytes.starts_with(b"\x7fELF") {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "Pinned ELF binary required",
        ));
    }
    Ok(bytes)
}

pub struct LinuxVerifier;
impl ExecutableVerifier for LinuxVerifier {
    fn verify(&self, path: &Path, digest: &str) -> Result<Vec<u8>> {
        verify_executable_bounded(path, digest, MAX_PROVIDER_EXECUTABLE_BYTES)
    }
}

pub fn verify_sealed_tool_executable(path: &Path, digest: &str) -> Result<Vec<u8>> {
    verify_executable_bounded(path, digest, MAX_SEALED_TOOL_EXECUTABLE_BYTES)
}
fn merged_usr_alias(path: &Path, target: &Path) -> bool {
    std::fs::read_link(path).is_ok_and(|link| {
        link == target
            || target
                .strip_prefix("/")
                .is_ok_and(|relative| link == relative)
    })
}

pub fn verify_application_executable(path: &Path, digest: &str) -> Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)?;
    let before = file.metadata()?;
    // SAFETY: getuid has no pointer arguments or preconditions.
    let uid = unsafe { libc::getuid() };
    if !before.is_file()
        || before.nlink() != 1
        || before.len() < 4
        || before.len() > MAX_APPLICATION_EXECUTABLE_BYTES
        || before.mode() & 0o022 != 0
        || !(before.uid() == uid || before.uid() == 0)
    {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Application executable must be owned/root, regular, single-link, bounded, and not writable by others",
        ));
    }
    let expected = digest.trim();
    if expected.len() != 64
        || !expected
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(Error::invalid(
            "Application executable digest must be canonical lowercase SHA-256",
        ));
    }
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut prefix = [0u8; 4];
    let mut prefix_len = 0usize;
    let mut total = 0u64;
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        let copy = (4 - prefix_len).min(count);
        if copy > 0 {
            prefix[prefix_len..prefix_len + copy].copy_from_slice(&buffer[..copy]);
            prefix_len += copy;
        }
        total = total.saturating_add(count as u64);
        if total > MAX_APPLICATION_EXECUTABLE_BYTES {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Application executable exceeds verification budget",
            ));
        }
        hasher.update(&buffer[..count]);
    }
    let after = file.metadata()?;
    if before.dev() != after.dev()
        || before.ino() != after.ino()
        || before.len() != after.len()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
        || total != before.len()
    {
        return Err(Error::new(
            ErrorCode::Conflict,
            "Application executable changed while being verified",
        ));
    }
    if prefix != *b"\x7fELF" {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "Pinned ELF application executable required",
        ));
    }
    if hex::encode(hasher.finalize()) != expected {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Application executable digest mismatch",
        ));
    }
    Ok(())
}
fn materialized_destination(mount: &Mount) -> Result<String> {
    mount.validate()?;
    let prefix = match mount.class {
        MountClass::Workspace => "/workspace/",
        MountClass::SystemConfig => "/etc/",
        MountClass::Secret => "/run/secrets/",
    };
    Ok(format!("{prefix}{}", mount.logical_name))
}

pub struct LinuxSandbox;
impl SandboxLauncher for LinuxSandbox {
    fn mechanism(&self) -> &'static str {
        "bubblewrap+landlock_required"
    }
    fn available(&self, helper: &Path) -> bool {
        Path::new("/usr/bin/bwrap").is_file() && helper.is_file()
    }
    fn diagnostics(&self, helper: &Path) -> serde_json::Value {
        serde_json::json!({"available":self.available(helper),"bubblewrap_present":Path::new("/usr/bin/bwrap").is_file(),"helper_present":helper.is_file(),"mechanism":self.mechanism()})
    }
    fn command(&self, s: &SandboxSpec) -> Result<Command> {
        s.validate()?;
        if !self.available(&s.helper) {
            return Err(Error::new(
                ErrorCode::SandboxDenied,
                "Bubblewrap and Landlock helper required; no unsandboxed fallback",
            ));
        }
        let mut p = Command::new("/usr/bin/bwrap");
        p.args([
            "--die-with-parent",
            "--new-session",
            "--unshare-all",
            "--clearenv",
            "--cap-drop",
            "ALL",
        ]);
        if let Some((_, raw_fd)) = s
            .environment
            .iter()
            .find(|(name, _)| name == SANDBOX_BWRAP_INFO_FD_ENV)
        {
            let fd = raw_fd.parse::<i32>().map_err(|_| {
                Error::new(
                    ErrorCode::SandboxDenied,
                    "Bubblewrap info descriptor marker is invalid",
                )
            })?;
            if fd < 3 {
                return Err(Error::new(
                    ErrorCode::SandboxDenied,
                    "Bubblewrap info descriptor must not use standard I/O",
                ));
            }
            p.arg("--info-fd").arg(fd.to_string());
        }
        if s.network {
            p.arg("--share-net");
        }
        p.args([
            "--proc",
            "/proc",
            "--dev",
            "/dev",
            "--perms",
            "1777",
            "--tmpfs",
            "/dev/shm",
            "--tmpfs",
            "/tmp",
            "--dir",
            "/home",
            "--dir",
            "/workspace",
            "--dir",
            "/plugin",
            "--dir",
            "/plugin/tools",
            "--dir",
            "/run",
            "--dir",
            "/run/secrets",
        ]);
        if s.nvidia_gpu {
            let (nodes, proc) = validated_nvidia_compute_sources()?;
            let mut pinned_sources = Vec::new();
            for (path, file) in nodes {
                p.arg("--dev-bind")
                    .arg(format!("/proc/self/fd/{}", file.as_raw_fd()))
                    .arg(path);
                pinned_sources.push(file);
            }
            p.arg("--ro-bind")
                .arg(format!("/proc/self/fd/{}", proc.as_raw_fd()))
                .arg(NVIDIA_PROC_PATH);
            pinned_sources.push(proc);
            // Hold O_PATH|O_NOFOLLOW descriptors through exec, so namespace construction
            // binds the verified inode rather than reopening a mutable HOST path.
            // SAFETY: pre_exec only applies fcntl to live owned descriptors, with no allocation.
            unsafe {
                p.pre_exec(move || {
                    for file in &pinned_sources {
                        if libc::fcntl(file.as_raw_fd(), libc::F_SETFD, 0) != 0 {
                            return Err(std::io::Error::last_os_error());
                        }
                    }
                    Ok(())
                });
            }
            p.args([
                "--setenv",
                semwright_platform_api::launch::NVIDIA_GPU_ENV,
                "1",
            ]);
        }
        for r in ["/usr", "/lib", "/lib64"] {
            if Path::new(r).exists() {
                p.args(["--ro-bind", r, r]);
            }
        }
        // Ubuntu/Debian merged-usr hosts expose /bin as a symlink into /usr/bin.
        // The sandbox already grants /usr read+execute. Recreate only that alias so
        // POSIX APIs such as popen(), which require /bin/sh, resolve to the same
        // already-authorized bytes without adding another executable filesystem tree.
        if merged_usr_alias(Path::new("/bin"), Path::new("/usr/bin")) {
            p.args(["--symlink", "usr/bin", "/bin"]);
        }
        p.args(["--dir", "/etc"]);
        if Path::new("/etc/ld.so.cache").exists() {
            p.args(["--ro-bind", "/etc/ld.so.cache", "/etc/ld.so.cache"]);
        }
        // Preserve the frozen baseline's driver-only update-alternatives support.
        if s.kind == SandboxKind::Driver && Path::new("/etc/alternatives").is_dir() {
            p.args(["--ro-bind", "/etc/alternatives", "/etc/alternatives"]);
        }
        for m in s
            .mounts
            .iter()
            .filter(|m| m.class == MountClass::SystemConfig)
        {
            let destination = materialized_destination(m)?;
            p.arg("--ro-bind").arg(&m.source).arg(destination);
        }
        for m in s.mounts.iter().filter(|m| m.class == MountClass::Secret) {
            let destination = materialized_destination(m)?;
            p.arg("--ro-bind").arg(&m.source).arg(destination);
        }
        for tool in &s.sealed_tools {
            let fd = match &tool.source {
                SealedToolSource::UnixFd(fd) => *fd,
                SealedToolSource::VerifiedFile { .. } => {
                    return Err(Error::new(
                        ErrorCode::SandboxDenied,
                        "Linux sealed tools require Host-owned immutable file descriptors",
                    ));
                }
            };
            // Materialize the Host-verified sealed bytes directly into the private
            // sandbox root. The child receives no write/remove/create Landlock rights
            // for this path, so the executable remains immutable after policy install.
            // Avoid --ro-bind-data here: Bubblewrap unlinks its backing tempfile after
            // bind-mounting it, which can make later execve() resolve as ENOENT under
            // deleted-file mediation on Ubuntu/AppArmor.
            p.args(["--perms", "0500", "--file"])
                .arg(fd.to_string())
                .arg(format!("/plugin/tools/{}", tool.name));
        }
        p.arg("--ro-bind")
            .arg(&s.staged_executable)
            .arg("/plugin/bin");
        p.arg("--ro-bind").arg(&s.helper).arg("/plugin/sandbox");
        if s.nvidia_gpu {
            // The same trusted helper installs the final policy in Blender's
            // own PID, before exec, so future CUDA threads remain in its scope.
            p.arg("--ro-bind")
                .arg(&s.helper)
                .arg("/plugin/gpu-native-helper");
        }
        for m in s.mounts.iter().filter(|m| m.class == MountClass::Workspace) {
            let destination = materialized_destination(m)?;
            p.arg(if m.read_only { "--ro-bind" } else { "--bind" })
                .arg(&m.source)
                .arg(destination);
        }
        p.args([
            "--setenv",
            "HOME",
            "/home",
            "--setenv",
            "PATH",
            "/usr/bin:/bin",
            "--setenv",
            "LANG",
            "C.UTF-8",
        ]);
        let materialized_mounts = s
            .mounts
            .iter()
            .map(|mount| {
                Ok(MaterializedMount {
                    class: mount.class,
                    logical_name: mount.logical_name.clone(),
                    path: materialized_destination(mount)?,
                    read_only: mount.read_only,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let materialized_tools = s
            .sealed_tools
            .iter()
            .map(|tool| MaterializedTool {
                name: tool.name.clone(),
                path: format!("/plugin/tools/{}", tool.name),
            })
            .collect::<Vec<_>>();
        let typed_args = s
            .environment
            .iter()
            .any(|(name, value)| name == SANDBOX_HOST_TOOL_TYPED_ARGS_ENV && value == "1");
        let resolved_args = resolve_host_tool_args(
            &s.args,
            &materialized_mounts,
            &materialized_tools,
            typed_args,
        )?;

        // Only Semwright application drivers consume the logical mount/tool tables through
        // driver-sdk helpers. Do not expose internal topology to plugins or external MCP
        // children that do not need this authority-bearing metadata.
        if s.kind == SandboxKind::Driver {
            let mount_table = encode_materialized_mounts(&materialized_mounts)?;
            p.arg("--setenv").arg(SANDBOX_MOUNTS_ENV).arg(mount_table);

            if !materialized_tools.is_empty() {
                let tool_table = encode_materialized_tools(&materialized_tools)?;
                p.arg("--setenv").arg(SANDBOX_TOOLS_ENV).arg(tool_table);
            }
        }
        let sandbox_cwd = if let Some((_, requested)) = s
            .environment
            .iter()
            .find(|(name, _)| name == SANDBOX_HOST_TOOL_CWD_ENV)
        {
            let allowed = s
                .mounts
                .iter()
                .filter(|mount| mount.class == MountClass::Workspace)
                .map(materialized_destination)
                .collect::<Result<Vec<_>>>()?;
            if !allowed.iter().any(|path| path == requested) {
                return Err(Error::new(
                    ErrorCode::SandboxDenied,
                    "Linux Host-tool working directory is outside selected mounts",
                ));
            }
            requested.clone()
        } else {
            "/tmp".into()
        };
        for (name, value) in &s.environment {
            if matches!(
                name.as_str(),
                SANDBOX_HOST_TOOL_CHILD_ENV
                    | SANDBOX_HOST_TOOL_CWD_ENV
                    | SANDBOX_HOST_TOOL_TYPED_ARGS_ENV
                    | SANDBOX_BWRAP_INFO_FD_ENV
            ) {
                continue;
            }
            p.arg("--setenv").arg(name).arg(value);
        }
        if matches!(s.kind, SandboxKind::Driver | SandboxKind::ExternalMcp) {
            p.args([
                "--setenv",
                "XDG_CACHE_HOME",
                "/tmp/cache",
                "--setenv",
                "XDG_CONFIG_HOME",
                "/tmp/config",
                "--setenv",
                "XDG_DATA_HOME",
                "/tmp/data",
            ]);
            match s.kind {
                SandboxKind::Driver => {
                    p.args(["--setenv", "SEMWRIGHT_DRIVER_SANDBOX", "landlock-bwrap-v1"]);
                }
                SandboxKind::ExternalMcp => {
                    p.args(["--setenv", "SEMWRIGHT_MCP_SANDBOX", "landlock-bwrap-v1"]);
                }
                SandboxKind::Plugin => {}
            }
        }
        p.arg("--chdir")
            .arg(&sandbox_cwd)
            .args(["--", "/plugin/sandbox"]);
        if s.nvidia_gpu {
            p.arg("--nvidia-gpu");
        }
        if let Some(l) = &s.limits {
            for (flag, n) in [
                ("--limit-nofile", l.open_files),
                ("--limit-nproc", l.processes),
                ("--limit-cpu", l.cpu_seconds),
                ("--limit-as", l.address_space_bytes),
                ("--limit-fsize", l.file_size_bytes),
            ] {
                p.arg(flag).arg(n.to_string());
            }
        }
        for m in &s.mounts {
            let destination = materialized_destination(m)?;
            if !m.read_only {
                p.arg("--write-root").arg(destination);
            } else if m.execute {
                p.arg("--exec-root").arg(destination);
            } else {
                p.arg("--read-root").arg(destination);
            }
        }
        for tool in &s.sealed_tools {
            p.arg("--exec-root")
                .arg(format!("/plugin/tools/{}", tool.name));
        }
        p.args(["--", "/plugin/bin"])
            .args(&resolved_args)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        Ok(p)
    }

    fn spawn(&self, s: &SandboxSpec) -> Result<SandboxProcess> {
        self.spawn_with_stderr_observer(s, None)
    }

    fn spawn_with_stderr_observer(
        &self,
        s: &SandboxSpec,
        observer: Option<Arc<dyn SandboxStderrObserver>>,
    ) -> Result<SandboxProcess> {
        if s.environment
            .iter()
            .any(|(name, _)| name == SANDBOX_BWRAP_INFO_FD_ENV)
        {
            return Err(Error::new(
                ErrorCode::SandboxDenied,
                "Bubblewrap lifecycle descriptor marker is Host-reserved",
            ));
        }
        if s.environment.len() >= 16 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Sandbox environment has no room for Host lifecycle metadata",
            ));
        }

        let (read_fd, write_fd) = bwrap_info_pipe()?;
        let mut spec = s.clone();
        spec.environment.push((
            SANDBOX_BWRAP_INFO_FD_ENV.into(),
            write_fd.as_raw_fd().to_string(),
        ));
        let mut command = self.command(&spec)?;
        let observing_stderr = observer.is_some();
        if let Some(observer) = observer {
            command.stderr(private_stderr_pipe(observer)?);
        }
        let mut monitor = command.spawn().map_err(|_| {
            Error::new(
                ErrorCode::SandboxDenied,
                "Platform sandbox process failed to start",
            )
        })?;
        // Command retains the configured writer. Drop it so EOF reflects child
        // writers, not a parent-owned duplicate. The default command is unchanged.
        if observing_stderr {
            drop(command);
        }
        drop(write_fd);

        let child_pid = match read_bwrap_child_pid(&read_fd, &mut monitor) {
            Ok(pid) => pid,
            Err(error) => {
                let _ = monitor.start_kill();
                return Err(error);
            }
        };
        let child_pidfd = match open_pidfd(child_pid) {
            Ok(pidfd) => pidfd,
            Err(error) => {
                let _ = monitor.start_kill();
                return Err(error);
            }
        };

        let stdin = monitor
            .stdin
            .take()
            .map(|value| Box::new(value) as SandboxStdin);
        let stdout = monitor
            .stdout
            .take()
            .map(|value| Box::new(value) as SandboxStdout);
        let (Some(stdin), Some(stdout)) = (stdin, stdout) else {
            let _ = monitor.start_kill();
            return Err(Error::new(
                ErrorCode::ProtocolMismatch,
                "Linux sandbox child must expose piped stdin and stdout",
            ));
        };

        Ok(SandboxProcess::from_parts(
            stdin,
            stdout,
            Box::new(LinuxSandboxChild {
                workspace_mounts: s
                    .mounts
                    .iter()
                    .filter(|mount| mount.class == MountClass::Workspace)
                    .cloned()
                    .collect(),
                monitor,
                child_pid,
                child_pidfd,
                exit_code: None,
            }),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[derive(Default)]
    struct StderrObservation {
        bytes: std::sync::Mutex<Vec<u8>>,
        terminal: std::sync::Mutex<Option<SandboxStderrTerminal>>,
    }

    impl SandboxStderrObserver for StderrObservation {
        fn started(&self) {}
        fn bytes(&self, bytes: &[u8]) {
            self.bytes.lock().unwrap().extend_from_slice(bytes);
        }
        fn terminal(&self, terminal: SandboxStderrTerminal) {
            *self.terminal.lock().unwrap() = Some(terminal);
        }
    }

    fn await_stderr(observation: &StderrObservation) -> SandboxStderrTerminal {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(terminal) = *observation.terminal.lock().unwrap() {
                return terminal;
            }
            assert!(
                Instant::now() < deadline,
                "stderr drain did not become terminal"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn startup_stderr_drains_before_synchronous_metadata_wait_on_one_runtime_thread() {
        let observed = Arc::new(StderrObservation::default());
        let (read_fd, write_fd) = bwrap_info_pipe().unwrap();
        let writer = write_fd.as_raw_fd();
        // This fixture writes far more than a pipe buffer before reporting its
        // PID. The parent then blocks in the real metadata reader on this same
        // current-thread runtime. Only the independent drain can make progress.
        let script = format!(
            "i=0; while [ $i -lt 16384 ]; do printf 0123456789abcdef >&2; i=$((i+1)); done; printf '{{\"child-pid\":%s}}' $$ > /proc/self/fd/{writer}"
        );
        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg(script)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(private_stderr_pipe(observed.clone()).unwrap());
        let mut child = command.spawn().unwrap();
        drop(command);
        drop(write_fd);
        assert!(read_bwrap_child_pid(&read_fd, &mut child).unwrap() > 0);
        assert!(child.wait().await.unwrap().success());
        assert_eq!(await_stderr(&observed), SandboxStderrTerminal::Eof);
        let bytes = observed.bytes.lock().unwrap();
        assert_eq!(bytes.len(), 16 * 16384);
        assert!(
            bytes
                .chunks_exact(16)
                .all(|chunk| chunk == b"0123456789abcdef")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn startup_stderr_early_metadata_failure_keeps_original_bytes_and_error() {
        let observed = Arc::new(StderrObservation::default());
        let (read_fd, write_fd) = bwrap_info_pipe().unwrap();
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "printf 'original pre-Hello failure' >&2; exit 1"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(private_stderr_pipe(observed.clone()).unwrap());
        let mut child = command.spawn().unwrap();
        drop(command);
        drop(write_fd);
        let error = read_bwrap_child_pid(&read_fd, &mut child).unwrap_err();
        assert_eq!(error.code, ErrorCode::SandboxDenied);
        assert_eq!(child.wait().await.unwrap().code(), Some(1));
        assert_eq!(await_stderr(&observed), SandboxStderrTerminal::Eof);
        assert_eq!(
            *observed.bytes.lock().unwrap(),
            b"original pre-Hello failure"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn startup_stderr_reader_survives_a_successful_handshake_boundary_and_cleanup() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let observed = Arc::new(StderrObservation::default());
        let mut command = Command::new("/bin/sh");
        command
            .args([
                "-c",
                "printf ready; read answer; printf 'post-Hello stderr' >&2",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(private_stderr_pipe(observed.clone()).unwrap());
        let mut child = command.spawn().unwrap();
        drop(command);
        let mut ready = [0; 5];
        child
            .stdout
            .take()
            .unwrap()
            .read_exact(&mut ready)
            .await
            .unwrap();
        assert_eq!(&ready, b"ready");
        assert!(observed.terminal.lock().unwrap().is_none());
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"continue\n")
            .await
            .unwrap();
        assert!(
            child.wait().await.unwrap().success(),
            "observer did not break child stderr"
        );
        assert_eq!(await_stderr(&observed), SandboxStderrTerminal::Eof);
        assert_eq!(*observed.bytes.lock().unwrap(), b"post-Hello stderr");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn startup_stderr_observer_failure_does_not_break_the_child_pipe() {
        struct FailedObserver(std::sync::Mutex<Option<SandboxStderrTerminal>>);
        impl SandboxStderrObserver for FailedObserver {
            fn started(&self) {}
            fn bytes(&self, _: &[u8]) {
                panic!("synthetic observer failure");
            }
            fn terminal(&self, terminal: SandboxStderrTerminal) {
                *self.0.lock().unwrap() = Some(terminal);
            }
        }
        let observed = Arc::new(FailedObserver(std::sync::Mutex::new(None)));
        let mut command = Command::new("/bin/sh");
        command
            .args([
                "-c",
                "i=0; while [ $i -lt 16384 ]; do printf 0123456789abcdef >&2; i=$((i+1)); done",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(private_stderr_pipe(observed.clone()).unwrap());
        let mut child = command.spawn().unwrap();
        drop(command);
        assert!(
            child.wait().await.unwrap().success(),
            "failed observer still drains and discards"
        );
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(terminal) = *observed.0.lock().unwrap() {
                assert_eq!(terminal, SandboxStderrTerminal::Interrupted);
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn already_exited_child_does_not_become_a_sandbox_denial() {
        let pidfd = open_pidfd(i32::MAX as u32).expect("missing child PID is terminal");
        assert!(pidfd.is_none());
    }

    #[test]
    fn pidfd_wait_observes_owned_child_exit() {
        let mut child = std::process::Command::new("/usr/bin/true")
            .spawn()
            .expect("spawn short-lived child");
        let pidfd = open_pidfd(child.id())
            .expect("open pidfd")
            .expect("child is live when pidfd is opened");
        child.wait().expect("reap short-lived child");
        wait_pidfd_exit(&pidfd, Duration::from_secs(1)).expect("pidfd becomes terminal");
    }

    #[test]
    fn bounded_executable_verifier_honors_requested_limit() {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("fixture");
        let bytes = b"\x7fELFbounded-verifier-fixture";
        std::fs::write(&executable, bytes).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o500)).unwrap();
        let digest = hex::encode(Sha256::digest(bytes));

        assert!(
            verify_executable_bounded(&executable, &digest, bytes.len() as u64).is_ok(),
            "exact bounded executable should verify"
        );
        let error =
            verify_executable_bounded(&executable, &digest, bytes.len() as u64 - 1).unwrap_err();
        assert_eq!(error.code, ErrorCode::PermissionDenied);
    }

    #[test]
    fn merged_usr_alias_accepts_relative_and_absolute_usr_targets_only() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let relative = root.path().join("bin-relative");
        let absolute = root.path().join("bin-absolute");
        let wrong = root.path().join("bin-wrong");
        symlink("usr/bin", &relative).unwrap();
        symlink("/usr/bin", &absolute).unwrap();
        symlink("usr/local/bin", &wrong).unwrap();

        assert!(merged_usr_alias(&relative, Path::new("/usr/bin")));
        assert!(merged_usr_alias(&absolute, Path::new("/usr/bin")));
        assert!(!merged_usr_alias(&wrong, Path::new("/usr/bin")));
        assert!(!merged_usr_alias(
            &root.path().join("missing"),
            Path::new("/usr/bin")
        ));
    }

    #[test]
    fn bubblewrap_child_pid_metadata_is_strict_and_bounded() {
        assert_eq!(
            parse_bwrap_child_pid(br#"{"child-pid":4321}"#).unwrap(),
            4321
        );
        for invalid in [
            br#"{}"#.as_slice(),
            br#"{"child-pid":0}"#.as_slice(),
            br#"{"child-pid":"4321"}"#.as_slice(),
            br#"{"child-pid":2147483648}"#.as_slice(),
            br#"{"child-pid":4321"#.as_slice(),
        ] {
            assert!(parse_bwrap_child_pid(invalid).is_err());
        }
        assert!(parse_bwrap_child_pid(&vec![b'x'; MAX_BWRAP_INFO_BYTES + 1]).is_err());
    }

    #[test]
    fn sealed_tool_budget_is_distinct_from_provider_budget() {
        assert_eq!(MAX_PROVIDER_EXECUTABLE_BYTES, 64 * 1024 * 1024);
        assert_eq!(MAX_SEALED_TOOL_EXECUTABLE_BYTES, 256 * 1024 * 1024);
        assert_eq!(MAX_APPLICATION_EXECUTABLE_BYTES, 512 * 1024 * 1024);
    }

    #[test]
    fn nvidia_compute_nodes_cannot_be_regular_files_symlinks_or_display_grants() {
        use std::os::unix::fs::symlink;
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("nvidia0");
        std::fs::write(&file, b"fake device").unwrap();
        let link = directory.path().join("nvidiactl");
        symlink("/dev/null", &link).unwrap();
        assert!(!is_root_owned_character_device(
            &std::fs::symlink_metadata(file).unwrap()
        ));
        assert!(!is_root_owned_character_device(
            &std::fs::symlink_metadata(link).unwrap()
        ));
        let paths = semwright_platform_api::launch::NVIDIA_COMPUTE_DEVICE_PATHS;
        assert_eq!(
            paths,
            [
                "/dev/nvidia0",
                "/dev/nvidiactl",
                "/dev/nvidia-uvm",
                "/dev/nvidia-uvm-tools"
            ]
        );
        assert!(
            paths
                .iter()
                .all(|path| !path.contains("dri") && !path.contains("modeset") && *path != "/dev")
        );
    }
}

#[cfg(test)]
mod workspace_binding_instance_tests {
    use super::*;
    fn fixture(path: &Path) -> LinuxWorkspaceMountBinding {
        LinuxWorkspaceMountBinding {
            mount: Mount {
                source: path.to_path_buf(),
                class: MountClass::Workspace,
                logical_name: "manim-runtime".into(),
                read_only: true,
                execute: false,
            },
            mounted_root: LinuxWorkspaceMountBinding::open_instance(path).unwrap(),
            child_root_path: path.to_path_buf(),
            child_pidfd: open_pidfd(std::process::id()).unwrap().unwrap(),
        }
    }
    #[test]
    fn mount_identity_matches_the_actual_retained_local_root() {
        use semwright_platform_api::filesystem::ScopedFilesystem;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("root");
        std::fs::create_dir(&path).unwrap();
        let binding = fixture(&path);
        let identity = binding.root_instance_identity().unwrap();
        let local = crate::filesystem::LinuxFilesystem
            .open_root_with_identity(&path, true, false, &identity)
            .unwrap();
        assert_eq!(local.root_instance_identity().unwrap(), identity);
        assert!(binding.mount().read_only);
        assert!(!binding.mount().execute);
    }

    #[test]
    fn mount_identity_refuses_replacement_while_local_root_keeps_original_custody() {
        use semwright_platform_api::filesystem::ScopedFilesystem;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("root");
        let retained = directory.path().join("original-root");
        std::fs::create_dir(&path).unwrap();
        let binding = fixture(&path);
        let identity = binding.root_instance_identity().unwrap();
        let local = crate::filesystem::LinuxFilesystem
            .open_root_with_identity(&path, true, false, &identity)
            .unwrap();
        std::fs::rename(&path, &retained).unwrap();
        std::fs::create_dir(&path).unwrap();
        let changed = binding.root_instance_identity();
        assert_eq!(local.root_instance_identity().unwrap(), identity);
        std::fs::remove_dir(&path).unwrap();
        std::fs::rename(&retained, &path).unwrap();
        assert_eq!(changed.unwrap_err().code, ErrorCode::StaleReference);
        assert_eq!(binding.root_instance_identity().unwrap(), identity);
    }

    #[test]
    fn file_workspace_binding_does_not_claim_directory_identity() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("workspace.sqlite3");
        std::fs::write(&path, b"fixture").unwrap();
        let binding = fixture(&path);
        binding.validate_current().unwrap();
        assert_eq!(
            binding.root_instance_identity().unwrap_err().code,
            ErrorCode::Unsupported
        );
    }

    #[test]
    fn retained_root_rejects_replacement_at_the_same_owner_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("root");
        std::fs::create_dir(&path).unwrap();
        let binding = fixture(&path);
        binding.validate_current().unwrap();
        let old = dir.path().join("old-root");
        std::fs::rename(&path, &old).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert_eq!(
            binding.validate_current().unwrap_err().code,
            ErrorCode::StaleReference
        );
        let original = std::fs::metadata(&old).unwrap();
        assert_eq!(
            binding.mounted_root.metadata().unwrap().ino(),
            original.ino()
        );
    }
    #[test]
    fn retained_root_rejects_owner_path_redirected_by_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("root");
        std::fs::create_dir(&path).unwrap();
        let binding = fixture(&path);
        let old = dir.path().join("old-root");
        std::fs::rename(&path, &old).unwrap();
        std::os::unix::fs::symlink(&old, &path).unwrap();
        assert_eq!(
            binding.validate_current().unwrap_err().code,
            ErrorCode::StaleReference
        );
    }
}
