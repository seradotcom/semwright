//! Desktop composition ONLY. The broker, policy and protocols are shared.
use semwright_adapters::chromium::BrowserConfig;
use semwright_platform_api::DesktopHost;
use semwright_platform_common::{Application, fake::FakeDesktop};
use semwright_types::Result;
use std::{
    collections::BTreeMap,
    future::Future,
    path::{Path, PathBuf},
    sync::Arc,
};

pub async fn bootstrap(
    fake: bool,
    runtime: &Path,
    state: &Path,
    applications: BTreeMap<String, Application>,
    browser: BrowserConfig,
    blender_socket: Option<PathBuf>,
) -> Result<DesktopHost> {
    if fake {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        let root_fixture_only = semwright_platform_services::current_uid() == 0;
        #[cfg(target_os = "windows")]
        let root_fixture_only = false;
        return Ok(DesktopHost {
            backends: vec![Arc::new(FakeDesktop::new())],
            environment: serde_json::json!({"os":std::env::consts::OS,"fake":true,"broker_session_bus_owned":false,"root_fixture_only":root_fixture_only}),
            keepalive: Box::new(()),
        });
    }
    let mut host = native(runtime, state, applications, browser, blender_socket).await?;
    host.environment["root_fixture_only"] = serde_json::json!(false);
    Ok(host)
}

#[cfg(target_os = "linux")]
async fn native(
    r: &Path,
    state: &Path,
    a: BTreeMap<String, Application>,
    b: BrowserConfig,
    s: Option<PathBuf>,
) -> Result<DesktopHost> {
    semwright_platform_linux::bootstrap(r, state, a, b, s).await
}

#[cfg(target_os = "macos")]
async fn native(
    r: &Path,
    _state: &Path,
    _a: BTreeMap<String, Application>,
    _b: BrowserConfig,
    _s: Option<PathBuf>,
) -> Result<DesktopHost> {
    Ok(DesktopHost {
        backends: vec![Arc::new(
            semwright_platform_macos::Macos::new(&r.join("artifacts")).await?,
        )],
        environment: serde_json::json!({"os":"macos","native_api":"public","minimum_os":"14.0","application_launch":"unavailable","linux_application_adapter_launch":"unavailable","third_party_driver_sandbox":"unavailable_fail_closed"}),
        keepalive: Box::new(()),
    })
}

#[cfg(target_os = "windows")]
async fn native(
    _r: &Path,
    _state: &Path,
    _a: BTreeMap<String, Application>,
    _b: BrowserConfig,
    _s: Option<PathBuf>,
) -> Result<DesktopHost> {
    let principal = semwright_platform_services::current_principal()?;
    Ok(DesktopHost {
        backends: vec![Arc::new(semwright_platform_windows::Windows::new()?)],
        environment: serde_json::json!({
            "os":"windows","native_api":"public","minimum_os":"Windows 11",
            "identity":principal,"interactive_session":"required_for_desktop_actions",
            "uia":"actor_thread","send_input":"uipi_respected","capture":"wgc_targeting_present_readback_pending",
            "third_party_driver_sandbox":"unavailable_fail_closed"
        }),
        keepalive: Box::new(()),
    })
}

#[cfg(target_os = "linux")]
pub fn run<F: Future<Output = Result<()>> + Send + 'static>(
    runtime: tokio::runtime::Runtime,
    f: F,
) -> Result<()> {
    runtime.block_on(f)
}
#[cfg(target_os = "macos")]
pub fn run<F: Future<Output = Result<()>> + Send + 'static>(
    runtime: tokio::runtime::Runtime,
    f: F,
) -> Result<()> {
    semwright_platform_macos::transport::run(runtime, f)
}
#[cfg(target_os = "windows")]
pub fn run<F: Future<Output = Result<()>> + Send + 'static>(
    runtime: tokio::runtime::Runtime,
    f: F,
) -> Result<()> {
    runtime.block_on(f)
}
