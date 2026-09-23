use semwright_platform_api::launch::{ExecutableVerifier, SandboxLauncher, SandboxSpec};
use semwright_types::{Error, ErrorCode, Result};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
};
use tokio::process::Command;
pub struct MacVerifier;
impl ExecutableVerifier for MacVerifier {
    fn verify(&self, path: &Path, digest: &str) -> Result<Vec<u8>> {
        let mut f = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(path)?;
        let m = f.metadata()?;
        // SAFETY: getuid has no memory/lifetime requirements.
        let uid = unsafe { libc::getuid() };
        if !m.is_file()
            || m.nlink() != 1
            || m.len() > 67_108_864
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
            .take(67_108_865)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 67_108_864
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
}
/// Even a valid Developer ID signature does not establish filesystem/network confinement.
pub struct MacSandbox;
impl SandboxLauncher for MacSandbox {
    fn command(&self, spec: &SandboxSpec) -> Result<Command> {
        spec.validate()?;
        Err(Error::new(
            ErrorCode::SandboxDenied,
            "Arbitrary driver/plugin isolation has no implemented supported macOS enforcement; execution is disabled",
        ))
    }
    fn available(&self, _: &Path) -> bool {
        false
    }
    fn mechanism(&self) -> &'static str {
        "unavailable:macos-arbitrary-child-isolation"
    }
}
