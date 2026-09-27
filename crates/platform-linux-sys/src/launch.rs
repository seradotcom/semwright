use semwright_platform_api::launch::{
    ExecutableVerifier, MaterializedMount, Mount, MountClass, SANDBOX_MOUNTS_ENV, SandboxKind,
    SandboxLauncher, SandboxSpec, encode_materialized_mounts,
};
use semwright_types::{Error, ErrorCode, Result};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
    process::Stdio,
};
use tokio::process::Command;

const MAX_PROVIDER_EXECUTABLE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SEALED_TOOL_EXECUTABLE_BYTES: u64 = 256 * 1024 * 1024;

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
            // Materialize the Host-verified sealed bytes directly into the private
            // sandbox root. The child receives no write/remove/create Landlock rights
            // for this path, so the executable remains immutable after policy install.
            // Avoid --ro-bind-data here: Bubblewrap unlinks its backing tempfile after
            // bind-mounting it, which can make later execve() resolve as ENOENT under
            // deleted-file mediation on Ubuntu/AppArmor.
            p.args(["--perms", "0500", "--file"])
                .arg(tool.fd.to_string())
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
        // Only Semwright application drivers consume the logical mount table through
        // driver-sdk helpers. Do not expose internal mount topology to plugins or
        // external MCP children that do not need this authority-bearing metadata.
        if s.kind == SandboxKind::Driver {
            let mount_table = encode_materialized_mounts(
                &s.mounts
                    .iter()
                    .map(|mount| {
                        Ok(MaterializedMount {
                            class: mount.class,
                            logical_name: mount.logical_name.clone(),
                            path: materialized_destination(mount)?,
                            read_only: mount.read_only,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?,
            )?;
            p.arg("--setenv").arg(SANDBOX_MOUNTS_ENV).arg(mount_table);
        }
        for (name, value) in &s.environment {
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
        p.args(["--chdir", "/tmp", "--", "/plugin/sandbox"]);
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
            .args(&s.args)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        Ok(p)
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
    fn sealed_tool_budget_is_distinct_from_provider_budget() {
        assert_eq!(MAX_PROVIDER_EXECUTABLE_BYTES, 64 * 1024 * 1024);
        assert_eq!(MAX_SEALED_TOOL_EXECUTABLE_BYTES, 256 * 1024 * 1024);
    }
}
