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

fn runtime_entry(runtime_root: &Path, sealed_tool: &Path, name: &str) -> Result<PathBuf, String> {
    if !matches!(name, "melt" | "ffprobe") {
        return Err("runtime entrypoint is not allowlisted".into());
    }
    let root_meta = std::fs::symlink_metadata(runtime_root)
        .map_err(|_| "MLT runtime root is unavailable".to_string())?;
    if !runtime_root.is_absolute() || !root_meta.is_dir() || root_meta.file_type().is_symlink() {
        return Err("MLT runtime root must be an absolute non-symlink directory".into());
    }
    let canonical_root = std::fs::canonicalize(runtime_root)
        .map_err(|_| "MLT runtime root could not be canonicalized".to_string())?;
    let entry = runtime_root.join("bin").join(name);
    let candidate = std::fs::canonicalize(&entry)
        .map_err(|_| format!("{name} runtime entrypoint is unavailable"))?;
    if candidate.strip_prefix(&canonical_root).is_err() {
        return Err(format!(
            "{name} runtime entrypoint escaped the delegated runtime root"
        ));
    }
    let candidate_meta = std::fs::symlink_metadata(&candidate)
        .map_err(|_| format!("{name} runtime entrypoint metadata is unavailable"))?;
    if !candidate_meta.is_file() || candidate_meta.file_type().is_symlink() {
        return Err(format!(
            "Canonical {name} runtime entrypoint must be a regular file"
        ));
    }
    let (sealed_hash, sealed_size) = reader_hash(
        std::fs::File::open(sealed_tool)
            .map_err(|_| format!("sealed {name} dependency is unreadable"))?,
        64 * 1024 * 1024,
    )
    .map_err(|error| format!("{}: {}", error.code, error.message))?;
    let (runtime_hash, runtime_size) = reader_hash(
        std::fs::File::open(&candidate)
            .map_err(|_| format!("{name} runtime entrypoint is unreadable"))?,
        64 * 1024 * 1024,
    )
    .map_err(|error| format!("{}: {}", error.code, error.message))?;
    if sealed_size != runtime_size || sealed_hash != runtime_hash {
        return Err(format!(
            "{name} runtime entrypoint does not match the Host-sealed tool bytes"
        ));
    }
    Ok(candidate)
}

fn execute(
    executable: &Path,
    args: Vec<OsString>,
    cwd: &Path,
    timeout: Duration,
    cpu_seconds: u64,
    address_space_bytes: u64,
) -> Result<(Vec<u8>, Vec<u8>), String> {
    let cancel = AtomicBool::new(false);
    let result = run(
        &ProcessSpec {
            executable: executable.to_path_buf(),
            args,
            cwd: cwd.to_path_buf(),
            timeout,
            cpu_seconds,
            address_space_bytes,
            environment: constrained_environment(),
        },
        &cancel,
    )
    .map_err(|error| format!("{}: {}", error.code, error.message))?
    .checked()
    .map_err(|error| format!("{}: {}", error.code, error.message))?;
    Ok((result.stdout, result.stderr))
}

fn direct_component(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 128
        || value == "."
        || value == ".."
        || value.contains('/')
        || value.contains('\\')
        || value.chars().any(char::is_control)
    {
        return Err(format!("{label} must be one bounded path component"));
    }
    Ok(())
}

fn scratch_file(
    scratch_root: &Path,
    directory: &str,
    name: &str,
) -> Result<(PathBuf, PathBuf), String> {
    direct_component(directory, "scratch directory")?;
    direct_component(name, "scratch filename")?;
    let root_meta = std::fs::symlink_metadata(scratch_root)
        .map_err(|_| "scratch root is unavailable".to_string())?;
    if !scratch_root.is_absolute() || !root_meta.is_dir() || root_meta.file_type().is_symlink() {
        return Err("scratch root must be an absolute non-symlink directory".into());
    }
    let work = scratch_root.join(directory);
    let work_meta = std::fs::symlink_metadata(&work)
        .map_err(|_| "scratch job directory is unavailable".to_string())?;
    if !work_meta.is_dir() || work_meta.file_type().is_symlink() {
        return Err("scratch job directory must be a non-symlink directory".into());
    }
    let target = work.join(name);
    let target_meta = std::fs::symlink_metadata(&target)
        .map_err(|_| "scratch media file is unavailable".to_string())?;
    if !target_meta.is_file() || target_meta.file_type().is_symlink() {
        return Err("scratch media file must be a regular non-symlink file".into());
    }
    Ok((work, target))
}

fn probe(
    ffprobe: &Path,
    scratch_root: &Path,
    directory: &str,
    name: &str,
) -> Result<Value, String> {
    let (work, target) = scratch_file(scratch_root, directory, name)?;
    let args = [
        "-v",
        "error",
        "-show_streams",
        "-show_format",
        "-of",
        "json",
    ]
    .into_iter()
    .map(OsString::from)
    .chain(std::iter::once(target.into_os_string()))
    .collect();
    let (stdout, _stderr) = execute(
        ffprobe,
        args,
        &work,
        Duration::from_secs(5),
        30,
        1_073_741_824,
    )?;
    let media: Value = serde_json::from_slice(&stdout)
        .map_err(|_| "ffprobe returned malformed JSON".to_string())?;
    if !media.is_object() {
        return Err("ffprobe result must be a JSON object".into());
    }
    Ok(json!({
        "schema": 1,
        "operation": "probe",
        "media": media,
    }))
}

fn discover(melt: &Path) -> Result<Value, String> {
    let (stdout, stderr) = execute(
        melt,
        vec!["-version".into()],
        Path::new("/tmp"),
        Duration::from_secs(3),
        30,
        4_294_967_296,
    )?;
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
        let (stdout, stderr) = execute(
            melt,
            vec![OsString::from("-query"), OsString::from(group)],
            Path::new("/tmp"),
            Duration::from_secs(3),
            30,
            4_294_967_296,
        )?;
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

fn required_flag(
    args: &mut impl Iterator<Item = String>,
    expected: &str,
    description: &str,
) -> Result<String, String> {
    if args.next().as_deref() != Some(expected) {
        return Err(format!("{description} requires {expected} <value>"));
    }
    args.next()
        .ok_or_else(|| format!("{description} requires a value for {expected}"))
}

fn parse() -> Result<Value, String> {
    let mut args = std::env::args().skip(1);
    let operation = args
        .next()
        .ok_or_else(|| "runtime operation is required".to_string())?;
    match operation.as_str() {
        "discover" => {
            let runtime_root =
                PathBuf::from(required_flag(&mut args, "--runtime-root", "discover")?);
            let sealed_melt =
                dependency_path(&required_flag(&mut args, "--melt-sealed", "discover")?)?;
            if args.next().is_some() {
                return Err("runtime runner received unexpected arguments".into());
            }
            let melt = runtime_entry(&runtime_root, &sealed_melt, "melt")?;
            discover(&melt)
        }
        "probe" => {
            let runtime_root = PathBuf::from(required_flag(&mut args, "--runtime-root", "probe")?);
            let sealed_ffprobe =
                dependency_path(&required_flag(&mut args, "--ffprobe-sealed", "probe")?)?;
            let scratch_root = PathBuf::from(required_flag(&mut args, "--scratch-root", "probe")?);
            let directory = required_flag(&mut args, "--directory", "probe")?;
            let name = required_flag(&mut args, "--name", "probe")?;
            if args.next().is_some() {
                return Err("runtime runner received unexpected arguments".into());
            }
            let ffprobe = runtime_entry(&runtime_root, &sealed_ffprobe, "ffprobe")?;
            probe(&ffprobe, &scratch_root, &directory, &name)
        }
        _ => Err("runtime operation is unsupported".into()),
    }
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
