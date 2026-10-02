//! Host-owned wrapper for the three declared Ardour utility dependencies.
//! It preserves the required native environment without granting ambient executable discovery.
use serde::Deserialize;
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    args: Vec<String>,
    project_root: PathBuf,
    output_root: PathBuf,
}
fn run() -> Result<i32, Box<dyn std::error::Error>> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.len() != 6
        || argv[0] != "--tool"
        || argv[2] != "--project-root"
        || argv[4] != "--output-root"
    {
        return Err("invalid Host runtime argument shape".into());
    }
    let executable = Path::new(&argv[1]);
    if !executable.is_absolute() || !executable.is_file() {
        return Err("pinned utility missing".into());
    }
    let project = Path::new(&argv[3]).canonicalize()?;
    let output = Path::new(&argv[5]).canonicalize()?;
    if !project.is_dir() || !output.is_dir() {
        return Err("Host workspace mount missing".into());
    }
    let mut bytes = Vec::new();
    std::io::stdin().take(131073).read_to_end(&mut bytes)?;
    if bytes.len() > 131072 {
        return Err("Ardour request exceeds byte budget".into());
    }
    let request: Request = serde_json::from_slice(&bytes)?;
    if request.args.is_empty()
        || request.args.len() > 64
        || request
            .args
            .iter()
            .any(|arg| arg.len() > 4096 || arg.contains('\0'))
    {
        return Err("Ardour native argument budget exceeded".into());
    }
    let mut args = Vec::new();
    for argument in request.args {
        let path = Path::new(&argument);
        let translated = [
            (request.project_root.as_path(), project.as_path()),
            (request.output_root.as_path(), output.as_path()),
        ]
        .into_iter()
        .find_map(|(original, actual)| {
            path.strip_prefix(original)
                .ok()
                .map(|relative| (actual, relative))
        });
        if let Some((actual, relative)) = translated {
            if relative
                .components()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
            {
                return Err("Ardour workspace path is not canonical relative data".into());
            }
            args.push(actual.join(relative).to_string_lossy().into_owned());
        } else {
            args.push(argument);
        }
    }
    let private = tempfile::Builder::new()
        .prefix("ardour-host-home-")
        .tempdir()?;
    let data = semwright_ardour_audio::runtime::provision_static_template(private.path())?;
    for path in [".cache", ".config", ".local/share"] {
        std::fs::create_dir_all(private.path().join(path))?;
    }
    let status = Command::new(executable)
        .args(args)
        .current_dir(private.path())
        .env_clear()
        .env("HOME", private.path())
        .env("XDG_CACHE_HOME", private.path().join(".cache"))
        .env("XDG_CONFIG_HOME", private.path().join(".config"))
        .env("XDG_DATA_HOME", private.path().join(".local/share"))
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .env("LC_ALL", "C.UTF-8")
        .env("LD_LIBRARY_PATH", "/usr/lib/ardour8")
        .env(
            "ARDOUR_DATA_PATH",
            format!("{}:/usr/share/ardour8", data.display()),
        )
        .env("ARDOUR_CONFIG_PATH", "/etc/ardour8")
        .env("ARDOUR_DLL_PATH", "/usr/lib/ardour8")
        .env("VAMP_PATH", "/usr/lib/ardour8/vamp")
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;
    Ok(status.code().unwrap_or(1))
}
fn main() {
    match run() {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            let _ = writeln!(std::io::stderr(), "Ardour Host wrapper failed: {error}");
            std::process::exit(1);
        }
    }
}
