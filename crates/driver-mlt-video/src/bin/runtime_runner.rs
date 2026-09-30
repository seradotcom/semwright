//! Semwright-owned runtime helper for Host-mediated MLT tools.
//!
//! The Driver Host supplies owner-pinned dependency paths through protocol-v7 typed
//! ToolPath arguments. This helper never searches PATH or accepts shell fragments.

use semwright_mlt_video::{
    hash::reader_hash,
    runtime::{ProcessSpec, ServiceCatalog, constrained_environment, run},
};
use serde_json::{Map, Value, json};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
    time::Duration,
};

const GROUPS: [&str; 6] = [
    "producers",
    "filters",
    "transitions",
    "consumers",
    "video_codecs",
    "audio_codecs",
];

fn dependency_path(value: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(value);
    if !path.is_absolute() || value.contains('\0') {
        return Err("runtime dependency must be an absolute Host-materialized path".into());
    }
    let metadata = std::fs::symlink_metadata(&path)
        .map_err(|_| "runtime dependency is unavailable".to_string())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("runtime dependency must be a regular non-symlink file".into());
    }
    Ok(path)
}

fn runtime_entry(runtime_root: &Path, sealed_melt: &Path) -> Result<PathBuf, String> {
    let root_meta = std::fs::symlink_metadata(runtime_root)
        .map_err(|_| "MLT runtime root is unavailable".to_string())?;
    if !runtime_root.is_absolute() || !root_meta.is_dir() || root_meta.file_type().is_symlink() {
        return Err("MLT runtime root must be an absolute non-symlink directory".into());
    }
    let candidate = runtime_root.join("bin").join("melt");
    let candidate_meta = std::fs::symlink_metadata(&candidate)
        .map_err(|_| "MLT runtime entrypoint is unavailable".to_string())?;
    if !candidate_meta.is_file() || candidate_meta.file_type().is_symlink() {
        return Err("MLT runtime entrypoint must be a regular non-symlink file".into());
    }
    let (sealed_hash, sealed_size) = reader_hash(
        std::fs::File::open(sealed_melt)
            .map_err(|_| "sealed melt dependency is unreadable".to_string())?,
        64 * 1024 * 1024,
    )
    .map_err(|error| format!("{}: {}", error.code, error.message))?;
    let (runtime_hash, runtime_size) = reader_hash(
        std::fs::File::open(&candidate)
            .map_err(|_| "MLT runtime entrypoint is unreadable".to_string())?,
        64 * 1024 * 1024,
    )
    .map_err(|error| format!("{}: {}", error.code, error.message))?;
    if sealed_size != runtime_size || sealed_hash != runtime_hash {
        return Err("MLT runtime entrypoint does not match the Host-sealed melt bytes".into());
    }
    Ok(candidate)
}

fn execute(melt: &Path, args: Vec<OsString>) -> Result<(Vec<u8>, Vec<u8>), String> {
    let cancel = AtomicBool::new(false);
    let result = run(
        &ProcessSpec {
            executable: melt.to_path_buf(),
            args,
            cwd: PathBuf::from("/tmp"),
            timeout: Duration::from_secs(3),
            cpu_seconds: 30,
            // Match the production MLT process ceiling. The Host job already
            // enforces the same outer 4 GiB maximum, so this nested supervisor
            // may only reduce that inherited authority, never raise it.
            address_space_bytes: 4_294_967_296,
            environment: constrained_environment(),
        },
        &cancel,
    )
    .map_err(|error| format!("{}: {}", error.code, error.message))?
    .checked()
    .map_err(|error| format!("{}: {}", error.code, error.message))?;
    Ok((result.stdout, result.stderr))
}

fn discover(melt: &Path) -> Result<Value, String> {
    let (stdout, stderr) = execute(melt, vec!["-version".into()])?;
    let version_text = format!(
        "{} {}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    );
    let version = semwright_mlt_video::json::display(
        version_text
            .lines()
            .find(|line| line.to_ascii_lowercase().contains("melt"))
            .unwrap_or("Version output did not identify melt"),
    );

    let mut groups = Map::new();
    for group in GROUPS {
        let (stdout, stderr) =
            execute(melt, vec![OsString::from("-query"), OsString::from(group)])?;
        let text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&stdout),
            String::from_utf8_lossy(&stderr)
        );
        let values = ServiceCatalog::parse_list(&text)
            .map_err(|error| format!("{}: {}", error.code, error.message))?;
        groups.insert(
            group.into(),
            Value::Array(values.into_iter().map(Value::String).collect()),
        );
    }

    Ok(json!({
        "schema": 1,
        "operation": "discover",
        "version": version,
        "groups": groups,
    }))
}

fn parse() -> Result<Value, String> {
    let mut args = std::env::args().skip(1);
    let operation = args
        .next()
        .ok_or_else(|| "runtime operation is required".to_string())?;
    if operation != "discover" {
        return Err("runtime operation is unsupported".into());
    }
    if args.next().as_deref() != Some("--runtime-root") {
        return Err("discover requires --runtime-root <Host MountPath>".into());
    }
    let runtime_root = PathBuf::from(
        args.next()
            .ok_or_else(|| "discover requires an MLT runtime root".to_string())?,
    );
    if args.next().as_deref() != Some("--melt-sealed") {
        return Err("discover requires --melt-sealed <Host ToolPath>".into());
    }
    let sealed_melt = dependency_path(
        &args
            .next()
            .ok_or_else(|| "discover requires a sealed melt dependency".to_string())?,
    )?;
    if args.next().is_some() {
        return Err("runtime runner received unexpected arguments".into());
    }
    let melt = runtime_entry(&runtime_root, &sealed_melt)?;
    discover(&melt)
}

fn main() {
    let value = match parse() {
        Ok(value) => value,
        Err(error) => {
            let message: String = error
                .chars()
                .filter(|ch| !ch.is_control())
                .take(1024)
                .collect();
            let value = json!({
                "schema": 1,
                "operation": "error",
                "error": message
            });
            match serde_json::to_string(&value) {
                Ok(encoded) if encoded.len() <= 4096 => print!("{encoded}"),
                _ => {
                    print!(r#"{{"schema":1,"operation":"error","error":"runtime runner failed"}}"#)
                }
            }
            std::process::exit(2);
        }
    };
    match serde_json::to_string(&value) {
        Ok(encoded) if encoded.len() <= 256 * 1024 => print!("{encoded}"),
        _ => {
            print!(
                r#"{{"schema":1,"operation":"error","error":"runtime runner output exceeded its bound"}}"#
            );
            std::process::exit(1);
        }
    }
}
