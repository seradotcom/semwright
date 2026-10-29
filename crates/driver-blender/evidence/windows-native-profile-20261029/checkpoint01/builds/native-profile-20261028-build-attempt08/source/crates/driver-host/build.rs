//! Read-only integrity check. Synchronization is an explicit developer action.
use std::{fs, path::Path};
pub const SOURCES: &[(&str, &str, &str)] = &[
    ("bridge.py", "../driver-blender/src/bridge.py", "bridge.py"),
    (
        "commands.py",
        "../../adapters/blender/semwright_blender/commands.py",
        "semwright_blender_runtime/commands.py",
    ),
    (
        "commands.json",
        "../../adapters/blender/semwright_blender/commands.json",
        "semwright_blender_runtime/commands.json",
    ),
    (
        "validation.py",
        "../../adapters/blender/semwright_blender/validation.py",
        "semwright_blender_runtime/validation.py",
    ),
    (
        "semantic.py",
        "../driver-blender/src/semantic.py",
        "semwright_blender_runtime/semantic.py",
    ),
    (
        "authoring_native.py",
        "../driver-blender/src/authoring_native.py",
        "semwright_blender_runtime/authoring_native.py",
    ),
    (
        "export_scope.py",
        "../../adapters/blender/semwright_blender/export_scope.py",
        "semwright_blender_runtime/export_scope.py",
    ),
];

fn bounded_read(path: &Path) -> Result<Vec<u8>, String> {
    let meta = fs::metadata(path)
        .map_err(|e| format!("Missing runtime resource {}: {e}", path.display()))?;
    if !meta.is_file() || meta.len() > 4 * 1024 * 1024 {
        return Err(format!(
            "Runtime resource exceeds regular-file bound: {}",
            path.display()
        ));
    }
    fs::read(path).map_err(|e| format!("Read runtime resource {}: {e}", path.display()))
}
pub fn verify_bundle(root: &Path) -> Result<(), String> {
    let repository = SOURCES
        .iter()
        .any(|(_, canonical, _)| root.join(canonical).exists())
        || (root.join("../../Cargo.toml").is_file()
            && root.join("../driver-sdk/Cargo.toml").is_file());
    for (name, canonical, _) in SOURCES {
        let bundled = root.join("src/windows_blender52_runtime").join(name);
        let bytes = bounded_read(&bundled)?;
        if repository {
            let original = root.join(canonical);
            if bounded_read(&original)? != bytes {
                return Err(format!(
                    "First-party runtime drift: {canonical} != src/windows_blender52_runtime/{name}. Run tools/sync_windows_blender_runtime.py explicitly after reviewing the canonical change."
                ));
            }
        }
    }
    Ok(())
}
fn main() {
    let root = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("Cargo manifest directory"),
    );
    for (name, canonical, _) in SOURCES {
        println!(
            "cargo:rerun-if-changed={}",
            root.join("src/windows_blender52_runtime")
                .join(name)
                .display()
        );
        println!("cargo:rerun-if-changed={}", root.join(canonical).display());
    }
    for marker in [
        "../../Cargo.toml",
        "../driver-sdk/Cargo.toml",
        "src/windows_blender52_runtime/manifest.json",
    ] {
        println!("cargo:rerun-if-changed={}", root.join(marker).display());
    }
    if let Err(error) = verify_bundle(&root) {
        panic!("{error}");
    }
}
