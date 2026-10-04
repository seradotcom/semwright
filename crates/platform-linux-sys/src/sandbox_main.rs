//! Single-threaded pre-exec Landlock helper. Never run restrictions on a Tokio worker.
use landlock::{
    ABI, Access, AccessFs, CompatLevel, Compatible, PathBeneath, PathFd, Ruleset, RulesetAttr,
    RulesetCreatedAttr, RulesetStatus,
};
use std::{
    collections::BTreeSet,
    fs::File,
    io::Write,
    os::fd::{AsRawFd, FromRawFd},
    os::unix::{fs::FileTypeExt, process::CommandExt},
    path::Path,
    process::Command,
};

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

fn valid_read_root(path: &str) -> bool {
    let secret = path
        .strip_prefix("/run/secrets/")
        .is_some_and(|name| !name.is_empty() && !name.contains('/'));
    (path.starts_with("/workspace/") || path.starts_with("/etc/") || secret)
        && !path.contains("..")
        && !path.contains('\0')
}

fn valid_exec_root(path: &str) -> bool {
    let sealed_tool = path
        .strip_prefix("/plugin/tools/")
        .is_some_and(|name| !name.is_empty() && !name.contains('/'));
    (path.starts_with("/workspace/") || sealed_tool) && !path.contains("..") && !path.contains('\0')
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1).peekable();
    if args
        .peek()
        .is_some_and(|arg| arg == "--nvidia-native-child")
    {
        args.next();
        return run_nvidia_native_child(args);
    }
    let mut writable = vec!["/tmp".to_owned(), "/dev/shm".to_owned()];
    let mut readable: Vec<(String, bool)> = Vec::new();
    let mut seen = BTreeSet::new();
    let mut nofile = 128u64;
    let mut nproc = 32u64;
    let mut cpu = 20u64;
    let mut address_space = 536_870_912u64;
    let mut file_size = 16_777_216u64;
    let mut nvidia_gpu = false;

    loop {
        let argument = args.next().ok_or("sandbox terminator missing")?;
        match argument.as_str() {
            "--nvidia-gpu" => {
                if nvidia_gpu {
                    return Err("duplicate NVIDIA compute grant".into());
                }
                nvidia_gpu = true;
            }
            "--write-root" => {
                let path = args.next().ok_or("write root missing")?;
                if !path.starts_with("/workspace/") || path.contains("..") || path.contains('\0') {
                    return Err("invalid sandbox root".into());
                }
                writable.push(path);
            }
            "--read-root" => {
                let path = args.next().ok_or("read root missing")?;
                if !valid_read_root(&path) {
                    return Err("invalid sandbox read root".into());
                }
                readable.push((path, false));
            }
            "--exec-root" => {
                let path = args.next().ok_or("exec root missing")?;
                if !valid_exec_root(&path) {
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
                cpu = bounded_limit(args.next(), 5, 86_400)?;
            }
            "--limit-as" => {
                if !seen.insert(argument.clone()) {
                    return Err("duplicate sandbox resource limit".into());
                }
                address_space = bounded_limit(args.next(), 134_217_728, 8_589_934_592)?;
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
    if executable != "/plugin/bin" {
        return Err("sandbox executable is fixed".into());
    }
    let executable_args = args.collect::<Vec<_>>();
    if executable_args.len() > 64
        || executable_args
            .iter()
            .any(|arg| arg.len() > 4096 || arg.contains('\0'))
    {
        return Err("sandbox executable arguments exceed bounds".into());
    }
    validate_nvidia_address_space(address_space, nvidia_gpu)?;
    apply_limits(nofile, nproc, cpu, address_space, file_size)?;
    if nvidia_gpu {
        if std::env::var("SEMWRIGHT_NVIDIA_GPU").as_deref() != Ok("1") {
            return Err("NVIDIA handoff requires the platform compute grant".into());
        }
        validate_nvidia_runner(&writable, &readable, &executable_args)?;
        let plan = GpuPolicy {
            writable,
            readable,
            limits: [nofile, nproc, cpu, address_space, file_size],
        };
        let policy = create_nvidia_policy_fd(&encode_nvidia_policy(&plan)?)?;
        // This parent executes only the reviewed, Host-pinned session runner.
        // Bubblewrap mounts/capabilities/namespaces and limits are already active.
        // The final Landlock policy is applied by the fixed native-child helper;
        // applying it here would irreversibly deny future child-thread comm writes.
        let error = Command::new(executable)
            .args(executable_args)
            .env(NVIDIA_POLICY_FD_ENV, policy.as_raw_fd().to_string())
            .exec();
        return Err(Box::new(error));
    }
    install_landlock(writable, readable, false, None)?;
    let error = Command::new(executable).args(executable_args).exec();
    Err(Box::new(error))
}

fn validate_nvidia_address_space(
    value: u64,
    nvidia_gpu: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let maximum = if nvidia_gpu {
        8_589_934_592
    } else {
        4_294_967_296
    };
    if !(134_217_728..=maximum).contains(&value) {
        return Err("address space exceeds the selected CPU/GPU sandbox bound".into());
    }
    Ok(())
}

fn apply_limits(
    nofile: u64,
    nproc: u64,
    cpu: u64,
    address_space: u64,
    file_size: u64,
) -> Result<(), Box<dyn std::error::Error>> {
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

    Ok(())
}

fn install_landlock(
    writable: Vec<String>,
    readable: Vec<(String, bool)>,
    nvidia_gpu: bool,
    own_task_root: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let abi = ABI::V3;
    let all = AccessFs::from_all(abi);
    // AccessFs::from_read() includes Execute. Driver mount execution is an
    // explicit capability, so generic data roots must use a no-exec read set.
    let read_only = AccessFs::ReadFile | AccessFs::ReadDir;
    let read_exec = read_only | AccessFs::Execute;
    let read_write_noexec = read_only | AccessFs::from_write(abi);
    let mut ruleset = Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(all)?
        .create()?;

    // System binaries and ELF interpreters remain executable. Other broad
    // roots are readable only; /workspace permissions come solely from the
    // explicit SandboxSpec mounts below.
    for (path, access) in [
        ("/usr", read_exec),
        ("/lib", read_exec),
        ("/lib64", read_exec),
        ("/etc", read_only),
        ("/plugin", read_only),
        ("/run/secrets", read_only),
        ("/dev", read_only),
        ("/proc", read_only),
    ] {
        if Path::new(path).exists() {
            ruleset = ruleset.add_rule(PathBeneath::new(PathFd::new(path)?, access))?;
        }
    }

    // The staged, digest-verified driver is the only executable below /plugin.
    ruleset = ruleset.add_rule(PathBeneath::new(
        PathFd::new("/plugin/bin")?,
        AccessFs::Execute | AccessFs::ReadFile,
    ))?;

    // Bind mounts are separate Landlock hierarchies. Read-only data mounts do
    // not get Execute; only a manifest mount with execute=true receives it.
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

    // Stdio::null() opens /dev/null for writing. Keep every other device node
    // non-writable, apart from the private /dev/shm tmpfs admitted below.
    ruleset = ruleset.add_rule(PathBeneath::new(
        PathFd::new("/dev/null")?,
        AccessFs::ReadFile | AccessFs::WriteFile,
    ))?;
    if nvidia_gpu {
        for (index, path) in semwright_platform_api::launch::NVIDIA_COMPUTE_DEVICE_PATHS
            .iter()
            .enumerate()
        {
            let metadata = match std::fs::symlink_metadata(path) {
                Ok(metadata) => metadata,
                Err(error) if index == 3 && error.kind() == std::io::ErrorKind::NotFound => {
                    continue;
                }
                Err(error) => return Err(error.into()),
            };
            // The Linux launcher already pinned a root-owned HOST inode with O_NOFOLLOW.
            // UID 0 can map to the overflow UID inside Bubblewrap's user namespace.
            if !metadata.file_type().is_char_device() {
                return Err("NVIDIA compute node is not a character device".into());
            }
            ruleset = ruleset.add_rule(PathBeneath::new(
                PathFd::new(path)?,
                AccessFs::ReadFile | AccessFs::WriteFile,
            ))?;
        }
    }

    if let Some(path) = own_task_root {
        if !nvidia_gpu {
            return Err("own-thread proc writes require NVIDIA native-child policy".into());
        }
        // CUDA names threads created after initialization starts. A single comm
        // inode cannot cover those future threads. This grants ONLY WriteFile and
        // Truncate under this native process's task directory; it includes other
        // writable own-thread metadata (mem/sched/oom/attr), subject to DAC/LSM.
        // Other PID trees, proc driver state and create/remove/execute stay denied.
        ruleset = ruleset.add_rule(PathBeneath::new(
            PathFd::new(path)?,
            AccessFs::WriteFile | AccessFs::Truncate,
        ))?;
    }

    // Writable project/output/tmp roots are intentionally non-executable.
    // Platform mount validation already forbids write+execute; Landlock mirrors
    // that contract instead of accidentally granting Execute via from_all().
    for path in writable {
        ruleset = ruleset.add_rule(PathBeneath::new(PathFd::new(path)?, read_write_noexec))?;
    }
    let status = ruleset.restrict_self()?;
    if status.ruleset != RulesetStatus::FullyEnforced {
        return Err("Landlock was not fully enforced".into());
    }
    Ok(())
}

const NVIDIA_POLICY_FD_ENV: &str = "SEMWRIGHT_INTERNAL_NVIDIA_POLICY_FD";
const MAX_NVIDIA_POLICY_BYTES: usize = 65_536;
const NVIDIA_POLICY_SEALS: i32 =
    libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL;
const NVIDIA_NATIVE_EXECUTABLE: &str = "/plugin/tools/blender";

#[derive(Debug)]
struct GpuPolicy {
    writable: Vec<String>,
    readable: Vec<(String, bool)>,
    limits: [u64; 5],
}

fn validate_nvidia_policy(plan: &GpuPolicy) -> Result<(), Box<dyn std::error::Error>> {
    if plan.writable.len() < 2
        || plan.writable.len() > 34
        || plan.readable.len() > 64
        || plan.writable[0] != "/tmp"
        || plan.writable[1] != "/dev/shm"
    {
        return Err("invalid NVIDIA policy root bounds".into());
    }
    let mut seen = BTreeSet::new();
    for path in &plan.writable {
        if path.len() > 4096
            || !seen.insert(path)
            || (path != "/tmp"
                && path != "/dev/shm"
                && (!path.starts_with("/workspace/") || path.contains("..") || path.contains('\0')))
        {
            return Err("invalid NVIDIA writable root".into());
        }
    }
    for (path, execute) in &plan.readable {
        if path.len() > 4096
            || !seen.insert(path)
            || !if *execute {
                valid_exec_root(path)
            } else {
                valid_read_root(path)
            }
        {
            return Err("invalid NVIDIA readable root".into());
        }
    }
    if !plan
        .readable
        .iter()
        .any(|(p, x)| p == NVIDIA_NATIVE_EXECUTABLE && *x)
    {
        return Err("NVIDIA policy requires sealed Blender executable".into());
    }
    for (value, (minimum, maximum)) in plan.limits.iter().zip([
        (32, 1024),
        (8, 256),
        (5, 86_400),
        (134_217_728, 8_589_934_592),
        (1_048_576, 1_073_741_824),
    ]) {
        if !(minimum..=maximum).contains(value) {
            return Err("NVIDIA policy resource limit outside hard bounds".into());
        }
    }
    Ok(())
}

fn validate_nvidia_runner(
    writable: &[String],
    readable: &[(String, bool)],
    args: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let flags = [
        "--blender",
        "--workspace",
        "--runtime",
        "--scratch",
        "--fontconfig",
    ];
    if args.len() != 10
        || flags
            .iter()
            .enumerate()
            .any(|(i, flag)| args[i * 2] != *flag)
        || args[1] != NVIDIA_NATIVE_EXECUTABLE
        || !writable.contains(&args[3])
        || !writable.contains(&args[7])
        || !readable.iter().any(|(p, x)| p == &args[5] && !*x)
        || !readable.iter().any(|(p, x)| p == &args[9] && !*x)
    {
        return Err("NVIDIA handoff requires the fixed typed Blender runner contract".into());
    }
    Ok(())
}

fn encode_nvidia_policy(plan: &GpuPolicy) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    validate_nvidia_policy(plan)?;
    let bytes = serde_json::to_vec(&serde_json::json!({
        "version": 1, "nvidia_gpu": true, "writable": plan.writable,
        "readable": plan.readable, "limits": plan.limits,
    }))?;
    if bytes.is_empty() || bytes.len() > MAX_NVIDIA_POLICY_BYTES {
        return Err("NVIDIA policy exceeds its sealed transport bound".into());
    }
    Ok(bytes)
}

fn decode_nvidia_policy(bytes: &[u8]) -> Result<GpuPolicy, Box<dyn std::error::Error>> {
    if bytes.is_empty() || bytes.len() > MAX_NVIDIA_POLICY_BYTES {
        return Err("invalid NVIDIA policy size".into());
    }
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    let object = value.as_object().ok_or("NVIDIA policy must be an object")?;
    if object.len() != 5
        || value["version"].as_u64() != Some(1)
        || value["nvidia_gpu"].as_bool() != Some(true)
        || ["version", "nvidia_gpu", "writable", "readable", "limits"]
            .iter()
            .any(|key| !object.contains_key(*key))
    {
        return Err("unsupported NVIDIA policy fields".into());
    }
    let writable = value["writable"]
        .as_array()
        .ok_or("NVIDIA writable roots missing")?
        .iter()
        .map(|row| {
            row.as_str()
                .map(str::to_owned)
                .ok_or("invalid writable root")
        })
        .collect::<Result<Vec<_>, _>>()?;
    let readable = value["readable"]
        .as_array()
        .ok_or("NVIDIA readable roots missing")?
        .iter()
        .map(|row| {
            let row = row
                .as_array()
                .filter(|row| row.len() == 2)
                .ok_or("invalid readable root")?;
            Ok((
                row[0].as_str().ok_or("invalid readable path")?.to_owned(),
                row[1].as_bool().ok_or("invalid readable execute flag")?,
            ))
        })
        .collect::<Result<Vec<_>, &'static str>>()?;
    let values = value["limits"]
        .as_array()
        .filter(|row| row.len() == 5)
        .ok_or("invalid NVIDIA resource limits")?;
    let mut limits = [0u64; 5];
    for (out, value) in limits.iter_mut().zip(values) {
        *out = value.as_u64().ok_or("invalid NVIDIA resource limit")?;
    }
    let plan = GpuPolicy {
        writable,
        readable,
        limits,
    };
    validate_nvidia_policy(&plan)?;
    // The trusted parent emits one canonical encoding. Requiring that exact
    // encoding also rejects duplicate JSON fields and ambiguous parser inputs.
    if encode_nvidia_policy(&plan)? != bytes {
        return Err("NVIDIA policy must use its canonical sealed encoding".into());
    }
    Ok(plan)
}

fn create_nvidia_policy_fd(bytes: &[u8]) -> Result<File, Box<dyn std::error::Error>> {
    if bytes.is_empty() || bytes.len() > MAX_NVIDIA_POLICY_BYTES {
        return Err("invalid NVIDIA policy size".into());
    }
    // SAFETY: memfd_create receives a fixed NUL-terminated name and scalar flags.
    let fd =
        unsafe { libc::memfd_create(c"semwright-nvidia-policy".as_ptr(), libc::MFD_ALLOW_SEALING) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: successful memfd_create returns a new owned descriptor.
    let mut file = unsafe { File::from_raw_fd(fd) };
    file.write_all(bytes)?;
    // No filesystem path or editable file backs the policy. The descriptor is
    // inherited by exactly the trusted runner and fixed native-child helper.
    // SAFETY: fchmod operates only on this newly owned memfd.
    if unsafe { libc::fchmod(fd, 0o400) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: fcntl seals only this newly owned memfd after its bytes are written.
    if unsafe { libc::fcntl(fd, libc::F_ADD_SEALS, NVIDIA_POLICY_SEALS) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(file)
}

fn read_nvidia_policy_fd(fd: i32) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    if fd < 3 {
        return Err("NVIDIA policy descriptor cannot be standard I/O".into());
    }
    // SAFETY: F_GET_SEALS reads scalar state; ownership is taken only after validation.
    let seals = unsafe { libc::fcntl(fd, libc::F_GET_SEALS) };
    if seals < 0 || seals & NVIDIA_POLICY_SEALS != NVIDIA_POLICY_SEALS {
        return Err("NVIDIA policy descriptor lacks mandatory immutability seals".into());
    }
    // SAFETY: child owns its inherited descriptor; no other owner is created here.
    let file = unsafe { File::from_raw_fd(fd) };
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_NVIDIA_POLICY_BYTES as u64
    {
        return Err("NVIDIA policy descriptor is not a bounded regular memfd".into());
    }
    let mut bytes = vec![0u8; metadata.len() as usize];
    let mut offset = 0;
    while offset < bytes.len() {
        // SAFETY: buffer is writable and pread uses its own bounded offset,
        // independent of the descriptor offset shared with the parent runner.
        let count = unsafe {
            libc::pread(
                fd,
                bytes[offset..].as_mut_ptr().cast(),
                bytes.len() - offset,
                offset as libc::off_t,
            )
        };
        if count < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error.into());
        }
        if count == 0 {
            return Err("NVIDIA policy descriptor ended early".into());
        }
        offset += count as usize;
    }
    // file closes here, before installing policy or loading any Blender code.
    Ok(bytes)
}

fn nvidia_own_task_root(pid: u32) -> Result<String, Box<dyn std::error::Error>> {
    if pid == 0 || pid > i32::MAX as u32 {
        return Err("invalid native process PID".into());
    }
    Ok(format!("/proc/{pid}/task"))
}

fn run_nvidia_native_child(
    mut args: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("SEMWRIGHT_NVIDIA_GPU").as_deref() != Ok("1")
        || args.next().as_deref() != Some("--policy-fd")
    {
        return Err("NVIDIA native-child handoff is missing".into());
    }
    let fd = args
        .next()
        .ok_or("NVIDIA policy descriptor missing")?
        .parse::<i32>()?;
    if args.next().as_deref() != Some("--")
        || args.next().as_deref() != Some(NVIDIA_NATIVE_EXECUTABLE)
    {
        return Err("NVIDIA native-child executable is fixed".into());
    }
    let native_args = args.collect::<Vec<_>>();
    if native_args.len() > 64
        || native_args
            .iter()
            .any(|arg| arg.len() > 4096 || arg.contains('\0'))
    {
        return Err("NVIDIA native arguments exceed bounds".into());
    }
    let plan = decode_nvidia_policy(&read_nvidia_policy_fd(fd)?)?;
    let [nofile, nproc, cpu, address_space, file_size] = plan.limits;
    apply_limits(nofile, nproc, cpu, address_space, file_size)?;
    let task_root = nvidia_own_task_root(std::process::id())?;
    install_landlock(plan.writable, plan.readable, true, Some(task_root))?;
    let error = Command::new(NVIDIA_NATIVE_EXECUTABLE)
        .args(native_args)
        .env_remove(NVIDIA_POLICY_FD_ENV)
        .exec();
    Err(Box::new(error))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nvidia_test_policy() -> GpuPolicy {
        GpuPolicy {
            writable: vec![
                "/tmp".into(),
                "/dev/shm".into(),
                "/workspace/project".into(),
                "/workspace/scratch".into(),
            ],
            readable: vec![
                (NVIDIA_NATIVE_EXECUTABLE.into(), true),
                ("/workspace/blender-runtime".into(), false),
                ("/etc/fonts".into(), false),
            ],
            limits: [256, 128, 300, 4_294_967_296, 1_073_741_824],
        }
    }

    #[test]
    fn nvidia_sealed_policy_roundtrip_and_offset_are_exact() {
        let bytes = encode_nvidia_policy(&nvidia_test_policy()).unwrap();
        let mut file = create_nvidia_policy_fd(&bytes).unwrap();
        // The writer left its shared offset at EOF; child pread must ignore it.
        // SAFETY: F_DUPFD_CLOEXEC creates a separately owned descriptor for reader.
        let fd = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 3) };
        assert!(fd >= 3);
        let read = read_nvidia_policy_fd(fd).unwrap();
        assert_eq!(read, bytes);
        assert_eq!(
            decode_nvidia_policy(&read).unwrap().limits,
            [256, 128, 300, 4_294_967_296, 1_073_741_824]
        );
        assert!(file.write_all(b"tamper").is_err());
    }

    #[test]
    fn nvidia_policy_rejects_unsealed_and_oversized_descriptors() {
        // SAFETY: fixed name and scalar memfd flags create an owned test descriptor.
        let fd = unsafe {
            libc::memfd_create(c"semwright-unsealed-test".as_ptr(), libc::MFD_ALLOW_SEALING)
        };
        assert!(fd >= 3);
        // SAFETY: fd is newly created and uniquely owned by this File.
        let mut file = unsafe { File::from_raw_fd(fd) };
        file.write_all(b"{}").unwrap();
        assert!(read_nvidia_policy_fd(fd).is_err());
        assert!(create_nvidia_policy_fd(&vec![b' '; MAX_NVIDIA_POLICY_BYTES + 1]).is_err());
        assert!(read_nvidia_policy_fd(0).is_err());
    }

    #[test]
    fn nvidia_policy_rejects_unknown_duplicate_and_escalated_fields() {
        let bytes = encode_nvidia_policy(&nvidia_test_policy()).unwrap();
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        value["unknown"] = serde_json::json!(true);
        assert!(decode_nvidia_policy(&serde_json::to_vec(&value).unwrap()).is_err());
        let duplicate =
            String::from_utf8(bytes.clone())
                .unwrap()
                .replacen("{", "{\"version\":1,", 1);
        assert!(decode_nvidia_policy(duplicate.as_bytes()).is_err());
        let mut plan = nvidia_test_policy();
        plan.writable.push("/proc".into());
        assert!(encode_nvidia_policy(&plan).is_err());
        plan = nvidia_test_policy();
        plan.readable.push(("/workspace/project".into(), true));
        assert!(encode_nvidia_policy(&plan).is_err());
        plan = nvidia_test_policy();
        plan.limits[3] = 8_589_934_593;
        assert!(encode_nvidia_policy(&plan).is_err());
        assert!(decode_nvidia_policy(&vec![b' '; MAX_NVIDIA_POLICY_BYTES + 1]).is_err());
    }

    #[test]
    fn nvidia_runner_handoff_rejects_wrong_exec_flags_and_roots() {
        let plan = nvidia_test_policy();
        let args = [
            "--blender",
            NVIDIA_NATIVE_EXECUTABLE,
            "--workspace",
            "/workspace/project",
            "--runtime",
            "/workspace/blender-runtime",
            "--scratch",
            "/workspace/scratch",
            "--fontconfig",
            "/etc/fonts",
        ]
        .map(str::to_owned)
        .to_vec();
        assert!(validate_nvidia_runner(&plan.writable, &plan.readable, &args).is_ok());
        for (index, wrong) in [
            (1, "/usr/bin/sh"),
            (3, "/proc"),
            (5, "/workspace/project"),
            (7, "/workspace/other"),
            (8, "--other"),
        ] {
            let mut changed = args.clone();
            changed[index] = wrong.into();
            assert!(validate_nvidia_runner(&plan.writable, &plan.readable, &changed).is_err());
        }
    }

    #[test]
    fn nvidia_native_address_space_is_eight_gib_only_for_gpu_policy() {
        assert!(validate_nvidia_address_space(4_294_967_296, false).is_ok());
        assert!(validate_nvidia_address_space(4_294_967_297, false).is_err());
        assert!(validate_nvidia_address_space(8_589_934_592, true).is_ok());
        assert!(validate_nvidia_address_space(8_589_934_593, true).is_err());
        let mut plan = nvidia_test_policy();
        plan.limits[3] = 8_589_934_592;
        let bytes = encode_nvidia_policy(&plan).unwrap();
        assert_eq!(
            decode_nvidia_policy(&bytes).unwrap().limits,
            [256, 128, 300, 8_589_934_592, 1_073_741_824]
        );
        plan.limits[3] += 1;
        assert!(encode_nvidia_policy(&plan).is_err());
    }

    #[test]
    fn nvidia_task_write_scope_is_own_process_only_and_noncreating() {
        assert_eq!(nvidia_own_task_root(34).unwrap(), "/proc/34/task");
        assert!(nvidia_own_task_root(0).is_err());
        assert!(nvidia_own_task_root(u32::MAX).is_err());
        let rights = AccessFs::WriteFile | AccessFs::Truncate;
        assert!(!rights.contains(AccessFs::MakeReg));
        assert!(!rights.contains(AccessFs::RemoveFile));
        assert!(!rights.contains(AccessFs::Execute));
        assert!(!rights.contains(AccessFs::ReadDir));
        let baseline_proc = AccessFs::ReadFile | AccessFs::ReadDir;
        assert!(!baseline_proc.intersects(rights));
    }

    #[test]
    fn secret_read_roots_are_single_file_and_confined() {
        assert!(valid_read_root("/run/secrets/godot-pairing"));
        assert!(!valid_read_root("/run/secrets"));
        assert!(!valid_read_root("/run/secrets/nested/key"));
        assert!(!valid_read_root("/run/secrets/../escape"));
        assert!(!valid_read_root("/run/other/key"));
    }

    #[test]
    fn sealed_tool_exec_roots_are_single_file_and_confined() {
        assert!(valid_exec_root("/plugin/tools/godot"));
        assert!(!valid_exec_root("/plugin/tools"));
        assert!(!valid_exec_root("/plugin/tools/nested/tool"));
        assert!(!valid_exec_root("/plugin/tools/../escape"));
        assert!(!valid_exec_root("/plugin/other/tool"));
    }

    #[test]
    fn resource_bounds_match_driver_manifest_hard_limits() {
        assert_eq!(bounded_limit(Some("32".into()), 32, 1024).unwrap(), 32);
        assert!(bounded_limit(Some("31".into()), 32, 1024).is_err());
        assert!(bounded_limit(Some("1025".into()), 32, 1024).is_err());
        assert!(bounded_limit(Some("not-a-number".into()), 32, 1024).is_err());
        assert_eq!(
            bounded_limit(Some("86400".into()), 5, 86_400).unwrap(),
            86_400
        );
        assert!(bounded_limit(Some("86401".into()), 5, 86_400).is_err());
        assert_eq!(
            bounded_limit(Some("4294967296".into()), 134_217_728, 4_294_967_296).unwrap(),
            4_294_967_296
        );
        assert!(bounded_limit(Some("4294967297".into()), 134_217_728, 4_294_967_296).is_err());
    }
}
