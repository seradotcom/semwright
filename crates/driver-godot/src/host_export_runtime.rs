//! Closed export entrypoint for a Host-sealed Godot dependency and owner mounts.
use semwright_types::{Error, ErrorCode, Result};
use std::{ffi::OsString, os::unix::process::CommandExt, path::Path, process::Command};

fn validate_export_argv(argv: &[OsString], project_root: &Path, output_root: &Path) -> Result<()> {
    if argv.len() != 6
        || argv[0] != "--headless"
        || argv[1] != "--path"
        || !matches!(
            argv[3].to_str(),
            Some("--export-release" | "--export-debug" | "--export-pack")
        )
    {
        return Err(Error::invalid(
            "Host Godot export accepts only the closed export argument shape",
        ));
    }
    let preset = argv[4]
        .to_str()
        .ok_or_else(|| Error::invalid("Godot export preset is not UTF8"))?;
    if preset.is_empty() || preset.len() > 128 || preset.chars().any(char::is_control) {
        return Err(Error::invalid("Godot export preset exceeds bounds"));
    }
    let project = Path::new(&argv[2]);
    let output = Path::new(&argv[5]);
    if !project.is_absolute()
        || !output.is_absolute()
        || project.canonicalize()? != project
        || !project.starts_with(project_root)
        || !output.starts_with(output_root)
        || !output
            .parent()
            .ok_or_else(|| Error::invalid("Godot export output has no parent"))?
            .canonicalize()?
            .starts_with(output_root)
    {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Godot export path escaped its Host mount",
        ));
    }
    if let Ok(metadata) = std::fs::symlink_metadata(output)
        && (!metadata.is_file() || metadata.file_type().is_symlink())
    {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Godot export output must be a regular non-symlink file",
        ));
    }
    Ok(())
}

pub fn execute(args: &[OsString]) -> Result<()> {
    if std::env::var("SEMWRIGHT_DRIVER_SANDBOX").as_deref() != Ok("landlock-bwrap-v1")
        || std::env::var_os("SEMWRIGHT_SANDBOX_TOOLS_V1").is_none()
        || std::env::var_os("SEMWRIGHT_SANDBOX_MOUNTS_V1").is_none()
        || args.len() != 9
    {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Godot export runtime requires a Host tool invocation",
        ));
    }
    let executable = Path::new(&args[0]);
    semwright_driver_sdk::validate_materialized_tool_argument("godot", executable)?;
    let output_mount = args[1]
        .to_str()
        .ok_or_else(|| Error::invalid("Godot output mount is not UTF8"))?;
    let project_mount = args[2]
        .to_str()
        .ok_or_else(|| Error::invalid("Godot project mount is not UTF8"))?;
    let output_root = semwright_driver_sdk::workspace_mount(output_mount)?.canonicalize()?;
    let project_root = semwright_driver_sdk::workspace_mount(project_mount)?.canonicalize()?;
    validate_export_argv(&args[3..], &project_root, &output_root)?;
    let home = output_root.join(".semwright-home");
    match std::fs::symlink_metadata(&home) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
        Ok(_) => {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "Godot private home must be a non-symlink directory",
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => std::fs::create_dir(&home)?,
        Err(error) => return Err(error.into()),
    }
    // The Host bounds the combined output pipe; retain Godot's stderr in it.
    // SAFETY: dup2 operates on this process's standard output/error descriptors.
    if unsafe { libc::dup2(1, 2) } < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let error = Command::new(executable)
        .args(&args[3..])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .env("HOME", &home)
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .exec();
    Err(error.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_entry_rejects_other_operations_and_paths_outside_owner_mounts() {
        let root = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        std::fs::create_dir(&project).unwrap();
        let argv = vec![
            "--headless".into(),
            "--path".into(),
            project.clone().into_os_string(),
            "--export-release".into(),
            "Linux".into(),
            root.path().join("game.x86_64").into_os_string(),
        ];
        validate_export_argv(&argv, root.path(), root.path()).unwrap();
        let mut changed = argv.clone();
        changed[3] = "--script".into();
        assert!(validate_export_argv(&changed, root.path(), root.path()).is_err());
        let mut changed = argv.clone();
        changed[5] = other.path().join("game.x86_64").into_os_string();
        assert!(validate_export_argv(&changed, root.path(), root.path()).is_err());
        let mut changed = argv.clone();
        changed.push("--extra".into());
        assert!(validate_export_argv(&changed, root.path(), root.path()).is_err());
        let output = root.path().join("game.x86_64");
        std::os::unix::fs::symlink(other.path().join("foreign"), output).unwrap();
        assert!(validate_export_argv(&argv, root.path(), root.path()).is_err());
    }
}
