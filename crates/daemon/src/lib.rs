//! Owner configuration, local IPC service and trusted operator console.
pub mod config;
#[cfg(unix)]
pub mod console;
#[cfg(target_os = "windows")]
#[path = "console_windows.rs"]
pub mod console;
pub mod server;
