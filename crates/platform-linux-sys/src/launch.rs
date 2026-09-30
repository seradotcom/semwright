use async_trait::async_trait;
use semwright_platform_api::launch::{
    ExecutableVerifier, MaterializedMount, MaterializedTool, Mount, MountClass,
    SANDBOX_HOST_TOOL_CHILD_ENV, SANDBOX_HOST_TOOL_CWD_ENV, SANDBOX_HOST_TOOL_TYPED_ARGS_ENV,
    SANDBOX_MOUNTS_ENV, SANDBOX_TOOLS_ENV, SandboxChildControl, SandboxKind, SandboxLauncher,
    SandboxProcess, SandboxSpec, SandboxStdin, SandboxStdout, SealedToolSource,
    encode_materialized_mounts, encode_materialized_tools, resolve_host_tool_args,
};
use semwright_types::{Error, ErrorCode, Result};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::Path,
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::process::{Child, Command};

const MAX_PROVIDER_EXECUTABLE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SEALED_TOOL_EXECUTABLE_BYTES: u64 = 256 * 1024 * 1024;
const SANDBOX_BWRAP_INFO_FD_ENV: &str = "SEMWRIGHT_INTERNAL_BWRAP_INFO_FD";
const MAX_BWRAP_INFO_BYTES: usize = 16 * 1024;
const BWRAP_INFO_TIMEOUT: Duration = Duration::from_secs(5);

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

fn open_pidfd(pid: u32) -> Result<OwnedFd> {
    // SAFETY: pidfd_open takes a numeric PID and flags only.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    if fd < 0 {
        return Err(Error::new(
            ErrorCode::SandboxDenied,
            "pidfd_open is required for Linux sandbox child lifecycle safety",
        ));
    }
    // SAFETY: successful pidfd_open returns a newly-owned descriptor.
    Ok(unsafe { OwnedFd::from_raw_fd(fd as i32) })
}

struct LinuxSandboxChild {
    monitor: Child,
    child_pid: u32,
    child_pidfd: OwnedFd,
    exit_code: Option<i32>,
}

#[async_trait]
impl SandboxChildControl for LinuxSandboxChild {
    fn id(&self) -> Option<u32> {
        Some(self.child_pid)
    }

    async fn kill(&mut self) -> Result<()> {
        // Kill the exact Bubblewrap child first. This keeps Host-owned stdio open while the
        // runtime is terminated, so a persistent child cannot interpret pipe EOF as a clean exit.
        // SAFETY: pidfd pins the exact sandbox child and no user pointers are passed.
        let signalled = unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                self.child_pidfd.as_raw_fd(),
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
        || format!("{:x}", Sha256::digest(&bytes)) != digest.to_ascii_lowercase()
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
        for r in ["/usr", "/lib", "/lib64"] {
            if Path::new(r).exists() {
                p.args(["--ro-bind", r, r]);
            }
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
        let mut monitor = command.spawn().map_err(|_| {
            Error::new(
                ErrorCode::SandboxDenied,
                "Platform sandbox process failed to start",
            )
        })?;
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
                // child_pid was just reported by the still-owned Bubblewrap monitor, so it cannot
                // have been recycled without the monitor first observing that exit.
                // SAFETY: kill takes a numeric PID/signal only; errors are best-effort cleanup here.
                let _ = unsafe { libc::kill(child_pid as i32, libc::SIGKILL) };
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

    #[test]
    fn bounded_executable_verifier_honors_requested_limit() {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("fixture");
        let bytes = b"\x7fELFbounded-verifier-fixture";
        std::fs::write(&executable, bytes).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o500)).unwrap();
        let digest = format!("{:x}", Sha256::digest(bytes));

        assert!(
            verify_executable_bounded(&executable, &digest, bytes.len() as u64).is_ok(),
            "exact bounded executable should verify"
        );
        let error =
            verify_executable_bounded(&executable, &digest, bytes.len() as u64 - 1).unwrap_err();
        assert_eq!(error.code, ErrorCode::PermissionDenied);
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
    }
}
