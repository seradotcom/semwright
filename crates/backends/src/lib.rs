//! Linux integration backends. Platform absence is a capability state, not success.
pub mod atspi;
pub mod bridge;
pub mod clipboard;
pub mod fake;
pub mod filesystem;
pub mod hyprland;
pub mod portal;
pub mod sway;
pub mod system;
pub mod x11;

pub fn environment() -> serde_json::Value {
    serde_json::json!({
        "os":"linux",
        "session_type":std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_|if std::env::var_os("WAYLAND_DISPLAY").is_some(){"wayland".into()}else if std::env::var_os("DISPLAY").is_some(){"x11".into()}else{"headless".into()}),
        "desktop":std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(),
        "wayland_display_present":std::env::var_os("WAYLAND_DISPLAY").is_some(),
        "x11_display_present":std::env::var_os("DISPLAY").is_some(),
        "session_bus_configured":std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some(),
        "note":"Environment variables are hints. Live backend probes are authoritative."
    })
}
