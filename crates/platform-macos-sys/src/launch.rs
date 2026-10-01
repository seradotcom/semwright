use crate::sandbox_main::{AS_ENV, CPU_ENV, CWD_ENV, FSIZE_ENV, NOFILE_ENV, NPROC_ENV, PARENT_ARG};
use async_trait::async_trait;
use semwright_platform_api::launch::{
    ExecutableVerifier, MaterializedMount, MaterializedTool, MountClass,
    SANDBOX_HOST_TOOL_CHILD_ENV, SANDBOX_HOST_TOOL_CWD_ENV, SANDBOX_HOST_TOOL_TYPED_ARGS_ENV,
    SANDBOX_MOUNTS_ENV, SANDBOX_TOOLS_ENV, SandboxChildControl, SandboxKind, SandboxLauncher,
    SandboxProcess, SandboxSpec, SandboxStdin, SandboxStdout, SealedToolSource,
    encode_materialized_mounts, encode_materialized_tools, resolve_host_tool_args,
};
use semwright_types::{Error, ErrorCode, Result, unique_id};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
    os::unix::{
        fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Command as StdCommand, Stdio},
};
use tokio::process::{Child, Command};

const MAX_PROVIDER_EXECUTABLE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SEALED_TOOL_EXECUTABLE_BYTES: u64 = 256 * 1024 * 1024;

fn verify_executable_bounded(path: &Path, digest: &str, max_bytes: u64) -> Result<Vec<u8>> {
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)?;
    let m = f.metadata()?;
    // SAFETY: getuid has no memory/lifetime requirements.
    let uid = unsafe { libc::getuid() };
    if !m.is_file()
        || m.nlink() != 1
        || m.len() > max_bytes
        || m.mode() & 0o022 != 0
        || !(m.uid() == 0 || m.uid() == uid)
    {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Unsafe executable ownership/type/size",
        ));
    }
    let mut bytes = vec![];
    std::io::Read::by_ref(&mut f)
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
    let arches = super::macho::architectures(&bytes)?;
    let wanted = match std::env::consts::ARCH {
        "aarch64" => super::macho::Architecture::Arm64,
        "x86_64" => super::macho::Architecture::X86_64,
        _ => {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "Unsupported host architecture",
            ));
        }
    };
    if !arches.contains(&wanted) {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "No native architecture slice; no Rosetta fallback",
        ));
    }
    Ok(bytes)
}

pub struct MacVerifier;
impl ExecutableVerifier for MacVerifier {
    fn verify(&self, path: &Path, digest: &str) -> Result<Vec<u8>> {
        verify_executable_bounded(path, digest, MAX_PROVIDER_EXECUTABLE_BYTES)
    }
}

pub fn verify_sealed_tool_executable(path: &Path, digest: &str) -> Result<Vec<u8>> {
    verify_executable_bounded(path, digest, MAX_SEALED_TOOL_EXECUTABLE_BYTES)
}
const CODESIGN: &str = "/usr/bin/codesign";

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn exception_path(path: &Path) -> Result<String> {
    let canonical = std::fs::canonicalize(path).map_err(|_| {
        Error::new(
            ErrorCode::NotFound,
            "macOS sandbox entitlement path does not exist",
        )
    })?;
    if !canonical.is_absolute() {
        return Err(Error::invalid(
            "macOS sandbox entitlement path must be absolute",
        ));
    }
    let mut value = canonical
        .to_str()
        .ok_or_else(|| Error::invalid("macOS sandbox paths must be Unicode"))?
        .to_owned();
    if canonical.is_dir() && !value.ends_with('/') {
        value.push('/');
    }
    Ok(value)
}

fn write_private(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn entitlements(
    read_only: &BTreeSet<String>,
    read_write: &BTreeSet<String>,
    network: bool,
) -> String {
    fn array(key: &str, values: &BTreeSet<String>) -> String {
        if values.is_empty() {
            return String::new();
        }
        let rows = values
            .iter()
            .map(|value| format!("    <string>{}</string>\n", xml_escape(value)))
            .collect::<String>();
        format!("  <key>{key}</key>\n  <array>\n{rows}  </array>\n")
    }
    let network = if network {
        "  <key>com.apple.security.network.client</key><true/>\n"
    } else {
        ""
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
<plist version=\"1.0\"><dict>\n\
  <key>com.apple.security.app-sandbox</key><true/>\n\
{network}{}{}\
</dict></plist>\n",
        array(
            "com.apple.security.temporary-exception.files.absolute-path.read-only",
            read_only
        ),
        array(
            "com.apple.security.temporary-exception.files.absolute-path.read-write",
            read_write
        )
    )
}

fn child_entitlements() -> &'static [u8] {
    b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
<plist version=\"1.0\"><dict>\n\
  <key>com.apple.security.app-sandbox</key><true/>\n\
  <key>com.apple.security.inherit</key><true/>\n\
</dict></plist>\n"
}

fn codesign(path: &Path, entitlements: &Path, identifier: &str) -> Result<()> {
    let status = StdCommand::new(CODESIGN)
        .args([
            "--force",
            "--sign",
            "-",
            "--options",
            "runtime",
            "--entitlements",
        ])
        .arg(entitlements)
        .arg("-i")
        .arg(identifier)
        .arg(path)
        .status()
        .map_err(|_| Error::new(ErrorCode::SandboxDenied, "macOS codesign failed to start"))?;
    if !status.success() {
        return Err(Error::new(
            ErrorCode::SandboxDenied,
            "macOS App Sandbox ad-hoc signing failed",
        ));
    }
    Ok(())
}

fn copy_helper(source: &Path, destination: &Path) -> Result<()> {
    std::fs::copy(source, destination)?;
    std::fs::set_permissions(destination, std::fs::Permissions::from_mode(0o500))?;
    Ok(())
}

fn materialize(spec: &SandboxSpec) -> Result<(Vec<MaterializedMount>, Vec<MaterializedTool>)> {
    let mounts = spec
        .mounts
        .iter()
        .map(|mount| {
            mount.validate()?;
            let path = exception_path(&mount.source)?;
            Ok(MaterializedMount {
                class: mount.class,
                logical_name: mount.logical_name.clone(),
                path,
                read_only: mount.read_only,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let tools = spec
        .sealed_tools
        .iter()
        .map(|tool| {
            tool.validate()?;
            let path = match &tool.source {
                SealedToolSource::VerifiedFile { path, sha256 } => {
                    let _ = verify_sealed_tool_executable(path, sha256)?;
                    exception_path(path)?
                }
                SealedToolSource::UnixFd(_) => {
                    return Err(Error::new(
                        ErrorCode::SandboxDenied,
                        "macOS sandbox requires verified-file sealed tools",
                    ));
                }
            };
            Ok(MaterializedTool {
                name: tool.name.clone(),
                path,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((mounts, tools))
}

struct SandboxRoot {
    path: PathBuf,
    armed: bool,
}

impl SandboxRoot {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn into_path(mut self) -> PathBuf {
        self.armed = false;
        self.path.clone()
    }
}

impl Drop for SandboxRoot {
    fn drop(&mut self) {
        if self.armed {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

struct MacSandboxChild {
    child: Child,
    process_group: i32,
    root: PathBuf,
    exit_code: Option<i32>,
}

impl MacSandboxChild {
    fn cleanup(&self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }

    fn kill_group(&self) -> Result<()> {
        // SAFETY: process_group is the positive PID assigned to the sandbox parent group.
        let result = unsafe { libc::kill(-self.process_group, libc::SIGKILL) };
        if result == 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            Ok(())
        } else {
            Err(error.into())
        }
    }
}

impl Drop for MacSandboxChild {
    fn drop(&mut self) {
        let _ = self.kill_group();
        let _ = self.child.start_kill();
        self.cleanup();
    }
}

#[async_trait]
impl SandboxChildControl for MacSandboxChild {
    fn id(&self) -> Option<u32> {
        self.child.id()
    }

    async fn kill(&mut self) -> Result<()> {
        self.kill_group()?;
        let _ = self.child.wait().await;
        self.cleanup();
        Ok(())
    }

    async fn wait(&mut self) -> Result<()> {
        let status = self.child.wait().await?;
        self.exit_code = status.code();
        self.cleanup();
        Ok(())
    }

    fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }
}

/// App Sandbox confinement is established by a signed Semwright-owned parent/helper bundle.
pub struct MacSandbox;
impl SandboxLauncher for MacSandbox {
    fn command(&self, spec: &SandboxSpec) -> Result<Command> {
        spec.validate()?;
        Err(Error::new(
            ErrorCode::SandboxDenied,
            "macOS App Sandbox launch requires platform-owned spawn",
        ))
    }

    fn spawn(&self, spec: &SandboxSpec) -> Result<SandboxProcess> {
        spec.validate()?;
        if !self.available(&spec.helper) {
            return Err(Error::new(
                ErrorCode::SandboxDenied,
                "macOS App Sandbox requires the Semwright helper and /usr/bin/codesign",
            ));
        }

        let helper = std::fs::canonicalize(&spec.helper).map_err(|_| {
            Error::new(
                ErrorCode::SandboxDenied,
                "macOS sandbox helper is unavailable",
            )
        })?;
        if helper != spec.helper {
            return Err(Error::new(
                ErrorCode::SandboxDenied,
                "macOS sandbox helper path must already be canonical",
            ));
        }

        let parent = spec.staged_executable.parent().ok_or_else(|| {
            Error::new(
                ErrorCode::SandboxDenied,
                "macOS staged executable has no private parent directory",
            )
        })?;
        let root = parent.join(format!(".semwright-macos-sandbox-{}", unique_id()));
        std::fs::create_dir(&root)?;
        let root_guard = SandboxRoot::new(root.clone());
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?;

        let app = root.join("SemwrightSandbox.app");
        let contents = app.join("Contents");
        let macos = contents.join("MacOS");
        let helpers = contents.join("Helpers");
        let home = root.join("home");
        let tmp = root.join("tmp");
        for path in [&app, &contents, &macos, &helpers, &home, &tmp] {
            std::fs::create_dir(path)?;
        }
        std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o700))?;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o700))?;

        let app_exec = macos.join("SemwrightSandbox");
        let exec_helper = helpers.join("semwright-sandbox-exec");
        copy_helper(&helper, &app_exec)?;
        copy_helper(&helper, &exec_helper)?;

        let info = contents.join("Info.plist");
        write_private(
            &info,
            b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
<plist version=\"1.0\"><dict>\n\
  <key>CFBundleIdentifier</key><string>com.semwright.runtime.sandbox</string>\n\
  <key>CFBundleExecutable</key><string>SemwrightSandbox</string>\n\
  <key>CFBundleName</key><string>SemwrightSandbox</string>\n\
  <key>CFBundlePackageType</key><string>APPL</string>\n\
</dict></plist>\n",
            0o600,
        )?;

        let (materialized_mounts, materialized_tools) = materialize(spec)?;
        let mut read_only = BTreeSet::new();
        let mut read_write = BTreeSet::new();
        read_only.insert(exception_path(&spec.staged_executable)?);
        for mount in &materialized_mounts {
            if mount.read_only {
                read_only.insert(mount.path.clone());
            } else {
                read_write.insert(mount.path.clone());
            }
        }
        for tool in &materialized_tools {
            read_only.insert(tool.path.clone());
        }
        read_write.insert(exception_path(&home)?);
        read_write.insert(exception_path(&tmp)?);

        let parent_entitlements = root.join("parent.entitlements");
        write_private(
            &parent_entitlements,
            entitlements(&read_only, &read_write, spec.network).as_bytes(),
            0o600,
        )?;
        let child_entitlements_path = root.join("child.entitlements");
        write_private(&child_entitlements_path, child_entitlements(), 0o600)?;

        if let Err(error) = codesign(
            &exec_helper,
            &child_entitlements_path,
            "com.semwright.runtime.sandbox.exec",
        )
        .and_then(|_| codesign(&app, &parent_entitlements, "com.semwright.runtime.sandbox"))
        {
            let _ = std::fs::remove_dir_all(&root);
            return Err(error);
        }

        let typed_args = spec
            .environment
            .iter()
            .any(|(name, value)| name == SANDBOX_HOST_TOOL_TYPED_ARGS_ENV && value == "1");
        let resolved_args = resolve_host_tool_args(
            &spec.args,
            &materialized_mounts,
            &materialized_tools,
            typed_args,
        )?;

        let requested_cwd = spec
            .environment
            .iter()
            .find(|(name, _)| name == SANDBOX_HOST_TOOL_CWD_ENV)
            .map(|(_, value)| PathBuf::from(value));
        let cwd = if let Some(requested) = requested_cwd {
            let canonical = std::fs::canonicalize(&requested).map_err(|_| {
                Error::new(
                    ErrorCode::SandboxDenied,
                    "macOS Host-tool working directory is unavailable",
                )
            })?;
            let allowed = spec
                .mounts
                .iter()
                .filter(|mount| mount.class == MountClass::Workspace)
                .map(|mount| std::fs::canonicalize(&mount.source))
                .collect::<std::io::Result<Vec<_>>>()?;
            if !allowed.iter().any(|root| canonical.starts_with(root)) {
                let _ = std::fs::remove_dir_all(&root);
                return Err(Error::new(
                    ErrorCode::SandboxDenied,
                    "macOS Host-tool working directory exceeds selected workspace mounts",
                ));
            }
            canonical
        } else {
            tmp.clone()
        };

        let mut environment = BTreeMap::<String, String>::new();
        for (name, value) in &spec.environment {
            if matches!(
                name.as_str(),
                SANDBOX_HOST_TOOL_CHILD_ENV
                    | SANDBOX_HOST_TOOL_CWD_ENV
                    | SANDBOX_HOST_TOOL_TYPED_ARGS_ENV
            ) {
                continue;
            }
            environment.insert(name.clone(), value.clone());
        }
        environment.insert("HOME".into(), home.to_string_lossy().into_owned());
        environment.insert("TMPDIR".into(), tmp.to_string_lossy().into_owned());
        environment.insert("PATH".into(), "/usr/bin:/bin".into());
        environment.insert("LANG".into(), "C.UTF-8".into());
        environment.insert(CWD_ENV.into(), cwd.to_string_lossy().into_owned());

        if matches!(spec.kind, SandboxKind::Driver | SandboxKind::ExternalMcp) {
            environment.insert(
                "XDG_CACHE_HOME".into(),
                tmp.join("cache").to_string_lossy().into_owned(),
            );
            environment.insert(
                "XDG_CONFIG_HOME".into(),
                tmp.join("config").to_string_lossy().into_owned(),
            );
            environment.insert(
                "XDG_DATA_HOME".into(),
                tmp.join("data").to_string_lossy().into_owned(),
            );
        }
        if spec.kind == SandboxKind::Driver {
            environment.insert(
                "SEMWRIGHT_DRIVER_SANDBOX".into(),
                "macos-app-sandbox-v1".into(),
            );
            environment.insert(
                SANDBOX_MOUNTS_ENV.into(),
                encode_materialized_mounts(&materialized_mounts)?,
            );
            if !materialized_tools.is_empty() {
                environment.insert(
                    SANDBOX_TOOLS_ENV.into(),
                    encode_materialized_tools(&materialized_tools)?,
                );
            }
        }
        if let Some(limits) = &spec.limits {
            environment.insert(NOFILE_ENV.into(), limits.open_files.to_string());
            environment.insert(NPROC_ENV.into(), limits.processes.to_string());
            environment.insert(CPU_ENV.into(), limits.cpu_seconds.to_string());
            environment.insert(AS_ENV.into(), limits.address_space_bytes.to_string());
            environment.insert(FSIZE_ENV.into(), limits.file_size_bytes.to_string());
        }

        let mut command = Command::new(&app_exec);
        command
            .arg(PARENT_ARG)
            .arg(&exec_helper)
            .arg(&spec.staged_executable)
            .args(resolved_args)
            .env_clear()
            .envs(environment)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(false);
        command.as_std_mut().process_group(0);

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(_) => {
                let _ = std::fs::remove_dir_all(&root);
                return Err(Error::new(
                    ErrorCode::SandboxDenied,
                    "macOS App Sandbox parent failed to start",
                ));
            }
        };
        let process_group = child.id().ok_or_else(|| {
            let _ = child.start_kill();
            let _ = std::fs::remove_dir_all(&root);
            Error::new(ErrorCode::SandboxDenied, "macOS sandbox parent has no PID")
        })? as i32;
        let (stdin, stdout) = match (child.stdin.take(), child.stdout.take()) {
            (Some(stdin), Some(stdout)) => (
                Box::new(stdin) as SandboxStdin,
                Box::new(stdout) as SandboxStdout,
            ),
            _ => {
                let _ = child.start_kill();
                return Err(Error::new(
                    ErrorCode::ProtocolMismatch,
                    "macOS sandbox child must expose piped stdin and stdout",
                ));
            }
        };

        Ok(SandboxProcess::from_parts(
            stdin,
            stdout,
            Box::new(MacSandboxChild {
                child,
                process_group,
                root: root_guard.into_path(),
                exit_code: None,
            }),
        ))
    }

    fn available(&self, helper: &Path) -> bool {
        Path::new(CODESIGN).is_file() && helper.is_file()
    }

    fn mechanism(&self) -> &'static str {
        "app-sandbox+inherit+exec-pinned-v1"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn native_fixture() -> Vec<u8> {
        let cpu = match std::env::consts::ARCH {
            "aarch64" => 0x0100000cu32,
            "x86_64" => 0x01000007u32,
            other => panic!("unsupported test architecture: {other}"),
        };
        let mut bytes = vec![0u8; 32];
        bytes[..4].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]);
        bytes[4..8].copy_from_slice(&cpu.to_le_bytes());
        bytes[12..16].copy_from_slice(&2u32.to_le_bytes());
        bytes
    }

    #[test]
    fn provider_and_secondary_tools_share_macho_trust_checks() {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("fixture");
        let bytes = native_fixture();
        std::fs::write(&executable, &bytes).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o500)).unwrap();
        let digest = format!("{:x}", Sha256::digest(&bytes));

        assert_eq!(MacVerifier.verify(&executable, &digest).unwrap(), bytes);
        assert_eq!(
            verify_sealed_tool_executable(&executable, &digest).unwrap(),
            native_fixture()
        );
        let error = verify_sealed_tool_executable(&executable, &"0".repeat(64)).unwrap_err();
        assert_eq!(error.code, ErrorCode::PermissionDenied);
    }

    #[test]
    fn secondary_tool_budget_is_distinct_and_direct_command_stays_disabled() {
        assert_eq!(MAX_PROVIDER_EXECUTABLE_BYTES, 64 * 1024 * 1024);
        assert_eq!(MAX_SEALED_TOOL_EXECUTABLE_BYTES, 256 * 1024 * 1024);
        assert_eq!(MacSandbox.mechanism(), "app-sandbox+inherit+exec-pinned-v1");
        let spec = SandboxSpec {
            kind: semwright_platform_api::launch::SandboxKind::Driver,
            staged_executable: "/tmp/driver".into(),
            helper: "/tmp/helper".into(),
            mounts: vec![],
            args: vec![],
            environment: vec![],
            sealed_tools: vec![],
            network: false,
            limits: Some(semwright_platform_api::launch::ResourceLimits {
                open_files: 64,
                processes: 16,
                cpu_seconds: 20,
                address_space_bytes: 512 * 1024 * 1024,
                file_size_bytes: 16 * 1024 * 1024,
            }),
        };
        let error = match MacSandbox.command(&spec) {
            Err(error) => error,
            Ok(_) => panic!("macOS sandbox command path must stay unavailable"),
        };
        assert_eq!(error.code, ErrorCode::SandboxDenied);
    }
}
