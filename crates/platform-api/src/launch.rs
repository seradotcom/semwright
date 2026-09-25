//! Ownership, digest, format and isolation are separate checks. No signature grants a sandbox.
use async_trait::async_trait;
use semwright_types::{Error, ErrorCode, Result};
use std::path::{Path, PathBuf};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    process::{Child, Command},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgramFormat {
    Elf,
    MachO,
    Pe,
}

pub trait ExecutableVerifier: Send + Sync {
    fn verify(&self, path: &Path, sha256: &str) -> Result<Vec<u8>>;
}

/// Logical mount identity. The platform host materializes its own filesystem namespace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MountClass {
    Workspace,
    SystemConfig,
}

#[derive(Clone, Debug)]
pub struct Mount {
    pub source: PathBuf,
    pub class: MountClass,
    pub logical_name: String,
    pub read_only: bool,
}
impl Mount {
    pub fn validate(&self) -> Result<()> {
        if !self.source.is_absolute() {
            return Err(Error::invalid("Mount source must be absolute"));
        }
        let p = Path::new(&self.logical_name);
        super::filesystem::validate_relative_path(p)?;
        if self.logical_name.len() > 255 {
            return Err(Error::invalid("Logical mount name exceeds budget"));
        }
        if self.class == MountClass::SystemConfig && !self.read_only {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "System-config mounts must be read-only",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct ResourceLimits {
    pub open_files: u64,
    pub processes: u64,
    pub cpu_seconds: u64,
    pub address_space_bytes: u64,
    pub file_size_bytes: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SandboxKind {
    Driver,
    Plugin,
    ExternalMcp,
}
#[derive(Clone, Debug)]
pub struct SandboxSpec {
    pub kind: SandboxKind,
    pub staged_executable: PathBuf,
    pub helper: PathBuf,
    pub mounts: Vec<Mount>,
    /// Arguments are passed directly to the staged executable; no shell is involved.
    pub args: Vec<String>,
    pub network: bool,
    pub limits: Option<ResourceLimits>,
}
impl SandboxSpec {
    pub fn validate(&self) -> Result<()> {
        if !self.staged_executable.is_absolute() || !self.helper.is_absolute() {
            return Err(Error::invalid(
                "Pinned absolute executable and helper required",
            ));
        }
        let mut names = std::collections::BTreeSet::new();
        for m in &self.mounts {
            m.validate()?;
            if !names.insert((m.class as u8, m.logical_name.clone())) {
                return Err(Error::invalid("Duplicate logical sandbox mount"));
            }
        }
        if self.args.len() > 64
            || self
                .args
                .iter()
                .any(|arg| arg.len() > 4096 || arg.contains('\0'))
        {
            return Err(Error::invalid("Sandbox executable arguments exceed bounds"));
        }
        if matches!(self.kind, SandboxKind::Driver | SandboxKind::ExternalMcp)
            && self.limits.is_none()
        {
            return Err(Error::invalid(
                "Driver and external MCP resource limits are required",
            ));
        }
        Ok(())
    }
}

pub type SandboxStdin = Box<dyn AsyncWrite + Send + Unpin>;
pub type SandboxStdout = Box<dyn AsyncRead + Send + Unpin>;

#[async_trait]
trait SandboxChildControl: Send {
    fn id(&self) -> Option<u32>;
    async fn kill(&mut self) -> Result<()>;
    async fn wait(&mut self) -> Result<()>;
}

struct TokioSandboxChild {
    child: Child,
}

#[async_trait]
impl SandboxChildControl for TokioSandboxChild {
    fn id(&self) -> Option<u32> {
        self.child.id()
    }

    async fn kill(&mut self) -> Result<()> {
        self.child.kill().await.map_err(Into::into)
    }

    async fn wait(&mut self) -> Result<()> {
        self.child.wait().await.map(|_| ()).map_err(Into::into)
    }
}

/// A child process whose isolation was established by the platform before this value is returned.
///
/// The broker-side hosts receive only bounded stdio plus lifecycle operations. Native process
/// handles, tokens, Job Objects and sandbox authorities remain owned by the platform backend.
pub struct SandboxProcess {
    stdin: Option<SandboxStdin>,
    stdout: Option<SandboxStdout>,
    control: Box<dyn SandboxChildControl>,
}

impl SandboxProcess {
    pub fn from_tokio_child(mut child: Child) -> Result<Self> {
        let stdin = child
            .stdin
            .take()
            .map(|value| Box::new(value) as SandboxStdin);
        let stdout = child
            .stdout
            .take()
            .map(|value| Box::new(value) as SandboxStdout);
        let (Some(stdin), Some(stdout)) = (stdin, stdout) else {
            let _ = child.start_kill();
            return Err(Error::new(
                ErrorCode::ProtocolMismatch,
                "Sandbox child must expose piped stdin and stdout",
            ));
        };
        Ok(Self {
            stdin: Some(stdin),
            stdout: Some(stdout),
            control: Box::new(TokioSandboxChild { child }),
        })
    }

    pub fn take_stdin(&mut self) -> Result<SandboxStdin> {
        self.stdin.take().ok_or_else(|| {
            Error::new(
                ErrorCode::ProtocolMismatch,
                "Sandbox child stdin was already taken",
            )
        })
    }

    pub fn take_stdout(&mut self) -> Result<SandboxStdout> {
        self.stdout.take().ok_or_else(|| {
            Error::new(
                ErrorCode::ProtocolMismatch,
                "Sandbox child stdout was already taken",
            )
        })
    }

    pub fn id(&self) -> Option<u32> {
        self.control.id()
    }

    pub async fn kill(&mut self) -> Result<()> {
        self.control.kill().await
    }

    pub async fn wait(&mut self) -> Result<()> {
        self.control.wait().await
    }
}

pub trait SandboxLauncher: Send + Sync {
    fn command(&self, spec: &SandboxSpec) -> Result<Command>;

    /// Spawn a process only after the platform-specific containment boundary is established.
    ///
    /// Linux keeps using the existing Bubblewrap + Landlock command path through this default.
    /// Platforms that require pre-first-instruction setup (for example Windows) override this
    /// method and must not fall back to an ordinary direct process spawn.
    fn spawn(&self, spec: &SandboxSpec) -> Result<SandboxProcess> {
        let mut command = self.command(spec)?;
        let child = command.spawn().map_err(|_| {
            Error::new(
                ErrorCode::SandboxDenied,
                "Platform sandbox process failed to start",
            )
        })?;
        SandboxProcess::from_tokio_child(child)
    }

    fn available(&self, helper: &Path) -> bool;
    fn mechanism(&self) -> &'static str;
    fn diagnostics(&self, helper: &Path) -> serde_json::Value {
        serde_json::json!({"available":self.available(helper),"helper_present":helper.is_file(),"mechanism":self.mechanism()})
    }
}
