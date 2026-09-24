//! Owner configuration, local IPC service and trusted operator console.
pub mod config;
#[cfg(unix)]
pub mod console;
pub mod server;
