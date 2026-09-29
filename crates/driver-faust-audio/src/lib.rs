//! Curated Faust backend.
//!
//! Agent input is semantic audio data. This crate never accepts arbitrary Faust,
//! C++, compiler flags, executable paths, shell strings or environment values.

pub mod analysis_driver;
pub mod analysis_runtime;
pub mod driver;
pub mod faust;
pub mod runtime;
