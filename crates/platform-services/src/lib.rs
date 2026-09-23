//! The only compile-time system-service composition point. No desktop frameworks here.
mod unix;
use semwright_platform_api::{
    PlatformPaths,
    launch::{ExecutableVerifier, SandboxLauncher},
};
#[cfg(target_os = "linux")]
pub use semwright_platform_linux_sys::filesystem::Root;
#[cfg(target_os = "macos")]
pub use semwright_platform_macos_sys::filesystem::Root;
#[cfg(target_os = "linux")]
use semwright_types::Error;
use semwright_types::Result;
use std::path::{Path, PathBuf};
pub use unix::{current_uid, private_directory, validate_peer};
#[cfg(target_os = "linux")]
pub fn verifier() -> impl ExecutableVerifier {
    semwright_platform_linux_sys::launch::LinuxVerifier
}
#[cfg(target_os = "macos")]
pub fn verifier() -> impl ExecutableVerifier {
    semwright_platform_macos_sys::launch::MacVerifier
}
#[cfg(target_os = "linux")]
pub fn launcher() -> impl SandboxLauncher {
    semwright_platform_linux_sys::launch::LinuxSandbox
}
#[cfg(target_os = "macos")]
pub fn launcher() -> impl SandboxLauncher {
    semwright_platform_macos_sys::launch::MacSandbox
}
pub fn verify_executable(p: &Path, d: &str) -> Result<Vec<u8>> {
    verifier().verify(p, d)
}
pub fn sandbox_command(
    s: &semwright_platform_api::launch::SandboxSpec,
) -> Result<tokio::process::Command> {
    launcher().command(s)
}
pub fn sandbox_available(helper: &Path) -> bool {
    launcher().available(helper)
}
pub fn sandbox_mechanism() -> &'static str {
    launcher().mechanism()
}
#[cfg(target_os = "linux")]
pub fn sandbox_main() {
    semwright_platform_linux_sys::sandbox_main::main()
}
#[cfg(target_os = "macos")]
pub fn sandbox_main() {
    eprintln!("SandboxDenied: no macOS arbitrary-child sandbox");
    std::process::exit(5);
}
#[cfg(target_os = "linux")]
pub fn runtime_directory() -> Result<PathBuf> {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| Error::unavailable("XDG_RUNTIME_DIR is required"))?;
    private_directory(&base)?;
    let p = base.join("semwright");
    private_directory(&p)?;
    Ok(p)
}
#[cfg(target_os = "macos")]
pub fn runtime_directory() -> Result<PathBuf> {
    Ok(paths()?.runtime)
}
#[cfg(target_os = "linux")]
pub fn paths() -> Result<PlatformPaths> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| Error::unavailable("HOME is required"))?;
    let base = |key: &str, default: &str| -> Result<PathBuf> {
        let p = std::env::var_os(key)
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(default));
        if !p.is_absolute() {
            return Err(Error::invalid("XDG path must be absolute"));
        }
        Ok(p.join("semwright"))
    };
    Ok(PlatformPaths {
        runtime: runtime_directory()?,
        state: base("XDG_STATE_HOME", ".local/state")?,
        config: base("XDG_CONFIG_HOME", ".config")?,
        cache: base("XDG_CACHE_HOME", ".cache")?,
    })
}
#[cfg(target_os = "macos")]
pub fn paths() -> Result<PlatformPaths> {
    semwright_platform_macos_sys::paths::paths()
}

#[cfg(target_os = "linux")]
pub fn filesystem() -> impl semwright_platform_api::filesystem::ScopedFilesystem {
    semwright_platform_linux_sys::filesystem::LinuxFilesystem
}
#[cfg(target_os = "macos")]
pub fn filesystem() -> impl semwright_platform_api::filesystem::ScopedFilesystem {
    semwright_platform_macos_sys::filesystem::MacFilesystem
}
pub fn sandbox_diagnostics(helper: &Path) -> serde_json::Value {
    launcher().diagnostics(helper)
}
