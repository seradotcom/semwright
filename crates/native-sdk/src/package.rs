//! Operator-side canonical package installer. No URLs, extraction scripts, or
//! executable launch; all bytes and installation paths are operator inputs.
use semwright_driver_registry::{
    Index, InstallRoots, create_package, inspect_package, install_from_index, remove_installed,
};
use semwright_driver_sdk::Manifest;
use semwright_types::{Error, Result};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};
fn manifest_bytes(path: &Path) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 1_048_576 {
        return Err(Error::invalid("Manifest must be a bounded regular file"));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(1_048_577)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 1_048_576 {
        return Err(Error::invalid("Manifest exceeds the byte limit"));
    }
    Ok(bytes)
}
fn roots(data: &str, config: &str) -> Result<InstallRoots> {
    let roots = InstallRoots {
        data: PathBuf::from(data),
        config: PathBuf::from(config),
    };
    if !roots.data.is_absolute() || !roots.config.is_absolute() {
        return Err(Error::invalid(
            "DATA_ROOT and CONFIG_ROOT must be absolute paths",
        ));
    }
    Ok(roots)
}
fn run() -> Result<serde_json::Value> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("inspect") if args.len() == 3 => {
            let (meta, _, digest) = inspect_package(&PathBuf::from(&args[2]))?;
            Ok(serde_json::json!({"metadata":meta,"package_sha256":digest}))
        }
        Some("pack") if args.len() == 5 => {
            let manifest: Manifest = serde_json::from_slice(&manifest_bytes(Path::new(&args[2]))?)?;
            manifest.validate()?;
            let digest = create_package(&manifest, &args[3], &PathBuf::from(&args[4]))?;
            Ok(serde_json::json!({"package_sha256":digest}))
        }
        Some("install") if args.len() == 7 => {
            let roots = roots(&args[5], &args[6])?;
            let path = PathBuf::from(&args[2]);
            let index = Index::load(&path)?;
            let entry = index
                .drivers
                .iter()
                .find(|e| e.id == args[3] && e.version == args[4])
                .ok_or_else(|| Error::invalid("Exact package pin not present"))?;
            let receipt = install_from_index(&path, entry, None, &roots)?;
            Ok(serde_json::to_value(receipt)?)
        }
        Some("remove") if args.len() == 6 => {
            let roots = roots(&args[4], &args[5])?;
            remove_installed(&args[2], &args[3], &roots)?;
            Ok(serde_json::json!({"removed":true}))
        }
        _ => Err(Error::invalid(
            "Use inspect PACKAGE | pack MANIFEST CORE_RANGE OUTPUT | install INDEX ID VERSION DATA_ROOT CONFIG_ROOT | remove ID VERSION DATA_ROOT CONFIG_ROOT",
        )),
    }
}
fn main() {
    match run() {
        Ok(v) => println!("{v}"),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(e.exit_code());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn relative_install_roots_are_rejected_before_mutation() {
        assert!(roots("relative", "/tmp/config").is_err());
        assert!(roots("/tmp/data", "relative").is_err());
        assert!(roots("/tmp/data", "/tmp/config").is_ok());
    }
}
