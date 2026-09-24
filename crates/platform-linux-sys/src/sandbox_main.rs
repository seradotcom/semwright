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
    let mut writable = vec!["/tmp".to_owned()];
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
                address_space = bounded_limit(args.next(), 134_217_728, 4_294_967_296)?;
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
    let read = AccessFs::from_read(abi);
    let mut ruleset = Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(all)?
        .create()?;
    for path in [
        "/usr",
        "/lib",
        "/lib64",
        "/etc",
        "/plugin",
        "/workspace",
        "/dev",
        "/proc",
    ] {
        if Path::new(path).exists() {
            ruleset = ruleset.add_rule(PathBeneath::new(PathFd::new(path)?, read))?;
        }
    }
    // Bind mounts are separate Landlock hierarchies. Data mounts get read-only
    // rights; execute is added only for an explicitly attested executable mount.
    for (path, execute) in readable {
        let is_dir = Path::new(&path).is_dir();
        let access = match (is_dir, execute) {
            (true, true) => read,
            (false, true) => AccessFs::Execute | AccessFs::ReadFile,
            (true, false) => AccessFs::ReadFile | AccessFs::ReadDir,
            (false, false) => AccessFs::ReadFile.into(),
        };
        ruleset = ruleset.add_rule(PathBeneath::new(PathFd::new(&path)?, access))?;
    }
    // Child processes commonly redirect stdout/stderr through Stdio::null(), which opens
    // /dev/null for writing. Grant that single device write access while keeping the rest
    // of /dev under the read-only rule above.
    ruleset = ruleset.add_rule(PathBeneath::new(
        PathFd::new("/dev/null")?,
        AccessFs::ReadFile | AccessFs::WriteFile,
    ))?;
    for path in writable {
        ruleset = ruleset.add_rule(PathBeneath::new(PathFd::new(path)?, all))?;
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
            bounded_limit(Some("4294967296".into()), 134_217_728, 4_294_967_296).unwrap(),
            4_294_967_296
        );
        assert!(bounded_limit(Some("4294967297".into()), 134_217_728, 4_294_967_296).is_err());
    }
}
