//! First-party Ardour backend for Semwright's backend-neutral audio domain.
//!
//! The live surface uses Ardour's official OSC control surface on loopback.
//! A separate deep projection models richer session state obtained from
//! Semwright-owned Ardour/Lua inspection. Agent data is never interpreted as
//! Lua source, shell text, a host address, or a filesystem path.

pub mod driver;
pub mod live_projection;
pub mod native;
pub mod osc;
pub mod projection;
pub mod runtime;
pub mod script;

pub const DRIVER_ID: &str = "ardour-audio";
pub const DRIVER_SCOPE: &str = "driver:ardour-audio";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
