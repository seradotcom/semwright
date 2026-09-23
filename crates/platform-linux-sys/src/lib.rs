//! Linux enforcement mechanisms. Native primitives never leak into Driver Protocol.
#![cfg(target_os = "linux")]
pub mod filesystem;
pub mod launch;
// The unmodified baseline sandbox helper is transplanted by integration/apply.py.
pub mod sandbox_main;
