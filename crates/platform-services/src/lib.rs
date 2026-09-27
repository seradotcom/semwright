//! The only compile-time system-service composition point. No desktop frameworks here.
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix;
use semwright_platform_api::{
    PlatformPaths,
    launch::{ExecutableVerifier, SandboxLauncher},
};
use semwright_types::Result;
use std::path::{Path, PathBuf};

#[cfg(target_os = "linux")]
pub use semwright_platform_linux_sys::filesystem::Root;
#[cfg(target_os = "macos")]
pub use semwright_platform_macos_sys::filesystem::Root;
#[cfg(target_os = "windows")]
pub use semwright_platform_windows_sys::filesystem::Root;

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub use unix::{current_uid, private_directory, validate_peer};

#[cfg(target_os = "windows")]
pub fn private_directory(path: &Path) -> Result<()> {
    semwright_platform_windows_sys::paths::ensure_private_directory(path)
}
#[cfg(target_os = "windows")]
pub fn windows_pipe_path(kind: &str) -> Result<PathBuf> {
    semwright_platform_windows_sys::pipe::pipe_path(kind)
}
#[cfg(target_os = "windows")]
pub fn windows_pipe_server(
    path: &Path,
    first: bool,
) -> Result<tokio::net::windows::named_pipe::NamedPipeServer> {
    semwright_platform_windows_sys::pipe::create_tokio_server(path, first)
}
#[cfg(target_os = "windows")]
pub fn windows_pipe_client(
    path: &Path,
) -> Result<tokio::net::windows::named_pipe::NamedPipeClient> {
    semwright_platform_windows_sys::pipe::open_tokio_client(path)
}
#[cfg(target_os = "windows")]
pub fn validate_windows_server_peer(
    pipe: &tokio::net::windows::named_pipe::NamedPipeServer,
) -> Result<u32> {
    semwright_platform_windows_sys::pipe::validate_tokio_server_peer(pipe)
}
#[cfg(target_os = "windows")]
pub fn validate_windows_client_peer(
    pipe: &tokio::net::windows::named_pipe::NamedPipeClient,
) -> Result<u32> {
    semwright_platform_windows_sys::pipe::validate_tokio_client_peer(pipe)
}

#[cfg(target_os = "linux")]
pub fn verifier() -> impl ExecutableVerifier {
    semwright_platform_linux_sys::launch::LinuxVerifier
}
#[cfg(target_os = "macos")]
pub fn verifier() -> impl ExecutableVerifier {
    semwright_platform_macos_sys::launch::MacVerifier
}
#[cfg(target_os = "windows")]
pub fn verifier() -> impl ExecutableVerifier {
    semwright_platform_windows_sys::launch::WindowsVerifier
}

#[cfg(target_os = "linux")]
pub fn launcher() -> impl SandboxLauncher {
    semwright_platform_linux_sys::launch::LinuxSandbox
}
#[cfg(target_os = "macos")]
pub fn launcher() -> impl SandboxLauncher {
    semwright_platform_macos_sys::launch::MacSandbox
}
#[cfg(target_os = "windows")]
pub fn launcher() -> impl SandboxLauncher {
    semwright_platform_windows_sys::launch::WindowsSandbox
}

pub fn verify_executable(p: &Path, d: &str) -> Result<Vec<u8>> {
    verifier().verify(p, d)
}
pub fn sandbox_command(
    s: &semwright_platform_api::launch::SandboxSpec,
) -> Result<tokio::process::Command> {
    launcher().command(s)
}
pub fn sandbox_spawn(
    s: &semwright_platform_api::launch::SandboxSpec,
) -> Result<semwright_platform_api::launch::SandboxProcess> {
    launcher().spawn(s)
}
pub fn sandbox_available(helper: &Path) -> bool {
    launcher().available(helper)
}
pub fn sandbox_mechanism() -> &'static str {
    launcher().mechanism()
}
pub fn sandbox_diagnostics(helper: &Path) -> serde_json::Value {
    launcher().diagnostics(helper)
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
#[cfg(target_os = "windows")]
pub fn sandbox_main() {
    eprintln!(
        "SandboxDenied: Windows arbitrary-child sandbox requires secure pre-exec spawn contract"
    );
    std::process::exit(5);
}

#[cfg(target_os = "linux")]
pub fn runtime_directory() -> Result<PathBuf> {
    use semwright_types::Error;
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| Error::unavailable("XDG_RUNTIME_DIR is required"))?;
    private_directory(&base)?;
    let p = base.join("semwright");
    private_directory(&p)?;
    Ok(p)
}
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub fn runtime_directory() -> Result<PathBuf> {
    Ok(paths()?.runtime)
}

#[cfg(target_os = "linux")]
pub fn paths() -> Result<PlatformPaths> {
    use semwright_types::Error;
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
#[cfg(target_os = "windows")]
pub fn paths() -> Result<PlatformPaths> {
    semwright_platform_windows_sys::paths::paths()
}

#[cfg(target_os = "linux")]
pub fn filesystem() -> impl semwright_platform_api::filesystem::ScopedFilesystem {
    semwright_platform_linux_sys::filesystem::LinuxFilesystem
}
#[cfg(target_os = "macos")]
pub fn filesystem() -> impl semwright_platform_api::filesystem::ScopedFilesystem {
    semwright_platform_macos_sys::filesystem::MacFilesystem
}
#[cfg(target_os = "windows")]
pub fn filesystem() -> impl semwright_platform_api::filesystem::ScopedFilesystem {
    semwright_platform_windows_sys::filesystem::WindowsFilesystem
}

/// Semantic user identity for non-Unix callers. Do not expose raw token handles.
#[cfg(target_os = "windows")]
pub fn current_principal() -> Result<String> {
    semwright_platform_windows_sys::identity::current_principal()
}
