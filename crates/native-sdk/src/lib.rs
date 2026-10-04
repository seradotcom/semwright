//! Cooperation interfaces for applications that own their model and transactions.
//!
//! No filesystem, runtime, Broker or Graph authority is required by default.
//! The JSON document implementation is an explicit `file-backed` profile.

pub use async_trait::async_trait;
pub use semwright_types::{
    self as types, CommandDescriptor, Error, ErrorCode, Idempotency, NativeTarget, Result, Risk,
};
pub use serde_json::{self, Value, json};

#[cfg(feature = "driver")]
pub use semwright_driver_sdk::{
    self as driver_sdk, Capability, Driver, DriverExecutionContext, DriverInterfaces,
    descriptor_digest, serve,
};
#[cfg(feature = "driver")]
pub use tokio;

#[cfg(feature = "file-backed")]
mod file_profile;
#[cfg(feature = "file-backed")]
pub use file_profile::{
    Model, NATIVE_SCHEMA, NativeApp, Operation, SDK_VERSION, request_digest, run, sha256,
};

#[cfg(feature = "effects")]
pub mod composition_report;
#[cfg(all(feature = "effects", feature = "file-backed"))]
pub mod cooperation_profile;
#[cfg(feature = "effects")]
pub mod effects_readback;
#[cfg(feature = "graph")]
pub mod graph_adapter;
#[cfg(feature = "effects")]
pub mod report_validation;

pub mod cooperation;
#[cfg(feature = "driver")]
pub mod driver;
#[cfg(feature = "driver")]
pub use driver::NativeDriver;

#[cfg(feature = "process-bridge")]
pub mod process_bridge;
