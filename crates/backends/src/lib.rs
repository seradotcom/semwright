//! Compatibility exports. New host construction uses semwright-platform-host.
pub use semwright_platform_common::{fake, filesystem};
#[cfg(target_os = "linux")]
pub use semwright_platform_linux::{
    atspi, bridge, clipboard, eis, environment, hyprland, portal, sway, system, x11,
};
#[cfg(target_os = "macos")]
pub fn environment() -> serde_json::Value {
    serde_json::json!({"os":"macos","note":"Use platform-host for live permission probes"})
}
