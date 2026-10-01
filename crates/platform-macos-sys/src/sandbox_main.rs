//! App Sandbox parent/exec helper used only by the Semwright-owned sandbox binary.
use std::{
    ffi::{OsStr, OsString},
    os::unix::process::CommandExt,
    process::Command,
};

pub(crate) const PARENT_ARG: &str = "--macos-sandbox-parent";
pub(crate) const EXEC_ARG: &str = "--macos-sandbox-exec";
pub(crate) const CWD_ENV: &str = "SEMWRIGHT_MACOS_SANDBOX_CWD";
pub(crate) const NOFILE_ENV: &str = "SEMWRIGHT_MACOS_RLIMIT_NOFILE";
pub(crate) const NPROC_ENV: &str = "SEMWRIGHT_MACOS_RLIMIT_NPROC";
pub(crate) const CPU_ENV: &str = "SEMWRIGHT_MACOS_RLIMIT_CPU";
pub(crate) const AS_ENV: &str = "SEMWRIGHT_MACOS_RLIMIT_AS";
pub(crate) const FSIZE_ENV: &str = "SEMWRIGHT_MACOS_RLIMIT_FSIZE";

fn parsed_limit(name: &str) -> Result<libc::rlim_t, String> {
    let value = std::env::var(name).map_err(|_| format!("missing {name}"))?;
    value
        .parse::<libc::rlim_t>()
        .map_err(|_| format!("invalid {name}"))
}
fn set_limit(resource: i32, value: libc::rlim_t) -> Result<(), String> {
    let limit = libc::rlimit {
        rlim_cur: value,
        rlim_max: value,
    };
    // SAFETY: resource is a fixed RLIMIT_* constant and limit points to initialized scalars.
    if unsafe { libc::setrlimit(resource as _, &limit) } != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(())
}

fn apply_limits() -> Result<(), String> {
    set_limit(libc::RLIMIT_NOFILE as i32, parsed_limit(NOFILE_ENV)?)?;
    set_limit(libc::RLIMIT_NPROC as i32, parsed_limit(NPROC_ENV)?)?;
    set_limit(libc::RLIMIT_CPU as i32, parsed_limit(CPU_ENV)?)?;
    set_limit(libc::RLIMIT_AS as i32, parsed_limit(AS_ENV)?)?;
    set_limit(libc::RLIMIT_FSIZE as i32, parsed_limit(FSIZE_ENV)?)?;
    Ok(())
}

fn internal_environment(name: &OsStr) -> bool {
    [CWD_ENV, NOFILE_ENV, NPROC_ENV, CPU_ENV, AS_ENV, FSIZE_ENV]
        .iter()
        .any(|candidate| name == OsStr::new(candidate))
}
fn run_parent(mut args: impl Iterator<Item = OsString>) -> i32 {
    let Some(wrapper) = args.next() else {
        eprintln!("SandboxDenied: missing macOS inherited wrapper");
        return 64;
    };
    let Some(payload) = args.next() else {
        eprintln!("SandboxDenied: missing macOS sandbox payload");
        return 64;
    };
    let mut child = Command::new(wrapper);
    child.arg(EXEC_ARG).arg(payload).args(args);
    match child.status() {
        Ok(status) => status.code().unwrap_or(128),
        Err(error) => {
            eprintln!("SandboxDenied: macOS inherited wrapper failed: {error}");
            65
        }
    }
}

fn exec_payload(mut args: impl Iterator<Item = OsString>) -> i32 {
    let Some(payload) = args.next() else {
        eprintln!("SandboxDenied: missing pinned macOS payload");
        return 64;
    };
    if let Err(error) = apply_limits() {
        eprintln!("SandboxDenied: macOS resource limit failed: {error}");
        return 66;
    }
    let cwd = std::env::var_os(CWD_ENV);
    let inherited = std::env::vars_os()
        .filter(|(name, _)| !internal_environment(name))
        .collect::<Vec<_>>();
    let mut command = Command::new(payload);
    command.args(args).env_clear().envs(inherited);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let error = command.exec();
    eprintln!("SandboxDenied: macOS pinned payload exec failed: {error}");
    67
}

pub fn main() {
    let mut args = std::env::args_os().skip(1);
    let role = args.next();
    let code = match role.as_deref().and_then(OsStr::to_str) {
        Some(PARENT_ARG) => run_parent(args),
        Some(EXEC_ARG) => exec_payload(args),
        _ => {
            eprintln!("SandboxDenied: invalid macOS sandbox helper role");
            64
        }
    };
    std::process::exit(code);
}
