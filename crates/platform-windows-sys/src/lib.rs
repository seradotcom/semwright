//! Low-level Windows implementation details. Public Semwright contracts never expose HWND/HANDLE/SID.
#![cfg_attr(not(target_os = "windows"), allow(dead_code, unused_imports))]

pub mod pe;

#[cfg(target_os = "windows")]
pub mod capture;
#[cfg(target_os = "windows")]
pub mod clipboard;
#[cfg(target_os = "windows")]
pub mod dll;
#[cfg(target_os = "windows")]
pub mod filesystem;
#[cfg(target_os = "windows")]
pub mod identity;
#[cfg(target_os = "windows")]
pub mod input;
#[cfg(target_os = "windows")]
pub mod job;
#[cfg(target_os = "windows")]
pub mod launch;
#[cfg(target_os = "windows")]
pub mod paths;
#[cfg(target_os = "windows")]
pub mod pipe;
#[cfg(target_os = "windows")]
pub mod window;

#[cfg(not(target_os = "windows"))]
pub const NATIVE_WINDOWS_IMPLEMENTATION_AVAILABLE: bool = false;
#[cfg(target_os = "windows")]
pub const NATIVE_WINDOWS_IMPLEMENTATION_AVAILABLE: bool = true;
