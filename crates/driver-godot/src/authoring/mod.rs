//! Typed managed Godot authoring. Generated source is a backend, never caller code.
pub mod compiler;
pub mod model;
pub mod validate;
pub use compiler::{CompiledProject, compile};
pub use model::*;
pub use validate::validate;
