//! Explicitly feature-gated test fixture; not a production runner command surface.
#[cfg(windows)]
#[allow(dead_code)]
#[path = "session_runner/windows.rs"]
mod windows;
#[cfg(windows)]
fn main() {
    if let Err(error) = windows::fixture::run() {
        eprintln!("fixture failed: {error:?}");
        std::process::exit(2);
    }
}
#[cfg(not(windows))]
fn main() {
    std::process::exit(2);
}
