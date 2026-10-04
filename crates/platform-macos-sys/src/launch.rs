use semwright_platform_api::launch::{ExecutableVerifier, SandboxLauncher, SandboxSpec};
use semwright_types::{Error, ErrorCode, Result};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
};
use tokio::process::Command;

const MAX_PROVIDER_EXECUTABLE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SEALED_TOOL_EXECUTABLE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_APPLICATION_EXECUTABLE_BYTES: u64 = 512 * 1024 * 1024;

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
        || hex::encode(Sha256::digest(&bytes)) != digest.to_ascii_lowercase()
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

pub fn verify_application_executable(path: &Path, digest: &str) -> Result<()> {
    verify_executable_bounded(path, digest, MAX_APPLICATION_EXECUTABLE_BYTES).map(|_| ())
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
        let digest = hex::encode(Sha256::digest(&bytes));

        assert_eq!(MacVerifier.verify(&executable, &digest).unwrap(), bytes);
        assert_eq!(
            verify_sealed_tool_executable(&executable, &digest).unwrap(),
            native_fixture()
        );
        let error = verify_sealed_tool_executable(&executable, &"0".repeat(64)).unwrap_err();
        assert_eq!(error.code, ErrorCode::PermissionDenied);
    }

    #[test]
    fn secondary_tool_budget_is_distinct_but_sandbox_stays_fail_closed() {
        assert_eq!(MAX_PROVIDER_EXECUTABLE_BYTES, 64 * 1024 * 1024);
        assert_eq!(MAX_SEALED_TOOL_EXECUTABLE_BYTES, 256 * 1024 * 1024);
        assert_eq!(MAX_APPLICATION_EXECUTABLE_BYTES, 512 * 1024 * 1024);
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
        assert!(!MacSandbox.available(Path::new("/tmp/helper")));
        let error = match MacSandbox.command(&spec) {
            Err(error) => error,
            Ok(_) => panic!("macOS sandbox unexpectedly became available"),
        };
        assert_eq!(error.code, ErrorCode::SandboxDenied);
    }
}
