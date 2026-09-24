//! Single-threaded pre-exec Landlock helper. Never run restrictions on a Tokio worker.
use landlock::{
    ABI, Access, AccessFs, CompatLevel, Compatible, PathBeneath, PathFd, Ruleset, RulesetAttr,
    RulesetCreatedAttr, RulesetStatus,
};
use std::{collections::BTreeSet, os::unix::process::CommandExt, path::Path, process::Command};

pub fn main() {
    if run().is_err() {
        eprintln!("SandboxDenied: required isolation could not be established");
        std::process::exit(125);
    }
}

fn bounded_limit(
    value: Option<String>,
    minimum: u64,
    maximum: u64,
) -> Result<u64, Box<dyn std::error::Error>> {
    let value = value
        .ok_or("sandbox resource limit missing")?
        .parse::<u64>()?;
    if !(minimum..=maximum).contains(&value) {
        return Err("sandbox resource limit outside hard bounds".into());
    }
    Ok(value)
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mut writable = vec!["/tmp".to_owned(), "/dev/shm".to_owned()];
    let mut readable: Vec<(String, bool)> = Vec::new();
    let mut seen = BTreeSet::new();
    let mut nofile = 128u64;
    let mut nproc = 32u64;
    let mut cpu = 20u64;
    let mut address_space = 536_870_912u64;
    let mut file_size = 16_777_216u64;

    loop {
        let argument = args.next().ok_or("sandbox terminator missing")?;
        match argument.as_str() {
            "--write-root" => {
                let path = args.next().ok_or("write root missing")?;
                if !path.starts_with("/workspace/") || path.contains("..") || path.contains('\0') {
                    return Err("invalid sandbox root".into());
                }
                writable.push(path);
            }
            "--read-root" => {
                let path = args.next().ok_or("read root missing")?;
                if !(path.starts_with("/workspace/") || path.starts_with("/etc/"))
                    || path.contains("..")
                    || path.contains('\0')
                {
                    return Err("invalid sandbox read root".into());
                }
                readable.push((path, false));
            }
            "--exec-root" => {
                let path = args.next().ok_or("exec root missing")?;
                if !path.starts_with("/workspace/") || path.contains("..") || path.contains('\0') {
                    return Err("invalid sandbox exec root".into());
                }
                readable.push((path, true));
            }
            "--limit-nofile" => {
                if !seen.insert(argument.clone()) {
                    return Err("duplicate sandbox resource limit".into());
                }
                nofile = bounded_limit(args.next(), 32, 1024)?;
            }
            "--limit-nproc" => {
                if !seen.insert(argument.clone()) {
                    return Err("duplicate sandbox resource limit".into());
                }
                nproc = bounded_limit(args.next(), 8, 256)?;
            }
            "--limit-cpu" => {
                if !seen.insert(argument.clone()) {
                    return Err("duplicate sandbox resource limit".into());
                }
                cpu = bounded_limit(args.next(), 5, 300)?;
            }
            "--limit-as" => {
                if !seen.insert(argument.clone()) {
                    return Err("duplicate sandbox resource limit".into());
                }
                address_space = bounded_limit(args.next(), 134_217_728, 17_179_869_184)?;
            }
            "--limit-fsize" => {
                if !seen.insert(argument.clone()) {
                    return Err("duplicate sandbox resource limit".into());
                }
                file_size = bounded_limit(args.next(), 1_048_576, 1_073_741_824)?;
            }
            "--" => break,
            _ => return Err("invalid sandbox arguments".into()),
        }
    }

    let executable = args.next().ok_or("executable missing")?;
    if executable != "/plugin/bin" || args.next().is_some() {
        return Err("sandbox executable is fixed".into());
    }
    // SAFETY: prctl with PR_SET_NO_NEW_PRIVS takes integer options only.
    if unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0 {
        return Err("no_new_privs failed".into());
    }
    for (resource, limit) in [
        (libc::RLIMIT_NOFILE, nofile),
        (libc::RLIMIT_NPROC, nproc),
        (libc::RLIMIT_CPU, cpu),
        (libc::RLIMIT_AS, address_space),
        (libc::RLIMIT_FSIZE, file_size),
        (libc::RLIMIT_CORE, 0),
    ] {
        let limits = libc::rlimit {
            rlim_cur: limit as libc::rlim_t,
            rlim_max: limit as libc::rlim_t,
        };
        // SAFETY: setrlimit synchronously reads a live repr(C) rlimit pointer.
        if unsafe { libc::setrlimit(resource, &limits) } != 0 {
            return Err("resource limit failed".into());
        }
    }

    let abi = ABI::V3;
    let all = AccessFs::from_all(abi);
    // landlock::AccessFs::from_read() includes Execute.  Driver mounts model
    // execution as a separate, explicit capability, so never use from_read()
    // for generic data roots.
    let read_only = AccessFs::ReadFile | AccessFs::ReadDir;
    let read_exec = read_only | AccessFs::Execute;
    let read_write_noexec = read_only | AccessFs::from_write(abi);
    let mut ruleset = Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(all)?
        .create()?;

    // System executables and ELF interpreters remain executable.  Everything
    // else is non-executable unless a narrower rule below explicitly grants it.
    for (path, access) in [
        ("/usr", read_exec),
        ("/lib", read_exec),
        ("/lib64", read_exec),
        ("/etc", read_only),
        ("/plugin", read_only),
        ("/workspace", read_only),
        ("/dev", read_only),
        ("/proc", read_only),
    ] {
        if Path::new(path).exists() {
            ruleset = ruleset.add_rule(PathBeneath::new(PathFd::new(path)?, access))?;
        }
    }

    // The staged, digest-verified driver is the only executable under /plugin.
    ruleset = ruleset.add_rule(PathBeneath::new(
        PathFd::new("/plugin/bin")?,
        AccessFs::Execute | AccessFs::ReadFile,
    ))?;

    // Bind mounts are separate Landlock hierarchies.  Data mounts stay
    // non-executable; only an owner-approved execute mount gets Execute.
    for (path, execute) in readable {
        let is_dir = Path::new(&path).is_dir();
        let access = match (is_dir, execute) {
            (true, true) => read_exec,
            (false, true) => AccessFs::Execute | AccessFs::ReadFile,
            (true, false) => read_only,
            (false, false) => AccessFs::ReadFile.into(),
        };
        ruleset = ruleset.add_rule(PathBeneath::new(PathFd::new(&path)?, access))?;
    }

    // Rust's Stdio::null() opens /dev/null for writing when a child redirects
    // stdout or stderr. Keep every host device node non-writable.
    ruleset = ruleset.add_rule(PathBeneath::new(
        PathFd::new("/dev/null")?,
        AccessFs::ReadFile | AccessFs::WriteFile,
    ))?;

    // Bubblewrap creates these as fresh tmpfs instances inside each sandbox.
    // Browser subprocesses need writable temporary/shared-memory storage, but
    // neither location needs Execute and neither exposes host filesystem data.
    for path in ["/tmp", "/dev/shm"] {
        if Path::new(path).is_dir() {
            ruleset = ruleset.add_rule(PathBeneath::new(
                PathFd::new(path)?,
                read_write_noexec,
            ))?;
        }
    }

    // Writable driver data is deliberately non-executable.  Platform Mount
    // validation already forbids write+execute; Landlock must enforce the same
    // contract rather than accidentally granting Execute through from_all().
    for path in writable {
        ruleset = ruleset.add_rule(PathBeneath::new(
            PathFd::new(path)?,
            read_write_noexec,
        ))?;
    }
    let status = ruleset.restrict_self()?;
    if status.ruleset != RulesetStatus::FullyEnforced {
        return Err("Landlock was not fully enforced".into());
    }
    let error = Command::new(executable).exec();
    Err(Box::new(error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_bounds_match_driver_manifest_hard_limits() {
        assert_eq!(bounded_limit(Some("32".into()), 32, 1024).unwrap(), 32);
        assert!(bounded_limit(Some("31".into()), 32, 1024).is_err());
        assert!(bounded_limit(Some("1025".into()), 32, 1024).is_err());
        assert!(bounded_limit(Some("not-a-number".into()), 32, 1024).is_err());
        assert_eq!(
            bounded_limit(Some("17179869184".into()), 134_217_728, 17_179_869_184).unwrap(),
            17_179_869_184
        );
        assert!(
            bounded_limit(Some("17179869185".into()), 134_217_728, 17_179_869_184).is_err()
        );
    }
}
